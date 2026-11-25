package io.github.zitronenjoghurt.citrine

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.os.Build
import android.os.IBinder
import android.os.SystemClock
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import java.io.File
import java.io.RandomAccessFile
import java.nio.ByteBuffer
import java.nio.ByteOrder

class StepService : Service(), SensorEventListener {
    private var listening = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val running = foreground()
        if (!running || !kept(this) || !granted(this)) {
            ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
            stopSelf()
            return START_NOT_STICKY
        }
        listen()
        return START_STICKY
    }

    override fun onDestroy() {
        if (listening) getSystemService(SensorManager::class.java).unregisterListener(this)
        listening = false
        super.onDestroy()
    }

    private fun foreground(): Boolean {
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "Pokéwalker steps", NotificationManager.IMPORTANCE_LOW),
        )
        val open = Intent(this, StartActivity::class.java)
            .putExtra(Notifications.EXTRA_KIND, KIND)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        val tap = PendingIntent.getActivity(
            this,
            NOTIFICATION,
            open,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val notification = NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setColor(LEMON)
            .setContentTitle("Pokéwalker")
            .setContentText("Counting your steps")
            .setOngoing(true)
            .setShowWhen(false)
            .setCategory(NotificationCompat.CATEGORY_STATUS)
            .setContentIntent(tap)
            .build()
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_HEALTH
        } else {
            0
        }
        return runCatching { ServiceCompat.startForeground(this, NOTIFICATION, notification, type) }
            .onFailure { Log.w(TAG, "could not count steps in the background", it) }
            .isSuccess
    }

    private fun listen() {
        if (listening) return
        val manager = getSystemService(SensorManager::class.java)
        val sensor = manager.getDefaultSensor(Sensor.TYPE_STEP_COUNTER) ?: return
        listening = manager.registerListener(this, sensor, SensorManager.SENSOR_DELAY_NORMAL, BATCH_MICROS)
    }

    override fun onSensorChanged(event: SensorEvent) {
        val now = System.currentTimeMillis()
        val elapsed = SystemClock.elapsedRealtimeNanos()
        val age = ((elapsed - event.timestamp) / 1_000_000).coerceIn(0, elapsed / 1_000_000)
        val boot = (now - elapsed / 1_000_000) / 1000
        runCatching { record((now - age) / 1000, event.values[0].toLong(), boot) }
            .onFailure { Log.w(TAG, "could not log steps", it) }
    }

    override fun onAccuracyChanged(sensor: Sensor, accuracy: Int) = Unit

    private fun record(at: Long, count: Long, boot: Long) {
        val file = File(filesDir, LOG)
        file.parentFile?.mkdirs()
        val bytes = ByteBuffer.allocate(RECORD).order(ByteOrder.LITTLE_ENDIAN)
            .putLong(at).putLong(count).putLong(boot).array()
        RandomAccessFile(file, "rw").use { log ->
            val end = log.length() - log.length() % RECORD
            var write = end
            if (end >= RECORD) {
                val last = ByteArray(RECORD)
                log.seek(end - RECORD)
                log.readFully(last)
                val previous = ByteBuffer.wrap(last).order(ByteOrder.LITTLE_ENDIAN)
                val lastAt = previous.getLong(0)
                val lastBoot = previous.getLong(16)
                if (lastAt / 60 == at / 60 && kotlin.math.abs(lastBoot - boot) <= SAME_BOOT_SECS) {
                    write = end - RECORD
                }
            }
            log.seek(write)
            log.write(bytes)
            log.setLength(write + RECORD)
        }
        if (file.length() > MAX_LOG_BYTES) trim(file)
    }

    private fun trim(file: File) {
        val bytes = file.readBytes()
        val keep = ((MAX_LOG_BYTES / 2) - (MAX_LOG_BYTES / 2) % RECORD).toInt()
        val start = (bytes.size - bytes.size % RECORD - keep).coerceAtLeast(0)
        val next = File(file.parentFile, "$LOG_NAME.next")
        next.writeBytes(bytes.copyOfRange(start, bytes.size - bytes.size % RECORD))
        if (!next.renameTo(file)) next.delete()
    }

    class Boot : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action in setOf(Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED)) {
                resume(context)
            }
        }
    }

    companion object {
        const val KIND = "walker"
        private const val TAG = "CitrineSteps"
        private const val CHANNEL = "walker_steps"
        private const val NOTIFICATION = 7304
        private const val PREFS = "walker"
        private const val KEEP = "keep"
        private const val LOG_NAME = "phone-steps.log"
        private const val LOG = "walker/$LOG_NAME"
        private const val RECORD = 24
        private const val MAX_LOG_BYTES = 8L * 1440 * RECORD
        private const val SAME_BOOT_SECS = 120L
        private const val BATCH_MICROS = 60_000_000
        private const val LEMON = 0xFFE5C33B.toInt()

        fun follow(context: Context, keep: Boolean) {
            if (kept(context) != keep) {
                context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(KEEP, keep).apply()
            }
            if (keep) resume(context) else context.stopService(Intent(context, StepService::class.java))
        }

        fun resume(context: Context) {
            if (!kept(context) || !granted(context)) return
            runCatching { context.startForegroundService(Intent(context, StepService::class.java)) }
                .onFailure { Log.w(TAG, "could not start counting steps", it) }
        }

        private fun kept(context: Context): Boolean =
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(KEEP, false)

        private fun granted(context: Context): Boolean =
            Build.VERSION.SDK_INT < Build.VERSION_CODES.Q ||
                context.checkSelfPermission(Manifest.permission.ACTIVITY_RECOGNITION) ==
                PackageManager.PERMISSION_GRANTED
    }
}
