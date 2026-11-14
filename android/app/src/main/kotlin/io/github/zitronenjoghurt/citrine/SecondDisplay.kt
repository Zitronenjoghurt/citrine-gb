package io.github.zitronenjoghurt.citrine

import android.app.Activity
import android.app.Presentation
import android.hardware.display.DisplayManager
import android.view.Display
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView

class SecondDisplay(
    private val activity: Activity,
    private val onDisplay: (String?) -> Unit,
    private val onSurface: (Surface?, Int, Int, Float) -> Unit,
    private val onTouch: (Int, Float, Float) -> Unit,
) : DisplayManager.DisplayListener {
    private val displays = activity.getSystemService(DisplayManager::class.java)
    private var shown: Presentation? = null

    fun start() {
        displays.registerDisplayListener(this, null)
        report()
    }

    fun stop() {
        displays.unregisterDisplayListener(this)
        close()
    }

    private fun target(): Display? =
        displays.getDisplays(DisplayManager.DISPLAY_CATEGORY_PRESENTATION).firstOrNull()

    private fun report() = onDisplay(target()?.name)

    fun open() {
        if (shown != null) return
        val display = target() ?: return
        val density = activity.createDisplayContext(display).resources.displayMetrics.density
        val view = SurfaceView(activity)
        view.holder.addCallback(object : SurfaceHolder.Callback {
            override fun surfaceCreated(holder: SurfaceHolder) {
                val frame = holder.surfaceFrame
                onSurface(holder.surface, frame.width(), frame.height(), density)
            }

            override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) =
                onSurface(holder.surface, width, height, density)

            override fun surfaceDestroyed(holder: SurfaceHolder) = onSurface(null, 0, 0, density)
        })
        view.setOnTouchListener { _, event ->
            onTouch(event.actionMasked, event.x, event.y)
            true
        }
        val presentation = Presentation(activity, display)
        presentation.setContentView(view)
        presentation.setOnDismissListener {
            shown = null
            onSurface(null, 0, 0, density)
        }
        try {
            presentation.show()
            shown = presentation
        } catch (_: RuntimeException) {
            shown = null
        }
    }

    fun close() {
        shown?.dismiss()
        shown = null
    }

    override fun onDisplayAdded(displayId: Int) = report()

    override fun onDisplayRemoved(displayId: Int) {
        if (shown?.display?.displayId == displayId) close()
        report()
    }

    override fun onDisplayChanged(displayId: Int) = report()
}
