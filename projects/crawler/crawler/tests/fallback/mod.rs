//! Render-fallback routing acceptance.

use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use crawler::{
    CrawlOptions, FixtureBrowserProvider, HtmlExtractor, HttpBrowserProvider, MemoryEntry, MemoryTransport, PageYield,
    Transport, run_crawl_with_options, run_extract,
    types::{
        BrowserFallbackPolicy, BrowserFallbackTrigger, BrowserSnapshot, ChallengePolicy, CrawlEvent, CrawlRequest,
        ExtractBudget, ExtractContext, ExtractResult, FetchArtifact, PageChallengeState,
    },
};
use url::Url;

const JS_SHELL: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>JS 骨架页面</title></head>
<body>
<main><div id="mount">加载中</div></main>
<script>
window.addEventListener("load", () => {
    setTimeout(() => {
        document.title = "浏览器渲染文章样本";
        document.getElementById("mount").innerHTML =
            "<article>" +
            "<h1>浏览器渲染文章样本</h1>" +
            "<p>渲染正文标记：脚本注入后才会出现。</p>" +
            "<p>这一段由页面内联脚本在 load 事件之后写入 DOM，HTTP 直取只能拿到骨架。</p>" +
            "<h2>渲染小节</h2>" +
            "<p>第二段渲染正文用于确认注入后的段落与标题都能进入 notedown-ir。</p>" +
            "</article>";
    }, 0);
});
</script>
</body>
</html>
"#;

const STATIC_ARTICLE: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>静态文章样本</title></head>
<body>
<main>
<article>
<h1>静态文章样本</h1>
<p>静态正文标记：直取即可抽取。</p>
<p>这一段写在服务端返回的 HTML 里，HTTP 直取不需要执行脚本就能看到。</p>
<h2>静态小节</h2>
<p>第二段正文用于确认段落与标题都能进入 <code>notedown-ir</code>。</p>
</article>
</main>
</body>
</html>
"#;

const RENDERED_ARTICLE: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>浏览器渲染文章样本</title></head>
<body>
<main>
<article>
<h1>浏览器渲染文章样本</h1>
<p>渲染正文标记：脚本注入后才会出现。</p>
<p>这一段由页面内联脚本在 load 事件之后写入 DOM，HTTP 直取只能拿到骨架。</p>
<h2>渲染小节</h2>
<p>第二段渲染正文用于确认注入后的段落与标题都能进入 notedown-ir。</p>
</article>
</main>
</body>
</html>
"#;

fn request_for(url: &Url) -> CrawlRequest {
    let mut request = CrawlRequest::from_seed(url.as_str()).expect("seed");
    request.budget.max_requests = 1;
    request.budget.max_depth = 0;
    request
}

fn snapshot_for(url: &Url, document_html: &str) -> BrowserSnapshot {
    BrowserSnapshot {
        requested_url: url.clone(),
        final_url: url.clone(),
        document_html: document_html.to_string(),
        captured_at_epoch: 1_700_000_000,
        browser_engine: "fixture-chromium".into(),
        profile_id: "fixture".into(),
        challenge_state: PageChallengeState::Normal,
        boundary: Default::default(),
    }
}

fn extract(artifact: &FetchArtifact) -> ExtractResult {
    let extractor = HtmlExtractor;
    run_extract(
        artifact,
        &[&extractor as &dyn crawler::Extractor],
        ExtractBudget::default(),
        &ExtractContext { page_identity: Some(artifact.request_url.to_string()) },
    )
}

fn html_transport(url: &Url, html: &str) -> MemoryTransport {
    let mut transport = MemoryTransport::new();
    transport.insert(
        url.as_str(),
        MemoryEntry {
            status: 200,
            headers: [("content-type".to_string(), "text/html; charset=utf-8".to_string())].into(),
            body: html.as_bytes().to_vec(),
        },
    );
    transport
}

fn fallback_count(output: &crawler::CrawlOutput) -> usize {
    output.report.events.events().iter().filter(|event| matches!(event, CrawlEvent::BrowserFallback { .. })).count()
}

