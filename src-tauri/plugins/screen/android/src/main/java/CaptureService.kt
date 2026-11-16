package ai.littlebit.rst.screen

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.PixelFormat
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.Image
import android.media.ImageReader
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import android.util.DisplayMetrics
import android.view.Display
import android.view.WindowManager
import androidx.core.content.ContextCompat

/**
 * Holds the MediaProjection for as long as the person allows it. While
 * translation is on the screen is mirrored at full size into an ImageReader
 * and frames reach [Capture] every [FRAME_GAP_MS]: the newest image if one
 * came, else the last frame again. The mirror sends images only when the
 * screen changes, and P6 asks for detection and a strip only once the same
 * screen came twice, so a still screen is sent again [STILL_REPEATS] times
 * and then every [IDLE_EVERY] ticks (retries after a failed detection).
 * While it is off
 * the mirror has no surface, so nothing is captured (P5); the projection and
 * its one virtual display stay, because Android 14 lets a consent make only
 * one display and R29 asks only the first time.
 */
class CaptureService : Service() {
    private var projection: MediaProjection? = null
    private var display: VirtualDisplay? = null
    private var reader: ImageReader? = null
    private val thread = HandlerThread("rst-capture").apply { start() }
    private val handler = Handler(thread.looper)
    /** The newest image not yet sent; only the capture thread touches it. */
    private var held: Image? = null
    private var heldAt = 0L
    private var idle = 0
    private val tick = object : Runnable {
        override fun run() {
            val img = held
            if (img != null) {
                held = null
                idle = 0
                try {
                    if (!Capture.paused) Capture.onFrame(bitmap(img), heldAt)
                } finally {
                    img.close()
                }
            } else if (!Capture.paused) {
                idle += 1
                if (idle <= STILL_REPEATS || idle % IDLE_EVERY == 0) Capture.repeatLast()
            }
            handler.postDelayed(this, FRAME_GAP_MS)
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Android 14: the typed foreground service must run before the projection is taken.
        foreground()
        if (projection != null) {
            started()
            return START_NOT_STICKY
        }
        val grant = Capture.grant
        Capture.grant = null
        val mgr = getSystemService(MediaProjectionManager::class.java)
        val p = grant?.let { mgr.getMediaProjection(it.code, it.data) }
        if (p == null) {
            stopSelf()
            started()
            return START_NOT_STICKY
        }
        p.registerCallback(object : MediaProjection.Callback() {
            override fun onStop() {
                // Revoked from the system UI: capture is no longer allowed.
                Capture.alive = false
                stopSelf()
            }
        }, handler)
        val m = metrics()
        val r = newReader(m.widthPixels, m.heightPixels)
        display = p.createVirtualDisplay(
            "rst-capture", m.widthPixels, m.heightPixels, m.densityDpi,
            DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR, if (capturing) r.surface else null, null, handler,
        )
        projection = p
        reader = r
        running = this
        Capture.alive = true
        getSystemService(DisplayManager::class.java).registerDisplayListener(turns, handler)
        handler.post(tick)
        started()
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        running = null
        started()
        getSystemService(DisplayManager::class.java).unregisterDisplayListener(turns)
        handler.removeCallbacks(tick)
        handler.post {
            held?.close()
            held = null
        }
        Capture.alive = false
        Capture.paused = true
        Capture.clear()
        display?.release()
        reader?.close()
        projection?.stop()
        thread.quitSafely()
        super.onDestroy()
    }

    /** Three buffers: one held for the next tick, one being acquired, one being written. */
    private fun newReader(w: Int, h: Int): ImageReader {
        val r = ImageReader.newInstance(w, h, PixelFormat.RGBA_8888, 3)
        r.setOnImageAvailableListener({ ir ->
            val img = ir.acquireLatestImage() ?: return@setOnImageAvailableListener
            held?.close()
            held = img
            heldAt = SystemClock.uptimeMillis()
        }, handler)
        return r
    }

    /** On the capture thread: the screen turned or changed size. */
    private val turns = object : DisplayManager.DisplayListener {
        override fun onDisplayAdded(displayId: Int) {}
        override fun onDisplayRemoved(displayId: Int) {}
        override fun onDisplayChanged(displayId: Int) {
            if (displayId == Display.DEFAULT_DISPLAY) resize()
        }
    }

    /**
     * The mirror follows the screen's new size, so frames keep the screen's
     * coordinates and P6 sees the turned screen (a new picture in its size);
     * the overlay and the button are placed in the same coordinates.
     */
    private fun resize() {
        val d = display ?: return
        val old = reader ?: return
        val m = metrics()
        if (m.widthPixels == old.width && m.heightPixels == old.height) return
        val r = newReader(m.widthPixels, m.heightPixels)
        d.resize(m.widthPixels, m.heightPixels, m.densityDpi)
        d.setSurface(if (capturing) r.surface else null)
        reader = r
        held?.close()
        held = null
        old.close()
        Overlay.screenTurned()
    }

    private fun metrics(): DisplayMetrics {
        val m = DisplayMetrics()
        @Suppress("DEPRECATION")
        (getSystemService(Context.WINDOW_SERVICE) as WindowManager).defaultDisplay.getRealMetrics(m)
        return m
    }

    private fun bitmap(img: Image): Bitmap {
        val plane = img.planes[0]
        val rowPx = plane.rowStride / plane.pixelStride
        val full = Bitmap.createBitmap(rowPx, img.height, Bitmap.Config.ARGB_8888)
        full.copyPixelsFromBuffer(plane.buffer)
        return if (rowPx == img.width) full else Bitmap.createBitmap(full, 0, 0, img.width, img.height)
    }

    private fun foreground() {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CHANNEL, "화면 번역", NotificationManager.IMPORTANCE_LOW))
        val n = Notification.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.ic_menu_view)
            .setContentTitle("실시간 화면 번역")
            .setContentText("화면 캡처가 허용되어 있어요")
            .setOngoing(true)
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIFICATION_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION)
        } else {
            startForeground(NOTIFICATION_ID, n)
        }
    }

    companion object {
        const val FRAME_GAP_MS = 100L
        const val STILL_REPEATS = 3
        const val IDLE_EVERY = 10
        private const val START_WAIT_MS = 5000L
        private const val CHANNEL = "rst-capture"
        private const val NOTIFICATION_ID = 7031
        @Volatile private var running: CaptureService? = null
        @Volatile private var capturing = false

        /** P4 gives the mirror its surface back; P5 takes it away, and capture stops. */
        fun setCapturing(on: Boolean) {
            capturing = on
            val s = running ?: return
            s.handler.post {
                s.display?.setSurface(if (capturing) s.reader?.surface else null)
                if (!capturing) {
                    s.held?.close()
                    s.held = null
                }
            }
        }

        private val main = Handler(Looper.getMainLooper())
        private var whenStarted: (() -> Unit)? = null

        /**
         * Starts the service with the consent in [Capture.grant]; [then] runs
         * on the main thread once the projection is held or refused
         * ([Capture.alive] tells which), or after [START_WAIT_MS] at most.
         */
        fun start(ctx: Context, then: () -> Unit) {
            main.post {
                // A start still waiting is answered as it stands.
                whenStarted?.invoke()
                whenStarted = then
                main.postDelayed({ if (whenStarted === then) started() }, START_WAIT_MS)
                ContextCompat.startForegroundService(ctx, Intent(ctx, CaptureService::class.java))
            }
        }

        private fun started() = main.post {
            val then = whenStarted ?: return@post
            whenStarted = null
            then()
        }
    }
}
