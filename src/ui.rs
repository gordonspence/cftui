use crate::{
    cloudflare::{History, MetricRow},
    navigation::View,
    panels::{Panel, Panels},
    App,
};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Cell, Clear, Paragraph, Row, Table, Wrap},
    Frame,
};
use tui_term::{vt100, widget::PseudoTerminal};

const BG: Color = Color::Rgb(27, 30, 31);
const FG: Color = Color::Rgb(211, 217, 212);
const BORDER: Color = Color::Rgb(103, 114, 113);
const MUTED: Color = Color::Rgb(145, 157, 153);
const CYAN: Color = Color::Rgb(139, 201, 196);
const YELLOW: Color = Color::Rgb(225, 201, 89);
const PURPLE: Color = Color::Rgb(185, 147, 207);
const GREEN: Color = Color::Rgb(165, 189, 118);
const BAND: Color = Color::Rgb(47, 54, 54);

fn layout(area: Rect, panels: &Panels) -> [Rect; 5] {
    let content = Rect::new(
        area.x,
        area.y + 2,
        area.width,
        area.height.saturating_sub(3),
    );
    let mut result = [Rect::default(); 5];
    if let Some(panel) = panels.expanded {
        if panels.is_visible(panel) {
            result[panel.index()] = content;
            return result;
        }
    }
    let shell_height = if panels.visible[3] {
        (area.height.saturating_mul(panels.shell_percent) / 100)
            .max(5)
            .min(content.height)
    } else {
        0
    };
    let dashboard_height = content.height.saturating_sub(shell_height);
    let graphs = u16::from(panels.visible[0]) + u16::from(panels.visible[1]);
    let preferred = if dashboard_height >= 20 {
        8
    } else if dashboard_height >= 17 {
        6
    } else {
        4
    };
    let graph_height = dashboard_height
        .saturating_sub(u16::from(panels.visible[2]) * 3 + u16::from(panels.request_visible) * 5)
        .checked_div(graphs)
        .map_or(0, |height| preferred.min(height));
    let ids = Panel::ALL
        .into_iter()
        .filter(|p| panels.is_visible(*p))
        .collect::<Vec<_>>();
    let constraints = ids
        .iter()
        .map(|p| match p {
            Panel::Bash if ids.len() > 1 => Constraint::Length(shell_height),
            Panel::Workers | Panel::D1 if panels.visible[2] || panels.request_visible => {
                Constraint::Length(graph_height)
            }
            _ => Constraint::Fill(1),
        })
        .collect::<Vec<_>>();
    let regions = Layout::vertical(constraints).split(content);
    for (panel, region) in ids.iter().zip(regions.iter()) {
        result[panel.index()] = *region;
    }
    result
}
pub fn shell_area(area: Rect, panels: &Panels) -> Rect {
    layout(area, panels)[Panel::Bash.index()]
}
pub fn click(panels: &mut Panels, area: Rect, x: u16, y: u16) {
    for panel in Panel::ALL {
        let rect = layout(area, panels)[panel.index()];
        if rect.is_empty() || !rect.contains((x, y).into()) {
            continue;
        }
        if y == rect.y && x >= rect.right().saturating_sub(4) && x < rect.right() - 1 {
            panels.close(panel);
        } else if y == rect.y
            && x >= rect.right().saturating_sub(7)
            && x < rect.right().saturating_sub(4)
        {
            panels.zoom(panel);
        } else {
            panels.focus(panel);
        }
        break;
    }
}
pub fn mouse(app: &mut App, area: Rect, x: u16, y: u16) {
    if app.help {
        app.help = false;
        return;
    }
    if let Some(selected) = app.project_picker {
        let inner = panel("", CYAN).inner(modal(area));
        if inner.contains((x, y).into()) {
            let offset = selected.saturating_sub(inner.height.saturating_sub(1) as usize);
            let i = offset + (y - inner.y) as usize;
            if i < app.projects.len() {
                app.project_picker = Some(i);
            }
        }
        return;
    }
    if app.browser.editing || app.request.editing {
        return;
    }
    let resource = layout(area, &app.panels)[2];
    click(&mut app.panels, area, x, y);
    if resource.is_empty()
        || !resource.contains((x, y).into())
        || y < resource.y + 2
        || y >= resource.bottom() - 1
    {
        return;
    }
    if app.browser.view == View::List {
        if let Some(snapshot) = &app.snapshot {
            let rows = app.browser.rows(snapshot, app.tab);
            let selected = rows
                .iter()
                .position(|r| Some(&r.name) == app.browser.selected.as_ref())
                .unwrap_or(0);
            let offset = selected.saturating_sub(resource.height.saturating_sub(4) as usize);
            app.browser.selected = rows
                .get(offset + (y - resource.y - 2) as usize)
                .map(|r| r.name.clone())
                .or_else(|| app.browser.selected.clone());
        }
    } else if app.browser.view == View::Logs && !app.logs.expanded {
        let rows = app.logs.rows();
        let selected = app.logs.selected.min(rows.len().saturating_sub(1));
        let offset = selected.saturating_sub(resource.height.saturating_sub(4) as usize);
        let next = (offset + (y - resource.y - 2) as usize).min(rows.len().saturating_sub(1));
        drop(rows);
        app.logs.selected = next;
        app.logs.follow = false;
    }
}

