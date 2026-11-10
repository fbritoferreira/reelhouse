use dioxus::prelude::*;
use reelhouse_api::ItemKind;
use reelhouse_core::timefmt::format_time;

use crate::Route;
use crate::session::client;
use crate::widgets::{LoadError, Poster, art_url, caption, describe};

#[component]
pub fn ItemPage(id: i64) -> Element {
    let data = use_resource(move || async move {
        let c = client();
        let item = c.item(id).await?;
        let children = match item.kind {
            ItemKind::Show | ItemKind::Season => c.children(id).await?,
            _ => Vec::new(),
        };
        Ok::<_, reelhouse_core::ClientError>((item, children))
    });
    match &*data.read() {
        None => rsx! { p { class: "quiet", "Loading" } },
        Some(Err(e)) => rsx! { LoadError { error: describe(e) } },
        Some(Ok((item, children))) => {
            let playable = matches!(item.kind, ItemKind::Movie | ItemKind::Episode);
            let resume = item
                .progress
                .as_ref()
                .filter(|p| !p.played && p.position_s > 0.0)
                .map(|p| p.position_s);
            let cap = caption(item);
            rsx! {
                article { class: "detail",
                    if let Some(bg) = art_url(item.backdrop_url.clone()) {
                        div { class: "backdrop", style: "background-image: url('{bg}')" }
                    }
                    div { class: "detail-body",
                        h1 { "{item.title}" }
                        if !cap.is_empty() {
                            p { class: "quiet", "{cap}" }
                        }
                        if let Some(d) = item.duration_s {
                            p { class: "quiet", "{format_time(d)}" }
                        }
                        if playable {
                            div { class: "actions",
                                if let Some(at) = resume {
                                    Link { class: "primary", autofocus: true, to: Route::Play { id: item.id }, "Resume from {format_time(at)}" }
                                } else {
                                    Link { class: "primary", autofocus: true, to: Route::Play { id: item.id }, "Play" }
                                }
                            }
                        }
                        if let Some(o) = item.overview.clone() {
                            p { class: "overview", "{o}" }
                        }
                    }
                    if !children.is_empty() {
                        div { class: "grid",
                            for child in children.clone() {
                                Poster { key: "{child.id}", item: child }
                            }
                        }
                    }
                }
            }
        }
    }
}
