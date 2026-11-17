package ai.littlebit.rst.screen

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.PixelFormat
import android.graphics.RectF
import android.graphics.drawable.GradientDrawable
import android.hardware.input.InputManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.WindowInsets
import android.view.WindowManager
import android.widget.TextView

/** A placement from P8, in screen pixels. */
class Shape(val x: Int, val y: Int, val w: Int, val h: Int, val radius: Int, val ko: String, val fontPx: Int)

/**
 * Windows over other apps: one window per balloon that paints it white and
 * writes the Korean in it (R28), every touch passing through to the app
 * underneath (P4), and the floating on/off button (R29).
 */
object Overlay {
    private const val PAD = 4
    /** The smallest Korean the overlay shrinks to when the device font runs over. */
    private const val MIN_TEXT_PX = 6f
    private val main = Handler(Looper.getMainLooper())
    private var button: TextView? = null
    /**
     * The button's x, y, w, h on screen with a small margin, or null. The
     * capture holds it (A6), so the rows behind it are not saved (P9).
     */
    @Volatile var buttonRect: IntArray? = null
    /** Set while translation is on: the context shape windows are made with. */
    private var layer: Context? = null
    private val shapes = ArrayList<ShapeView>()

    fun showButton(ctx: Context) = main.post {
        if (button != null || !Settings.canDrawOverlays(ctx)) return@post
        val b = TextView(ctx).apply {
            text = "번역 켜기"
            setTextColor(Color.WHITE)
            textSize = 14f
            setPadding(36, 24, 36, 24)
            background = GradientDrawable().apply {
                cornerRadius = 48f
                setColor(0xCC222222.toInt())
            }
            setOnClickListener { Capture.toggle() }
            // A touch anywhere else reaches the reader's app; the button is
            // only told that one came (FLAG_WATCH_OUTSIDE_TOUCH below).
            setOnTouchListener { _, e ->
                if (e.actionMasked == MotionEvent.ACTION_OUTSIDE) Capture.touched()
                false
            }
        }
        val wrap = WindowManager.LayoutParams.WRAP_CONTENT
        // P4: near the bottom edge, where a thumb reaches, just above the navigation bar.
        val res = ctx.resources
        val navId = res.getIdentifier("navigation_bar_height", "dimen", "android")
        val nav = if (navId > 0) res.getDimensionPixelSize(navId) else 0
        val lp = params(wrap, wrap, touchable = true).apply {
            flags = flags or WindowManager.LayoutParams.FLAG_WATCH_OUTSIDE_TOUCH
            gravity = Gravity.END or Gravity.BOTTOM
            x = 16
            y = nav + (24 * res.displayMetrics.density).toInt()
        }
        // Its window moves when the screen turns: measured again after every layout pass.
        b.viewTreeObserver.addOnGlobalLayoutListener { measureButton(b) }
        ctx.getSystemService(WindowManager::class.java).addView(b, lp)
        button = b
        showProbe(ctx)
    }

    /**
     * The system bars showing now, as insets (left, top, right, bottom) in
     * screen pixels: the capture holds them, and they are not the picture.
     * A reader app that hides them leaves zeros.
     */
    @Volatile var barInsets = IntArray(4)
    private var probe: View? = null

