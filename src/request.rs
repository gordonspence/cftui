use anyhow::{bail, Context, Result};
use reqwest::{blocking::Client, redirect::Policy, Url};
use std::{
    io::Read,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

const LIMIT: u64 = 256 * 1024;
pub struct Response {
    pub url: String,
    pub status: u16,
    pub reason: String,
    pub headers_ms: u128,
    pub total_ms: u128,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub bytes: usize,
    pub truncated: bool,
    pub warning: Option<String>,
}
#[derive(Default)]
pub struct Inspector {
    pub url: String,
    pub editing: bool,
    pub body_tab: bool,
    pub scroll: usize,
    pub response: Option<Response>,
    pub error: Option<String>,
    pending: Option<Receiver<Result<Response, String>>>,
}
impl Inspector {
    pub fn send(&mut self) {
        if self.pending.is_some() {
            return;
        }
        let url = self.url.trim().to_owned();
        if let Err(error) = validate(&url) {
            self.error = Some(error.to_string());
            return;
        }
        self.url = url.clone();
        self.response = None;
        self.error = None;
        self.scroll = 0;
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        thread::spawn(move || {
            let _ = tx.send(fetch(&url).map_err(|e| format!("{e:#}")));
            crate::wake();
        });
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn drain(&mut self) -> bool {
        let was_busy = self.busy();
        if let Some(input) = &self.pending {
            match input.try_recv() {
                Ok(Ok(response)) => {
                    self.response = Some(response);
                    self.error = None;
                    self.pending = None;
                }
                Ok(Err(error)) => {
                    self.error = Some(error);
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("Request worker ended unexpectedly".into());
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        was_busy && !self.busy()
    }
}
fn validate(text: &str) -> Result<Url> {
    let url = Url::parse(text).context("Enter a complete URL, including http:// or https://")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("Only HTTP and HTTPS URLs are supported");
    }
    Ok(url)
}
fn fetch(text: &str) -> Result<Response> {
    let url = validate(text)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(5))
        .redirect(Policy::none())
        .build()?;
    let start = Instant::now();
    let response = client
        .get(url)
        .header("User-Agent", "cftui-request-inspector/0.1")
        .send()
        .context("GET failed")?;
    let headers_ms = start.elapsed().as_millis();
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.to_string(),
                value
                    .to_str()
                    .map_or_else(|_| format!("{value:?}"), str::to_owned),
            )
        })
        .collect();
    let mut bytes = Vec::new();
    let warning = response
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .err()
        .map(|e| format!("Body read failed: {e}"));
    let total_ms = start.elapsed().as_millis();
    let truncated = bytes.len() > LIMIT as usize;
    bytes.truncate(LIMIT as usize);
    let body = if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
        format!(
            "Binary or non-UTF-8 response ({} bytes captured). First 128 bytes:\n{}",
            bytes.len(),
            bytes
                .iter()
                .take(128)
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" ")
        )
    } else {
        let text = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
        safe_text(&text)
    };
    Ok(Response {
        url: text.into(),
        status: status.as_u16(),
        reason: status.canonical_reason().unwrap_or("").into(),
        headers_ms,
        total_ms,
        headers,
        body,
        bytes: bytes.len(),
        truncated,
        warning,
    })
}
pub fn safe_text(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() && !matches!(c, '\n' | '\t') {
                format!("\\u{{{:x}}}", c as u32)
            } else {
                c.to_string()
            }
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, net::TcpListener};
    fn server(response: Vec<u8>) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/check", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut received = Vec::new();
            let mut byte = [0; 1];
            while !received.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                received.push(byte[0]);
            }
            let _ = stream.write_all(&response);
            String::from_utf8(received).unwrap()
        });
        (url, handle)
    }
    #[test]
    fn captures_error_status_repeated_headers_and_formats_json() {
        let body = br#"{"error":"missing"}"#;
        let (url,handle)=server(format!("HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nContent-Type: application/json\r\nCF-Ray: test-ray\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\nConnection: close\r\n\r\n{}",body.len(),String::from_utf8_lossy(body)).into_bytes());
        let response = fetch(&url).unwrap();
        assert_eq!(response.status, 404);
        assert_eq!(response.bytes, body.len());
        assert_eq!(
            response
                .headers
                .iter()
                .filter(|(k, _)| k == "set-cookie")
                .count(),
            2
        );
        assert!(response.body.contains("\n  \"error\""));
        assert!(!response.truncated);
        let request = handle.join().unwrap();
        assert!(request.starts_with("GET /check"));
        assert!(!request.to_lowercase().contains("authorization:"));
    }
    #[test]
    fn leaves_redirect_visible_and_caps_large_bodies() {
        let (url,handle)=server(b"HTTP/1.1 302 Found\r\nLocation: /elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec());
        assert_eq!(fetch(&url).unwrap().status, 302);
        handle.join().unwrap();
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            LIMIT + 100
        )
        .into_bytes();
        response.extend(vec![b'x'; LIMIT as usize + 100]);
        let (url, handle) = server(response);
        let response = fetch(&url).unwrap();
        assert!(response.truncated);
        assert_eq!(response.bytes, LIMIT as usize);
        handle.join().unwrap();
        assert!(validate("file:///secret").is_err());
        assert_eq!(safe_text("\u{1b}test"), "\\u{1b}test");
    }
}
