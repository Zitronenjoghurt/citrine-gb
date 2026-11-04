package io.github.zitronenjoghurt.citrine

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.graphics.ImageFormat
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import android.util.Size
import android.view.Surface

class CameraFeed(
    private val activity: Activity,
    private val rotation: () -> Int,
    private val onFrame: (Int, Int, ByteArray, Boolean) -> Unit,
    private val onDenied: () -> Unit,
) {
    private val manager = activity.getSystemService(CameraManager::class.java)
    private var thread: HandlerThread? = null
    private var device: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var reader: ImageReader? = null
    private var wanted = false

    fun follow(on: Boolean) {
        if (on == wanted) return
        wanted = on
        if (on) start() else stop()
    }

    fun pause() = stop()

    fun resume() {
        if (wanted) start()
    }

    fun permissionAnswered(granted: Boolean) {
        if (!wanted) return
        if (granted) start() else onDenied()
    }

    private fun start() {
        if (device != null) return
        if (activity.checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            activity.requestPermissions(arrayOf(Manifest.permission.CAMERA), PERMISSION_REQUEST)
            return
        }
        val id = pickCamera() ?: return onDenied()
        val traits = manager.getCameraCharacteristics(id)
        val front = traits.get(CameraCharacteristics.LENS_FACING) == CameraCharacteristics.LENS_FACING_FRONT
        val sensorTurn = traits.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0
        val size = smallSize(traits)
        val worker = HandlerThread("camera").also { it.start() }
        val handler = Handler(worker.looper)
        thread = worker
        val images = ImageReader.newInstance(size.width, size.height, ImageFormat.YUV_420_888, 2)
        reader = images
        images.setOnImageAvailableListener({ source ->
            val image = source.acquireLatestImage() ?: return@setOnImageAvailableListener
            image.use {
                val plane = it.planes[0]
                val luma = packLuma(plane.buffer, plane.rowStride, plane.pixelStride, it.width, it.height)
                val turn = turnDegrees(sensorTurn, front)
                val (width, height, upright) = rotate(luma, it.width, it.height, turn)
                onFrame(width, height, upright, front)
            }
        }, handler)
        try {
            manager.openCamera(id, object : CameraDevice.StateCallback() {
                override fun onOpened(camera: CameraDevice) {
                    device = camera
                    if (!wanted) return stop()
                    capture(camera, images.surface, handler)
                }

                override fun onDisconnected(camera: CameraDevice) = stop()

                override fun onError(camera: CameraDevice, error: Int) = stop()
            }, handler)
        } catch (error: SecurityException) {
            stop()
            onDenied()
        }
    }

    @Suppress("DEPRECATION")
    private fun capture(camera: CameraDevice, surface: Surface, handler: Handler) {
        camera.createCaptureSession(listOf(surface), object : CameraCaptureSession.StateCallback() {
            override fun onConfigured(created: CameraCaptureSession) {
                session = created
                val request = camera.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW).apply {
                    addTarget(surface)
                    set(CaptureRequest.CONTROL_MODE, CaptureRequest.CONTROL_MODE_AUTO)
                }
                created.setRepeatingRequest(request.build(), null, handler)
            }

            override fun onConfigureFailed(created: CameraCaptureSession) = stop()
        }, handler)
    }

    private fun stop() {
        session?.close()
        session = null
        device?.close()
        device = null
        reader?.close()
        reader = null
        thread?.quitSafely()
        thread = null
    }

    private fun pickCamera(): String? {
        val ids = manager.cameraIdList
        return ids.firstOrNull {
            manager.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) ==
                CameraCharacteristics.LENS_FACING_FRONT
        } ?: ids.firstOrNull()
    }

    private fun smallSize(traits: CameraCharacteristics): Size {
        val map = traits.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
        val sizes = map?.getOutputSizes(ImageFormat.YUV_420_888).orEmpty()
        return sizes.filter { it.width >= 160 && it.height >= 144 }
            .minByOrNull { it.width * it.height }
            ?: sizes.firstOrNull()
            ?: Size(320, 240)
    }

    private fun turnDegrees(sensorTurn: Int, front: Boolean): Int {
        val screen = when (rotation()) {
            Surface.ROTATION_90 -> 90
            Surface.ROTATION_180 -> 180
            Surface.ROTATION_270 -> 270
            else -> 0
        }
        return if (front) (sensorTurn + screen) % 360 else (sensorTurn - screen + 360) % 360
    }

    companion object {
        const val PERMISSION_REQUEST = 7301

        private fun packLuma(
            buffer: java.nio.ByteBuffer,
            rowStride: Int,
            pixelStride: Int,
            width: Int,
            height: Int,
        ): ByteArray {
            val out = ByteArray(width * height)
            for (y in 0 until height) {
                for (x in 0 until width) {
                    out[y * width + x] = buffer.get(y * rowStride + x * pixelStride)
                }
            }
            return out
        }

        fun rotate(luma: ByteArray, width: Int, height: Int, degrees: Int): Triple<Int, Int, ByteArray> {
            if (degrees == 0) return Triple(width, height, luma)
            val out = ByteArray(luma.size)
            return when (degrees) {
                90 -> {
                    for (y in 0 until height) for (x in 0 until width) {
                        out[x * height + (height - 1 - y)] = luma[y * width + x]
                    }
                    Triple(height, width, out)
                }
                180 -> {
                    for (i in luma.indices) out[luma.size - 1 - i] = luma[i]
                    Triple(width, height, out)
                }
                else -> {
                    for (y in 0 until height) for (x in 0 until width) {
                        out[(width - 1 - x) * height + y] = luma[y * width + x]
                    }
                    Triple(height, width, out)
                }
            }
        }
    }
}
