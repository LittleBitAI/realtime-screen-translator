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
import android.os.SystemClock
import android.util.DisplayMetrics
import android.view.WindowManager
import androidx.core.content.ContextCompat

/**
 * Holds the MediaProjection for as long as the person allows it. While
 * translation is on the screen is mirrored at full size into an ImageReader
 * and frames reach [Capture] at most every [FRAME_GAP_MS]. While it is off
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
    private var last = 0L

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Android 14: the typed foreground service must run before the projection is taken.
        foreground()
        if (projection != null) return START_NOT_STICKY
        val grant = Capture.grant
        Capture.grant = null
        val mgr = getSystemService(MediaProjectionManager::class.java)
        val p = grant?.let { mgr.getMediaProjection(it.code, it.data) }
        if (p == null) {
            stopSelf()
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
        val r = ImageReader.newInstance(m.widthPixels, m.heightPixels, PixelFormat.RGBA_8888, 2)
        r.setOnImageAvailableListener({ ir ->
            val img = ir.acquireLatestImage() ?: return@setOnImageAvailableListener
            try {
                val now = SystemClock.uptimeMillis()
                if (!Capture.paused && now - last >= FRAME_GAP_MS) {
                    last = now
                    Capture.onFrame(bitmap(img))
                }
            } finally {
                img.close()
            }
        }, handler)
        display = p.createVirtualDisplay(
            "rst-capture", m.widthPixels, m.heightPixels, m.densityDpi,
            DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR, if (capturing) r.surface else null, null, handler,
        )
        projection = p
        reader = r
        running = this
        Capture.alive = true
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        running = null
        Capture.alive = false
        Capture.paused = true
        Capture.clear()
        display?.release()
        reader?.close()
        projection?.stop()
        thread.quitSafely()
        super.onDestroy()
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
        private const val CHANNEL = "rst-capture"
        private const val NOTIFICATION_ID = 7031
        @Volatile private var running: CaptureService? = null
        @Volatile private var capturing = false

        /** P4 gives the mirror its surface back; P5 takes it away, and capture stops. */
        fun setCapturing(on: Boolean) {
            capturing = on
            val s = running ?: return
            s.handler.post { s.display?.setSurface(if (capturing) s.reader?.surface else null) }
        }

        fun start(ctx: Context) {
            ContextCompat.startForegroundService(ctx, Intent(ctx, CaptureService::class.java))
        }
    }
}
