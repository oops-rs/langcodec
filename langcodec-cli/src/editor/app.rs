use std::collections::HashSet;

use langcodec::{
    Codec, FormatType, Resource, Translation,
    types::EntryStatus,
};
use ratatui::widgets::ListState;

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Search,
    Edit,
    ConfirmQuit,
    /// Full-screen help overlay — any key dismisses it
    Help,
    /// Waiting for y/n to confirm deleting the selected key
    ConfirmDelete,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StatusTone {
    Success,
    Error,
}

/// Which subset of keys to show in the list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterMode {
    /// Show all keys.
    All,
    /// Show only keys that have at least one missing translation for any language.
    Missing,
}

/// A single saved undo step (one-level undo).
#[derive(Debug, Clone)]
pub struct UndoEntry {
    pub key: String,
    pub lang: String,
    /// `None` means the translation was absent before the edit.
    pub old_value: Option<String>,
}

pub struct App {
    pub codec: Codec,
    pub file_path: String,
    pub inferred_format: FormatType,
    /// All unique keys across all resources, sorted alphabetically
    pub all_keys: Vec<String>,
    /// Filtered/searched subset currently visible in the key panel
    pub filtered_keys: Vec<String>,
    /// Languages present, with "en" first if available
    pub languages: Vec<String>,
    /// ratatui list state for the key panel
    pub key_list_state: ListState,
    /// Index of the currently highlighted language in the translations panel
    pub selected_lang_index: usize,
    pub search_query: String,
    /// Text being typed in edit mode
    pub edit_buffer: String,
    /// Cursor position (char index) inside `edit_buffer`
    pub cursor_pos: usize,
    pub input_mode: InputMode,
    pub filter_mode: FilterMode,
    pub dirty: bool,
    pub status_message: Option<(String, StatusTone)>,
    /// One-level undo for the last committed translation edit
    pub undo_entry: Option<UndoEntry>,
    /// Whether the help overlay is visible
    pub show_help: bool,
    /// Key-panel percentage width (default 38); adjusted with < / >
    pub split_ratio: u8,
    /// Vertical scroll offset inside the translations panel (for the selected entry)
    pub translation_scroll: u16,
    /// Set to true while waiting for delete confirmation
    pub confirm_delete: bool,
    /// Toggled on every key navigation; used by the renderer to force
    /// ratatui's diff to re-send all cells in the translations panel,
    /// eliminating ghost glyphs from complex-script characters.
    pub redraw_token: bool,
}

impl App {
    pub fn new(codec: Codec, file_path: String, inferred_format: FormatType) -> Self {
        let mut key_set: HashSet<String> = HashSet::new();
        for resource in &codec.resources {
            for entry in &resource.entries {
                key_set.insert(entry.id.clone());
            }
        }
        let mut all_keys: Vec<String> = key_set.into_iter().collect();
        all_keys.sort();

        let mut languages: Vec<String> = codec
            .resources
            .iter()
            .map(|r| r.metadata.language.clone())
            .filter(|l| !l.is_empty())
            .collect();
        languages.sort();
        if let Some(pos) = languages.iter().position(|l| l == "en") {
            languages.remove(pos);
            languages.insert(0, "en".to_string());
        }

        let filtered_keys = all_keys.clone();

        let mut key_list_state = ListState::default();
        if !filtered_keys.is_empty() {
            key_list_state.select(Some(0));
        }

        Self {
            codec,
            file_path,
            inferred_format,
            all_keys,
            filtered_keys,
            languages,
            key_list_state,
            selected_lang_index: 0,
            search_query: String::new(),
            edit_buffer: String::new(),
            cursor_pos: 0,
            input_mode: InputMode::Normal,
            filter_mode: FilterMode::All,
            dirty: false,
            status_message: None,
            undo_entry: None,
            show_help: false,
            split_ratio: 38,
            translation_scroll: 0,
            confirm_delete: false,
            redraw_token: false,
        }
    }

    // ── Query helpers ────────────────────────────────────────────────────────

    pub fn selected_key(&self) -> Option<&str> {
        self.key_list_state
            .selected()
            .and_then(|i| self.filtered_keys.get(i))
            .map(|s| s.as_str())
    }

    pub fn selected_language(&self) -> Option<&str> {
        self.languages
            .get(self.selected_lang_index)
            .map(|s| s.as_str())
    }

