use serial_test::serial;
use std::sync::{Arc, Mutex};

use boost_log::{Filter, Level, LogEvent, LogSink, VecSink, clear_global_sink};
use crawler::{FetchRequest, HttpTransport, Transport, map_transport_error};
use url::Url;

struct SharedSink(Arc<Mutex<VecSink>>);

impl LogSink for SharedSink {
    fn emit(&mut self, event: &LogEvent) {
        self.0.lock().expect("vec sink mutex poisoned").emit(event);
    }
}

#[test]
#[serial]
#[cfg(feature = "logger")]
fn map_transport_error_emits_logger_event_when_feature_enabled() {
    clear_global_sink();
    let sink = Arc::new(Mutex::new(VecSink::new()));
    boost_log::set_global_sink(Box::new(SharedSink(sink.clone())));
    boost_log::set_global_filter(Filter::new(Level::Trace));

    let _ = map_transport_error("robots denied");

    let guard = sink.lock().expect("vec sink mutex poisoned");
    assert_eq!(guard.events().len(), 1);
    assert_eq!(guard.events()[0].target(), "crawler.transport");
    assert_eq!(guard.events()[0].level(), Level::Error);
    clear_global_sink();
}

#[test]
#[serial]
fn fetch_failure_maps_to_transport_error() {
    let transport = HttpTransport::new();
    let request = FetchRequest { url: Url::parse("file:///missing.html").expect("url"), headers: Default::default() };
    let error = transport.fetch(&request).expect_err("fetch should fail");
    assert!(matches!(error, crawler_types::CrawlError::Diagnostic(_)));
    assert_eq!(error.code(), Some("crawler.fetch.transport-failed"));
}
