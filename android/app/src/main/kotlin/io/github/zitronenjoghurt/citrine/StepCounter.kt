package io.github.zitronenjoghurt.citrine

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.os.Build
import android.os.SystemClock

class StepCounter(
    private val activity: Activity,
    private val onSteps: (Long, Long) -> Unit,
    private val onDenied: () -> Unit,
) : SensorEventListener {
    private val manager = activity.getSystemService(SensorManager::class.java)
    private var wanted = false
    private var listening = false
    private var asked = false

    fun follow(on: Boolean, ask: Boolean) {
        wanted = on
        if (!on) return stop()
        if (granted()) return start()
        if (ask && !asked && Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            asked = true
            val wanted = listOfNotNull(
                Manifest.permission.ACTIVITY_RECOGNITION,
                Manifest.permission.POST_NOTIFICATIONS.takeIf {
                    Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && !Notifications.permitted(activity)
                },
            )
            activity.requestPermissions(wanted.toTypedArray(), PERMISSION_REQUEST)
        }
    }

    fun permissionAnswered(granted: Boolean) {
        if (!granted) return onDenied()
        if (wanted) start()
        StepService.resume(activity)
    }

    private fun granted(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.Q ||
            activity.checkSelfPermission(Manifest.permission.ACTIVITY_RECOGNITION) == PackageManager.PERMISSION_GRANTED

    private fun start() {
        if (listening) return
        val sensor = manager.getDefaultSensor(Sensor.TYPE_STEP_COUNTER) ?: return
        listening = manager.registerListener(this, sensor, SensorManager.SENSOR_DELAY_NORMAL)
    }

    private fun stop() {
        if (listening) manager.unregisterListener(this)
        listening = false
    }

    override fun onSensorChanged(event: SensorEvent) {
        val boot = System.currentTimeMillis() - SystemClock.elapsedRealtime()
        onSteps(event.values[0].toLong(), boot / 1000)
    }

    override fun onAccuracyChanged(sensor: Sensor, accuracy: Int) = Unit

    companion object {
        const val PERMISSION_REQUEST = 7303
    }
}
