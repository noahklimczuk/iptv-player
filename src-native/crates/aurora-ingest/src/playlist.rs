//! Fetch a playlist and hand it straight to the streaming parser.
//!
//! Deliberately thin: `aurora_core::m3u` already does the hard part. The point here is
//! that the bytes are *streamed* into it, so a 400MB playlist never lands in memory
//! (README §4.2).

use std::fs::File;
use std::io::BufReader;

use aurora_core::m3u::{self, Parsed};
use aurora_core::neterr::NetFailure;

use crate::http::HttpClient;

/// Fetch and parse a playlist from an HTTP(S) URL or a local path.
pub fn fetch(http: &HttpClient, url: &str) -> Result<Parsed, NetFailure> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let reader = http.fetch_reader(url)?;
        Ok(m3u::parse(BufReader::new(reader)))
    } else {
        let file = File::open(url).map_err(|e| {
            let mut failure = NetFailure::classify(&e.to_string());
            failure.message = "Aurora could not open that playlist file".into();
            failure.cause = format!("{url}: {e}");
            failure
        })?;
        Ok(m3u::parse(BufReader::new(file)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HttpConfig;
    use crate::testserver::{Reply, TestServer};
    use std::io::Write;

    fn client() -> HttpClient {
        HttpClient::new(HttpConfig {
            max_attempts: 1,
            ..Default::default()
        })
        .unwrap()
    }

    const PLAYLIST: &str = "#EXTM3U url-tvg=\"https://example.com/epg.xml.gz\"\n\
        #EXTINF:-1 tvg-id=\"cnn.us\" tvg-chno=\"202\" group-title=\"News\",CNN HD\n\
        https://example.com/live/u/p/1.ts\n\
        #EXTINF:-1 group-title=\"Movies\",Some Film (2019)\n\
        https://example.com/movie/u/p/2.mkv\n";

    #[test]
    fn fetches_and_parses_over_http() {
        let server = TestServer::always(Reply::ok(PLAYLIST));
        let parsed = fetch(&client(), &server.url("/list.m3u")).unwrap();

        assert_eq!(parsed.result.entries.len(), 2);
        assert_eq!(
            parsed.header.epg_urls,
            vec!["https://example.com/epg.xml.gz"]
        );
        assert_eq!(parsed.result.entries[0].number, Some(202));
        assert_eq!(
            parsed.result.entries[1].kind,
            aurora_core::model::MediaKind::Movie
        );
    }

    #[test]
    fn handles_a_gzipped_playlist() {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(PLAYLIST.as_bytes()).unwrap();
        let server = TestServer::always(Reply::ok(e.finish().unwrap()));

        let parsed = fetch(&client(), &server.url("/list.m3u.gz")).unwrap();
        assert_eq!(parsed.result.entries.len(), 2);
    }

    #[test]
    fn reads_a_local_file() {
        let dir = std::env::temp_dir().join(format!("aurora-pl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("list.m3u");
        std::fs::write(&path, PLAYLIST).unwrap();

        let parsed = fetch(&client(), path.to_str().unwrap()).unwrap();
        assert_eq!(parsed.result.entries.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_local_file_is_a_readable_error() {
        let err = fetch(&client(), "/nonexistent/aurora/list.m3u").unwrap_err();
        assert!(err.message.contains("could not open"), "{}", err.message);
    }

    #[test]
    fn a_broken_line_does_not_lose_the_playlist() {
        let body = "#EXTM3U\n#EXTINF:-1,Good\nhttps://example.com/a.ts\n\
                    garbage-not-a-url\n#EXTINF:-1,Also good\nhttps://example.com/b.ts\n";
        let server = TestServer::always(Reply::ok(body));
        let parsed = fetch(&client(), &server.url("/list.m3u")).unwrap();

        assert_eq!(parsed.result.entries.len(), 2);
        assert_eq!(parsed.result.skipped, 1);
        assert!(!parsed.result.warnings.is_empty());
    }

    #[test]
    fn an_http_failure_is_reported_rather_than_parsed_as_empty() {
        let server = TestServer::always(Reply::status(404));
        assert!(fetch(&client(), &server.url("/gone.m3u")).is_err());
    }
}
