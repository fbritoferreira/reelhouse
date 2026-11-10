//! Media3 ExoPlayer under the transparent webview, called over raw JNI.
//!
//! The `#[manganis::ffi]` block only makes dx bundle `native/android` as a Gradle module. Its generated
//! calls are not used: in 0.7.10 they look the class up from the Dioxus thread, where only system
//! classes are visible (Phase 0 results, section 3.1).

use manganis::android::with_activity;
use manganis::jni::JNIEnv;
use manganis::jni::objects::{GlobalRef, JClass, JObject, JString, JValue, JValueOwned};

#[manganis::ffi("/native/android")]
extern "Kotlin" {
    pub type PlayerBridge;
    pub fn play(this: &PlayerBridge);
}

const CLASS: &str = "dev.reelhouse.app.PlayerBridge";

fn load_class<'a>(
    env: &mut JNIEnv<'a>,
    activity: &JObject,
) -> manganis::jni::errors::Result<JClass<'a>> {
    let loader = env
        .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let name = env.new_string(CLASS)?;
    let class = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )?
        .l()?;
    Ok(JClass::from(class))
}

/// The app's private files directory, for the saved session.
pub fn files_dir() -> Option<String> {
    with_activity(|env, activity| {
        let class = load_class(env, activity).ok()?;
        let dir = env
            .call_static_method(
                &class,
                "filesDir",
                "(Landroid/app/Activity;)Ljava/lang/String;",
                &[JValue::Object(activity)],
            )
            .ok()?
            .l()
            .ok()?;
        env.get_string(&JString::from(dir)).ok().map(String::from)
    })
}

pub struct Player {
    bridge: Result<GlobalRef, String>,
}

impl Player {
    pub fn new() -> Self {
        let bridge = with_activity(|env, activity| {
            Some(
                (|| {
                    let class = load_class(env, activity)?;
                    let obj = env.new_object(
                        class,
                        "(Landroid/app/Activity;)V",
                        &[JValue::Object(activity)],
                    )?;
                    env.new_global_ref(obj)
                })()
                .map_err(|e| format!("could not start the player: {e}")),
            )
        })
        .unwrap_or_else(|| Err("no Android activity".into()));
        Player { bridge }
    }

    fn call<T>(
        &self,
        method: &str,
        sig: &str,
        args: &[JValue],
        read: impl FnOnce(&mut JNIEnv, JValueOwned) -> Option<T>,
    ) -> Option<T> {
        let bridge = self.bridge.as_ref().ok()?;
        with_activity(|env, _| {
            let value = env.call_method(bridge, method, sig, args).ok()?;
            read(env, value)
        })
    }

    pub fn load(&self, url: &str) {
        let Ok(bridge) = &self.bridge else { return };
        with_activity(|env, _| {
            let url = JObject::from(env.new_string(url).ok()?);
            env.call_method(
                bridge,
                "load",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&url)],
            )
            .ok()
            .map(|_| ())
        });
    }

    pub fn play(&self) {
        self.call("play", "()V", &[], |_, _| Some(()));
    }

    pub fn pause(&self) {
        self.call("pause", "()V", &[], |_, _| Some(()));
    }

    pub fn seek(&self, seconds: f64) {
        self.call("seek", "(D)V", &[JValue::Double(seconds)], |_, _| Some(()));
    }

    pub fn position(&self) -> f64 {
        self.call("position", "()D", &[], |_, v| v.d().ok())
            .unwrap_or(0.0)
    }

    pub fn duration(&self) -> f64 {
        self.call("duration", "()D", &[], |_, v| v.d().ok())
            .unwrap_or(0.0)
    }

    /// Audio and subtitle tracks in the playing title.
    pub fn tracks(&self) -> Vec<super::Track> {
        self.call("tracks", "()Ljava/lang/String;", &[], |env, v| {
            let obj = v.l().ok()?;
            let json: String = env.get_string(&JString::from(obj)).ok()?.into();
            serde_json::from_str(&json).ok()
        })
        .unwrap_or_default()
    }

    pub fn select_track(&self, t: &super::Track) {
        self.select(&t.kind, t.group, t.index);
    }

    pub fn subtitles_off(&self) {
        self.select("text", -1, -1);
    }

    fn select(&self, kind: &str, group: i32, index: i32) {
        let Ok(bridge) = &self.bridge else { return };
        with_activity(|env, _| {
            let kind = JObject::from(env.new_string(kind).ok()?);
            env.call_method(
                bridge,
                "selectTrack",
                "(Ljava/lang/String;II)V",
                &[
                    JValue::Object(&kind),
                    JValue::Int(group),
                    JValue::Int(index),
                ],
            )
            .ok()
            .map(|_| ())
        });
    }

    pub fn is_playing(&self) -> bool {
        self.call("isPlaying", "()Z", &[], |_, v| v.z().ok())
            .unwrap_or(false)
    }

    pub fn error(&self) -> Option<String> {
        if let Err(e) = &self.bridge {
            return Some(e.clone());
        }
        self.call("error", "()Ljava/lang/String;", &[], |env, v| {
            let obj = v.l().ok()?;
            if obj.is_null() {
                return None;
            }
            env.get_string(&JString::from(obj)).ok().map(String::from)
        })
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.call("release", "()V", &[], |_, _| Some(()));
    }
}
