use crate::config::Project;
use anyhow::{Context, Result};
use serde_json::Value;
use std::{
    cell::{Ref, RefCell},
    collections::VecDeque,
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    thread,
};

#[derive(Clone)]
pub struct Entry {
    pub summary: String,
    pub detail: String,
    pub error: bool,
    pub invocation: bool,
    search: String,
}
const BUFFER_BYTES: usize = 16 * 1024 * 1024;
const ENTRY_LIMIT: usize = 500;
impl Entry {
    fn new(summary: String, detail: String, error: bool, invocation: bool) -> Self {
        fn bounded(mut text: String, limit: usize) -> String {
            if text.len() > limit {
                let mut end = limit.saturating_sub(32);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                text.truncate(end);
                text.push_str("\n[truncated by cftui]");
            }
            text.shrink_to_fit();
            text
        }
        let summary = bounded(summary, 2048);
        let detail = bounded(detail, 32 * 1024);
        let mut search = detail.to_lowercase();
        search.shrink_to_fit();
        Self {
            summary,
            detail,
            search,
            error,
            invocation,
        }
    }
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + 2 * std::mem::size_of::<usize>()
            + self.summary.capacity()
            + self.detail.capacity()
            + self.search.capacity()
    }
}
#[derive(Default)]
struct RowCache {
    key: Option<(String, bool)>,
    rows: Vec<Arc<Entry>>,
}
pub struct Logs {
    child: Option<Child>,
    input: Option<Receiver<Entry>>,
    dropped: Arc<AtomicUsize>,
    entries: VecDeque<Arc<Entry>>,
    frozen: Option<Vec<Arc<Entry>>>,
    bytes: usize,
    last_dropped: usize,
    cache: RefCell<RowCache>,
    pub filter: String,
    pub errors_only: bool,
    pub selected: usize,
    pub expanded: bool,
    pub follow: bool,
    pub target: Option<String>,
    pub status: String,
    configuration: Option<std::path::PathBuf>,
}
impl Default for Logs {
    fn default() -> Self {
        Self {
            child: None,
            input: None,
            dropped: Arc::new(AtomicUsize::new(0)),
            entries: VecDeque::new(),
            frozen: None,
            bytes: 0,
            last_dropped: 0,
            cache: RefCell::default(),
            filter: String::new(),
            errors_only: false,
            selected: 0,
            expanded: false,
            follow: true,
            target: None,
            status: "Select a Worker and press L".into(),
            configuration: None,
        }
    }
}
impl Logs {
    pub fn start(
        &mut self,
        shell: &Path,
        project: &Project,
        account: &str,
        worker: &str,
        demo: bool,
    ) -> Result<()> {
        self.stop();
        self.entries.clear();
        self.bytes = 0;
        self.invalidate();
        self.frozen = None;
        self.selected = 0;
        self.expanded = false;
        self.follow = true;
        self.target = Some(worker.into());
        self.dropped.store(0, Ordering::Relaxed);
        self.last_dropped = 0;
        if demo {
            self.status = "DEMO sample events · no remote tail started".into();
            for text in [
                r#"{"outcome":"ok","eventTimestamp":1720000000000,"event":{"request":{"method":"GET","url":"https://example.com/health"}},"logs":[{"level":"log","message":["health check passed"]}],"exceptions":[]}"#,
                r#"{"outcome":"exception","eventTimestamp":1720000001000,"event":{"request":{"method":"POST","url":"https://example.com/api"}},"logs":[],"exceptions":[{"name":"Error","message":"Demo: database request failed"}]}"#,
            ] {
                self.push(parse_event(&serde_json::from_str(text)?));
            }
            return Ok(());
        }
        // Analytics supplies the deployed name, already including any environment suffix.
        // A project's account_id overrides the environment in Wrangler.
        // This credential-free config makes the dashboard account authoritative.
        static NEXT_CONFIG: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "cftui-tail-{}-{}.toml",
            std::process::id(),
            NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(
            &path,
            toml::to_string(&std::collections::BTreeMap::from([("account_id", account)]))?,
        )?;
        self.configuration = Some(path.clone());
        let script = tail_script(worker, &path);
        let mut command = Command::new(shell);
        command
            .args(["--login", "-c", &script])
            .current_dir(&project.path)
            .env_remove("CFTUI_API_TOKEN")
            .env("CLOUDFLARE_ACCOUNT_ID", account)
            .env("WRANGLER_SEND_METRICS", "false")
            .env("NO_COLOR", "1")
            .env("CI", "true")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .context("Could not launch Wrangler log stream")?;
        let (tx, rx) = mpsc::sync_channel(8);
        reader(
            child.stdout.take().unwrap(),
            tx.clone(),
            self.dropped.clone(),
        );
        reader(child.stderr.take().unwrap(), tx, self.dropped.clone());
        self.input = Some(rx);
        self.child = Some(child);
        self.status = "Connecting via Wrangler · read-only live tail".into();
        Ok(())
    }
    pub fn drain(&mut self) -> bool {
        let old_status = self.status.clone();
        let mut changed = false;
        for _ in 0..8 {
            let Some(input) = &self.input else { break };
            let Ok(entry) = input.try_recv() else { break };
            if entry.invocation {
                self.status = "Live · receiving invocations via Wrangler".into();
            }
            self.push(entry);
            changed = true;
        }
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(code)) => {
                    self.status=format!("Tail ended ({code}) · L restarts · inspect messages for authentication errors");
                    self.child = None;
                }
                Err(e) => self.status = format!("Tail status unavailable: {e}"),
                _ => {}
            }
        }
        let dropped = self.dropped();
        changed |= dropped != self.last_dropped;
        self.last_dropped = dropped;
        changed || old_status != self.status
    }
    fn invalidate(&mut self) {
        *self.cache.get_mut() = RowCache::default();
    }
    fn push(&mut self, entry: Entry) {
        self.invalidate();
        self.bytes += entry.bytes();
        self.entries.push_back(Arc::new(entry));
        while self.entries.len() > ENTRY_LIMIT || self.bytes > BUFFER_BYTES {
            if let Some(old) = self.entries.pop_front() {
                self.bytes -= old.bytes();
                if self.frozen.is_none() {
                    self.selected = self.selected.saturating_sub(1);
                }
            }
        }
    }
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let _ = Command::new("taskkill")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .creation_flags(0x08000000)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = child.kill();
            let _ = child.wait();
        }
        self.input = None;
        if let Some(path) = self.configuration.take() {
            let _ = std::fs::remove_file(path);
        }
        self.status = "Tail stopped · L restarts".into();
    }
    pub fn pause(&mut self) {
        self.invalidate();
        self.frozen = if self.frozen.is_some() {
            None
        } else {
            Some(self.entries.iter().cloned().collect())
        };
        let last = self.rows().len().saturating_sub(1);
        self.selected = self.selected.min(last);
    }
    pub fn paused(&self) -> bool {
        self.frozen.is_some()
    }
    pub fn dropped(&self) -> usize {
        self.dropped.load(Ordering::Relaxed)
    }
    pub fn rows(&self) -> Ref<'_, Vec<Arc<Entry>>> {
        let mut cache = self.cache.borrow_mut();
        if !cache
            .key
            .as_ref()
            .is_some_and(|(filter, errors)| filter == &self.filter && *errors == self.errors_only)
        {
            let filter = self.filter.to_lowercase();
            cache.rows = self.frozen.as_ref().map_or_else(
                || {
                    self.entries
                        .iter()
                        .filter(|e| (!self.errors_only || e.error) && e.search.contains(&filter))
                        .cloned()
                        .collect()
                },
                |entries| {
                    entries
                        .iter()
                        .filter(|e| (!self.errors_only || e.error) && e.search.contains(&filter))
                        .cloned()
                        .collect()
                },
            );
            cache.key = Some((self.filter.clone(), self.errors_only));
        }
        drop(cache);
        Ref::map(self.cache.borrow(), |cache| &cache.rows)
    }
}
impl Drop for Logs {
    fn drop(&mut self) {
        self.stop();
    }
}
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\"'\"'"))
}
fn tail_script(worker: &str, configuration: &Path) -> String {
    let configuration = quote(&configuration.to_string_lossy());
    format!("if command -v wrangler >/dev/null 2>&1; then exec wrangler tail --config {} --format json -- {}; else exec npx --no-install wrangler tail --config {} --format json -- {}; fi",configuration,quote(worker),configuration,quote(worker))
}
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}
fn parse_event(value: &Value) -> Entry {
    let error = value.get("error").is_some()
        || value["outcome"].as_str().is_some_and(|s| s != "ok")
        || value["exceptions"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        || value["logs"]
            .as_array()
            .is_some_and(|a| a.iter().any(|l| l["level"] == "error"));
    let outcome = value["outcome"].as_str().unwrap_or("event");
    let timestamp = value["eventTimestamp"]
        .as_i64()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map_or(String::new(), |t| t.format("%H:%M:%S UTC").to_string());
    let request = &value["event"]["request"];
    let message = value["exceptions"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|e| e["message"].as_str())
        .map(str::to_owned)
        .or_else(|| {
            value["logs"]
                .as_array()
                .and_then(|a| a.first())
                .map(|l| l["message"].to_string())
        })
        .or_else(|| value["error"].as_str().map(str::to_owned))
        .unwrap_or_default();
    Entry::new(
        clean(&format!(
            "{timestamp} {outcome} {} {} {message}",
            request["method"].as_str().unwrap_or(""),
            request["url"].as_str().unwrap_or("")
        ))
        .replace('\n', " "),
        clean(&bounded_json(value)),
        error,
        value["outcome"].is_string(),
    )
}
fn bounded_json(value: &Value) -> String {
    struct Output(Vec<u8>);
    impl std::io::Write for Output {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let available = (32 * 1024usize).saturating_sub(self.0.len());
            if available == 0 {
                return Err(std::io::Error::other("log detail limit"));
            }
            let n = bytes.len().min(available);
            self.0.extend_from_slice(&bytes[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Output(Vec::new());
    let truncated = serde_json::to_writer_pretty(&mut output, value).is_err();
    let mut detail = String::from_utf8_lossy(&output.0).into_owned();
    if truncated {
        detail.push_str("\n[truncated by cftui]");
    }
    detail
}
fn reader(
    input: impl Read + Send + 'static,
    tx: mpsc::SyncSender<Entry>,
    dropped: Arc<AtomicUsize>,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(input);
        let mut line = Vec::new();
        let mut json = String::new();
        loop {
            line.clear();
            // Cap each physical line and event so a stream cannot grow memory without bound.
            let result = reader.by_ref().take(65_537).read_until(b'\n', &mut line);
            let Ok(n) = result else { break };
            if n == 0 {
                break;
            }
            if n > 65_536 {
                json.clear();
                dropped.fetch_add(1, Ordering::Relaxed);
                while line.last() != Some(&b'\n') {
                    line.clear();
                    if reader
                        .by_ref()
                        .take(65_537)
                        .read_until(b'\n', &mut line)
                        .unwrap_or(0)
                        == 0
                    {
                        break;
                    }
                }
                continue;
            }
            let text = String::from_utf8_lossy(&line);
            if json.is_empty() && !text.trim_start().starts_with('{') {
                if text.trim().is_empty() {
                    continue;
                }
                let text = clean(text.trim());
                let entry = Entry::new(
                    text.clone(),
                    text.clone(),
                    text.to_lowercase().contains("error") || text.to_lowercase().contains("failed"),
                    false,
                );
                if tx.try_send(entry).is_err() {
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
                crate::wake();
                continue;
            }
            json.push_str(&text);
            if json.len() > 1_048_576 {
                json.clear();
                dropped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            match serde_json::from_str::<Value>(&json) {
                Ok(value) => {
                    if tx.try_send(parse_event(&value)).is_err() {
                        dropped.fetch_add(1, Ordering::Relaxed);
                    }
                    json.clear();
                    crate::wake();
                }
                Err(e) if e.is_eof() => {}
                Err(_) => {
                    json.clear();
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        crate::wake();
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_bytes_are_bounded_and_pause_shares_strings() {
        let mut logs = Logs::default();
        for _ in 0..600 {
            logs.push(Entry::new(
                "large".into(),
                "x".repeat(32 * 1024),
                false,
                true,
            ));
        }
        assert!(logs.bytes <= BUFFER_BYTES);
        assert!(logs.entries.len() < ENTRY_LIMIT);
        logs.pause();
        let frozen = logs.frozen.as_ref().unwrap();
        assert!(Arc::ptr_eq(&frozen[0], logs.entries.front().unwrap()));
        let first = frozen[0].clone();
        for _ in 0..600 {
            logs.push(Entry::new(
                "next".into(),
                "y".repeat(32 * 1024),
                false,
                true,
            ));
        }
        let frozen_bytes: usize = logs
            .frozen
            .as_ref()
            .unwrap()
            .iter()
            .map(|e| e.bytes())
            .sum();
        assert!(logs.bytes + frozen_bytes <= 2 * BUFFER_BYTES);
        assert!(Arc::ptr_eq(&first, &logs.rows()[0]));
        logs.pause();
        assert_eq!(logs.rows()[0].summary, "next");
    }
    #[test]
    fn detail_truncation_and_cached_filters() {
        let entry = parse_event(&serde_json::json!({"logs":[{"message":"é".repeat(100_000)}]}));
        assert!(entry.detail.len() <= 32 * 1024);
        assert!(entry.detail.contains("[truncated by cftui]"));
        let mut logs = Logs::default();
        logs.push(Entry::new("event".into(), "Mixed CASE".into(), false, true));
        let pointer = logs.rows().as_ptr();
        assert_eq!(logs.rows().as_ptr(), pointer);
        logs.filter = "CASE".into();
        assert_eq!(logs.rows().len(), 1);
        logs.errors_only = true;
        assert!(logs.rows().is_empty());
    }
    #[test]
    fn parses_pretty_events_and_filters_frozen_buffer() {
        let mut logs = Logs::default();
        logs.start(
            Path::new("unused"),
            &Project {
                name: "Demo".into(),
                path: ".".into(),
                environment: String::new(),
            },
            "",
            "demo",
            true,
        )
        .unwrap();
        assert_eq!(logs.rows().len(), 2);
        logs.errors_only = true;
        assert_eq!(logs.rows().len(), 1);
        logs.filter = "database".into();
        assert_eq!(logs.rows().len(), 1);
        logs.pause();
        logs.entries.clear();
        assert_eq!(logs.rows().len(), 1);
        logs.pause();
        assert!(logs.rows().is_empty());
        let (tx, rx) = mpsc::sync_channel(4);
        let dropped = Arc::new(AtomicUsize::new(0));
        reader(
            std::io::Cursor::new(b"Connecting...\n{\n  \"outcome\": \"ok\",\n  \"logs\": []\n}\n"),
            tx,
            dropped.clone(),
        );
        let events = rx.iter().collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        assert!(!events[1].error);
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn shell_arguments_are_literal_and_control_text_is_removed() {
        assert_eq!(quote("a'b $(pwd)"), "'a'\"'\"'b $(pwd)'");
        assert_eq!(clean("hello\u{1b}\u{7}"), "hello");
        let command = tail_script("api-production", Path::new("config.toml"));
        assert!(command.contains("-- 'api-production'"));
        assert!(!command.contains("--env"));
        assert!(command.contains("--config 'config.toml'"));
        let error = parse_event(&serde_json::json!({"error":"Authentication failed"}));
        assert!(error.error);
        assert!(error.summary.contains("Authentication failed"));
    }
}
