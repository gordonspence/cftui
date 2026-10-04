use anyhow::{bail, Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Cloudflare metrics and an embedded Git Bash terminal"
)]
pub struct Args {
    #[arg(long)]
    pub demo: bool,
    #[arg(long, default_value = "cftui.toml")]
    pub config: PathBuf,
    #[arg(long)]
    pub project: Option<PathBuf>,
    #[arg(long)]
    pub shell: Option<PathBuf>,
    /// Print metrics as JSON without opening the TUI.
    #[arg(long)]
    pub check: bool,
    /// Run a local PTY smoke check without opening the TUI.
    #[arg(long)]
    pub check_shell: bool,
    /// Export the actual demo layout as a standalone HTML preview; starts no shell.
    #[arg(long)]
    pub preview: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub account_id: String,
    pub shell: PathBuf,
    pub project_dir: PathBuf,
    pub refresh_seconds: u64,
    pub projects: Vec<Project>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            account_id: String::new(),
            shell: if cfg!(windows) {
                PathBuf::from(r"C:\Program Files\Git\bin\bash.exe")
            } else {
                PathBuf::from("/bin/bash")
            },
            project_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            refresh_seconds: 60,
            projects: vec![],
        }
    }
}

impl Config {
    pub fn load(args: &Args) -> Result<Self> {
        let mut cfg: Self = if args.config.exists() {
            toml::from_str(&std::fs::read_to_string(&args.config)?)
                .context("Invalid configuration")?
        } else {
            Self::default()
        };
        if let Ok(id) = std::env::var("CFTUI_ACCOUNT_ID") {
            cfg.account_id = id;
        }
        if let Some(path) = &args.project {
            cfg.project_dir = path.clone();
        }
        if let Some(path) = &args.shell {
            cfg.shell = path.clone();
        }
        cfg.project_dir = cfg
            .project_dir
            .canonicalize()
            .context("Project folder does not exist")?;
        if !cfg.project_dir.is_dir() {
            bail!("Project path must be a directory");
        }
        if !cfg.shell.is_file() {
            bail!(
                "Bash not found at {}. Use --shell PATH",
                cfg.shell.display()
            );
        }
        cfg.refresh_seconds = cfg.refresh_seconds.max(15);
        for project in &mut cfg.projects {
            project.path = project
                .path
                .canonicalize()
                .with_context(|| format!("Project {} folder does not exist", project.name))?;
            if !project.path.is_dir() {
                bail!("Project {} must be a directory", project.name);
            }
        }
        cfg.projects.insert(
            0,
            Project {
                name: "Default".into(),
                path: cfg.project_dir.clone(),
                environment: String::new(),
            },
        );
        Ok(cfg)
    }
    pub fn credentials(&self) -> Result<String> {
        if self.account_id.len() != 32 || !self.account_id.bytes().all(|c| c.is_ascii_hexdigit()) {
            bail!(
                "Set CFTUI_ACCOUNT_ID or account_id in cftui.toml to your 32-character account ID"
            );
        }
        let token = std::env::var("CFTUI_API_TOKEN").context(
            "Set CFTUI_API_TOKEN to a read-only Cloudflare analytics token, or use --demo",
        )?;
        if token.trim().is_empty() {
            bail!("CFTUI_API_TOKEN is empty");
        }
        Ok(token)
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub environment: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedState {
    pub panels: crate::panels::Panels,
    pub project: String,
}
impl SavedState {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut saved: Self = toml::from_str(&std::fs::read_to_string(path)?)?;
        saved.panels.shell_percent = saved.panels.shell_percent.clamp(15, 70);
        if !saved.panels.is_visible(saved.panels.focused) {
            saved.panels.next();
        }
        if saved
            .panels
            .expanded
            .is_some_and(|p| !saved.panels.is_visible(p))
        {
            saved.panels.expanded = None;
        }
        Ok(saved)
    }
    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        let temporary = path.with_extension("state.tmp");
        std::fs::write(&temporary, toml::to_string(self)?)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_layout_round_trips_and_replaces_existing_state() {
        let path =
            std::env::temp_dir().join(format!("cftui-state-test-{}.toml", std::process::id()));
        let mut saved = SavedState {
            project: "Work".into(),
            ..SavedState::default()
        };
        saved.panels.close(crate::panels::Panel::D1);
        saved.panels.shell_percent = 45;
        saved.save(&path).unwrap();
        let restored = SavedState::load(&path).unwrap();
        assert!(!restored.panels.visible[1]);
        assert_eq!(restored.panels.shell_percent, 45);
        assert_eq!(restored.project, "Work");
        saved.panels.zoom(crate::panels::Panel::Bash);
        saved.save(&path).unwrap();
        assert_eq!(
            SavedState::load(&path).unwrap().panels.expanded,
            Some(crate::panels::Panel::Bash)
        );
        std::fs::remove_file(path).unwrap();
    }
}
