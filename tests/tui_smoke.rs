//! Run explicitly on a Windows machine with Git Bash and ConPTY available.
#![cfg(windows)]
#[path = "../src/shell.rs"]
#[allow(dead_code)]
mod shell;
fn wake() {}
use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires local Git Bash and nested ConPTY"]
fn idle_dashboard_sleeps_and_shell_output_wakes_it() {
    let directory = std::env::temp_dir().join(format!("cftui-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let mut terminal =
        shell::Shell::start(Path::new(r"C:\Program Files\Git\bin\bash.exe"), &directory).unwrap();
    terminal.resize(40, 180).unwrap();
    let executable = env!("CARGO_BIN_EXE_cftui").replace('\\', "/");
    terminal
        .send(
            format!(
                "'{}' --demo --config smoke.toml\r",
                executable.replace('\'', "'\"'\"'")
            )
            .as_bytes(),
        )
        .unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(45) {
        terminal.drain().unwrap();
        let screen = terminal.parser.screen().contents();
        if screen.contains("sample deployment")
            && screen.contains("Wrangler")
            && !screen.contains("checking…")
        {
            break;
        }
        thread::sleep(Duration::from_millis(30));
    }
    let screen = terminal.parser.screen().contents();
    assert!(
        screen.contains("DEMO · sample data"),
        "dashboard did not load: {screen}"
    );
    assert!(
        screen.contains("sample deployment"),
        "deployment result did not wake UI: {screen}"
    );
    assert!(
        !screen.contains("checking…"),
        "context did not finish: {screen}"
    );
    let settle = Instant::now();
    while settle.elapsed() < Duration::from_secs(1) {
        terminal.drain().unwrap();
        thread::sleep(Duration::from_millis(30));
    }
    let before = terminal.parser.screen().contents();
    terminal.drain().unwrap();
    let idle = Instant::now();
    let mut output = false;
    while idle.elapsed() < Duration::from_secs(2) {
        output |= terminal.drain().unwrap();
        thread::sleep(Duration::from_millis(30));
    }
    assert!(
        !output,
        "idle TUI emitted terminal output; before:\n{before}\nafter:\n{}",
        terminal.parser.screen().contents()
    );
    // F6 focuses Bash; normal input must still be forwarded to the nested shell.
    terminal.send(b"\x1b[17~").unwrap();
    thread::sleep(Duration::from_millis(200));
    terminal.send(b"printf 'WAKE_%s\\n' VERIFIED\r").unwrap();
    let start = Instant::now();
    loop {
        terminal.drain().unwrap();
        if terminal
            .parser
            .screen()
            .contents()
            .contains("WAKE_VERIFIED")
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "shell output did not wake dashboard"
        );
        thread::sleep(Duration::from_millis(30));
    }
    terminal.resize(36, 120).unwrap();
    thread::sleep(Duration::from_millis(300));
    assert!(terminal.drain().unwrap(), "resize did not redraw dashboard");
    terminal.send(b"\x1b[21~").unwrap();
    thread::sleep(Duration::from_millis(500));
    drop(terminal);
    // Files created by the smoke run contain only its demo layout.
    let _ = std::fs::remove_file(directory.join("smoke.state.toml"));
    let _ = std::fs::remove_dir(directory);
}
