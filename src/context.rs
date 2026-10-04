use crate::{config::Project, request::safe_text};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, PartialEq, Eq)]
pub struct Status {
    pub git: String,
    pub wrangler: String,
    pub cf: String,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            git: "Git checking…".into(),
            wrangler: "checking…".into(),
            cf: "—".into(),
        }
    }
}
pub struct Monitor {
    shell: PathBuf,
    pending: Option<Receiver<(usize, Status)>>,
    versions: Option<(usize, String, String)>,
}
impl Monitor {
    pub fn new(shell: PathBuf) -> Self {
        Self {
            shell,
            pending: None,
            versions: None,
        }
    }
    pub fn refresh(&mut self, index: usize, project: &Project) {
        if self.pending.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let shell = self.shell.clone();
        let project = project.clone();
        let versions = self.versions.clone().filter(|v| v.0 == index);
        thread::spawn(move || {
            let git = run(
                &shell,
                &project.path,
                "git status --porcelain=v2 --branch --untracked-files=normal",
            )
            .map_or_else(|| "Git unavailable".into(), |text| git_status(&text));
            let (wrangler, cf) = versions.map_or_else(|| {
                let wrangler = run(&shell, &project.path, "if command -v wrangler >/dev/null 2>&1; then wrangler --version; else npx --no-install wrangler --version; fi");
                let cf = run(&shell, &project.path, "if command -v cf >/dev/null 2>&1; then cf --version; fi");
                (version(wrangler), version(cf))
            }, |(_, wrangler, cf)| (wrangler, cf));
            let _ = tx.send((index, Status { git, wrangler, cf }));
            crate::wake();
        });
    }
    pub fn drain(&mut self, index: usize, status: &mut Status) -> bool {
        let Some(rx) = &self.pending else {
            return false;
        };
        match rx.try_recv() {
            Ok((source, result)) => {
                self.pending = None;
                self.versions = Some((source, result.wrangler.clone(), result.cf.clone()));
                if source == index && *status != result {
                    *status = result;
                    return true;
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
            Err(mpsc::TryRecvError::Empty) => {}
        }
        false
    }
}
fn version(text: Option<String>) -> String {
    text.and_then(|text| {
        text.split_whitespace()
            .map(|token| token.trim_start_matches('v'))
            .find(|token| token.starts_with(|c: char| c.is_ascii_digit()) && token.contains('.'))
            .map(|token| safe_text(token).chars().take(32).collect())
    })
    .unwrap_or_else(|| "unavailable".into())
}
fn run(shell: &Path, cwd: &Path, script: &str) -> Option<String> {
    let mut command = Command::new(shell);
    command
        .args(["--login", "-c", script])
        .current_dir(cwd)
        .env_remove("CFTUI_API_TOKEN")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("WRANGLER_SEND_METRICS", "false")
        .env("NO_COLOR", "1")
        .env("CI", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().ok()?;
    let output = child.stdout.take()?;
    let (output_tx, output_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = output.take(1024 * 1024).read_to_end(&mut bytes);
        let _ = output_tx.send(bytes);
    });
    let start = Instant::now();
    let success = loop {
        match child.try_wait() {
            Ok(Some(code)) => break code.success(),
            Err(_) => break false,
            _ => {}
        }
        if start.elapsed() >= Duration::from_secs(4) {
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
            break false;
        }
        thread::sleep(Duration::from_millis(20));
    };
    if !success {
        return None;
    }
    let bytes = output_rx
        .recv_timeout(Duration::from_secs(4).saturating_sub(start.elapsed()))
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}
fn git_status(text: &str) -> String {
    let branch = text
        .lines()
        .find_map(|line| line.strip_prefix("# branch.head "))
        .unwrap_or("unborn");
    let dirty = text
        .lines()
        .any(|line| !line.starts_with('#') && !line.is_empty());
    let tracking = text
        .lines()
        .find_map(|line| line.strip_prefix("# branch.ab "));
    let mut result = format!("{}{}", safe_text(branch), if dirty { "*" } else { "" });
    if let Some(tracking) = tracking {
        let mut parts = tracking.split_whitespace();
        let ahead = parts.next().unwrap_or("+0").trim_start_matches('+');
        let behind = parts.next().unwrap_or("-0").trim_start_matches('-');
        if ahead != "0" {
            result.push_str(&format!(" ↑{ahead}"));
        }
        if behind != "0" {
            result.push_str(&format!(" ↓{behind}"));
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn branch_dirty_and_tracking_states() {
        assert_eq!(
            git_status("# branch.head main\n# branch.ab +2 -3\n? new.txt\n"),
            "main* ↑2 ↓3"
        );
        assert_eq!(git_status("# branch.head (detached)\n"), "(detached)");
        assert_eq!(
            git_status("# branch.head main\n# branch.ab +0 -0\n"),
            "main"
        );
    }
    #[test]
    fn reads_version_numbers_with_cli_prefixes() {
        assert_eq!(version(Some("wrangler 4.12.0\n".into())), "4.12.0");
        assert_eq!(version(Some("cf v0.3.0-beta.1\n".into())), "0.3.0-beta.1");
        assert_eq!(version(None), "unavailable");
    }
}
