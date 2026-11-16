package ai.littlebit.rst.screen

import ai.onnxruntime.OnnxTensor
import ai.onnxruntime.OrtEnvironment
import ai.onnxruntime.OrtSession
import android.content.Context
import android.graphics.Bitmap
import java.io.File
import java.nio.FloatBuffer
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/**
 * R25: the balloon model (`balloons.onnx`, the same file and contract as
 * `src-tauri/src/detect.rs`): YOLOv8 export, input [1, 3, 640, 640] RGB in
 * 0..1 letterboxed on gray 114, output [1, 4 + classes, n]; class 0 dialogue
 * text, class 1 sound-effect text.
 */
class Detector private constructor(private val env: OrtEnvironment, private val session: OrtSession) {
    class Box(val x: Float, val y: Float, val w: Float, val h: Float, val cls: Int, val score: Float)

    fun detect(bmp: Bitmap): List<Box> {
        val scale = min(SIZE / bmp.width.toFloat(), SIZE / bmp.height.toFloat())
        val w = (bmp.width * scale).roundToInt()
        val h = (bmp.height * scale).roundToInt()
        val padX = (SIZE - w) / 2
        val padY = (SIZE - h) / 2
        val small = Bitmap.createScaledBitmap(bmp, w, h, false)
        val px = IntArray(w * h)
        small.getPixels(px, 0, w, 0, 0, w, h)
        val n = SIZE * SIZE
        val input = FloatBuffer.allocate(3 * n)
        for (i in 0 until 3 * n) input.put(i, 114f / 255f)
        for (y in 0 until h) {
            for (x in 0 until w) {
                val c = px[y * w + x]
                val d = (y + padY) * SIZE + x + padX
                input.put(d, ((c shr 16) and 255) / 255f)
                input.put(n + d, ((c shr 8) and 255) / 255f)
                input.put(2 * n + d, (c and 255) / 255f)
            }
        }
        val boxes = ArrayList<Box>()
        OnnxTensor.createTensor(env, input, longArrayOf(1, 3, SIZE.toLong(), SIZE.toLong())).use { tensor ->
            session.run(mapOf(session.inputNames.first() to tensor)).use { result ->
                @Suppress("UNCHECKED_CAST")
                val out = (result[0].value as Array<Array<FloatArray>>)[0]
                val channels = out.size
                if (channels < 5) return emptyList()
                for (i in out[0].indices) {
                    var cls = 0
                    for (c in 5 until channels) if (out[c][i] > out[4 + cls][i]) cls = c - 4
                    val score = out[4 + cls][i]
                    if (score < MIN_SCORE) continue
                    val bw = out[2][i] / scale
                    val bh = out[3][i] / scale
                    boxes.add(Box((out[0][i] - padX) / scale - bw / 2, (out[1][i] - padY) / scale - bh / 2, bw, bh, cls, score))
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
        const val MIN_SCORE = 0.35f
        const val MAX_IOU = 0.45f
        const val FILE = "balloons.onnx"

        @Volatile private var loaded: Detector? = null

        /** The model, or null while no model file is on the device (`detector_missing`). */
        fun get(ctx: Context): Detector? {
            loaded?.let { return it }
            synchronized(this) {
                loaded?.let { return it }
                val bytes = modelBytes(ctx) ?: return null
                val env = OrtEnvironment.getEnvironment()
                return Detector(env, env.createSession(bytes, OrtSession.SessionOptions())).also { loaded = it }
            }
        }

        private fun modelBytes(ctx: Context): ByteArray? {
            for (dir in listOf(ctx.dataDir, ctx.filesDir)) {
                val f = File(dir, "models/$FILE")
                if (f.isFile) return f.readBytes()
            }
            return runCatching { ctx.assets.open(FILE).use { it.readBytes() } }.getOrNull()
        }
    }
}
