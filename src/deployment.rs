use crate::cloudflare::Cloudflare;
use std::{
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Status {
    pub worker: Option<String>,
    pub summary: String,
}
pub struct Monitor {
    client: Option<Cloudflare>,
    pending: Option<Receiver<(String, String)>>,
    last: Instant,
}
impl Monitor {
    pub fn new(client: Option<Cloudflare>) -> Self {
        Self {
            client,
            pending: None,
            last: Instant::now(),
        }
    }
    pub fn update(&mut self, worker: Option<&str>, status: &mut Status, force: bool) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.pending {
            if let Ok((source, summary)) = rx.try_recv() {
                self.pending = None;
                if worker == Some(source.as_str()) {
                    status.summary = summary;
                    changed = true;
                }
            }
        }
        let switched = status.worker.as_deref() != worker;
        if switched {
            status.worker = worker.map(str::to_owned);
            status.summary = if worker.is_some() {
                "Checking deployment…".into()
            } else {
                String::new()
            };
            changed = true;
        }
        let Some(worker) = worker else {
            return changed;
        };
        if self.pending.is_none()
            && (switched
                || status.summary == "Checking deployment…"
                || force
                || self.last.elapsed() >= Duration::from_secs(60))
        {
            self.last = Instant::now();
            let worker = worker.to_owned();
            let client = self.client.clone();
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            thread::spawn(move || {
                let summary = client.map_or_else(
                    || "DEMO · Serving · sample deployment · demo0001 100%".into(),
                    |client| {
                        client
                            .deployment(&worker)
                            .unwrap_or_else(|e| format!("Unavailable · {e}"))
                    },
                );
                let _ = tx.send((worker, summary));
                crate::wake();
            });
        }
        changed
    }
}
