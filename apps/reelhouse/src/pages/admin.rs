use dioxus::prelude::*;
use reelhouse_api::{LibraryKind, NewLibrary, NewUser};

use crate::session::client;
use crate::widgets::describe;

#[component]
pub fn Admin() -> Element {
    let mut libs = use_resource(|| async move { client().libraries().await });
    let mut users = use_resource(|| async move { client().users().await });
    let mut status = use_signal(|| None::<String>);

    let mut lib_name = use_signal(String::new);
    let mut lib_path = use_signal(String::new);
    let mut lib_kind = use_signal(|| LibraryKind::Movies);
    let add_library = move |e: FormEvent| async move {
        e.prevent_default();
        let new = NewLibrary {
            name: lib_name(),
            kind: lib_kind(),
            path: lib_path(),
        };
        match client().create_library(&new).await {
            Ok(lib) => {
                lib_name.set(String::new());
                lib_path.set(String::new());
                status.set(Some(format!("Added {}. Scanning…", lib.name)));
                match client().scan(lib.id).await {
                    Ok(r) => status.set(Some(format!("Scanned {}: {} added.", lib.name, r.added))),
                    Err(e) => status.set(Some(describe(&e))),
                }
                libs.restart();
            }
            Err(e) => status.set(Some(describe(&e))),
        }
    };

    let mut user_name = use_signal(String::new);
    let mut user_pass = use_signal(String::new);
    let add_user = move |e: FormEvent| async move {
        e.prevent_default();
        let new = NewUser {
            name: user_name(),
            password: user_pass(),
            is_admin: false,
        };
        match client().create_user(&new).await {
            Ok(u) => {
                user_name.set(String::new());
                user_pass.set(String::new());
                status.set(Some(format!("Added {}.", u.name)));
                users.restart();
            }
            Err(e) => status.set(Some(describe(&e))),
        }
    };

    rsx! {
        div { class: "admin",
            if let Some(msg) = status() {
                p { class: "notice", "{msg}" }
            }
            section {
                h2 { "Libraries" }
                ul { class: "list",
                    if let Some(Ok(list)) = &*libs.read() {
                        for lib in list.clone() {
                            li { key: "{lib.id}",
                                span { "{lib.name}" }
                                span { class: "quiet", "{lib.path}" }
                                button {
                                    onclick: move |_| {
                                        let (id, name) = (lib.id, lib.name.clone());
                                        async move {
                                        match client().scan(id).await {
                                            Ok(r) => status.set(Some(format!("Scanned {name}: {} added, {} removed.", r.added, r.removed))),
                                            Err(e) => status.set(Some(describe(&e))),
                                        }
                                        }
                                    },
                                    "Scan"
                                }
                            }
                        }
                    }
                }
                form { class: "inline-form", onsubmit: add_library,
                    input { placeholder: "Name, e.g. Movies", value: "{lib_name}", oninput: move |e| lib_name.set(e.value()) }
                    input { placeholder: "Folder on the server, e.g. /media/movies", value: "{lib_path}", oninput: move |e| lib_path.set(e.value()) }
                    select {
                        onchange: move |e| lib_kind.set(if e.value() == "tv" { LibraryKind::Tv } else { LibraryKind::Movies }),
                        option { value: "movies", "Movies" }
                        option { value: "tv", "TV shows" }
                    }
                    button { class: "primary", r#type: "submit", "Add and scan" }
                }
            }
            section {
                h2 { "People" }
                ul { class: "list",
                    if let Some(Ok(list)) = &*users.read() {
                        for u in list.clone() {
                            li { key: "{u.id}",
                                span { "{u.name}" }
                                if u.is_admin {
                                    span { class: "quiet", "Admin" }
                                }
                            }
                        }
                    }
                }
                form { class: "inline-form", onsubmit: add_user,
                    input { placeholder: "Name", value: "{user_name}", oninput: move |e| user_name.set(e.value()) }
                    input { r#type: "password", placeholder: "Password", value: "{user_pass}", oninput: move |e| user_pass.set(e.value()) }
                    button { class: "primary", r#type: "submit", "Add person" }
                }
            }
        }
    }
}
