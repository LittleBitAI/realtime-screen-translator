package ai.littlebit.rst.screen

import android.app.Activity
import android.content.ContentValues
import android.content.Intent
import android.graphics.Bitmap
import android.media.projection.MediaProjectionConfig
import android.media.projection.MediaProjectionManager
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.provider.Settings
import android.util.Base64
import android.widget.Toast
import androidx.activity.result.ActivityResult
import android.Manifest
import app.tauri.PermissionState
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.ByteArrayOutputStream
import java.io.File
import java.util.concurrent.Executors

@InvokeArg
class RangeArgs {
    var seq: Long = 0
    var top: Int = 0
    var bottom: Int = 0
}

@InvokeArg
class PlacementArg {
    var x: Int = 0
    var y: Int = 0
    var w: Int = 0
    var h: Int = 0
    var radius: Int = 0
    var ko: String = ""
    var font_px: Int = 16
}

@InvokeArg
class DrawArgs {
    var placements: Array<PlacementArg> = arrayOf()
    var clear: Boolean = false
}

@InvokeArg
class DetectArgs {
    var seq: Long = 0
    var top: Int = 0
    var bottom: Int = 0
    var model: String = ""
}

@InvokeArg
class PublishArgs {
    var path: String = ""
}

@InvokeArg
class BytesArgs {
    var data: String = ""
}

/**
 * The Android screen side (A1, A2, A6), called from Rust only:
 * permissions and the floating on/off button (R29), MediaProjection frames
 * as 96-px grayscale thumbnails (`next`), balloon detection with the local
 * ONNX model (`detect`, R25), clean screen rows (`strip`), the overlay
 * (`draw`, R28, R31) and the Keystore that seals the Gemini key (`seal`).
 */
// Android 8 and 9 (API 26-28) write to the shared gallery only with this permission.
@TauriPlugin(permissions = [Permission(strings = [Manifest.permission.WRITE_EXTERNAL_STORAGE], alias = "storage")])
class ScreenPlugin(private val activity: Activity) : Plugin(activity) {
    // Model runs and PNG encoding stay off the UI thread.
    private val worker = Executors.newSingleThreadExecutor()

    private companion object {
        /** `detect::EDGE`: a box this close to the screen's top or bottom may be cut by it. */
        const val EDGE = 12f
    }

    /**
     * Drawing over other apps. From Android 12 an opaque window of another
     * app holds back the touches under it unless it is an accessibility
     * overlay, so there the overlay also needs the "번역 오버레이" service
     * (Overlay.kt): without it the white could not hide the Japanese (R28)
     * while every touch still reaches the reader's app (P4).
     */
    private fun overlayReady(): Boolean =
        Settings.canDrawOverlays(activity) &&
            (Build.VERSION.SDK_INT < Build.VERSION_CODES.S || OverlayService.enabled(activity))

    private fun state(): JSObject = JSObject().apply {
        put("capture", Capture.alive)
        put("overlay", overlayReady())
    }

    /**
     * This app's own screens came to the front or left it: while they show,
     * nothing is captured, and the floating button and the shapes step aside
     * (the screens have their own on/off button).
     */
    private fun front(shown: Boolean) {
        Capture.appInFront = shown
        Overlay.appInFront(shown)
    }

    override fun onResume() = front(true)

    override fun onPause() = front(false)

    private fun resumedNow(): Boolean =
        (activity as? androidx.lifecycle.LifecycleOwner)?.lifecycle?.currentState
            ?.isAtLeast(androidx.lifecycle.Lifecycle.State.RESUMED) == true

    @Command
    fun permissions(invoke: Invoke) {
        invoke.resolve(state())
    }

