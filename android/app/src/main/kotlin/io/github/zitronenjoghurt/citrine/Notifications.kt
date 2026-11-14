package io.github.zitronenjoghurt.citrine

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat

data class Alert(
    val kind: String,
    val from: String,
    val name: String,
    val count: Int,
    val session: String,
) {
    val channel: String
        get() = when (kind) {
            FRIEND_REQUEST -> Notifications.FRIENDS
            INVITE -> Notifications.INVITES
            else -> Notifications.MESSAGES
        }

    val text: String
        get() = when (kind) {
            FRIEND_REQUEST -> "Sent you a friend request"
            INVITE -> "Invited you to play"
            else -> if (count > 1) "$count new messages" else "New message"
        }

    val tag: String
        get() = "$kind:$from"

    companion object {
        const val FRIEND_REQUEST = "friend_request"
        const val INVITE = "invite"
        const val CHAT = "chat"

        fun from(data: Map<String, String>): Alert? {
            val kind = data["kind"]?.takeIf { it in setOf(FRIEND_REQUEST, INVITE, CHAT) } ?: return null
            val from = data["from"]?.takeIf { it.isNotEmpty() } ?: return null
            val session = data["session"].orEmpty()
            if (kind == INVITE && session.isEmpty()) return null
            return Alert(
                kind = kind,
                from = from,
                name = data["name"].orEmpty().ifEmpty { "A friend" },
                count = data["count"]?.toIntOrNull()?.coerceAtLeast(1) ?: 1,
                session = session,
            )
        }
    }
}

object Notifications {
    const val FRIENDS = "friend_requests"
    const val INVITES = "invites"
    const val MESSAGES = "messages"
    const val PERMISSION_REQUEST = 7302
    const val EXTRA_KIND = "citrine.kind"
    const val EXTRA_FROM = "citrine.from"
    const val EXTRA_SESSION = "citrine.session"
    private const val INVITE_MILLIS = 5 * 60 * 1000L
    private const val LEMON = 0xFFE5C33B.toInt()

    fun channels(context: Context) {
        context.getSystemService(NotificationManager::class.java).createNotificationChannels(
            listOf(
                NotificationChannel(FRIENDS, "Friend requests", NotificationManager.IMPORTANCE_DEFAULT),
                NotificationChannel(INVITES, "Play invites", NotificationManager.IMPORTANCE_HIGH),
                NotificationChannel(MESSAGES, "Messages", NotificationManager.IMPORTANCE_HIGH),
            ),
        )
    }

    fun permitted(context: Context): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED

    fun allowed(context: Context): Boolean =
        permitted(context) && NotificationManagerCompat.from(context).areNotificationsEnabled()

    fun show(context: Context, alert: Alert) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        val open = Intent(context, StartActivity::class.java)
            .putExtra(EXTRA_KIND, alert.kind)
            .putExtra(EXTRA_FROM, alert.from)
            .putExtra(EXTRA_SESSION, alert.session)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        val tap = PendingIntent.getActivity(
            context,
            alert.tag.hashCode(),
            open,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val builder = NotificationCompat.Builder(context, alert.channel)
            .setSmallIcon(R.drawable.ic_notification)
            .setColor(LEMON)
            .setContentTitle(alert.name)
            .setContentText(alert.text)
            .setAutoCancel(true)
            .setContentIntent(tap)
        when (alert.kind) {
            Alert.CHAT -> builder.setNumber(alert.count).setCategory(NotificationCompat.CATEGORY_MESSAGE)
            Alert.INVITE -> builder.setTimeoutAfter(INVITE_MILLIS).setCategory(NotificationCompat.CATEGORY_SOCIAL)
            else -> builder.setCategory(NotificationCompat.CATEGORY_SOCIAL)
        }
        NotificationManagerCompat.from(context).notify(alert.tag, 0, builder.build())
    }
}
