//! Watch positions the server did not get, mostly from playing a downloaded copy offline. They are
//! kept on the device, used to resume, and sent once the server answers again.

use std::collections::BTreeMap;

use reelhouse_api::Progress;
use reelhouse_core::Client;

const KEY: &str = "reelhouse.unsent_progress";

/// `m:<id>` for library titles, `v:<id>` for on-demand ones; the position in seconds.
type Unsent = BTreeMap<String, f64>;

fn key(id: i64, on_demand: bool) -> String {
    format!("{}:{id}", if on_demand { "v" } else { "m" })
}

fn parse(key: &str) -> Option<(i64, bool)> {
    let (kind, id) = key.split_once(':')?;
    Some((id.parse().ok()?, kind == "v"))
}

fn read() -> Unsent {
    crate::session::load(KEY)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write(list: &Unsent) {
    let json = serde_json::to_string(list).ok();
    crate::session::save(KEY, json.as_deref().filter(|_| !list.is_empty()));
}

/// The position kept for a title, newer than the server's if there is one.
pub fn position(id: i64, on_demand: bool) -> Option<f64> {
    read().get(&key(id, on_demand)).copied()
}

/// Saves a title's position on the server, or keeps it here when the server is out of reach.
/// A save that gets through also sends whatever was kept.
pub async fn save(client: &Client, id: i64, on_demand: bool, position_s: f64) {
    if put(client, id, on_demand, position_s).await {
        let mut list = read();
        if list.remove(&key(id, on_demand)).is_some() {
            write(&list);
        }
        send(client).await;
    } else {
        let mut list = read();
        list.insert(key(id, on_demand), position_s);
        write(&list);
    }
}

/// Sends every kept position; those the server still does not take stay.
pub async fn send(client: &Client) {
    let list = read();
    if list.is_empty() {
        return;
    }
    let mut left = Unsent::new();
    for (k, pos) in list {
        let Some((id, on_demand)) = parse(&k) else {
            continue;
        };
        if !put(client, id, on_demand, pos).await {
            left.insert(k, pos);
        }
    }
    write(&left);
}

/// Whether the server got it. One that answered with an error (the title is gone, say) counts as
/// got: only a server out of reach, or a session to sign back into, is worth trying again.
async fn put(client: &Client, id: i64, on_demand: bool, position_s: f64) -> bool {
    let p = Progress {
        position_s,
        played: false,
    };
    let result = if on_demand {
        client.put_vod_progress(id, &p).await
    } else {
        client.put_progress(id, &p).await
    };
    !matches!(
        result,
        Err(reelhouse_core::ClientError::Network(_) | reelhouse_core::ClientError::Unauthorized)
    )
}

#[cfg(test)]
mod tests {
    use super::{key, parse};

    #[test]
    fn keys_round_trip() {
        assert_eq!(key(42, false), "m:42");
        assert_eq!(key(7, true), "v:7");
        assert_eq!(parse("m:42"), Some((42, false)));
        assert_eq!(parse("v:7"), Some((7, true)));
        assert_eq!(parse("v:x"), None);
        assert_eq!(parse("nonsense"), None);
    }
}
