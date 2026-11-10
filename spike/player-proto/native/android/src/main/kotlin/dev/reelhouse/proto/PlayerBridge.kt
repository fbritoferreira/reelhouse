package dev.reelhouse.proto

import android.app.Activity
import android.graphics.Color
import android.os.Handler
import android.os.Looper
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer

/**
 * ExoPlayer drawn in a SurfaceView under the Dioxus WebView.
 *
 * manganis constructs this with the current Activity. Calls arrive off the main thread, but ExoPlayer
 * must be used from its own looper, so commands are posted to main and getters read state that a
 * main-thread ticker copies into volatile fields every 100 ms.
 */
class PlayerBridge(private val activity: Activity) {
    private val main = Handler(Looper.getMainLooper())
    private var player: ExoPlayer? = null

    @Volatile private var positionS = 0.0
    @Volatile private var durationS = 0.0
    @Volatile private var playing = false
    @Volatile private var lastError: String? = null

    init {
        main.post { attach() }
    }

    private fun attach() {
        val webView = findWebView(activity.window.decorView) ?: run {
            lastError = "WebView not found"
            return
        }
        webView.setBackgroundColor(Color.TRANSPARENT)
        val parent = webView.parent as? ViewGroup ?: return
        val surface = SurfaceView(activity)
        parent.addView(surface, 0, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        val exo = ExoPlayer.Builder(activity).build()
        exo.setVideoSurfaceView(surface)
        exo.addListener(object : Player.Listener {
            override fun onPlayerError(error: PlaybackException) {
                lastError = "${error.errorCodeName}: ${error.message}"
            }
        })
        player = exo
        tick()
    }

    private fun tick() {
        val exo = player ?: return
        positionS = exo.currentPosition / 1000.0
        durationS = if (exo.duration > 0) exo.duration / 1000.0 else 0.0
        playing = exo.isPlaying
        main.postDelayed({ tick() }, 100)
    }

    private fun findWebView(view: View): WebView? {
        if (view is WebView) return view
        if (view is ViewGroup) {
            for (i in 0 until view.childCount) findWebView(view.getChildAt(i))?.let { return it }
        }
        return null
    }

    fun load(url: String) {
        main.post {
            lastError = null
            player?.setMediaItem(MediaItem.fromUri(url))
            player?.prepare()
        }
    }

    fun play() { main.post { player?.play() } }
    fun pause() { main.post { player?.pause() } }
    fun seek(seconds: Double) { main.post { player?.seekTo((seconds * 1000).toLong()) } }
    fun position(): Double = positionS
    fun duration(): Double = durationS
    fun isPlaying(): Boolean = playing
    fun error(): String? = lastError
}
