//! Extended M3U playlists (`#EXTINF` lines with `tvg-*` attributes).

use super::SourceChannel;
use crate::scan::naming::{clean, season_episode_at, year_of};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VodKind {
    Movie,
    Episode,
}

/// A film or a series episode a playlist offers on demand.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceVod {
    pub kind: VodKind,
    /// The film's title, or the episode's own name.
    pub title: String,
    pub year: Option<u16>,
    pub show: Option<String>,
    pub season: Option<u16>,
    pub episode: Option<u16>,
    pub poster_url: Option<String>,
    pub group_title: Option<String>,
    pub stream_url: String,
}

/// Splits an on-demand entry's name. Providers write episodes as `Show S01E03 - Name` and
/// often repeat the show and season first: `Show S01 Show - S01E03/04 - Name`.
fn vod_name(
    kind: VodKind,
    name: &str,
) -> (
    String,
    Option<u16>,
    Option<String>,
    Option<u16>,
    Option<u16>,
) {
    if kind == VodKind::Movie {
        return match year_of(name) {
            Some((year, at)) => (clean(&name[..at]), Some(year), None, None, None),
            None => (clean(name), None, None, None, None),
        };
    }
    let Some((season, episode, at)) = season_episode_at(name) else {
        return (clean(name), None, Some(clean(name)), None, None);
    };
    // The show is what precedes the first season marker, `S01 ` or `S01E03`.
    let lower = name.to_ascii_lowercase();
    let first_season = lower
        .match_indices(&format!("s{season:02}"))
        .map(|(i, _)| i)
        .find(|&i| i == 0 || !lower.as_bytes()[i - 1].is_ascii_alphanumeric())
        .unwrap_or(at.start);
    let show = clean(&name[..first_season.min(at.start)]);
    // After the marker: an optional `/04` (a double episode), then ` - Name`.
    let rest = name[at.end..].trim_start_matches(|c: char| c == '/' || c.is_ascii_digit());
    let title = clean(rest);
    let title = if title.is_empty() {
        format!("Episode {episode}")
    } else {
        title
    };
    (title, None, Some(show), Some(season), Some(episode))
}

/// Films and series episodes in a playlist; live channels are left to [`parse`].
pub fn parse_vod(text: &str) -> Vec<SourceVod> {
    let mut out = Vec::new();
    let mut pending: Option<(Vec<(String, String)>, String)> = None;
    for line in text.lines().map(str::trim) {
        if let Some(body) = line.strip_prefix("#EXTINF:") {
            let (attrs, name) = extinf(body);
            let attrs = attrs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            pending = Some((attrs, name.to_string()));
        } else if !line.is_empty()
            && !line.starts_with('#')
            && let Some((attrs, name)) = pending.take()
            && is_on_demand(line)
        {
            let attr = |k: &str| {
                attrs
                    .iter()
                    .find(|(key, v)| key == k && !v.is_empty())
                    .map(|(_, v)| v.clone())
            };
            let name = if name.is_empty() {
                attr("tvg-name").unwrap_or_default()
            } else {
                name
            };
            let lower = line.to_ascii_lowercase();
            let kind = if lower.contains("/series/") || season_episode_at(&name).is_some() {
                VodKind::Episode
            } else {
                VodKind::Movie
            };
            let (title, year, show, season, episode) = vod_name(kind, &name);
            out.push(SourceVod {
                kind,
                title,
                year,
                show,
                season,
                episode,
                poster_url: attr("tvg-logo"),
                group_title: attr("group-title"),
                stream_url: line.to_string(),
            });
        }
    }
    out
}

/// The `key="value"` attributes and the display name of one `#EXTINF:` line body.
fn extinf(body: &str) -> (Vec<(&str, &str)>, &str) {
    let mut attrs = Vec::new();
    let mut rest = body;
    loop {
        // The name follows the first comma outside quotes.
        let comma = rest.find(',').unwrap_or(rest.len());
        let Some(eq) = rest[..comma].find("=\"") else {
            return (attrs, rest.get(comma + 1..).unwrap_or("").trim());
        };
        let key = rest[..eq].rsplit(' ').next().unwrap_or("");
        let value_start = eq + 2;
        let Some(len) = rest[value_start..].find('"') else {
            return (attrs, "");
        };
        attrs.push((key, &rest[value_start..value_start + len]));
        rest = &rest[value_start + len + 1..];
    }
}

/// The guide a playlist names in its `#EXTM3U` header (`url-tvg` or `x-tvg-url`).
pub fn epg_url(text: &str) -> Option<String> {
    let header = text.lines().next()?.trim().strip_prefix("#EXTM3U")?;
    let (attrs, _) = extinf(header);
    attrs
        .iter()
        .find(|(k, v)| (*k == "url-tvg" || *k == "x-tvg-url") && !v.is_empty())
        .map(|(_, v)| v.to_string())
}

/// Films and series that providers mix into their playlists; they are not channels.
fn is_on_demand(url: &str) -> bool {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();
    path.contains("/movie/")
        || path.contains("/series/")
        || [".mp4", ".mkv", ".avi", ".m4v", ".mov"]
            .iter()
            .any(|ext| path.ends_with(ext))
}

