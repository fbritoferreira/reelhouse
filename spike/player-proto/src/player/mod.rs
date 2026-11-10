#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::Player;

#[cfg(not(any(target_arch = "wasm32", target_os = "ios", target_os = "android", feature = "desktop")))]
mod stub;
#[cfg(not(any(target_arch = "wasm32", target_os = "ios", target_os = "android", feature = "desktop")))]
pub use stub::Player;
