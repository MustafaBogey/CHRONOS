//! Optional browser integration endpoint.
//!
//! A tiny localhost HTTP listener (std only) that accepts website attribution hints from the
//! CHRONOS browser extension. The extension POSTs the active tab domain plus the pairing
//! token shown in Settings → Integrations. Without the extension, CHRONOS never guesses
//! domains from window titles.
//!
//! `GET /pair` serves a small local pairing page. When the CHRONOS extension is installed
//! in the visiting browser, its content script reads the pairing token from this page and
//! stores it in extension-local storage — no manual token copying. The page is served on
//! loopback only, is never cached, and displays no secrets.

use crate::tracker::TrackerHandle;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

pub const INTEGRATION_PORT: u16 = 53711;

pub fn token() -> String {
    // Stable per-install token, generated on first use.
    if let Ok(t) = std::env::var("CHRONOS_EXT_TOKEN") {
        return t;
    }
    // Caller (lib.rs) stores the token in the settings table; fall back to a constant name.
    "pair-in-settings".to_string()
}

/// What the listener should do with an incoming request.
#[derive(Debug, PartialEq)]
enum Routed {
    /// A well-formed attribution hint carrying the given (unverified) token and domain.
    DomainHint { token: String, domain: String },
    /// The local pairing page.
    PairPage,
    /// The distraction block list for the browser extension (token in query string).
    BlockList { token: String },
    /// A browser extension's enforcement acknowledgment (applied rule count).
    BlockListAck { token: String, applied: i64, enabled: bool },
    /// Anything else: reject.
    Invalid,
}

fn route(text: &str) -> Routed {
    if text.starts_with("GET /pair") {
        return Routed::PairPage;
    }
    if text.starts_with("GET /blocklist") {
        // Token travels in the query string (fetched from extension context, not a page).
        let query = text.split_whitespace().nth(1).unwrap_or("");
        let token = query
            .split(&['?', '&'][..])
            .find_map(|kv| kv.strip_prefix("token="))
            .unwrap_or("");
        return Routed::BlockList { token: token.to_string() };
    }
    if text.starts_with("POST /blocklist-ack") {
        let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
        let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
            return Routed::Invalid;
        };
        let token = v.get("token").and_then(|t| t.as_str()).unwrap_or("");
        if token.is_empty() {
            return Routed::Invalid;
        }
        let applied = v.get("applied").and_then(|a| a.as_i64()).unwrap_or(0).clamp(0, 100_000);
        let enabled = v.get("enabled").and_then(|e| e.as_bool()).unwrap_or(false);
        return Routed::BlockListAck { token: token.to_string(), applied, enabled };
    }
    if !text.starts_with("POST /domain") {
        return Routed::Invalid;
    }
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return Routed::Invalid;
    };
    let token = v.get("token").and_then(|t| t.as_str()).unwrap_or("");
    let domain = v.get("domain").and_then(|d| d.as_str()).unwrap_or("");
    if token.is_empty() || domain.is_empty() {
        return Routed::Invalid;
    }
    // Only accept well-formed domains.
    if domain.len() > 253 || domain.contains(' ') || domain.contains('/') {
        return Routed::Invalid;
    }
    Routed::DomainHint { token: token.to_string(), domain: domain.to_string() }
}

/// JSON the extension consumes to sync declarativeNetRequest redirect rules.
/// `enabled` mirrors the blocking toggle; `domains` are the enabled website
/// targets. The extension treats any non-200 response as "no blocking".
fn blocklist_json(conn: &rusqlite::Connection) -> String {
    let blocking: bool = conn
        .query_row("SELECT value FROM settings WHERE key='blocking'", [], |r| r.get::<_, String>(0))
        .map(|v| v == "1")
        .unwrap_or(false);
    let domains: Vec<String> = conn
        .prepare("SELECT target FROM block_list WHERE enabled = 1 AND kind = 'domain' ORDER BY id")
        .ok()
        .and_then(|mut stmt| {
            stmt.query_map([], |r| r.get::<_, String>(0))
                .map(|it| it.filter_map(|x| x.ok()).collect())
                .ok()
        })
        .unwrap_or_default();
    let list = serde_json::to_string(&domains).unwrap_or_else(|_| "[]".into());
    format!("{{\"enabled\":{},\"domains\":{list}}}", if blocking { "true" } else { "false" })
}

