//! The artwork disk cache (README §12).
//!
//! Enrichment stores the remote URL, not a local path, so a library that moves between
//! machines — or a portable install on a USB stick — keeps working. This is the
//! accelerator that makes that affordable: content-addressed by the URL, so the key is a
//! pure function the UI can compute without asking, and safe to delete at any moment
//! because nothing depends on it existing.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aurora_core::neterr::{ErrorAction, ErrorCode, NetFailure};
use sha2::{Digest, Sha256};

use crate::http::HttpClient;

/// Default ceiling for the cache. A library of 40,000 titles at two images each, at
/// w342/w1280, lands well under this; the cap is what stops a pathological library
/// filling a disk.
pub const DEFAULT_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Refuse a single image larger than this. A poster is tens of kilobytes; a megabyte
/// means the URL is not an image.
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

/// Extensions we will write. Anything else is stored as `.img` rather than trusted.
const KNOWN_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "webp", "svg"];

/// The cache key for a URL: a hash plus the original extension.
///
/// Pure and stable, so the UI can name the file it wants without a round trip, and so
/// the same image fetched via two equivalent URLs is stored once per URL rather than
/// being guessed at.
pub fn key_for(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    let hex: String = digest.iter().take(16).map(|b| format!("{b:02x}")).collect();
    match extension_of(url) {
        Some(ext) => format!("{hex}.{ext}"),
        None => format!("{hex}.img"),
    }
}

fn extension_of(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    KNOWN_EXTENSIONS.contains(&ext.as_str()).then_some(ext)
}

