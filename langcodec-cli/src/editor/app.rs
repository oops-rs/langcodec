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
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StatusTone {
    Success,
    Error,
}

pub struct App {
    pub codec: Codec,
    pub file_path: String,
    pub inferred_format: FormatType,
    /// All unique keys across all resources, sorted alphabetically
    pub all_keys: Vec<String>,
    /// Filtered subset of all_keys based on search_query
    pub filtered_keys: Vec<String>,
    /// Languages present, with "en" first if available
    pub languages: Vec<String>,
    /// ratatui list state for the key panel
    pub key_list_state: ListState,
    /// Index of the currently selected language in the translations panel
    pub selected_lang_index: usize,
    pub search_query: String,
    pub edit_buffer: String,
    pub input_mode: InputMode,
    pub dirty: bool,
    pub status_message: Option<(String, StatusTone)>,
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
        // Promote "en" to the front for readability
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
            input_mode: InputMode::Normal,
            dirty: false,
            status_message: None,
        }
    }

    pub fn selected_key(&self) -> Option<&str> {
        self.key_list_state
            .selected()
            .and_then(|i| self.filtered_keys.get(i))
            .map(|s| s.as_str())
    }

    pub fn selected_language(&self) -> Option<&str> {
        self.languages.get(self.selected_lang_index).map(|s| s.as_str())
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
                Translation::Plural(p) => {
                    // Show the "other" form as a summary for plurals
                    p.forms
                        .get(&langcodec::types::PluralCategory::Other)
                        .cloned()
                }
            })
    }

    pub fn apply_filter(&mut self) {
        let query = self.search_query.to_lowercase();
        if query.is_empty() {
            self.filtered_keys = self.all_keys.clone();
        } else {
            self.filtered_keys = self
                .all_keys
                .iter()
                .filter(|k| {
                    if k.to_lowercase().contains(&query) {
                        return true;
                    }
                    // Also search translation values
                    self.languages.iter().any(|lang| {
                        self.get_translation(k, lang)
                            .map(|v| v.to_lowercase().contains(&query))
                            .unwrap_or(false)
                    })
                })
                .cloned()
                .collect();
        }

        // Clamp or reset selection
        let new_len = self.filtered_keys.len();
        if new_len == 0 {
            self.key_list_state.select(None);
        } else {
            let clamped = self.key_list_state.selected().unwrap_or(0).min(new_len - 1);
            self.key_list_state.select(Some(clamped));
        }
    }

    pub fn enter_edit_mode(&mut self) {
        let key = self.selected_key().map(|s| s.to_string());
        let lang = self.selected_language().map(|s| s.to_string());
        if let (Some(key), Some(lang)) = (key, lang) {
            self.edit_buffer = self.get_translation(&key, &lang).unwrap_or_default();
            self.input_mode = InputMode::Edit;
        }
    }

    pub fn commit_edit(&mut self) {
        let key = self.selected_key().map(|s| s.to_string());
        let lang = self.selected_language().map(|s| s.to_string());
        if let (Some(key), Some(lang)) = (key, lang) {
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
                        Some(("Translation updated".to_string(), StatusTone::Success));
                }
                Err(e) => {
                    self.status_message =
                        Some((format!("Error: {}", e), StatusTone::Error));
                }
            }
        }
        self.input_mode = InputMode::Normal;
    }

    pub fn cancel_edit(&mut self) {
        self.edit_buffer.clear();
        self.input_mode = InputMode::Normal;
    }

    pub fn save(&mut self) -> Result<(), String> {
        let resources: Vec<Resource> = self.codec.resources.clone();
        let format = self.inferred_format.clone();
        langcodec::convert_resources_to_format(resources, &self.file_path, format)
            .map_err(|e| format!("Save failed: {}", e))?;
        self.dirty = false;
        self.status_message = Some(("Saved successfully".to_string(), StatusTone::Success));
        Ok(())
    }

    // — Key list navigation —

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
        self.status_message = None;
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
        self.status_message = None;
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
        self.status_message = None;
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
        self.status_message = None;
    }

    pub fn key_jump_top(&mut self) {
        if !self.filtered_keys.is_empty() {
            self.key_list_state.select(Some(0));
            self.status_message = None;
        }
    }

    pub fn key_jump_bottom(&mut self) {
        if !self.filtered_keys.is_empty() {
            self.key_list_state.select(Some(self.filtered_keys.len() - 1));
            self.status_message = None;
        }
    }

    // — Language navigation —

    pub fn lang_next(&mut self) {
        if self.languages.is_empty() {
            return;
        }
        self.selected_lang_index =
            (self.selected_lang_index + 1).min(self.languages.len() - 1);
    }

    pub fn lang_prev(&mut self) {
        self.selected_lang_index = self.selected_lang_index.saturating_sub(1);
    }
}