    pub fn get_translation(&self, key: &str, lang: &str) -> Option<String> {
        self.codec
            .get_by_language(lang)?
            .entries
            .iter()
            .find(|e| e.id == key)
            .and_then(|e| match &e.value {
                Translation::Singular(s) => Some(s.clone()),
                Translation::Empty => None,
                Translation::Plural(p) => p
                    .forms
                    .get(&langcodec::types::PluralCategory::Other)
                    .cloned(),
            })
    }

    /// True if `key` is missing a translation for at least one language.
    pub fn has_missing(&self, key: &str) -> bool {
        self.languages
            .iter()
            .any(|lang| self.get_translation(key, lang).is_none())
    }

    // ── Filter ───────────────────────────────────────────────────────────────

    pub fn apply_filter(&mut self) {
        let query = self.search_query.to_lowercase();
        self.filtered_keys = self
            .all_keys
            .iter()
            .filter(|k| {
                // FilterMode::Missing hides fully-translated keys
                if self.filter_mode == FilterMode::Missing && !self.has_missing(k) {
                    return false;
                }
                if query.is_empty() {
                    return true;
                }
                if k.to_lowercase().contains(&query) {
                    return true;
                }
                self.languages.iter().any(|lang| {
                    self.get_translation(k, lang)
                        .map(|v| v.to_lowercase().contains(&query))
                        .unwrap_or(false)
                })
            })
            .cloned()
            .collect();

        let new_len = self.filtered_keys.len();
        if new_len == 0 {
            self.key_list_state.select(None);
        } else {
            let clamped = self.key_list_state.selected().unwrap_or(0).min(new_len - 1);
            self.key_list_state.select(Some(clamped));
        }
    }

    pub fn cycle_filter_mode(&mut self) {
        self.filter_mode = match self.filter_mode {
            FilterMode::All => FilterMode::Missing,
            FilterMode::Missing => FilterMode::All,
        };
        self.apply_filter();
        let label = match self.filter_mode {
            FilterMode::All => "Filter: all keys",
            FilterMode::Missing => "Filter: missing translations only",
        };
        self.status_message = Some((label.to_string(), StatusTone::Success));
    }

    // ── Edit mode ────────────────────────────────────────────────────────────

    pub fn enter_edit_mode(&mut self) {
        let key = self.selected_key().map(|s| s.to_string());
        let lang = self.selected_language().map(|s| s.to_string());
        if let (Some(key), Some(lang)) = (key, lang) {
            self.edit_buffer = self.get_translation(&key, &lang).unwrap_or_default();
            // Place cursor at end
            self.cursor_pos = self.edit_buffer.chars().count();
            self.input_mode = InputMode::Edit;
        }
    }

    pub fn commit_edit(&mut self) {
        let key = self.selected_key().map(|s| s.to_string());
        let lang = self.selected_language().map(|s| s.to_string());
        if let (Some(key), Some(lang)) = (key, lang) {
            // Save undo snapshot before overwriting
            self.undo_entry = Some(UndoEntry {
                key: key.clone(),
                lang: lang.clone(),
                old_value: self.get_translation(&key, &lang),
            });

            let value = self.edit_buffer.clone();
            let translation = Translation::Singular(value);
            let result = if self.codec.has_entry(&key, &lang) {
                self.codec.update_translation(&key, &lang, translation, None)
            } else {
                self.codec
                    .add_entry(&key, &lang, translation, None, Some(EntryStatus::Translated))
            };
            match result {
                Ok(()) => {
                    self.dirty = true;
                    self.status_message =
                        Some(("Translation updated  [u] to undo".to_string(), StatusTone::Success));
                }
                Err(e) => {
                    self.status_message = Some((format!("Error: {e}"), StatusTone::Error));
                }
            }
        }
        self.edit_buffer.clear();
        self.cursor_pos = 0;
        self.input_mode = InputMode::Normal;
    }

    pub fn cancel_edit(&mut self) {
        self.edit_buffer.clear();
        self.cursor_pos = 0;
        self.input_mode = InputMode::Normal;
    }

    /// Copy the source-language translation into the edit buffer (Ctrl+Y in edit mode).
    pub fn copy_from_source_lang(&mut self) {
        let key = self.selected_key().map(|s| s.to_string());
        if let Some(key) = key {
            let source = self.languages.first().cloned().unwrap_or_default();
            if let Some(v) = self.get_translation(&key, &source) {
                self.edit_buffer = v;
                self.cursor_pos = self.edit_buffer.chars().count();
            }
        }
    }

