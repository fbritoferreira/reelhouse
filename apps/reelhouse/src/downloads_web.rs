//! Downloads in the browser, with the same calls as the native module so the rings, the
//! Downloads screen and offline playback work unchanged. Files live in the origin's private file
//! system (OPFS) as `dl-<id>`; the index (what is kept, how far each got) in local storage.
//!
//! A small JavaScript engine does the fetching, one download at a time: it writes chunks as they
//! arrive, resumes from the bytes it has when the server allows ranges, and keeps the index's
//! progress up to date. Titles the browser cannot play as they are come as an MP4 the server
//! remuxes for it (see `GET /api/v1/items/{id}/offline`).

use dioxus::prelude::*;
use reelhouse_api::Item;
use reelhouse_core::Client;
use serde::{Deserialize, Serialize};

const INDEX: &str = "reelhouse.downloads";

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
    /// Waiting for the downloads ahead of it.
    Queued,
    Running {
        fraction: f64,
    },
    Done,
    Failed(String),
}

/// The engine, installed once per page.
const ENGINE: &str = r#"
(() => {
  if (window.__rhDl) return;
  const KEY = 'reelhouse.downloads';
  const live = window.__rhDl = {};
  const queue = [];
  let busy = false;
  let lastSave = 0;
  const save = (id, bytes, total, done, force) => {
    const now = Date.now();
    if (!force && now - lastSave < 1000) return;
    lastSave = now;
    const list = JSON.parse(localStorage.getItem(KEY) || '[]');
    const d = list.find(d => d.item.id === id);
    if (d) { d.bytes = bytes; d.total = total; d.done = done; localStorage.setItem(KEY, JSON.stringify(list)); }
  };
  const fetchOne = async ({ id, url, resumable }) => {
    const st = live[id] = { status: 'running', bytes: 0, total: null };
    let w;
    try {
      const root = await navigator.storage.getDirectory();
      const fh = await root.getFileHandle('dl-' + id, { create: true });
      let offset = resumable ? (await fh.getFile()).size : 0;
      w = await fh.createWritable({ keepExistingData: offset > 0 });
      const res = await fetch(url, { headers: offset > 0 ? { Range: 'bytes=' + offset + '-' } : {} });
      if (res.status !== 200 && res.status !== 206) throw new Error('the server answered ' + res.status);
      if (res.status === 200) { offset = 0; await w.truncate(0); }
      await w.seek(offset);
      const range = res.headers.get('content-range');
      const length = res.headers.get('content-length');
      st.total = range && range.includes('/') ? Number(range.split('/')[1])
        : length ? offset + Number(length) : null;
      st.bytes = offset;
      const reader = res.body.getReader();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        await w.write(value);
        st.bytes += value.length;
        save(id, st.bytes, st.total, false, false);
      }
      await w.close();
      save(id, st.bytes, st.bytes, true, true);
      delete live[id];
    } catch (e) {
      try { if (w) await w.close(); } catch (_) {}
      st.status = 'failed';
      st.error = String((e && e.message) || e);
      save(id, st.bytes, st.total, false, true);
    }
  };
  const run = async () => {
    if (busy) return;
    busy = true;
    while (queue.length) await fetchOne(queue.shift());
    busy = false;
  };
  window.__rhDlQueue = job => {
    if (queue.some(q => q.id === job.id) || (live[job.id] && live[job.id].status === 'running')) return;
    live[job.id] = { status: 'queued', bytes: 0, total: null };
    queue.push(job);
    run();
  };
  window.__rhDlRemove = id => {
    const i = queue.findIndex(q => q.id === id);
    if (i >= 0) queue.splice(i, 1);
    delete live[id];
    navigator.storage.getDirectory().then(r => r.removeEntry('dl-' + id)).catch(() => {});
  };
})()
"#;

fn engine() {
    let _ = js_sys::eval(ENGINE);
}

