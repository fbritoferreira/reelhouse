//! Android TV behaviour: the remote's Back button, leaving the app, the 10-foot layout class,
//! and putting focus on each screen's main action. No-ops on other platforms.

#[cfg(target_os = "android")]
use dioxus::prelude::*;

/// Calls `on_back` when the remote's Back button is pressed. The activity forwards Back to
/// `window.__rhBack` (android/MainActivity.kt); the most recently mounted screen owns it.
#[cfg(target_os = "android")]
pub fn use_back(on_back: impl FnMut() + 'static) {
    let mut on_back = on_back;
    let cb = use_callback(move |_: ()| on_back());
    use_future(move || async move {
        let mut eval = document::eval("window.__rhBack = () => dioxus.send(true);");
        while eval.recv::<bool>().await.is_ok() {
            cb.call(());
        }
    });
}

#[cfg(not(target_os = "android"))]
pub fn use_back(_on_back: impl FnMut() + 'static) {}

/// Closes the app, as Back does on the first screen.
#[cfg(target_os = "android")]
pub fn exit_app() {
    manganis::android::with_activity(|env, activity| {
        env.call_method(activity, "finish", "()V", &[])
            .ok()
            .map(|_| ())
    });
}

#[cfg(not(target_os = "android"))]
pub fn exit_app() {}

/// Marks the page as a TV screen (10-foot stylesheet) and installs D-pad focus movement.
pub fn use_tv_layout() {
    #[cfg(target_os = "android")]
    use_hook(|| {
        document::eval("document.documentElement.classList.add('tv');");
        document::eval(SPATIAL_NAVIGATION);
    });
}

/// One press moves focus to exactly one neighbour: Left/Right stay in the row when there is
/// one, Up/Down go to the nearest row, keeping roughly the same column. The focused element is
/// scrolled into view. Text fields keep Left/Right for the cursor.
#[cfg(target_os = "android")]
const SPATIAL_NAVIGATION: &str = r#"
(() => {
  if (window.__rhSpatial) return;
  window.__rhSpatial = true;
  const SELECTOR = 'a[href], button:not([disabled]), input, select, [tabindex]:not([tabindex="-1"])';
  const visible = el => { const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== 'hidden'; };
  const DIRS = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
  document.addEventListener('keydown', e => {
    const dir = DIRS[e.key];
    if (!dir) return;
    const cur = document.activeElement;
    if (cur && cur.tagName === 'INPUT' && dir[0] !== 0) return;
    // In the player, Left/Right seek; the player handles them.
    if (dir[0] !== 0 && document.querySelector('[data-rh-seek]')) return;
    const from = (cur && cur !== document.body) ? cur.getBoundingClientRect() : { left: 0, right: 0, top: 0, bottom: 0, width: 0, height: 0 };
    const cx = from.left + from.width / 2, cy = from.top + from.height / 2;
    let best = null, bestScore = Infinity;
    for (const el of document.querySelectorAll(SELECTOR)) {
      if (el === cur || !visible(el)) continue;
      const r = el.getBoundingClientRect();
      const ex = r.left + r.width / 2, ey = r.top + r.height / 2;
      let main, cross, overlap;
      if (dir[0] !== 0) {
        main = dir[0] > 0 ? r.left - from.right : from.left - r.right;
        cross = Math.abs(ey - cy);
        overlap = r.bottom > from.top + 2 && r.top < from.bottom - 2;
      } else {
        main = dir[1] > 0 ? r.top - from.bottom : from.top - r.bottom;
        cross = Math.abs(ex - cx);
        overlap = r.right > from.left + 2 && r.left < from.right - 2;
      }
      if (main < -2) continue;
      // Left/Right: stay in the row when possible. Up/Down: the nearest row first, then the
      // closest column, so Up from a row reaches the buttons above it, not the far corner.
      const score = dir[0] !== 0
        ? (overlap ? 0 : 100000) + Math.max(main, 0) * 10 + cross
        : Math.max(main, 0) * 10 + cross;
      if (score < bestScore) { bestScore = score; best = el; }
    }
    e.preventDefault();
    if (!best) return;
    best.focus({ preventScroll: true });
    best.scrollIntoView({ block: dir[1] !== 0 ? 'center' : 'nearest', inline: 'nearest', behavior: 'instant' });
  }, true);
})();
"#;

/// After a screen renders, focus its main action so the remote starts there, not on the top bar.
pub fn focus_main_action() {
    #[cfg(target_os = "android")]
    document::eval(
        "setTimeout(() => { const el = document.querySelector('.primary, [autofocus]'); if (el) el.focus(); }, 350);",
    );
}
