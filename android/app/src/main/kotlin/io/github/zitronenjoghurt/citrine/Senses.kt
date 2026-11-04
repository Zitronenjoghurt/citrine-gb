package io.github.zitronenjoghurt.citrine

import android.app.Activity
import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.view.InputDevice
import android.view.Surface

class Senses(
    private val activity: Activity,
    private val onMotion: (Float, Float, Float, Float) -> Unit,
    private val onLight: (Float) -> Unit,
) : SensorEventListener {
    private val manager = activity.getSystemService(SensorManager::class.java)
    private val gravity = FloatArray(3)
    private var turn = 0f
    private var motion = false
    private var light = false
    private var vibrating = 0f

    fun follow(wantMotion: Boolean, wantLight: Boolean) {
        if (wantMotion == motion && wantLight == light) return
        manager.unregisterListener(this)
        motion = wantMotion
        light = wantLight
        if (motion) {
            val tilt = manager.getDefaultSensor(Sensor.TYPE_GRAVITY)
                ?: manager.getDefaultSensor(Sensor.TYPE_ACCELEROMETER)
            tilt?.let { manager.registerListener(this, it, SensorManager.SENSOR_DELAY_GAME) }
            manager.getDefaultSensor(Sensor.TYPE_GYROSCOPE)
                ?.let { manager.registerListener(this, it, SensorManager.SENSOR_DELAY_GAME) }
        }
        if (light) {
            manager.getDefaultSensor(Sensor.TYPE_LIGHT)
                ?.let { manager.registerListener(this, it, SensorManager.SENSOR_DELAY_NORMAL) }
        }
    }

    fun pause() = manager.unregisterListener(this)

    fun resume() {
        val wanted = Pair(motion, light)
        motion = false
        light = false
        follow(wanted.first, wanted.second)
    }

    override fun onSensorChanged(event: SensorEvent) {
        when (event.sensor.type) {
            Sensor.TYPE_GRAVITY, Sensor.TYPE_ACCELEROMETER -> {
                val (x, y) = screenAxes(event.values[0], event.values[1])
                gravity[0] = x
                gravity[1] = y
                gravity[2] = event.values[2]
                onMotion(gravity[0], gravity[1], gravity[2], turn)
            }
            Sensor.TYPE_GYROSCOPE -> {
                turn = event.values[2]
                onMotion(gravity[0], gravity[1], gravity[2], turn)
            }
            Sensor.TYPE_LIGHT -> onLight(event.values[0])
        }
    }

    override fun onAccuracyChanged(sensor: Sensor, accuracy: Int) = Unit

    private fun screenAxes(x: Float, y: Float): Pair<Float, Float> =
        when (rotation()) {
            Surface.ROTATION_90 -> Pair(-y, x)
            Surface.ROTATION_180 -> Pair(-x, -y)
            Surface.ROTATION_270 -> Pair(y, -x)
            else -> Pair(x, y)
        }

    @Suppress("DEPRECATION")
    fun rotation(): Int =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            activity.display?.rotation ?: Surface.ROTATION_0
        } else {
            activity.windowManager.defaultDisplay.rotation
        }

    fun rumble(strength: Float) {
        if (strength == vibrating) return
        vibrating = strength
        vibrators(activity).forEach { vibrator ->
            if (strength <= 0f) {
                vibrator.cancel()
            } else {
                val amplitude = (strength * 255).toInt().coerceIn(1, 255)
                val level = if (vibrator.hasAmplitudeControl()) amplitude else VibrationEffect.DEFAULT_AMPLITUDE
                vibrator.vibrate(VibrationEffect.createOneShot(RUMBLE_MS, level))
            }
        }
    }

    companion object {
        private const val RUMBLE_MS = 60_000L

        @Suppress("DEPRECATION")
        fun vibrators(context: Context): List<Vibrator> {
            val phone = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                context.getSystemService(VibratorManager::class.java)?.defaultVibrator
            } else {
                context.getSystemService(Vibrator::class.java)
            }
            val pads = InputDevice.getDeviceIds().toList().mapNotNull { id ->
                val device = InputDevice.getDevice(id) ?: return@mapNotNull null
                if (device.sources and InputDevice.SOURCE_GAMEPAD != InputDevice.SOURCE_GAMEPAD) {
                    return@mapNotNull null
                }
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    device.vibratorManager.defaultVibrator
                } else {
                    device.vibrator
                }
            }
            return (listOfNotNull(phone) + pads).filter { it.hasVibrator() }
        }
    }
}
