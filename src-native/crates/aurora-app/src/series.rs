//! Episode listings, fetched the first time a show is opened.
//!
//! A refresh writes the series row and stops there. Asking a panel for the episodes of
//! every show it lists would be one request each — 28,693 of them on the subscription
//! this was measured against — so the listing is deferred until somebody actually
//! wants it. The refresh has said so in a warning since F-04 was fixed; what was
//! missing was anything that then did it, so every series on every panel sat at
//! "0 seasons" permanently.

use aurora_db::repo::library;
use aurora_db::rusqlite::OptionalExtension;
use aurora_ingest::xtream::XtreamClient;

use crate::error::{AppError, Result};
use crate::services::Services;

/// What the database knows about a show, and who to ask about it.
struct Show {
    base_url: String,
    username: String,
    credential_ref: Option<String>,
    /// The panel's own id, out of the `series:1234` key an import writes.
    stream_id: u32,
}

/// Fetch one show's episodes from its provider and store them.
///
/// Returns the number written. Doing nothing is a success: a show really can have no
/// episodes listed yet, and that is not a failure to report to the viewer.
pub fn fetch_episodes(services: &Services, series_id: i64) -> Result<usize> {
    let show = match look_up(services, series_id)? {
        Some(s) => s,
        // An M3U library builds its episodes from the playlist during the import, so
        // there is nothing to go and ask for. Neither is there for a show whose row
        // predates provider keys.
        None => return Ok(0),
    };

    let password = match &show.credential_ref {
        None => String::new(),
        Some(key) => services.credentials.get(key).map_err(|e| {
            AppError::Other(format!(
                "Aurora could not read this provider's saved password: {e}. Edit the \
                 provider in Settings and enter it again."
            ))
        })?,
    };

    let client = XtreamClient::new(&services.http, &show.base_url, &show.username, &password);
    // Worded the same way a refresh words its failures, because to the viewer they are
    // the same event: the provider would not answer.
    let info = client
        .series_info(show.stream_id)
        .map_err(|e| AppError::Other(format!("{}: {}", e.message, e.cause)))?;

    let mut episodes = Vec::with_capacity(info.episodes.len());
    for ep in &info.episodes {
        // An episode with no id has no URL, and one with no number has nowhere to sit
        // in a season. Either way it cannot be played or listed, so it is dropped
        // rather than stored as a row that looks like an episode and is not.
        let (Some(id), Some(number)) = (ep.id.as_deref(), ep.episode_num) else {
            continue;
        };
        let Ok(stream_id) = id.trim().parse::<u32>() else {
            continue;
        };
        episodes.push(library::NewEpisode {
            season: ep.season.unwrap_or(1).min(u32::from(u16::MAX)) as u16,
            episode: number.min(u32::from(u16::MAX)) as u16,
            title: ep.title.clone().filter(|t| !t.trim().is_empty()),
            url: client.stream_url(
                aurora_core::model::MediaKind::Episode,
                stream_id,
                ep.container_extension.as_deref(),
            ),
            still: ep.info.movie_image.clone().filter(|s| !s.trim().is_empty()),
        });
    }

    if episodes.is_empty() {
        return Ok(0);
    }

    let mut db = services.db.lock();
    let written = library::upsert_episodes(&mut db, series_id, &episodes, crate::now_unix())?;
    tracing::info!(series = series_id, written, "episode listing stored");
    Ok(written)
}

/// The provider behind a series, and the panel's id for it. `None` when this is not a
/// show an Xtream panel can be asked about.
fn look_up(services: &Services, series_id: i64) -> Result<Option<Show>> {
    let db = services.db.lock();
    let row = db
        .query_row(
            "SELECT p.kind, p.base_url, COALESCE(p.username, ''), p.credential_ref,
                    s.provider_key
             FROM series s JOIN providers p ON p.id = s.provider_id
             WHERE s.id = ?1",
            [series_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        )
        .optional()
        .map_err(aurora_db::DbError::from)?;

    let Some((kind, base_url, username, credential_ref, provider_key)) = row else {
        return Err(AppError::Other(format!("no series {series_id}")));
    };
    if kind != "xtream" {
        return Ok(None);
    }
    let Some(stream_id) = provider_key
        .strip_prefix("series:")
        .and_then(|id| id.trim().parse::<u32>().ok())
    else {
        return Ok(None);
    };

    Ok(Some(Show {
        base_url,
        username,
        credential_ref,
        stream_id,
    }))
}

#[cfg(test)]
mod tests {

    /// The key an import writes is `series:1234`; anything else is not a panel show.
    #[test]
    fn a_provider_key_yields_the_panels_own_id() {
        let read = |key: &str| {
            key.strip_prefix("series:")
                .and_then(|id| id.trim().parse::<u32>().ok())
        };
        assert_eq!(read("series:1234"), Some(1234));
        assert_eq!(read("series: 77 "), Some(77));
        assert_eq!(read("1234"), None, "an unprefixed key is not ours");
        assert_eq!(read("movie:1234"), None);
        assert_eq!(read("series:"), None);
        assert_eq!(read("series:abc"), None);
    }
}
