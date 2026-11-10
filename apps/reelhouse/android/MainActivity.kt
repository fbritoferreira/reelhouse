package dev.dioxus.main

typealias BuildConfig = dev.reelhouse.app.BuildConfig

class MainActivity : WryActivity() {
    // tao keeps process-wide native state for one activity (ndk-context asserts it is set up
    // once per process). If this activity goes away, end the process so the next launch starts
    // clean instead of re-running startup on top of the old state and crashing.
    override fun onDestroy() {
        super.onDestroy()
        android.os.Process.killProcess(android.os.Process.myPid())
    }
}
