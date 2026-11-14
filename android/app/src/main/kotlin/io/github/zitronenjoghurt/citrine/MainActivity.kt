package io.github.zitronenjoghurt.citrine

import android.Manifest
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.Signature
import android.os.Build
import android.hardware.input.InputManager
import android.os.Bundle
import android.os.Process
import android.provider.Settings
import android.text.InputType
import android.view.HapticFeedbackConstants
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.Surface
import android.view.SurfaceHolder
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.core.content.FileProvider
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import com.google.androidgamesdk.GameActivity
import com.google.androidgamesdk.gametextinput.State
import java.io.File

class MainActivity : GameActivity(), InputManager.InputDeviceListener {
    private val documents = Documents(this)
    private val pads = Pads()
    private var typing = false
    private val senses by lazy {
        Senses(this, { x, y, z, turn -> nativeMotion(x, y, z, turn) }, ::nativeLight)
    }
    private val camera by lazy {
        CameraFeed(this, { senses.rotation() }, ::nativeCameraFrame, ::nativeCameraDenied)
    }
    private val stepCounter by lazy { StepCounter(this, ::nativeSteps, ::nativeStepsDenied) }
    private val second by lazy {
        SecondDisplay(this, ::nativeSecondDisplay, ::nativeSecondSurface, ::nativeSecondTouch)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        setImeEditorInfoFields(
            InputType.TYPE_CLASS_TEXT,
            EditorInfo.IME_ACTION_NONE,
            EditorInfo.IME_FLAG_NO_FULLSCREEN or EditorInfo.IME_FLAG_NO_EXTRACT_UI,
        )
        super.onCreate(savedInstanceState)
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() = nativeBack()
        })
        val input = getSystemService(InputManager::class.java)
        input.registerInputDeviceListener(this, null)
        InputDevice.getDeviceIds().forEach(::onInputDeviceAdded)
        second.start()
        PushService.onToken = ::nativePushToken
        PushService.fetchToken()
        opened(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        opened(intent)
    }

    private fun opened(intent: Intent?) {
        val kind = intent?.getStringExtra(Notifications.EXTRA_KIND) ?: return
        nativeOpen(
            kind,
            intent.getStringExtra(Notifications.EXTRA_FROM).orEmpty(),
            intent.getStringExtra(Notifications.EXTRA_SESSION).orEmpty(),
        )
        intent.removeExtra(Notifications.EXTRA_KIND)
    }

    private fun tellPush() = nativePush(PushService.AVAILABLE, PushService.AVAILABLE && Notifications.allowed(this))

    fun askNotifications() = runOnUiThread {
        if (Notifications.allowed(this)) {
            tellPush()
            return@runOnUiThread
        }
        val blocked = getSharedPreferences(PUSH_PREFS, MODE_PRIVATE).getBoolean(BLOCKED, false)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && !Notifications.permitted(this) && !blocked) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), Notifications.PERMISSION_REQUEST)
        } else {
            openNotificationSettings()
        }
    }

    fun notificationSettings() = runOnUiThread { openNotificationSettings() }

    private fun openNotificationSettings() {
        startActivity(
            Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, packageName),
        )
    }

    override fun onDestroy() {
        PushService.onToken = null
        getSystemService(InputManager::class.java).unregisterInputDeviceListener(this)
        second.stop()
        super.onDestroy()
        if (!isChangingConfigurations) Process.killProcess(Process.myPid())
    }

    override fun onApplyWindowInsets(v: View, insets: WindowInsetsCompat): WindowInsetsCompat {
        val safe = insets.getInsets(
            WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout(),
        )
        val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
        nativeSafeArea(safe.left, safe.top, safe.right, safe.bottom, ime.bottom)
        return super.onApplyWindowInsets(v, insets)
    }

    override fun onPause() {
        super.onPause()
        inFront = false
        senses.pause()
        camera.pause()
        senses.rumble(0f)
        nativePause()
    }

    override fun onResume() {
        super.onResume()
        inFront = true
        tellPush()
        senses.resume()
        camera.resume()
    }

    override fun onRequestPermissionsResult(code: Int, permissions: Array<out String>, results: IntArray) {
        super.onRequestPermissionsResult(code, permissions, results)
        if (code == CameraFeed.PERMISSION_REQUEST) {
            camera.permissionAnswered(results.firstOrNull() == PackageManager.PERMISSION_GRANTED)
        }
        if (code == StepCounter.PERMISSION_REQUEST) {
            stepCounter.permissionAnswered(results.firstOrNull() == PackageManager.PERMISSION_GRANTED)
        }
        if (code == Notifications.PERMISSION_REQUEST) {
            val granted = results.firstOrNull() == PackageManager.PERMISSION_GRANTED
            if (!granted && !shouldShowRequestPermissionRationale(Manifest.permission.POST_NOTIFICATIONS)) {
                getSharedPreferences(PUSH_PREFS, MODE_PRIVATE).edit().putBoolean(BLOCKED, true).apply()
            }
            tellPush()
        }
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        nativeVisible(true)
        super.surfaceCreated(holder)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        nativeVisible(false)
        super.surfaceDestroyed(holder)
    }

    override fun stateChanged(newState: State, dismissed: Boolean) {
        nativeText(newState.text, newState.selectionEnd)
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        val device = event.device
        if (pads.isPad(device) && !event.isSystem) {
            if (event.action == KeyEvent.ACTION_DOWN || event.action == KeyEvent.ACTION_UP) {
                nativePadKey(
                    event.deviceId,
                    event.keyCode,
                    event.action == KeyEvent.ACTION_DOWN,
                    event.repeatCount > 0,
                )
            }
            return true
        }
        if (device == null || device.isVirtual || event.isSystem) {
            return super.dispatchKeyEvent(event)
        }
        if (event.keyCode == KeyEvent.KEYCODE_V && event.isCtrlPressed) {
            if (event.action == KeyEvent.ACTION_DOWN) paste()
            return true
        }
        return when (event.action) {
            KeyEvent.ACTION_DOWN -> onKeyDown(event.keyCode, event)
            KeyEvent.ACTION_UP -> onKeyUp(event.keyCode, event)
            else -> super.dispatchKeyEvent(event)
        }
    }

    override fun dispatchGenericMotionEvent(event: MotionEvent): Boolean {
        if (event.action == MotionEvent.ACTION_MOVE && pads.isPad(event.device)) {
            nativePadAxes(event.deviceId, pads.axes(event))
            return true
        }
        return super.dispatchGenericMotionEvent(event)
    }

    override fun onInputDeviceAdded(deviceId: Int) {
        val device = InputDevice.getDevice(deviceId) ?: return
        if (!pads.isPad(device)) return
        nativePadAdded(deviceId, device.name, device.vendorId, device.productId, device.descriptor)
    }

    override fun onInputDeviceRemoved(deviceId: Int) = nativePadRemoved(deviceId)

    override fun onInputDeviceChanged(deviceId: Int) = onInputDeviceAdded(deviceId)

    private fun paste() {
        val clip = getSystemService(ClipboardManager::class.java).primaryClip ?: return
        if (clip.itemCount == 0) return
        val text = clip.getItemAt(0).coerceToText(this)?.toString() ?: return
        if (text.isNotEmpty()) nativePaste(text)
    }

    fun keyboard(show: Boolean, seed: String) = runOnUiThread {
        val view = mSurfaceView ?: return@runOnUiThread
        val manager = getSystemService(InputMethodManager::class.java)
        typing = show
        if (show) {
            setTextInputState(State(seed, seed.length, seed.length, -1, -1))
            view.requestFocus()
            manager.showSoftInput(view, 0)
        } else {
            manager.hideSoftInputFromWindow(view.windowToken, 0)
        }
    }

    fun setText(seed: String) = runOnUiThread {
        if (typing) setTextInputState(State(seed, seed.length, seed.length, -1, -1))
    }

    fun leave() = runOnUiThread { moveTaskToBack(true) }

    fun lightBars(light: Boolean) = runOnUiThread {
        WindowCompat.getInsetsController(window, window.decorView).apply {
            isAppearanceLightStatusBars = light
            isAppearanceLightNavigationBars = light
        }
    }

    fun immersive(on: Boolean) = runOnUiThread {
        WindowCompat.getInsetsController(window, window.decorView).apply {
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            if (on) hide(WindowInsetsCompat.Type.systemBars()) else show(WindowInsetsCompat.Type.systemBars())
        }
    }

    fun haptic() = runOnUiThread {
        if (!window.decorView.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)) {
            Haptics.tick(this)
        }
    }

    fun senses(motion: Boolean, light: Boolean, wantCamera: Boolean) = runOnUiThread {
        senses.follow(motion, light)
        camera.follow(wantCamera)
    }

    fun rumble(strength: Float) = runOnUiThread { senses.rumble(strength) }

    fun steps(on: Boolean, ask: Boolean) = runOnUiThread { stepCounter.follow(on, ask) }

    fun openSecondDisplay() = runOnUiThread { second.open() }

    fun closeSecondDisplay() = runOnUiThread { second.close() }

    fun copyText(text: String) = runOnUiThread {
        getSystemService(ClipboardManager::class.java)
            .setPrimaryClip(ClipData.newPlainText("Citrine", text))
    }

    fun shareFile(path: String, mime: String) = runOnUiThread {
        val uri = FileProvider.getUriForFile(this, "$packageName.files", File(path))
        val send = Intent(Intent.ACTION_SEND).apply {
            type = mime
            putExtra(Intent.EXTRA_STREAM, uri)
            clipData = ClipData.newRawUri(null, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        startActivity(Intent.createChooser(send, null))
    }

    fun installUpdate(path: String): Int {
        val file = File(path)
        val theirs = signatures(file) ?: return 2
        val ours = signatures(null) ?: return 2
        if (theirs != ours) return 1
        val uri = FileProvider.getUriForFile(this, "$packageName.files", file)
        val install = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        runOnUiThread { startActivity(install) }
        return 0
    }

    @Suppress("DEPRECATION")
    private fun signatures(apk: File?): Set<String>? {
        val flags = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            PackageManager.GET_SIGNING_CERTIFICATES
        } else {
            PackageManager.GET_SIGNATURES
        }
        val info = try {
            if (apk == null) {
                packageManager.getPackageInfo(packageName, flags)
            } else {
                packageManager.getPackageArchiveInfo(apk.path, flags)
            }
        } catch (_: PackageManager.NameNotFoundException) {
            null
        } ?: return null
        val signed: Array<Signature>? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            info.signingInfo?.apkContentsSigners
        } else {
            info.signatures
        }
        return signed?.map { it.toCharsString() }?.toSet()?.takeIf { it.isNotEmpty() }
    }

    fun openDocuments(id: Int, multiple: Boolean) = runOnUiThread { documents.open(id, multiple) }

    fun saveDocuments(id: Int, names: Array<String>, data: Array<ByteArray>) =
        runOnUiThread { documents.save(id, names.zip(data)) }

    fun openTree(id: Int) = runOnUiThread { documents.openTree(id) }

    fun copyIntoTree(tree: String, dir: String) = documents.copyIntoTree(tree, File(dir))

    fun listRoms(id: Int, tree: String) = documents.listRoms(id, tree)

    fun readRom(id: Int, uri: String) = documents.readRom(id, uri)

    external fun nativeOpened(id: Int, names: Array<String>, data: Array<ByteArray>)

    external fun nativeSaved(id: Int, names: Array<String>, error: String?)

    external fun nativeTree(id: Int, tree: String?)

    external fun nativeRoms(id: Int, names: Array<String>, uris: Array<String>, readable: Boolean)

    private external fun nativeSafeArea(left: Int, top: Int, right: Int, bottom: Int, ime: Int)

    private external fun nativePause()

    private external fun nativeVisible(visible: Boolean)

    private external fun nativeBack()

    private external fun nativeText(text: String, cursor: Int)

    private external fun nativePaste(text: String)

    private external fun nativePadAdded(id: Int, name: String, vendor: Int, product: Int, descriptor: String)

    private external fun nativePadRemoved(id: Int)

    private external fun nativePadKey(id: Int, code: Int, down: Boolean, repeat: Boolean)

    private external fun nativePadAxes(id: Int, axes: FloatArray)

    private external fun nativeMotion(x: Float, y: Float, z: Float, turn: Float)

    private external fun nativeLight(lux: Float)

    private external fun nativeSteps(count: Long, boot: Long)

    private external fun nativeStepsDenied()

    private external fun nativeCameraFrame(width: Int, height: Int, luma: ByteArray, mirror: Boolean)

    private external fun nativeCameraDenied()

    private external fun nativeSecondDisplay(name: String?)

    private external fun nativeSecondSurface(surface: Surface?, width: Int, height: Int, density: Float)

    private external fun nativeSecondTouch(action: Int, x: Float, y: Float)

    private external fun nativePush(available: Boolean, allowed: Boolean)

    private external fun nativePushToken(token: String)

    private external fun nativeOpen(kind: String, from: String, session: String)

    companion object {
        private const val PUSH_PREFS = "push"
        private const val BLOCKED = "blocked"

        @Volatile
        var inFront = false
    }
}
