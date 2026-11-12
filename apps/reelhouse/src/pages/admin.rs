use dioxus::prelude::*;
use reelhouse_api::{
    JobKind, JobStatus, Library, LibraryKind, NewLibrary, NewTvSource, NewUser, TvSource,
    TvSourceKind,
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
                libs.restart();
                match client().scan(lib.id).await {
                    Ok(_) => status.set(Some(format!(
                        "Added {}. Its scan is under Activity.",
                        lib.name
                    ))),
                    Err(e) => status.set(Some(describe(&e))),
                }
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
                                LibraryRow { key: "{lib.id}", lib, status, on_removed: move |_| libs.restart() }
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
            Activity {}
            p { class: "quiet credit",
                "Metadata and artwork from "
                a { href: "https://www.themoviedb.org", target: "_blank", rel: "noopener", "TMDB" }
                ". This product uses the TMDB API but is not endorsed or certified by TMDB."
            }
        }
    }
}

#[component]
fn LibraryRow(lib: Library, status: Signal<Option<String>>, on_removed: EventHandler) -> Element {
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
                            Ok(_) => status.set(Some(format!("Scanning {name}. Progress is under Activity."))),
                            Err(e) => status.set(Some(describe(&e))),
                        }
                        scanning.set(false);
                        count.restart();
                    }
                },
                if scanning() { "Scanning…" } else { "Scan" }
            }
            ConfirmRemove {
                what: lib.name.clone(),
                on_confirm: {
                    let name = lib.name.clone();
                    move |_| {
                        let name = name.clone();
                        spawn(async move {
                            match client().delete_library(id).await {
                                Ok(()) => {
                                    status.set(Some(format!("Removed {name}. Its files are untouched.")));
                                    on_removed.call(());
                                }
                                Err(e) => status.set(Some(describe(&e))),
                            }
                        });
                    }
                },
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
                            SourceRow { key: "{src.id}", src, status, on_changed: move |_| sources.restart() }
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
    r: Result<reelhouse_api::Job, reelhouse_core::ClientError>,
) -> String {
    match r {
        Ok(_) => format!("Loading {name}'s channels and guide. Progress is under Activity."),
        Err(e) => format!("{name}: {}", describe(&e)),
    }
}

