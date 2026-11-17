//! What to play, and the mpv settings that follow from it.

use std::path::PathBuf;

use aurora_core::timeshift::Budget;

/// Sent when the playlist names no User-Agent (E6). The catalogue requests send the
/// same, so the player and the catalogue look like one client to the provider.
pub const DEFAULT_USER_AGENT: &str = concat!("AuroraTV/", env!("CARGO_PKG_VERSION"));

/// mpv's own defaults, restored for everything that is not a buffered live stream, so a
/// channel that was buffered does not leave its settings on the film played after it.
pub const DEFAULT_AHEAD_BYTES: u64 = 150 * 1024 * 1024;
pub const DEFAULT_BEHIND_BYTES: u64 = 50 * 1024 * 1024;

/// Rewind for live TV (README §7.6): where to keep it, and how much.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeshift {
    pub budget: Budget,
    pub dir: PathBuf,
}

/// One file to play.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Request {
    pub url: String,
    /// Extra HTTP headers, in order.
    pub headers: Vec<(String, String)>,
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    /// Where to start a film or episode. Ignored for live (E9).
    pub start_secs: Option<f64>,
    pub live: bool,
    /// The network buffer, seconds (the Low latency / Balanced / Unstable preset).
    pub cache_secs: u32,
    /// Keep a rewind buffer. Only honoured for live (E21).
    pub timeshift: Option<Timeshift>,
}

/// A setting for mpv: a plain string, or a list (for the header fields, whose values
/// may contain the commas a string list would split on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Setting {
    Text(&'static str, String),
    List(&'static str, Vec<String>),
}

impl Setting {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Text(n, _) | Self::List(n, _) => n,
        }
    }
}

impl Request {
    /// How far ahead mpv reads. The viewer's network-buffer preset; on a buffered
    /// channel, the whole rewind span, so a paused channel goes on recording. That costs
    /// no latency: nothing past the live edge exists to be read early.
    pub fn read_ahead_secs(&self) -> u32 {
        let preset = self.cache_secs.clamp(1, 60);
        match self.rewind() {
            Some(ts) => preset.max(ts.budget.secs),
            None => preset,
        }
    }

    /// The buffer actually in force: a film never gets one.
    pub fn rewind(&self) -> Option<&Timeshift> {
        self.timeshift.as_ref().filter(|_| self.live)
    }

    /// Every per-file setting, each one present whatever this file needs (E5).
    ///
    /// These are properties of a handle that outlives the file. A setting sent only when
    /// it has a value leaves the previous file's value in force — one provider's headers
    /// sent to another, or a channel's rewind buffer filling the disk under a film. So
    /// each is always said, empty or default where there is nothing to say.
    pub fn settings(&self) -> Vec<Setting> {
        use Setting::{List, Text};
        let ua = self
            .user_agent
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_USER_AGENT);
        let mut out = vec![
            Text("user-agent", ua.to_string()),
            Text(
                "referrer",
                self.referrer.as_deref().unwrap_or("").trim().to_string(),
            ),
            List(
                "http-header-fields",
                self.headers
                    .iter()
                    .filter(|(k, _)| !k.trim().is_empty())
                    .map(|(k, v)| format!("{}: {}", k.trim(), v.trim()))
                    .collect(),
            ),
            Text("cache-secs", self.read_ahead_secs().to_string()),
            Text(
                "start",
                match self
                    .start_secs
                    .filter(|s| !self.live && s.is_finite() && *s > 0.0)
                {
                    Some(s) => format!("{s:.3}"),
                    None => "none".into(),
                },
            ),
        ];
        // E7: a live TS is identified from very little, and every byte probed is time
        // before the first frame; a film is worth reading further into, or a second audio
        // track or the subtitle streams can be missed.
        let (probe, analyse) = if self.live {
            ("1000000", "1.5")
        } else {
            ("5000000", "5")
        };
        out.push(Text("demuxer-lavf-probesize", probe.into()));
        out.push(Text("demuxer-lavf-analyzeduration", analyse.into()));

