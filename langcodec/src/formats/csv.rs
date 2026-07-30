//! Support for CSV localization format.
//!
//! Supports the conventional wide `key,<language>...` schema for simple
//! singular catalogs and a versioned long schema for lossless resources.
//! Resource conversion selects the basic schema only when its singular entries
//! and canonical basic metadata can be reconstructed exactly. Even singular
//! resources use the extended schema when basic decoding would otherwise
//! invent or discard model fields.
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::{BufRead, Write},
};

use crate::{
    error::Error,
    traits::Parser,
    types::{Entry, EntryStatus, Metadata, Plural, PluralCategory, Resource, Translation},
};

/// Represents a multi-language CSV record where the first column is the key
/// and subsequent columns are translations for different languages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiLanguageCSVRecord {
    pub key: String,
    pub translations: HashMap<String, String>,
}

impl MultiLanguageCSVRecord {
    /// Creates a new multi-language CSV record.
    pub fn new(key: String) -> Self {
        Self {
            key,
            translations: HashMap::new(),
        }
    }

    /// Adds a translation for a specific language.
    pub fn add_translation(&mut self, language: String, value: String) {
        self.translations.insert(language, value);
    }

    /// Gets a translation for a specific language.
    pub fn get_translation(&self, language: &str) -> Option<&String> {
        self.translations.get(language)
    }
}

/// Represents the CSV format containing all basic-schema records.
///
/// Lossless extended data is held privately so it cannot appear as fabricated
/// user records. Construct values with [`Format::new`] or
/// [`Format::with_records`]; the private schema field intentionally prevents
/// external struct literals from bypassing schema invariants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    pub records: Vec<MultiLanguageCSVRecord>,
    schema: TabularSchema,
}

impl Format {
    /// Creates a new CSV format with empty records.
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            schema: TabularSchema::Basic {
                language_order: Vec::new(),
            },
        }
    }

    /// Creates a new CSV format with the given records.
    pub fn with_records(records: Vec<MultiLanguageCSVRecord>) -> Self {
        Self {
            records,
            schema: TabularSchema::Basic {
                language_order: Vec::new(),
            },
        }
    }

    /// Adds a record to the format.
    pub fn add_record(&mut self, record: MultiLanguageCSVRecord) {
        self.records.push(record);
    }

    /// Gets all records.
    pub fn get_records(&self) -> &[MultiLanguageCSVRecord] {
        &self.records
    }

    /// Gets all records as mutable.
    pub fn get_records_mut(&mut self) -> &mut [MultiLanguageCSVRecord] {
        &mut self.records
    }

    /// Returns whether this value uses langcodec's lossless extended schema.
    ///
    /// This is useful to callers which add legacy provenance metadata after
    /// parsing: extended files already carry exact resource metadata and must
    /// not be decorated or overwritten.
    pub fn is_extended(&self) -> bool {
        matches!(&self.schema, TabularSchema::Extended { .. })
    }
}

impl Default for Format {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) const EXTENDED_HEADER: [&str; 16] = [
    "__langcodec_extended_v1",
    "row_kind",
    "resource_index",
    "entry_index",
    "language",
    "domain",
    "resource_custom",
    "key",
    "value_kind",
    "plural_id",
    "plural_category",
    "value",
    "status",
    "comment_kind",
    "comment",
    "entry_custom",
];

