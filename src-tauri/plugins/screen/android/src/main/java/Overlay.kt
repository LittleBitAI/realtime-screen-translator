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
import android.view.View
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
    private val main = Handler(Looper.getMainLooper())
    private var button: TextView? = null
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
        }
        val wrap = WindowManager.LayoutParams.WRAP_CONTENT
        val lp = params(wrap, wrap, touchable = true).apply {
            gravity = Gravity.END or Gravity.CENTER_VERTICAL
            x = 16
        }
        ctx.getSystemService(WindowManager::class.java).addView(b, lp)
        button = b
    }

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
                val v = ShapeView(host).also { it.show(s) }
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
            text.textSize = s.fontPx.toFloat()
            val inner = maxOf(1, s.w - 2 * PAD)
            val layout = StaticLayout.Builder.obtain(s.ko, 0, s.ko.length, text, inner)
                .setAlignment(Layout.Alignment.ALIGN_CENTER)
                .setLineSpacing(0f, 1.2f)
                .setIncludePad(false)
                .build()
            canvas.save()
            canvas.translate(PAD.toFloat(), (s.h - layout.height) / 2f)
            layout.draw(canvas)
            canvas.restore()
        }
    }
}
