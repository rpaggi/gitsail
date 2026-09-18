//! Terminal lifecycle (US-043 criterion 1): entering raw mode and the
//! alternate screen, and restoring both on every exit path — normal
//! shutdown, a handled error, or a panic — so a crash mid-render never
//! leaves the user's shell in raw mode with a hidden cursor.

use std::io::{self, Stdout};

use crossterm::event::{DisableFocusChange, EnableFocusChange};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Enters raw mode and the alternate screen, and installs a panic hook
/// that restores the terminal *before* the default panic handler prints —
/// otherwise a panic during rendering would print its message into a
/// terminal still in raw mode / on the alternate screen, garbling it and
/// leaving the shell unusable until the user runs `reset`.
pub fn init() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // Enables `crossterm::event::Event::FocusGained`/`FocusLost` (T-234/
    // US-082 criterion 2): regaining OS-level focus re-detects the
    // in-progress operation and the rest of the refresh set, the same way
    // `apps/desktop`'s window `focus` event already does. Best-effort — a
    // terminal emulator that does not report focus changes simply never
    // sends the event, and the person still has the manual refresh key
    // (`r`) and the after-mutation/on-open refreshes as before.
    execute!(stdout, EnableFocusChange)?;

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore();
        default_hook(panic_info);
    }));

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

/// Leaves the alternate screen and disables raw mode. Called on every exit
/// path (normal return, a propagated error, and — via the panic hook
/// installed by [`init`] — a panic), so restoration never depends on which
/// path was taken. Safe to call more than once.
pub fn restore() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), DisableFocusChange, LeaveAlternateScreen)?;
    Ok(())
}
