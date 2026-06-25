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

    pub(super) fn terminal_mut(&mut self) -> crate::error::Result<&mut AppTerminal> {
        self.terminal
            .as_mut()
            .ok_or_else(|| crate::error::Error::failed("terminal guard is missing terminal"))
    }
}

fn restore_terminal_best_effort(terminal: &mut AppTerminal) {
    let _raw_mode_result = disable_raw_mode();
    let _screen_result = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    );
    let _cursor_result = terminal.show_cursor();
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if let Some(terminal) = self.terminal.as_mut() {
            restore_terminal_best_effort(terminal);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::TerminalGuard;

    #[test]
    fn drop_ignores_none_terminal() {
        let drop_result = catch_unwind(AssertUnwindSafe(|| {
            let guard = TerminalGuard { terminal: None };
            drop(guard);
        }));

        assert!(drop_result.is_ok());
    }
}
