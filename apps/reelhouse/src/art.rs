//! Native apps load artwork through the app's own origin. Their webview page is served over
//! HTTPS (`https://dioxus.index.html`), so `http://` images from a LAN server are blocked as
//! mixed content. `/art/<id>/<poster|backdrop>` is answered in Rust instead, which fetches the
//! image with the session token in a header, keeping the token out of URLs and system logs.

/// `/api/v1/items/5/poster` → `/art/5/poster`.
pub fn local_path(api_path: &str) -> Option<String> {
    let rest = api_path.strip_prefix("/api/v1/items/")?;
    parse(rest).map(|(id, kind)| format!("/art/{id}/{kind}"))
}

/// `/art/5/poster` → `/api/v1/items/5/poster`. Anything else is refused, so the handler
/// cannot be used to reach other server paths.
#[cfg(any(feature = "mobile", test))]
pub fn api_path(local: &str) -> Option<String> {
    let rest = local.strip_prefix("/art/")?;
    parse(rest).map(|(id, kind)| format!("/api/v1/items/{id}/{kind}"))
}

fn parse(rest: &str) -> Option<(u64, &str)> {
    let (id, kind) = rest.split_once('/')?;
    let kind = kind.split(['?', '#']).next()?;
    matches!(kind, "poster" | "backdrop").then_some(())?;
    Some((id.parse().ok()?, kind))
}

/// Answers `/art/...` requests from the webview.
#[cfg(feature = "mobile")]
pub fn use_art_handler() {
    use dioxus::mobile::wry::http::Response;
    use dioxus::prelude::*;

    let session = crate::session::use_session();
    dioxus::mobile::use_asset_handler("art", move |request, responder| {
        let client = session.peek().client.clone();
        let path = request.uri().path().to_string();
        std::thread::spawn(move || {
            let fetched = api_path(&path).and_then(|api| {
                let mut req =
                    reqwest::blocking::Client::new().get(format!("{}{api}", client.base()));
                if let Some(token) = client.token() {
                    req = req.bearer_auth(token);
                }
                let res = req.send().ok()?.error_for_status().ok()?;
                let content_type = res
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("image/jpeg")
                    .to_string();
                Some((content_type, res.bytes().ok()?.to_vec()))
            });
            let response = match fetched {
                Some((content_type, bytes)) => Response::builder()
                    .header("Content-Type", content_type)
                    .header("Cache-Control", "private, max-age=604800")
                    .body(bytes),
                None => Response::builder().status(404).body(Vec::new()),
            };
            if let Ok(response) = response {
                responder.respond(response);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_artwork_both_ways() {
        assert_eq!(
            local_path("/api/v1/items/5/poster").as_deref(),
            Some("/art/5/poster")
        );
        assert_eq!(
            api_path("/art/5/backdrop").as_deref(),
            Some("/api/v1/items/5/backdrop")
        );
    }

    #[test]
    fn refuses_anything_but_artwork() {
        assert_eq!(api_path("/art/5/file"), None);
        assert_eq!(api_path("/art/../users"), None);
        assert_eq!(api_path("/art/abc/poster"), None);
        assert_eq!(api_path("/other/5/poster"), None);
        assert_eq!(local_path("/api/v1/items/5/file"), None);
    }
}