#[test]
fn js_shell_routes_to_render_and_keeps_body() {
    let url = Url::parse("https://example.test/js-shell.html").expect("url");
    let transport = html_transport(&url, JS_SHELL);

    let mut browser = FixtureBrowserProvider::new();
    let rendered_url = url.clone();
    browser.insert(snapshot_for(&rendered_url, RENDERED_ARTICLE));

    let options = CrawlOptions {
        browser: Some(&browser),
        browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
        challenge_policy: ChallengePolicy::Stop,
        ..CrawlOptions::default()
    };
    let output = run_crawl_with_options(request_for(&url), transport, &[&HtmlExtractor], options).expect("crawl");

    let direct = {
        let artifact = crawler_fetch_artifact(&url, JS_SHELL);
        let extraction = extract(&artifact);
        PageYield::measure(&artifact, &extraction)
    };
    let delivered = extract(&output.artifacts[0]);
    let delivered_yield = PageYield::measure(&output.artifacts[0], &delivered);

    assert_eq!(output.report.committed, 1);
    assert_eq!(fallback_count(&output), 1);
    let fallback_event = output
        .report
        .events
        .events()
        .iter()
        .find_map(|event| match event {
            CrawlEvent::BrowserFallback { trigger, observed_text_chars, observed_content_blocks, .. } => {
                Some((trigger, observed_text_chars, observed_content_blocks))
            }
            _ => None,
        })
        .expect("browser_fallback event");
    assert_eq!(*fallback_event.0, BrowserFallbackTrigger::LowYield);
    assert_eq!(fallback_event.1, &Some(direct.text_chars));
    assert_eq!(fallback_event.2, &Some(direct.content_blocks));
    assert!(output.artifacts[0].body_bytes().is_some_and(|body| String::from_utf8_lossy(body).contains("渲染正文标记")));
    assert!(direct.is_low(), "direct fetch must remain low yield");
    assert!(delivered_yield.text_chars >= 64, "rendered body must clear the threshold");

    println!(
        "js_shell: direct={}/{} blocks low={} rendered={}/{} blocks fallbacks={}",
        direct.text_chars,
        direct.content_blocks,
        direct.is_low(),
        delivered_yield.text_chars,
        delivered_yield.content_blocks,
        fallback_count(&output)
    );
}

#[test]
fn static_page_stays_on_http_path() {
    let url = Url::parse("https://example.test/static.html").expect("url");
    let transport = html_transport(&url, STATIC_ARTICLE);

    let mut browser = FixtureBrowserProvider::new();
    browser.insert(snapshot_for(&url, RENDERED_ARTICLE));

    let options = CrawlOptions {
        browser: Some(&browser),
        browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
        ..CrawlOptions::default()
    };
    let output = run_crawl_with_options(request_for(&url), transport, &[&HtmlExtractor], options).expect("crawl");

    let delivered = extract(&output.artifacts[0]);
    let delivered_yield = PageYield::measure(&output.artifacts[0], &delivered);

    assert_eq!(output.report.committed, 1);
    assert_eq!(fallback_count(&output), 0);
    assert!(output.artifacts[0].body_bytes().is_some_and(|body| String::from_utf8_lossy(body).contains("静态正文标记")));
    assert!(!delivered_yield.is_low());

    println!(
        "static: rendered={}/{} blocks low={} fallbacks={}",
        delivered_yield.text_chars,
        delivered_yield.content_blocks,
        delivered_yield.is_low(),
        fallback_count(&output)
    );
}

#[test]
fn missing_fixture_reports_structured_diagnostic() {
    let url = Url::parse("https://example.test/js-shell.html").expect("url");
    let transport = html_transport(&url, JS_SHELL);
    let browser = FixtureBrowserProvider::new();

    let options = CrawlOptions {
        browser: Some(&browser),
        browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
        ..CrawlOptions::default()
    };
    let output = run_crawl_with_options(request_for(&url), transport, &[&HtmlExtractor], options).expect("crawl");

    assert_eq!(output.report.committed, 0);
    assert_eq!(output.report.failed, 1);
    let failure = output
        .report
        .events
        .events()
        .iter()
        .find_map(|event| match event {
            CrawlEvent::BrowserFallbackFailed { trigger, diagnostic_code, provider_code, reason, .. } => {
                Some((trigger, diagnostic_code, provider_code, reason))
            }
            _ => None,
        })
        .expect("browser_fallback_failed event");
    assert_eq!(*failure.0, BrowserFallbackTrigger::LowYield);
    assert_eq!(failure.1.as_str(), "crawler.browser.unavailable");
    assert!(failure.2.is_none());
    assert!(failure.3.contains("no fixture snapshot"));

    let serialized = serde_json::to_string(&output.report).expect("report json");
    assert!(serialized.contains("crawler.browser.unavailable"));

    println!("missing_fixture: diagnostic={} reason={}", failure.1, failure.3);
}

