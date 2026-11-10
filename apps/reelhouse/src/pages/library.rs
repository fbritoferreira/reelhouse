use dioxus::prelude::*;

use crate::session::client;
use crate::widgets::{LoadError, Poster, describe};

#[component]
pub fn LibraryPage(id: i64) -> Element {
    let data = use_resource(move || async move {
        let c = client();
        let lib = c.libraries().await?.into_iter().find(|l| l.id == id);
        Ok::<_, reelhouse_core::ClientError>((lib, c.library_items(id).await?))
    });
    match &*data.read() {
        None => rsx! { div { class: "page", p { class: "quiet", "Loading" } } },
        Some(Err(e)) => rsx! { div { class: "page", LoadError { error: describe(e) } } },
        Some(Ok((lib, items))) => {
            let name = lib.as_ref().map(|l| l.name.clone()).unwrap_or_default();
            let count = items.len();
            rsx! {
                div { class: "page page-top",
                    header { class: "page-head",
                        h1 { "{name}" }
                        p { class: "quiet", if count == 1 { "1 title" } else { "{count} titles" } }
                    }
                    if items.is_empty() {
                        p { class: "empty", "Nothing here yet. Scan this library from Manage." }
                    } else {
                        div { class: "grid",
                            for item in items.clone() {
                                Poster { key: "{item.id}", item }
                            }
                        }
                    }
                }
            }
        }
    }
}