    // ── Cursor movement inside edit buffer ───────────────────────────────────

    pub fn cursor_move_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
        }
    }

    pub fn cursor_move_right(&mut self) {
        let len = self.edit_buffer.chars().count();
        if self.cursor_pos < len {
            self.cursor_pos += 1;
        }
    }

    pub fn cursor_home(&mut self) {
        self.cursor_pos = 0;
    }

    pub fn cursor_end(&mut self) {
        self.cursor_pos = self.edit_buffer.chars().count();
    }

    /// Insert a character at the cursor position.
    pub fn edit_insert(&mut self, ch: char) {
        let byte_pos = char_to_byte_index(&self.edit_buffer, self.cursor_pos);
        self.edit_buffer.insert(byte_pos, ch);
        self.cursor_pos += 1;
    }

    /// Delete the character before the cursor (backspace).
    pub fn edit_backspace(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let byte_pos = char_to_byte_index(&self.edit_buffer, self.cursor_pos);
        // Step back one char boundary
        let prev = self.edit_buffer[..byte_pos]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.edit_buffer.drain(prev..byte_pos);
        self.cursor_pos -= 1;
    }

    /// Delete the character at the cursor position (delete key).
    pub fn edit_delete_forward(&mut self) {
        let len = self.edit_buffer.chars().count();
        if self.cursor_pos >= len {
            return;
        }
        let byte_pos = char_to_byte_index(&self.edit_buffer, self.cursor_pos);
        let next = self.edit_buffer[byte_pos..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| byte_pos + i)
            .unwrap_or(self.edit_buffer.len());
        self.edit_buffer.drain(byte_pos..next);
    }

    // ── Undo ─────────────────────────────────────────────────────────────────

    pub fn undo(&mut self) {
        let Some(entry) = self.undo_entry.take() else {
            self.status_message = Some(("Nothing to undo".to_string(), StatusTone::Error));
            return;
        };
        let result = match &entry.old_value {
            Some(v) => self.codec.update_translation(
                &entry.key,
                &entry.lang,
                Translation::Singular(v.clone()),
                None,
            ),
            None => self.codec.remove_entry(&entry.key, &entry.lang),
        };
        match result {
            Ok(()) => {
                self.dirty = true;
                self.status_message =
                    Some(("Undo applied".to_string(), StatusTone::Success));
            }
            Err(e) => {
                self.status_message = Some((format!("Undo failed: {e}"), StatusTone::Error));
            }
        }
    }

    // ── Delete key ───────────────────────────────────────────────────────────

    pub fn confirm_delete_key(&mut self) {
        let Some(key) = self.selected_key().map(|s| s.to_string()) else {
            return;
        };
        let mut had_error = false;
        for lang in self.languages.clone() {
            if !self.codec.has_entry(&key, &lang) {
                continue;
            }
            if let Err(e) = self.codec.remove_entry(&key, &lang) {
                self.status_message = Some((format!("Delete failed: {e}"), StatusTone::Error));
                had_error = true;
                break;
            }
        }
        if !had_error {
            // Remove from key lists
            self.all_keys.retain(|k| k != &key);
            self.apply_filter();
            self.dirty = true;
            self.status_message =
                Some((format!("Deleted key '{key}'"), StatusTone::Success));
            self.redraw_token = !self.redraw_token;
        }
        self.confirm_delete = false;
        self.input_mode = InputMode::Normal;
    }

    // ── Missing-translation navigation ───────────────────────────────────────

    /// Jump to the next key (wrapping) that has a missing translation for any language.
    pub fn next_missing(&mut self) {
        let start = self.key_list_state.selected().unwrap_or(0);
        let len = self.filtered_keys.len();
        if len == 0 {
            return;
        }
        for delta in 1..=len {
            let idx = (start + delta) % len;
            if self.has_missing(&self.filtered_keys[idx].clone()) {
                self.key_list_state.select(Some(idx));
                self.translation_scroll = 0;
                self.status_message = None;
                self.redraw_token = !self.redraw_token;
                return;
            }
        }
        self.status_message = Some(("No missing translations found".to_string(), StatusTone::Error));
    }

    /// Jump to the previous key (wrapping) that has a missing translation.
    pub fn prev_missing(&mut self) {
        let start = self.key_list_state.selected().unwrap_or(0);
        let len = self.filtered_keys.len();
        if len == 0 {
            return;
        }
        for delta in 1..=len {
            let idx = (start + len - delta) % len;
            if self.has_missing(&self.filtered_keys[idx].clone()) {
                self.key_list_state.select(Some(idx));
                self.translation_scroll = 0;
                self.status_message = None;
                self.redraw_token = !self.redraw_token;
                return;
            }
        }
        self.status_message = Some(("No missing translations found".to_string(), StatusTone::Error));
    }

    // ── Save ─────────────────────────────────────────────────────────────────

    pub fn save(&mut self) -> Result<(), String> {
        let resources: Vec<Resource> = self.codec.resources.clone();
        let format = self.inferred_format.clone();
        langcodec::convert_resources_to_format(resources, &self.file_path, format)
            .map_err(|e| format!("Save failed: {e}"))?;
        self.dirty = false;
        self.status_message = Some(("Saved successfully".to_string(), StatusTone::Success));
        Ok(())
    }

    // ── Key list navigation ───────────────────────────────────────────────────

    pub fn key_next(&mut self) {
        if self.filtered_keys.is_empty() {
            return;
        }
        let len = self.filtered_keys.len();
        let next = self
            .key_list_state
            .selected()
            .map(|i| (i + 1).min(len - 1))
            .unwrap_or(0);
        self.key_list_state.select(Some(next));
        self.translation_scroll = 0;
        self.status_message = None;
        self.redraw_token = !self.redraw_token;
    }

    pub fn key_prev(&mut self) {
        if self.filtered_keys.is_empty() {
            return;
        }
        let prev = self
            .key_list_state
            .selected()
            .map(|i| i.saturating_sub(1))
            .unwrap_or(0);
        self.key_list_state.select(Some(prev));
        self.translation_scroll = 0;
        self.status_message = None;
        self.redraw_token = !self.redraw_token;
    }

    pub fn key_next_page(&mut self, page_size: usize) {
        if self.filtered_keys.is_empty() {
            return;
        }
        let len = self.filtered_keys.len();
        let next = self
            .key_list_state
            .selected()
            .map(|i| (i + page_size).min(len - 1))
            .unwrap_or(0);
        self.key_list_state.select(Some(next));
        self.translation_scroll = 0;
        self.status_message = None;
        self.redraw_token = !self.redraw_token;
    }

    pub fn key_prev_page(&mut self, page_size: usize) {
        if self.filtered_keys.is_empty() {
            return;
        }
        let prev = self
            .key_list_state
            .selected()
            .map(|i| i.saturating_sub(page_size))
            .unwrap_or(0);
        self.key_list_state.select(Some(prev));
        self.translation_scroll = 0;
        self.status_message = None;
        self.redraw_token = !self.redraw_token;
    }

    pub fn key_jump_top(&mut self) {
        if !self.filtered_keys.is_empty() {
            self.key_list_state.select(Some(0));
            self.translation_scroll = 0;
            self.status_message = None;
            self.redraw_token = !self.redraw_token;
        }
    }

    pub fn key_jump_bottom(&mut self) {
        if !self.filtered_keys.is_empty() {
            self.key_list_state
                .select(Some(self.filtered_keys.len() - 1));
            self.translation_scroll = 0;
            self.status_message = None;
            self.redraw_token = !self.redraw_token;
        }
    }

    // ── Language navigation ───────────────────────────────────────────────────

    pub fn lang_next(&mut self) {
        if self.languages.is_empty() {
            return;
        }
        self.selected_lang_index =
            (self.selected_lang_index + 1).min(self.languages.len() - 1);
        self.translation_scroll = 0;
    }

    pub fn lang_prev(&mut self) {
        if self.selected_lang_index > 0 {
            self.selected_lang_index -= 1;
            self.translation_scroll = 0;
        }
    }

    // ── Panel resize ─────────────────────────────────────────────────────────

    pub fn split_wider(&mut self) {
        self.split_ratio = (self.split_ratio + 2).min(70);
    }

    pub fn split_narrower(&mut self) {
        self.split_ratio = self.split_ratio.saturating_sub(2).max(20);
    }

    // ── Translation scroll ───────────────────────────────────────────────────

    pub fn translation_scroll_down(&mut self) {
        self.translation_scroll = self.translation_scroll.saturating_add(1);
    }

    pub fn translation_scroll_up(&mut self) {
        self.translation_scroll = self.translation_scroll.saturating_sub(1);
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Convert a character index into a UTF-8 byte offset in `s`.
fn char_to_byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}
