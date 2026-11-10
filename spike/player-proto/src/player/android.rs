//! Media3 ExoPlayer backend, called over raw JNI.
//!
//! The `#[manganis::ffi]` block exists only so dx bundles native/android as a Gradle module. Its
//! generated calls are unusable in 0.7.10: the class is looked up as `com.example.PlayerBridge` (the
//! leading `/` in the path makes it miss build.gradle.kts), `env.find_class` runs on the Dioxus thread
//! where only system classes are visible, and `bool` returns do not compile.

use manganis::android::with_activity;
use manganis::jni::JNIEnv;
use manganis::jni::objects::{GlobalRef, JClass, JObject, JString, JValue, JValueOwned};

#[manganis::ffi("/native/android")]
extern "Kotlin" {
    pub type PlayerBridge;
    pub fn play(this: &PlayerBridge);
}

const CLASS: &str = "dev.reelhouse.proto.PlayerBridge";

pub struct Player {
    bridge: Result<GlobalRef, String>,
}

fn create() -> Result<GlobalRef, String> {
    with_activity(|env, activity| {
        Some(
            (|| -> manganis::jni::errors::Result<GlobalRef> {
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
                let instance = env.new_object(
                    JClass::from(class),
                    "(Landroid/app/Activity;)V",
                    &[JValue::Object(activity)],
                )?;
                env.new_global_ref(instance)
            })()
            .map_err(|e| format!("PlayerBridge init failed: {e}")),
        )
    })
    .unwrap_or_else(|| Err("no JNI environment".into()))
}

impl Player {
    pub fn new() -> Player {
        Player { bridge: create() }
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
