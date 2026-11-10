use dioxus::prelude::*;

use crate::Route;
use crate::session::client;
use crate::widgets::{LoadError, Row, describe};

#[component]
pub fn Home() -> Element {
    let data = use_resource(|| async move {
        let c = client();
        let cont = c.continue_watching().await?;
        let libs = c.libraries().await?;
        let mut rows = Vec::new();
        for lib in libs {
            let items = c.library_items(lib.id).await?;
            rows.push((lib, items));
        }
        Ok::<_, reelhouse_core::ClientError>((cont, rows))
    });
    match &*data.read() {
        None => rsx! { p { class: "quiet", "Loading" } },
        Some(Err(e)) => rsx! { LoadError { error: describe(e) } },
        Some(Ok((cont, rows))) => rsx! {
            if !cont.is_empty() {
                Row { title: "Continue watching", items: cont.clone() }
            }
            if rows.is_empty() {
                div { class: "empty",
                    p { "No libraries yet." }
                    Link { class: "primary", to: Route::Admin {}, "Add a library" }
                }
            }
            for (lib, items) in rows.clone() {
                section { class: "row",
                    h2 { Link { to: Route::LibraryPage { id: lib.id }, "{lib.name}" } }
                    div { class: "row-scroll",
                        for item in items.into_iter().take(24) {
                            crate::widgets::Poster { key: "{item.id}", item }
                        }
                    }
                }
            }
        },
    }
}
