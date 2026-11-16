package com.canvasstudio.canvas_studio

import android.content.ActivityNotFoundException
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.view.MotionEvent
import io.flutter.embedding.engine.FlutterShellArgs
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File
import java.security.MessageDigest
import java.util.concurrent.Executors

class MainActivity : FlutterActivity() {
    private val updateExecutor = Executors.newSingleThreadExecutor()

    private fun graphicsMode(): String =
        getSharedPreferences("display", Context.MODE_PRIVATE).getString("graphics", "opengl") ?: "opengl"

    // Large touch panels often ship slow or software Vulkan drivers, which makes
    // every frame take seconds. OpenGL is the default; the user can switch.
    override fun getFlutterShellArgs(): FlutterShellArgs {
        val args = super.getFlutterShellArgs()
        when (graphicsMode()) {
            "vulkan" -> args.add("--impeller-backend=vulkan")
            "classic" -> args.add("--enable-impeller=false")
            else -> args.add("--impeller-backend=opengles")
        }
        return args
    }

    // Deliver touch samples as they arrive instead of batching them per frame.
    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN) {
            window.decorView.requestUnbufferedDispatch(event)
        }
        return super.dispatchTouchEvent(event)
    }

    @Suppress("DEPRECATION")
    private fun displayInfo(): Map<String, Any> {
        val metrics = resources.displayMetrics
        val refresh = try { windowManager.defaultDisplay.refreshRate } catch (_: Exception) { 0f }
        return mapOf(
            "graphics" to graphicsMode(),
            "width" to metrics.widthPixels,
            "height" to metrics.heightPixels,
            "density" to metrics.density.toDouble(),
            "refresh" to refresh.toDouble(),
            "device" to "${Build.MANUFACTURER} ${Build.MODEL}",
            "android" to Build.VERSION.RELEASE,
            "hardware" to (if (Build.VERSION.SDK_INT >= 31) "${Build.SOC_MANUFACTURER} ${Build.SOC_MODEL}" else Build.HARDWARE),
        )
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "canvas_studio/display")
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "info" -> result.success(displayInfo())
                    "setGraphics" -> {
                        val mode = call.argument<String>("mode")
                        if (mode != "opengl" && mode != "vulkan" && mode != "classic") {
                            result.error("INVALID_MODE", "Unknown graphics engine.", null)
                        } else {
                            getSharedPreferences("display", Context.MODE_PRIVATE)
                                .edit().putString("graphics", mode).commit()
                            result.success(null)
                            // The engine is chosen when the app starts, so restart it.
                            Handler(Looper.getMainLooper()).postDelayed({
                                val launch = packageManager.getLaunchIntentForPackage(packageName)
                                if (launch != null) {
                                    startActivity(Intent.makeRestartActivityTask(launch.component))
                                }
                                Runtime.getRuntime().exit(0)
                            }, 250)
                        }
                    }
                    else -> result.notImplemented()
                }
            }
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "canvas_studio/updates")
            .setMethodCallHandler { call, result ->
                try {
                    when (call.method) {
                        "appInfo" -> {
                            val info = installedPackage()
                            result.success(mapOf(
                                "versionName" to (info.versionName ?: "Unknown"),
                                "versionCode" to versionCode(info),
                            ))
                        }
                        "canInstall" -> result.success(canInstallUpdates())
                        "requestInstallPermission" -> {
                            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !canInstallUpdates()) {
                                startActivity(Intent(
                                    Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                                    Uri.parse("package:$packageName"),
                                ))
                            }
                            result.success(null)
                        }
                        "installUpdate" -> {
                            val path = call.argument<String>("path")
                            if (path.isNullOrBlank()) {
                                throw UpdateError("INVALID_PATH", "The downloaded update file is missing.")
                            }
                            if (!canInstallUpdates()) {
                                throw UpdateError(
                                    "INSTALL_PERMISSION_REQUIRED",
                                    "Allow Canvas Studio to install updates in Android settings, then tap Install again.",
                                )
                            }
                            updateExecutor.execute {
                                try {
                                    val apk = validateUpdate(path)
                                    runOnUiThread {
                                        try {
                                            val uri = FileProvider.getUriForFile(this, "$packageName.updates", apk)
                                            startActivity(Intent(Intent.ACTION_VIEW).apply {
                                                setDataAndType(uri, "application/vnd.android.package-archive")
                                                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                                            })
                                            result.success(null)
                                        } catch (error: Exception) {
                                            reportError(result, error)
                                        }
                                    }
                                } catch (error: Exception) {
                                    runOnUiThread { reportError(result, error) }
                                }
                            }
                        }
                        else -> result.notImplemented()
                    }
                } catch (error: Exception) {
                    reportError(result, error)
                }
            }
    }

    private fun canInstallUpdates(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.O || packageManager.canRequestPackageInstalls()

    @Suppress("DEPRECATION")
    private fun signingFlags(): Int = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        PackageManager.GET_SIGNING_CERTIFICATES
    } else {
        PackageManager.GET_SIGNATURES
    }

    @Suppress("DEPRECATION")
    private fun installedPackage(): PackageInfo = packageManager.getPackageInfo(packageName, signingFlags())

    @Suppress("DEPRECATION")
    private fun versionCode(info: PackageInfo): Long = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        info.longVersionCode
    } else {
        info.versionCode.toLong()
    }

    @Suppress("DEPRECATION")
    private fun signerDigests(info: PackageInfo): Set<String> {
        val signers = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            info.signingInfo?.apkContentsSigners
        } else {
            info.signatures
        }
        return signers?.map { signature ->
            MessageDigest.getInstance("SHA-256").digest(signature.toByteArray())
                .joinToString("") { byte -> "%02x".format(byte.toInt() and 0xff) }
        }?.toSet() ?: emptySet()
    }

    @Suppress("DEPRECATION")
    private fun validateUpdate(path: String): File {
        val root = File(cacheDir, "updates").canonicalFile
        val apk = File(path).canonicalFile
        if (!apk.path.startsWith(root.path + File.separator) || !apk.isFile || !apk.name.endsWith(".apk")) {
            throw UpdateError("INVALID_PATH", "The update must be a downloaded APK in the app's update cache.")
        }
        val candidate = packageManager.getPackageArchiveInfo(apk.path, signingFlags())
            ?: throw UpdateError("INVALID_APK", "Android could not read the downloaded update.")
        if (candidate.packageName != packageName) {
            throw UpdateError("WRONG_PACKAGE", "This update belongs to a different app.")
        }
        val current = installedPackage()
        if (versionCode(candidate) <= versionCode(current)) {
            throw UpdateError("NOT_NEWER", "This update is not newer than the installed version.")
        }
        val trusted = signerDigests(current)
        if (trusted.isEmpty() || signerDigests(candidate) != trusted) {
            throw UpdateError("SIGNATURE_MISMATCH", "This update was not signed with the installed app's signing key.")
        }
        return apk
    }

    private fun reportError(result: MethodChannel.Result, error: Exception) {
        when (error) {
            is UpdateError -> result.error(error.code, error.message, null)
            is ActivityNotFoundException -> result.error("INSTALLER_UNAVAILABLE", "Android could not open the installer or install-permission settings.", null)
            is SecurityException -> result.error("PERMISSION_DENIED", "Android denied access to the update installer. Check install permissions and retry.", null)
            else -> result.error("UPDATE_ERROR", "The update could not be opened. Please download it again and retry.", null)
        }
    }

    override fun onDestroy() {
        updateExecutor.shutdown()
        super.onDestroy()
    }

    private class UpdateError(val code: String, message: String) : Exception(message)
}
