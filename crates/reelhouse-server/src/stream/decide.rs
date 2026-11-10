//! Direct play when the client handles the file as is; remux when only the container is wrong.

use reelhouse_api::{Capabilities, PlaybackMethod};

use crate::scan::probe::MediaInfo;

fn has(list: &[String], value: &Option<String>) -> bool {
    value
        .as_ref()
        .is_none_or(|v| list.iter().any(|x| x.eq_ignore_ascii_case(v)))
}

pub fn decide(info: &MediaInfo, caps: &Capabilities) -> PlaybackMethod {
    let codecs_ok =
        has(&caps.video_codecs, &info.video_codec) && has(&caps.audio_codecs, &info.audio_codec);
    let height_ok = info.height.is_none_or(|h| h <= i64::from(caps.max_height));
    match (
        codecs_ok && height_ok,
        has(&caps.containers, &info.container),
    ) {
        (true, true) => PlaybackMethod::Direct,
        (true, false) => PlaybackMethod::Remux,
        (false, _) => PlaybackMethod::Transcode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(containers: &[&str], video: &[&str]) -> Capabilities {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect();
        Capabilities {
            containers: v(containers),
            video_codecs: v(video),
            audio_codecs: v(&["aac", "ac3"]),
            max_height: 2160,
        }
    }

    fn info(container: &str, video: &str) -> MediaInfo {
        MediaInfo {
            duration_s: Some(10.0),
            container: Some(container.into()),
            video_codec: Some(video.into()),
            audio_codec: Some("aac".into()),
            height: Some(1080),
        }
    }

    #[test]
    fn decide_direct_for_matching_caps() {
        assert_eq!(
            decide(&info("mp4", "h264"), &caps(&["mp4"], &["h264"])),
            PlaybackMethod::Direct
        );
    }

    #[test]
    fn decide_remux_for_mkv_on_apple_caps() {
        assert_eq!(
            decide(
                &info("mkv", "hevc"),
                &caps(&["mp4", "mov"], &["h264", "hevc"])
            ),
            PlaybackMethod::Remux
        );
    }

    #[test]
    fn decide_transcode_for_hevc_on_h264_only() {
        assert_eq!(
            decide(&info("mp4", "hevc"), &caps(&["mp4"], &["h264"])),
            PlaybackMethod::Transcode
        );
    }
}
