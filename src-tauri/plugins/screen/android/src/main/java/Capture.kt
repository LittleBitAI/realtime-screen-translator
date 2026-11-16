package ai.littlebit.rst.screen

import android.content.Intent
import android.graphics.Bitmap
import android.util.Base64
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject

/**
 * Frames from [CaptureService] and taps of the on/off button, handed to
 * Rust one at a time through the pending `next` call. Only the newest frame
 * waits: if Rust is still busy, older frames are dropped, so the app being
 * read never waits for the translator (R7).
 */
object Capture {
    class Grant(val code: Int, val data: Intent)

    /** Consent from the capture prompt, used once by [CaptureService]. */
    @Volatile var grant: Grant? = null
    /** The capture service holds a live MediaProjection. */
    @Volatile var alive = false
    /** Translation is off: frames are not delivered. */
    @Volatile var paused = true

    private const val KEEP = 3
    private const val THUMB_W = 96
    private val lock = Object()
    private var waiting: Invoke? = null
    private var taps = 0
    private var seq = 0L
    private var fresh = 0L
    private val frames = LinkedHashMap<Long, Bitmap>()
    private val thumbs = HashMap<Long, ByteArray>()

    fun onFrame(bmp: Bitmap) {
        if (paused) return
        val thumb = thumbnail(bmp)
        synchronized(lock) {
            seq += 1
            frames[seq] = bmp
            thumbs[seq] = thumb
            while (frames.size > KEEP) {
                val old = frames.keys.first()
                frames.remove(old)
                thumbs.remove(old)
            }
            fresh = seq
            deliver()
        }
    }

    /** The last frame once more, under a new number: the screen has not changed. */
    fun repeatLast() {
        if (paused) return
        synchronized(lock) {
            val last = frames.keys.lastOrNull() ?: return
            val bmp = frames[last] ?: return
            val thumb = thumbs[last] ?: return
            seq += 1
            frames[seq] = bmp
            thumbs[seq] = thumb
            while (frames.size > KEEP) {
                val old = frames.keys.first()
                frames.remove(old)
                thumbs.remove(old)
            }
            fresh = seq
            deliver()
        }
    }

    fun toggle() {
        synchronized(lock) {
            taps += 1
            deliver()
        }
    }

    fun next(invoke: Invoke) {
        synchronized(lock) {
            waiting?.reject("superseded")
            waiting = invoke
            deliver()
        }
    }

    fun frame(seq: Long): Bitmap? = synchronized(lock) { frames[seq] }

    fun clear() {
        synchronized(lock) {
            frames.clear()
            thumbs.clear()
            fresh = 0
        }
    }

    private fun deliver() {
        val invoke = waiting ?: return
        if (taps > 0) {
            taps -= 1
            waiting = null
            invoke.resolve(JSObject().apply { put("kind", "toggle") })
            return
        }
        val s = fresh
        val bmp = frames[s] ?: return
        fresh = 0
        waiting = null
        invoke.resolve(JSObject().apply {
            put("kind", "frame")
            put("seq", s)
            put("width", bmp.width)
            put("height", bmp.height)
            put("thumb", Base64.encodeToString(thumbs[s], Base64.NO_WRAP))
            put("capturedAt", System.currentTimeMillis())
            Overlay.buttonRect?.let { r -> put("button", JSArray().apply { r.forEach { put(it) } }) }
        })
    }

    /** A2: 96 px wide, grayscale, `height * 96 / width` rows. */
    private fun thumbnail(bmp: Bitmap): ByteArray {
        val rows = bmp.height * THUMB_W / bmp.width
        val small = Bitmap.createScaledBitmap(bmp, THUMB_W, rows, true)
        val px = IntArray(THUMB_W * rows)
        small.getPixels(px, 0, THUMB_W, 0, 0, THUMB_W, rows)
        return ByteArray(px.size) { i ->
            val c = px[i]
            ((299 * ((c shr 16) and 255) + 587 * ((c shr 8) and 255) + 114 * (c and 255)) / 1000).toByte()
        }
    }
}
