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
    git: Option<PathBuf>,
    pending: Option<Receiver<(usize, Status)>>,
    versions: Option<(usize, String, String)>,
}
impl Monitor {
    pub fn new(shell: PathBuf) -> Self {
        Self {
            shell,
            git: find_git(),
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
        let git = self.git.clone();
        let project = project.clone();
        let versions = self.versions.clone().filter(|v| v.0 == index);
        thread::spawn(move || {
            let git = git_output(git.as_deref(), &shell, &project.path)
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
fn find_git() -> Option<PathBuf> {
    let executable = if cfg!(windows) { "git.exe" } else { "git" };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(executable))
        .find(|path| path.is_file())
        // Resolve relative PATH entries before the command changes working directory.
        .and_then(|path| path.canonicalize().ok())
}

fn git_output(git: Option<&Path>, shell: &Path, cwd: &Path) -> Option<String> {
    if let Some(git) = git {
        let mut command = Command::new(git);
        command.args([
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=normal",
        ]);
        run_command(command, cwd)
    } else {
        run(
            shell,
            cwd,
            "git status --porcelain=v2 --branch --untracked-files=normal",
        )
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
    command.args(["--login", "-c", script]);
    run_command(command, cwd)
}

fn run_command(mut command: Command, cwd: &Path) -> Option<String> {
    command
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
    fn direct_git_works_without_bash_and_matches_the_shell_fallback() {
        let git = find_git().expect("Git must be on PATH for this test");
        let cwd = Path::new(env!("CARGO_MANIFEST_DIR"));
        let output = git_output(Some(&git), Path::new("missing-bash"), cwd)
            .expect("direct Git status should succeed without Bash");
        assert!(output.contains("# branch.head "));
        let shell = crate::config::Config::default().shell;
        let fallback = git_output(None, &shell, cwd).expect("Bash fallback should find Git");
        assert_eq!(output, fallback);
    }

    #[test]
    fn direct_git_failure_does_not_turn_into_a_shell_command() {
        let git = find_git().expect("Git must be on PATH for this test");
        let cwd = std::env::temp_dir().join(format!("cftui-git-check-{}", std::process::id()));
        std::fs::create_dir(&cwd).unwrap();
        let result = git_output(Some(&git), Path::new("missing-bash"), &cwd);
        std::fs::remove_dir(&cwd).unwrap();
        assert!(
            result.is_none(),
            "a non-repository should remain unavailable"
        );
    }

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
