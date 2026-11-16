package ai.littlebit.rst.screen

import android.content.Intent
import android.graphics.Bitmap
import android.os.SystemClock
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
    /** When the image of each frame came from the mirror (uptime ms). */
    private val arrived = HashMap<Long, Long>()

    /**
     * Frames Rust still works on: the one `next` last handed over and the
     * clean frame taken for it. They outlive [KEEP], so its strips and
     * detection find them however many frames come meanwhile.
     */
    private val pinned = HashSet<Long>()
    /** The frame `next` last handed over: the one a clean frame is taken for. */
    private var delivered = 0L

    /** How long [cleanFrame] waits for an image with the button gone, at most. */
    private const val CLEAN_WAIT_MS = 700L
    /** By this long after the button was hidden, the mirror has sent the screen without it. */
    private const val SETTLE_MS = 50L

    /** A new image from the mirror, which came at uptime [at]. */
    fun onFrame(bmp: Bitmap, at: Long) {
        if (paused) return
        val thumb = thumbnail(bmp)
        synchronized(lock) {
            store(bmp, thumb, at)
            (lock as java.lang.Object).notifyAll()
        }
    }

    private fun store(bmp: Bitmap, thumb: ByteArray, at: Long) {
        seq += 1
        frames[seq] = bmp
        thumbs[seq] = thumb
        arrived[seq] = at
        while (frames.size > KEEP) {
            val old = frames.keys.first { it !in pinned }
            frames.remove(old)
            thumbs.remove(old)
            arrived.remove(old)
        }
        fresh = seq
        deliver()
    }

    /**
     * A new frame of the screen with the floating button hidden for it, or
     * null. The capture holds the button (A6): the rows behind it are saved
     * clean from this frame (P9). The button is gone for about one frame.
     */
    fun cleanFrame(): JSObject? {
        if (paused) return null
        val hiddenAt = Overlay.hideButtonNow() ?: return null
        try {
            // The button going away changes the screen, so the mirror sends
            // it again: once that has had time to come, the newest image
            // since the hiding is the clean one, even if the screen then
            // stays still and no other image follows. One composed before
            // the hiding may still come late: it shows the button, and is
            // passed over for the next ([buttonGone]; Rust checks it again).
            SystemClock.sleep(maxOf(0L, hiddenAt + SETTLE_MS - SystemClock.uptimeMillis()))
            CaptureService.flush()
            return newestSince(hiddenAt)
        } finally {
            Overlay.restoreButton()
        }
    }

    /**
     * The newest frame whose image came at [after] or later and no longer
     * shows the button, waiting for one.
     */
    private fun newestSince(after: Long): JSObject? {
        val deadline = SystemClock.uptimeMillis() + CLEAN_WAIT_MS
        synchronized(lock) {
            while (true) {
                frames.keys.lastOrNull { (arrived[it] ?: 0) >= after }?.takeIf { buttonGone(delivered, it) }?.let {
                    pinned.add(it)
                    return frameJson(it)
                }
                val left = deadline - SystemClock.uptimeMillis()
                if (left <= 0) return null
                (lock as java.lang.Object).wait(left)
            }
        }
    }

    /**
     * Whether the button, which frame [held] shows, is gone from frame [c]:
     * the thumbnail cells wholly under it differ (`pipeline::button_gone`).
     */
    private fun buttonGone(held: Long, c: Long): Boolean {
        val a = thumbs[held] ?: return false
        val b = thumbs[c] ?: return false
        val width = frames[c]?.width ?: return false
        val r = Overlay.buttonRect ?: return false
        if (a.size != b.size) return false
        val scale = width.toDouble() / THUMB_W
        val rows = a.size / THUMB_W
        val c0 = Math.ceil(r[0] / scale).toInt()
        val c1 = minOf(THUMB_W, Math.floor((r[0] + r[2]) / scale).toInt())
        val r0 = Math.ceil(r[1] / scale).toInt()
        val r1 = minOf(rows, Math.floor((r[1] + r[3]) / scale).toInt())
        var sum = 0L
        var n = 0
        for (row in r0 until r1) for (col in c0 until c1) {
            val i = row * THUMB_W + col
            sum += Math.abs((a[i].toInt() and 255) - (b[i].toInt() and 255))
            n += 1
        }
        return n > 0 && sum > GONE * n
    }

    /** Mean difference under the button above which it is gone (`tracker::MATCH`). */
    private const val GONE = 6.0

    /** The last frame once more, under a new number: the screen has not changed. */
    fun repeatLast() {
        if (paused) return
        synchronized(lock) {
            val last = frames.keys.lastOrNull() ?: return
            val bmp = frames[last] ?: return
            val thumb = thumbs[last] ?: return
            store(bmp, thumb, arrived[last] ?: 0)
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
            arrived.clear()
            pinned.clear()
            delivered = 0
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
        val json = frameJson(s) ?: return
        fresh = 0
        // Rust is done with the frames before: only this one is worked on now.
        pinned.clear()
        pinned.add(s)
        delivered = s
        waiting = null
        invoke.resolve(json)
    }

    private fun frameJson(s: Long): JSObject? {
        val bmp = frames[s] ?: return null
        return JSObject().apply {
            put("kind", "frame")
            put("seq", s)
            put("width", bmp.width)
            put("height", bmp.height)
            put("thumb", Base64.encodeToString(thumbs[s], Base64.NO_WRAP))
            put("capturedAt", System.currentTimeMillis())
            Overlay.buttonRect?.let { r -> put("button", JSArray().apply { r.forEach { put(it) } }) }
            put("bars", bars(bmp.width, bmp.height))
        }
    }

    /** The system bars showing now ([Overlay.barInsets]) as [x, y, w, h] in a `w` x `h` frame. */
    private fun bars(w: Int, h: Int): JSArray {
        val (left, top, right, bottom) = Overlay.barInsets
        val out = JSArray()
        fun rect(x: Int, y: Int, rw: Int, rh: Int) {
            if (rw > 0 && rh > 0) out.put(JSArray().apply { put(x); put(y); put(rw); put(rh) })
        }
        rect(0, 0, w, top)
        rect(0, h - bottom, w, bottom)
        rect(0, 0, left, h)
        rect(w - right, 0, right, h)
        return out
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