pub fn parse(text: &str) -> Vec<SourceChannel> {
    let mut out = Vec::new();
    let mut pending: Option<SourceChannel> = None;
    for line in text.lines().map(str::trim) {
        if let Some(body) = line.strip_prefix("#EXTINF:") {
            let (attrs, name) = extinf(body);
            let attr = |k: &str| {
                attrs
                    .iter()
                    .find(|(key, v)| *key == k && !v.is_empty())
                    .map(|(_, v)| v.to_string())
            };
            pending = Some(SourceChannel {
                number: attr("tvg-chno").and_then(|n| n.parse().ok()),
                name: if name.is_empty() {
                    attr("tvg-name").unwrap_or_default()
                } else {
                    name.to_string()
                },
                logo_url: attr("tvg-logo"),
                stream_url: String::new(),
                epg_channel_id: attr("tvg-id"),
                group_title: attr("group-title"),
            });
        } else if !line.is_empty()
            && !line.starts_with('#')
            && let Some(mut c) = pending.take()
            && !is_on_demand(line)
        {
            c.stream_url = line.to_string();
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYLIST: &str = r#"#EXTM3U url-tvg="http://example.com/epg.xml"
#EXTINF:-1 tvg-id="bbc1.uk" tvg-name="BBC One" tvg-logo="http://logo/bbc1.png" group-title="UK, General" tvg-chno="101",BBC One HD
#EXTVLCOPT:http-user-agent=VLC
http://stream/bbc1.ts

#EXTINF:-1,Plain Channel
http://stream/plain.m3u8
#EXTINF:-1 tvg-id="orphan",No URL follows
"#;

    #[test]
    fn parses_attributes_and_name() {
        let chans = parse(PLAYLIST);
        assert_eq!(chans.len(), 2);
        assert_eq!(
            chans[0],
            SourceChannel {
                number: Some(101),
                name: "BBC One HD".into(),
                logo_url: Some("http://logo/bbc1.png".into()),
                stream_url: "http://stream/bbc1.ts".into(),
                epg_channel_id: Some("bbc1.uk".into()),
                group_title: Some("UK, General".into()),
            }
        );
    }

    #[test]
    fn channel_without_attributes() {
        let chans = parse(PLAYLIST);
        assert_eq!(chans[1].name, "Plain Channel");
        assert_eq!(chans[1].stream_url, "http://stream/plain.m3u8");
        assert_eq!(chans[1].epg_channel_id, None);
        assert_eq!(chans[1].number, None);
    }

    #[test]
    fn falls_back_to_tvg_name() {
        let chans = parse("#EXTINF:-1 tvg-name=\"Named\",\nhttp://s/1\r\n");
        assert_eq!(chans[0].name, "Named");
        assert_eq!(chans[0].stream_url, "http://s/1");
    }

    #[test]
    fn skips_films_and_series() {
        let chans = parse(
            "#EXTINF:-1,Live\nhttp://h/live/u/p/1\n\
             #EXTINF:-1,Film\nhttp://h/movie/u/p/2.mp4\n\
             #EXTINF:-1,Episode\nhttp://h/series/u/p/3.mkv\n\
             #EXTINF:-1,Bare file\nhttp://h/u/p/4.avi\n",
        );
        let names: Vec<&str> = chans.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Live"]);
    }

    #[test]
    fn reads_the_guide_url_from_the_header() {
        assert_eq!(
            epg_url(PLAYLIST).as_deref(),
            Some("http://example.com/epg.xml")
        );
        assert_eq!(
            epg_url("#EXTM3U x-tvg-url=\"http://g/x.xml\"\n").as_deref(),
            Some("http://g/x.xml")
        );
        assert_eq!(epg_url("#EXTM3U\n"), None);
    }

    #[test]
    fn films_and_episodes_become_on_demand() {
        let vod = parse_vod(
            "#EXTINF:-1 tvg-logo=\"http://p/re.jpg\" group-title=\"Horror\",Resident Evil (2026)\n\
             http://h/movie/u/p/1.mp4\n\
             #EXTINF:-1 group-title=\"IT | Crime\",Viola Come Il Mare (IT) S01 Viola Come Il Mare (IT) - S01E03/04 - Blu come il pregiudizio\n\
             http://h/series/u/p/2.mkv\n\
             #EXTINF:-1,Live channel\nhttp://h/live/u/p/3\n",
        );
        assert_eq!(vod.len(), 2);
        assert_eq!(
            vod[0],
            SourceVod {
                kind: VodKind::Movie,
                title: "Resident Evil".into(),
                year: Some(2026),
                show: None,
                season: None,
                episode: None,
                poster_url: Some("http://p/re.jpg".into()),
                group_title: Some("Horror".into()),
                stream_url: "http://h/movie/u/p/1.mp4".into(),
            }
        );
        assert_eq!(vod[1].kind, VodKind::Episode);
        assert_eq!(vod[1].show.as_deref(), Some("Viola Come Il Mare (IT)"));
        assert_eq!((vod[1].season, vod[1].episode), (Some(1), Some(3)));
        assert_eq!(vod[1].title, "Blu come il pregiudizio");
    }

    #[test]
    fn plain_episode_names() {
        let vod = parse_vod("#EXTINF:-1,The Wire S02E05\nhttp://h/series/a/b/9.mp4\n");
        assert_eq!(vod[0].show.as_deref(), Some("The Wire"));
        assert_eq!((vod[0].season, vod[0].episode), (Some(2), Some(5)));
        assert_eq!(vod[0].title, "Episode 5");
    }
}
