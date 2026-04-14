mod app;
mod handler;
mod ui;

use std::{
    io::{Stdout, stdout},
    panic,
    time::Duration,
};

use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste, poll, read},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use langcodec::Codec;
use ratatui::{Terminal, backend::CrosstermBackend};

use self::{
    app::App,
    handler::{HandlerResult, handle_event},
};

pub struct BrowseOptions {
    pub input: String,
    pub lang: Option<String>,
}

struct TermGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TermGuard {
    fn new() -> Result<Self, String> {
        enable_raw_mode().map_err(|e| {
            format!("Failed to enable raw mode: {e}\nHint: 'browse' requires an interactive terminal (TTY).")
        })?;
        let mut out = stdout();
        execute!(out, EnterAlternateScreen, EnableBracketedPaste)
            .map_err(|e| format!("Failed to enter alternate screen: {e}"))?;
        let backend = CrosstermBackend::new(out);
        let terminal = Terminal::new(backend)
            .map_err(|e| format!("Failed to create terminal: {e}"))?;
        Ok(Self { terminal })
    }
}

impl Drop for TermGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}

pub fn run_browse_command(opts: BrowseOptions) -> Result<(), String> {
    let file_path = opts.input.clone();

    // Infer format from extension so we can write back in the same format
    let inferred_format = langcodec::infer_format_from_path(&file_path)
        .ok_or_else(|| format!("Cannot detect format for '{file_path}'"))?;

    // Load the file
    let mut codec = Codec::new();
    codec
        .read_file_by_extension(&file_path, opts.lang)
        .map_err(|e| format!("Failed to read '{file_path}': {e}"))?;

    if codec.resources.is_empty() {
        return Err(format!("No localization data found in '{file_path}'"));
    }

    let mut app = App::new(codec, file_path, inferred_format);

    // Install panic hook that restores the terminal before printing the panic
    let hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let mut out = stdout();
        let _ = execute!(out, DisableBracketedPaste, LeaveAlternateScreen);
        hook(info);
    }));

    let mut term = TermGuard::new()?;

    let mut prev_key_idx = app.key_list_state.selected();

    loop {
        term.terminal
            .draw(|frame| ui::render(frame, &mut app))
            .map_err(|e| format!("Render error: {e}"))?;

        if poll(Duration::from_millis(50))
            .map_err(|e| format!("Input poll error: {e}"))?
        {
            let event = read().map_err(|e| format!("Input read error: {e}"))?;
            if let HandlerResult::Quit = handle_event(&mut app, event) {
                break;
            }
            // Force full repaint when key selection changes to clear complex-script
            // glyph artifacts that ratatui's diff renderer leaves behind.
            let cur_key_idx = app.key_list_state.selected();
            if cur_key_idx != prev_key_idx {
                prev_key_idx = cur_key_idx;
                term.terminal.clear().ok();
            }
        }
    }

    Ok(())
}