/// A content-addressed store of downloaded images.
#[derive(Debug, Clone)]
pub struct Cache {
    dir: PathBuf,
    max_bytes: u64,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }

    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Where a URL's image lives, whether or not it is there yet.
    pub fn path_for(&self, url: &str) -> PathBuf {
        self.dir.join(key_for(url))
    }

    /// Whether this URL is already on disk.
    pub fn contains(&self, url: &str) -> bool {
        // A zero-byte file is a crashed download, not a cached image.
        fs::metadata(self.path_for(url))
            .map(|m| m.is_file() && m.len() > 0)
            .unwrap_or(false)
    }

    /// The local file for a URL, downloading it if it is not already there.
    pub fn fetch(&self, http: &HttpClient, url: &str) -> Result<PathBuf, NetFailure> {
        let path = self.path_for(url);
        if self.contains(url) {
            // Touch it so eviction treats it as recently used, and carry on.
            touch(&path);
            return Ok(path);
        }

        fs::create_dir_all(&self.dir).map_err(|e| io_failure("create the artwork folder", &e))?;

        let mut reader = http.fetch_reader(url)?;
        let mut bytes = Vec::new();
        reader
            .by_ref()
            .take(MAX_IMAGE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| io_failure("read the image", &e))?;

        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err(NetFailure {
                code: ErrorCode::Unknown,
                message: "That artwork is implausibly large".into(),
                cause: format!(
                    "The server sent more than {MAX_IMAGE_BYTES} bytes, so the URL is \
                     probably not an image."
                ),
                actions: vec![ErrorAction::ReportBroken],
                retryable: false,
            });
        }
        if bytes.is_empty() {
            return Err(NetFailure {
                code: ErrorCode::NotFound,
                message: "That artwork is no longer available".into(),
                cause: "The server answered with an empty body.".into(),
                actions: vec![ErrorAction::ReportBroken],
                retryable: false,
            });
        }

        // Write beside the target and rename, so a crash mid-download cannot leave a
        // truncated file that `contains` would then call a cache hit.
        let temp = path.with_extension("part");
        fs::write(&temp, &bytes).map_err(|e| io_failure("write the image", &e))?;
        fs::rename(&temp, &path).map_err(|e| io_failure("store the image", &e))?;
        Ok(path)
    }

    /// Total bytes held. Ignores anything unreadable rather than failing.
    pub fn total_bytes(&self) -> u64 {
        entries(&self.dir).iter().map(|e| e.bytes).sum()
    }

    pub fn len(&self) -> usize {
        entries(&self.dir).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Delete least-recently-used images until the cache fits its budget.
    ///
    /// Returns how many were removed. Approximate by design: modification time is what
    /// the filesystem gives us for free, and an artwork cache does not warrant tracking
    /// access times of its own.
    pub fn evict(&self) -> usize {
        let mut files = entries(&self.dir);
        let mut total: u64 = files.iter().map(|e| e.bytes).sum();
        if total <= self.max_bytes {
            return 0;
        }

        files.sort_by_key(|e| e.modified);
        let mut removed = 0;
        for entry in files {
            if total <= self.max_bytes {
                break;
            }
            if fs::remove_file(&entry.path).is_ok() {
                total -= entry.bytes;
                removed += 1;
            }
        }
        removed
    }

    /// Empty the cache. Safe at any time — nothing depends on it existing.
    pub fn clear(&self) -> usize {
        let files = entries(&self.dir);
        let mut removed = 0;
        for entry in files {
            if fs::remove_file(&entry.path).is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

#[derive(Debug)]
struct Entry {
    path: PathBuf,
    bytes: u64,
    modified: u64,
}

fn entries(dir: &Path) -> Vec<Entry> {
    let Ok(read) = fs::read_dir(dir) else {
        // No directory yet is an empty cache, not an error.
        return Vec::new();
    };
    read.filter_map(|e| e.ok())
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            // Half-written downloads are not cache contents.
            if e.path().extension().is_some_and(|x| x == "part") {
                return None;
            }
            Some(Entry {
                path: e.path(),
                bytes: meta.len(),
                modified: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            })
        })
        .collect()
}

fn touch(path: &Path) {
    // Best effort: a cache entry that cannot be touched is still a valid cache entry,
    // it just looks older to the evictor than it is.
    if let Ok(file) = fs::OpenOptions::new().append(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

fn io_failure(what: &str, e: &std::io::Error) -> NetFailure {
    NetFailure {
        code: ErrorCode::Unknown,
        message: format!("Aurora could not {what}"),
        cause: e.to_string(),
        actions: vec![ErrorAction::Retry],
        retryable: true,
    }
}

/// How many images one prefetch pass will consider.
pub const DEFAULT_PREFETCH_LIMIT: u32 = 500;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefetchReport {
    pub downloaded: usize,
    /// Already on disk, so nothing was requested.
    pub cached: usize,
    /// The URL could not be fetched. Left uncached; the next pass tries again.
    pub failed: usize,
    pub evicted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefetchProgress {
    pub done: usize,
    pub total: usize,
}

/// Download the library's artwork into the cache, then evict down to the budget.
///
/// A failure never stops the pass: one dead poster URL among five hundred must not cost
/// the other four hundred and ninety-nine.
pub fn prefetch(
    db: &aurora_db::rusqlite::Connection,
    http: &HttpClient,
    cache: &Cache,
    limit: u32,
    mut on_progress: impl FnMut(PrefetchProgress),
) -> aurora_db::Result<PrefetchReport> {
    let urls = aurora_db::repo::enrichment::artwork_urls(db, limit)?;
    let total = urls.len();
    let mut report = PrefetchReport::default();

    for (done, url) in urls.iter().enumerate() {
        on_progress(PrefetchProgress { done, total });
        if cache.contains(url) {
            report.cached += 1;
            continue;
        }
        match cache.fetch(http, url) {
            Ok(_) => report.downloaded += 1,
            Err(e) => {
                tracing::debug!(
                    "artwork fetch failed for {}: {}",
                    crate::http::redact(url),
                    e.message
                );
                report.failed += 1;
            }
        }
    }

    on_progress(PrefetchProgress { done: total, total });
    report.evicted = cache.evict();
    Ok(report)
}

/// The URL a WebView can load a cached file from.
///
/// Tauri 2 serves local files through its asset protocol, which spells itself
/// differently per platform: `http://asset.localhost/...` on Windows and Android,
/// `asset://localhost/...` elsewhere. Building the string here rather than in the UI
/// keeps the UI from having to know the cache exists at all.
///
/// **Unverified on hardware.** The format is from Tauri's documentation; nothing has run
/// the app to confirm the WebView serves it (docs/ROADMAP.md).
pub fn asset_url(path: &Path) -> String {
    let encoded = urlencode_path(&path.to_string_lossy());
    if cfg!(any(windows, target_os = "android")) {
        format!("http://asset.localhost/{encoded}")
    } else {
        format!("asset://localhost/{encoded}")
    }
}

/// Percent-encode a path for a URL, leaving the separators that make it a path.
fn urlencode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | '/' | ':' => out.push(ch),
            // Windows separators become forward slashes; the protocol handler maps back.
            '\\' => out.push('/'),
            _ => {
                let mut buf = [0u8; 4];
                for b in ch.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HttpConfig;
    use crate::testserver::{Reply, TestServer};

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aurora-art-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn http() -> HttpClient {
        HttpClient::new(HttpConfig::default()).unwrap()
    }

    #[test]
    fn the_key_is_stable_and_keeps_the_extension() {
        let url = "https://image.tmdb.org/t/p/w342/abc.jpg";
        assert_eq!(key_for(url), key_for(url), "the key must not vary per call");
        assert!(key_for(url).ends_with(".jpg"));
        assert_ne!(
            key_for(url),
            key_for("https://image.tmdb.org/t/p/w1280/abc.jpg")
        );
    }

    #[test]
    fn a_query_string_does_not_hide_the_extension() {
        assert!(key_for("https://example.com/a.png?v=2").ends_with(".png"));
        // And an unknown extension is not trusted onto the filesystem.
        assert!(key_for("https://example.com/a.php?x=1").ends_with(".img"));
        assert!(key_for("https://example.com/noext").ends_with(".img"));
    }

    #[test]
    fn an_extension_from_the_url_cannot_be_anything_it_likes() {
        // A URL ending in something executable must not produce a file named that way.
        assert!(key_for("https://example.com/evil.exe").ends_with(".img"));
        assert!(key_for("https://example.com/x.jpg.ps1").ends_with(".img"));
    }

    #[test]
    fn a_fetched_image_lands_on_disk_and_is_reused() {
        let server = TestServer::always(Reply::ok(vec![1u8; 4096]));
        let dir = tempdir("hit");
        let cache = Cache::new(&dir);
        let url = server.url("/p.jpg");

        assert!(!cache.contains(&url));
        let path = cache.fetch(&http(), &url).unwrap();
        assert_eq!(fs::read(&path).unwrap().len(), 4096);
        assert!(cache.contains(&url));

        // Second call is a cache hit: no second request reaches the server.
        cache.fetch(&http(), &url).unwrap();
        assert_eq!(server.request_count(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_folder_is_created_on_first_use() {
        let server = TestServer::always(Reply::ok(vec![1u8; 10]));
        let dir = tempdir("mkdir").join("nested").join("artwork");
        let cache = Cache::new(&dir);
        cache.fetch(&http(), &server.url("/p.jpg")).unwrap();
        assert!(dir.is_dir());
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_missing_image_is_a_readable_failure_and_caches_nothing() {
        let server = TestServer::always(Reply::status(404));
        let dir = tempdir("404");
        let cache = Cache::new(&dir);
        let url = server.url("/gone.jpg");

        let err = cache.fetch(&http(), &url).unwrap_err();
        assert!(!err.message.is_empty());
        assert!(
            !cache.contains(&url),
            "a failure must not leave a cache entry"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_body_is_refused_rather_than_cached_as_a_broken_image() {
        let server = TestServer::always(Reply::ok(Vec::new()));
        let dir = tempdir("empty");
        let cache = Cache::new(&dir);
        let url = server.url("/blank.jpg");

        assert!(cache.fetch(&http(), &url).is_err());
        // Nothing on disk, so the next attempt tries again instead of serving nothing.
        assert!(!cache.contains(&url));
        assert_eq!(cache.len(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_zero_byte_file_is_not_treated_as_a_hit() {
        let dir = tempdir("zero");
        fs::create_dir_all(&dir).unwrap();
        let cache = Cache::new(&dir);
        let url = "https://example.com/p.jpg";
        // Simulate a download killed mid-write.
        fs::write(cache.path_for(url), b"").unwrap();

        assert!(
            !cache.contains(url),
            "a crashed download is not a cached image"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_half_written_part_file_is_not_cache_contents() {
        let dir = tempdir("part");
        fs::create_dir_all(&dir).unwrap();
        let cache = Cache::new(&dir);
        fs::write(dir.join("abc.part"), vec![1u8; 500]).unwrap();

        assert_eq!(cache.len(), 0);
        assert_eq!(cache.total_bytes(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn eviction_removes_the_oldest_until_it_fits() {
        let dir = tempdir("evict");
        fs::create_dir_all(&dir).unwrap();
        let cache = Cache::new(&dir).with_max_bytes(2_500);

        // Three 1,000-byte files with increasing modification times.
        for (i, name) in ["old.jpg", "mid.jpg", "new.jpg"].iter().enumerate() {
            let path = dir.join(name);
            fs::write(&path, vec![0u8; 1_000]).unwrap();
            let when = UNIX_EPOCH + std::time::Duration::from_secs(1_000_000 + i as u64 * 100);
            fs::File::options()
                .append(true)
                .open(&path)
                .unwrap()
                .set_modified(when)
                .unwrap();
        }
        assert_eq!(cache.total_bytes(), 3_000);

        assert_eq!(cache.evict(), 1);
        assert!(!dir.join("old.jpg").exists(), "the oldest goes first");
        assert!(dir.join("mid.jpg").exists());
        assert!(dir.join("new.jpg").exists());
        assert_eq!(cache.total_bytes(), 2_000);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn eviction_does_nothing_when_the_cache_already_fits() {
        let dir = tempdir("fits");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.jpg"), vec![0u8; 100]).unwrap();
        let cache = Cache::new(&dir).with_max_bytes(10_000);

        assert_eq!(cache.evict(), 0);
        assert_eq!(cache.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_absent_cache_folder_reads_as_empty_rather_than_failing() {
        let cache = Cache::new(tempdir("absent"));
        assert_eq!(cache.total_bytes(), 0);
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
        assert_eq!(cache.evict(), 0);
        assert_eq!(cache.clear(), 0);
    }

    #[test]
    fn clearing_empties_the_cache_without_removing_the_folder() {
        let dir = tempdir("clear");
        fs::create_dir_all(&dir).unwrap();
        for name in ["a.jpg", "b.jpg"] {
            fs::write(dir.join(name), vec![0u8; 10]).unwrap();
        }
        let cache = Cache::new(&dir);

        assert_eq!(cache.clear(), 2);
        assert!(cache.is_empty());
        assert!(
            dir.is_dir(),
            "the folder survives so the next fetch needs no mkdir"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    fn seeded_db(urls: &[&str]) -> aurora_db::rusqlite::Connection {
        let conn = aurora_db::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        for (i, url) in urls.iter().enumerate() {
            conn.execute(
                "INSERT INTO movies (provider_id,provider_key,title,match_key,url,poster,last_seen_at)
                 VALUES (1,?1,?2,?2,'https://example.com/m.mkv',?3,0)",
                aurora_db::rusqlite::params![format!("m{i}"), format!("Film {i}"), url],
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn prefetch_downloads_what_is_missing_and_skips_what_is_not() {
        let server = TestServer::always(Reply::ok(vec![7u8; 2_048]));
        let dir = tempdir("prefetch");
        let cache = Cache::new(&dir);
        let urls: Vec<String> = (0..3).map(|i| server.url(&format!("/p{i}.jpg"))).collect();
        let db = seeded_db(&urls.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        let report = prefetch(&db, &http(), &cache, 100, |_| {}).unwrap();
        assert_eq!(report.downloaded, 3);
        assert_eq!(report.cached, 0);
        assert_eq!(report.failed, 0);
        assert_eq!(cache.len(), 3);

        // Second pass touches the network for nothing.
        let before = server.request_count();
        let again = prefetch(&db, &http(), &cache, 100, |_| {}).unwrap();
        assert_eq!(again.cached, 3);
        assert_eq!(again.downloaded, 0);
        assert_eq!(server.request_count(), before);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_dead_url_does_not_cost_the_rest_of_the_pass() {
        // The second request 404s; the first and third must still land.
        let server = TestServer::start(|n, _| {
            if n == 1 {
                Reply::status(404)
            } else {
                Reply::ok(vec![1u8; 512])
            }
        });
        let dir = tempdir("partial");
        let cache = Cache::new(&dir);
        let urls: Vec<String> = (0..3).map(|i| server.url(&format!("/p{i}.jpg"))).collect();
        let db = seeded_db(&urls.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        let report = prefetch(&db, &http(), &cache, 100, |_| {}).unwrap();
        assert_eq!(report.downloaded, 2);
        assert_eq!(report.failed, 1);
        assert_eq!(cache.len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefetch_evicts_down_to_the_budget_when_it_is_done() {
        let server = TestServer::always(Reply::ok(vec![9u8; 1_000]));
        let dir = tempdir("prefetch-evict");
        // Room for two of the three images.
        let cache = Cache::new(&dir).with_max_bytes(2_000);
        let urls: Vec<String> = (0..3).map(|i| server.url(&format!("/p{i}.jpg"))).collect();
        let db = seeded_db(&urls.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        let report = prefetch(&db, &http(), &cache, 100, |_| {}).unwrap();
        assert_eq!(report.downloaded, 3);
        assert!(
            report.evicted >= 1,
            "the cache must be brought back inside its budget"
        );
        assert!(cache.total_bytes() <= 2_000);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefetch_progress_runs_to_completion() {
        let server = TestServer::always(Reply::ok(vec![1u8; 100]));
        let dir = tempdir("prefetch-progress");
        let cache = Cache::new(&dir);
        let urls: Vec<String> = (0..2).map(|i| server.url(&format!("/p{i}.jpg"))).collect();
        let db = seeded_db(&urls.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        let mut seen = Vec::new();
        prefetch(&db, &http(), &cache, 100, |p| seen.push((p.done, p.total))).unwrap();
        assert_eq!(seen.first(), Some(&(0, 2)));
        assert_eq!(seen.last(), Some(&(2, 2)));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_library_with_no_artwork_prefetches_nothing() {
        let dir = tempdir("prefetch-empty");
        let cache = Cache::new(&dir);
        let db = seeded_db(&[]);
        let report = prefetch(&db, &http(), &cache, 100, |_| {}).unwrap();
        assert_eq!(report, PrefetchReport::default());
    }

    #[test]
    fn an_asset_url_is_percent_encoded_and_keeps_its_separators() {
        let url = asset_url(Path::new("/home/user/art/ab12.jpg"));
        assert!(url.ends_with("/home/user/art/ab12.jpg"), "{url}");
        assert!(
            url.starts_with("asset://localhost/") || url.starts_with("http://asset.localhost/")
        );

        // A space in the path must not break the URL.
        let spaced = asset_url(Path::new("/home/a user/art/ab12.jpg"));
        assert!(spaced.contains("a%20user"), "{spaced}");
        assert!(!spaced.contains(' '));
    }

    #[test]
    fn a_windows_path_becomes_a_url_path() {
        let url = asset_url(Path::new(r"C:\Users\You\AppData\art\ab12.jpg"));
        assert!(!url.contains('\\'), "{url}");
        assert!(url.contains("C:/Users/You/AppData/art/ab12.jpg"), "{url}");
    }

    #[test]
    fn two_urls_for_the_same_file_are_cached_separately() {
        // Deliberate: the URL is the identity. Deduplicating by content would mean
        // hashing every download twice for a saving an image cache does not need.
        let dir = tempdir("distinct");
        let cache = Cache::new(&dir);
        assert_ne!(
            cache.path_for("https://a.example.com/p.jpg"),
            cache.path_for("https://b.example.com/p.jpg")
        );
    }
}