const EXTENDED_RESERVED_PREFIX: &str = "__langcodec_extended_";
const EXTENDED_ROW_VERSION: &str = "v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TabularSchema {
    Basic { language_order: Vec<String> },
    Extended { resources: Vec<Resource> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BasicRecord {
    pub key: String,
    pub translations: HashMap<String, String>,
}

fn data_mismatch(format_name: &str, row: usize, message: impl AsRef<str>) -> Error {
    Error::DataMismatch(format!(
        "Invalid {format_name} row {row}: {}",
        message.as_ref()
    ))
}

fn exact_header(row: &[String]) -> bool {
    row.len() == EXTENDED_HEADER.len()
        && row
            .iter()
            .zip(EXTENDED_HEADER)
            .all(|(actual, expected)| actual == expected)
}

pub(crate) fn parse_tabular<R: BufRead>(
    reader: R,
    delimiter: u8,
    format_name: &str,
) -> Result<(Vec<BasicRecord>, TabularSchema), Error> {
    let mut csv_reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(reader);
    let rows = csv_reader
        .records()
        .map(|record| {
            record
                .map(|record| record.iter().map(ToOwned::to_owned).collect::<Vec<_>>())
                .map_err(Error::CsvParse)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let Some(first) = rows.first() else {
        return Ok((
            Vec::new(),
            TabularSchema::Basic {
                language_order: Vec::new(),
            },
        ));
    };

    if exact_header(first) {
        let resources = parse_extended_rows(&rows[1..], format_name)?;
        return Ok((Vec::new(), TabularSchema::Extended { resources }));
    }

    if first
        .first()
        .is_some_and(|field| field.starts_with(EXTENDED_RESERVED_PREFIX))
    {
        return Err(data_mismatch(
            format_name,
            1,
            "unknown or malformed extended schema header",
        ));
    }

    parse_basic_rows(&rows, format_name)
}

fn parse_basic_rows(
    rows: &[Vec<String>],
    format_name: &str,
) -> Result<(Vec<BasicRecord>, TabularSchema), Error> {
    let first = &rows[0];
    if first.len() < 2 {
        return Err(data_mismatch(
            format_name,
            1,
            "expected at least two columns",
        ));
    }

    let (language_order, data_start, width) =
        if first.len() == 2 && !first[0].trim().eq_ignore_ascii_case("key") {
            (vec!["default".to_string()], 0, 2)
        } else {
            if !first[0].trim().eq_ignore_ascii_case("key") {
                return Err(data_mismatch(
                    format_name,
                    1,
                    "wide schema header must begin with `key`",
                ));
            }
            let languages = first.iter().skip(1).cloned().collect::<Vec<_>>();
            validate_languages(&languages, format_name, 1)?;
            (languages, 1, first.len())
        };

    let mut seen_keys = HashSet::new();
    let mut records = Vec::with_capacity(rows.len().saturating_sub(data_start));
    for (index, row) in rows.iter().enumerate().skip(data_start) {
        let row_number = index + 1;
        if row.len() != width {
            return Err(data_mismatch(
                format_name,
                row_number,
                format!("expected {width} columns, found {}", row.len()),
            ));
        }
        if !seen_keys.insert(row[0].clone()) {
            return Err(data_mismatch(
                format_name,
                row_number,
                format!("duplicate key `{}`", row[0]),
            ));
        }
        let translations = language_order
            .iter()
            .cloned()
            .zip(row.iter().skip(1).cloned())
            .collect();
        records.push(BasicRecord {
            key: row[0].clone(),
            translations,
        });
    }

    Ok((records, TabularSchema::Basic { language_order }))
}

fn validate_languages(languages: &[String], format_name: &str, row: usize) -> Result<(), Error> {
    let mut seen = HashSet::new();
    for language in languages {
        if language.is_empty() {
            return Err(data_mismatch(
                format_name,
                row,
                "language headers cannot be empty",
            ));
        }
        if language.trim() != language.as_str() {
            return Err(data_mismatch(
                format_name,
                row,
                format!("language header `{language}` has leading or trailing whitespace"),
            ));
        }
        if !seen.insert(language) {
            return Err(data_mismatch(
                format_name,
                row,
                format!("duplicate language header `{language}`"),
            ));
        }
    }
    Ok(())
}

pub(crate) fn write_tabular<W: Write>(
    writer: W,
    delimiter: u8,
    format_name: &str,
    records: &[BasicRecord],
    schema: &TabularSchema,
) -> Result<(), Error> {
    let mut csv_writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(writer);

    match schema {
        TabularSchema::Extended { resources } => {
            if !records.is_empty() {
                return Err(Error::DataMismatch(format!(
                    "Invalid {format_name}: extended schema cannot contain basic records"
                )));
            }
            csv_writer
                .write_record(EXTENDED_HEADER)
                .map_err(Error::CsvParse)?;
            for row in extended_rows(resources)? {
                csv_writer.write_record(row).map_err(Error::CsvParse)?;
            }
        }
        TabularSchema::Basic { language_order } => {
            if records.is_empty() && language_order.is_empty() {
                return Ok(());
            }
            let languages = ordered_languages(records, language_order);
            validate_languages(&languages, format_name, 1)?;
            if languages.is_empty() {
                return Err(Error::DataMismatch(format!(
                    "Invalid {format_name}: basic records require at least one language"
                )));
            }

            let mut seen_keys = HashSet::new();
            for (index, record) in records.iter().enumerate() {
                if !seen_keys.insert(&record.key) {
                    return Err(data_mismatch(
                        format_name,
                        index + 2,
                        format!("duplicate key `{}`", record.key),
                    ));
                }
            }

            let mut header = vec!["key".to_string()];
            header.extend(languages.iter().cloned());
            csv_writer.write_record(header).map_err(Error::CsvParse)?;
            for record in records {
                let mut row = vec![record.key.clone()];
                row.extend(languages.iter().map(|language| {
                    record
                        .translations
                        .get(language)
                        .cloned()
                        .unwrap_or_default()
                }));
                csv_writer.write_record(row).map_err(Error::CsvParse)?;
            }
        }
    }

    csv_writer.flush().map_err(Error::Io)
}

pub(crate) fn tabular_from_resources(
    resources: Vec<Resource>,
) -> Result<(Vec<BasicRecord>, TabularSchema), Error> {
    if resources.is_empty() {
        return Ok((
            Vec::new(),
            TabularSchema::Basic {
                language_order: Vec::new(),
            },
        ));
    }
    validate_plural_payloads(&resources, "tabular output")?;

    if basic_is_compatible(&resources) {
        let language_order = resources
            .iter()
            .map(|resource| resource.metadata.language.clone())
            .collect::<Vec<_>>();
        let records = resources[0]
            .entries
            .iter()
            .enumerate()
            .map(|(entry_index, entry)| {
                let translations = resources
                    .iter()
                    .map(|resource| {
                        let Translation::Singular(value) = &resource.entries[entry_index].value
                        else {
                            unreachable!("basic_is_compatible rejects non-singular values")
                        };
                        (resource.metadata.language.clone(), value.clone())
                    })
                    .collect();
                BasicRecord {
                    key: entry.id.clone(),
                    translations,
                }
            })
            .collect();
        Ok((records, TabularSchema::Basic { language_order }))
    } else {
        Ok((Vec::new(), TabularSchema::Extended { resources }))
    }
}

fn basic_is_compatible(resources: &[Resource]) -> bool {
    let Some(first) = resources.first() else {
        return true;
    };

    let source_language = &first.metadata.language;
    let expected_custom = HashMap::from([
        ("source_language".to_string(), source_language.clone()),
        ("version".to_string(), "1.0".to_string()),
    ]);
    let expected_keys = first
        .entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    let unique_keys = expected_keys.iter().copied().collect::<HashSet<_>>();
    if unique_keys.len() != expected_keys.len() {
        return false;
    }

    let mut languages = HashSet::new();
    resources.iter().all(|resource| {
        !resource.metadata.language.is_empty()
            && resource.metadata.language.trim() == resource.metadata.language.as_str()
            && languages.insert(resource.metadata.language.clone())
            && metadata_is_basic_compatible(resource, &expected_custom)
            && resource.entries.len() == expected_keys.len()
            && resource
                .entries
                .iter()
                .zip(&expected_keys)
                .all(|(entry, expected_key)| {
                    entry.id.as_str() == *expected_key
                        && matches!(&entry.value, Translation::Singular(_))
                        && entry.comment.is_none()
                        && entry.status == EntryStatus::Translated
                        && entry.custom.is_empty()
                })
    })
}

fn metadata_is_basic_compatible(
    resource: &Resource,
    expected_custom: &HashMap<String, String>,
) -> bool {
    resource.metadata.domain.is_empty() && &resource.metadata.custom == expected_custom
}

pub(crate) fn tabular_into_resources(
    records: Vec<BasicRecord>,
    schema: TabularSchema,
) -> Result<Vec<Resource>, Error> {
    match schema {
        TabularSchema::Extended { resources } => {
            if records.is_empty() {
                Ok(resources)
            } else {
                Err(Error::DataMismatch(
                    "Extended tabular schema cannot contain basic records".to_string(),
                ))
            }
        }
        TabularSchema::Basic { language_order } => {
            if records.is_empty() && language_order.is_empty() {
                return Ok(Vec::new());
            }

            let mut seen_keys = HashSet::new();
            for (index, record) in records.iter().enumerate() {
                if !seen_keys.insert(&record.key) {
                    return Err(data_mismatch(
                        "tabular",
                        index + 1,
                        format!("duplicate key `{}`", record.key),
                    ));
                }
            }

            let languages = ordered_languages(&records, &language_order);
            validate_languages(&languages, "tabular", 1)?;
            if languages.is_empty() {
                return Err(Error::DataMismatch(
                    "Basic tabular records require at least one language".to_string(),
                ));
            }

            let source_language = languages
                .first()
                .cloned()
                .unwrap_or_else(|| "en".to_string());
            let custom = HashMap::from([
                ("source_language".to_string(), source_language),
                ("version".to_string(), "1.0".to_string()),
            ]);
            let resources = languages
                .into_iter()
                .map(|language| {
                    let entries = records
                        .iter()
                        .map(|record| Entry {
                            id: record.key.clone(),
                            value: Translation::Singular(
                                record
                                    .translations
                                    .get(&language)
                                    .cloned()
                                    .unwrap_or_default(),
                            ),
                            comment: None,
                            status: EntryStatus::Translated,
                            custom: HashMap::new(),
                        })
                        .collect();
                    Resource {
                        metadata: Metadata {
                            language,
                            domain: String::new(),
                            custom: custom.clone(),
                        },
                        entries,
                    }
                })
                .collect();
            Ok(resources)
        }
    }
}

fn ordered_languages(records: &[BasicRecord], declared_languages: &[String]) -> Vec<String> {
    if records.is_empty() {
        return declared_languages.to_vec();
    }

    let declared = declared_languages.iter().cloned().collect::<HashSet<_>>();
    let mut languages = declared_languages
        .iter()
        .filter(|language| {
            records
                .iter()
                .any(|record| record.translations.contains_key(*language))
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut added_languages = records
        .iter()
        .flat_map(|record| record.translations.keys().cloned())
        .filter(|language| !declared.contains(language))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    added_languages.sort();
    languages.extend(added_languages);
    languages
}

fn extended_rows(resources: &[Resource]) -> Result<Vec<Vec<String>>, Error> {
    let mut rows = Vec::new();
    for (resource_index, resource) in resources.iter().enumerate() {
        rows.push(vec![
            EXTENDED_ROW_VERSION.to_string(),
            "resource".to_string(),
            resource_index.to_string(),
            String::new(),
            resource.metadata.language.clone(),
            resource.metadata.domain.clone(),
            encode_custom(&resource.metadata.custom)?,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]);
        for (entry_index, entry) in resource.entries.iter().enumerate() {
            let (value_kind, plural_id, value) = match &entry.value {
                Translation::Empty => ("empty", String::new(), String::new()),
                Translation::Singular(value) => ("singular", String::new(), value.clone()),
                Translation::Plural(plural) => ("plural", plural.id.clone(), String::new()),
            };
            rows.push(vec![
                EXTENDED_ROW_VERSION.to_string(),
                "entry".to_string(),
                resource_index.to_string(),
                entry_index.to_string(),
                String::new(),
                String::new(),
                String::new(),
                entry.id.clone(),
                value_kind.to_string(),
                plural_id,
                String::new(),
                value,
                status_name(&entry.status).to_string(),
                if entry.comment.is_some() {
                    "some".to_string()
                } else {
                    "none".to_string()
                },
                entry.comment.clone().unwrap_or_default(),
                encode_custom(&entry.custom)?,
            ]);
            if let Translation::Plural(plural) = &entry.value {
                for (category, value) in &plural.forms {
                    rows.push(vec![
                        EXTENDED_ROW_VERSION.to_string(),
                        "plural_form".to_string(),
                        resource_index.to_string(),
                        entry_index.to_string(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        plural_category_name(category).to_string(),
                        value.clone(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                    ]);
                }
            }
        }
    }
    Ok(rows)
}

fn parse_extended_rows(rows: &[Vec<String>], format_name: &str) -> Result<Vec<Resource>, Error> {
    let mut resources: Vec<Resource> = Vec::new();
    for (offset, row) in rows.iter().enumerate() {
        let row_number = offset + 2;
        if row.len() != EXTENDED_HEADER.len() {
            return Err(data_mismatch(
                format_name,
                row_number,
                format!(
                    "expected {} columns, found {}",
                    EXTENDED_HEADER.len(),
                    row.len()
                ),
            ));
        }
        if row[0] != EXTENDED_ROW_VERSION {
            return Err(data_mismatch(
                format_name,
                row_number,
                format!("expected row schema `{EXTENDED_ROW_VERSION}`"),
            ));
        }
        match row[1].as_str() {
            "resource" => {
                require_blank(
                    row,
                    &[3, 7, 8, 9, 10, 11, 12, 13, 14, 15],
                    format_name,
                    row_number,
                )?;
                let resource_index =
                    parse_index(&row[2], "resource_index", format_name, row_number)?;
                if resource_index != resources.len() {
                    return Err(data_mismatch(
                        format_name,
                        row_number,
                        format!(
                            "resource_index must be {}, found {resource_index}",
                            resources.len()
                        ),
                    ));
                }
                resources.push(Resource {
                    metadata: Metadata {
                        language: row[4].clone(),
                        domain: row[5].clone(),
                        custom: decode_custom(&row[6], "resource_custom", format_name, row_number)?,
                    },
                    entries: Vec::new(),
                });
            }
            "entry" => {
                require_blank(row, &[4, 5, 6, 10], format_name, row_number)?;
                let resource_index =
                    parse_index(&row[2], "resource_index", format_name, row_number)?;
                let current_resource_index = resources.len().checked_sub(1).ok_or_else(|| {
                    data_mismatch(
                        format_name,
                        row_number,
                        "entry row appears before a resource row",
                    )
                })?;
                let Some(resource) = resources.last_mut() else {
                    return Err(data_mismatch(
                        format_name,
                        row_number,
                        "entry row appears before a resource row",
                    ));
                };
                if resource_index != current_resource_index {
                    return Err(data_mismatch(
                        format_name,
                        row_number,
                        "entry row must belong to the current resource",
                    ));
                }
                let entry_index = parse_index(&row[3], "entry_index", format_name, row_number)?;
                if entry_index != resource.entries.len() {
                    return Err(data_mismatch(
                        format_name,
                        row_number,
                        format!(
                            "entry_index must be {}, found {entry_index}",
                            resource.entries.len()
                        ),
                    ));
                }
                let value = match row[8].as_str() {
                    "empty" => {
                        require_blank(row, &[9, 11], format_name, row_number)?;
                        Translation::Empty
                    }
                    "singular" => {
                        require_blank(row, &[9], format_name, row_number)?;
                        Translation::Singular(row[11].clone())
                    }
                    "plural" => {
                        require_blank(row, &[11], format_name, row_number)?;
                        Translation::Plural(Plural {
                            id: row[9].clone(),
                            forms: BTreeMap::new(),
                        })
                    }
                    other => {
                        return Err(data_mismatch(
                            format_name,
                            row_number,
                            format!("unknown value_kind `{other}`"),
                        ));
                    }
                };
                let status = row[12]
                    .parse::<EntryStatus>()
                    .map_err(|message| data_mismatch(format_name, row_number, message))?;
                let comment = match row[13].as_str() {
                    "none" => {
                        require_blank(row, &[14], format_name, row_number)?;
                        None
                    }
                    "some" => Some(row[14].clone()),
                    other => {
                        return Err(data_mismatch(
                            format_name,
                            row_number,
                            format!("unknown comment_kind `{other}`"),
                        ));
                    }
                };
                resource.entries.push(Entry {
                    id: row[7].clone(),
                    value,
                    comment,
                    status,
                    custom: decode_custom(&row[15], "entry_custom", format_name, row_number)?,
                });
            }
            "plural_form" => {
                require_blank(
                    row,
                    &[4, 5, 6, 7, 8, 9, 12, 13, 14, 15],
                    format_name,
                    row_number,
                )?;
                let resource_index =
                    parse_index(&row[2], "resource_index", format_name, row_number)?;
                let current_resource_index = resources.len().checked_sub(1).ok_or_else(|| {
                    data_mismatch(
                        format_name,
                        row_number,
                        "plural_form row appears before a resource row",
                    )
                })?;
                let Some(resource) = resources.last_mut() else {
                    return Err(data_mismatch(
                        format_name,
                        row_number,
                        "plural_form row appears before a resource row",
                    ));
                };
                if resource_index != current_resource_index {
                    return Err(data_mismatch(
                        format_name,
                        row_number,
                        "plural_form row must belong to the current resource",
                    ));
                }
                let entry_index = parse_index(&row[3], "entry_index", format_name, row_number)?;
                let current_entry_index =
                    resource.entries.len().checked_sub(1).ok_or_else(|| {
                        data_mismatch(
                            format_name,
                            row_number,
                            "plural_form row appears before an entry row",
                        )
                    })?;
                let Some(entry) = resource.entries.last_mut() else {
                    return Err(data_mismatch(
                        format_name,
                        row_number,
                        "plural_form row appears before an entry row",
                    ));
                };
                if entry_index != current_entry_index {
                    return Err(data_mismatch(
                        format_name,
                        row_number,
                        "plural_form row must belong to the current entry",
                    ));
                }
                let Translation::Plural(plural) = &mut entry.value else {
                    return Err(data_mismatch(
                        format_name,
                        row_number,
                        "plural_form row belongs to a non-plural entry",
                    ));
                };
                let category = row[10]
                    .parse::<PluralCategory>()
                    .map_err(|message| data_mismatch(format_name, row_number, message))?;
                if plural.forms.insert(category, row[11].clone()).is_some() {
                    return Err(data_mismatch(
                        format_name,
                        row_number,
                        format!("duplicate plural category `{}`", row[10]),
                    ));
                }
            }
            other => {
                return Err(data_mismatch(
                    format_name,
                    row_number,
                    format!("unknown row_kind `{other}`"),
                ));
            }
        }
    }
    validate_plural_payloads(&resources, format_name)?;
    Ok(resources)
}

fn validate_plural_payloads(resources: &[Resource], context: &str) -> Result<(), Error> {
    for resource in resources {
        for entry in &resource.entries {
            if let Translation::Plural(plural) = &entry.value
                && plural.forms.is_empty()
            {
                return Err(Error::DataMismatch(format!(
                    "Invalid {context}: plural entry `{}` in language `{}` has no forms",
                    entry.id, resource.metadata.language
                )));
            }
        }
    }
    Ok(())
}

fn parse_index(value: &str, field: &str, format_name: &str, row: usize) -> Result<usize, Error> {
    value.parse::<usize>().map_err(|_| {
        data_mismatch(
            format_name,
            row,
            format!("{field} must be a non-negative integer"),
        )
    })
}

fn require_blank(
    row: &[String],
    columns: &[usize],
    format_name: &str,
    row_number: usize,
) -> Result<(), Error> {
    for &column in columns {
        if !row[column].is_empty() {
            return Err(data_mismatch(
                format_name,
                row_number,
                format!(
                    "column `{}` must be blank for `{}` rows",
                    EXTENDED_HEADER[column], row[1]
                ),
            ));
        }
    }
    Ok(())
}

fn encode_custom(custom: &HashMap<String, String>) -> Result<String, Error> {
    let mut pairs = custom.iter().collect::<Vec<_>>();
    pairs.sort_by_key(|(key, _)| *key);
    serde_json::to_string(&pairs).map_err(Error::Parse)
}

fn decode_custom(
    encoded: &str,
    field: &str,
    format_name: &str,
    row: usize,
) -> Result<HashMap<String, String>, Error> {
    let pairs = serde_json::from_str::<Vec<(String, String)>>(encoded).map_err(|error| {
        data_mismatch(
            format_name,
            row,
            format!("{field} must be a JSON array of string pairs: {error}"),
        )
    })?;
    let mut custom = HashMap::with_capacity(pairs.len());
    for (key, value) in pairs {
        if custom.insert(key.clone(), value).is_some() {
            return Err(data_mismatch(
                format_name,
                row,
                format!("{field} contains duplicate key `{key}`"),
            ));
        }
    }
    Ok(custom)
}

fn status_name(status: &EntryStatus) -> &'static str {
    match status {
        EntryStatus::DoNotTranslate => "do_not_translate",
        EntryStatus::New => "new",
        EntryStatus::Stale => "stale",
        EntryStatus::NeedsReview => "needs_review",
        EntryStatus::Translated => "translated",
    }
}

fn plural_category_name(category: &PluralCategory) -> &'static str {
    match category {
        PluralCategory::Zero => "zero",
        PluralCategory::One => "one",
        PluralCategory::Two => "two",
        PluralCategory::Few => "few",
        PluralCategory::Many => "many",
        PluralCategory::Other => "other",
    }
}

impl Parser for Format {
    fn from_reader<R: BufRead>(reader: R) -> Result<Self, Error> {
        let (records, schema) = parse_tabular(reader, b',', "CSV")?;
        Ok(Self {
            records: records
                .into_iter()
                .map(|record| MultiLanguageCSVRecord {
                    key: record.key,
                    translations: record.translations,
                })
                .collect(),
            schema,
        })
    }

    fn to_writer<W: Write>(&self, writer: W) -> Result<(), Error> {
        let records = self
            .records
            .iter()
            .map(|record| BasicRecord {
                key: record.key.clone(),
                translations: record.translations.clone(),
            })
            .collect::<Vec<_>>();
        write_tabular(writer, b',', "CSV", &records, &self.schema)
    }
}

impl TryFrom<Vec<Resource>> for Format {
    type Error = Error;

    fn try_from(resources: Vec<Resource>) -> Result<Self, Self::Error> {
        let (records, schema) = tabular_from_resources(resources)?;
        Ok(Self {
            records: records
                .into_iter()
                .map(|record| MultiLanguageCSVRecord {
                    key: record.key,
                    translations: record.translations,
                })
                .collect(),
            schema,
        })
    }
}

impl TryFrom<Format> for Vec<Resource> {
    type Error = Error;

    fn try_from(format: Format) -> Result<Self, Self::Error> {
        let records = format
            .records
            .into_iter()
            .map(|record| BasicRecord {
                key: record.key,
                translations: record.translations,
            })
            .collect();
        tabular_into_resources(records, format.schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::Parser;
    use crate::types::{Resource, Translation};
    use std::io::Cursor;

    #[test]
    fn test_parse_simple_csv() {
        let csv_content = "hello,Hello\nbye,Goodbye\n";
        let format = Format::from_reader(Cursor::new(csv_content)).unwrap();
        assert_eq!(format.records.len(), 2);
        assert_eq!(format.records[0].key, "hello");
        assert_eq!(
            format.records[0].get_translation("default"),
            Some(&"Hello".to_string())
        );
        assert_eq!(format.records[1].key, "bye");
        assert_eq!(
            format.records[1].get_translation("default"),
            Some(&"Goodbye".to_string())
        );
    }

    #[test]
    fn test_round_trip_csv_resource_csv() {
        let csv_content = "hello,Hello\nbye,Goodbye\n";
        let format = Format::from_reader(Cursor::new(csv_content)).unwrap();
        let resources = Vec::<Resource>::try_from(format.clone()).unwrap();
        let serialized: Format = TryFrom::try_from(resources).unwrap();

        // Sort records by key for comparison since order may not be guaranteed
        let mut original_records = format.records.clone();
        let mut serialized_records = serialized.records.clone();
        original_records.sort_by(|a, b| a.key.cmp(&b.key));
        serialized_records.sort_by(|a, b| a.key.cmp(&b.key));

        assert_eq!(original_records, serialized_records);
    }

    #[test]
    fn test_csv_row_with_empty_value() {
        let csv_content = "empty,\nhello,Hello\n";
        let format = Format::from_reader(Cursor::new(csv_content)).unwrap();
        assert_eq!(format.records.len(), 2);
        assert_eq!(format.records[0].key, "empty");
        assert_eq!(
            format.records[0].get_translation("default"),
            Some(&"".to_string())
        );
        let resources = Vec::<Resource>::try_from(format.clone()).unwrap();
        assert_eq!(resources.len(), 1);
        // The entry with empty value should be present and its value should be empty
        let resource = &resources[0];
        assert_eq!(resource.entries.len(), 2);
        let entry = &resource.entries[0];
        assert_eq!(entry.id, "empty");
        assert_eq!(
            match &entry.value {
                Translation::Singular(s) => s,
                _ => panic!("Expected singular translation"),
            },
            ""
        );
    }

    #[test]
    fn test_parse_multi_language_csv() {
        let csv_content = "key,en,cn\nhello,Hello,你好\nbye,Goodbye,再见\n";
        let format = Format::from_reader(Cursor::new(csv_content)).unwrap();
        assert_eq!(format.records.len(), 2);

        // Check first record (first data row after header)
        assert_eq!(format.records[0].key, "hello");
        assert_eq!(
            format.records[0].get_translation("en"),
            Some(&"Hello".to_string())
        );
        assert_eq!(
            format.records[0].get_translation("cn"),
            Some(&"你好".to_string())
        );

        // Check second record
        assert_eq!(format.records[1].key, "bye");
        assert_eq!(
            format.records[1].get_translation("en"),
            Some(&"Goodbye".to_string())
        );
        assert_eq!(
            format.records[1].get_translation("cn"),
            Some(&"再见".to_string())
        );
    }

    #[test]
    fn test_parse_single_language_csv_as_multi() {
        let csv_content = "hello,Hello\nbye,Goodbye\n";
        let format = Format::from_reader(Cursor::new(csv_content)).unwrap();
        assert_eq!(format.records.len(), 2);

        // Check first record
        assert_eq!(format.records[0].key, "hello");
        assert_eq!(
            format.records[0].get_translation("default"),
            Some(&"Hello".to_string())
        );

        // Check second record
        assert_eq!(format.records[1].key, "bye");
        assert_eq!(
            format.records[1].get_translation("default"),
            Some(&"Goodbye".to_string())
        );
    }

    #[test]
    fn test_parse_single_language_header_csv() {
        let csv_content = "key,en\nhello,Hello\nbye,Goodbye\n";
        let format = Format::from_reader(Cursor::new(csv_content)).unwrap();
        assert_eq!(format.records.len(), 2);
        assert_eq!(
            format.records[0].get_translation("en"),
            Some(&"Hello".to_string())
        );
        assert_eq!(
            format.records[1].get_translation("en"),
            Some(&"Goodbye".to_string())
        );

        let resources = Vec::<Resource>::try_from(format).unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].metadata.language, "en");
    }

    #[test]
    fn test_multi_language_csv_to_resources() {
        let csv_content = "key,en,cn\nhello,Hello,你好\nbye,Goodbye,再见\n";
        let format = Format::from_reader(Cursor::new(csv_content)).unwrap();
        let resources = Vec::<Resource>::try_from(format).unwrap();

        assert_eq!(resources.len(), 2);

        // Check English resource
        let en_resource = resources
            .iter()
            .find(|r| r.metadata.language == "en")
            .unwrap();
        assert_eq!(en_resource.entries.len(), 2);
        assert_eq!(en_resource.entries[0].id, "hello");
        assert_eq!(en_resource.entries[1].id, "bye");

        // Check Chinese resource
        let cn_resource = resources
            .iter()
            .find(|r| r.metadata.language == "cn")
            .unwrap();
        assert_eq!(cn_resource.entries.len(), 2);
        assert_eq!(cn_resource.entries[0].id, "hello");
        assert_eq!(cn_resource.entries[1].id, "bye");
    }

    #[test]
    fn test_write_multi_language_csv() {
        let mut record1 = MultiLanguageCSVRecord::new("hello".to_string());
        record1.add_translation("en".to_string(), "Hello".to_string());
        record1.add_translation("cn".to_string(), "你好".to_string());

        let mut record2 = MultiLanguageCSVRecord::new("bye".to_string());
        record2.add_translation("en".to_string(), "Goodbye".to_string());
        record2.add_translation("cn".to_string(), "再见".to_string());

        let records = vec![record1, record2];

        let mut output = Vec::new();
        Format::with_records(records)
            .to_writer(&mut output)
            .unwrap();
        let output_str = String::from_utf8(output).unwrap();

        // The output should have a header row and data rows
        let lines: Vec<&str> = output_str.lines().collect();
        assert_eq!(lines.len(), 3); // header + 2 data rows

        // Check header contains key, en, cn (sorted)
        assert!(lines[0].contains("key"));
        assert!(lines[0].contains("cn"));
        assert!(lines[0].contains("en"));
    }

    #[test]
    fn test_multi_language_csv_record_methods() {
        let mut record = MultiLanguageCSVRecord::new("test_key".to_string());

        // Test initial state
        assert_eq!(record.key, "test_key");
        assert_eq!(record.translations.len(), 0);
        assert_eq!(record.get_translation("en"), None);

        // Test adding translations
        record.add_translation("en".to_string(), "Hello".to_string());
        record.add_translation("cn".to_string(), "你好".to_string());
        record.add_translation("es".to_string(), "Hola".to_string());

        // Test getting translations
        assert_eq!(record.get_translation("en"), Some(&"Hello".to_string()));
        assert_eq!(record.get_translation("cn"), Some(&"你好".to_string()));
        assert_eq!(record.get_translation("es"), Some(&"Hola".to_string()));
        assert_eq!(record.get_translation("fr"), None);

        // Test updating existing translation
        record.add_translation("en".to_string(), "Updated Hello".to_string());
        assert_eq!(
            record.get_translation("en"),
            Some(&"Updated Hello".to_string())
        );

        // Test translations count
        assert_eq!(record.translations.len(), 3);
    }

    #[test]
    fn test_multi_language_csv_record_clone() {
        let mut record1 = MultiLanguageCSVRecord::new("key1".to_string());
        record1.add_translation("en".to_string(), "Hello".to_string());
        record1.add_translation("cn".to_string(), "你好".to_string());

        let record2 = record1.clone();

        assert_eq!(record1.key, record2.key);
        assert_eq!(record1.translations, record2.translations);
        assert_eq!(record1.get_translation("en"), record2.get_translation("en"));
        assert_eq!(record1.get_translation("cn"), record2.get_translation("cn"));
    }

    #[test]
    fn test_multi_language_csv_record_debug() {
        let mut record = MultiLanguageCSVRecord::new("test_key".to_string());
        record.add_translation("en".to_string(), "Hello".to_string());
        record.add_translation("cn".to_string(), "你好".to_string());

        let debug_str = format!("{:?}", record);
        assert!(debug_str.contains("MultiLanguageCSVRecord"));
        assert!(debug_str.contains("test_key"));
        assert!(debug_str.contains("Hello"));
        assert!(debug_str.contains("你好"));
    }

    #[test]
    fn test_multi_language_csv_record_partial_eq() {
        let mut record1 = MultiLanguageCSVRecord::new("key1".to_string());
        record1.add_translation("en".to_string(), "Hello".to_string());
        record1.add_translation("cn".to_string(), "你好".to_string());

        let mut record2 = MultiLanguageCSVRecord::new("key1".to_string());
        record2.add_translation("en".to_string(), "Hello".to_string());
        record2.add_translation("cn".to_string(), "你好".to_string());

        let mut record3 = MultiLanguageCSVRecord::new("key2".to_string());
        record3.add_translation("en".to_string(), "Hello".to_string());

        assert_eq!(record1, record2);
        assert_ne!(record1, record3);
        assert_ne!(record2, record3);
    }

    #[test]
    fn test_multi_language_csv_record_empty_translations() {
        let record = MultiLanguageCSVRecord::new("empty_key".to_string());

        assert_eq!(record.key, "empty_key");
        assert_eq!(record.translations.len(), 0);
        assert_eq!(record.get_translation("en"), None);
        assert_eq!(record.get_translation("cn"), None);
    }

    #[test]
    fn test_multi_language_csv_record_unicode_keys() {
        let mut record = MultiLanguageCSVRecord::new("测试键".to_string());
        record.add_translation("en".to_string(), "Test Key".to_string());
        record.add_translation("cn".to_string(), "测试键".to_string());

        assert_eq!(record.key, "测试键");
        assert_eq!(record.get_translation("en"), Some(&"Test Key".to_string()));
        assert_eq!(record.get_translation("cn"), Some(&"测试键".to_string()));
    }

    #[test]
    fn test_csv_language_key_preservation() {
        // Create a CSV with specific language keys
        let csv_content =
            "key,en,fr,de\nhello,Hello,Bonjour,Hallo\nbye,Goodbye,Au revoir,Auf Wiedersehen\n";
        let format = Format::from_reader(Cursor::new(csv_content)).unwrap();

        // Check that the language keys are preserved
        assert_eq!(format.records.len(), 2);

        // Check first record
        let first_record = &format.records[0];
        assert_eq!(first_record.key, "hello");
        assert_eq!(
            first_record.get_translation("en"),
            Some(&"Hello".to_string())
        );
        assert_eq!(
            first_record.get_translation("fr"),
            Some(&"Bonjour".to_string())
        );
        assert_eq!(
            first_record.get_translation("de"),
            Some(&"Hallo".to_string())
        );

        // Check second record
        let second_record = &format.records[1];
        assert_eq!(second_record.key, "bye");
        assert_eq!(
            second_record.get_translation("en"),
            Some(&"Goodbye".to_string())
        );
        assert_eq!(
            second_record.get_translation("fr"),
            Some(&"Au revoir".to_string())
        );
        assert_eq!(
            second_record.get_translation("de"),
            Some(&"Auf Wiedersehen".to_string())
        );
    }

    #[test]
    fn test_csv_to_resources_language_preservation() {
        // Create a CSV with specific language keys
        let csv_content =
            "key,en,fr,de\nhello,Hello,Bonjour,Hallo\nbye,Goodbye,Au revoir,Auf Wiedersehen\n";
        let format = Format::from_reader(Cursor::new(csv_content)).unwrap();

        // Convert to resources
        let resources = Vec::<Resource>::try_from(format).unwrap();

        // Check that we have resources for each language
        assert_eq!(resources.len(), 3);

        // Check English resource
        let en_resource = resources
            .iter()
            .find(|r| r.metadata.language == "en")
            .unwrap();
        assert_eq!(en_resource.entries.len(), 2);
        assert_eq!(en_resource.entries[0].id, "hello");
        assert_eq!(
            en_resource.entries[0].value,
            Translation::Singular("Hello".to_string())
        );
        assert_eq!(en_resource.entries[1].id, "bye");
        assert_eq!(
            en_resource.entries[1].value,
            Translation::Singular("Goodbye".to_string())
        );

        // Check French resource
        let fr_resource = resources
            .iter()
            .find(|r| r.metadata.language == "fr")
            .unwrap();
        assert_eq!(fr_resource.entries.len(), 2);
        assert_eq!(fr_resource.entries[0].id, "hello");
        assert_eq!(
            fr_resource.entries[0].value,
            Translation::Singular("Bonjour".to_string())
        );
        assert_eq!(fr_resource.entries[1].id, "bye");
        assert_eq!(
            fr_resource.entries[1].value,
            Translation::Singular("Au revoir".to_string())
        );

        // Check German resource
        let de_resource = resources
            .iter()
            .find(|r| r.metadata.language == "de")
            .unwrap();
        assert_eq!(de_resource.entries.len(), 2);
        assert_eq!(de_resource.entries[0].id, "hello");
        assert_eq!(
            de_resource.entries[0].value,
            Translation::Singular("Hallo".to_string())
        );
        assert_eq!(de_resource.entries[1].id, "bye");
        assert_eq!(
            de_resource.entries[1].value,
            Translation::Singular("Auf Wiedersehen".to_string())
        );
    }

    #[test]
    fn test_csv_round_trip_language_preservation() {
        // Create a CSV with specific language keys
        let csv_content =
            "key,en,fr,de\nhello,Hello,Bonjour,Hallo\nbye,Goodbye,Au revoir,Auf Wiedersehen\n";
        let original_format = Format::from_reader(Cursor::new(csv_content)).unwrap();

        // Convert to resources and back to CSV
        let resources = Vec::<Resource>::try_from(original_format.clone()).unwrap();
        let round_trip_format = Format::try_from(resources).unwrap();

        // Check that language keys are preserved in round trip
        assert_eq!(
            original_format.records.len(),
            round_trip_format.records.len()
        );

        // Sort records by key for comparison
        let mut original_records = original_format.records.clone();
        let mut round_trip_records = round_trip_format.records.clone();
        original_records.sort_by(|a, b| a.key.cmp(&b.key));
        round_trip_records.sort_by(|a, b| a.key.cmp(&b.key));

        for (original, round_trip) in original_records.iter().zip(round_trip_records.iter()) {
            assert_eq!(original.key, round_trip.key);
            assert_eq!(original.translations, round_trip.translations);
        }
    }

    #[test]
    fn test_multi_language_csv_record_special_characters() {
        let mut record = MultiLanguageCSVRecord::new("key_with_special_chars".to_string());
        record.add_translation("en".to_string(), "Hello, World!".to_string());
        record.add_translation("cn".to_string(), "你好，世界！".to_string());
        record.add_translation("es".to_string(), "¡Hola, mundo!".to_string());

        assert_eq!(
            record.get_translation("en"),
            Some(&"Hello, World!".to_string())
        );
        assert_eq!(
            record.get_translation("cn"),
            Some(&"你好，世界！".to_string())
        );
        assert_eq!(
            record.get_translation("es"),
            Some(&"¡Hola, mundo!".to_string())
        );
    }

    #[test]
    fn test_multi_language_csv_record_overwrite_translation() {
        let mut record = MultiLanguageCSVRecord::new("overwrite_test".to_string());

        // Add initial translation
        record.add_translation("en".to_string(), "Original".to_string());
        assert_eq!(record.get_translation("en"), Some(&"Original".to_string()));

        // Overwrite with new translation
        record.add_translation("en".to_string(), "Updated".to_string());
        assert_eq!(record.get_translation("en"), Some(&"Updated".to_string()));
        assert_eq!(record.translations.len(), 1); // Should still be only one entry
    }

    #[test]
    fn test_multi_language_csv_record_multiple_languages() {
        let mut record = MultiLanguageCSVRecord::new("multilingual".to_string());

        let languages = vec![
            ("en", "English"),
            ("cn", "中文"),
            ("es", "Español"),
            ("fr", "Français"),
            ("de", "Deutsch"),
            ("ja", "日本語"),
            ("ko", "한국어"),
            ("ru", "Русский"),
        ];

        for (code, translation) in &languages {
            record.add_translation(code.to_string(), translation.to_string());
        }

        assert_eq!(record.translations.len(), 8);

        for (code, translation) in languages {
            assert_eq!(record.get_translation(code), Some(&translation.to_string()));
        }
    }
}
