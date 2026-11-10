//! XMLTV guides: `<channel>` and `<programme start stop channel>` elements.

use anyhow::Result;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuideChannel {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuideProgramme {
    pub channel: String,
    pub start: i64,
    pub stop: i64,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub series_key: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Guide {
    pub channels: Vec<GuideChannel>,
    pub programmes: Vec<GuideProgramme>,
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The (year, month, day) of a day count since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `YYYYMMDDHHMMSS +ZZZZ` (offset optional, UTC if absent) to Unix seconds.
pub fn parse_time(s: &str) -> Option<i64> {
    let mut parts = s.split_whitespace();
    let digits = parts.next()?;
    if digits.len() < 14 || !digits.as_bytes()[..14].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n = |r: std::ops::Range<usize>| digits[r].parse::<i64>().ok();
    let days = days_from_civil(n(0..4)?, n(4..6)?, n(6..8)?);
    let local = days * 86_400 + n(8..10)? * 3600 + n(10..12)? * 60 + n(12..14)?;
    let offset = match parts.next() {
        Some(z) if z.len() == 5 && z.is_ascii() => {
            let sign = match &z[..1] {
                "+" => 1,
                "-" => -1,
                _ => return None,
            };
            sign * (z[1..3].parse::<i64>().ok()? * 3600 + z[3..5].parse::<i64>().ok()? * 60)
        }
        Some(_) => return None,
        None => 0,
    };
    Some(local - offset)
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    None,
    DisplayName,
    Title,
    SubTitle,
    Desc,
    ProgId,
}

fn attr(e: &BytesStart, name: &str) -> Result<Option<String>> {
    Ok(match e.try_get_attribute(name)? {
        Some(a) => Some(a.unescape_value()?.into_owned()),
        None => None,
    })
}

pub fn parse(xml: &str) -> Result<Guide> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut guide = Guide::default();
    // Programmes with their stop time, which XMLTV allows to be missing.
    let mut raw: Vec<(GuideProgramme, Option<i64>)> = Vec::new();
    let mut chan: Option<GuideChannel> = None;
    let mut prog: Option<(GuideProgramme, Option<i64>)> = None;
    let mut field = Field::None;
    loop {
        let text = match reader.read_event()? {
            Event::Start(e) => {
                field = Field::None;
                match e.name().as_ref() {
                    b"channel" => {
                        chan = attr(&e, "id")?.map(|id| GuideChannel { id, name: None });
                    }
                    b"programme" => {
                        let start = attr(&e, "start")?.as_deref().and_then(parse_time);
                        let stop = attr(&e, "stop")?.as_deref().and_then(parse_time);
                        prog = match (start, attr(&e, "channel")?) {
                            (Some(start), Some(channel)) => Some((
                                GuideProgramme {
                                    channel,
                                    start,
                                    ..Default::default()
                                },
                                stop,
                            )),
                            _ => None,
                        };
                    }
                    b"display-name" => field = Field::DisplayName,
                    b"title" => field = Field::Title,
                    b"sub-title" => field = Field::SubTitle,
                    b"desc" => field = Field::Desc,
                    b"episode-num" if attr(&e, "system")?.as_deref() == Some("dd_progid") => {
                        field = Field::ProgId;
                    }
                    _ => {}
                }
                continue;
            }
            Event::End(e) => {
                field = Field::None;
                match e.name().as_ref() {
                    b"channel" => guide.channels.extend(chan.take()),
                    b"programme" => raw.extend(prog.take()),
                    _ => {}
                }
                continue;
            }
            Event::Text(t) => t.unescape()?.into_owned(),
            Event::CData(c) => String::from_utf8_lossy(&c).into_owned(),
            Event::Eof => break,
            _ => continue,
        };
        // Several `lang` variants may follow each other: keep the first.
        match (field, &mut chan, &mut prog) {
            (Field::DisplayName, Some(c), _) => {
                c.name.get_or_insert(text);
            }
            (Field::Title, _, Some((p, _))) if p.title.is_empty() => p.title = text,
            (Field::SubTitle, _, Some((p, _))) => {
                p.subtitle.get_or_insert(text);
            }
            (Field::Desc, _, Some((p, _))) => {
                p.description.get_or_insert(text);
            }
            // `EP01234567.0001`: the part before the dot names the series.
            (Field::ProgId, _, Some((p, _))) => {
                p.series_key = text.split('.').next().map(str::to_owned);
            }
            _ => {}
        }
    }
    raw.sort_by(|(a, _), (b, _)| (&a.channel, a.start).cmp(&(&b.channel, b.start)));
    for (i, (p, stop)) in raw.iter().enumerate() {
        let next_start = raw
            .get(i + 1)
            .filter(|(n, _)| n.channel == p.channel)
            .map(|(n, _)| n.start);
        if let Some(stop) = stop.or(next_start) {
            guide.programmes.push(GuideProgramme { stop, ..p.clone() });
        }
    }
    Ok(guide)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUIDE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE tv SYSTEM "xmltv.dtd">
<tv generator-info-name="test">
  <channel id="bbc1.uk">
    <display-name lang="en">BBC One</display-name>
    <icon src="http://logo/bbc1.png"/>
  </channel>
  <programme start="20261110200000 +0100" stop="20261110210000 +0100" channel="bbc1.uk">
    <title lang="en">Doctor Who</title>
    <sub-title lang="en">The Eleventh Hour</sub-title>
    <desc lang="en">A man &amp; his box.</desc>
    <episode-num system="xmltv_ns">4.0.</episode-num>
    <episode-num system="dd_progid">EP01234567.0001</episode-num>
  </programme>
  <programme start="20261110210000 +0100" channel="bbc1.uk">
    <title>News</title>
  </programme>
  <programme start="20261110220000 +0100" channel="bbc1.uk">
    <title>Last, no stop</title>
  </programme>
</tv>"#;

    #[test]
    fn time_with_offset() {
        // 2026-11-10 19:00:00 UTC
        assert_eq!(parse_time("20261110200000 +0100"), Some(1_794_337_200));
        assert_eq!(parse_time("20261110190000"), Some(1_794_337_200));
        assert_eq!(parse_time("20261110190000 -0130"), Some(1_794_342_600));
        assert_eq!(parse_time("garbage"), None);
    }

    #[test]
    fn civil_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        for d in [-1, 0, 11_016, 11_017, 20_402] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d);
        }
        assert_eq!(civil_from_days(20_402), (2025, 11, 10));
    }

    #[test]
    fn parses_channels() {
        let g = parse(GUIDE).unwrap();
        assert_eq!(
            g.channels,
            vec![GuideChannel {
                id: "bbc1.uk".into(),
                name: Some("BBC One".into()),
            }]
        );
    }

    #[test]
    fn parses_programme_details() {
        let g = parse(GUIDE).unwrap();
        assert_eq!(
            g.programmes[0],
            GuideProgramme {
                channel: "bbc1.uk".into(),
                start: 1_794_337_200,
                stop: 1_794_340_800,
                title: "Doctor Who".into(),
                subtitle: Some("The Eleventh Hour".into()),
                description: Some("A man & his box.".into()),
                series_key: Some("EP01234567".into()),
            }
        );
    }

    #[test]
    fn missing_stop_uses_next_start() {
        let g = parse(GUIDE).unwrap();
        assert_eq!(g.programmes.len(), 2, "the last programme has no end");
        assert_eq!(g.programmes[1].title, "News");
        assert_eq!(g.programmes[1].stop, 1_794_344_400);
    }

    #[test]
    fn rejects_broken_xml() {
        assert!(parse("<tv><programme start=").is_err());
    }
}
