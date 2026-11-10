# Reelhouse

An open-source media server written in Rust, with clients for iOS, Android, Android TV / Google TV, Fire TV, Apple TV, web and desktop.

Status: early design. Nothing usable yet.

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