/// The local pairing page. The token is embedded as a data attribute for the extension's
/// content script (charset is hex/dash, so no HTML escaping is required) and is never
/// rendered visibly. No caching.
fn pair_page(token: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>CHRONOS browser pairing</title>
<style>
  body {{ font-family: system-ui, sans-serif; background: #080f22; color: #f3f1ff; margin: 0;
         min-height: 100vh; display: grid; place-items: center; }}
  main {{ max-width: 34rem; padding: 2.5rem 1.5rem; text-align: center; }}
  h1 {{ font-size: 1.4rem; font-weight: 550; margin: 0 0 1.2rem; }}
  h1 span {{ color: #b47aff; }}
  .state {{ font-size: 1.05rem; line-height: 1.6; margin: 0 0 1.4rem; }}
  .state.waiting {{ color: #b1b9d8; }}
  .state.paired {{ color: #09e5ac; }}
  .hint {{ font-size: .85rem; line-height: 1.7; color: #b1b9d8; }}
  .dim {{ opacity: .8; margin-top: 1rem; }}
  code {{ color: #d9c8ff; }}
</style>
</head>
<body data-token="{token}">
<main>
  <h1><span>CHRONOS</span> browser pairing</h1>
  <p id="state" class="state waiting">Waiting for the CHRONOS extension&hellip;</p>
  <p class="hint">This page is served by the CHRONOS desktop app on this machine (127.0.0.1 only).
     If the CHRONOS extension is installed in this browser, this tab pairs automatically
     and can be closed.</p>
  <p class="hint dim">Nothing happening? The extension is not installed in this browser yet.
     Follow the steps under CHRONOS &rarr; Settings &amp; Integrations &rarr; Browser connection.</p>
</main>
</body>
</html>
"#
    )
}

pub fn start(tracker: Arc<TrackerHandle>, conn: Arc<parking_lot::Mutex<rusqlite::Connection>>, expected_token: String) {
    std::thread::Builder::new()
        .name("chronos-browser-ipc".into())
        .spawn(move || {
            let listener = match TcpListener::bind(("127.0.0.1", INTEGRATION_PORT)) {
                Ok(l) => l,
                Err(_) => return, // port busy: integration simply unavailable
            };
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                match handle_request(&mut stream, &tracker, &conn, &expected_token) {
                    Outcome::HintAccepted => {
                        let _ = write_response(&mut stream, 200, "application/json", "{\"ok\":true}", None);
                    }
                    Outcome::PairPage => {
                        let page = pair_page(&expected_token);
                        let _ = write_response(&mut stream, 200, "text/html; charset=utf-8", &page, Some("no-store"));
                    }
                    Outcome::BlockList(body) => {
                        let _ = write_response(&mut stream, 200, "application/json", &body, Some("no-store"));
                    }
                    Outcome::BlockListAcked => {
                        let _ = write_response(&mut stream, 200, "application/json", "{\"ok\":true}", None);
                    }
                    Outcome::Rejected => {
                        let _ = write_response(&mut stream, 403, "application/json", "{\"ok\":false}", None);
                    }
                }
            }
        })
        .ok();
}

enum Outcome {
    HintAccepted,
    PairPage,
    BlockList(String),
    BlockListAcked,
    Rejected,
}

fn handle_request(stream: &mut TcpStream, tracker: &Arc<TrackerHandle>, conn: &Arc<parking_lot::Mutex<rusqlite::Connection>>, expected_token: &str) -> Outcome {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = [0u8; 2048];
    let Ok(n) = stream.read(&mut buf) else { return Outcome::Rejected };
    let text = String::from_utf8_lossy(&buf[..n]).to_string();
    match route(&text) {
        Routed::PairPage => Outcome::PairPage,
        Routed::BlockList { token } => {
            if token != expected_token {
                return Outcome::Rejected;
            }
            let c = conn.lock();
            Outcome::BlockList(blocklist_json(&c))
        }
        Routed::BlockListAck { token, applied, enabled } => {
            if token != expected_token {
                return Outcome::Rejected;
            }
            // Persist the acknowledgment: the only honest evidence that browser
            // enforcement is real. Frontend status derives from these values.
            let c = conn.lock();
            let _ = c.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('blocklist_ack_ms', ?1)",
                [crate::db::now_ms().to_string()],
            );
            let _ = c.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('blocklist_ack_applied', ?1)",
                [applied.to_string()],
            );
            let _ = c.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('blocklist_ack_enabled', ?1)",
                [if enabled { "1" } else { "0" }.to_string()],
            );
            Outcome::BlockListAcked
        }
        Routed::DomainHint { token, domain } => {
            if token != expected_token {
                return Outcome::Rejected;
            }
            tracker.set_domain_hint(domain);
            Outcome::HintAccepted
        }
        Routed::Invalid => Outcome::Rejected,
    }
}

fn write_response(
    stream: &mut TcpStream,
    code: u16,
    content_type: &str,
    body: &str,
    cache_control: Option<&str>,
) -> std::io::Result<()> {
    let reason = if code == 200 { "OK" } else { "Forbidden" };
    let cache = cache_control
        .map(|c| format!("\r\nCache-Control: {c}"))
        .unwrap_or_default();
    stream.write_all(
        format!(
            "HTTP/1.1 {code} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}{cache}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(body: &str) -> String {
        format!("POST /domain HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\r\n{body}")
    }

    #[test]
    fn routes_domain_hint_with_valid_payload() {
        let r = route(&post(r#"{"token":"abc","domain":"example.com"}"#));
        assert_eq!(r, Routed::DomainHint { token: "abc".into(), domain: "example.com".into() });
    }

    #[test]
    fn rejects_malformed_or_unsafe_requests() {
        assert_eq!(route(&post(r#"{"token":"","domain":"example.com"}"#)), Routed::Invalid);
        assert_eq!(route(&post(r#"{"token":"abc","domain":""}"#)), Routed::Invalid);
        assert_eq!(route(&post(r#"{"token":"abc","domain":"bad domain"}"#)), Routed::Invalid);
        assert_eq!(route(&post(r#"{"token":"abc","domain":"a/b"}"#)), Routed::Invalid);
        assert_eq!(route(&post(r#"{"token":"abc","domain":""}"#.repeat(1).as_str())), Routed::Invalid);
        let long = "a".repeat(254);
        assert_eq!(route(&post(&format!(r#"{{"token":"abc","domain":"{long}"}}"#))), Routed::Invalid);
        assert_eq!(route("GET /other HTTP/1.1\r\n\r\n"), Routed::Invalid);
        assert_eq!(route("DELETE /domain HTTP/1.1\r\n\r\n"), Routed::Invalid);
        assert_eq!(route("garbage"), Routed::Invalid);
    }

    #[test]
    fn routes_pair_page() {
        assert_eq!(route("GET /pair HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"), Routed::PairPage);
    }

    #[test]
    fn routes_blocklist_with_query_token() {
        assert_eq!(
            route("GET /blocklist?token=abc HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"),
            Routed::BlockList { token: "abc".into() }
        );
        assert_eq!(
            route("GET /blocklist?x=1&token=abc HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"),
            Routed::BlockList { token: "abc".into() }
        );
        assert_eq!(
            route("GET /blocklist HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"),
            Routed::BlockList { token: String::new() }
        );
    }

    #[test]
    fn routes_blocklist_ack_with_payload() {
        assert_eq!(
            route(&post_ack(r#"{"token":"abc","applied":2,"enabled":true}"#)),
            Routed::BlockListAck { token: "abc".into(), applied: 2, enabled: true }
        );
        assert_eq!(route(&post_ack(r#"{"token":"abc","applied":0,"enabled":false}"#)),
            Routed::BlockListAck { token: "abc".into(), applied: 0, enabled: false });
        // Missing or empty token is rejected; applied is clamped, not trusted.
        assert_eq!(route(&post_ack(r#"{"applied":1,"enabled":true}"#)), Routed::Invalid);
        assert_eq!(route(&post_ack(r#"{"token":"","applied":1,"enabled":true}"#)), Routed::Invalid);
        assert_eq!(route(&post_ack(r#"{"token":"abc","applied":-5,"enabled":false}"#)),
            Routed::BlockListAck { token: "abc".into(), applied: 0, enabled: false });
        assert_eq!(route("POST /blocklist-ack HTTP/1.1\r\nHost: x\r\n\r\tnot json"), Routed::Invalid);
    }

    fn post_ack(body: &str) -> String {
        format!("POST /blocklist-ack HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\r\n{body}")
    }

    #[test]
    fn blocklist_json_reflects_settings_and_targets() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn.execute("INSERT INTO block_list (label, kind, target, enabled, created_at) VALUES ('YouTube','domain','youtube.com',1,0)", []).unwrap();
        conn.execute("INSERT INTO block_list (label, kind, target, enabled, created_at) VALUES ('Off','domain','off.com',0,0)", []).unwrap();
        conn.execute("INSERT INTO block_list (label, kind, target, enabled, created_at) VALUES ('Steam','app','steam',1,0)", []).unwrap();
        let body = blocklist_json(&conn);
        assert_eq!(body, "{\"enabled\":false,\"domains\":[\"youtube.com\"]}", "app targets and disabled rows never reach the browser");
        conn.execute("INSERT INTO settings (key, value) VALUES ('blocking', '1')", []).unwrap();
        assert_eq!(blocklist_json(&conn), "{\"enabled\":true,\"domains\":[\"youtube.com\"]}");
    }

    #[test]
    fn pair_page_embeds_token_invisibly() {
        let page = pair_page("abcd1234-90ef");
        assert!(page.contains(r#"<body data-token="abcd1234-90ef">"#));
        // The token must not be rendered as visible text.
        assert!(!page.contains(">abcd1234-90ef<"));
        // No script of our own: the extension's content script does the pairing.
        assert!(!page.contains("<script"));
    }
}
