mod cloudflare;
mod config;
mod input;
mod logs;
mod navigation;
mod panels;
mod preview;
mod request;
mod sessions;
mod shell;
mod ui;

use anyhow::{bail, Result};
use clap::Parser;
use cloudflare::{Cloudflare, Snapshot};
use config::{Args, Config};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
    },
    execute,
};
use panels::{Panel, Panels};
use shell::Shell;
use std::{
    io,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub struct App {
    pub snapshot: Option<Snapshot>,
    pub demo: bool,
    pub panels: Panels,
    pub tab: usize,
    pub offset: usize,
    pub refreshing: bool,
    pub account: String,
    pub browser: navigation::Browser,
    pub logs: logs::Logs,
    pub projects: Vec<config::Project>,
    pub project: usize,
    pub project_picker: Option<usize>,
    pub message: String,
    pub help: bool,
    pub request: request::Inspector,
}
impl App {
    pub fn new(demo: bool, account: String, projects: Vec<config::Project>) -> Self {
        Self {
            snapshot: None,
            demo,
            panels: Panels::default(),
            tab: 0,
            offset: 0,
            refreshing: true,
            account,
            browser: navigation::Browser::default(),
            logs: logs::Logs::default(),
            projects,
            project: 0,
            project_picker: None,
            message: String::new(),
            help: false,
            request: request::Inspector::default(),
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::load(&args)?;
    if let Some(path) = &args.preview {
        return preview::write(path);
    }
    if args.check_shell {
        return check_shell(&config);
    }
    let client = if args.demo {
        None
    } else {
        Some(Cloudflare::new(
            config.account_id.clone(),
            config.credentials()?,
        )?)
    };
    if args.check {
        let snapshot = client
            .as_ref()
            .map_or_else(Snapshot::demo, Cloudflare::fetch);
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
        if !snapshot.warnings.is_empty() {
            bail!("One or more datasets are unavailable");
        }
        return Ok(());
    }
    let state_path = args.config.with_extension("state.toml");
    let (saved, state_warning) = match config::SavedState::load(&state_path) {
        Ok(saved) => (saved, String::new()),
        Err(e) => (
            config::SavedState::default(),
            format!("Saved layout ignored: {e}"),
        ),
    };
    let project = config
        .projects
        .iter()
        .position(|p| p.name == saved.project)
        .unwrap_or(0);
    let mut sessions = sessions::Sessions::new(config.projects.clone(), project, &config.shell)?;
    // One worker owns all HTTP requests. No overlapping refreshes.
    let (request_tx, request_rx) = mpsc::sync_channel::<()>(1);
    let (result_tx, result_rx) = mpsc::channel();
    thread::spawn(move || {
        while request_rx.recv().is_ok() {
            let snapshot = client
                .as_ref()
                .map_or_else(Snapshot::demo, Cloudflare::fetch);
            if result_tx.send(snapshot).is_err() {
                break;
            }
        }
    });
    request_tx.send(())?;
    let mut app = App::new(
        args.demo,
        config.account_id.clone(),
        config.projects.clone(),
    );
    app.panels = saved.panels;
    app.project = project;
    app.message = state_warning;
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        execute!(io::stdout(), EnableBracketedPaste, EnableMouseCapture)?;
        let mut last_refresh = Instant::now();
        loop {
            sessions.drain()?;
            app.logs.drain();
            app.request.drain();
            if app.logs.follow && !app.logs.paused() && !app.logs.expanded {
                app.logs.selected = app.logs.rows().len().saturating_sub(1);
            }
            if let Ok(snapshot) = result_rx.try_recv() {
                app.snapshot = Some(snapshot);
                if let Some(snapshot) = &app.snapshot {
                    app.browser.reconcile(snapshot, app.tab);
                }
                app.refreshing = false;
                last_refresh = Instant::now();
            }
            if !app.refreshing
                && last_refresh.elapsed() >= Duration::from_secs(config.refresh_seconds)
            {
                request_tx.try_send(())?;
                app.refreshing = true;
            }
            let area = terminal.size()?;
            let shell_area = ui::shell_area(
                ratatui::layout::Rect::new(0, 0, area.width, area.height),
                &app.panels,
            );
            if !shell_area.is_empty() {
                sessions.shell().resize(
                    shell_area.height.saturating_sub(2),
                    shell_area.width.saturating_sub(2),
                )?;
            }
            let shell = sessions.shell();
            terminal.draw(|frame| ui::draw(frame, &app, shell.parser.screen(), shell.ended))?;
            if !event::poll(Duration::from_millis(33))? {
                continue;
            }
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    match input::key(&mut app, key) {
                        input::Action::Quit => break,
                        input::Action::None => {}
                        input::Action::SendRequest => app.request.send(),
                        input::Action::Refresh if !app.refreshing => {
                            request_tx.try_send(())?;
                            app.refreshing = true;
                        }
                        input::Action::Refresh => {}
                        input::Action::Save => {
                            app.message = match (config::SavedState {
                                panels: app.panels.clone(),
                                project: app.projects[app.project].name.clone(),
                            })
                            .save(&state_path)
                            {
                                Ok(()) => "Layout and project saved".into(),
                                Err(e) => format!("Could not save layout: {e}"),
                            };
                        }
                        input::Action::Switch(index) => {
                            match sessions.switch(index, &config.shell) {
                                Ok(()) => {
                                    if index != app.project {
                                        app.logs.stop();
                                    }
                                    app.project = index;
                                    app.message = format!(
                                        "Project: {} · separate Bash session",
                                        app.projects[index].name
                                    );
                                    app.panels.visible[3] = true;
                                    app.panels.focus(Panel::Bash);
                                }
                                Err(e) => app.message = format!("Could not switch project: {e}"),
                            }
                        }
                        input::Action::Tail(worker) => {
                            match app.logs.start(
                                &config.shell,
                                &app.projects[app.project],
                                &app.account,
                                &worker,
                                app.demo,
                            ) {
                                Ok(()) => {
                                    app.browser.worker = Some(worker);
                                    app.browser.view = navigation::View::Logs;
                                    app.panels.visible[2] = true;
                                    app.panels.focus(Panel::Resources);
                                    app.offset = 0;
                                }
                                Err(e) => app.message = format!("Could not start logs: {e}"),
                            }
                        }
                        input::Action::Shell(key) => {
                            let shell = sessions.shell();
                            if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
                                && key.modifiers.contains(KeyModifiers::SHIFT)
                            {
                                shell.scroll(key.code == KeyCode::PageUp);
                            } else {
                                shell.key(key)?;
                            }
                        }
                    }
                }
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    let size = terminal.size()?;
                    ui::mouse(
                        &mut app,
                        ratatui::layout::Rect::new(0, 0, size.width, size.height),
                        mouse.column,
                        mouse.row,
                    );
                }
                Event::Paste(text)
                    if app.request.editing && app.project_picker.is_none() && !app.help =>
                {
                    app.request.url.extend(
                        text.chars()
                            .filter(|c| !c.is_control())
                            .take(4096usize.saturating_sub(app.request.url.chars().count())),
                    );
                }
                Event::Paste(text) if app.browser.editing => {
                    let filter = if app.browser.view == navigation::View::Logs {
                        &mut app.logs.filter
                    } else {
                        &mut app.browser.filter
                    };
                    filter.extend(
                        text.chars()
                            .filter(|c| !c.is_control())
                            .take(256usize.saturating_sub(filter.chars().count())),
                    );
                    if let Some(snapshot) = &app.snapshot {
                        app.browser.reconcile(snapshot, app.tab);
                    }
                }
                Event::Paste(text)
                    if app.panels.shell_input() && app.project_picker.is_none() && !app.help =>
                {
                    let shell = sessions.shell();
                    if shell.parser.screen().bracketed_paste() {
                        shell.send(b"\x1b[200~")?;
                        shell.send(text.as_bytes())?;
                        shell.send(b"\x1b[201~")?;
                    } else {
                        shell.send(text.as_bytes())?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    })();
    let _ = execute!(io::stdout(), DisableBracketedPaste, DisableMouseCapture);
    ratatui::restore();
    if let Err(e) = (config::SavedState {
        panels: app.panels.clone(),
        project: app.projects[app.project].name.clone(),
    })
    .save(&state_path)
    {
        eprintln!("Could not save layout: {e}");
    }
    result
}

fn check_shell(config: &Config) -> Result<()> {
    let mut shell = Shell::start(&config.shell, &config.project_dir)?;
    shell.resize(30, 100)?;
    shell.send(b"pwd; node --version; WRANGLER_SEND_METRICS=false wrangler --version; printf 'CFTUI_PTY_%s\\n' OK\r")?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(45) {
        shell.drain()?;
        if shell.parser.screen().contents().contains("CFTUI_PTY_OK") {
            println!(
                "Git Bash PTY smoke check passed: command input, parsed output, and resize.\n{}",
                shell.parser.screen().contents()
            );
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    bail!("Git Bash did not return the smoke-check marker within 45 seconds (exited: {}). Captured output:\n{}", shell.ended, shell.parser.screen().contents())
}
