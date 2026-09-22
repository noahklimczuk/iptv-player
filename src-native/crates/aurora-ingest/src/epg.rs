//! Fetch XMLTV and stream it into the parser, batching straight into the database.
//!
//! README §4.4: a 7-day EPG for 3,000 channels can exceed 1GB uncompressed, so nothing
//! here collects the whole document.

use std::io::BufReader;

use aurora_core::model::{EpgChannel, Programme};
use aurora_core::neterr::NetFailure;
use aurora_core::xmltv::{self, XmltvSink, XmltvStats};

use crate::http::HttpClient;

/// How many programmes accumulate before a flush. Large enough that the per-transaction
/// overhead disappears, small enough that memory stays flat on a 1GB document.
pub const BATCH_SIZE: usize = 2_000;

/// A sink that hands batches to a callback instead of collecting everything.
pub struct BatchingSink<F, G> {
    channels: Vec<EpgChannel>,
    programmes: Vec<Programme>,
    on_channels: F,
    on_programmes: G,
    batch_size: usize,
    /// Set when a callback fails, so parsing can stop reporting success.
    pub error: Option<String>,
}

impl<F, G> BatchingSink<F, G>
where
    F: FnMut(&[EpgChannel]) -> Result<(), String>,
    G: FnMut(&[Programme]) -> Result<(), String>,
{
    pub fn new(on_channels: F, on_programmes: G, batch_size: usize) -> Self {
        Self {
            channels: Vec::new(),
            programmes: Vec::new(),
            on_channels,
            on_programmes,
            batch_size: batch_size.max(1),
            error: None,
        }
    }

    /// Flush whatever is left. Must be called once parsing finishes.
    pub fn finish(&mut self) {
        if !self.channels.is_empty() {
            if let Err(e) = (self.on_channels)(&self.channels) {
                self.error.get_or_insert(e);
            }
            self.channels.clear();
        }
        if !self.programmes.is_empty() {
            if let Err(e) = (self.on_programmes)(&self.programmes) {
                self.error.get_or_insert(e);
            }
            self.programmes.clear();
        }
    }
}

impl<F, G> XmltvSink for BatchingSink<F, G>
where
    F: FnMut(&[EpgChannel]) -> Result<(), String>,
    G: FnMut(&[Programme]) -> Result<(), String>,
{
    fn channel(&mut self, channel: EpgChannel) {
        self.channels.push(channel);
        if self.channels.len() >= self.batch_size {
            if let Err(e) = (self.on_channels)(&self.channels) {
                self.error.get_or_insert(e);
            }
            self.channels.clear();
        }
    }

    fn programme(&mut self, programme: Programme) {
        self.programmes.push(programme);
        if self.programmes.len() >= self.batch_size {
            if let Err(e) = (self.on_programmes)(&self.programmes) {
                self.error.get_or_insert(e);
            }
            self.programmes.clear();
        }
    }
}

/// Fetch an XMLTV document and stream it into `sink`.
pub fn fetch_into<S: XmltvSink>(
    http: &HttpClient,
    url: &str,
    sink: &mut S,
) -> Result<XmltvStats, NetFailure> {
    let reader = http.fetch_reader(url)?;
    xmltv::parse(BufReader::new(reader), sink).map_err(|e| {
        let mut failure = NetFailure::classify("malformed");
        failure.message = "Your EPG source is not valid XMLTV".into();
        failure.cause = e.to_string();
        failure
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpClient, HttpConfig};
    use crate::testserver::{Reply, TestServer};
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    fn client() -> HttpClient {
        HttpClient::new(HttpConfig {
            max_attempts: 1,
            ..Default::default()
        })
        .unwrap()
    }

    fn document(programmes: usize) -> String {
        let mut s = String::from("<tv><channel id=\"a\"><display-name>A</display-name></channel>");
        for i in 0..programmes {
            s.push_str(&format!(
                "<programme start=\"2024011512{:02}00 +0000\" \
                 stop=\"20240115130000 +0000\" channel=\"a\"><title>P{i}</title></programme>",
                i % 60
            ));
        }
        s.push_str("</tv>");
        s
    }

    #[test]
    fn streams_programmes_in_batches() {
        let server = TestServer::always(Reply::ok(document(250)));
        let batches: Arc<Mutex<Vec<usize>>> = Arc::default();
        let total: Arc<Mutex<usize>> = Arc::default();

        let (b, t) = (batches.clone(), total.clone());
        let mut sink = BatchingSink::new(
            |_c| Ok(()),
            move |p: &[Programme]| {
                b.lock().unwrap().push(p.len());
                *t.lock().unwrap() += p.len();
                Ok(())
            },
            100,
        );

        let stats = fetch_into(&client(), &server.url("/epg.xml"), &mut sink).unwrap();
        sink.finish();

        assert_eq!(stats.programmes, 250);
        assert_eq!(
            *total.lock().unwrap(),
            250,
            "every programme must reach the sink"
        );
        let sizes = batches.lock().unwrap().clone();
        assert_eq!(
            sizes,
            vec![100, 100, 50],
            "should flush in batches, remainder last"
        );
    }

    #[test]
    fn handles_a_gzipped_epg() {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(document(3).as_bytes()).unwrap();
        let server = TestServer::always(Reply::ok(e.finish().unwrap()));

        let mut sink = BatchingSink::new(|_| Ok(()), |_| Ok(()), BATCH_SIZE);
        let stats = fetch_into(&client(), &server.url("/epg.xml.gz"), &mut sink).unwrap();
        sink.finish();
        assert_eq!(stats.programmes, 3);
        assert_eq!(stats.channels, 1);
    }

    #[test]
    fn a_sink_failure_is_recorded_rather_than_swallowed() {
        let server = TestServer::always(Reply::ok(document(5)));
        let mut sink = BatchingSink::new(|_| Ok(()), |_| Err("disk full".to_string()), 2);
        fetch_into(&client(), &server.url("/epg.xml"), &mut sink).unwrap();
        sink.finish();
        assert_eq!(sink.error.as_deref(), Some("disk full"));
    }

    #[test]
    fn finish_flushes_a_partial_batch() {
        let server = TestServer::always(Reply::ok(document(3)));
        let seen: Arc<Mutex<usize>> = Arc::default();
        let s = seen.clone();

        let mut sink = BatchingSink::new(
            |_| Ok(()),
            move |p: &[Programme]| {
                *s.lock().unwrap() += p.len();
                Ok(())
            },
            1000,
        );
        fetch_into(&client(), &server.url("/epg.xml"), &mut sink).unwrap();
        assert_eq!(*seen.lock().unwrap(), 0, "nothing flushed before finish");
        sink.finish();
        assert_eq!(*seen.lock().unwrap(), 3);
    }

    #[test]
    fn a_404_is_reported_not_treated_as_an_empty_guide() {
        let server = TestServer::always(Reply::status(404));
        let mut sink = BatchingSink::new(|_| Ok(()), |_| Ok(()), BATCH_SIZE);
        assert!(fetch_into(&client(), &server.url("/epg.xml"), &mut sink).is_err());
    }
}
