use dioxus::prelude::*;
use reelhouse_api::Item;

use crate::Route;
use crate::session::client;
use crate::widgets::{Facts, Hero, LoadError, PlayButton, Poster, Row, WideCard, describe};

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
        None => rsx! { div { class: "page", p { class: "quiet", "Loading" } } },
        Some(Err(e)) => rsx! { div { class: "page", LoadError { error: describe(e) } } },
        Some(Ok((cont, rows))) => {
            // The hero is what you are in the middle of, or else the first title with artwork.
            let featured: Option<Item> = cont
                .iter()
                .find(|i| i.backdrop_url.is_some())
                .or_else(|| {
                    rows.iter()
                        .flat_map(|(_, items)| items)
                        .find(|i| i.backdrop_url.is_some())
                })
                .cloned();
            rsx! {
                if let Some(item) = featured {
                    Hero {
                        backdrop: item.backdrop_url.clone(),
                        title: item.show_title.clone().unwrap_or(item.title.clone()),
                        above: rsx! {},
                        Facts { item: item.clone() }
                        div { class: "hero-actions",
                            PlayButton { item: item.clone() }
                            Link { class: "secondary big", to: Route::ItemPage { id: item.show_id.unwrap_or(item.id) }, "More about it" }
                        }
                        if let Some(o) = item.overview.clone() {
                            p { class: "overview", "{o}" }
                        }
                    }
                }
                div { class: "page rows",
                    if !cont.is_empty() {
                        Row { title: "Continue watching", to: None,
                            for item in cont.clone() {
                                WideCard { key: "{item.id}", item }
                            }
                        }
                    }
                    if rows.is_empty() {
                        div { class: "empty",
                            h2 { "No libraries yet" }
                            p { "Add your Movies and TV folders, and Reelhouse finds artwork and details for everything in them." }
                            Link { class: "primary big", to: Route::Admin {}, "Add a library" }
                        }
                    }
                    for (lib, items) in rows.clone() {
                        Row { key: "{lib.id}", title: lib.name.clone(), to: Some(Route::LibraryPage { id: lib.id }),
                            for item in items.into_iter().take(30) {
                                Poster { key: "{item.id}", item }
                            }
                        }
                    }
                }
            }
        }
    }
}
