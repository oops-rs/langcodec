use crate::{
    Codec, Error,
    formats::stringsdict::{
        LOCALIZED_FORMAT_CUSTOM_KEY, VALUE_TYPE_CUSTOM_KEY, VARIABLE_NAME_CUSTOM_KEY,
    },
    types::Entry,
};
use std::collections::HashMap;

const STRINGSDICT_STRUCTURAL_KEYS: [&str; 3] = [
    LOCALIZED_FORMAT_CUSTOM_KEY,
    VARIABLE_NAME_CUSTOM_KEY,
    VALUE_TYPE_CUSTOM_KEY,
];

#[derive(Debug, Clone)]
pub struct NormalizeOptions {
    pub normalize_placeholders: bool,
    pub key_style: KeyStyle,
}

impl Default for NormalizeOptions {
    fn default() -> Self {
        Self {
            normalize_placeholders: true,
            key_style: KeyStyle::None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KeyStyle {
    #[default]
    None,
    Snake,
    Kebab,
    Camel,
}

#[derive(Debug, Clone, Default)]
pub struct NormalizeReport {
    pub changed: bool,
}

pub fn normalize_codec(
    codec: &mut Codec,
    options: &NormalizeOptions,
) -> Result<NormalizeReport, Error> {
    reject_unsafe_stringsdict_normalization(codec, options)?;

    let mut normalized = codec.clone();
    let report = normalize_codec_in_place(&mut normalized, options)?;
    *codec = normalized;
    Ok(report)
}

pub(crate) fn has_stringsdict_structural_metadata(entry: &Entry) -> bool {
    STRINGSDICT_STRUCTURAL_KEYS
        .iter()
        .any(|key| entry.custom.contains_key(*key))
}

fn reject_unsafe_stringsdict_normalization(
    codec: &Codec,
    options: &NormalizeOptions,
) -> Result<(), Error> {
    let rewrites_placeholders = options.normalize_placeholders;
    let rewrites_keys = options.key_style != KeyStyle::None;
    if !rewrites_placeholders && !rewrites_keys {
        return Ok(());
    }

    for resource in &codec.resources {
        for entry in &resource.entries {
            if !has_stringsdict_structural_metadata(entry) {
                continue;
            }

            let present = STRINGSDICT_STRUCTURAL_KEYS
                .iter()
                .copied()
                .filter(|key| entry.custom.contains_key(*key))
                .collect::<Vec<_>>();
            if present.len() != STRINGSDICT_STRUCTURAL_KEYS.len() {
                let missing = STRINGSDICT_STRUCTURAL_KEYS
                    .iter()
                    .copied()
                    .filter(|key| !entry.custom.contains_key(*key))
                    .collect::<Vec<_>>();
                return Err(Error::DataMismatch(format!(
                    "entry '{}' in language '{}' (domain '{}') has partial .stringsdict selector metadata (present: {}; missing: {}). Supply all three structural keys or remove the partial metadata after converting away from .stringsdict before normalizing placeholders or keys",
                    entry.id,
                    resource.metadata.language,
                    resource.metadata.domain,
                    present.join(", "),
                    missing.join(", "),
                )));
            }

            let mut requested = Vec::new();
            if rewrites_placeholders {
                requested.push("placeholder");
            }
            if rewrites_keys {
                requested.push("key");
            }
            return Err(Error::UnsupportedFormat(format!(
                "{} normalization is not supported for .stringsdict selector entry '{}' in language '{}' (domain '{}') because its structural metadata must remain aligned with its key and printf value type. Disable placeholder normalization and use KeyStyle::None, or convert away from .stringsdict before normalizing",
                requested.join(" and "),
                entry.id,
                resource.metadata.language,
                resource.metadata.domain,
            )));
        }
    }

    Ok(())
}

fn normalize_codec_in_place(
    codec: &mut Codec,
    options: &NormalizeOptions,
) -> Result<NormalizeReport, Error> {
    let mut changed = false;

    for resource in &mut codec.resources {
        if options.key_style != KeyStyle::None {
            let mut transformed_ids = Vec::with_capacity(resource.entries.len());
            let mut seen: HashMap<String, String> = HashMap::new();

            for entry in &resource.entries {
                let transformed = transform_key_style(&entry.id, options.key_style);
                if let Some(existing) = seen.get(&transformed) {
                    return Err(Error::validation_error(format!(
                        "key-style collision in language '{}' (domain '{}'): '{}' and '{}' both normalize to '{}'",
                        resource.metadata.language,
                        resource.metadata.domain,
                        existing,
                        entry.id,
                        transformed
                    )));
                }
                seen.insert(transformed.clone(), entry.id.clone());
                transformed_ids.push(transformed);
            }

            for (entry, transformed_id) in resource.entries.iter_mut().zip(transformed_ids) {
                if entry.id != transformed_id {
                    entry.id = transformed_id;
                    changed = true;
                }
            }
        }

        if options.normalize_placeholders {
            for entry in &mut resource.entries {
                if normalize_entry_placeholders(entry) {
                    changed = true;
                }
            }
        }

        let already_sorted = resource
            .entries
            .windows(2)
            .all(|pair| pair[0].id <= pair[1].id);
        if !already_sorted {
            resource
                .entries
                .sort_by(|left, right| left.id.cmp(&right.id));
            changed = true;
        }
    }

    Ok(NormalizeReport { changed })
}

fn normalize_entry_placeholders(entry: &mut crate::types::Entry) -> bool {
    use crate::placeholder::normalize_placeholders;
    use crate::types::Translation;

    match &mut entry.value {
        Translation::Empty => false,
        Translation::Singular(value) => {
            let normalized = normalize_placeholders(value);
            if *value != normalized {
                *value = normalized;
                true
            } else {
                false
            }
        }
        Translation::Plural(plural) => {
            let mut changed = false;
            for value in plural.forms.values_mut() {
                let normalized = normalize_placeholders(value);
                if *value != normalized {
                    *value = normalized;
                    changed = true;
                }
            }
            changed
        }
    }
}

fn transform_key_style(input: &str, key_style: KeyStyle) -> String {
    match key_style {
        KeyStyle::None => input.to_string(),
        KeyStyle::Snake => join_words(input, "_"),
        KeyStyle::Kebab => join_words(input, "-"),
        KeyStyle::Camel => camel_case(input),
    }
}

fn join_words(input: &str, separator: &str) -> String {
    let words = split_words(input);
    if words.is_empty() {
        return input.to_string();
    }
    words.join(separator)
}

fn camel_case(input: &str) -> String {
    let words = split_words(input);
    if words.is_empty() {
        return input.to_string();
    }

    let mut out = String::new();
    out.push_str(&words[0]);
    for word in words.iter().skip(1) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.extend(chars);
        }
    }
    out
}

fn lowercase_word(input: &str) -> String {
    input.chars().flat_map(|ch| ch.to_lowercase()).collect()
}

fn split_words(input: &str) -> Vec<String> {
    let chars: Vec<char> = input.chars().collect();
    let mut words = Vec::new();
    let mut current = String::new();

    for (idx, ch) in chars.iter().enumerate() {
        if !ch.is_alphanumeric() {
            if !current.is_empty() {
                words.push(lowercase_word(&current));
                current.clear();
            }
            continue;
        }

        let should_split = if current.is_empty() {
            false
        } else {
            let is_upper = ch.is_uppercase();
            let prev = chars[idx - 1];
            let prev_is_lower_or_digit = prev.is_lowercase() || prev.is_numeric();
            let prev_is_upper = prev.is_uppercase();
            let next_is_lower = chars
                .get(idx + 1)
                .map(|next| next.is_lowercase())
                .unwrap_or(false);

            is_upper && (prev_is_lower_or_digit || (prev_is_upper && next_is_lower))
        };

        if should_split {
            words.push(lowercase_word(&current));
            current.clear();
        }
        current.push(*ch);
    }

    if !current.is_empty() {
        words.push(lowercase_word(&current));
    }

    words
}
