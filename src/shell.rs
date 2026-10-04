use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::{
    io::{Read, Write},
    path::Path,
    sync::mpsc::{self, Receiver},
    thread,
};
use tui_term::vt100;

pub struct Shell {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    pub parser: vt100::Parser<Replies>,
    size: (u16, u16),
    pub ended: bool,
}
impl Shell {
    pub fn start(executable: &Path, cwd: &Path) -> Result<Self> {
        let pair = native_pty_system().openpty(size(24, 80))?;
        let mut command = CommandBuilder::new(executable);
        command.args(["--login", "-i"]);
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        command.env("CHERE_INVOKING", "1");
        // The dashboard credential is not Wrangler's credential.
        command.env_remove("CFTUI_API_TOKEN");
        let child = pair
            .slave
            .spawn_command(command)
            .context("Could not launch Git Bash")?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let (tx, output) = mpsc::sync_channel(128);
        thread::spawn(move || {
            let mut buffer = [0; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buffer[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Ok(Self {
            master: pair.master,
            child,
            writer,
            output,
            parser: vt100::Parser::new_with_callbacks(24, 80, 2000, Replies::default()),
            size: (24, 80),
            ended: false,
        })
    }
    pub fn drain(&mut self) -> Result<()> {
        // Bound work per frame so a busy command cannot starve keyboard input.
        for _ in 0..128 {
            match self.output.try_recv() {
                Ok(bytes) => {
                    self.parser.process(&bytes);
                    let replies = std::mem::take(&mut self.parser.callbacks_mut().bytes);
                    if !replies.is_empty() {
                        self.writer.write_all(&replies)?;
                        self.writer.flush()?;
                    }
                }
                Err(_) => break,
            }
        }
        self.ended = self.child.try_wait()?.is_some();
        Ok(())
    }
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        let dims = (rows.max(1), cols.max(1));
        if self.size != dims {
            self.master.resize(size(dims.0, dims.1))?;
            self.parser.screen_mut().set_size(dims.0, dims.1);
            self.size = dims;
        }
        Ok(())
    }
    pub fn send(&mut self, bytes: &[u8]) -> Result<()> {
        if !self.ended {
            self.parser.screen_mut().set_scrollback(0);
            self.writer.write_all(bytes)?;
            self.writer.flush()?;
        }
        Ok(())
    }
    pub fn scroll(&mut self, up: bool) {
        let offset = self.parser.screen().scrollback();
        self.parser.screen_mut().set_scrollback(if up {
            offset.saturating_add(5)
        } else {
            offset.saturating_sub(5)
        });
    }
    pub fn key(&mut self, key: KeyEvent) -> Result<()> {
        let bytes = encode_key(key, self.parser.screen().application_cursor());
        self.send(&bytes)
    }
}
impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// ConPTY asks for a cursor position before it starts the child console.
/// Reply through the PTY, using the virtual screen rather than the outer TUI.
#[derive(Default)]
pub struct Replies {
    bytes: Vec<u8>,
}
impl vt100::Callbacks for Replies {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let parameter = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        match (i1, c, parameter) {
            (None, 'n', 6) => {
                let (row, col) = screen.cursor_position();
                self.bytes
                    .extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
            }
            (None, 'n', 5) => self.bytes.extend_from_slice(b"\x1b[0n"),
            (None, 'c', 0) => self.bytes.extend_from_slice(b"\x1b[?1;2c"),
            _ => {}
        }
    }
}

fn encode_key(key: KeyEvent, application_cursor: bool) -> Vec<u8> {
    let mut bytes = match key.code {
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            match c.to_ascii_lowercase() {
                'a'..='z' => vec![c.to_ascii_lowercase() as u8 - b'a' + 1],
                ' ' | '@' => vec![0],
                '[' => vec![27],
                '\\' => vec![28],
                ']' => vec![29],
                '^' => vec![30],
                '_' => vec![31],
                _ => vec![],
            }
        }
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![127],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => vec![27],
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::Right
        | KeyCode::Left
        | KeyCode::Home
        | KeyCode::End => {
            let final_byte = match key.code {
                KeyCode::Up => 'A',
                KeyCode::Down => 'B',
                KeyCode::Right => 'C',
                KeyCode::Left => 'D',
                KeyCode::Home => 'H',
                _ => 'F',
            };
            let modifier = 1
                + u8::from(key.modifiers.contains(KeyModifiers::SHIFT))
                + 2 * u8::from(key.modifiers.contains(KeyModifiers::ALT))
                + 4 * u8::from(key.modifiers.contains(KeyModifiers::CONTROL));
            if modifier > 1 {
                format!("\x1b[1;{modifier}{final_byte}").into_bytes()
            } else {
                format!(
                    "\x1b{}{final_byte}",
                    if application_cursor { 'O' } else { '[' }
                )
                .into_bytes()
            }
        }
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        _ => vec![],
    };
    if key.modifiers.contains(KeyModifiers::ALT) && matches!(key.code, KeyCode::Char(_)) {
        bytes.insert(0, 27);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replies_to_fragmented_conpty_cursor_handshake() {
        let mut parser = vt100::Parser::new_with_callbacks(24, 80, 0, Replies::default());
        parser.process(b"\x1b[");
        parser.process(b"6n");
        assert_eq!(parser.callbacks().bytes, b"\x1b[1;1R");
        parser.callbacks_mut().bytes.clear();
        parser.process(b"\x1b[3;7H\x1b[6n");
        assert_eq!(parser.callbacks().bytes, b"\x1b[3;7R");
    }
    #[test]
    fn forwards_interrupt_completion_and_unicode() {
        assert_eq!(
            encode_key(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                false
            ),
            vec![3]
        );
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), false),
            vec![9]
        );
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Char('é'), KeyModifiers::NONE), false),
            "é".as_bytes()
        );
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), true),
            b"\x1bOA"
        );
    }
}
