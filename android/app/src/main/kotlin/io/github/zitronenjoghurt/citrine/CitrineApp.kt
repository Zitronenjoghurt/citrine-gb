package io.github.zitronenjoghurt.citrine

import android.app.Application
import java.io.File

class CitrineApp : Application() {
    override fun onCreate() {
        super.onCreate()
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, error ->
            runCatching {
                File(filesDir, CrashReport.JAVA_FILE)
                    .writeText("Thread ${thread.name}\n\n${error.stackTraceToString()}")
            }
            previous?.uncaughtException(thread, error)
        }
    }
}