    /**
     * A window over the whole screen that shows nothing and takes no touch
     * (fully clear, so Android 12+ lets touches through it): Android tells it
     * where the status bar and the navigation bar are, and whether they show.
     */
    private fun showProbe(ctx: Context) {
        if (probe != null) return
        val v = View(ctx)
        v.setOnApplyWindowInsetsListener { _, insets ->
            barInsets = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                val i = insets.getInsets(WindowInsets.Type.systemBars())
                intArrayOf(i.left, i.top, i.right, i.bottom)
            } else {
                @Suppress("DEPRECATION")
                intArrayOf(insets.systemWindowInsetLeft, insets.systemWindowInsetTop, insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
            }
            insets
        }
        val match = WindowManager.LayoutParams.MATCH_PARENT
        val flags = WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
            WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE or
            WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN or
            WindowManager.LayoutParams.FLAG_LAYOUT_INSET_DECOR
        val lp = WindowManager.LayoutParams(match, match, WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY, flags, PixelFormat.TRANSPARENT).apply {
            gravity = Gravity.TOP or Gravity.START
            alpha = 0f
            // Laid over the bars, not fitted inside them: else it is told of none.
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) fitInsetsTypes = 0
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
            }
        }
        ctx.getSystemService(WindowManager::class.java).addView(v, lp)
        probe = v
    }

    /**
     * Hides the button from the screen and the capture, from a worker thread:
     * answered with the uptime it was gone at, or null if there is none.
     */
    fun hideButtonNow(): Long? {
        val done = java.util.concurrent.CountDownLatch(1)
        var at: Long? = null
        main.post {
            button?.takeIf { it.visibility == View.VISIBLE }?.let {
                it.visibility = View.INVISIBLE
                at = android.os.SystemClock.uptimeMillis()
            }
            done.countDown()
        }
        done.await(500, java.util.concurrent.TimeUnit.MILLISECONDS)
        return at
    }

    fun restoreButton() = main.post { button?.visibility = View.VISIBLE }

    /** While [Capture.peekFrame] looks under them: shapes, new ones too, stay hidden. */
    private var shapesHidden = false

    /**
     * Hides the shapes from the screen and the capture, from a worker thread:
     * answered with the uptime they were gone at, or null if none shows.
     */
    fun hideShapesNow(): Long? {
        val done = java.util.concurrent.CountDownLatch(1)
        var at: Long? = null
        main.post {
            if (shapes.isNotEmpty() && !shapesHidden) {
                shapesHidden = true
                shapes.forEach { it.visibility = View.INVISIBLE }
                at = android.os.SystemClock.uptimeMillis()
            }
            done.countDown()
        }
        done.await(500, java.util.concurrent.TimeUnit.MILLISECONDS)
        return at
    }

    fun restoreShapes() = main.post {
        shapesHidden = false
        shapes.forEach { it.visibility = View.VISIBLE }
    }

    private fun measureButton(v: View) {
        val at = IntArray(2)
        v.getLocationOnScreen(at)
        val m = 4
        buttonRect = intArrayOf(maxOf(0, at[0] - m), maxOf(0, at[1] - m), v.width + 2 * m, v.height + 2 * m)
    }

    /** The screen turned: the button's place is measured again once its window has moved. */
    fun screenTurned() = main.postDelayed({ button?.let { measureButton(it) } }, 300)

    fun setOn(on: Boolean) = main.post {
        button?.text = if (on) "번역 끄기" else "번역 켜기"
    }

    /**
     * Translation is on: shapes may be drawn, each in its own small window
     * that never takes a touch (P4: touches always reach the reader's app).
     * Android 12+ holds back a touch that passes through another app's
     * window unless that window is at most 80% opaque or an accessibility
     * overlay, so there translation starts only with [OverlayService] on
     * (ScreenPlugin.overlayReady): the shapes are accessibility overlays and
     * fully opaque, and the white hides the Japanese completely (R28). Only
     * while the service is still binding, or after it was switched off
     * mid-session, are they drawn as opaque as Android still lets touches
     * through (`InputManager.maximumObscuringOpacityForTouch`).
     */
    fun showLayer(ctx: Context) = main.post {
        if (layer == null) layer = ctx
    }

    /** Takes every shape down (P5): nothing stays over other apps but the button. */
    fun hideLayer() = main.post {
        removeShapes()
        layer = null
    }

    /** The accessibility service came or went: shapes are made again in the right kind of window. */
    fun serviceChanged() = main.post { removeShapes() }

    private fun removeShapes() {
        for (v in shapes) runCatching { v.context.getSystemService(WindowManager::class.java).removeView(v) }
        shapes.clear()
    }

    fun draw(list: List<Shape>) = main.post {
        val app = layer ?: return@post
        val service = OverlayService.running
        val host: Context = service ?: app
        if (shapes.any { it.context !== host }) removeShapes()
        val wm = host.getSystemService(WindowManager::class.java)
        while (shapes.size > list.size) runCatching { wm.removeView(shapes.removeAt(shapes.lastIndex)) }
        list.forEachIndexed { i, s ->
            val lp = params(maxOf(1, s.w), maxOf(1, s.h), touchable = false, trusted = service != null).apply {
                x = s.x
                y = s.y
                alpha = if (service != null) 1f else passThroughAlpha(app)
            }
            if (i < shapes.size) {
                shapes[i].show(s)
                wm.updateViewLayout(shapes[i], lp)
            } else {
                val v = ShapeView(host).also {
                    it.show(s)
                    if (shapesHidden) it.visibility = View.INVISIBLE
                }
                wm.addView(v, lp)
                shapes.add(v)
            }
        }
    }

    private fun passThroughAlpha(ctx: Context): Float {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return 1f
        val max = ctx.getSystemService(InputManager::class.java)?.maximumObscuringOpacityForTouch ?: 0.8f
        return minOf(max, 0.8f)
    }

    private fun params(w: Int, h: Int, touchable: Boolean, trusted: Boolean = false): WindowManager.LayoutParams {
        var flags = WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
            WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN or
            WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS
        if (!touchable) flags = flags or WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE
        val type = if (trusted) WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY else WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY
        return WindowManager.LayoutParams(w, h, type, flags, PixelFormat.TRANSLUCENT).apply {
            gravity = Gravity.TOP or Gravity.START
            // x / y are full-screen capture coordinates: not shifted below
            // the status bar or beside the navigation bar (Android 11+ fits
            // windows inside the bars unless told not to).
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) fitInsetsTypes = 0
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
            }
        }
    }

    /** One placement: the white shape filling the window and the Korean centred in it. */
    private class ShapeView(ctx: Context) : View(ctx) {
        private var shape: Shape? = null
        private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.WHITE }
        private val text = TextPaint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.BLACK }

        fun show(s: Shape) {
            shape = s
            invalidate()
        }

        override fun onDraw(canvas: Canvas) {
            val s = shape ?: return
            val r = s.radius.toFloat()
            canvas.drawRoundRect(RectF(0f, 0f, s.w.toFloat(), s.h.toFloat()), r, r, fill)
            val inner = maxOf(1, s.w - 2 * PAD)
            val room = maxOf(1, s.h - 2 * PAD)
            // `layout.rs` fitted the size at 1.2 em a line: the lines are set
            // that far apart, and the size comes down while the device's own
            // font still takes more room than the shape has.
            var size = s.fontPx.toFloat()
            var layout = textLayout(s.ko, size, inner)
            while (layout.height > room && size > MIN_TEXT_PX) {
                size -= 1f
                layout = textLayout(s.ko, size, inner)
            }
            canvas.save()
            canvas.translate(PAD.toFloat(), (s.h - layout.height) / 2f)
            layout.draw(canvas)
            canvas.restore()
        }

        private fun textLayout(ko: String, size: Float, inner: Int): StaticLayout {
            text.textSize = size
            return StaticLayout.Builder.obtain(ko, 0, ko.length, text, inner)
                .setAlignment(Layout.Alignment.ALIGN_CENTER)
                // From the font's own line height to 1.2 em, as `layout::text_height`.
                .setLineSpacing(1.2f * size - text.fontSpacing, 1f)
                .setIncludePad(false)
                .build()
        }
    }
}
