package io.github.zitronenjoghurt.citrine

import android.app.Application
import java.io.File

class CitrineApp : Application() {
    override fun onCreate() {
        super.onCreate()
        if (PushService.AVAILABLE) Notifications.channels(this)
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, error ->
            runCatching {
                File(cacheDir, CrashReport.JAVA_FILE)
                    .writeText("Thread ${thread.name}\n\n${error.stackTraceToString()}")
            }
            previous?.uncaughtException(thread, error)
        }
    }
}
