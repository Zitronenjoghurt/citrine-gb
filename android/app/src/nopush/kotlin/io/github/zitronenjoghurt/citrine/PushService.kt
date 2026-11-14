package io.github.zitronenjoghurt.citrine

object PushService {
    const val AVAILABLE = false

    @Volatile
    var onToken: ((String) -> Unit)? = null

    fun fetchToken() {}
}