pub fn draw(frame: &mut Frame, app: &App, screen: &vt100::Screen, ended: bool) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG).fg(FG)), area);
    if area.width < 50 || area.height < 15 {
        frame.render_widget(
            Paragraph::new("Enlarge to 50 columns × 15 rows. F10 exits.")
                .style(Style::default().fg(YELLOW)),
            area,
        );
        return;
    }
    let regions = layout(area, &app.panels);
    let mode = if app.demo {
        "DEMO · sample data"
    } else {
        "LIVE · adaptive analytics"
    };
    let account = if app.demo {
        "demo account".into()
    } else {
        format!(
            "account {}…",
            app.account.chars().take(10).collect::<String>()
        )
    };
    let status = if !app.message.is_empty() {
        app.message.as_str()
    } else if app.refreshing {
        "refreshing"
    } else {
        app.snapshot
            .as_ref()
            .map_or("waiting", |s| s.fetched_at.as_str())
    };
    let project = app.projects.get(app.project);
    let project_name = project.map_or("demo-project", |p| p.name.as_str());
    let environment = project.map_or("default", |p| {
        if p.environment.is_empty() {
            "default"
        } else {
            p.environment.as_str()
        }
    });
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", crate::request::safe_text(project_name)),
                Style::default()
                    .fg(BG)
                    .bg(CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " {} │ {} │ CF {account} │ Wrangler {} │ cf {} │ cftui {} alpha",
                    app.context.git,
                    crate::request::safe_text(environment),
                    app.context.wrangler,
                    app.context.cf,
                    env!("CARGO_PKG_VERSION")
                ),
                Style::default().fg(FG),
            ),
        ]))
        .style(Style::default().bg(BAND)),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let deployment = app.deployment.worker.as_ref().map_or_else(
        || "Select a Worker for deployment status".into(),
        |worker| {
            format!(
                "Deploy {}: {}",
                crate::request::safe_text(worker),
                app.deployment.summary
            )
        },
    );
    frame.render_widget(
        Paragraph::new(format!(" {mode} · {status} │ {deployment}"))
            .style(Style::default().fg(MUTED).bg(BAND)),
        Rect::new(area.x, area.y + 1, area.width, 1),
    );
    for kind in [Panel::Workers, Panel::D1] {
        let rect = regions[kind.index()];
        if rect.is_empty() {
            continue;
        }
        if let Some(snapshot) = &app.snapshot {
            let workers = kind == Panel::Workers;
            activity(
                frame,
                rect,
                if workers { "Workers" } else { "D1" },
                if workers {
                    snapshot.workers.as_deref()
                } else {
                    snapshot.databases.as_deref()
                },
                if workers {
                    snapshot.worker_history.as_ref()
                } else {
                    snapshot.database_history.as_ref()
                },
                if workers {
                    ["requests", "errors"]
                } else {
                    ["rows read", "rows written"]
                },
                (workers, app.panels.focused == kind),
            );
        } else {
            frame.render_widget(
                Paragraph::new(" Connecting…").block(controlled_panel(
                    if kind == Panel::Workers {
                        " Workers "
                    } else {
                        " D1 "
                    },
                    BORDER,
                )),
                rect,
            );
        }
    }
    if !regions[2].is_empty() {
        resources(frame, regions[2], app);
    }
    if !regions[4].is_empty() {
        request(frame, regions[4], app);
    }
    if app.panels.visible.iter().all(|v| !*v) && !app.panels.request_visible {
        frame.render_widget(
            Paragraph::new(" All panels hidden. F1-F4 toggle · c request · F9 restore all")
                .style(Style::default().fg(CYAN)),
            Rect::new(area.x, area.y + 2, area.width, 2),
        );
    }
    let title = if ended {
        " Git Bash · exited / restart to reopen "
    } else if app.panels.shell_input() {
        " Git Bash · input "
    } else {
        " Git Bash · F6 focus "
    };
    let project = app.projects.get(app.project);
    let title = format!(
        "{} {}{} ",
        title.trim(),
        project.map_or("Default", |p| p.name.as_str()),
        project
            .filter(|p| !p.environment.is_empty())
            .map_or(String::new(), |p| format!(" [{}]", p.environment))
    );
    if !regions[3].is_empty() {
        frame.render_widget(
            PseudoTerminal::new(screen)
                .style(Style::default().fg(FG).bg(BG))
                .block(controlled_panel(
                    &title,
                    if app.panels.shell_input() {
                        CYAN
                    } else {
                        BORDER
                    },
                )),
            regions[3],
        );
    }
    for kind in Panel::ALL {
        let rect = regions[kind.index()];
        if !rect.is_empty() {
            frame.render_widget(
                Paragraph::new(if app.panels.expanded == Some(kind) {
                    "[-][x]"
                } else {
                    "[+][x]"
                })
                .style(Style::default().fg(CYAN).bg(BG)),
                Rect::new(rect.right() - 7, rect.y, 6, 1),
            );
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            key(" F6 "),
            Span::raw("next "),
            key(" F8 "),
            Span::raw("hide "),
            key(" F9 "),
            Span::raw("all "),
            key(" F11 "),
            Span::raw("zoom "),
            key(" F10 "),
            Span::raw("quit  "),
            Span::styled(
                "c HTTP · F7 project · F12 save · ? help",
                Style::default().fg(MUTED),
            ),
        ]))
        .style(Style::default().bg(BAND).fg(FG)),
        Rect::new(area.x, area.bottom() - 1, area.width, 1),
    );
    if let Some(index) = app.project_picker {
        project_picker(frame, app, index);
    }
    if app.help {
        help(frame);
    }
}
fn key(label: &'static str) -> Span<'static> {
    Span::styled(
        label,
        Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
    )
}
fn request(frame: &mut Frame, area: Rect, app: &App) {
    let request = &app.request;
    let title = format!(
        " HTTP request · GET · {} ",
        if request.body_tab { "BODY" } else { "HEADERS" }
    );
    let block=controlled_panel(&title,if app.panels.focused==Panel::Request {CYAN}else{BORDER})
        .title_bottom(Line::styled(" u URL · Ctrl+U clear while editing · Enter/r send · Tab view · arrows scroll · Esc close ",Style::default().fg(MUTED)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        return;
    }
    let url = format!(
        " URL > {}{}",
        request.url,
        if request.editing { "_" } else { "" }
    );
    frame.render_widget(
        Paragraph::new(url).style(Style::default().fg(if request.editing { CYAN } else { FG })),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    if inner.height < 2 {
        return;
    }
    let body = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    if request.busy() {
        frame.render_widget(
            Paragraph::new(" Sending GET... (15-second timeout)")
                .style(Style::default().fg(YELLOW)),
            body,
        );
        return;
    }
    if let Some(error) = &request.error {
        frame.render_widget(
            Paragraph::new(format!(
                " Request failed: {}",
                crate::request::safe_text(error)
            ))
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(PURPLE)),
            body,
        );
        return;
    }
    let Some(response) = &request.response else {
        frame.render_widget(Paragraph::new(" Enter an http:// or https:// URL and press Enter.\n GET requests are real, including in demo mode.\n Redirects are shown without following them.\n Use Tab for response headers/body; [+] expands this panel.")
            .wrap(Wrap{trim:false}).style(Style::default().fg(MUTED)),body);
        return;
    };
    let mut lines = vec![
        Line::styled(
            format!(
                " HTTP {} {} · headers {} ms · total {} ms · {} bytes{}",
                response.status,
                response.reason,
                response.headers_ms,
                response.total_ms,
                response.bytes,
                if response.truncated {
                    " (first 256 KiB; truncated)"
                } else {
                    ""
                }
            ),
            Style::default().fg(if response.status >= 400 {
                PURPLE
            } else if response.status >= 300 {
                YELLOW
            } else {
                GREEN
            }),
        ),
        Line::styled(
            format!(" GET {}", crate::request::safe_text(&response.url)),
            Style::default().fg(MUTED),
        ),
    ];
    if let Some(warning) = &response.warning {
        lines.push(Line::styled(
            crate::request::safe_text(warning),
            Style::default().fg(PURPLE),
        ));
    }
    if request.body_tab {
        if response.body.is_empty() {
            lines.push(Line::styled(
                " (empty response body)",
                Style::default().fg(MUTED),
            ));
        } else {
            lines.extend(response.body.lines().map(|line| Line::raw(line.to_owned())));
        }
    } else {
        lines.extend(response.headers.iter().map(|(name, value)| {
            Line::from(vec![
                Span::styled(
                    format!("{name}: "),
                    Style::default().fg(
                        if name.starts_with("cf-")
                            || name.starts_with("access-control-")
                            || matches!(
                                name.as_str(),
                                "location" | "cache-control" | "content-type"
                            )
                        {
                            CYAN
                        } else {
                            MUTED
                        },
                    ),
                ),
                Span::raw(crate::request::safe_text(value)),
            ])
        }));
    }
    let height = lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(body.width.max(1) as usize))
        .sum::<usize>();
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((
            request
                .scroll
                .min(height.saturating_sub(body.height as usize))
                .min(u16::MAX as usize) as u16,
            0,
        )),
        body,
    );
}
fn modal(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).min(96);
    let height = area.height.saturating_sub(4).min(20);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}
