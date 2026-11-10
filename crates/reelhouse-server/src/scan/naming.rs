//! Plex-style file names: `Movies/Title (Year)/Title (Year).ext`,
//! `TV/Show/Season NN/Show - SxxEyy - Name.ext`.

use std::path::Path;

pub const VIDEO_EXTENSIONS: &[&str] = &["mkv", "mp4", "m4v", "avi", "mov", "ts", "webm"];

#[derive(Debug, PartialEq)]
pub enum Parsed {
    Movie { title: String, year: Option<u16> },
    Episode { show: String, season: u16, episode: u16 },
}

pub fn parse_movie(path: &Path) -> Parsed {
    let stem = file_stem(path);
    let folder = path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("");
    // Prefer the folder when it carries the year (Plex layout), else the file name.
    let source = if year_of(folder).is_some() { folder } else { stem };
    match year_of(source) {
        Some((year, at)) => Parsed::Movie { title: clean(&source[..at]), year: Some(year) },
        None => Parsed::Movie { title: stem.to_string(), year: None },
    }
}

pub fn parse_episode(path: &Path) -> Option<Parsed> {
    let (season, episode) = season_episode(file_stem(path))?;
    let parent = path.parent()?;
    let parent_name = parent.file_name()?.to_str()?;
    let show_dir = if parent_name.to_ascii_lowercase().starts_with("season") { parent.parent()? } else { parent };
    let show = show_dir.file_name()?.to_str()?;
    Some(Parsed::Episode { show: clean(show), season, episode })
}

fn file_stem(path: &Path) -> &str {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or("")
}

/// Finds `(YYYY)`; returns the year and the byte offset of `(`.
fn year_of(s: &str) -> Option<(u16, usize)> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(5)).find_map(|i| {
        let digits = &s.get(i + 1..i + 5)?;
        (b[i] == b'(' && b[i + 5] == b')' && digits.bytes().all(|c| c.is_ascii_digit()))
            .then(|| (digits.parse().ok().filter(|y| (1880..=2100).contains(y)).map(|y| (y, i))))
            .flatten()
    })
}

/// Case-insensitive `SxxEyy`.
fn season_episode(s: &str) -> Option<(u16, u16)> {
    let lower = s.to_ascii_lowercase();
    let b = lower.as_bytes();
    (0..b.len()).find_map(|i| {
        if b[i] != b's' {
            return None;
        }
        let season_len = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        let e = i + 1 + season_len;
        if season_len == 0 || b.get(e) != Some(&b'e') {
            return None;
        }
        let episode_len = b[e + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if episode_len == 0 {
            return None;
        }
        Some((lower[i + 1..e].parse().ok()?, lower[e + 1..e + 1 + episode_len].parse().ok()?))
    })
}

/// Drops `{...}` and `[...]` tags and trims separators.
fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim_matches(|c: char| c.is_whitespace() || c == '-' || c == '.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movie_folder_with_year() {
        assert_eq!(
            parse_movie(Path::new("/m/Blade Runner (1982)/Blade Runner (1982).mkv")),
            Parsed::Movie { title: "Blade Runner".into(), year: Some(1982) }
        );
    }

    #[test]
    fn movie_file_with_year_and_tags() {
        assert_eq!(
            parse_movie(Path::new("/m/Dune Part Two (2024) {imdb-tt15239678} [2160p].mkv")),
            Parsed::Movie { title: "Dune Part Two".into(), year: Some(2024) }
        );
    }

    #[test]
    fn unparsed_movie_keeps_stem() {
        assert_eq!(
            parse_movie(Path::new("/m/movie.final.v2.mkv")),
            Parsed::Movie { title: "movie.final.v2".into(), year: None }
        );
    }

    #[test]
    fn episode_plex_layout() {
        assert_eq!(
            parse_episode(Path::new("/tv/The Bear/Season 02/The Bear - S02E07 - Forks.mkv")),
            Some(Parsed::Episode { show: "The Bear".into(), season: 2, episode: 7 })
        );
    }

    #[test]
    fn episode_lowercase_marker_uses_show_folder() {
        assert_eq!(
            parse_episode(Path::new("/tv/Severance/Season 1/severance.s01e03.1080p.mkv")),
            Some(Parsed::Episode { show: "Severance".into(), season: 1, episode: 3 })
        );
    }

    #[test]
    fn non_episode_is_none() {
        assert_eq!(parse_episode(Path::new("/tv/Extras/trailer.mkv")), None);
    }
}
