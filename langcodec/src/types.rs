//! Core, format-agnostic types for langcodec.
//! Parsers decode into these; encoders serialize these.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
    str::FromStr,
};

use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

use crate::{error::Error, traits::Parser};

// Static regex patterns for HTML tag removal
lazy_static! {
    static ref HTML_TAG_REGEX: Regex = Regex::new(r"<[^>]+>").unwrap();
    static ref HTML_CLOSE_TAG_REGEX: Regex = Regex::new(r"</[^>]+>").unwrap();
}

impl Parser for Vec<Resource> {
    /// Parse from any reader.
    fn from_reader<R: std::io::BufRead>(reader: R) -> Result<Self, Error> {
        serde_json::from_reader(reader).map_err(Error::Parse)
    }

    /// Write to any writer (file, memory, etc.).
    fn to_writer<W: std::io::Write>(&self, mut writer: W) -> Result<(), Error> {
        serde_json::to_writer(&mut writer, self).map_err(Error::Parse)
    }
}

/// A complete localization resource (corresponds to a `.strings`, `.xml`, `.xcstrings`, etc. file).
/// Contains metadata and all entries for a single language and domain.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Resource {
    /// Optional header-level metadata (language code, domain/project, etc.).
    pub metadata: Metadata,

    /// Ordered list of all entries in this resource.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    pub entries: Vec<Entry>,
}

impl Resource {
    /// Add an entry to the resource.
    ///
    /// ```rust
    /// use langcodec::types::{Resource, Entry, Translation, EntryStatus, Metadata};
    /// use std::collections::HashMap;
    ///
    /// let mut resource = Resource {
    ///     metadata: Metadata {
    ///         language: "en".to_string(),
    ///         domain: "test".to_string(),
    ///         custom: HashMap::new(),
    ///     },
    ///     entries: Vec::new(),
    /// };
    /// resource.add_entry(Entry {
    ///     id: "hello".to_string(),
    ///     value: Translation::Singular("Hello".to_string()),
    ///     status: EntryStatus::Translated,
    ///     comment: None,
    ///     custom: HashMap::new(),
    /// });
    /// ```
    ///
    /// # Returns
    ///
    /// The added entry.
    pub fn add_entry(&mut self, entry: Entry) {
        self.entries.push(entry);
    }

    /// Find an entry by its id.
    ///
    /// ```rust
    /// use langcodec::types::{Resource, Entry, Translation, EntryStatus, Metadata};
    /// use std::collections::HashMap;
    ///
    /// let mut resource = Resource {
    ///     metadata: Metadata {
    ///         language: "en".to_string(),
    ///         domain: "test".to_string(),
    ///         custom: HashMap::new(),
    ///     },
    ///     entries: Vec::new(),
    /// };
    /// resource.add_entry(Entry {
    ///     id: "hello".to_string(),
    ///     value: Translation::Singular("Hello".to_string()),
    ///     status: EntryStatus::Translated,
    ///     comment: None,
    ///     custom: HashMap::new(),
    /// });
    /// let entry = resource.find_entry("hello").unwrap();
    /// assert_eq!(entry.value, Translation::Singular("Hello".to_string()));
    /// assert_eq!(entry.status, EntryStatus::Translated);
    /// assert_eq!(entry.comment, None);
    /// ```
    pub fn find_entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Find a mutable entry by its id.
    ///
    /// ```rust
    /// use langcodec::types::{Resource, Entry, Translation, EntryStatus, Metadata};
    /// use std::collections::HashMap;
    ///
    /// let mut resource = Resource {
    ///     metadata: Metadata {
    ///         language: "en".to_string(),
    ///         domain: "test".to_string(),
    ///         custom: HashMap::new(),
    ///     },
    ///     entries: Vec::new(),
    /// };
    /// resource.add_entry(Entry {
    ///     id: "hello".to_string(),
    ///     value: Translation::Singular("Hello".to_string()),
    ///     status: EntryStatus::Translated,
    ///     comment: None,
    ///     custom: HashMap::new(),
    /// });
    /// let entry = resource.find_entry_mut("hello").unwrap();
    /// assert_eq!(entry.value, Translation::Singular("Hello".to_string()));
    /// assert_eq!(entry.status, EntryStatus::Translated);
    /// assert_eq!(entry.comment, None);
    /// entry.value = Translation::Singular("Hello, World!".to_string());
    /// entry.status = EntryStatus::NeedsReview;
    /// entry.comment = Some("Hello, World!".to_string());
    /// assert_eq!(entry.value, Translation::Singular("Hello, World!".to_string()));
    /// assert_eq!(entry.status, EntryStatus::NeedsReview);
    /// assert_eq!(entry.comment, Some("Hello, World!".to_string()));
    /// ```
    pub fn find_entry_mut(&mut self, id: &str) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    pub fn parse_language_identifier(&self) -> Option<LanguageIdentifier> {
        self.metadata.language.replace('_', "-").parse().ok()
    }

