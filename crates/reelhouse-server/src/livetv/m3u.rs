//! Extended M3U playlists (`#EXTINF` lines with `tvg-*` attributes).

use super::SourceChannel;

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
}
