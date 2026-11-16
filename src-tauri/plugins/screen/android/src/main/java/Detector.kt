package ai.littlebit.rst.screen

import ai.onnxruntime.OnnxTensor
import ai.onnxruntime.OrtEnvironment
import ai.onnxruntime.OrtSession
import android.graphics.Bitmap
import java.io.File
import java.nio.FloatBuffer
import java.nio.LongBuffer
import kotlin.math.max
import kotlin.math.min

/**
 * R25: the balloon model, the same file and contract as
 * `src-tauri/src/detect.rs`: the pinned RT-DETR-v2 "comic-text-and-bubble-
 * detector" that Rust downloads into the app data folder (`models.rs`).
 * Inputs `images` [1, 3, 640, 640] RGB in 0..1, the screen resized (not
 * letterboxed), and `orig_target_sizes` [1, 2] (width, height); outputs
 * `labels` [1, n], `boxes` [1, n, 4] (x1, y1, x2, y2 in screen pixels) and
 * `scores` [1, n]. Label 1 is dialogue text, 2 sound-effect text, 0 the
 * balloon outline (left out).
 */
class Detector private constructor(private val env: OrtEnvironment, private val session: OrtSession) {
    class Box(val x: Float, val y: Float, val w: Float, val h: Float, val label: Long, val score: Float)

    fun detect(bmp: Bitmap): List<Box> {
        val small = Bitmap.createScaledBitmap(bmp, SIZE, SIZE, false)
        val px = IntArray(SIZE * SIZE)
        small.getPixels(px, 0, SIZE, 0, 0, SIZE, SIZE)
        val n = SIZE * SIZE
        val input = FloatBuffer.allocate(3 * n)
        for (d in 0 until n) {
            val c = px[d]
            input.put(d, ((c shr 16) and 255) / 255f)
            input.put(n + d, ((c shr 8) and 255) / 255f)
            input.put(2 * n + d, (c and 255) / 255f)
        }
        val sizes = LongBuffer.wrap(longArrayOf(bmp.width.toLong(), bmp.height.toLong()))
        val boxes = ArrayList<Box>()
        OnnxTensor.createTensor(env, input, longArrayOf(1, 3, SIZE.toLong(), SIZE.toLong())).use { images ->
            OnnxTensor.createTensor(env, sizes, longArrayOf(1, 2)).use { orig ->
                session.run(mapOf("images" to images, "orig_target_sizes" to orig)).use { result ->
                    @Suppress("UNCHECKED_CAST")
                    val labels = (result.get("labels").get().value as Array<LongArray>)[0]
                    @Suppress("UNCHECKED_CAST")
                    val xyxy = (result.get("boxes").get().value as Array<Array<FloatArray>>)[0]
                    @Suppress("UNCHECKED_CAST")
                    val scores = (result.get("scores").get().value as Array<FloatArray>)[0]
                    for (i in labels.indices) {
                        val l = labels[i]
                        if ((l != DIALOGUE && l != EFFECT) || scores[i] < MIN_SCORE) continue
                        val b = xyxy[i]
                        val w = b[2] - b[0]
                        val h = b[3] - b[1]
                        if (w > 0 && h > 0) boxes.add(Box(b[0], b[1], w, h, l, scores[i]))
                    }
                }
            }
        }
        return nms(boxes)
    }

    private fun nms(boxes: List<Box>): List<Box> {
        val keep = ArrayList<Box>()
        for (b in boxes.sortedByDescending { it.score }) {
            if (keep.all { iou(it, b) <= MAX_IOU }) keep.add(b)
        }
        return keep
    }

    private fun iou(a: Box, b: Box): Float {
        val w = min(a.x + a.w, b.x + b.w) - max(a.x, b.x)
        val h = min(a.y + a.h, b.y + b.h) - max(a.y, b.y)
        if (w <= 0 || h <= 0) return 0f
        return w * h / (a.w * a.h + b.w * b.h - w * h)
    }

    companion object {
        const val SIZE = 640
        const val MIN_SCORE = 0.45f
        const val MAX_IOU = 0.5f
        const val DIALOGUE = 1L
        const val EFFECT = 2L

        @Volatile private var loaded: Detector? = null

        /** The model at `path`, or null until Rust has it in place (`detector_missing`). */
        fun get(path: String): Detector? {
            loaded?.let { return it }
            synchronized(this) {
                loaded?.let { return it }
                if (!File(path).isFile) return null
                val env = OrtEnvironment.getEnvironment()
                return Detector(env, env.createSession(path, OrtSession.SessionOptions())).also { loaded = it }
            }
        }
    }
}