    /**
     * Screen capture first, then drawing over other apps (R29). Named apart
     * from `Plugin.requestPermissions`, the alias permission command it
     * would otherwise hide.
     */
    @Command
    fun askCapture(invoke: Invoke) {
        if (Capture.alive) {
            askOverlay(invoke)
            return
        }
        val mgr = activity.getSystemService(MediaProjectionManager::class.java)
        // The whole screen (A2): from Android 14 the prompt also offers a
        // single app, whose capture would stay on that app when the reader
        // moves to another and be laid out in its window, not the screen.
        val intent = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            mgr.createScreenCaptureIntent(MediaProjectionConfig.createConfigForDefaultDisplay())
        } else {
            mgr.createScreenCaptureIntent()
        }
        startActivityForResult(invoke, intent, "captureAnswered")
    }

    @ActivityCallback
    private fun captureAnswered(invoke: Invoke, result: ActivityResult) {
        val data = result.data
        if (result.resultCode == Activity.RESULT_OK && data != null) {
            // The service starts now, while the app is in front, and stays
            // paused until translation is switched on. The answer waits for
            // it, so P3 reports the capture as allowed once it really is.
            Capture.grant = Capture.Grant(result.resultCode, data)
            CaptureService.start(activity) { askOverlay(invoke) }
            return
        }
        askOverlay(invoke)
    }

    private fun askOverlay(invoke: Invoke) {
        if (Settings.canDrawOverlays(activity)) {
            overlayAnswered(invoke, ActivityResult(Activity.RESULT_OK, null))
            return
        }
        val intent = Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, Uri.parse("package:" + activity.packageName))
        startActivityForResult(invoke, intent, "overlayAnswered")
    }

    @ActivityCallback
    private fun overlayAnswered(invoke: Invoke, @Suppress("UNUSED_PARAMETER") result: ActivityResult) {
        if (Settings.canDrawOverlays(activity)) {
            Overlay.showButton(activity.applicationContext)
            if (!overlayReady()) {
                // The second half of the overlay permission on Android 12+ (overlayReady).
                Toast.makeText(activity, R.string.rst_overlay_service_ask, Toast.LENGTH_LONG).show()
                startActivityForResult(invoke, Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS), "serviceAnswered")
                return
            }
        }
        invoke.resolve(state())
    }

    @ActivityCallback
    private fun serviceAnswered(invoke: Invoke, @Suppress("UNUSED_PARAMETER") result: ActivityResult) {
        invoke.resolve(state())
    }

    @Command
    fun start(invoke: Invoke) {
        if (!Capture.alive || !overlayReady()) {
            invoke.reject("permission_required")
            return
        }
        // The first resume can come before this plugin was loaded.
        front(resumedNow())
        Overlay.showButton(activity.applicationContext)
        Overlay.showLayer(activity.applicationContext)
        Overlay.setOn(true)
        CaptureService.setCapturing(true)
        Capture.paused = false
        invoke.resolve()
    }

    @Command
    fun stop(invoke: Invoke) {
        Capture.paused = true
        CaptureService.setCapturing(false)
        Capture.clear()
        // The layer comes down with translation; only the on/off button stays.
        Overlay.hideLayer()
        Overlay.setOn(false)
        invoke.resolve()
    }

    /**
     * An exported picture (a file in the app's cache) goes into the gallery
     * under Pictures/RealtimeTranslator through MediaStore (R19); answered
     * with its content URI.
     */
    @Command
    fun publish(invoke: Invoke) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q && getPermissionState("storage") != PermissionState.GRANTED) {
            // Asked when the first export is saved, answered in storageAnswered.
            requestPermissionForAlias("storage", invoke, "storageAnswered")
            return
        }
        save(invoke)
    }

    @PermissionCallback
    private fun storageAnswered(invoke: Invoke) {
        if (getPermissionState("storage") == PermissionState.GRANTED) save(invoke) else invoke.reject("storage permission denied")
    }

    private fun save(invoke: Invoke) {
        val args = invoke.parseArgs(PublishArgs::class.java)
        worker.execute {
            val file = File(args.path)
            val resolver = activity.contentResolver
            val values = ContentValues().apply {
                put(MediaStore.Images.Media.DISPLAY_NAME, file.name)
                put(MediaStore.Images.Media.MIME_TYPE, if (file.name.endsWith(".webp")) "image/webp" else "image/png")
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    put(MediaStore.Images.Media.RELATIVE_PATH, Environment.DIRECTORY_PICTURES + "/RealtimeTranslator")
                    put(MediaStore.Images.Media.IS_PENDING, 1)
                }
            }
            var uri: Uri? = null
            try {
                val u = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values) ?: throw IllegalStateException("gallery refused the picture")
                uri = u
                resolver.openOutputStream(u)!!.use { out -> file.inputStream().use { it.copyTo(out) } }
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    resolver.update(u, ContentValues().apply { put(MediaStore.Images.Media.IS_PENDING, 0) }, null, null)
                }
                file.delete()
                invoke.resolve(JSObject().apply { put("location", u.toString()) })
            } catch (e: Exception) {
                uri?.let { runCatching { resolver.delete(it, null, null) } }
                // No room in the gallery is told apart from a refusal (P11).
                invoke.reject(if (noSpace(e)) "storage_full" else (e.message ?: "gallery save failed"))
            }
        }
    }

    /** Whether [e] or one of its causes is the disk being full (ENOSPC). */
    private fun noSpace(e: Throwable): Boolean =
        generateSequence(e) { it.cause }.take(8).any {
            (it is android.system.ErrnoException && it.errno == android.system.OsConstants.ENOSPC) ||
                it.message?.contains("ENOSPC") == true || it.message?.contains("No space left") == true
        }

    /** Answered with the next frame or button tap, whenever it comes. */
    @Command
    fun next(invoke: Invoke) {
        Capture.next(invoke)
    }

    @Command
    fun detect(invoke: Invoke) {
        val args = invoke.parseArgs(DetectArgs::class.java)
        worker.execute {
            val crops = JSArray()
            val detector = runCatching { Detector.get(args.model) }.getOrNull()
            val frame = Capture.frame(args.seq)
            // Rows the model could not look at are asked for again later.
            val found = if (detector != null && frame != null) runCatching { detector.detect(frame) }.getOrNull() else null
            if (frame != null && found != null) {
                for (b in found) {
                    // As `detect::crops` in Rust: a box reaching into the asked
                    // rows or within EDGE of them; P7 holds one at the screen's
                    // edge until it knows whether the edge cut it.
                    if (b.y >= args.bottom + EDGE || b.y + b.h <= args.top - EDGE) continue
                    val x = b.x.toInt().coerceIn(0, frame.width - 1)
                    val y = b.y.toInt().coerceIn(0, frame.height - 1)
                    val w = minOf((b.x + b.w).toInt(), frame.width) - x
                    val h = minOf((b.y + b.h).toInt(), frame.height) - y
                    if (w < 4 || h < 4) continue
                    crops.put(JSObject().apply {
                        put("x", x)
                        put("y", y)
                        put("w", w)
                        put("h", h)
                        put("kind", if (b.label == Detector.EFFECT) "effect" else "dialogue")
                        put("score", b.score.toDouble())
                        put("image", png(Bitmap.createBitmap(frame, x, y, w, h)))
                    })
                }
            }
            invoke.resolve(JSObject().apply {
                put("missing", detector == null)
                put("failed", found == null)
                put("crops", crops)
            })
        }
    }

    @Command
    fun strip(invoke: Invoke) {
        val args = invoke.parseArgs(RangeArgs::class.java)
        worker.execute {
            val frame = Capture.frame(args.seq)
            val top = args.top.coerceIn(0, frame?.height ?: 0)
            val bottom = args.bottom.coerceIn(top, frame?.height ?: 0)
            val data = if (frame != null && bottom > top) png(Bitmap.createBitmap(frame, 0, top, frame.width, bottom - top)) else null
            invoke.resolve(JSObject().apply { put("data", data) })
        }
    }

    /** A new frame with the floating button hidden for it (`Capture.cleanFrame`), or kind "none". */
    @Command
    fun cleanFrame(invoke: Invoke) {
        worker.execute {
            invoke.resolve(Capture.cleanFrame() ?: JSObject().apply { put("kind", "none") })
        }
    }

    /** A new frame with the translation shapes hidden for it (`Capture.peekFrame`), or kind "none". */
    @Command
    fun peekFrame(invoke: Invoke) {
        worker.execute {
            invoke.resolve(Capture.peekFrame() ?: JSObject().apply { put("kind", "none") })
        }
    }

    @Command
    fun draw(invoke: Invoke) {
        val args = invoke.parseArgs(DrawArgs::class.java)
        Overlay.draw(args.placements.map { Shape(it.x, it.y, it.w, it.h, it.radius, it.ko, it.font_px) })
        invoke.resolve()
    }

    @Command
    fun seal(invoke: Invoke) {
        val args = invoke.parseArgs(BytesArgs::class.java)
        worker.execute {
            val out = runCatching { KeyVault.seal(Base64.decode(args.data, Base64.NO_WRAP)) }.getOrNull()
            invoke.resolve(JSObject().apply { put("data", out?.let { Base64.encodeToString(it, Base64.NO_WRAP) }) })
        }
    }

    @Command
    fun unseal(invoke: Invoke) {
        val args = invoke.parseArgs(BytesArgs::class.java)
        worker.execute {
            val out = KeyVault.unseal(Base64.decode(args.data, Base64.NO_WRAP))
            invoke.resolve(JSObject().apply { put("data", out?.let { Base64.encodeToString(it, Base64.NO_WRAP) }) })
        }
    }

    private fun png(b: Bitmap): String {
        val out = ByteArrayOutputStream()
        b.compress(Bitmap.CompressFormat.PNG, 100, out)
        return Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP)
    }
}
