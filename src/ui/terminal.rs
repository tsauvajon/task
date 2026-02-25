use std::io;

use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub(super) type AppTerminal = Terminal<CrosstermBackend<io::Stdout>>;

pub(super) struct TerminalGuard {
    terminal: Option<AppTerminal>,
}

impl TerminalGuard {
    pub(super) fn new() -> crate::error::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        execute!(stdout, crossterm::event::EnableMouseCapture)?;

        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal: Some(terminal),
        })
    }

    pub(super) fn terminal_mut(&mut self) -> &mut AppTerminal {
        self.terminal
            .as_mut()
            .expect("terminal guard must contain terminal")
    }
}

fn restore_terminal(terminal: &mut AppTerminal) -> crate::error::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if let Some(terminal) = self.terminal.as_mut() {
            let _ = restore_terminal(terminal);
        }
    }
}