fn index() -> Vec<Download> {
    crate::session::load(INDEX)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_index(list: &[Download]) {
    if let Ok(json) = serde_json::to_string(list) {
        crate::session::save(INDEX, Some(&json));
    }
}

/// What the engine is doing with `id` right now, if anything.
#[derive(Deserialize)]
struct Live {
    status: String,
    bytes: u64,
    total: Option<u64>,
    #[serde(default)]
    error: Option<String>,
}

fn live(id: i64) -> Option<Live> {
    engine();
    js_sys::eval(&format!("JSON.stringify(window.__rhDl[{id}] || null)"))
        .ok()?
        .as_string()
        .and_then(|s| serde_json::from_str(&s).ok())
        .flatten()
}

fn fraction(bytes: u64, total: Option<u64>) -> f64 {
    total
        .filter(|t| *t > 0)
        .map_or(0.0, |t| (bytes as f64 / t as f64).clamp(0.0, 1.0))
}

pub fn list() -> Vec<Download> {
    let mut list = index();
    list.reverse();
    list
}

pub fn state(id: i64) -> State {
    if let Some(l) = live(id) {
        return match l.status.as_str() {
            "queued" => State::Queued,
            "failed" => State::Failed(l.error.unwrap_or_else(|| "the download stopped".into())),
            _ => State::Running {
                fraction: fraction(l.bytes, l.total),
            },
        };
    }
    match index().into_iter().find(|d| d.item.id == id) {
        Some(d) if d.done => State::Done,
        Some(d) => State::Paused {
            fraction: fraction(d.bytes, d.total),
        },
        None => State::NotDownloaded,
    }
}

pub fn item(id: i64) -> Option<Item> {
    index()
        .into_iter()
        .find(|d| d.item.id == id && d.done)
        .map(|d| d.item)
}

/// Adds `item` to the queue (or resumes it), unless it is kept already or under way.
pub fn start(client: Client, item: Item) {
    let id = item.id;
    if matches!(
        state(id),
        State::Done | State::Queued | State::Running { .. }
    ) {
        return;
    }
    let mut list = index();
    if !list.iter().any(|d| d.item.id == id) {
        list.push(Download {
            item,
            bytes: 0,
            total: None,
            done: false,
        });
        save_index(&list);
    }
    spawn(async move {
        let source = client
            .offline_source(id, &crate::player::capabilities())
            .await;
        let Ok(source) = source else { return };
        let url = client.media_url(&source.url);
        engine();
        let job = serde_json::json!({ "id": id, "url": url, "resumable": source.resumable });
        let _ = js_sys::eval(&format!("window.__rhDlQueue({job})"));
    });
}

pub fn start_all(client: Client, items: Vec<Item>) {
    for item in items {
        start(client.clone(), item);
    }
}

/// Picks up downloads cut short when the page last closed.
pub fn resume_unfinished(client: Client) {
    for d in index().into_iter().filter(|d| !d.done) {
        start(client.clone(), d.item);
    }
}

/// How many of `ids` are kept, and whether any are queued or downloading.
pub fn summary(ids: &[i64]) -> (usize, bool) {
    let mut done = 0;
    let mut busy = false;
    for id in ids {
        match state(*id) {
            State::Done => done += 1,
            State::Queued | State::Running { .. } => busy = true,
            _ => {}
        }
    }
    (done, busy)
}

pub fn remove(id: i64) {
    engine();
    let _ = js_sys::eval(&format!("window.__rhDlRemove({id})"));
    let mut list = index();
    list.retain(|d| d.item.id != id);
    save_index(&list);
}

/// The URL the web player opens for a kept copy: `opfs:<id>`, resolved in the player.
pub fn local_url(id: i64) -> Option<String> {
    index()
        .iter()
        .any(|d| d.item.id == id && d.done)
        .then(|| format!("opfs:{id}"))
}

#[cfg(test)]
mod tests {
    use super::fraction;

    #[test]
    fn fraction_needs_a_total() {
        assert_eq!(fraction(50, Some(200)), 0.25);
        assert_eq!(fraction(50, None), 0.0);
        assert_eq!(fraction(500, Some(200)), 1.0);
    }
}
