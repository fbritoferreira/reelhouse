//! Which TMDB requests the proxy answers, and for how long the edge may cache each one.

const DAY: u32 = 86_400;

/// Query parameters passed through to TMDB; everything else (including any `api_key`) is dropped.
const ALLOWED_PARAMS: &[&str] = &["query", "year", "first_air_date_year", "language", "page"];

/// Maps a request path and query to the TMDB path and query to fetch, and its cache lifetime in
/// seconds. `None` for anything Reelhouse does not use, so the Worker is not an open TMDB proxy.
pub fn route(path: &str, query: &str) -> Option<(String, u32)> {
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let id = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let ttl = match parts.as_slice() {
        ["3", "search", "movie" | "tv"] => DAY,
        ["3", "movie" | "tv", n, "external_ids"] if id(n) => 7 * DAY,
        ["3", "tv", n, "season", s, "episode", e] if id(n) && id(s) && id(e) => 7 * DAY,
        _ => return None,
    };
    let mut params: Vec<(&str, &str)> = query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .filter(|(k, _)| ALLOWED_PARAMS.contains(k))
        .collect();
    // Sorted so equivalent requests share one cache entry.
    params.sort();
    let q: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let upstream = if q.is_empty() {
        format!("/{}", parts.join("/"))
    } else {
        format!("/{}?{}", parts.join("/"), q.join("&"))
    };
    Some((upstream, ttl))
}

#[cfg(test)]
mod tests {
    use super::route;

    #[test]
    fn search_keeps_query_and_drops_api_key() {
        assert_eq!(
            route(
                "/3/search/movie",
                "year=1982&api_key=stolen&query=Blade%20Runner"
            ),
            Some((
                "/3/search/movie?query=Blade%20Runner&year=1982".into(),
                86_400
            ))
        );
    }

    #[test]
    fn equivalent_queries_share_a_cache_key() {
        assert_eq!(
            route("/3/search/tv", "query=Bear&language=en").unwrap().0,
            route("/3/search/tv", "language=en&query=Bear").unwrap().0
        );
    }

    #[test]
    fn details_cache_for_a_week() {
        assert_eq!(
            route("/3/movie/78/external_ids", ""),
            Some(("/3/movie/78/external_ids".into(), 604_800))
        );
        assert_eq!(
            route("/3/tv/136315/season/2/episode/7", ""),
            Some(("/3/tv/136315/season/2/episode/7".into(), 604_800))
        );
    }

    #[test]
    fn rejects_anything_else() {
        assert_eq!(route("/3/movie/popular", ""), None);
        assert_eq!(route("/3/account", ""), None);
        assert_eq!(route("/3/tv/abc/external_ids", ""), None);
        assert_eq!(route("/4/list/1", ""), None);
    }
}
