package com.canvasstudio.rust

import android.app.Activity
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.UUID

/** Android/network API glue only. Rust draws the update offer and all status. */
class Updater(private val activity: Activity, private val message: (String) -> Unit) {
    private val main = Handler(Looper.getMainLooper())
    private val prefs by lazy { activity.getSharedPreferences("installer", Activity.MODE_PRIVATE) }
    @Volatile private var busy = false
    private var cachedUrl = ""
    private var cachedFile: File? = null

    private fun deliver(block: () -> Unit) { main.post { if (!activity.isFinishing && !activity.isDestroyed) block() } }
    private fun report(text: String) { deliver { message(text) } }

    fun check(manual: Boolean, done: (version: String, url: String, failed: Boolean) -> Unit) {
        if (busy) { deliver { done("", "", true) }; return }
        busy = true
        Thread({
            var version = ""; var url = ""; var failed = false
            // The office build machine serves updates on the local network first.
            try {
                val feed = JSONObject(String(fetch(LAN_FEED, 64 * 1024), Charsets.UTF_8))
                val apk = feed.getString("apk")
                require(!apk.contains('/') && apk.endsWith(".apk")) { "Bad update file name" }
                version = feed.getString("version")
                url = LAN_BASE + apk
            } catch (error: Exception) { Log.i(Handwriting.TAG, "Local update feed unavailable: ${error.message}") }
            if (url.isEmpty()) try {
                val connection = open(RELEASES)
                val bytes = try { connection.inputStream.use { input ->
                    val output = ByteArrayOutputStream()
                    val buffer = ByteArray(16 * 1024)
                    while (true) {
                        val n = input.read(buffer); if (n < 0) break
                        require(output.size() + n <= 2 * 1024 * 1024) { "Release response is too large" }
                        output.write(buffer, 0, n)
                    }
                    output.toByteArray()
                } } finally { connection.disconnect() }
                val releases = JSONArray(String(bytes, Charsets.UTF_8))
                for (i in 0 until releases.length()) {
                    val release = releases.getJSONObject(i)
                    val tag = release.optString("tag_name")
                    if (!tag.startsWith(PREFIX) || release.optBoolean("draft") || release.optBoolean("prerelease")) continue
                    val assets = release.getJSONArray("assets")
                    for (a in 0 until assets.length()) {
                        val asset = assets.getJSONObject(a)
                        if (asset.optString("name") == "canvas-studio-rust.apk") {
                            version = tag.removePrefix(PREFIX)
                            url = asset.getString("browser_download_url")
                            break
                        }
                    }
                    if (url.isNotEmpty()) break
                }
            } catch (error: Exception) { failed = true; Log.w(Handwriting.TAG, "Update check failed", error) }
            busy = false
            deliver { done(version, url, failed) }
        }, "update-check").start()
    }

    private fun fetch(url: String, limit: Int): ByteArray {
        val connection = open(url, 3_000)
        return try { connection.inputStream.use { input ->
            val output = ByteArrayOutputStream()
            val buffer = ByteArray(16 * 1024)
            while (true) {
                val n = input.read(buffer); if (n < 0) break
                require(output.size() + n <= limit) { "Response is too large" }
                output.write(buffer, 0, n)
            }
            output.toByteArray()
        } } finally { connection.disconnect() }
    }

    private fun open(url: String, connectTimeout: Int = 15_000): HttpURLConnection {
        // Cleartext only for the local build machine; the APK's signature is checked before install.
        require(URL(url).protocol == "https" || url.startsWith(LAN_BASE)) { "Updates require HTTPS" }
        val connection = URL(url).openConnection() as HttpURLConnection
        connection.connectTimeout = connectTimeout
        connection.readTimeout = 30_000
        connection.instanceFollowRedirects = true
        connection.setRequestProperty("User-Agent", "canvas-studio-updater")
        if (connection.responseCode !in 200..299) { connection.disconnect(); throw java.io.IOException("Update server rejected the request") }
        return connection
    }

