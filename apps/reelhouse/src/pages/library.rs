use dioxus::prelude::*;

use crate::session::client;
use crate::widgets::{LoadError, Poster, describe};

#[component]
pub fn LibraryPage(id: i64) -> Element {
    let data = use_resource(move || async move { client().library_items(id).await });
    match &*data.read() {
        None => rsx! { p { class: "quiet", "Loading" } },
        Some(Err(e)) => rsx! { LoadError { error: describe(e) } },
        Some(Ok(items)) if items.is_empty() => rsx! {
            p { class: "empty", "Nothing here yet. Scan this library from Manage." }
        },
        Some(Ok(items)) => rsx! {
            div { class: "grid",
                for item in items.clone() {
                    Poster { key: "{item.id}", item }
                }
            }
        },
    }
}
