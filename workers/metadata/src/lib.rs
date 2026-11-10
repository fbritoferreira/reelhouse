//! Reelhouse metadata proxy: answers the TMDB lookups Reelhouse servers make, using one shared key
//! held as a Worker secret, and caches every answer at Cloudflare's edge.

mod route;

use worker::*;

const TMDB: &str = "https://api.themoviedb.org";

#[event(fetch)]
async fn fetch(req: Request, env: Env, ctx: Context) -> Result<Response> {
    if req.method() != Method::Get {
        return Response::error("method not allowed", 405);
    }
    let url = req.url()?;
    let Some((upstream, ttl)) = route::route(url.path(), url.query().unwrap_or("")) else {
        return Response::error("not found", 404);
    };

    // The cache key is the normalised upstream path, without the key.
    let cache = Cache::default();
    let cache_key = format!("https://metadata-cache.reelhouse{upstream}");
    if let Some(hit) = cache.get(cache_key.as_str(), false).await? {
        return Ok(hit);
    }

    let key = env.secret("TMDB_API_KEY")?.to_string();
    let sep = if upstream.contains('?') { '&' } else { '?' };
    let mut res = Fetch::Url(format!("{TMDB}{upstream}{sep}api_key={key}").parse()?)
        .send()
        .await?;
    let status = res.status_code();
    let body = res.bytes().await?;
    let headers = Headers::new();
    headers.set("content-type", "application/json;charset=utf-8")?;
    // Only successful answers and "not found" are cached; TMDB errors and rate limits are not.
    let cacheable = status == 200 || status == 404;
    if cacheable {
        headers.set("cache-control", &format!("public, max-age={ttl}"))?;
    }
    let mut response = Response::from_bytes(body)?
        .with_status(status)
        .with_headers(headers);
    if cacheable {
        let copy = response.cloned()?;
        ctx.wait_until(async move {
            let _ = Cache::default().put(cache_key.as_str(), copy).await;
        });
    }
    Ok(response)
}