    fun download(url: String) {
        if (busy) { report("An update operation is already running"); return }
        if (!url.startsWith("https://github.com/tejasgowda0012/canvas/releases/download/") && !url.startsWith(LAN_BASE)) { report("This update address is invalid"); return }
        busy = true
        report("Downloading the update…")
        Thread({
            val partial = File(activity.cacheDir, "update.part")
            try {
                val existing = cachedFile
                val file = if (url == cachedUrl && existing?.isFile == true) existing else {
                    val connection = open(url)
                    try { connection.inputStream.use { input -> partial.outputStream().use { output ->
                        val buffer = ByteArray(64 * 1024); var total = 0L
                        while (true) {
                            val n = input.read(buffer); if (n < 0) break
                            total += n
                            require(total <= 160L * 1024 * 1024) { "Update file is too large" }
                            output.write(buffer, 0, n)
                        }
                        require(total > 0) { "The update download is empty" }
                        output.fd.sync()
                    } } } finally { connection.disconnect() }
                    val target = File(activity.cacheDir, "update.apk")
                    check(partial.renameTo(target)) { "Cannot finish the update download" }
                    target
                }
                validate(file)
                cachedUrl = url; cachedFile = file
                if (Build.VERSION.SDK_INT >= 26 && !activity.packageManager.canRequestPackageInstalls()) {
                    deliver {
                        message("Allow Canvas Studio to install updates, then tap Update again")
                        activity.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${activity.packageName}")))
                    }
                } else {
                    report("Download verified. Confirm the update in Android's installer")
                    install(file)
                    file.delete(); cachedFile = null
                }
            } catch (error: Exception) {
                partial.delete()
                Log.w(Handwriting.TAG, "Update installation failed", error)
                report("The update could not be downloaded or verified. Please retry")
            } finally { busy = false }
        }, "update-install").start()
    }

    @Suppress("DEPRECATION")
    private fun version(info: PackageInfo): Long = if (Build.VERSION.SDK_INT >= 28) info.longVersionCode else info.versionCode.toLong()
    @Suppress("DEPRECATION")
    private fun signers(info: PackageInfo): Set<String> {
        val signatures = if (Build.VERSION.SDK_INT >= 28) info.signingInfo?.apkContentsSigners else info.signatures
        return signatures?.map { signature -> MessageDigest.getInstance("SHA-256").digest(signature.toByteArray()).joinToString("") { "%02x".format(it.toInt() and 255) } }?.toSet() ?: emptySet()
    }
    @Suppress("DEPRECATION")
    private fun validate(file: File) {
        val flags = if (Build.VERSION.SDK_INT >= 28) PackageManager.GET_SIGNING_CERTIFICATES else PackageManager.GET_SIGNATURES
        val candidate = activity.packageManager.getPackageArchiveInfo(file.path, flags) ?: error("Invalid APK")
        val installed = activity.packageManager.getPackageInfo(activity.packageName, flags)
        require(candidate.packageName == activity.packageName && version(candidate) > version(installed)) { "This APK is not an update to this app" }
        val trusted = signers(installed)
        require(trusted.isNotEmpty() && trusted == signers(candidate)) { "APK signing certificate does not match" }
    }

    private fun install(file: File) {
        val installer = activity.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
        params.setAppPackageName(activity.packageName)
        if (Build.VERSION.SDK_INT >= 31) params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_REQUIRED)
        val id = installer.createSession(params)
        val nonce = UUID.randomUUID().toString()
        prefs.edit().putString("sessionNonce", nonce).apply()
        try { installer.openSession(id).use { session ->
            session.openWrite("apk", 0, file.length()).use { output -> file.inputStream().use { it.copyTo(output) }; session.fsync(output) }
            val intent = Intent(activity, MainActivity::class.java).setAction(ACTION).putExtra(NONCE, nonce)
            val flags = PendingIntent.FLAG_UPDATE_CURRENT or if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0
            session.commit(PendingIntent.getActivity(activity, id, intent, flags).intentSender)
        } } catch (error: Exception) { installer.abandonSession(id); throw error }
    }

    /** PackageInstaller adds its status to the nonce-bearing private PendingIntent. */
    fun onResult(intent: Intent) {
        val expected = prefs.getString("sessionNonce", null) ?: return
        if (intent.getStringExtra(NONCE) != expected) return
        when (intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                @Suppress("DEPRECATION")
                val confirm = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT)
                if (confirm != null) activity.startActivity(confirm)
            }
            PackageInstaller.STATUS_SUCCESS -> { prefs.edit().remove("sessionNonce").apply(); message("Update installed") }
            PackageInstaller.STATUS_FAILURE_ABORTED -> { prefs.edit().remove("sessionNonce").apply(); message("Update cancelled") }
            else -> { prefs.edit().remove("sessionNonce").apply(); message("Android could not install the update") }
        }
    }

    companion object {
        const val ACTION = "com.canvasstudio.rust.UPDATE_RESULT"
        private const val NONCE = "com.canvasstudio.rust.UPDATE_NONCE"
        const val PREFIX = "rust-v"
        const val LAN_BASE = "http://192.168.88.67:8790/"
        const val LAN_FEED = LAN_BASE + "latest.json"
        const val RELEASES = "https://api.github.com/repos/tejasgowda0012/canvas/releases?per_page=30"
    }
}
