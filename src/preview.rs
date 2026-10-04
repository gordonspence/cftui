use crate::{cloudflare::Snapshot, ui, App};
use anyhow::Result;
use ratatui::{
    backend::TestBackend,
    style::{Color, Modifier},
    Terminal,
};
use std::{fmt::Write, path::Path};
use tui_term::vt100;

/// Export the same cell buffer used by the TUI, not a separate design mockup.
pub fn write(path: &Path) -> Result<()> {
    let mut app = App::new(
        true,
        String::new(),
        vec![crate::config::Project {
            name: "demo-project".into(),
            path: "./project".into(),
            environment: "production".into(),
        }],
    );
    app.context = crate::context::Status {
        git: "main* ↑1".into(),
        wrangler: "demo".into(),
        cf: "demo".into(),
    };
    app.deployment.worker = Some("api-production".into());
    app.deployment.summary = "DEMO · Serving · demo0001 100%".into();
    app.snapshot = Some(Snapshot::demo());
    app.refreshing = false;
    let mut screen = vt100::Parser::new(11, 118, 0);
    screen.process(b"\x1b[32mpreview@local\x1b[0m MINGW64 ~/project\r\n$ \r\n\r\nLayout preview only. Launch cftui to open the real Git Bash session.");
    let mut terminal = Terminal::new(TestBackend::new(120, 40))?;
    terminal.draw(|frame| ui::draw(frame, &mut app, screen.screen(), false))?;
    let buffer = terminal.backend().buffer();
    let mut html = String::from("<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>cftui · layout preview</title><style>body{margin:0;padding:24px;background:#141718}pre{margin:0;width:max-content;font:14px/18px Consolas,'DejaVu Sans Mono',monospace;white-space:pre;font-variant-ligatures:none}span{display:inline-block;width:1ch;height:18px;overflow:hidden;vertical-align:top}</style><pre aria-label=\"cftui demo layout preview\">");
    for y in 0..40 {
        for x in 0..120 {
            let cell = &buffer[(x, y)];
            let fg = color(cell.fg, "#d3d9d4");
            let bg = color(cell.bg, "#1b1e1f");
            write!(
                html,
                "<span style=\"color:{fg};background:{bg};font-weight:{}\">{}</span>",
                if cell.modifier.contains(Modifier::BOLD) {
                    700
                } else {
                    400
                },
                escape(cell.symbol())
            )?;
        }
        html.push('\n');
    }
    html.push_str("</pre></html>");
    std::fs::write(path, html)?;
    println!("Layout preview written to {}", path.display());
    Ok(())
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn color(color: Color, default: &str) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Black => "#1b1e1f".into(),
        Color::Red => "#cc7777".into(),
        Color::Green => "#a5bd76".into(),
        Color::Yellow => "#e1c959".into(),
        Color::Blue => "#91aaca".into(),
        Color::Magenta => "#b993cf".into(),
        Color::Cyan => "#8bc9c4".into(),
        Color::White => "#d3d9d4".into(),
        _ => default.into(),
    }
}
