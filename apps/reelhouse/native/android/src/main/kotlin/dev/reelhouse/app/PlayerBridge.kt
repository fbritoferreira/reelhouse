package dev.reelhouse.app

import android.app.Activity
import android.graphics.Color
import android.os.Handler
import android.os.Looper
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import android.widget.FrameLayout
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.Tracks
import androidx.media3.common.VideoSize
import androidx.media3.common.text.CueGroup
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.MediaSession
import androidx.media3.ui.AspectRatioFrameLayout
import androidx.media3.ui.SubtitleView
import org.json.JSONArray
import org.json.JSONObject

/**
 * ExoPlayer drawn in a SurfaceView under the transparent Dioxus WebView (proven in Phase 0).
 *
 * Calls arrive from the Dioxus thread over JNI; ExoPlayer must be used on its own looper, so
 * commands are posted to main and getters read state a main-thread ticker copies every 100 ms.
 */
class PlayerBridge(private val activity: Activity) {
    private val main = Handler(Looper.getMainLooper())
    private var player: ExoPlayer? = null
    /** Makes this the app the remote's media keys go to while it plays (else the last one that did). */
    private var session: MediaSession? = null
    private var frame: AspectRatioFrameLayout? = null

    @Volatile private var positionS = 0.0
    @Volatile private var durationS = 0.0
    @Volatile private var playing = false
    @Volatile private var lastError: String? = null
    /** The audio and subtitle tracks, as JSON, refreshed whenever they change. */
    @Volatile private var tracksJson = "[]"

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
        val frame = AspectRatioFrameLayout(activity)
        frame.setBackgroundColor(Color.BLACK)
        val surface = SurfaceView(activity)
        frame.addView(surface, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        // Subtitles draw over the picture, under the WebView's controls.
        val subtitles = SubtitleView(activity)
        subtitles.setUserDefaultStyle()
        subtitles.setUserDefaultTextSize()
        frame.addView(subtitles, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        parent.addView(frame, 0, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        // The remote's fast-forward and rewind skip 10 s, as the on-screen buttons do.
        val exo = ExoPlayer.Builder(activity)
            .setSeekForwardIncrementMs(10_000)
            .setSeekBackIncrementMs(10_000)
            .build()
        exo.setVideoSurfaceView(surface)
        exo.addListener(object : Player.Listener {
            override fun onPlayerError(error: PlaybackException) {
                lastError = "${error.errorCodeName}: ${error.message}"
            }
            override fun onTracksChanged(tracks: Tracks) {
                tracksJson = describe(tracks)
            }
            override fun onCues(cues: CueGroup) {
                subtitles.setCues(cues.cues)
            }
            override fun onVideoSizeChanged(size: VideoSize) {
                if (size.height > 0) frame.setAspectRatio(size.width * size.pixelWidthHeightRatio / size.height)
            }
        })
        this.frame = frame
        player = exo
        // One session per player screen; ids must be unique while one is still being released.
        session = MediaSession.Builder(activity, exo).setId("reelhouse-${System.nanoTime()}").build()
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
    fun tracks(): String = tracksJson

    /** Picks track `index` of group `group` for audio or text; a negative group turns text off. */
    fun selectTrack(kind: String, group: Int, index: Int) {
        main.post {
            val exo = player ?: return@post
            val type = if (kind == "audio") C.TRACK_TYPE_AUDIO else C.TRACK_TYPE_TEXT
            val params = exo.trackSelectionParameters.buildUpon()
            if (group < 0) {
                params.setTrackTypeDisabled(type, true)
            } else {
                val g = exo.currentTracks.groups.getOrNull(group) ?: return@post
                params.setTrackTypeDisabled(type, false)
                    .setOverrideForType(TrackSelectionOverride(g.mediaTrackGroup, index))
            }
            exo.trackSelectionParameters = params.build()
        }
    }

    private fun describe(tracks: Tracks): String {
        val out = JSONArray()
        tracks.groups.forEachIndexed { g, group ->
            val kind = when (group.type) {
                C.TRACK_TYPE_AUDIO -> "audio"
                C.TRACK_TYPE_TEXT -> "text"
                else -> return@forEachIndexed
            }
            for (i in 0 until group.length) {
                if (!group.isTrackSupported(i)) continue
                val f = group.getTrackFormat(i)
                out.put(
                    JSONObject()
                        .put("kind", kind)
                        .put("group", g)
                        .put("index", i)
                        .put("label", f.label ?: "")
                        .put("language", f.language ?: "")
                        .put("channels", f.channelCount)
                        .put("selected", group.isTrackSelected(i)),
                )
            }
        }
        return out.toString()
    }

    /** Stops playback and removes the video layer when the player screen closes. */
    fun release() {
        main.post {
            session?.release()
            session = null
            player?.release()
            player = null
            frame?.let { (it.parent as? ViewGroup)?.removeView(it) }
            frame = null
        }
    }

    companion object {
        /** App-private storage for the saved session. */
        @JvmStatic
        fun filesDir(activity: Activity): String = activity.filesDir.absolutePath
    }
}
