package io.github.zitronenjoghurt.citrine

import android.app.Activity
import android.content.ClipData
import android.content.Intent
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Bundle
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import androidx.core.content.FileProvider
import androidx.core.view.WindowCompat
import java.io.File

class StartActivity : Activity() {
    private var report = ""
    private var summary = ""
    private var full = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val found = runCatching { CrashReport.find(this) }.getOrNull()
        if (found == null) {
            start()
            return
        }
        report = found
        summary = runCatching { CrashReport.summary(found) }.getOrDefault(found.take(4000))
        val body = show()
        Thread {
            val withLog = CrashReport.withLog(found)
            runOnUiThread {
                report = withLog
                if (full) body.text = withLog
            }
        }.start()
    }

    private fun start() {
        startActivity(Intent(this, MainActivity::class.java))
        finish()
    }

    private fun show(): TextView {
        val root = FrameLayout(this).apply {
            fitsSystemWindows = true
            setBackgroundColor(BASE)
        }
        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(20), dp(20), dp(20))
        }
        column.addView(label("Citrine closed last time", 22f, Typeface.DEFAULT_BOLD))
        column.addView(
            label("This is what went wrong. Share it so it can be fixed.", 15f, Typeface.DEFAULT).apply {
                setPadding(0, dp(6), 0, dp(14))
            },
        )
        val buttons = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        buttons.addView(button("Start Citrine", true) { start() })
        buttons.addView(button("Share", false) { share() })
        column.addView(buttons)
        val body = label(summary, 12f, Typeface.MONOSPACE).apply {
            setTextIsSelectable(true)
            setPadding(dp(12), dp(12), dp(12), dp(12))
        }
        lateinit var toggle: Button
        toggle = button("Show full report", false) {
            full = !full
            body.text = if (full) report else summary
            toggle.text = if (full) "Show short report" else "Show full report"
        }
        toggle.layoutParams = LinearLayout.LayoutParams(WRAP_CONTENT, dp(44)).apply { topMargin = dp(8) }
        column.addView(toggle)
        val scroll = ScrollView(this).apply {
            background = rounded(MANTLE)
            addView(body)
        }
        column.addView(
            scroll,
            LinearLayout.LayoutParams(MATCH_PARENT, 0, 1f).apply { topMargin = dp(14) },
        )
        root.addView(column, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        setContentView(root)
        WindowCompat.getInsetsController(window, window.decorView).apply {
            isAppearanceLightStatusBars = false
            isAppearanceLightNavigationBars = false
        }
        return body
    }

    private fun share() {
        val file = File(cacheDir, "crash/citrine-crash-report.txt")
        val attached = runCatching {
            file.parentFile?.mkdirs()
            file.writeText(CrashReport.trim(report))
            FileProvider.getUriForFile(this, "$packageName.files", file)
        }.getOrNull()
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_SUBJECT, "Citrine crash report")
            putExtra(Intent.EXTRA_TEXT, summary)
            if (attached != null) {
                putExtra(Intent.EXTRA_STREAM, attached)
                clipData = ClipData.newRawUri(null, attached)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
        }
        startActivity(Intent.createChooser(send, "Share the report"))
    }

    private fun label(text: String, size: Float, face: Typeface) = TextView(this).apply {
        this.text = text
        setTextColor(TEXT)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, size)
        typeface = face
    }

    private fun button(text: String, primary: Boolean, action: () -> Unit) = Button(this).apply {
        this.text = text
        isAllCaps = false
        stateListAnimator = null
        setTextColor(if (primary) CRUST else TEXT)
        background = rounded(if (primary) AMBER else SURFACE)
        setPadding(dp(16), 0, dp(16), 0)
        minWidth = 0
        minimumWidth = 0
        setOnClickListener { action() }
        layoutParams = LinearLayout.LayoutParams(WRAP_CONTENT, dp(44)).apply { marginEnd = dp(8) }
        gravity = Gravity.CENTER
        textAlignment = View.TEXT_ALIGNMENT_CENTER
    }

    private fun rounded(color: Int) = GradientDrawable().apply {
        setColor(color)
        cornerRadius = dp(8).toFloat()
    }

    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()

    private companion object {
        val BASE = Color.parseColor("#1E1E2E")
        val MANTLE = Color.parseColor("#181825")
        val SURFACE = Color.parseColor("#313244")
        val TEXT = Color.parseColor("#CDD6F4")
        val CRUST = Color.parseColor("#11111B")
        val AMBER = Color.parseColor("#F5AD33")
    }
}
