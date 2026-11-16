package com.canvasstudio.rust

import android.app.Activity
import android.app.AlertDialog
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.PackageInstaller
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.widget.Toast
import org.json.JSONArray
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/** Over-the-air updates from the project's GitHub releases (tags named rust-v1.2.3). */
class Updater(private val activity: Activity) {
    private val main = Handler(Looper.getMainLooper())
    @Volatile
    private var busy = false

    private fun toast(text: String) = Toast.makeText(activity, text, Toast.LENGTH_LONG).show()

    private fun current(): String =
        activity.packageManager.getPackageInfo(activity.packageName, 0).versionName ?: "0"

    /** Looks for a newer release. When `manual`, also says so if there is none. */
    fun check(manual: Boolean) {
        if (busy) return
        busy = true
        Thread {
            var found: Pair<String, String>? = null
            var failed = false
            try {
                val releases = JSONArray(String(open(RELEASES).inputStream.use { it.readBytes() }))
                for (i in 0 until releases.length()) {
                    val r = releases.getJSONObject(i)
                    val tag = r.optString("tag_name")
                    if (!tag.startsWith(PREFIX) || r.optBoolean("draft") || r.optBoolean("prerelease")) continue
                    val assets = r.getJSONArray("assets")
                    for (a in 0 until assets.length()) {
                        val asset = assets.getJSONObject(a)
                        if (asset.optString("name").endsWith(".apk")) {
                            found = Pair(tag.removePrefix(PREFIX), asset.getString("browser_download_url"))
                            break
                        }
                    }
                    if (found != null) break
                }
            } catch (e: Exception) {
                failed = true
                Log.w(Handwriting.TAG, "update check failed", e)
            }
            main.post {
                busy = false
                if (activity.isFinishing || activity.isDestroyed) return@post
                val release = found
                if (release != null && newer(release.first, current())) {
                    AlertDialog.Builder(activity)
                        .setTitle("Update available")
                        .setMessage("Version ${release.first} is ready. You have ${current()}.")
                        .setPositiveButton("Update") { _, _ -> download(release.second) }
                        .setNegativeButton("Later", null)
                        .show()
                } else if (manual) {
                    toast(if (failed) "Could not check for updates" else "You have the latest version (${current()})")
                }
            }
        }.start()
    }

    private fun open(url: String): HttpURLConnection {
        val c = URL(url).openConnection() as HttpURLConnection
        c.connectTimeout = 15000
        c.readTimeout = 30000
        c.instanceFollowRedirects = true
        c.setRequestProperty("User-Agent", "canvas-studio-updater")
        c.setRequestProperty("Accept", "application/vnd.github+json, application/octet-stream")
        if (c.responseCode !in 200..299) throw java.io.IOException("HTTP ${c.responseCode}")
        return c
    }

    private fun download(url: String) {
        if (busy) return
        busy = true
        toast("Downloading the update…")
        Thread {
            try {
                val file = File(activity.cacheDir, "update.apk")
                open(url).inputStream.use { input -> file.outputStream().use { input.copyTo(it) } }
                install(file)
                file.delete()
            } catch (e: Exception) {
                Log.w(Handwriting.TAG, "update failed", e)
                main.post { toast("The update could not be downloaded") }
            }
            busy = false
        }.start()
    }

    private fun install(file: File) {
        val installer = activity.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
        val id = installer.createSession(params)
        installer.openSession(id).use { session ->
            session.openWrite("apk", 0, file.length()).use { out ->
                file.inputStream().use { it.copyTo(out) }
                session.fsync(out)
            }
            val intent = Intent(activity, MainActivity::class.java).setAction(ACTION)
            val flags = PendingIntent.FLAG_UPDATE_CURRENT or (if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0)
            session.commit(PendingIntent.getActivity(activity, id, intent, flags).intentSender)
        }
    }

    /** The installer reports back through the activity's onNewIntent. */
    fun onResult(intent: Intent) {
        when (intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                @Suppress("DEPRECATION")
                val confirm = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT) ?: return
                activity.startActivity(confirm)
            }
            PackageInstaller.STATUS_SUCCESS -> {}
            PackageInstaller.STATUS_FAILURE_ABORTED -> {}
            else -> toast("The update could not be installed: " + (intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: "unknown error"))
        }
    }

    companion object {
        const val ACTION = "com.canvasstudio.rust.UPDATE_RESULT"
        const val PREFIX = "rust-v"
        const val RELEASES = "https://api.github.com/repos/tejasgowda0012/canvas/releases?per_page=30"

        /** True when dotted version `a` is higher than `b`. */
        fun newer(a: String, b: String): Boolean {
            val x = a.split('.', '-', '+').map { it.toIntOrNull() ?: 0 }
            val y = b.split('.', '-', '+').map { it.toIntOrNull() ?: 0 }
            for (i in 0 until maxOf(x.size, y.size)) {
                val d = x.getOrElse(i) { 0 } - y.getOrElse(i) { 0 }
                if (d != 0) return d > 0
            }
            return false
        }
    }
}
