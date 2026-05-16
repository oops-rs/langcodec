use serde::{ser::SerializeMap, Deserialize, Serialize, Serializer};
use std::{
    collections::{BTreeMap, HashMap},
    io::{BufRead, Write},
    str::FromStr,
};

use crate::{
    error::Error,
    traits::Parser,
    types::{Entry, EntryStatus, Metadata, Plural, PluralCategory, Resource, Translation},
};

fn serialize_sorted_map<S, V>(map: &HashMap<String, V>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    V: Serialize,
{
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_unstable();

    let mut out = serializer.serialize_map(Some(map.len()))?;
    for k in keys {
        out.serialize_entry(k, &map[k])?;
    }
    out.end()
}

fn serialize_sorted_plural_map<S>(
    map: &HashMap<PluralCategory, PluralVariation>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut keys: Vec<&PluralCategory> = map.keys().collect();
    keys.sort_unstable();

    let mut out = serializer.serialize_map(Some(map.len()))?;
    for k in keys {
        out.serialize_entry(k, &map[k])?;
    }
    out.end()
}

fn serialize_optional_sorted_plural_map<S>(
    map: &Option<HashMap<PluralCategory, PluralVariation>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match map {
        Some(map) => serialize_sorted_plural_map(map, serializer),
        None => serializer.serialize_none(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Format {
    pub source_language: String,
    #[serde(serialize_with = "serialize_sorted_map")]
    pub strings: HashMap<String, Item>,
    pub version: String,
}

impl Parser for Format {
    /// Parses the xcstrings format from a reader.
    fn from_reader<R: BufRead>(reader: R) -> Result<Self, Error> {
        serde_json::from_reader(reader).map_err(Error::Parse)
    }

    /// Serializes the xcstrings format to a writer.
    fn to_writer<W: Write>(&self, writer: W) -> Result<(), Error> {
        serde_json::to_writer_pretty(writer, &self).map_err(Error::Parse)
    }
}

impl TryFrom<Vec<Resource>> for Format {
    type Error = Error;

    fn try_from(mut resources: Vec<Resource>) -> Result<Self, Self::Error> {
        // Key: String ID of the item (e.g. "hello world")
        // Value: Item containing localizations and metadata
        let mut strings = HashMap::<String, Item>::new();
        let mut source_language = String::new();
        let mut version = String::new();

        resources.sort_by(|left, right| {
            left.metadata
                .language
                .cmp(&right.metadata.language)
                .then_with(|| left.metadata.domain.cmp(&right.metadata.domain))
        });

        for mut resource in resources {
            // source_language
            if source_language.is_empty() {
                if let Some(v) = resource.metadata.custom.remove("source_language") {
                    source_language = v; // moved, no clone
                } else {
                    return Err(Error::InvalidResource(
                        "No source language found in metadata".into(),
                    ));
                }
            } else if let Some(v) = resource.metadata.custom.get("source_language") {
                if source_language != *v {
                    return Err(Error::DataMismatch(format!(
                        "Source language mismatch: expected {}, found {}",
                        source_language, v
                    )));
                }
            } else {
                return Err(Error::InvalidResource(
                    "No source language found in metadata".into(),
                ));
            }

            if version.is_empty() {
                if let Some(v) = resource.metadata.custom.remove("version") {
                    version = v; // move, no clone
                } else {
                    return Err(Error::InvalidResource(
                        "No version found in metadata".into(),
                    ));
                }
            } else if let Some(v) = resource.metadata.custom.get("version") {
                if version != *v {
                    return Err(Error::DataMismatch(format!(
                        "Version mismatch: expected {}, found {}",
                        version, v
                    )));
                }
            } else {
                return Err(Error::InvalidResource(
                    "No version found in metadata".into(),
                ));
            }

            for entry in resource.entries {
                let id = entry.id.clone();
                if let Some(item) = Item::new(entry, resource.metadata.language.clone()) {
                    strings
                        .entry(id)
                        .or_insert(Item {
                            localizations: HashMap::new(),
                            comment: item.comment,
                            extraction_state: item.extraction_state,
                            should_translate: item.should_translate,
                            is_comment_auto_generated: item.is_comment_auto_generated,
                        })
                        .localizations
                        .extend(item.localizations);
                }
            }
        }

        Ok(Format {
            source_language,
            version,
            strings,
        })
    }
}

impl TryFrom<Format> for Vec<Resource> {
    type Error = Error;

    fn try_from(format: Format) -> Result<Self, Self::Error> {
        // Key: Language code, e.g. "en", "fr", etc.
        // Value: Resource containing all items for that language
        let mut resource_map = BTreeMap::<String, Resource>::new();
        let mut pending_empty_translatable_entries =
            Vec::<(String, Option<String>, HashMap<String, String>)>::new();

        let mut custom_meta = HashMap::<String, String>::new();
        custom_meta.insert(String::from("source_language"), format.source_language);
        custom_meta.insert(String::from("version"), format.version);

        for (id, item) in format.strings {
            let mut custom = HashMap::new();
            if let Some(extraction_state) = &item.extraction_state {
                custom.insert("extraction_state".to_string(), extraction_state.to_string());
            }
            if let Some(is_comment_auto_generated) = item.is_comment_auto_generated {
                custom.insert(
                    "is_comment_auto_generated".to_string(),
                    is_comment_auto_generated.to_string(),
                );
            }

            if item.localizations.is_empty() {
                if item.should_translate.unwrap_or(true) {
                    // Defer empty translatable entries until language buckets are known.
                    pending_empty_translatable_entries.push((id.clone(), item.comment, custom));
                } else {
                    // Preserve non-translatable metadata-only entries by attaching an empty
                    // do-not-translate entry to source language.
                    let source_language = custom_meta
                        .get("source_language")
                        .cloned()
                        .unwrap_or_else(|| "en".to_string());
                    resource_map
                        .entry(source_language.clone())
                        .or_insert(Resource {
                            metadata: Metadata {
                                language: source_language,
                                domain: String::default(),
                                custom: custom_meta.clone(),
                            },
                            entries: Vec::new(),
                        })
                        .add_entry(Entry {
                            id: id.clone(),
                            value: Translation::Empty,
                            comment: item.comment.clone(),
                            status: EntryStatus::DoNotTranslate,
                            custom: custom.clone(),
                        });
                }
                continue;
            }

            let source_language = custom_meta
                .get("source_language")
                .cloned()
                .unwrap_or_else(|| "en".to_string());
            let mut saw_source_language = false;

            for (lang_code, localization) in item.localizations {
                if lang_code == source_language {
                    saw_source_language = true;
                }
                if let Some(translation) = localization.to_translation() {
                    let lang_code = lang_code.to_string();
                    resource_map
                        .entry(lang_code.clone())
                        .or_insert(Resource {
                            metadata: Metadata {
                                language: lang_code.clone(),
                                domain: String::default(),
                                custom: custom_meta.clone(),
                            },
                            entries: Vec::new(),
                        })
                        .add_entry(Entry {
                            id: id.clone(),
                            value: translation,
                            comment: item.comment.clone(),
                            status: localization.state(),
                            custom: custom.clone(),
                        });
                }
            }

            if !saw_source_language && item.should_translate.unwrap_or(true) {
                resource_map
                    .entry(source_language.clone())
                    .or_insert(Resource {
                        metadata: Metadata {
                            language: source_language.clone(),
                            domain: String::default(),
                            custom: custom_meta.clone(),
                        },
                        entries: Vec::new(),
                    })
                    .add_entry(Entry {
                        id: id.clone(),
                        value: if id.is_empty() {
                            Translation::Empty
                        } else {
                            Translation::Singular(id.clone())
                        },
                        comment: item.comment.clone(),
                        status: EntryStatus::New,
                        custom: custom.clone(),
                    });
            }
        }

        if !pending_empty_translatable_entries.is_empty() {
            let mut lang_codes = resource_map.keys().cloned().collect::<Vec<_>>();
            if lang_codes.is_empty() {
                lang_codes.push(
                    custom_meta
                        .get("source_language")
                        .cloned()
                        .unwrap_or_else(|| "en".to_string()),
                );
            }
            for (id, comment, custom) in pending_empty_translatable_entries {
                for lang_code in &lang_codes {
                    resource_map
                        .entry(lang_code.clone())
                        .or_insert(Resource {
                            metadata: Metadata {
                                language: lang_code.clone(),
                                domain: String::default(),
                                custom: custom_meta.clone(),
                            },
                            entries: Vec::new(),
                        })
                        .add_entry(Entry {
                            id: id.clone(),
                            value: Translation::Empty,
                            comment: comment.clone(),
                            status: EntryStatus::New,
                            custom: custom.clone(),
                        });
                }
            }
        }

        Ok(resource_map.into_values().collect())
    }
}

fn is_none_or_true(v: &Option<bool>) -> bool {
    v.is_none() || *v == Some(true)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_comment_auto_generated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_state: Option<ExtractionState>,
    #[serde(skip_serializing_if = "is_none_or_true")]
    pub should_translate: Option<bool>,
    #[serde(default)]
    #[serde(serialize_with = "serialize_sorted_map")]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub localizations: HashMap<String, Localization>,
}

impl Item {
    fn new(entry: Entry, language: String) -> Option<Self> {
        let mut localizations = HashMap::new();

        let should_translate = Some(entry.status != EntryStatus::DoNotTranslate);

        match entry.value {
            Translation::Empty => {} // Do nothing
            Translation::Singular(value) => {
                localizations.insert(
                    language,
                    Localization::from(StringUnit::new(entry.status, &value)),
                );
            }
            Translation::Plural(plural) => {
                localizations.insert(
                    language,
                    Localization::from(Variations::new(plural.forms.iter().map(
                        |(category, value)| {
                            (
                                category.clone(),
                                PluralVariation::new(entry.status.clone(), value),
                            )
                        },
                    ))),
                );
            }
        }

        let extraction_state = entry
            .custom
            .get("extraction_state")
            .and_then(|s| s.parse::<ExtractionState>().ok());

        let is_comment_auto_generated = entry
            .custom
            .get("is_comment_auto_generated")
            .and_then(|s| s.parse::<bool>().ok());

        Some(Item {
            localizations,
            comment: entry.comment,
            extraction_state,
            should_translate,
            is_comment_auto_generated,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionState {
    Manual,
    Stale,
    ExtractedWithValue,
    Migrated,
}

impl std::fmt::Display for ExtractionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractionState::Manual => write!(f, "manual"),
            ExtractionState::Stale => write!(f, "stale"),
            ExtractionState::ExtractedWithValue => write!(f, "extracted_with_value"),
            ExtractionState::Migrated => write!(f, "migrated"),
        }
    }
}

impl FromStr for ExtractionState {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "manual" => Ok(ExtractionState::Manual),
            "stale" => Ok(ExtractionState::Stale),
            "extracted_with_value" => Ok(ExtractionState::ExtractedWithValue),
            "migrated" => Ok(ExtractionState::Migrated),
            _ => Err(Error::DataMismatch(format!(
                "Unknown extraction state: {}",
                s
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Localization {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string_unit: Option<StringUnit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variations: Option<Variations>,
}

impl From<StringUnit> for Localization {
    fn from(string_unit: StringUnit) -> Self {
        Localization {
            string_unit: Some(string_unit),
            variations: None,
        }
    }
}

impl From<Variations> for Localization {
    fn from(variations: Variations) -> Self {
        Localization {
            string_unit: None,
            variations: Some(variations),
        }
    }
}

impl Localization {
    fn to_translation(&self) -> Option<Translation> {
        match (self.string_unit.as_ref(), self.variations.as_ref()) {
            (Some(string_unit), _) => Some(Translation::Singular(string_unit.value.clone())),
            (_, Some(variations)) => variations.to_translation(),
            (None, None) => None,
        }
    }

    fn state(&self) -> EntryStatus {
        if let Some(string_unit) = &self.string_unit {
            string_unit.state.clone()
        } else if let Some(variations) = &self.variations {
            // If variations exist, we assume all variations are in the same state
            variations
                .plural
                .as_ref()
                .and_then(|plural_map| {
                    let mut keys: Vec<&PluralCategory> = plural_map.keys().collect();
                    keys.sort_unstable();
                    keys.into_iter().find_map(|category| {
                        plural_map
                            .get(category)
                            .and_then(|variation| variation.string_unit.as_ref())
                            .map(|su| su.state.clone())
                    })
                })
                .unwrap_or(EntryStatus::Stale)
        } else {
            EntryStatus::Stale
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct StringUnit {
    pub state: EntryStatus,
    pub value: String,
}

impl StringUnit {
    pub fn new(state: EntryStatus, value: &str) -> Self {
        Self {
            state,
            value: crate::placeholder::to_ios_placeholders(value).replace("\\n", "\n"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Variations {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "serialize_optional_sorted_plural_map")]
    pub plural: Option<HashMap<PluralCategory, PluralVariation>>,
}

impl Variations {
    pub fn new(plural: impl Iterator<Item = (PluralCategory, PluralVariation)>) -> Self {
        let plural = plural.collect();
        Self {
            plural: Some(plural),
        }
    }
}

impl Variations {
    fn to_translation(&self) -> Option<Translation> {
        self.plural.as_ref().and_then(|plural_map| {
            let forms = plural_map.iter().filter_map(|(category, variation)| {
                let category = category.clone();
                let value = variation.string_unit.as_ref()?.value.clone();
                Some((category, value))
            });

            Plural::new("", forms).map(Translation::Plural)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluralVariation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string_unit: Option<StringUnit>,
}

impl PluralVariation {
    pub fn new(state: EntryStatus, value: &str) -> Self {
        Self {
            string_unit: Some(StringUnit::new(state, value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format_with_translated_before_empty() -> Format {
        for _ in 0..256 {
            let mut strings = HashMap::new();
            strings.insert(
                "translated_key".to_string(),
                Item {
                    localizations: {
                        let mut localizations = HashMap::new();
                        localizations.insert(
                            "en".to_string(),
                            Localization::from(StringUnit::new(EntryStatus::Translated, "Hello")),
                        );
                        localizations.insert(
                            "fr".to_string(),
                            Localization::from(StringUnit::new(EntryStatus::Translated, "Bonjour")),
                        );
                        localizations
                    },
                    comment: None,
                    extraction_state: None,
                    should_translate: Some(true),
                    is_comment_auto_generated: None,
                },
            );
            strings.insert(
                "empty_key".to_string(),
                Item {
                    localizations: HashMap::new(),
                    comment: Some("Missing translation".to_string()),
                    extraction_state: None,
                    should_translate: Some(true),
                    is_comment_auto_generated: None,
                },
            );

            let order: Vec<&str> = strings.keys().map(String::as_str).collect();
            let translated_pos = order
                .iter()
                .position(|key| *key == "translated_key")
                .expect("translated key in map");
            let empty_pos = order
                .iter()
                .position(|key| *key == "empty_key")
                .expect("empty key in map");

            if translated_pos < empty_pos {
                return Format {
                    source_language: "en".to_string(),
                    version: "1.0".to_string(),
                    strings,
                };
            }
        }

        panic!("failed to build deterministic key order for xcstrings test");
    }

    #[test]
    fn test_ios_placeholder_conversion_in_writer() {
        // Build resources that contain Android-style placeholders
        let res = Resource {
            metadata: Metadata {
                language: "en".to_string(),
                domain: String::new(),
                custom: {
                    let mut m = HashMap::new();
                    m.insert("source_language".into(), "en".into());
                    m.insert("version".into(), "1.0".into());
                    m
                },
            },
            entries: vec![
                Entry {
                    id: "greet".into(),
                    value: Translation::Singular("Hello %1$s and %s".into()),
                    comment: None,
                    status: EntryStatus::Translated,
                    custom: HashMap::new(),
                },
                Entry {
                    id: "files".into(),
                    value: Translation::Plural(Plural {
                        id: "files".into(),
                        forms: {
                            let mut f = std::collections::BTreeMap::new();
                            f.insert(PluralCategory::One, "%1$s file".into());
                            f.insert(PluralCategory::Other, "%1$s files".into());
                            f
                        },
                    }),
                    comment: None,
                    status: EntryStatus::Translated,
                    custom: HashMap::new(),
                },
            ],
        };

        let fmt = Format::try_from(vec![res]).expect("xcstrings from resources");
        // greet
        let item = fmt.strings.get("greet").expect("greet item");
        let en = item.localizations.get("en").expect("en loc");
        let val = en.string_unit.as_ref().unwrap().value.clone();
        assert!(val.contains("%1$@") && val.contains("%@"));

        // plurals
        let files = fmt.strings.get("files").expect("files item");
        let en_p = files.localizations.get("en").expect("en loc");
        let plural_map = en_p.variations.as_ref().unwrap().plural.as_ref().unwrap();
        assert!(plural_map
            .get(&PluralCategory::One)
            .unwrap()
            .string_unit
            .as_ref()
            .unwrap()
            .value
            .contains("%1$@"));
    }

    #[test]
    fn test_plural_variations_serialize_in_category_order() {
        let fmt = Format {
            source_language: "en".to_string(),
            version: "1.0".to_string(),
            strings: HashMap::from([(
                "files".to_string(),
                Item {
                    localizations: HashMap::from([(
                        "en".to_string(),
                        Localization::from(Variations::new(
                            [
                                (
                                    PluralCategory::Other,
                                    PluralVariation::new(EntryStatus::Translated, "%d files"),
                                ),
                                (
                                    PluralCategory::Zero,
                                    PluralVariation::new(EntryStatus::Translated, "No files"),
                                ),
                                (
                                    PluralCategory::Many,
                                    PluralVariation::new(EntryStatus::Translated, "Many files"),
                                ),
                                (
                                    PluralCategory::One,
                                    PluralVariation::new(EntryStatus::Translated, "One file"),
                                ),
                            ]
                            .into_iter(),
                        )),
                    )]),
                    comment: None,
                    extraction_state: None,
                    should_translate: Some(true),
                    is_comment_auto_generated: None,
                },
            )]),
        };

        let mut out = Vec::new();
        fmt.to_writer(&mut out).expect("serialize xcstrings");
        let json = String::from_utf8(out).expect("utf8 json");

        let zero = json.find("\"zero\"").expect("zero category");
        let one = json.find("\"one\"").expect("one category");
        let many = json.find("\"many\"").expect("many category");
        let other = json.find("\"other\"").expect("other category");

        assert!(zero < one);
        assert!(one < many);
        assert!(many < other);
    }

    #[test]
    fn test_empty_item_is_marked_new_when_expanded_for_existing_languages() {
        let format = format_with_translated_before_empty();
        let resources = Vec::<Resource>::try_from(format).expect("resources from xcstrings");
        assert_eq!(resources.len(), 2);

        for resource in resources {
            let empty_entry = resource
                .entries
                .iter()
                .find(|entry| entry.id == "empty_key")
                .expect("empty entry is generated for each known language");

            assert_eq!(empty_entry.value, Translation::Empty);
            assert_eq!(empty_entry.status, EntryStatus::New);
            assert_eq!(empty_entry.comment.as_deref(), Some("Missing translation"));
        }
    }

    #[test]
    fn test_non_translatable_item_without_localizations_is_preserved() {
        let mut strings = HashMap::new();
        strings.insert(
            "game_title".to_string(),
            Item {
                localizations: {
                    let mut localizations = HashMap::new();
                    localizations.insert(
                        "en".to_string(),
                        Localization::from(StringUnit::new(EntryStatus::Translated, "Game")),
                    );
                    localizations
                },
                comment: None,
                extraction_state: None,
                should_translate: Some(true),
                is_comment_auto_generated: None,
            },
        );
        strings.insert(
            "Carrom".to_string(),
            Item {
                localizations: HashMap::new(),
                comment: Some("The text label for the Carrom game button.".to_string()),
                extraction_state: None,
                should_translate: Some(false),
                is_comment_auto_generated: Some(true),
            },
        );
        let format = Format {
            source_language: "en".to_string(),
            version: "1.0".to_string(),
            strings,
        };

        let resources = Vec::<Resource>::try_from(format).expect("resources from xcstrings");
        let roundtrip = Format::try_from(resources).expect("xcstrings from resources");
        let carrom = roundtrip.strings.get("Carrom").expect("Carrom item exists");

        assert!(carrom.localizations.is_empty());
        assert_eq!(
            carrom.comment.as_deref(),
            Some("The text label for the Carrom game button.")
        );
        assert_eq!(carrom.should_translate, Some(false));
        assert_eq!(carrom.is_comment_auto_generated, Some(true));
    }

    #[test]
    fn test_translatable_metadata_only_item_without_localizations_is_preserved() {
        let mut strings = HashMap::new();
        strings.insert(
            "The following rewards have been sent to your backpack.".to_string(),
            Item {
                localizations: HashMap::new(),
                comment: Some("Text displayed in the tips view of the return user reward dialog, describing the rewards that have been sent to the user's backpack.".to_string()),
                extraction_state: None,
                should_translate: None,
                is_comment_auto_generated: Some(true),
            },
        );
        let format = Format {
            source_language: "en".to_string(),
            version: "1.0".to_string(),
            strings,
        };

        let resources = Vec::<Resource>::try_from(format).expect("resources from xcstrings");
        let roundtrip = Format::try_from(resources).expect("xcstrings from resources");
        let key = "The following rewards have been sent to your backpack.";
        let item = roundtrip.strings.get(key).expect("reward tip item exists");

        assert!(item.localizations.is_empty());
        assert_eq!(
            item.comment.as_deref(),
            Some(
                "Text displayed in the tips view of the return user reward dialog, describing the rewards that have been sent to the user's backpack."
            )
        );
        assert_eq!(item.is_comment_auto_generated, Some(true));
    }

    #[test]
    fn test_missing_source_language_localization_expands_to_key_as_new_entry() {
        let mut strings = HashMap::new();
        strings.insert(
            "99+ users have won tons of blue diamonds here".to_string(),
            Item {
                localizations: {
                    let mut localizations = HashMap::new();
                    localizations.insert(
                        "tr".to_string(),
                        Localization::from(StringUnit::new(
                            EntryStatus::Translated,
                            "99+ kullanici burada tonlarca mavi elmas kazandi",
                        )),
                    );
                    localizations
                },
                comment: None,
                extraction_state: None,
                should_translate: Some(true),
                is_comment_auto_generated: None,
            },
        );
        let format = Format {
            source_language: "en".to_string(),
            version: "1.0".to_string(),
            strings,
        };

        let resources = Vec::<Resource>::try_from(format).expect("resources from xcstrings");
        let en = resources
            .iter()
            .find(|resource| resource.metadata.language == "en")
            .expect("synthetic source-language resource exists");
        let entry = en
            .entries
            .iter()
            .find(|entry| entry.id == "99+ users have won tons of blue diamonds here")
            .expect("synthetic source-language entry exists");

        assert_eq!(
            entry.value,
            Translation::Singular("99+ users have won tons of blue diamonds here".to_string())
        );
        assert_eq!(entry.status, EntryStatus::New);
    }
}
