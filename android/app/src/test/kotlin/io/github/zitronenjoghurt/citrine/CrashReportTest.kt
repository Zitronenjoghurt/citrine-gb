package io.github.zitronenjoghurt.citrine

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CrashReportTest {
    private val rust = listOf(
        "thread main: panicked at crates/citrine-gb-app/src/app.rs:212:14:",
        "called `Option::unwrap()` on a `None` value",
        "",
        "   0: std::backtrace::Backtrace::force_capture",
        "             at /rustc/abc/library/std/src/backtrace.rs:331:13",
        "   1: citrine_gb_app::platform::android::install_crash_hook::{{closure}}",
        "   2: citrine_gb_app::autosave::install_panic_hook::{{closure}}",
        "   3: std::panicking::rust_panic_with_hook",
        "   4: core::option::unwrap_failed",
        "   5: citrine_gb_app::app::Citrine::new",
        "             at ./crates/citrine-gb-app/src/app.rs:212:14",
        "   6: eframe::native::run::run_native",
    ).joinToString("\n")

    private val java = listOf(
        "Thread main",
        "",
        "java.lang.IllegalStateException: no surface",
        "\tat io.github.zitronenjoghurt.citrine.MainActivity.keyboard(MainActivity.kt:120)",
        "\tat a.b.c(d.kt:1)",
        "\tat a.b.c(d.kt:2)",
        "\tat a.b.c(d.kt:3)",
        "\tat a.b.c(d.kt:4)",
        "\tat a.b.c(d.kt:5)",
        "\tat a.b.c(d.kt:6)",
        "Caused by: java.io.IOException: gone",
        "\tat x.y.z(w.kt:9)",
    ).joinToString("\n")

    private val logcat = (1..500).joinToString("\n") { "11-02 01:57:00.000 I citrine: line $it" }

    private val report = listOf(
        "samsung SM-S938B, Android 17 (SDK 37), Citrine 0.8.0 (800)",
        "== crash-rust.txt\n$rust",
        "== crash-java.txt\n$java",
        "== exit\nnative crash at 2026-11-02 01:57:00, status 6, process io.github.zitronenjoghurt.citrine\nAbort message",
        "== logcat\n$logcat",
    ).joinToString("\n\n")

    @Test
    fun summary_puts_the_cause_first() {
        val lines = CrashReport.summary(report).lines()
        assertEquals("samsung SM-S938B, Android 17 (SDK 37), Citrine 0.8.0 (800)", lines.first())
        assertTrue(lines.contains("thread main: panicked at crates/citrine-gb-app/src/app.rs:212:14:"))
        assertTrue(lines.contains("called `Option::unwrap()` on a `None` value"))
        assertTrue(lines.contains("  citrine_gb_app::app::Citrine::new"))
        assertTrue(lines.contains("    at ./crates/citrine-gb-app/src/app.rs:212:14"))
        assertTrue(lines.contains("java.lang.IllegalStateException: no surface"))
        assertTrue(lines.contains("Caused by: java.io.IOException: gone"))
        assertTrue(lines.any { it.startsWith("native crash at") })
    }

    @Test
    fun summary_leaves_out_hooks_std_frames_and_the_log() {
        val summary = CrashReport.summary(report)
        assertFalse(summary.contains("crash_hook"))
        assertFalse(summary.contains("panic_hook"))
        assertFalse(summary.contains("rust_panic_with_hook"))
        assertFalse(summary.contains("citrine: line"))
        assertFalse(summary.contains("d.kt:6"))
        assertTrue(summary.lines().size < 30)
    }

    @Test
    fun summary_shows_raw_frames_when_none_name_citrine() {
        val bare = "device\n\n== crash-rust.txt\nthread main: panicked at src/x.rs:1:1:\nboom\n\n   0: <unknown>\n   1: <unknown>"
        val lines = CrashReport.summary(bare).lines()
        assertTrue(lines.contains("boom"))
        assertTrue(lines.contains("   0: <unknown>"))
    }

    @Test
    fun summary_keeps_a_stop_message_without_a_trace() {
        val stopped = "device\n\n== crash-rust.txt\nCitrine stopped: EventLoop can't be recreated"
        assertTrue(CrashReport.summary(stopped).contains("Citrine stopped: EventLoop can't be recreated"))
    }
}