    /// Check if this resource has a specific language.
    pub fn has_language(&self, lang: &str) -> bool {
        match (
            self.parse_language_identifier(),
            lang.replace('_', "-").parse::<LanguageIdentifier>(),
        ) {
            (Some(lang_id), Ok(target_lang)) => lang_id.language == target_lang.language,
            _ => false,
        }
    }
}

/// Free-form metadata for the resource as a whole.
///
/// `language` and `domain` are standard; any extra fields can be placed in `custom`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Metadata {
    /// The language code (e.g. "en", "fr", "es", etc.).
    pub language: String,

    /// The domain or project name (e.g. "MyApp").
    #[serde(skip_serializing_if = "String::is_empty")]
    #[serde(default)]
    pub domain: String,

    /// Any other metadata fields not covered by the above.
    pub custom: HashMap<String, String>,
}

impl Display for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut map_all = self.custom.clone();
        map_all.insert("language".to_string(), self.language.clone());
        map_all.insert("domain".to_string(), self.domain.clone());
        write!(
            f,
            "Metadata {{ {} }}",
            map_all
                .iter()
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// A single message/translation entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Entry {
    /// Unique message identifier (key).  
    /// For PO/XLIFF this is `msgid` or `<trans-unit>@id`; for .strings it’s the key.
    pub id: String,

    /// Translation context corresponding to this message.
    pub value: Translation,

    /// Optional comment for translators.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub comment: Option<String>,

    /// Entry translation status.
    pub status: EntryStatus,

    /// Any additional, format-specific data attached to this entry.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    #[serde(default)]
    pub custom: HashMap<String, String>,
}

impl Display for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Entry {{ id: {}, value: {}, status: {:?} }}",
            self.id, self.value, self.status
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Translation {
    /// The translation is empty.
    Empty,

    /// A single translation without plural forms.
    Singular(String),

    /// A translation with plural forms.
    Plural(Plural),
}

impl Translation {
    pub fn plain_translation(translation: Translation) -> Translation {
        match translation {
            Translation::Empty => Translation::Empty,
            Translation::Singular(value) => {
                Translation::Singular(make_plain_translation_string(value))
            }
            Translation::Plural(plural) => {
                // Return the first plural form as a singular translation
                let id = plural.id;
                let forms = plural.forms.into_iter().next().map_or_else(
                    BTreeMap::new,
                    |(category, value)| {
                        let mut map = BTreeMap::new();
                        map.insert(category, make_plain_translation_string(value));
                        map
                    },
                );
                Translation::Plural(Plural { id, forms })
            }
        }
    }

    pub fn plain_translation_string(&self) -> String {
        match self {
            Translation::Empty => String::new(),
            Translation::Singular(value) => make_plain_translation_string(value.clone()),
            Translation::Plural(plural) => {
                // Return the plural ID, not the first form
                plural.id.clone()
            }
        }
    }
}

impl Display for Translation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Translation::Empty => write!(f, "Empty"),
            Translation::Singular(value) => write!(f, "{}", value),
            Translation::Plural(plural) => write!(f, "{}", plural.id), // Displaying only the ID for brevity
        }
    }
}

/// All plural forms for a single message.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Plural {
    /// The canonical plural ID (`msgid_plural` in PO).
    pub id: String,

    /// Map from category → translation.  
    /// Categories depend on the target locale’s rules.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(default)]
    pub forms: BTreeMap<PluralCategory, String>,
}

impl Plural {
    pub(crate) fn new(
        id: &str,
        forms: impl Iterator<Item = (PluralCategory, String)>,
    ) -> Option<Self> {
        let forms: BTreeMap<PluralCategory, String> = forms.collect();

        if forms.is_empty() {
            None // No plural forms provided
        } else {
            Some(Self {
                id: id.to_string(),
                forms,
            })
        }
    }
}

/// Standard CLDR plural forms.
#[derive(Ord, PartialOrd, Eq, PartialEq, Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Hash)]
pub enum PluralCategory {
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

impl FromStr for PluralCategory {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "ZERO" => Ok(PluralCategory::Zero),
            "ONE" => Ok(PluralCategory::One),
            "TWO" => Ok(PluralCategory::Two),
            "FEW" => Ok(PluralCategory::Few),
            "MANY" => Ok(PluralCategory::Many),
            "OTHER" => Ok(PluralCategory::Other),
            _ => Err(format!("Unknown plural category: {}", s)),
        }
    }
}

