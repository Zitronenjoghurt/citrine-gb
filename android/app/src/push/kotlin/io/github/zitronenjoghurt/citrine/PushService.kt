package io.github.zitronenjoghurt.citrine

import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

object PushService {
    const val AVAILABLE = true

    @Volatile
    var onToken: ((String) -> Unit)? = null

    fun fetchToken() {
        runCatching {
            FirebaseMessaging.getInstance().token.addOnSuccessListener { token -> onToken?.invoke(token) }
        }
    }
}

class CitrineMessaging : FirebaseMessagingService() {
    override fun onNewToken(token: String) {
        PushService.onToken?.invoke(token)
    }

    override fun onMessageReceived(message: RemoteMessage) {
        if (MainActivity.inFront) return
        val alert = Alert.from(message.data) ?: return
        Notifications.show(this, alert)
    }
}