#[test]
fn worker_413_reports_provider_code() {
    let page_url = Url::parse("https://example.test/js-shell.html").expect("url");
    let transport = html_transport(&page_url, JS_SHELL);
    let (worker_url, handle) = serve_once(
        "HTTP/1.1 413 Payload Too Large\r\ncontent-type: application/json\r\nx-pandark-code: pandark.browser.snapshot-too-large\r\ncontent-length: 45\r\nconnection: close\r\n\r\n{\"code\":\"pandark.browser.snapshot-too-large\"}",
    );
    let provider = HttpBrowserProvider::new(worker_url);

    let options = CrawlOptions {
        browser: Some(&provider),
        browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
        ..CrawlOptions::default()
    };
    let output = run_crawl_with_options(request_for(&page_url), transport, &[&HtmlExtractor], options).expect("crawl");
    handle.join().expect("worker thread");

    assert_eq!(output.report.committed, 0);
    assert_eq!(output.report.failed, 1);
    let failure = output
        .report
        .events
        .events()
        .iter()
        .find_map(|event| match event {
            CrawlEvent::BrowserFallbackFailed { trigger, diagnostic_code, provider_code, reason, .. } => {
                Some((trigger, diagnostic_code, provider_code, reason))
            }
            _ => None,
        })
        .expect("browser_fallback_failed event");
    assert_eq!(*failure.0, BrowserFallbackTrigger::LowYield);
    assert_eq!(failure.1.as_str(), "crawler.browser.worker-status");
    assert_eq!(failure.2.as_deref(), Some("pandark.browser.snapshot-too-large"));
    assert!(failure.3.contains("413"));

    println!("worker_413: diagnostic={} provider_code={:?} reason={}", failure.1, failure.2, failure.3);
}

fn browser_crash_error() -> crawler::types::CrawlError {
    use boost_diagnostic::{Diagnostic, DiagnosticSeverity, Message};

    crawler::types::CrawlError::from(Diagnostic::new(
        crawler::types::diagnostics::code(crawler::types::diagnostics::BROWSER_UNAVAILABLE),
        DiagnosticSeverity::Error,
        crawler::types::diagnostics::origin_browser(),
        Message::new("crawler.diagnostic.browserCrash").with_fallback("browser process crashed"),
    ))
}

#[test]
fn browser_crash_fails_closed_and_restart_recovers_once() {
    let url = Url::parse("https://example.test/js-shell.html").expect("url");

    let mut crashed = FixtureBrowserProvider::new();
    crashed.insert_failure(&url, browser_crash_error());
    let options = CrawlOptions {
        browser: Some(&crashed),
        browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
        ..CrawlOptions::default()
    };
    let first = run_crawl_with_options(request_for(&url), html_transport(&url, JS_SHELL), &[&HtmlExtractor], options)
        .expect("crashed crawl");

    assert_eq!(first.report.committed, 0, "a crashed browser must not commit a page");
    assert_eq!(first.report.failed, 1, "a crashed browser fails closed without retrying");
    assert_eq!(fallback_count(&first), 0, "a crashed browser must not report a successful fallback");
    assert_eq!(
        first.report.events.events().iter().filter(|event| matches!(event, CrawlEvent::Committed { .. })).count(),
        0,
        "a crashed browser must not start a page transaction"
    );
    let failures =
        first.report.events.events().iter().filter(|event| matches!(event, CrawlEvent::BrowserFallbackFailed { .. })).count();
    assert_eq!(failures, 1, "crash is terminal for the item, not retried in place");
    let failure = first
        .report
        .events
        .events()
        .iter()
        .find_map(|event| match event {
            CrawlEvent::BrowserFallbackFailed { diagnostic_code, reason, .. } => Some((diagnostic_code, reason)),
            _ => None,
        })
        .expect("browser_fallback_failed event");
    assert_eq!(failure.0.as_str(), "crawler.browser.unavailable");
    assert!(failure.1.contains("browser process crashed"));

    let mut restarted = FixtureBrowserProvider::new();
    restarted.insert(snapshot_for(&url, RENDERED_ARTICLE));
    let options = CrawlOptions {
        browser: Some(&restarted),
        browser_fallback: BrowserFallbackPolicy::OnFetchFailure,
        ..CrawlOptions::default()
    };
    let second = run_crawl_with_options(request_for(&url), html_transport(&url, JS_SHELL), &[&HtmlExtractor], options)
        .expect("restarted crawl");

    assert_eq!(second.report.committed, 1, "a healthy restart commits the page exactly once");
    assert_eq!(second.artifacts.len(), 1);
    assert_eq!(
        second.report.events.events().iter().filter(|event| matches!(event, CrawlEvent::Committed { .. })).count(),
        1,
        "restart must not replay the page transaction"
    );
}

fn crawler_fetch_artifact(url: &Url, html: &str) -> FetchArtifact {
    html_transport(url, html).fetch(&crawler::FetchRequest { url: url.clone(), headers: Default::default() }).expect("artifact")
}

fn serve_once(response: &'static str) -> (Url, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let url = Url::parse(&format!("http://127.0.0.1:{port}/")).expect("worker url");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buffer = [0u8; 4096];
        let _ = stream.read(&mut buffer);
        stream.write_all(response.as_bytes()).expect("write response");
        stream.flush().expect("flush");
    });
    (url, handle)
}