/// Scans and refreshes as they queue, run, retry and finish.
#[component]
fn Activity() -> Element {
    let mut jobs = use_resource(|| async move { client().jobs().await });
    let libs = use_resource(|| async move { client().libraries().await.unwrap_or_default() });
    let sources = use_resource(|| async move { client().tv_sources().await.unwrap_or_default() });
    // Poll while Manage is open, so jobs queued from any button (or another browser) show up.
    use_future(move || async move {
        loop {
            futures_timer::Delay::new(std::time::Duration::from_secs(3)).await;
            jobs.restart();
        }
    });
    let list = match &*jobs.read() {
        Some(Ok(list)) => list.iter().take(8).cloned().collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let name_of = |j: &reelhouse_api::Job| -> String {
        match j.kind {
            JobKind::ScanLibrary => libs
                .read()
                .as_ref()
                .and_then(|l| {
                    l.iter()
                        .find(|l| l.id == j.target_id)
                        .map(|l| l.name.clone())
                })
                .unwrap_or_else(|| "a removed library".into()),
            JobKind::RefreshSource => sources
                .read()
                .as_ref()
                .and_then(|s| {
                    s.iter()
                        .find(|s| s.id == j.target_id)
                        .map(|s| s.name.clone())
                })
                .unwrap_or_else(|| "a removed source".into()),
        }
    };
    rsx! {
        section { class: "panel",
            h2 { "Activity" }
            if list.is_empty() {
                p { class: "quiet", "Scans and Live TV refreshes appear here as they run." }
            }
            ul { class: "list jobs",
                for j in list {
                    {
                        let what = match j.kind {
                            JobKind::ScanLibrary => format!("Scan {}", name_of(&j)),
                            JobKind::RefreshSource => format!("Refresh {}", name_of(&j)),
                        };
                        let (state, class) = job_state(&j);
                        rsx! {
                            li { key: "{j.id}", class: "job {class}",
                                span { class: "job-dot", "aria-hidden": "true" }
                                span { class: "list-name", "{what}" }
                                span { class: "quiet job-state", "{state}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// What a job is doing, in words, and a class for its dot.
fn job_state(j: &reelhouse_api::Job) -> (String, &'static str) {
    let tries = reelhouse_api::JOB_MAX_ATTEMPTS;
    match j.status {
        JobStatus::Queued if j.attempts > 0 => (
            format!(
                "Failed, trying again ({} of {tries}): {}",
                j.attempts + 1,
                j.last_error.clone().unwrap_or_default()
            ),
            "retrying",
        ),
        JobStatus::Queued => ("Waiting".into(), "waiting"),
        JobStatus::Running if j.attempts > 1 => {
            (format!("Running, try {} of {tries}", j.attempts), "running")
        }
        JobStatus::Running => ("Running".into(), "running"),
        JobStatus::Done => (
            match (j.kind, &j.result) {
                (JobKind::ScanLibrary, Some(r)) => {
                    format!("{} added, {} removed", r["added"], r["removed"])
                }
                (JobKind::RefreshSource, Some(r)) => format!(
                    "{} channels, {} guide entries, {} on demand",
                    r["channels"], r["programmes"], r["on_demand"]
                ),
                _ => "Done".into(),
            },
            "done",
        ),
        JobStatus::Failed => (
            format!(
                "Failed after {tries} tries: {}",
                j.last_error.clone().unwrap_or_default()
            ),
            "failed",
        ),
    }
}

/// One source; `on_changed` after a refresh or removal, so the list is fetched again.
#[component]
fn SourceRow(src: TvSource, status: Signal<Option<String>>, on_changed: EventHandler) -> Element {
    let mut refreshing = use_signal(|| false);
    let mut editing = use_signal(|| false);
    let mut guide_open = use_signal(|| false);
    let mut guide_url = use_signal(|| src.epg_url.clone().unwrap_or_default());
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
            span { class: "quiet",
                match src.refreshed_at {
                    Some(at) => format!("Refreshed {}", reelhouse_core::timefmt::ago(crate::live_cache::now_s(), at)),
                    None => "Not refreshed yet".into(),
                }
            }
            {
                let now = crate::live_cache::now_s();
                match src.guide_until {
                    Some(until) if until > now => rsx! {
                        span { class: "quiet", "Guide for the next {reelhouse_core::timefmt::ahead(now, until)}" }
                    },
                    Some(until) => rsx! {
                        span { class: "notice", "Guide ran out {reelhouse_core::timefmt::ago(now, until)}; the provider has not sent a newer one" }
                    },
                    None => rsx! {},
                }
            }
            button {
                class: "secondary",
                disabled: refreshing(),
                onclick: move |_| {
                    let name = name.clone();
                    async move {
                        refreshing.set(true);
                        status.set(Some(refresh_message(&name, client().refresh_tv_source(id).await)));
                        refreshing.set(false);
                        on_changed.call(());
                    }
                },
                if refreshing() { "Refreshing…" } else { "Refresh" }
            }
            button {
                class: if editing() { "secondary current" } else { "secondary" },
                "aria-expanded": editing(),
                onclick: move |_| editing.toggle(),
                "Choose groups"
            }
            button {
                class: if guide_open() { "secondary current" } else { "secondary" },
                "aria-expanded": guide_open(),
                onclick: move |_| guide_open.toggle(),
                "Guide address"
            }
            ConfirmRemove {
                what: src.name.clone(),
                on_confirm: {
                    let name = src.name.clone();
                    move |_| {
                        let name = name.clone();
                        spawn(async move {
                            match client().delete_tv_source(id).await {
                                Ok(()) => {
                                    status.set(Some(format!("Removed {name} and its channels.")));
                                    on_changed.call(());
                                }
                                Err(e) => status.set(Some(describe(&e))),
                            }
                        });
                    }
                },
            }
        }
        if editing() {
            li { class: "group-picker",
                GroupPicker { source: id, status, on_done: move |_| editing.set(false) }
            }
        }
        if guide_open() {
            li {
                form {
                    class: "form-grid",
                    onsubmit: move |e: FormEvent| async move {
                        e.prevent_default();
                        match client().set_tv_source_guide(id, &guide_url()).await {
                            Ok(_) => {
                                status.set(Some(if guide_url().trim().is_empty() {
                                    "The guide comes from the playlist again. Refresh to load it.".to_string()
                                } else {
                                    "Guide address saved. Refresh to load it.".to_string()
                                }));
                                guide_open.set(false);
                                on_changed.call(());
                            }
                            Err(e) => status.set(Some(describe(&e))),
                        }
                    },
                    label { "Guide address (XMLTV), empty for the playlist's own"
                        input {
                            r#type: "url",
                            value: "{guide_url}",
                            placeholder: "http://…/xmltv.php",
                            oninput: move |e| guide_url.set(e.value()),
                        }
                    }
                    button { class: "primary", r#type: "submit", "Save" }
                }
            }
        }
    }
}

/// Remove, then a second press to confirm, in place: no browser dialog.
#[component]
fn ConfirmRemove(what: String, on_confirm: EventHandler) -> Element {
    let mut asking = use_signal(|| false);
    rsx! {
        if asking() {
            span { class: "confirm-remove",
                span { class: "quiet", "Remove {what}?" }
                button {
                    class: "danger",
                    onclick: move |_| {
                        asking.set(false);
                        on_confirm.call(());
                    },
                    "Remove"
                }
                button { class: "secondary", onclick: move |_| asking.set(false), "Keep" }
            }
        } else {
            button { class: "secondary", onclick: move |_| asking.set(true), "Remove" }
        }
    }
}

/// Which of a source's groups Live TV shows: a searchable checklist.
#[component]
fn GroupPicker(source: i64, status: Signal<Option<String>>, on_done: EventHandler) -> Element {
    let groups = use_resource(move || async move { client().source_groups(source).await });
    // Hidden group names; "" is the ungrouped. Filled once the list arrives.
    let mut hidden = use_signal(|| None::<std::collections::HashSet<String>>);
    let mut filter = use_signal(String::new);
    let mut saving = use_signal(|| false);
    let list = match &*groups.read() {
        Some(Ok(list)) => list.clone(),
        Some(Err(e)) => return rsx! { p { class: "notice", "{describe(e)}" } },
        None => return rsx! { p { class: "quiet", "Loading this source's groups" } },
    };
    let key = |g: &reelhouse_api::SourceGroup| g.name.clone().unwrap_or_default();
    if hidden.peek().is_none() {
        hidden.set(Some(list.iter().filter(|g| !g.shown).map(key).collect()));
    }
    let hidden_now = hidden().unwrap_or_default();
    let needle = filter().to_lowercase();
    let matching: Vec<_> = list
        .iter()
        .filter(|g| key(g).to_lowercase().contains(&needle))
        .cloned()
        .collect();
    let shown_count = list.len() - hidden_now.len();
    let mut set_all = move |show: bool, names: Vec<String>| {
        let mut h = hidden().unwrap_or_default();
        for n in names {
            if show {
                h.remove(&n);
            } else {
                h.insert(n);
            }
        }
        hidden.set(Some(h));
    };
    let names: Vec<String> = matching.iter().map(key).collect();
    let names2 = names.clone();
    rsx! {
        div { class: "group-picker-head",
            input {
                r#type: "search",
                placeholder: "Find a group",
                "aria-label": "Find a group",
                value: "{filter}",
                oninput: move |e| filter.set(e.value()),
            }
            span { class: "quiet", "{shown_count} of {list.len()} groups shown" }
            button { class: "secondary", onclick: move |_| set_all(true, names.clone()), "Show these" }
            button { class: "secondary", onclick: move |_| set_all(false, names2.clone()), "Hide these" }
        }
        ul { class: "group-checks",
            for g in matching {
                li { key: "{key(&g)}",
                    label {
                        input {
                            r#type: "checkbox",
                            checked: !hidden_now.contains(&key(&g)),
                            onchange: {
                                let k = key(&g);
                                move |e: FormEvent| {
                                    let mut h = hidden().unwrap_or_default();
                                    if e.checked() {
                                        h.remove(&k);
                                    } else {
                                        h.insert(k.clone());
                                    }
                                    hidden.set(Some(h));
                                }
                            },
                        }
                        span { "{g.name.clone().unwrap_or_else(|| \"Other channels\".into())}" }
                        span { class: "quiet", "{g.channels}" }
                    }
                }
            }
        }
        div { class: "group-picker-actions",
            button {
                class: "primary",
                disabled: saving(),
                onclick: move |_| async move {
                    saving.set(true);
                    let body = reelhouse_api::HiddenGroups {
                        hidden: hidden()
                            .unwrap_or_default()
                            .into_iter()
                            .map(|n| Some(n).filter(|n| !n.is_empty()))
                            .collect(),
                    };
                    match client().hide_groups(source, &body).await {
                        Ok(()) => {
                            status.set(Some("Saved. Live TV shows only the groups you chose.".into()));
                            on_done.call(());
                        }
                        Err(e) => status.set(Some(describe(&e))),
                    }
                    saving.set(false);
                },
                if saving() { "Saving…" } else { "Save groups" }
            }
            button { class: "secondary", onclick: move |_| on_done.call(()), "Cancel" }
        }
    }
}
