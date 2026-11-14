package io.github.zitronenjoghurt.citrine

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class NotificationsTest {
    @Test
    fun eachKindHasItsOwnChannel() {
        val request = Alert.from(mapOf("kind" to "friend_request", "from" to "AB12-CD34", "name" to "Ana"))!!
        val invite = Alert.from(mapOf("kind" to "invite", "from" to "AB12-CD34", "name" to "Ana", "session" to "99"))!!
        val chat = Alert.from(mapOf("kind" to "chat", "from" to "AB12-CD34", "name" to "Ana", "count" to "1"))!!
        assertEquals(Notifications.FRIENDS, request.channel)
        assertEquals(Notifications.INVITES, invite.channel)
        assertEquals(Notifications.MESSAGES, chat.channel)
        assertEquals("Sent you a friend request", request.text)
        assertEquals("Invited you to play", invite.text)
        assertEquals("New message", chat.text)
    }

    @Test
    fun aChatUpdatesOneNotificationPerFriend() {
        val first = Alert.from(mapOf("kind" to "chat", "from" to "AB12-CD34", "name" to "Ana", "count" to "1"))!!
        val third = Alert.from(mapOf("kind" to "chat", "from" to "AB12-CD34", "name" to "Ana", "count" to "3"))!!
        val other = Alert.from(mapOf("kind" to "chat", "from" to "EF56-GH78", "name" to "Ben", "count" to "1"))!!
        assertEquals(first.tag, third.tag)
        assertEquals("3 new messages", third.text)
        assertEquals(false, first.tag == other.tag)
    }

    @Test
    fun aBrokenMessageShowsNothing() {
        assertNull(Alert.from(mapOf("kind" to "news", "from" to "AB12-CD34")))
        assertNull(Alert.from(mapOf("kind" to "chat")))
        assertNull(Alert.from(mapOf("kind" to "invite", "from" to "AB12-CD34")))
        assertEquals("A friend", Alert.from(mapOf("kind" to "chat", "from" to "AB12-CD34"))!!.name)
    }
}
