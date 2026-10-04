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
            match rx.try_recv() {
                Ok((source, summary)) => {
                    self.pending = None;
                    if worker == Some(source.as_str()) {
                        status.summary = summary;
                        changed = true;
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    status.summary = "Deployment check failed".into();
                    changed = true;
                }
                Err(mpsc::TryRecvError::Empty) => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnected_check_clears_pending_and_can_be_retried() {
        let mut monitor = Monitor::new(None);
        let (tx, rx) = mpsc::channel();
        monitor.pending = Some(rx);
        drop(tx);
        let mut status = Status {
            worker: Some("api".into()),
            summary: "Checking deployment…".into(),
        };

        assert!(monitor.update(Some("api"), &mut status, false));
        assert!(monitor.pending.is_none());
        assert_eq!(status.summary, "Deployment check failed");

        // Automatic polling resumes after its normal interval.
        monitor.last = Instant::now() - Duration::from_secs(61);
        monitor.update(Some("api"), &mut status, false);
        let (source, summary) = monitor
            .pending
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert_eq!(source, "api");
        assert!(summary.contains("DEMO"));
    }

    #[test]
    fn empty_check_stays_pending_and_stale_result_is_ignored() {
        let mut monitor = Monitor::new(None);
        let (tx, rx) = mpsc::channel();
        monitor.pending = Some(rx);
        let mut status = Status {
            worker: Some("current".into()),
            summary: "Current deployment".into(),
        };
        assert!(!monitor.update(Some("current"), &mut status, false));
        assert!(monitor.pending.is_some());

        tx.send(("previous".into(), "Stale deployment".into()))
            .unwrap();
        assert!(!monitor.update(Some("current"), &mut status, false));
        assert!(monitor.pending.is_none());
        assert_eq!(status.summary, "Current deployment");
    }
}