/// Status of a translation entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    /// The entry is not translated and should not be.
    DoNotTranslate,

    /// The entry is new and has not been translated yet.
    New,

    /// The entry is outdated.
    Stale,

    /// The entry has been modified and needs review.
    NeedsReview,

    /// The entry is translated and reviewed.
    Translated,
}

impl FromStr for EntryStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "DO_NOT_TRANSLATE" => Ok(EntryStatus::DoNotTranslate),
            "NEW" => Ok(EntryStatus::New),
            "STALE" => Ok(EntryStatus::Stale),
            "NEEDS_REVIEW" => Ok(EntryStatus::NeedsReview),
            "TRANSLATED" => Ok(EntryStatus::Translated),
            _ => Err(format!("Unknown entry status: {}", s)),
        }
    }
}

/// Strategy for handling conflicts when merging resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Keep the first occurrence of a key
    First,
    /// Keep the last occurrence of a key (default)
    Last,
    /// Skip conflicting entries
    Skip,
}

// Remove HTML tags from translation string.
fn make_plain_translation_string(translation: String) -> String {
    let mut translation = translation;
    translation = translation.trim().to_string();

    // Remove all HTML tags (non-greedy)
    translation = HTML_TAG_REGEX.replace_all(&translation, "").to_string();

    // Remove all closing tags like </font>
    translation = HTML_CLOSE_TAG_REGEX
        .replace_all(&translation, "")
        .to_string();

    // Replace any newline characters with explicit "\n" for better formatting,
    translation = translation
        .lines()
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join(r"\n"); // Use r"\n" for a literal \n

    translation
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_resource_add_entry() {
        let mut resource = Resource {
            metadata: Metadata {
                language: "en".to_string(),
                domain: "test".to_string(),
                custom: HashMap::new(),
            },
            entries: Vec::new(),
        };

        let entry = Entry {
            id: "hello".to_string(),
            value: Translation::Singular("Hello".to_string()),
            comment: None,
            status: EntryStatus::Translated,
            custom: HashMap::new(),
        };

        resource.add_entry(entry);
        assert_eq!(resource.entries.len(), 1);
        assert_eq!(resource.entries[0].id, "hello");
    }

    #[test]
    fn test_resource_parse_language_identifier() {
        let resource = Resource {
            metadata: Metadata {
                language: "en-US".to_string(),
                domain: "test".to_string(),
                custom: HashMap::new(),
            },
            entries: Vec::new(),
        };

        let lang_id = resource.parse_language_identifier().unwrap();
        assert_eq!(lang_id.language.as_str(), "en");
        assert_eq!(lang_id.region.unwrap().as_str(), "US");
    }

    #[test]
    fn resource_language_identifier_normalizes_underscore_and_case() {
        let resource = Resource {
            metadata: Metadata {
                language: "PT_br".to_string(),
                domain: "test".to_string(),
                custom: HashMap::new(),
            },
            entries: Vec::new(),
        };

        assert_eq!(
            resource.parse_language_identifier().unwrap().to_string(),
            "pt-BR"
        );
        assert!(resource.has_language("pt_BR"));
    }

    #[test]
    fn test_resource_parse_invalid_language() {
        let resource = Resource {
            metadata: Metadata {
                language: "not-a-language".to_string(),
                domain: "test".to_string(),
                custom: HashMap::new(),
            },
            entries: Vec::new(),
        };

        // This should fail because "not-a-language" is not a valid BCP 47 language identifier
        assert!(resource.parse_language_identifier().is_none());
    }

    #[test]
    fn test_resource_has_language() {
        let resource = Resource {
            metadata: Metadata {
                language: "en-US".to_string(),
                domain: "test".to_string(),
                custom: HashMap::new(),
            },
            entries: Vec::new(),
        };

        assert!(resource.has_language("en"));
        assert!(resource.has_language("en-US"));
        assert!(!resource.has_language("fr"));
    }

    #[test]
    fn test_metadata_display() {
        let mut metadata = Metadata {
            language: "en".to_string(),
            domain: "test".to_string(),
            custom: HashMap::new(),
        };
        metadata
            .custom
            .insert("version".to_string(), "1.0".to_string());

        let display = format!("{}", metadata);
        assert!(display.contains("language: en"));
        assert!(display.contains("domain: test"));
        assert!(display.contains("version: 1.0"));
    }

    #[test]
    fn test_entry_display() {
        let entry = Entry {
            id: "hello".to_string(),
            value: Translation::Singular("Hello".to_string()),
            comment: Some("Greeting".to_string()),
            status: EntryStatus::Translated,
            custom: HashMap::new(),
        };

        let display = format!("{}", entry);
        assert!(display.contains("hello"));
        assert!(display.contains("Hello"));
        // The display format might not include comments, so we'll just check the basic structure
        assert!(!display.is_empty());
    }

    #[test]
    fn test_translation_plain_translation() {
        let singular = Translation::Singular("Hello".to_string());
        let plain = Translation::plain_translation(singular);
        assert!(matches!(plain, Translation::Singular(_)));
    }

    #[test]
    fn test_translation_plain_translation_string() {
        let singular = Translation::Singular("Hello".to_string());
        assert_eq!(singular.plain_translation_string(), "Hello");

        let plural = Translation::Plural(
            Plural::new(
                "apples",
                vec![
                    (PluralCategory::One, "1 apple".to_string()),
                    (PluralCategory::Other, "%d apples".to_string()),
                ]
                .into_iter(),
            )
            .unwrap(),
        );
        // For plural translations, we return the plural ID, not the first form
        assert_eq!(plural.plain_translation_string(), "apples");
    }

    #[test]
    fn test_translation_display() {
        let singular = Translation::Singular("Hello".to_string());
        assert_eq!(format!("{}", singular), "Hello");

        let plural = Translation::Plural(
            Plural::new(
                "apples",
                vec![
                    (PluralCategory::One, "1 apple".to_string()),
                    (PluralCategory::Other, "%d apples".to_string()),
                ]
                .into_iter(),
            )
            .unwrap(),
        );
        assert!(format!("{}", plural).contains("apples"));
    }

    #[test]
    fn test_plural_new() {
        let forms = vec![
            (PluralCategory::One, "1 apple".to_string()),
            (PluralCategory::Other, "%d apples".to_string()),
        ];

        let plural = Plural::new("apples", forms.into_iter()).unwrap();
        assert_eq!(plural.id, "apples");
        assert_eq!(plural.forms.len(), 2);
        assert_eq!(plural.forms.get(&PluralCategory::One).unwrap(), "1 apple");
        assert_eq!(
            plural.forms.get(&PluralCategory::Other).unwrap(),
            "%d apples"
        );
    }

    #[test]
    fn test_plural_new_empty() {
        let forms: Vec<(PluralCategory, String)> = vec![];
        let plural = Plural::new("apples", forms.into_iter());
        assert!(plural.is_none());
    }

    #[test]
    fn test_plural_category_from_str() {
        assert_eq!(
            PluralCategory::from_str("zero").unwrap(),
            PluralCategory::Zero
        );
        assert_eq!(
            PluralCategory::from_str("one").unwrap(),
            PluralCategory::One
        );
        assert_eq!(
            PluralCategory::from_str("two").unwrap(),
            PluralCategory::Two
        );
        assert_eq!(
            PluralCategory::from_str("few").unwrap(),
            PluralCategory::Few
        );
        assert_eq!(
            PluralCategory::from_str("many").unwrap(),
            PluralCategory::Many
        );
        assert_eq!(
            PluralCategory::from_str("other").unwrap(),
            PluralCategory::Other
        );
    }

    #[test]
    fn test_plural_category_from_str_invalid() {
        assert!(PluralCategory::from_str("invalid").is_err());
    }

    #[test]
    fn test_entry_status_from_str() {
        assert_eq!(
            EntryStatus::from_str("do_not_translate").unwrap(),
            EntryStatus::DoNotTranslate
        );
        assert_eq!(EntryStatus::from_str("new").unwrap(), EntryStatus::New);
        assert_eq!(EntryStatus::from_str("stale").unwrap(), EntryStatus::Stale);
        assert_eq!(
            EntryStatus::from_str("needs_review").unwrap(),
            EntryStatus::NeedsReview
        );
        assert_eq!(
            EntryStatus::from_str("translated").unwrap(),
            EntryStatus::Translated
        );
    }

    #[test]
    fn test_entry_status_from_str_invalid() {
        assert!(EntryStatus::from_str("invalid").is_err());
    }

    #[test]
    fn test_make_plain_translation_string() {
        let result = make_plain_translation_string("Hello".to_string());
        assert_eq!(result, "Hello");

        let result = make_plain_translation_string("Hello\nWorld".to_string());
        assert_eq!(result, "Hello\\nWorld");
    }

    #[test]
    fn test_resource_parser_trait() {
        let resources = vec![Resource {
            metadata: Metadata {
                language: "en".to_string(),
                domain: "test".to_string(),
                custom: HashMap::new(),
            },
            entries: vec![],
        }];

        let mut writer = Vec::new();
        resources.to_writer(&mut writer).unwrap();

        let reader = std::io::Cursor::new(writer);
        let parsed: Vec<Resource> = Vec::<Resource>::from_reader(reader).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].metadata.language, "en");
    }
}
