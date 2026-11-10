use dioxus::prelude::*;
use reelhouse_api::NewUser;

use crate::Route;
use crate::session::{self, client};
use crate::widgets::describe;

#[component]
pub fn Login() -> Element {
    let mut name = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut server = use_signal(|| {
        if session::needs_server() {
            "http://".to_string()
        } else {
            String::new()
        }
    });
    let mut error = use_signal(|| None::<String>);
    let ask_server = session::needs_server();
    let found = use_resource(move || async move { discover(ask_server).await });
    // Fill in the address as soon as one server answers, unless the user already typed one.
    use_effect(move || {
        if let Some(first) = found.read().as_ref().and_then(|list| list.first().cloned())
            && server.peek().trim_end_matches('/') == "http:"
        {
            server.set(first);
        }
    });
    let nav = navigator();
    let submit = move |e: FormEvent| async move {
        e.prevent_default();
        if ask_server {
            session::set_server(&server());
        }
        match client().login(&name(), &password()).await {
            Ok(res) => {
                session::sign_in(res.token, res.user);
                nav.replace(Route::Home {});
            }
            Err(reelhouse_core::ClientError::Unauthorized) => {
                error.set(Some("That name and password don't match.".into()))
            }
            Err(e) => error.set(Some(describe(&e))),
        }
    };
    // TVs sign in by code; the password form stays as a fallback.
    let code_base = if ask_server {
        server()
    } else {
        client().base().to_string()
    };
    let show_code = cfg!(not(target_arch = "wasm32"))
        && code_base.len() > "http://".len()
        && code_base.starts_with("http");
    rsx! {
        div { class: "auth",
            h1 { class: "brand big", "Reelhouse" }
            div { class: "signin",
            if show_code {
                CodeSignIn { key: "{code_base}", base: code_base.clone() }
            }
            form { class: "auth-form", onsubmit: submit,
                if show_code {
                    h2 { "Or sign in with a password" }
                }
                if ask_server {
                    label { "Server address, e.g. http://192.168.1.10:8420"
                        input { r#type: "url", value: "{server}", oninput: move |e| server.set(e.value()) }
                    }
                    match &*found.read() {
                        None => rsx! { p { class: "quiet", "Looking for Reelhouse on your network…" } },
                        Some(list) if list.is_empty() => rsx! { p { class: "quiet", "No server found on this network. Type its address." } },
                        Some(list) if list.len() == 1 => rsx! { p { class: "quiet", "Found Reelhouse at {list[0]}" } },
                        Some(list) => rsx! {
                            div { class: "choices",
                                for base in list.clone() {
                                    button {
                                        r#type: "button",
                                        onclick: { let base = base.clone(); move |_| server.set(base.clone()) },
                                        "{base}"
                                    }
                                }
                            }
                        },
                    }
                }
                label { "Name"
                    input { autofocus: true, value: "{name}", oninput: move |e| name.set(e.value()) }
                }
                label { "Password"
                    input { r#type: "password", value: "{password}", oninput: move |e| password.set(e.value()) }
                }
                if let Some(msg) = error() {
                    p { class: "notice", "{msg}" }
                }
                button { class: "primary", r#type: "submit", "Sign in" }
            }
            }
            p { class: "aside", "New server? " Link { to: Route::Setup {}, "Create the admin account" } }
        }
    }
}

#[component]
pub fn Setup() -> Element {
    let mut name = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let nav = navigator();
    let submit = move |e: FormEvent| async move {
        e.prevent_default();
        let c = client();
        let new = NewUser {
            name: name(),
            password: password(),
            is_admin: true,
        };
        let result = match c.setup(&new).await {
            Ok(_) => c.login(&new.name, &new.password).await,
            Err(e) => Err(e),
        };
        match result {
            Ok(res) => {
                session::sign_in(res.token, res.user);
                nav.replace(Route::Admin {});
            }
            Err(reelhouse_core::ClientError::Status(409, _)) => error.set(Some(
                "This server already has an admin. Sign in instead.".into(),
            )),
            Err(e) => error.set(Some(describe(&e))),
        }
    };
    rsx! {
        div { class: "auth",
            h1 { class: "brand big", "Set up Reelhouse" }
            p { class: "aside", "This account manages libraries and users." }
            form { class: "auth-form", onsubmit: submit,
                label { "Name"
                    input { autofocus: true, value: "{name}", oninput: move |e| name.set(e.value()) }
                }
                label { "Password (8 characters or more)"
                    input { r#type: "password", value: "{password}", oninput: move |e| password.set(e.value()) }
                }
                if let Some(msg) = error() {
                    p { class: "notice", "{msg}" }
                }
                button { class: "primary", r#type: "submit", "Create admin account" }
            }
        }
    }
}

/// Native apps look for servers on the local network; the web app is served by its server.
async fn discover(ask: bool) -> Vec<String> {
    #[cfg(not(target_arch = "wasm32"))]
    if ask {
        return reelhouse_core::discover::discover().await;
    }
    let _ = ask;
    Vec::new()
}

/// Shows a sign-in code and QR for `base`, and signs in once someone approves it from a browser.
#[component]
fn CodeSignIn(base: String) -> Element {
    let nav = navigator();
    let mut attempt = use_signal(|| 0u32);
    let code = use_resource({
        let base = base.clone();
        move || {
            let base = base.clone();
            let _ = attempt();
            async move { reelhouse_core::Client::new(base).device_start().await }
        }
    });
    use_future({
        let base = base.clone();
        move || {
            let base = base.clone();
            async move {
                let mut waited = 0u32;
                loop {
                    futures_timer::Delay::new(std::time::Duration::from_secs(3)).await;
                    let Some(Ok(dc)) = code
                        .peek()
                        .as_ref()
                        .map(|r| r.as_ref().map(|d| d.poll_token.clone()))
                    else {
                        // No code (server unreachable or too old): ask again about every 9 seconds.
                        waited += 1;
                        if waited.is_multiple_of(3)
                            && code.peek().as_ref().is_some_and(|r| r.is_err())
                        {
                            attempt += 1;
                        }
                        continue;
                    };
                    match reelhouse_core::Client::new(base.clone())
                        .device_poll(&dc)
                        .await
                    {
                        Ok(Some(login)) => {
                            session::set_server(&base);
                            session::sign_in(login.token, login.user);
                            nav.replace(Route::Home {});
                            return;
                        }
                        Ok(None) => {}
                        // Expired or used up: show a fresh code.
                        Err(reelhouse_core::ClientError::Status(410, _)) => attempt += 1,
                        Err(_) => {}
                    }
                }
            }
        }
    });
    let link = |c: &str| format!("{}/link?code={c}", base.trim_end_matches('/'));
    match &*code.read() {
        None => {
            rsx! { section { class: "code-signin", p { class: "quiet", "Getting a sign-in code…" } } }
        }
        Some(Err(reelhouse_core::ClientError::Status(404 | 405, _))) => rsx! {
            section { class: "code-signin",
                p { class: "notice", "This server doesn't support sign-in by code yet. Update it, or sign in with a password." }
            }
        },
        Some(Err(e)) => rsx! {
            section { class: "code-signin",
                p { class: "notice", "Couldn't get a sign-in code from {base}: {describe(e)}" }
            }
        },
        Some(Ok(dc)) => rsx! {
            section { class: "code-signin",
                h2 { "Sign in with your phone" }
                p { class: "quiet", "Scan the code, or open {base}/link on a signed-in device and enter:" }
                p { class: "device-code", "{dc.code}" }
                div { class: "qr", dangerous_inner_html: qr_svg(&link(&dc.code)) }
            }
        },
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn qr_svg(data: &str) -> String {
    use qrcode::render::svg;
    qrcode::QrCode::new(data.as_bytes())
        .map(|q| {
            q.render::<svg::Color>()
                .min_dimensions(220, 220)
                .dark_color(svg::Color("#1e1320"))
                .light_color(svg::Color("#efe6dc"))
                .build()
        })
        .unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
fn qr_svg(_data: &str) -> String {
    String::new()
}
