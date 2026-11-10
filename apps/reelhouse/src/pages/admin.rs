use dioxus::prelude::*;
use reelhouse_api::{
    Library, LibraryKind, NewLibrary, NewTvSource, NewUser, TvSource, TvSourceKind,
};

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
    let mut adding = use_signal(|| false);
    let add_library = move |e: FormEvent| async move {
        e.prevent_default();
        adding.set(true);
        let new = NewLibrary {
            name: lib_name(),
            kind: lib_kind(),
            path: lib_path(),
        };
        match client().create_library(&new).await {
            Ok(lib) => {
                lib_name.set(String::new());
                lib_path.set(String::new());
                status.set(Some(format!("Added {}. Scanning it now.", lib.name)));
                libs.restart();
                match client().scan(lib.id).await {
                    Ok(r) => status.set(Some(format!("{}: {} titles added.", lib.name, r.added))),
                    Err(e) => status.set(Some(describe(&e))),
                }
                libs.restart();
            }
            Err(e) => status.set(Some(describe(&e))),
        }
        adding.set(false);
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
                status.set(Some(format!("{} can now sign in.", u.name)));
                users.restart();
            }
            Err(e) => status.set(Some(describe(&e))),
        }
    };

    rsx! {
        div { class: "page page-top manage",
            header { class: "page-head",
                h1 { "Manage" }
                p { class: "quiet", "Libraries, people and where your media lives." }
            }
            if let Some(msg) = status() {
                p { class: "toast", role: "status", "{msg}" }
            }
            section { class: "panel",
                h2 { "Libraries" }
                match &*libs.read() {
                    Some(Ok(list)) if list.is_empty() => rsx! { p { class: "quiet", "No libraries yet. Add the folder that holds your films or shows." } },
                    Some(Ok(list)) => rsx! {
                        ul { class: "list",
                            for lib in list.clone() {
                                LibraryRow { key: "{lib.id}", lib, status }
                            }
                        }
                    },
                    Some(Err(e)) => rsx! { p { class: "notice", "{describe(e)}" } },
                    None => rsx! {},
                }
                form { class: "form-grid", onsubmit: add_library,
                    label { "Name"
                        input { placeholder: "Movies", value: "{lib_name}", oninput: move |e| lib_name.set(e.value()) }
                    }
                    label { "Folder on the server"
                        input { placeholder: "/movies", value: "{lib_path}", oninput: move |e| lib_path.set(e.value()) }
                    }
                    label { "Contains"
                        select {
                            onchange: move |e| lib_kind.set(if e.value() == "tv" { LibraryKind::Tv } else { LibraryKind::Movies }),
                            option { value: "movies", "Films" }
                            option { value: "tv", "TV shows" }
                        }
                    }
                    button { class: "primary", r#type: "submit", disabled: adding(),
                        if adding() { "Adding…" } else { "Add library" }
                    }
                }
            }
            section { class: "panel",
                h2 { "People" }
                ul { class: "list",
                    if let Some(Ok(list)) = &*users.read() {
                        for u in list.clone() {
                            li { key: "{u.id}",
                                span { class: "avatar", "{u.name.chars().next().unwrap_or('?').to_uppercase()}" }
                                span { class: "list-name", "{u.name}" }
                                if u.is_admin {
                                    span { class: "tag", "Admin" }
                                }
                            }
                        }
                    }
                }
                form { class: "form-grid", onsubmit: add_user,
                    label { "Name"
                        input { value: "{user_name}", oninput: move |e| user_name.set(e.value()) }
                    }
                    label { "Password"
                        input { r#type: "password", value: "{user_pass}", oninput: move |e| user_pass.set(e.value()) }
                    }
                    button { class: "primary", r#type: "submit", "Add person" }
                }
            }
            TvSources { status }
            p { class: "quiet credit",
                "Metadata and artwork from "
                a { href: "https://www.themoviedb.org", target: "_blank", rel: "noopener", "TMDB" }
                ". This product uses the TMDB API but is not endorsed or certified by TMDB."
            }
        }
    }
}

#[component]
fn LibraryRow(lib: Library, status: Signal<Option<String>>) -> Element {
    let id = lib.id;
    let mut count =
        use_resource(move || async move { client().library_items(id).await.map(|i| i.len()) });
    let mut scanning = use_signal(|| false);
    let name = lib.name.clone();
    let kind = match lib.kind {
        LibraryKind::Movies => "Films",
        LibraryKind::Tv => "TV shows",
    };
    let titles = match &*count.read() {
        Some(Ok(1)) => "1 title".to_string(),
        Some(Ok(n)) => format!("{n} titles"),
        _ => String::new(),
    };
    rsx! {
        li {
            span { class: "list-name", "{lib.name}" }
            span { class: "tag", "{kind}" }
            span { class: "quiet mono-path", "{lib.path}" }
            span { class: "quiet", "{titles}" }
            button {
                class: "secondary",
                disabled: scanning(),
                onclick: move |_| {
                    let name = name.clone();
                    async move {
                        scanning.set(true);
                        match client().scan(id).await {
                            Ok(r) => status.set(Some(format!("{name}: {} added, {} removed.", r.added, r.removed))),
                            Err(e) => status.set(Some(describe(&e))),
                        }
                        scanning.set(false);
                        count.restart();
                    }
                },
                if scanning() { "Scanning…" } else { "Scan" }
            }
        }
    }
}

/// IPTV sources: an M3U playlist or an Xtream Codes login, each with an optional guide.
#[component]
fn TvSources(status: Signal<Option<String>>) -> Element {
    let mut sources = use_resource(|| async move { client().tv_sources().await });
    let mut name = use_signal(String::new);
    let mut kind = use_signal(|| TvSourceKind::M3u);
    let mut url = use_signal(String::new);
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut epg = use_signal(String::new);
    let mut adding = use_signal(|| false);
    let optional = |s: String| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    let add = move |e: FormEvent| async move {
        e.prevent_default();
        adding.set(true);
        let new = NewTvSource {
            name: optional(name()).unwrap_or_else(|| "Live TV".into()),
            kind: kind(),
            url: url().trim().to_string(),
            username: optional(username()),
            password: optional(password()),
            epg_url: optional(epg()),
        };
        match client().create_tv_source(&new).await {
            Ok(src) => {
                for mut field in [name, url, username, password, epg] {
                    field.set(String::new());
                }
                status.set(Some(format!(
                    "Added {}. Loading its channels and guide now.",
                    src.name
                )));
                sources.restart();
                status.set(Some(refresh_message(
                    &src.name,
                    client().refresh_tv_source(src.id).await,
                )));
            }
            Err(e) => status.set(Some(describe(&e))),
        }
        adding.set(false);
    };
    rsx! {
        section { class: "panel",
            h2 { "Live TV" }
            match &*sources.read() {
                Some(Ok(list)) if list.is_empty() => rsx! { p { class: "quiet", "No sources yet. Add your provider's M3U playlist, or its Xtream Codes login." } },
                Some(Ok(list)) => rsx! {
                    ul { class: "list",
                        for src in list.clone() {
                            SourceRow { key: "{src.id}", src, status }
                        }
                    }
                },
                Some(Err(e)) => rsx! { p { class: "notice", "{describe(e)}" } },
                None => rsx! {},
            }
            form { class: "form-grid", onsubmit: add,
                label { "Name"
                    input { placeholder: "My provider", value: "{name}", oninput: move |e| name.set(e.value()) }
                }
                label { "Type"
                    select {
                        onchange: move |e| kind.set(if e.value() == "xtream" { TvSourceKind::Xtream } else { TvSourceKind::M3u }),
                        option { value: "m3u", "M3U playlist" }
                        option { value: "xtream", "Xtream Codes login" }
                    }
                }
                label { if kind() == TvSourceKind::Xtream { "Server address" } else { "Playlist address" }
                    input { placeholder: "https://", value: "{url}", oninput: move |e| url.set(e.value()) }
                }
                if kind() == TvSourceKind::Xtream {
                    label { "Username"
                        input { value: "{username}", autocomplete: "off", oninput: move |e| username.set(e.value()) }
                    }
                    label { "Password"
                        input { r#type: "password", value: "{password}", autocomplete: "off", oninput: move |e| password.set(e.value()) }
                    }
                }
                label { "Guide address (optional)"
                    input { placeholder: "XMLTV, if the playlist does not name one", value: "{epg}", oninput: move |e| epg.set(e.value()) }
                }
                button { class: "primary", r#type: "submit", disabled: adding(),
                    if adding() { "Adding…" } else { "Add source" }
                }
            }
        }
    }
}

fn refresh_message(
    name: &str,
    r: Result<reelhouse_api::RefreshReport, reelhouse_core::ClientError>,
) -> String {
    match r {
        Ok(r) => format!(
            "{name}: {} channels, {} guide entries.",
            r.channels, r.programmes
        ),
        Err(e) => format!("{name}: {}", describe(&e)),
    }
}

#[component]
fn SourceRow(src: TvSource, status: Signal<Option<String>>) -> Element {
    let mut refreshing = use_signal(|| false);
    let id = src.id;
    let name = src.name.clone();
    let kind = match src.kind {
        TvSourceKind::M3u => "M3U",
        TvSourceKind::Xtream => "Xtream",
    };
    rsx! {
        li {
            span { class: "list-name", "{src.name}" }
            span { class: "tag", "{kind}" }
            button {
                class: "secondary",
                disabled: refreshing(),
                onclick: move |_| {
                    let name = name.clone();
                    async move {
                        refreshing.set(true);
                        status.set(Some(refresh_message(&name, client().refresh_tv_source(id).await)));
                        refreshing.set(false);
                    }
                },
                if refreshing() { "Refreshing…" } else { "Refresh" }
            }
        }
    }
}