fn project_picker(frame: &mut Frame, app: &App, selected: usize) {
    let area = modal(frame.area());
    frame.render_widget(Clear, area);
    let block = panel(" Projects · Enter switch · Esc cancel ", CYAN);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let visible = inner.height as usize;
    let offset = selected.saturating_sub(visible.saturating_sub(1));
    let rows = app
        .projects
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible)
        .map(|(i, p)| {
            Line::styled(
                format!(
                    "{} {} [{}] {}",
                    if i == selected { ">" } else { " " },
                    p.name,
                    if p.environment.is_empty() {
                        "default"
                    } else {
                        &p.environment
                    },
                    p.path.display()
                ),
                Style::default()
                    .fg(if i == selected { CYAN } else { FG })
                    .bg(if i == selected { BAND } else { BG }),
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(rows), inner);
}
fn help(frame: &mut Frame) {
    let area = modal(frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new("F1-F4 toggle panels · F6 next · F8 hide · F9 restore · F11 expand\nF7 projects · F12 save layout · F10 quit\nc HTTP inspector · u URL · Enter/r send · Tab headers/body\n\n/ search · s cycle sorting · arrows select · Enter Worker details\nL logs · Esc resource list\nLogs: / search · e errors · Space pause/resume · f follow\nLogs: arrows select · Enter expand · x stop tail · L restart\n\n1 Overview · 2 Debugging · 3 Shell · [ / ] Bash panel size\nProject switching keeps a separate Bash session per project.\nLayouts save on exit. Sample logs in demo mode.\n\nAny key closes help. Shell focus forwards normal typing to Bash.")
        .wrap(Wrap{trim:false}).block(panel(" Help ",CYAN)),area);
}
fn logs(frame: &mut Frame, area: Rect, app: &App) {
    let logs = &app.logs;
    let title = format!(
        " Logs · {} · {} ",
        logs.target.as_deref().unwrap_or("none"),
        if logs.paused() {
            "PAUSED"
        } else if logs.follow {
            "FOLLOW"
        } else {
            "BROWSE"
        }
    );
    let block = controlled_panel(
        &title,
        if app.panels.focused == Panel::Resources {
            CYAN
        } else {
            BORDER
        },
    )
    .title_bottom(Line::styled(
        " / search · e errors · Space pause · Enter detail · x stop · Esc list ",
        Style::default().fg(MUTED),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        return;
    }
    let status = format!(
        "{} · {} · dropped {} · /{}{}",
        logs.status,
        if logs.errors_only {
            "errors only"
        } else {
            "all events"
        },
        logs.dropped(),
        logs.filter,
        if app.browser.editing { "_" } else { "" }
    );
    frame.render_widget(
        Paragraph::new(status).style(Style::default().fg(MUTED)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let body = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
    );
    if body.is_empty() {
        return;
    }
    let rows = logs.rows();
    let selected = logs.selected.min(rows.len().saturating_sub(1));
    if logs.expanded {
        if let Some(entry) = rows.get(selected) {
            let lines = entry
                .detail
                .lines()
                .map(|line| {
                    line.chars()
                        .count()
                        .max(1)
                        .div_ceil(body.width.max(1) as usize)
                })
                .sum::<usize>();
            frame.render_widget(
                Paragraph::new(entry.detail.as_str())
                    .wrap(Wrap { trim: false })
                    .scroll((
                        app.offset
                            .min(lines.saturating_sub(body.height as usize))
                            .min(u16::MAX as usize) as u16,
                        0,
                    ))
                    .style(Style::default().fg(if entry.error { PURPLE } else { FG })),
                body,
            );
        }
    } else if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(" No matching events yet. Live tails show new invocations only.")
                .style(Style::default().fg(MUTED)),
            body,
        );
    } else {
        let offset = selected.saturating_sub(body.height.saturating_sub(1) as usize);
        let lines = rows
            .iter()
            .enumerate()
            .skip(offset)
            .take(body.height as usize)
            .map(|(i, e)| {
                Line::styled(
                    format!("{} {}", if i == selected { ">" } else { " " }, e.summary),
                    Style::default()
                        .fg(if e.error { PURPLE } else { FG })
                        .bg(if i == selected { BAND } else { BG }),
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines), body);
    }
}
fn panel(title: &str, color: Color) -> Block<'_> {
    Block::bordered()
        .title(Span::styled(title, Style::default().fg(color)))
        .border_style(Style::default().fg(color))
        .style(Style::default().bg(BG).fg(FG))
}
fn controlled_panel(title: &str, color: Color) -> Block<'_> {
    panel(title, color)
        .title_top(Line::styled("[+][x]", Style::default().fg(CYAN)).alignment(Alignment::Right))
}
fn resources(frame: &mut Frame, area: Rect, app: &App) {
    if app.browser.view == View::Logs {
        logs(frame, area, app);
        return;
    }
    let Some(snapshot) = &app.snapshot else {
        frame.render_widget(
            Paragraph::new(" Connecting…").block(controlled_panel(" Resources ", BORDER)),
            area,
        );
        return;
    };
    if app.browser.view == View::Details {
        let name = app.browser.worker.as_deref().unwrap_or("");
        let row = snapshot
            .workers
            .as_ref()
            .and_then(|rows| rows.iter().find(|r| r.name == name));
        let history = snapshot
            .worker_histories
            .as_ref()
            .and_then(|histories| histories.get(name));
        let empty;
        let history = if history.is_none() && snapshot.worker_histories.is_some() {
            empty = History {
                start: snapshot
                    .worker_history
                    .as_ref()
                    .map_or_else(chrono::Utc::now, |h| h.start),
                first: vec![0; 24],
                second: vec![0; 24],
            };
            Some(&empty)
        } else {
            history
        };
        activity(
            frame,
            area,
            name,
            row.map(std::slice::from_ref),
            history,
            ["requests", "errors"],
            (true, app.panels.focused == Panel::Resources),
        );
        frame.render_widget(
            Paragraph::new(" Esc list · L live logs · [+] expand · each trace has its own scale ")
                .style(Style::default().fg(MUTED).bg(BG)),
            Rect::new(
                area.x + 1,
                area.bottom() - 1,
                area.width.saturating_sub(2),
                1,
            ),
        );
        return;
    }
    let (rows, headers) = if app.tab == 0 {
        (
            &snapshot.workers,
            ["Worker", "Requests / 24h", "Errors / 24h"],
        )
    } else {
        (
            &snapshot.databases,
            ["Database ID", "Reads / 24h", "Writes / 24h"],
        )
    };
    let count =
        |rows: &Option<Vec<MetricRow>>| rows.as_ref().map_or("?".into(), |r| r.len().to_string());
    let title = Line::from(vec![
        Span::raw(" Resources "),
        Span::styled(
            format!("Workers [{}]", count(&snapshot.workers)),
            Style::default()
                .fg(if app.tab == 0 { CYAN } else { MUTED })
                .add_modifier(if app.tab == 0 {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ),
        Span::raw(" / "),
        Span::styled(
            format!("D1 [{}] ", count(&snapshot.databases)),
            Style::default().fg(if app.tab == 1 { CYAN } else { MUTED }),
        ),
        Span::styled(
            format!(
                " /{}{}",
                app.browser.filter,
                if app.browser.editing { "_" } else { "" }
            ),
            Style::default().fg(YELLOW),
        ),
    ]);
    let warning = snapshot.warnings.first().cloned().unwrap_or_else(|| {
        format!(
            " / search · s sort: {} · Enter details · L logs ",
            app.browser.sort.label()
        )
    });
    let block = controlled_panel(
        "",
        if app.panels.focused == Panel::Resources {
            CYAN
        } else {
            BORDER
        },
    )
    .title(title)
    .title_bottom(Line::styled(
        warning,
        Style::default().fg(if snapshot.warnings.is_empty() {
            MUTED
        } else {
            YELLOW
        }),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    match rows {
        Some(rows) => {
            let _ = rows;
            let rows = app.browser.rows(snapshot, app.tab);
            let visible = inner.height.saturating_sub(1) as usize;
            let selected = rows
                .iter()
                .position(|r| Some(&r.name) == app.browser.selected.as_ref())
                .unwrap_or(0);
            let offset = selected.saturating_sub(visible.saturating_sub(1));
            frame.render_widget(
                Table::new(
                    rows.iter()
                        .skip(offset)
                        .take(visible)
                        .enumerate()
                        .map(|(i, row)| {
                            metric_row(
                                row,
                                i + offset,
                                app.tab,
                                Some(&row.name) == app.browser.selected.as_ref(),
                            )
                        }),
                    [
                        Constraint::Min(16),
                        Constraint::Length(if area.width < 75 { 14 } else { 19 }),
                        Constraint::Length(if area.width < 75 { 12 } else { 17 }),
                    ],
                )
                .column_spacing(1)
                .header(
                    Row::new(headers.map(Cell::from))
                        .style(Style::default().fg(BG).bg(CYAN))
                        .height(1),
                ),
                inner,
            );
            if rows.is_empty() && inner.height > 1 {
                frame.render_widget(
                    Paragraph::new(if app.browser.filter.is_empty() {
                        " No activity in the last 24 hours."
                    } else {
                        " No matching resources. / edit search · Esc clears search while editing."
                    })
                    .style(Style::default().fg(MUTED)),
                    Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1),
                );
            }
        }
        None => frame.render_widget(
            Paragraph::new(" Unavailable · check analytics permissions.")
                .style(Style::default().fg(YELLOW)),
            inner,
        ),
    }
}
fn activity(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    rows: Option<&[MetricRow]>,
    history: Option<&History>,
    labels: [&str; 2],
    context: (bool, bool),
) {
    let (workers, focused) = context;
    let title = format!(" {title} · rolling 24h / completed hours UTC ");
    let block = controlled_panel(&title, if focused { CYAN } else { BORDER });
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let columns = Layout::horizontal([
        Constraint::Length(if area.width < 75 { 21 } else { 25 }),
        Constraint::Min(1),
    ])
    .split(inner);
    let first = rows.map(|r| r.iter().map(|r| r.first).sum::<u64>());
    let second = rows.map(|r| r.iter().map(|r| r.second).sum::<u64>());
    let mut stats = vec![
        stat(first, labels[0], YELLOW),
        stat(second, labels[1], PURPLE),
    ];
    if inner.height >= 4 {
        stats.push(Line::styled(
            if workers {
                match (first, second) {
                    (Some(n), Some(e)) if n > 0 => {
                        format!(" {:.3}% error rate", e as f64 / n as f64 * 100.)
                    }
                    (Some(_), Some(_)) => " 0 requests · rate —".into(),
                    _ => " error rate —".into(),
                }
            } else {
                rows.map_or(" databases —".into(), |r| {
                    format!(" {} active databases", r.len())
                })
            },
            Style::default().fg(CYAN),
        ));
    }
    if inner.height >= 6 {
        stats.push(Line::styled(
            " Rolling 24-hour totals",
            Style::default().fg(MUTED),
        ));
    }
    frame.render_widget(Paragraph::new(stats), columns[0]);
    let traces = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(columns[1]);
    for (i, (label, color)) in [(labels[0], YELLOW), (labels[1], PURPLE)]
        .iter()
        .enumerate()
    {
        let values = history.map(|h| {
            if i == 0 {
                h.first.as_slice()
            } else {
                h.second.as_slice()
            }
        });
        trace(frame, traces[i], label, *color, values, history);
    }
}
fn stat(value: Option<u64>, label: &str, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            value.map_or(" unavailable".into(), |n| format!(" {}", number(n))),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {label}"), Style::default().fg(MUTED)),
    ])
}
fn trace(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    color: Color,
    values: Option<&[u64]>,
    history: Option<&History>,
) {
    if area.is_empty() {
        return;
    }
    let Some(values) = values else {
        frame.render_widget(
            Paragraph::new(format!("{label} · history unavailable"))
                .style(Style::default().fg(MUTED)),
            area,
        );
        return;
    };
    let peak = values.iter().copied().max().unwrap_or(0);
    if area.height == 1 {
        let label_width = 25.min(area.width);
        frame.render_widget(
            Paragraph::new(format!("{label} {}/h ", number(peak)))
                .style(Style::default().fg(color)),
            Rect::new(area.x, area.y, label_width, 1),
        );
        histogram(
            frame,
            Rect::new(area.x + label_width, area.y, area.width - label_width, 1),
            values,
            peak,
            color,
        );
        return;
    }
    let caption = if peak == 0 {
        format!("{label} · no activity")
    } else {
        format!("{label} · peak {}/h", number(peak))
    };
    frame.render_widget(
        Paragraph::new(caption).style(Style::default().fg(color)),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let axis_height = u16::from(area.height >= 4);
    let graph = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height - 1 - axis_height,
    );
    histogram(frame, graph, values, peak, color);
    if axis_height > 0 {
        if let Some(history) = history {
            let axis = Rect::new(area.x, area.bottom() - 1, area.width, 1);
            let start = format!("{} UTC", history.start.format("%d %b %H:%M"));
            let end = format!(
                "{} UTC",
                (history.start + chrono::Duration::hours(24)).format("%d %b %H:%M")
            );
            let caption = if axis.width as usize > start.len() + end.len() {
                let gap = axis.width as usize - start.len() - end.len();
                format!("{start}{}{end}", " ".repeat(gap))
            } else {
                start
            };
            frame.render_widget(
                Paragraph::new(caption).style(Style::default().fg(MUTED)),
                axis,
            );
        }
    }
}
// Background-colour cells avoid the fractional block glyphs missing in some Git Bash fonts.
fn histogram(frame: &mut Frame, area: Rect, values: &[u64], peak: u64, color: Color) {
    let samples = spread(values, area.width);
    let levels = b".:-=+*#%@";
    for (x, value) in samples.iter().enumerate() {
        if area.height == 1 {
            let level = if peak == 0 {
                0
            } else {
                ((*value as u128 * (levels.len() - 1) as u128).div_ceil(peak as u128)) as usize
            };
            frame.buffer_mut()[(area.x + x as u16, area.y)]
                .set_char(levels[level] as char)
                .set_fg(if *value == 0 { MUTED } else { color })
                .set_bg(BG);
        } else {
            let height = if peak == 0 {
                0
            } else {
                (*value as u128 * area.height as u128).div_ceil(peak as u128) as u16
            };
            let gap = area.width as usize >= values.len() * 3
                && x > 0
                && x * values.len() / area.width as usize
                    != (x - 1) * values.len() / area.width as usize;
            for y in 0..area.height {
                let filled = !gap && y >= area.height - height;
                frame.buffer_mut()[(area.x + x as u16, area.y + y)]
                    .set_char(if !filled && y == area.height - 1 {
                        '.'
                    } else {
                        ' '
                    })
                    .set_fg(MUTED)
                    .set_bg(if filled { color } else { BG });
            }
        }
    }
}
// Repeat hourly buckets across wider panels without inventing interpolated data.
fn spread(values: &[u64], width: u16) -> Vec<u64> {
    if values.is_empty() || width == 0 {
        return vec![];
    }
    (0..width as usize)
        .map(|i| values[i * values.len() / width as usize])
        .collect()
}
fn metric_row(row: &MetricRow, index: usize, tab: usize, selected: bool) -> Row<'static> {
    let numeric = |value, color| {
        Cell::from(
            Line::styled(number(value), Style::default().fg(color)).alignment(Alignment::Right),
        )
    };
    Row::new(vec![
        Cell::from(format!(
            "{}{}",
            if selected { "> " } else { "  " },
            row.name
        ))
        .style(Style::default().fg(if selected { CYAN } else { GREEN })),
        numeric(row.first, FG),
        numeric(
            row.second,
            if tab == 0 && row.second == 0 {
                MUTED
            } else {
                PURPLE
            },
        ),
    ])
    .style(Style::default().bg(if selected {
        BAND
    } else if index.is_multiple_of(2) {
        BG
    } else {
        Color::Rgb(32, 36, 36)
    }))
}
fn number(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloudflare::Snapshot;
    use ratatui::{backend::TestBackend, Terminal};
    fn app() -> App {
        let mut app = App::new(true, String::new(), vec![]);
        app.snapshot = Some(Snapshot::demo());
        app.refreshing = false;
        app
    }
    fn text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }
    #[test]
    fn header_keeps_project_and_git_context_visible() {
        let mut app = app();
        app.projects = vec![crate::config::Project {
            name: "my-api".into(),
            path: ".".into(),
            environment: "production".into(),
        }];
        app.context = crate::context::Status {
            git: "main* ↑2 ↓1".into(),
            wrangler: "4.0.0".into(),
            cf: "0.1.0-beta".into(),
        };
        app.deployment.worker = Some("api-production".into());
        app.deployment.summary = "Serving · demo version 100%".into();
        let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
        let screen = vt100::Parser::new(24, 80, 0);
        terminal
            .draw(|f| draw(f, &app, screen.screen(), false))
            .unwrap();
        let output = text(&terminal);
        for expected in [
            "my-api",
            "main* ↑2 ↓1",
            "production",
            "Wrangler 4.0.0",
            "cf 0.1.0-beta",
            "Serving · demo version 100%",
        ] {
            assert!(output.contains(expected), "missing {expected}");
        }
    }
    #[test]
    fn renders_graphs_and_unavailable_without_fake_activity() {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut app = app();
        let screen = vt100::Parser::new(24, 80, 0);
        terminal
            .draw(|f| draw(f, &app, screen.screen(), false))
            .unwrap();
        assert!(text(&terminal).contains("210,620"));
        assert!(text(&terminal).contains("api-production"));
        assert!(text(&terminal).contains("peak 18,100/h"));
        let snapshot = app.snapshot.as_mut().unwrap();
        snapshot.workers = None;
        snapshot.worker_history = None;
        snapshot
            .warnings
            .push("Workers: Analytics query rejected".into());
        terminal
            .draw(|f| draw(f, &app, screen.screen(), false))
            .unwrap();
        assert!(text(&terminal).contains("history unavailable"));
        assert!(text(&terminal).contains("Analytics query rejected"));
        assert!(!text(&terminal).contains("210,620"));
    }
    #[test]
    fn responsive_layout_keeps_shell_and_table_visible() {
        let screen = vt100::Parser::new(24, 80, 0);
        for (w, h) in [(50, 15), (80, 24), (120, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| draw(f, &app(), screen.screen(), false))
                .unwrap();
            assert!(text(&terminal).contains("Resources"));
            assert!(text(&terminal).contains("Git Bash"));
            assert!(text(&terminal).contains("F10"));
            assert!(shell_area(Rect::new(0, 0, w, h), &Panels::default()).height >= 5);
        }
        let area = Rect::new(0, 0, 100, 35);
        let mut panels = Panels::default();
        panels.zoom(Panel::Bash);
        assert!(shell_area(area, &panels).height > shell_area(area, &Panels::default()).height);
        assert_eq!(shell_area(area, &panels).bottom(), 34);
    }
    #[test]
    fn visibility_combinations_and_zoom_render_without_overlap() {
        let screen = vt100::Parser::new(24, 80, 0);
        for (w, h) in [(50, 15), (80, 24), (120, 40)] {
            for mask in 0..32 {
                let mut app = app();
                app.panels.visible = std::array::from_fn(|i| mask & (1 << i) != 0);
                app.panels.request_visible = mask & 16 != 0;
                let area = Rect::new(0, 0, w, h);
                let regions = layout(area, &app.panels);
                for (i, rect) in regions.iter().enumerate() {
                    if !app.panels.is_visible(Panel::ALL[i]) {
                        assert!(rect.is_empty());
                    }
                    for other in &regions[i + 1..] {
                        assert!(rect.intersection(*other).is_empty());
                    }
                }
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                terminal
                    .draw(|f| draw(f, &app, screen.screen(), false))
                    .unwrap();
                if mask == 0 {
                    assert!(text(&terminal).contains("All panels hidden"));
                }
                for panel in Panel::ALL {
                    if app.panels.is_visible(panel) {
                        app.panels.zoom(panel);
                        assert_eq!(layout(area, &app.panels)[panel.index()].height, h - 3);
                        terminal
                            .draw(|f| draw(f, &app, screen.screen(), false))
                            .unwrap();
                        assert!(text(&terminal).contains("[-][x]"));
                        app.panels.zoom(panel);
                    }
                }
            }
        }
    }
    #[test]
    fn mouse_controls_zoom_close_and_reopen() {
        let mut panels = Panels::default();
        let area = Rect::new(0, 0, 120, 40);
        let workers = layout(area, &panels)[0];
        click(&mut panels, area, workers.right() - 6, workers.y);
        assert_eq!(panels.expanded, Some(Panel::Workers));
        let zoomed = layout(area, &panels)[0];
        click(&mut panels, area, zoomed.right() - 3, zoomed.y);
        assert!(!panels.visible[0]);
        assert_eq!(panels.expanded, None);
        panels.toggle(Panel::Workers);
        assert!(panels.visible[0]);
        assert_eq!(panels.focused, Panel::Workers);
    }
    #[test]
    fn charts_use_font_safe_cells_and_preserve_zero_activity() {
        for height in [1, 3] {
            let mut terminal = Terminal::new(TestBackend::new(48, height)).unwrap();
            terminal
                .draw(|f| histogram(f, f.area(), &[0, 1, 4], 4, YELLOW))
                .unwrap();
            assert!(terminal
                .backend()
                .buffer()
                .content
                .iter()
                .all(|cell| cell.symbol().is_ascii()));
            if height > 1 {
                let buffer = terminal.backend().buffer();
                assert!(buffer.content.iter().any(|cell| cell.bg == YELLOW));
                assert_eq!(buffer[(0, height - 1)].bg, BG);
            }
            terminal
                .draw(|f| histogram(f, f.area(), &[0, 0, 0], 0, YELLOW))
                .unwrap();
            assert!(terminal
                .backend()
                .buffer()
                .content
                .iter()
                .all(|cell| cell.bg != YELLOW));
        }
    }
    #[test]
    fn worker_details_logs_and_mouse_selection_render() {
        let mut app = app();
        let area = Rect::new(0, 0, 120, 40);
        let resource = layout(area, &app.panels)[2];
        mouse(&mut app, area, resource.x + 3, resource.y + 3);
        assert_eq!(app.browser.selected.as_deref(), Some("public-site"));
        app.browser.worker = Some("api-production".into());
        app.browser.view = View::Details;
        app.panels.zoom(Panel::Resources);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let screen = vt100::Parser::new(24, 80, 0);
        terminal
            .draw(|f| draw(f, &app, screen.screen(), false))
            .unwrap();
        assert!(text(&terminal).contains("128,430"));
        assert!(!text(&terminal).contains("210,620"));
        app.snapshot.as_mut().unwrap().worker_histories = None;
        terminal
            .draw(|f| draw(f, &app, screen.screen(), false))
            .unwrap();
        assert!(text(&terminal).contains("history unavailable"));
        app.logs
            .start(
                std::path::Path::new("unused"),
                &crate::config::Project {
                    name: "Demo".into(),
                    path: ".".into(),
                    environment: String::new(),
                },
                "",
                "api-production",
                true,
            )
            .unwrap();
        app.browser.view = View::Logs;
        app.logs.errors_only = true;
        terminal
            .draw(|f| draw(f, &app, screen.screen(), false))
            .unwrap();
        assert!(text(&terminal).contains("database request failed"));
        app.logs.expanded = true;
        terminal
            .draw(|f| draw(f, &app, screen.screen(), false))
            .unwrap();
        assert!(text(&terminal).contains("exceptions"));
        for (w, h) in [(50, 15), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| draw(f, &app, screen.screen(), false))
                .unwrap();
            app.help = true;
            terminal
                .draw(|f| draw(f, &app, screen.screen(), false))
                .unwrap();
            app.help = false;
        }
    }
    #[test]
    fn request_panel_loads_old_layout_and_renders_errors() {
        let mut app = app();
        let screen = vt100::Parser::new(24, 80, 0);
        app.panels.zoom(Panel::Request);
        app.request.url = "http://localhost/check".into();
        app.request.error = Some("Connection refused".into());
        for (w, h) in [(50, 15), (80, 24), (120, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| draw(f, &app, screen.screen(), false))
                .unwrap();
            assert!(text(&terminal).contains("Connection refused"));
            assert!(text(&terminal).contains("HTTP request"));
        }
        let old: Panels =
            toml::from_str("visible = [true,true,true,true]\nfocused = 'Bash'\nshell_percent = 33")
                .unwrap();
        assert!(!old.request_visible);
        assert!(old.shell_input());
    }
    #[test]
    fn chart_spreading_preserves_bucket_values() {
        assert_eq!(spread(&[0, 2, 8], 6), vec![0, 0, 2, 2, 8, 8]);
        assert_eq!(spread(&[], 8), Vec::<u64>::new());
    }
}
