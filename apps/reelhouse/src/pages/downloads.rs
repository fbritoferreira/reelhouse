use dioxus::prelude::*;

/// Everything downloaded to this device, playable without the server.
#[cfg(not(target_arch = "wasm32"))]
#[component]
pub fn Downloads() -> Element {
    use crate::Route;
    {
        let mut refresh = use_signal(|| 0u32);
        let _ = refresh();
        let list = crate::downloads::list();
        rsx! {
            div { class: "page page-top",
                header { class: "page-head",
                    h1 { "Downloads" }
                    p { class: "quiet", "Saved on this device. They play without your server." }
                }
                if list.is_empty() {
                    p { class: "empty", "Nothing downloaded yet. Open a film or episode and choose Download." }
                } else {
                    ul { class: "downloads",
                        for d in list {
                            li { key: "{d.item.id}",
                                Link { class: "wide", to: Route::Play { id: d.item.id },
                                    div { class: "wide-art",
                                        img { src: "/art/{d.item.id}/backdrop", alt: "" }
                                    }
                                    span { class: "poster-title", "{d.item.show_title.clone().unwrap_or(d.item.title.clone())}" }
                                    span { class: "poster-caption",
                                        if d.done {
                                            "{size(d.bytes)}"
                                        } else {
                                            "Not finished: open it to resume"
                                        }
                                    }
                                }
                                button {
                                    class: "secondary",
                                    onclick: move |_| {
                                        crate::downloads::remove(d.item.id);
                                        refresh += 1;
                                    },
                                    "Remove"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[component]
pub fn Downloads() -> Element {
    rsx! {
        div { class: "page page-top", p { class: "quiet", "Downloads are available in the TV and mobile apps." } }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn size(bytes: u64) -> String {
    let gb = bytes as f64 / 1_073_741_824.0;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}
