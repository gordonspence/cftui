use crate::config::Project;
use anyhow::{Context, Result};
use serde_json::Value;
use std::{
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
}
pub struct Logs {
    child: Option<Child>,
    input: Option<Receiver<Entry>>,
    dropped: Arc<AtomicUsize>,
    pub entries: VecDeque<Entry>,
    frozen: Option<Vec<Entry>>,
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
        self.frozen = None;
        self.selected = 0;
        self.expanded = false;
        self.follow = true;
        self.target = Some(worker.into());
        self.dropped.store(0, Ordering::Relaxed);
        if demo {
            self.status = "DEMO sample events · no remote tail started".into();
            for text in [
                r#"{"outcome":"ok","eventTimestamp":1720000000000,"event":{"request":{"method":"GET","url":"https://example.com/health"}},"logs":[{"level":"log","message":["health check passed"]}],"exceptions":[]}"#,
                r#"{"outcome":"exception","eventTimestamp":1720000001000,"event":{"request":{"method":"POST","url":"https://example.com/api"}},"logs":[],"exceptions":[{"name":"Error","message":"Demo: database request failed"}]}"#,
            ] {
                self.entries
                    .push_back(parse_event(&serde_json::from_str(text)?));
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
        let (tx, rx) = mpsc::sync_channel(256);
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
    pub fn drain(&mut self) {
        if let Some(input) = &self.input {
            for _ in 0..256 {
                let Ok(entry) = input.try_recv() else { break };
                if entry.invocation {
                    self.status = "Live · receiving invocations via Wrangler".into();
                }
                self.entries.push_back(entry);
                if self.entries.len() > 500 {
                    self.entries.pop_front();
                    if self.frozen.is_none() {
                        self.selected = self.selected.saturating_sub(1);
                    }
                }
            }
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
        self.frozen = if self.frozen.is_some() {
            None
        } else {
            Some(self.entries.iter().cloned().collect())
        };
        self.selected = self.selected.min(self.rows().len().saturating_sub(1));
    }
    pub fn paused(&self) -> bool {
        self.frozen.is_some()
    }
    pub fn dropped(&self) -> usize {
        self.dropped.load(Ordering::Relaxed)
    }
    pub fn rows(&self) -> Vec<&Entry> {
        let source: Vec<_> = self.frozen.as_ref().map_or_else(
            || self.entries.iter().collect(),
            |entries| entries.iter().collect(),
        );
        let filter = self.filter.to_lowercase();
        source
            .into_iter()
            .filter(|e| (!self.errors_only || e.error) && e.detail.to_lowercase().contains(&filter))
            .collect()
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
    Entry {
        summary: clean(&format!(
            "{timestamp} {outcome} {} {} {message}",
            request["method"].as_str().unwrap_or(""),
            request["url"].as_str().unwrap_or("")
        ))
        .replace('\n', " "),
        detail: clean(&serde_json::to_string_pretty(value).unwrap_or_default()),
        error,
        invocation: value["outcome"].is_string(),
    }
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
                let entry = Entry {
                    summary: text.clone(),
                    error: text.to_lowercase().contains("error")
                        || text.to_lowercase().contains("failed"),
                    detail: text,
                    invocation: false,
                };
                if tx.try_send(entry).is_err() {
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
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
                }
                Err(e) if e.is_eof() => {}
                Err(_) => {
                    json.clear();
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
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
