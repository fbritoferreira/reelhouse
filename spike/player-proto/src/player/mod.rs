#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::Player;

#[cfg(not(any(
    target_arch = "wasm32",
    target_os = "ios",
    target_os = "android",
    feature = "desktop"
)))]
mod stub;
#[cfg(not(any(
    target_arch = "wasm32",
    target_os = "ios",
    target_os = "android",
    feature = "desktop"
)))]
pub use stub::Player;

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
pub use ios::Player;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::Player;
