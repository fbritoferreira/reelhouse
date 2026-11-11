# Reelhouse

An open-source media server written in Rust, with clients for iOS, Android, Android TV / Google TV, Fire TV, Apple TV, web and desktop.

Status: early, in daily use on one home server. The server, the web app and the Google TV app work; the other clients are not started.

What works today:

- **Libraries.** Movies and TV folders, matched to TMDB for artwork and details. Resolution badges, watched marks, Continue watching and Recently added.
- **Playback.** Direct play where the device can, otherwise HLS remuxed or converted by ffmpeg. Audio and subtitle choice, Up next, resume. The web player has full screen and YouTube's keyboard shortcuts; the TV app answers to the remote's media keys.
- **Downloads.** A title, a season or a whole show, queued, resumable, and playable offline on Google TV and in the browser.
- **Live TV.** M3U and Xtream sources refreshed every 12 hours, a channel guide, channels the provider refuses marked as such, record once or every episode, and the provider's films and series on demand.

Goals:

- Plugins as a first-class, stable API (WASM).
- Offline downloads that actually work.
- Live TV and DVR from M3U / Xtream IPTV sources, treated as a normal library.
- One shared Rust core and one monorepo for server and all clients.

## Layout

```
crates/reelhouse-server   the media server (binary)
crates/reelhouse-api      HTTP API types shared by server and clients
crates/reelhouse-core     client logic shared by every app
crates/reelhouse-ui       shared Dioxus components
apps/reelhouse            the app for web, Android and Google TV
crates/reelhouse          reserved crates.io name
```

## Running it

```sh
docker run -d --name reelhouse -p 8420:8420 \
  -v reelhouse-data:/data -v /path/to/movies:/movies:ro -v /path/to/tv:/tv:ro \
  -e TMDB_API_KEY=... ghcr.io/fbritoferreira/reelhouse:latest
```

Open `http://<host>:8420`, create the admin account, then add `/movies` and `/tv` as libraries.

Images: every merge to `main` publishes `ghcr.io/fbritoferreira/reelhouse:sha-<commit>` and `:main`. Releases are cut by release-please from [Conventional Commits](https://www.conventionalcommits.org) (PR titles such as `feat: ...` or `fix: ...`); a release tags the existing image for that commit as `:<version>`, `:<major>.<minor>` and `:latest`.

## License

MIT. See [LICENSE](LICENSE).