        match self.rewind() {
            Some(ts) => {
                out.push(Text("cache", "yes".into()));
                out.push(Text("cache-on-disk", "yes".into()));
                // `demuxer-cache-dir`: mpv has no `cache-dir`, and a setting it does not
                // know is refused, which leaves the buffer in memory (found on a real
                // channel: 76 MB of RAM and nothing on disk).
                out.push(Text(
                    "demuxer-cache-dir",
                    ts.dir.to_string_lossy().into_owned(),
                ));
                out.push(Text("demuxer-seekable-cache", "yes".into()));
                // Ahead as well as behind. Paused live TV is the playhead falling
                // behind while the channel goes on arriving, and all of that arrives
                // *ahead* of the playhead. Capped any lower, mpv stops reading a minute
                // into a pause and the connection goes stale.
                out.push(Text("demuxer-max-bytes", ts.budget.bytes.to_string()));
                out.push(Text("demuxer-max-back-bytes", ts.budget.bytes.to_string()));
            }
            None => {
                out.push(Text("cache", if self.live { "yes" } else { "auto" }.into()));
                out.push(Text("cache-on-disk", "no".into()));
                out.push(Text("demuxer-cache-dir", String::new()));
                out.push(Text("demuxer-seekable-cache", "auto".into()));
                out.push(Text("demuxer-max-bytes", DEFAULT_AHEAD_BYTES.to_string()));
                out.push(Text(
                    "demuxer-max-back-bytes",
                    DEFAULT_BEHIND_BYTES.to_string(),
                ));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text<'a>(s: &'a [Setting], name: &str) -> &'a str {
        s.iter()
            .find_map(|x| match x {
                Setting::Text(n, v) if *n == name => Some(v.as_str()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name} not set"))
    }

    fn film() -> Request {
        Request {
            url: "http://p/movie/1.mkv".into(),
            cache_secs: 10,
            ..Request::default()
        }
    }

    fn channel() -> Request {
        Request {
            url: "http://p/live/1.ts".into(),
            live: true,
            cache_secs: 4,
            ..Request::default()
        }
    }

    fn buffered() -> Request {
        Request {
            timeshift: Some(Timeshift {
                budget: Budget::default(),
                dir: PathBuf::from("C:/buf"),
            }),
            ..channel()
        }
    }

    #[test]
    fn every_setting_is_sent_for_every_kind_of_file() {
        let names = |r: &Request| r.settings().iter().map(Setting::name).collect::<Vec<_>>();
        assert_eq!(names(&film()), names(&channel()));
        assert_eq!(names(&film()), names(&buffered()));
    }

    #[test]
    fn a_file_with_no_headers_clears_the_last_ones() {
        let s = film().settings();
        assert!(s.contains(&Setting::List("http-header-fields", vec![])));
        assert_eq!(text(&s, "referrer"), "");
    }

    #[test]
    fn headers_referrer_and_agent_are_passed_through() {
        let r = Request {
            headers: vec![("X-Token".into(), "a,b".into())],
            referrer: Some("http://ref/".into()),
            user_agent: Some("VLC/3".into()),
            ..film()
        };
        let s = r.settings();
        assert!(s.contains(&Setting::List(
            "http-header-fields",
            vec!["X-Token: a,b".into()]
        )));
        assert_eq!(text(&s, "referrer"), "http://ref/");
        assert_eq!(text(&s, "user-agent"), "VLC/3");
    }

    #[test]
    fn a_user_agent_is_always_sent() {
        for ua in [None, Some(String::new()), Some("   ".into())] {
            let r = Request {
                user_agent: ua,
                ..film()
            };
            assert_eq!(text(&r.settings(), "user-agent"), DEFAULT_USER_AGENT);
        }
    }

    #[test]
    fn a_film_starts_where_asked_and_live_never_does() {
        let r = Request {
            start_secs: Some(754.5),
            ..film()
        };
        assert_eq!(text(&r.settings(), "start"), "754.500");
        let r = Request {
            start_secs: Some(754.5),
            ..channel()
        };
        assert_eq!(text(&r.settings(), "start"), "none");
    }

    #[test]
    fn a_film_with_no_resume_point_starts_at_the_beginning() {
        assert_eq!(text(&film().settings(), "start"), "none");
        let r = Request {
            start_secs: Some(f64::NAN),
            ..film()
        };
        assert_eq!(text(&r.settings(), "start"), "none");
    }

    #[test]
    fn the_network_buffer_is_never_zero() {
        let r = Request {
            cache_secs: 0,
            ..channel()
        };
        assert_eq!(text(&r.settings(), "cache-secs"), "1");
    }

    #[test]
    fn live_probes_less_than_a_film() {
        let p = |r: &Request| {
            text(&r.settings(), "demuxer-lavf-probesize")
                .parse::<u64>()
                .unwrap()
        };
        assert!(p(&channel()) < p(&film()));
    }

    #[test]
    fn a_buffered_channel_keeps_its_history_on_disk() {
        let s = buffered().settings();
        assert_eq!(text(&s, "cache-on-disk"), "yes");
        assert_eq!(text(&s, "demuxer-cache-dir"), "C:/buf");
        assert_eq!(
            text(&s, "demuxer-max-back-bytes"),
            Budget::default().bytes.to_string()
        );
    }

    #[test]
    fn a_paused_buffered_channel_goes_on_reading() {
        let r = buffered();
        let s = r.settings();
        let budget = Budget::default();
        assert_eq!(text(&s, "demuxer-max-bytes"), budget.bytes.to_string());
        assert_eq!(text(&s, "cache-secs"), budget.secs.to_string());
        // Without a buffer, the preset is what it says.
        assert_eq!(text(&channel().settings(), "cache-secs"), "4");
    }

    #[test]
    fn a_film_is_never_given_a_rewind_buffer() {
        let r = Request {
            timeshift: buffered().timeshift,
            ..film()
        };
        assert!(r.rewind().is_none());
        assert_eq!(text(&r.settings(), "cache-on-disk"), "no");
    }

    #[test]
    fn turning_the_buffer_off_puts_the_defaults_back() {
        let s = channel().settings();
        assert_eq!(text(&s, "cache-on-disk"), "no");
        assert_eq!(text(&s, "demuxer-cache-dir"), "");
        assert_eq!(
            text(&s, "demuxer-max-back-bytes"),
            DEFAULT_BEHIND_BYTES.to_string()
        );
    }
}
