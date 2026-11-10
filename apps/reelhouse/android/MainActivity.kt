package dev.dioxus.main

import android.view.KeyEvent
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView

typealias BuildConfig = dev.reelhouse.app.BuildConfig

class MainActivity : WryActivity() {
    // tao keeps process-wide native state for one activity (ndk-context asserts it is set up
    // once per process). If this activity goes away, end the process so the next launch starts
    // clean instead of re-running startup on top of the old state and crashing.
    override fun onDestroy() {
        super.onDestroy()
        android.os.Process.killProcess(android.os.Process.myPid())
    }

    // The remote's Back goes to the app first (src/tv.rs decides: previous screen, or leave).
    // Before the app is ready to answer, Back closes it as usual.
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (event.keyCode != KeyEvent.KEYCODE_BACK) return super.dispatchKeyEvent(event)
        if (event.action == KeyEvent.ACTION_UP) {
            val web = findWebView(window.decorView)
            if (web == null) {
                finish()
            } else {
                web.evaluateJavascript("window.__rhBack ? (window.__rhBack(), true) : null") { handled ->
                    if (handled != "true") finish()
                }
            }
        }
        return true
    }

    private fun findWebView(view: View): WebView? {
        if (view is WebView) return view
        if (view is ViewGroup) {
            for (i in 0 until view.childCount) findWebView(view.getChildAt(i))?.let { return it }
        }
        return null
    }
}
