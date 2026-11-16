//! What the page is told about the player: the engine's report, plus what is playing.
//!
//! Kept here rather than in the app so the contract test (`aurora-contract`), which
//! cannot depend on Tauri, can pair it with `shared/ipc.ts`.

use serde::{Deserialize, Serialize};

use crate::model::Report;

/// What kind of thing is playing, by the names the page uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Live,
    Movie,
    Episode,
    Recording,
}

impl ItemKind {
    /// The page's name for it; `None` for a name Aurora does not play (H2).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "live" => Self::Live,
            "movie" => Self::Movie,
            "episode" => Self::Episode,
            "recording" => Self::Recording,
            _ => return None,
        })
    }
}

/// `PlayerState` in `shared/ipc.ts`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub channel_id: Option<i64>,
    pub item_kind: Option<ItemKind>,
    pub item_id: Option<i64>,
    /// The stream dropped and Aurora is getting it back (H12). The page shows a quiet
    /// "reconnecting", not an error; `error` stays empty until it gives up.
    pub reconnecting: bool,
    #[serde(flatten)]
    pub report: Report,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_is_flattened_beside_what_is_playing() {
        let v = serde_json::to_value(PlayerState {
            title: Some("BBC One".into()),
            item_kind: Some(ItemKind::Live),
            ..PlayerState::default()
        })
        .unwrap();
        assert_eq!(v["title"], "BBC One");
        assert_eq!(v["itemKind"], "live");
        assert_eq!(v["status"], "idle");
        assert_eq!(v["reconnecting"], false);
        assert!(v.get("report").is_none(), "{v}");
    }

    #[test]
    fn h2_an_unknown_kind_is_not_guessed() {
        assert_eq!(ItemKind::parse("episode"), Some(ItemKind::Episode));
        assert_eq!(ItemKind::parse("podcast"), None);
    }
}
