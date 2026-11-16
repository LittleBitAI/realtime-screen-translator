package ai.littlebit.rst.screen

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.PixelFormat
import android.graphics.RectF
import android.graphics.drawable.GradientDrawable
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
 * Windows over other apps: the full-screen layer that paints each balloon
 * white and writes the Korean in it (R28), touches passing through to the
 * app underneath, and the floating on/off button (R29).
 */
object Overlay {
    private const val PAD = 4
    private val main = Handler(Looper.getMainLooper())
    private var button: TextView? = null
    private var layer: Layer? = null

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
        val lp = params(WindowManager.LayoutParams.WRAP_CONTENT, touchable = true).apply {
            gravity = Gravity.END or Gravity.CENTER_VERTICAL
            x = 16
        }
        ctx.getSystemService(WindowManager::class.java).addView(b, lp)
        button = b
    }

    fun setOn(on: Boolean) = main.post {
        button?.text = if (on) "번역 끄기" else "번역 켜기"
    }

    fun showLayer(ctx: Context) = main.post {
        if (layer != null) return@post
        val v = Layer(ctx)
        ctx.getSystemService(WindowManager::class.java).addView(v, params(WindowManager.LayoutParams.MATCH_PARENT, touchable = false))
        layer = v
    }

    fun draw(shapes: List<Shape>) = main.post {
        layer?.show(shapes)
    }

    private fun params(size: Int, touchable: Boolean): WindowManager.LayoutParams {
        var flags = WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
            WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN or
            WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS
        if (!touchable) flags = flags or WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE
        return WindowManager.LayoutParams(
            size, size, WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY, flags, PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
            }
        }
    }

    private class Layer(ctx: Context) : View(ctx) {
        private var shapes: List<Shape> = emptyList()
        private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.WHITE }
        private val text = TextPaint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.BLACK }

        fun show(s: List<Shape>) {
            shapes = s
            invalidate()
        }

        override fun onDraw(canvas: Canvas) {
            for (s in shapes) {
                val r = s.radius.toFloat()
                canvas.drawRoundRect(RectF(s.x.toFloat(), s.y.toFloat(), (s.x + s.w).toFloat(), (s.y + s.h).toFloat()), r, r, fill)
                text.textSize = s.fontPx.toFloat()
                val inner = maxOf(1, s.w - 2 * PAD)
                val layout = StaticLayout.Builder.obtain(s.ko, 0, s.ko.length, text, inner)
                    .setAlignment(Layout.Alignment.ALIGN_CENTER)
                    .setLineSpacing(0f, 1.2f)
                    .setIncludePad(false)
                    .build()
                canvas.save()
                canvas.translate((s.x + PAD).toFloat(), s.y + (s.h - layout.height) / 2f)
                layout.draw(canvas)
                canvas.restore()
            }
        }
    }
}
