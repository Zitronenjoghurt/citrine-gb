package io.github.zitronenjoghurt.citrine

import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.content.Context
import android.os.Build
import androidx.core.content.pm.PackageInfoCompat
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

object CrashReport {
    const val JAVA_FILE = "crash-java.txt"
    private const val RUST_FILE = "crash-rust.txt"
    private const val PREFS = "crash-report"
    private const val SEEN = "seen"
    private const val MAX_CHARS = 100_000
    private val FRAME = Regex("""^\s*\d+: .*""")

    fun find(context: Context): String? {
        val parts = mutableListOf<String>()
        for (name in listOf(RUST_FILE, JAVA_FILE)) {
            val file = File(context.filesDir, name)
            if (file.exists()) {
                runCatching { parts += "== $name\n${file.readText()}" }
                file.delete()
            }
        }
        exitInfo(context)?.let { parts += it }
        if (parts.isEmpty()) return null
        return (listOf(device(context)) + parts).joinToString("\n\n")
    }

    fun withLog(report: String): String {
        val main = logcat("logcat", "-d", "-t", "600", "-v", "threadtime")
        val crash = logcat("logcat", "-d", "-b", "crash", "-t", "200")
        return trim("$report\n\n== logcat crash\n$crash\n\n== logcat\n$main")
    }

    fun summary(report: String): String {
        val sections = report.split("\n\n== ")
        val lines = mutableListOf(sections.first().lineSequence().first())
        for (section in sections.drop(1)) {
            val body = section.substringAfter('\n', "")
            lines += when (section.substringBefore('\n')) {
                RUST_FILE -> rustCause(body)
                JAVA_FILE -> javaCause(body)
                "exit" -> body.lines().take(3)
                else -> emptyList()
            }
        }
        return lines.joinToString("\n")
    }

    private fun rustCause(body: String): List<String> {
        val message = body.substringBefore("\n\n").lines().take(8)
        val trace = body.substringAfter("\n\n", "").lines()
        val frames = mutableListOf<String>()
        for ((index, line) in trace.withIndex()) {
            if (frames.size >= 12) break
            val name = line.trim().substringAfter(": ", "")
            if (!FRAME.matches(line) || "citrine" !in name || name.contains("crash_hook") || name.contains("panic_hook")) continue
            frames += "  $name"
            trace.getOrNull(index + 1)?.trim()?.takeIf { it.startsWith("at ") }?.let { frames += "    $it" }
        }
        if (frames.isEmpty()) frames += trace.filter { FRAME.matches(it) }.take(8)
        return listOf("") + message + frames
    }

    private fun javaCause(body: String): List<String> {
        val lines = body.lines().filter { it.isNotBlank() }
        val picked = mutableListOf("")
        var frames = 0
        for (line in lines) {
            when {
                line.startsWith("Thread ") && picked.size == 1 -> picked += line
                line.trimStart().startsWith("at ") -> if (frames++ < 5) picked += line
                line.startsWith("Caused by") -> {
                    picked += line
                    frames = 2
                }
                picked.size <= 2 -> picked += line
            }
        }
        return picked
    }

    fun trim(text: String): String =
        if (text.length <= MAX_CHARS) text else text.take(MAX_CHARS / 2) + "\n...\n" + text.takeLast(MAX_CHARS / 2)

    private fun device(context: Context): String {
        val info = context.packageManager.getPackageInfo(context.packageName, 0)
        val version = "${info.versionName} (${PackageInfoCompat.getLongVersionCode(info)})"
        return "${Build.MANUFACTURER} ${Build.MODEL}, Android ${Build.VERSION.RELEASE} " +
            "(SDK ${Build.VERSION.SDK_INT}), Citrine $version"
    }

    private fun exitInfo(context: Context): String? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return null
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val updated = context.packageManager.getPackageInfo(context.packageName, 0).lastUpdateTime
        val after = maxOf(prefs.getLong(SEEN, 0), updated)
        val manager = context.getSystemService(ActivityManager::class.java)
        val exits = runCatching { manager.getHistoricalProcessExitReasons(context.packageName, 0, 10) }
            .getOrDefault(emptyList())
        val newest = exits.firstOrNull() ?: return null
        prefs.edit().putLong(SEEN, maxOf(after, newest.timestamp)).apply()
        val crash = exits.firstOrNull {
            it.timestamp > after && it.reason in listOf(
                ApplicationExitInfo.REASON_CRASH,
                ApplicationExitInfo.REASON_CRASH_NATIVE,
                ApplicationExitInfo.REASON_ANR,
            )
        } ?: return null
        val reason = when (crash.reason) {
            ApplicationExitInfo.REASON_CRASH -> "crash"
            ApplicationExitInfo.REASON_CRASH_NATIVE -> "native crash"
            else -> "not responding"
        }
        val time = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.ROOT).format(Date(crash.timestamp))
        val lines = mutableListOf(
            "== exit",
            "$reason at $time, status ${crash.status}, process ${crash.processName}",
        )
        crash.description?.let { lines += it }
        if (crash.reason == ApplicationExitInfo.REASON_ANR) {
            runCatching {
                crash.traceInputStream?.bufferedReader()?.use { lines += it.readText().take(MAX_CHARS / 4) }
            }
        }
        return lines.joinToString("\n")
    }

    private fun logcat(vararg command: String): String = runCatching {
        val process = ProcessBuilder(*command).redirectErrorStream(true).start()
        process.inputStream.bufferedReader().use { it.readText() }.also { process.waitFor() }
    }.getOrElse { "could not read the log: $it" }
}
