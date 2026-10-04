mod app;
mod cloudflare;
mod config;
mod context;
mod deployment;
mod help;
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
use app::App;
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
use shell::Shell;
use std::{
    io,
    sync::{mpsc, OnceLock},
    thread,
    time::{Duration, Instant},
};

enum Update {
    Wake,
    Input(Event),
}
static UPDATES: OnceLock<mpsc::SyncSender<Update>> = OnceLock::new();
pub(crate) fn wake() {
    if let Some(tx) = UPDATES.get() {
        let _ = tx.try_send(Update::Wake);
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
    let (updates_tx, updates_rx) = mpsc::sync_channel(512);
    let _ = UPDATES.set(updates_tx.clone());
    let mut sessions = sessions::Sessions::new(config.projects.clone(), project, &config.shell)?;
    let mut deployments = deployment::Monitor::new(client.clone());
    // One worker owns analytics requests. No overlapping refreshes.
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
            wake();
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
    let mut context = context::Monitor::new(config.shell.clone());
    context.refresh(project, &app.projects[project]);
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        execute!(io::stdout(), EnableBracketedPaste, EnableMouseCapture)?;
        thread::spawn(move || {
            while let Ok(event) = event::read() {
                if updates_tx.send(Update::Input(event)).is_err() {
                    break;
                }
            }
        });
        let mut last_refresh = Instant::now();
        let mut last_context = Instant::now();
        let mut dirty = true;
        loop {
            dirty |= sessions.drain()?;
            dirty |= app.logs.drain();
            dirty |= app.request.drain();
            if context.drain(app.project, &mut app.context) {
                dirty = true;
            }
            if last_context.elapsed() >= Duration::from_secs(5) {
                context.refresh(app.project, &app.projects[app.project]);
                last_context = Instant::now();
            }
            if dirty && app.logs.follow && !app.logs.paused() && !app.logs.expanded {
                let last = app.logs.rows().len().saturating_sub(1);
                app.logs.selected = last;
            }
            if let Ok(snapshot) = result_rx.try_recv() {
                app.refresh_completed(snapshot);
                last_refresh = Instant::now();
                dirty = true;
            }
            if !app.refreshing
                && last_refresh.elapsed() >= Duration::from_secs(config.refresh_seconds)
            {
                request_tx.try_send(())?;
                app.refreshing = true;
                dirty = true;
            }
            let selected_worker = app.browser.worker.as_deref().or({
                if app.tab == 0 {
                    app.browser.selected.as_deref()
                } else {
                    None
                }
            });
            dirty |= deployments.update(selected_worker, &mut app.deployment, false);
            if dirty {
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
                terminal
                    .draw(|frame| ui::draw(frame, &mut app, shell.parser.screen(), shell.ended))?;
                dirty = false;
            }
            let timeout = Duration::from_secs(5)
                .saturating_sub(last_context.elapsed())
                .min(if app.refreshing {
                    Duration::from_secs(5)
                } else {
                    Duration::from_secs(config.refresh_seconds)
                        .saturating_sub(last_refresh.elapsed())
                });
            let event = match updates_rx.recv_timeout(timeout) {
                Ok(Update::Input(event)) => event,
                Ok(Update::Wake) | Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            dirty = true;
            match event {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    match input::key(&mut app, key) {
                        input::Action::Quit => break,
                        input::Action::None => {}
                        input::Action::SendRequest => app.request.send(),
                        input::Action::Refresh if !app.refreshing => {
                            request_tx.try_send(())?;
                            app.refreshing = true;
                            let worker = app.deployment.worker.clone();
                            deployments.update(worker.as_deref(), &mut app.deployment, true);
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
                            if app.project_switch_completed(
                                index,
                                sessions.switch(index, &config.shell),
                            ) {
                                context.refresh(index, &app.projects[index]);
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
                                Ok(()) => app.logs_started(worker),
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
                Event::Mouse(mouse)
                    if matches!(
                        mouse.kind,
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    ) && app.help.is_some() =>
                {
                    if let Some(guide) = &mut app.help {
                        guide.scroll_by(if mouse.kind == MouseEventKind::ScrollUp {
                            -3
                        } else {
                            3
                        });
                    }
                }
                Event::Resize(_, _) => {}
                Event::Paste(text)
                    if app.request.editing
                        && app.project_picker.is_none()
                        && app.help.is_none() =>
                {
                    app.request.url.extend(
                        text.chars()
                            .filter(|c| !c.is_control())
                            .take(4096usize.saturating_sub(app.request.url.chars().count())),
                    );
                }
                Event::Paste(text)
                    if app.browser.editing
                        && app.project_picker.is_none()
                        && app.help.is_none() =>
                {
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
                    if app.panels.shell_input()
                        && app.project_picker.is_none()
                        && app.help.is_none() =>
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
