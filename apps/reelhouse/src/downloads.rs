//! Offline copies on native apps. Each download is the original file, fetched with HTTP range
//! requests so an interrupted download resumes, plus its poster and backdrop so the Downloads
//! screen works without the server. The list lives in `downloads.json` next to the files.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use reelhouse_api::Item;
use reelhouse_core::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Download {
    pub item: Item,
    pub bytes: u64,
    pub total: Option<u64>,
    pub done: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    NotDownloaded,
    /// Started earlier and stopped; `fraction` is how far it got.
    Paused {
        fraction: f64,
    },
    Running {
        fraction: f64,
    },
    Done,
    Failed(String),
}

/// Bytes written and total size so far, or why it failed.
type Progress = Result<(u64, Option<u64>), String>;

/// Progress of downloads running in this process: item id → (bytes, total) or an error.
fn running() -> &'static Mutex<HashMap<i64, Progress>> {
    static RUNNING: OnceLock<Mutex<HashMap<i64, Progress>>> = OnceLock::new();
    RUNNING.get_or_init(Default::default)
}

/// One writer at a time for `downloads.json`.
fn index_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Default::default)
}

fn base_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    let base = crate::player::files_dir()?;
    #[cfg(not(target_os = "android"))]
    let base = format!("{}/.reelhouse", std::env::var("HOME").ok()?);
    let dir = Path::new(&base).join("downloads");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn load_index(dir: &Path) -> Vec<Download> {
    fs::read(dir.join("downloads.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_index(dir: &Path, list: &[Download]) -> std::io::Result<()> {
    let tmp = dir.join("downloads.json.tmp");
    fs::write(&tmp, serde_json::to_vec(list)?)?;
    fs::rename(tmp, dir.join("downloads.json"))
}

fn update_index(dir: &Path, f: impl FnOnce(&mut Vec<Download>)) {
    let _guard = index_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut list = load_index(dir);
    f(&mut list);
    let _ = save_index(dir, &list);
}

fn media_path(dir: &Path, id: i64) -> PathBuf {
    dir.join(format!("{id}.media"))
}

fn art_path(dir: &Path, id: i64, kind: &str) -> PathBuf {
    dir.join(format!("{id}.{kind}"))
}

/// Total size from a `Content-Range` (`bytes 100-999/1000`), or from `Content-Length` plus
/// the bytes already on disk.
pub fn total_size(
    content_range: Option<&str>,
    content_length: Option<u64>,
    offset: u64,
) -> Option<u64> {
    content_range
        .and_then(|r| r.rsplit_once('/'))
        .and_then(|(_, total)| total.parse().ok())
        .or(content_length.map(|len| len + offset))
}

/// Finished and in-progress downloads, newest first.
pub fn list() -> Vec<Download> {
    let mut list = base_dir().map(|d| load_index(&d)).unwrap_or_default();
    list.reverse();
    list
}

pub fn state(id: i64) -> State {
    let fraction = |bytes: u64, total: Option<u64>| {
        total
            .filter(|t| *t > 0)
            .map(|t| bytes as f64 / t as f64)
            .unwrap_or(0.0)
    };
    if let Some(entry) = running().lock().unwrap_or_else(|e| e.into_inner()).get(&id) {
        return match entry {
            Ok((bytes, total)) => State::Running {
                fraction: fraction(*bytes, *total),
            },
            Err(e) => State::Failed(e.clone()),
        };
    }
    match list().into_iter().find(|d| d.item.id == id) {
        Some(d) if d.done => State::Done,
        Some(d) => State::Paused {
            fraction: fraction(d.bytes, d.total),
        },
        None => State::NotDownloaded,
    }
}

/// The downloaded file, if complete.
pub fn local_file(id: i64) -> Option<PathBuf> {
    let dir = base_dir()?;
    let done = load_index(&dir).iter().any(|d| d.item.id == id && d.done);
    let path = media_path(&dir, id);
    (done && path.exists()).then_some(path)
}

/// The downloaded item's details, for playing without the server.
pub fn item(id: i64) -> Option<Item> {
    list()
        .into_iter()
        .find(|d| d.item.id == id && d.done)
        .map(|d| d.item)
}

/// Stored artwork for a downloaded item, as (content type, bytes).
#[cfg(feature = "mobile")]
pub fn art(id: i64, kind: &str) -> Option<(String, Vec<u8>)> {
    let bytes = fs::read(art_path(&base_dir()?, id, kind)).ok()?;
    let content_type = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else {
        "image/jpeg"
    };
    Some((content_type.to_string(), bytes))
}

/// Starts (or resumes) downloading `item` in the background.
pub fn start(client: Client, item: Item) {
    let Some(dir) = base_dir() else { return };
    let id = item.id;
    {
        let mut map = running().lock().unwrap_or_else(|e| e.into_inner());
        if matches!(map.get(&id), Some(Ok(_))) {
            return;
        }
        map.insert(id, Ok((0, None)));
    }
    update_index(&dir, |list| {
        if !list.iter().any(|d| d.item.id == id) {
            list.push(Download {
                item: item.clone(),
                bytes: 0,
                total: None,
                done: false,
            });
        }
    });
    std::thread::spawn(move || {
        let result = fetch(&client, &dir, id);
        let mut map = running().lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(()) => {
                map.remove(&id);
                drop(map);
                save_art(&client, &dir, &item);
            }
            Err(e) => {
                map.insert(id, Err(e));
            }
        }
    });
}

fn fetch(client: &Client, dir: &Path, id: i64) -> Result<(), String> {
    let path = media_path(dir, id);
    let offset = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let mut req = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()
        .map_err(|e| e.to_string())?
        .get(format!("{}/api/v1/items/{id}/file", client.base()))
        .header("Range", format!("bytes={offset}-"));
    if let Some(token) = client.token() {
        req = req.bearer_auth(token);
    }
    let mut res = req
        .send()
        .map_err(|e| format!("couldn't reach the server: {e}"))?;
    let status = res.status().as_u16();
    if status == 416 {
        // Already complete on disk.
        update_index(dir, |list| mark_done(list, id, offset));
        return Ok(());
    }
    if status != 200 && status != 206 {
        return Err(format!("the server answered {status}"));
    }
    let header = |name: &str| {
        res.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let resumed = status == 206;
    let start = if resumed { offset } else { 0 };
    let total = total_size(
        header("content-range").as_deref(),
        header("content-length").and_then(|l| l.parse().ok()),
        start,
    );
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let mut written = start;
    let mut buf = vec![0u8; 256 * 1024];
    let mut since_save = 0u64;
    loop {
        let n = res
            .read(&mut buf)
            .map_err(|e| format!("download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("couldn't save: {e}"))?;
        written += n as u64;
        since_save += n as u64;
        running()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Ok((written, total)));
        // Record progress now and then, so a restart shows how far it got.
        if since_save > 32 * 1024 * 1024 {
            since_save = 0;
            update_index(dir, |list| {
                if let Some(d) = list.iter_mut().find(|d| d.item.id == id) {
                    d.bytes = written;
                    d.total = total;
                }
            });
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    if total.is_some_and(|t| written < t) {
        return Err("download stopped early; it will resume where it left off".into());
    }
    update_index(dir, |list| {
        mark_done(list, id, written);
        if let Some(d) = list.iter_mut().find(|d| d.item.id == id) {
            d.total = total.or(Some(written));
        }
    });
    Ok(())
}

fn mark_done(list: &mut [Download], id: i64, bytes: u64) {
    if let Some(d) = list.iter_mut().find(|d| d.item.id == id) {
        d.bytes = bytes;
        d.done = true;
    }
}

fn save_art(client: &Client, dir: &Path, item: &Item) {
    for (kind, url) in [
        ("poster", &item.poster_url),
        ("backdrop", &item.backdrop_url),
    ] {
        let Some(url) = url else { continue };
        let url = if url.starts_with('/') {
            format!("{}{url}", client.base())
        } else {
            url.clone()
        };
        let mut req = reqwest::blocking::Client::new().get(url);
        if let Some(token) = client.token() {
            req = req.bearer_auth(token);
        }
        if let Some(bytes) = req
            .send()
            .ok()
            .and_then(|r| r.error_for_status().ok())
            .and_then(|r| r.bytes().ok())
        {
            let _ = fs::write(art_path(dir, item.id, kind), &bytes);
        }
    }
}

/// Deletes the file, its artwork and its entry.
pub fn remove(id: i64) {
    let Some(dir) = base_dir() else { return };
    running()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    for path in [
        media_path(&dir, id),
        art_path(&dir, id, "poster"),
        art_path(&dir, id, "backdrop"),
    ] {
        let _ = fs::remove_file(path);
    }
    update_index(&dir, |list| list.retain(|d| d.item.id != id));
}

#[cfg(test)]
mod tests {
    use super::*;
    use reelhouse_api::ItemKind;

    fn item(id: i64) -> Item {
        Item {
            id,
            kind: ItemKind::Movie,
            parent_id: None,
            title: "Heat".into(),
            year: Some(1995),
            season_no: None,
            episode_no: None,
            duration_s: Some(10_200.0),
            overview: None,
            poster_url: None,
            backdrop_url: None,
            progress: None,
            show_id: None,
            show_title: None,
        }
    }

    #[test]
    fn total_from_content_range_or_length() {
        assert_eq!(
            total_size(Some("bytes 100-999/1000"), Some(900), 100),
            Some(1000)
        );
        assert_eq!(total_size(None, Some(900), 100), Some(1000));
        assert_eq!(total_size(Some("bytes */1000"), None, 0), Some(1000));
        assert_eq!(total_size(None, None, 0), None);
    }

    #[test]
    fn index_round_trips() {
        let dir = std::env::temp_dir().join(format!("rh-downloads-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let list = vec![Download {
            item: item(7),
            bytes: 42,
            total: Some(100),
            done: false,
        }];
        save_index(&dir, &list).unwrap();
        assert_eq!(load_index(&dir), list);
        update_index(&dir, |l| mark_done(l, 7, 100));
        assert!(load_index(&dir)[0].done);
        fs::remove_dir_all(&dir).unwrap();
    }
}
