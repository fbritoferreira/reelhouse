//! The Reelhouse app for web, Android and Google TV.

use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! { "Reelhouse" }
}
