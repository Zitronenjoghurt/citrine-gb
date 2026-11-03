package io.github.zitronenjoghurt.citrine

import android.view.InputDevice
import android.view.MotionEvent

class Pads {
    fun isPad(device: InputDevice?): Boolean {
        if (device == null || device.isVirtual) return false
        val sources = device.sources
        return sources and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD ||
            sources and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK
    }

    fun axes(event: MotionEvent): FloatArray = floatArrayOf(
        event.getAxisValue(MotionEvent.AXIS_X),
        event.getAxisValue(MotionEvent.AXIS_Y),
        event.getAxisValue(MotionEvent.AXIS_Z),
        event.getAxisValue(MotionEvent.AXIS_RZ),
        maxOf(event.getAxisValue(MotionEvent.AXIS_LTRIGGER), event.getAxisValue(MotionEvent.AXIS_BRAKE)),
        maxOf(event.getAxisValue(MotionEvent.AXIS_RTRIGGER), event.getAxisValue(MotionEvent.AXIS_GAS)),
        event.getAxisValue(MotionEvent.AXIS_HAT_X),
        event.getAxisValue(MotionEvent.AXIS_HAT_Y),
    )
}
