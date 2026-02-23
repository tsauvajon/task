use std::io;

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub(super) type AppTerminal = Terminal<CrosstermBackend<io::Stdout>>;

pub(super) fn init_terminal() -> Result<AppTerminal, String> {
    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| e.to_string())?;
    execute!(stdout, crossterm::event::EnableMouseCapture).map_err(|e| e.to_string())?;

    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).map_err(|e| e.to_string())
}

pub(super) fn restore_terminal(terminal: &mut AppTerminal) -> Result<(), String> {
    disable_raw_mode().map_err(|e| e.to_string())?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )
    .map_err(|e| e.to_string())?;
    terminal.show_cursor().map_err(|e| e.to_string())?;
    Ok(())
}
