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
            // Complex-script glyphs (Arabic, Bengali, Hindi, …) can render wider
            // than unicode-width reports, leaving ghost cells that ratatui's diff
            // renderer won't overwrite.  A full terminal clear fixes this, but it
            // causes a visible flash on every navigation.  Only clear when the
            // old or new key actually contains complex-script text.
            let cur_key_idx = app.key_list_state.selected();
            if cur_key_idx != prev_key_idx {
                let old_key = prev_key_idx.and_then(|i| app.filtered_keys.get(i).cloned());
                let new_key = cur_key_idx.and_then(|i| app.filtered_keys.get(i).cloned());
                prev_key_idx = cur_key_idx;

                let needs_clear = [old_key, new_key].iter().any(|k| {
                    k.as_ref().map(|key| {
                        app.languages.iter().any(|lang| {
                            app.get_translation(key, lang)
                                .map(|v| has_complex_scripts(&v))
                                .unwrap_or(false)
                        })
                    }).unwrap_or(false)
                });

                if needs_clear {
                    term.terminal.clear().ok();
                }
            }
        }
    }

    Ok(())
}

/// Returns true if the string contains characters from scripts whose glyphs
/// commonly render wider than unicode-width predicts (Arabic, Devanagari,
/// Bengali, Tamil, Thai, Gujarati, Gurmukhi, Kannada, Malayalam, Telugu, …).
/// Used to decide whether a full terminal clear is needed to avoid artifacts.
fn has_complex_scripts(s: &str) -> bool {
    s.chars().any(|c| {
        let cp = c as u32;
        matches!(cp,
            0x0600..=0x06FF  // Arabic
            | 0x0750..=0x077F // Arabic Supplement
            | 0xFB50..=0xFDFF // Arabic Pres. Forms-A
            | 0xFE70..=0xFEFF // Arabic Pres. Forms-B
            | 0x0900..=0x097F // Devanagari (Hindi, Marathi, …)
            | 0x0980..=0x09FF // Bengali / Assamese
            | 0x0A00..=0x0A7F // Gurmukhi (Punjabi)
            | 0x0A80..=0x0AFF // Gujarati
            | 0x0B00..=0x0B7F // Odia
            | 0x0B80..=0x0BFF // Tamil
            | 0x0C00..=0x0C7F // Telugu
            | 0x0C80..=0x0CFF // Kannada
            | 0x0D00..=0x0D7F // Malayalam
            | 0x0E00..=0x0E7F // Thai
            | 0x0E80..=0x0EFF // Lao
            | 0x0F00..=0x0FFF // Tibetan
            | 0x1000..=0x109F // Myanmar
        )
    })
}
