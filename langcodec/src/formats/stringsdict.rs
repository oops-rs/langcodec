//! Support for Apple's property-list based `.stringsdict` plural format.
//!
//! This module intentionally supports the lossless intersection between
//! `.stringsdict` and langcodec's data model: one bare
//! `NSStringPluralRuleType` selector per top-level key. Wrapper text,
//! Apple select/gender rules, and entries containing more than one variable
//! are rejected instead of being flattened.
//!
//! Converting a generic [`Resource`] into this format also requires explicit
//! selector identity in the three `stringsdict.*` entry custom fields. Plural
//! form text alone cannot identify which printf argument drives quantity.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::{BufRead, Cursor, Write},
};

use quick_xml::{
    Reader, Writer,
    events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event},
};

use crate::{
    error::Error,
    provenance::PROVENANCE_PREFIX,
    traits::Parser,
    types::{Entry, EntryStatus, Metadata, Plural, PluralCategory, Resource, Translation},
};

/// Entry custom key used to retain `NSStringLocalizedFormatKey`.
pub const LOCALIZED_FORMAT_CUSTOM_KEY: &str = "stringsdict.localized_format";
/// Entry custom key used to retain the plural variable's dictionary key.
pub const VARIABLE_NAME_CUSTOM_KEY: &str = "stringsdict.variable_name";
/// Entry custom key used to retain `NSStringFormatValueTypeKey`.
pub const VALUE_TYPE_CUSTOM_KEY: &str = "stringsdict.value_type";

const LOCALIZED_FORMAT_KEY: &str = "NSStringLocalizedFormatKey";
const SPEC_TYPE_KEY: &str = "NSStringFormatSpecTypeKey";
const VALUE_TYPE_KEY: &str = "NSStringFormatValueTypeKey";
const PLURAL_RULE_TYPE: &str = "NSStringPluralRuleType";
const MAX_DICT_DEPTH: usize = 3;
const APPLE_PLIST_DOCTYPE: &str = r#"plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd""#;

/// A parsed Apple `.stringsdict` file.
///
/// `.stringsdict` does not carry a language identifier, so parsed files use an
/// empty `language`. Callers that know the language (for example from an
/// `*.lproj` directory) may set it before converting into [`Resource`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    pub language: String,
    pub entries: Vec<PluralEntry>,
}

/// One supported `.stringsdict` top-level entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluralEntry {
    pub key: String,
    pub localized_format: String,
    pub variable_name: String,
    pub value_type: String,
    pub forms: BTreeMap<PluralCategory, String>,
}

impl Parser for Format {
    fn from_reader<R: BufRead>(mut reader: R) -> Result<Self, Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map_err(Error::Io)?;

        let root = if bytes.starts_with(b"bplist") {
            decode_binary_plist_document(bytes)?
        } else {
            PlistReader::new(Cursor::new(bytes)).parse_document()?
        };
        let entries = decode_entries(root)?;
        Ok(Self {
            language: String::new(),
            entries,
        })
    }

    fn to_writer<W: Write>(&self, writer: W) -> Result<(), Error> {
        validate_format(self)?;

        let mut writer = Writer::new_with_indent(writer, b' ', 2);
        writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
        writer.write_event(Event::DocType(BytesText::from_escaped(APPLE_PLIST_DOCTYPE)))?;

        let mut plist = BytesStart::new("plist");
        plist.push_attribute(("version", "1.0"));
        writer.write_event(Event::Start(plist))?;
        writer.write_event(Event::Start(BytesStart::new("dict")))?;

        let mut entries = self.entries.iter().collect::<Vec<_>>();
        entries.sort_by(|left, right| left.key.cmp(&right.key));
        for entry in entries {
            write_key(&mut writer, &entry.key)?;
            writer.write_event(Event::Start(BytesStart::new("dict")))?;

            write_string_member(&mut writer, LOCALIZED_FORMAT_KEY, &entry.localized_format)?;
            write_key(&mut writer, &entry.variable_name)?;
            writer.write_event(Event::Start(BytesStart::new("dict")))?;
            write_string_member(&mut writer, SPEC_TYPE_KEY, PLURAL_RULE_TYPE)?;
            write_string_member(&mut writer, VALUE_TYPE_KEY, &entry.value_type)?;
            for (category, value) in &entry.forms {
                write_string_member(&mut writer, plural_category_name(category), value)?;
            }
            writer.write_event(Event::End(BytesEnd::new("dict")))?;

            writer.write_event(Event::End(BytesEnd::new("dict")))?;
        }

        writer.write_event(Event::End(BytesEnd::new("dict")))?;
        writer.write_event(Event::End(BytesEnd::new("plist")))?;
        writer.get_mut().write_all(b"\n").map_err(Error::Io)
    }
}

impl From<Format> for Resource {
    fn from(format: Format) -> Self {
        let entries = format
            .entries
            .into_iter()
            .map(|entry| {
                let mut custom = HashMap::new();
                custom.insert(
                    LOCALIZED_FORMAT_CUSTOM_KEY.to_string(),
                    entry.localized_format.clone(),
                );
                custom.insert(
                    VARIABLE_NAME_CUSTOM_KEY.to_string(),
                    entry.variable_name.clone(),
                );
                custom.insert(VALUE_TYPE_CUSTOM_KEY.to_string(), entry.value_type.clone());
                let status = inferred_status(&entry.forms);

                Entry {
                    id: entry.key.clone(),
                    value: Translation::Plural(Plural {
                        id: entry.key,
                        forms: entry.forms,
                    }),
                    comment: None,
                    status,
                    custom,
                }
            })
            .collect();

        Resource {
            metadata: Metadata {
                language: format.language,
                domain: String::new(),
                custom: HashMap::new(),
            },
            entries,
        }
    }
}

impl TryFrom<Resource> for Format {
    type Error = Error;

    fn try_from(resource: Resource) -> Result<Self, Self::Error> {
        validate_resource_metadata(&resource.metadata)?;
        let mut seen_keys = HashSet::new();
        let mut entries = Vec::with_capacity(resource.entries.len());

        for entry in resource.entries {
            if !seen_keys.insert(entry.id.clone()) {
                return Err(invalid_entry(&entry.id, "duplicate top-level resource key"));
            }

            let Entry {
                id,
                value,
                comment,
                status,
                mut custom,
            } = entry;
            let plural = match value {
                Translation::Plural(plural) => plural,
                Translation::Singular(_) => {
                    return Err(Error::DataMismatch(format!(
                        ".stringsdict key '{id}' is singular; only plural translations are representable"
                    )));
                }
                Translation::Empty => {
                    return Err(Error::DataMismatch(format!(
                        ".stringsdict key '{id}' is empty; only plural translations are representable"
                    )));
                }
            };
            if !plural.id.is_empty() && plural.id != id {
                return Err(Error::DataMismatch(format!(
                    ".stringsdict key '{id}' has plural identifier '{}'; this format can only preserve an empty identifier or one equal to the entry key",
                    plural.id
                )));
            }
            if comment.is_some() {
                return Err(Error::DataMismatch(format!(
                    ".stringsdict key '{id}' has a comment, which Apple .stringsdict cannot preserve"
                )));
            }
            let inferred_status = inferred_status(&plural.forms);
            if status != inferred_status {
                return Err(Error::DataMismatch(format!(
                    ".stringsdict key '{id}' has status '{status:?}', but its forms imply '{inferred_status:?}' and .stringsdict has no status field"
                )));
            }

            let explicit_variable_name = custom.remove(VARIABLE_NAME_CUSTOM_KEY);
            let explicit_localized_format = custom.remove(LOCALIZED_FORMAT_CUSTOM_KEY);
            let explicit_value_type = custom.remove(VALUE_TYPE_CUSTOM_KEY);
            if !custom.is_empty() {
                let mut keys = custom.into_keys().collect::<Vec<_>>();
                keys.sort();
                return Err(Error::DataMismatch(format!(
                    ".stringsdict key '{id}' has custom metadata that cannot be preserved: {}",
                    keys.join(", ")
                )));
            }
            let (variable_name, localized_format, value_type) = match (
                explicit_variable_name,
                explicit_localized_format,
                explicit_value_type,
            ) {
                (Some(variable_name), Some(localized_format), Some(value_type)) => {
                    (variable_name, localized_format, value_type)
                }
                _ => {
                    return Err(Error::DataMismatch(format!(
                        ".stringsdict key '{id}' cannot be encoded safely because Resource does not identify which printf argument drives plural selection; set all three Entry.custom keys ('{LOCALIZED_FORMAT_CUSTOM_KEY}', '{VARIABLE_NAME_CUSTOM_KEY}', and '{VALUE_TYPE_CUSTOM_KEY}') explicitly to opt in with the selector format, variable name, and numeric value type"
                    )));
                }
            };

            let plural_entry = PluralEntry {
                key: id,
                localized_format,
                variable_name,
                value_type,
                forms: plural.forms,
            };
            validate_entry(&plural_entry)?;
            entries.push(plural_entry);
        }

        Ok(Self {
            language: resource.metadata.language,
            entries,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlistValue {
    String(String),
    Dict(Vec<(String, PlistValue)>),
}

fn decode_binary_plist_document(bytes: Vec<u8>) -> Result<PlistValue, Error> {
    let value = plist::Value::from_reader(Cursor::new(bytes)).map_err(|error| {
        Error::InvalidResource(format!("invalid binary .stringsdict plist: {error}"))
    })?;
    decode_binary_plist_value(value, "<root>", 1)
}

fn decode_binary_plist_value(
    value: plist::Value,
    path: &str,
    depth: usize,
) -> Result<PlistValue, Error> {
    match value {
        plist::Value::String(value) => Ok(PlistValue::String(value)),
        plist::Value::Dictionary(dictionary) => {
            if depth > MAX_DICT_DEPTH {
                return Err(Error::InvalidResource(format!(
                    "plist dictionary nesting at {path} exceeds the supported depth of {MAX_DICT_DEPTH}"
                )));
            }

            let mut members = Vec::with_capacity(dictionary.len());
            for (key, value) in dictionary {
                let value_path = format!("{path}.{key}");
                members.push((
                    key,
                    decode_binary_plist_value(value, &value_path, depth + 1)?,
                ));
            }
            Ok(PlistValue::Dict(members))
        }
        value => Err(Error::UnsupportedFormat(format!(
            "binary plist value at {path} must be a dictionary or string, found {}",
            binary_plist_value_kind(&value)
        ))),
    }
}

fn binary_plist_value_kind(value: &plist::Value) -> &'static str {
    match value {
        plist::Value::Array(_) => "array",
        plist::Value::Dictionary(_) => "dictionary",
        plist::Value::Boolean(_) => "boolean",
        plist::Value::Data(_) => "data",
        plist::Value::Date(_) => "date",
        plist::Value::Real(_) => "real",
        plist::Value::Integer(_) => "integer",
        plist::Value::String(_) => "string",
        plist::Value::Uid(_) => "UID",
        _ => "unknown value type",
    }
}

struct PlistReader<R: BufRead> {
    reader: Reader<R>,
    buffer: Vec<u8>,
}

impl<R: BufRead> PlistReader<R> {
    fn new(reader: R) -> Self {
        let mut reader = Reader::from_reader(reader);
        reader.config_mut().trim_text(false);
        reader.config_mut().check_comments = true;
        Self {
            reader,
            buffer: Vec::new(),
        }
    }

    fn parse_document(mut self) -> Result<PlistValue, Error> {
        let mut saw_declaration = false;
        let mut saw_doctype = false;
        let mut at_document_start = true;
        let root_start = loop {
            match self.next_raw()? {
                Event::Decl(declaration)
                    if at_document_start && !saw_declaration && !saw_doctype =>
                {
                    validate_xml_declaration(&declaration)?;
                    saw_declaration = true;
                    at_document_start = false;
                }
                Event::Decl(_) => {
                    return Err(Error::InvalidResource(
                        "the XML declaration must be the first event in .stringsdict".to_string(),
                    ));
                }
                Event::DocType(doctype) if !saw_doctype => {
                    validate_plist_doctype(&doctype)?;
                    saw_doctype = true;
                    at_document_start = false;
                }
                Event::Comment(_) => {
                    at_document_start = false;
                }
                Event::Text(text) if is_xml_whitespace(&decode_text(&text)?) => {
                    at_document_start = false;
                }
                Event::Start(start) if start.name().as_ref() == b"plist" => break start,
                Event::Eof => {
                    return Err(Error::InvalidResource(
                        ".stringsdict is missing its <plist> root element".to_string(),
                    ));
                }
                event => {
                    return Err(Error::InvalidResource(format!(
                        "expected <plist> root element, found {}",
                        event_description(&event)
                    )));
                }
            }
        };

        validate_plist_attributes(&root_start)?;
        let root_value = match self.next_content()? {
            Event::Start(start) if start.name().as_ref() == b"dict" => {
                ensure_no_attributes(&start, "<plist>/<dict>")?;
                PlistValue::Dict(self.parse_dict("<root>", 1)?)
            }
            Event::Empty(start) if start.name().as_ref() == b"dict" => {
                ensure_no_attributes(&start, "<plist>/<dict>")?;
                PlistValue::Dict(Vec::new())
            }
            event => {
                return Err(Error::InvalidResource(format!(
                    "<plist> must contain exactly one <dict>, found {}",
                    event_description(&event)
                )));
            }
        };

        match self.next_content()? {
            Event::End(end) if end.name().as_ref() == b"plist" => {}
            event => {
                return Err(Error::InvalidResource(format!(
                    "expected </plist> after the root dictionary, found {}",
                    event_description(&event)
                )));
            }
        }
        match self.next_content()? {
            Event::Eof => Ok(root_value),
            event => Err(Error::InvalidResource(format!(
                "unexpected content after </plist>: {}",
                event_description(&event)
            ))),
        }
    }

    fn parse_dict(&mut self, path: &str, depth: usize) -> Result<Vec<(String, PlistValue)>, Error> {
        let mut members = Vec::new();
        let mut keys = HashSet::new();

        loop {
            let key = match self.next_content()? {
                Event::End(end) if end.name().as_ref() == b"dict" => break,
                Event::Start(start) if start.name().as_ref() == b"key" => {
                    ensure_no_attributes(&start, &format!("{path}/<key>"))?;
                    self.parse_text_element(b"key", &format!("{path}/<key>"))?
                }
                Event::Empty(start) if start.name().as_ref() == b"key" => {
                    ensure_no_attributes(&start, &format!("{path}/<key>"))?;
                    String::new()
                }
                Event::Eof => {
                    return Err(Error::InvalidResource(format!(
                        "unexpected end of file while reading dictionary at {path}"
                    )));
                }
                event => {
                    return Err(Error::InvalidResource(format!(
                        "dictionary at {path} expected <key>, found {}",
                        event_description(&event)
                    )));
                }
            };

            if !keys.insert(key.clone()) {
                return Err(Error::InvalidResource(format!(
                    "duplicate plist key '{key}' in dictionary at {path}"
                )));
            }

            let value_path = format!("{path}.{key}");
            let value_event = self.next_content()?;
            if matches!(value_event, Event::End(_) | Event::Eof) {
                return Err(Error::InvalidResource(format!(
                    "plist key '{key}' at {path} is missing its value"
                )));
            }
            let value = self.parse_value(value_event, &value_path, depth + 1)?;
            members.push((key, value));
        }

        Ok(members)
    }

    fn parse_value(
        &mut self,
        event: Event<'static>,
        path: &str,
        depth: usize,
    ) -> Result<PlistValue, Error> {
        match event {
            Event::Start(start) if start.name().as_ref() == b"dict" => {
                if depth > MAX_DICT_DEPTH {
                    return Err(Error::InvalidResource(format!(
                        "plist dictionary nesting at {path} exceeds the supported depth of {MAX_DICT_DEPTH}"
                    )));
                }
                ensure_no_attributes(&start, path)?;
                Ok(PlistValue::Dict(self.parse_dict(path, depth)?))
            }
            Event::Empty(start) if start.name().as_ref() == b"dict" => {
                if depth > MAX_DICT_DEPTH {
                    return Err(Error::InvalidResource(format!(
                        "plist dictionary nesting at {path} exceeds the supported depth of {MAX_DICT_DEPTH}"
                    )));
                }
                ensure_no_attributes(&start, path)?;
                Ok(PlistValue::Dict(Vec::new()))
            }
            Event::Start(start) if start.name().as_ref() == b"string" => {
                ensure_no_attributes(&start, path)?;
                Ok(PlistValue::String(
                    self.parse_text_element(b"string", path)?,
                ))
            }
            Event::Empty(start) if start.name().as_ref() == b"string" => {
                ensure_no_attributes(&start, path)?;
                Ok(PlistValue::String(String::new()))
            }
            event => Err(Error::InvalidResource(format!(
                "plist value at {path} must be <dict> or <string>, found {}",
                event_description(&event)
            ))),
        }
    }

    fn parse_text_element(&mut self, element_name: &[u8], path: &str) -> Result<String, Error> {
        let mut value = String::new();
        loop {
            match self.next_raw()? {
                Event::Text(text) => value.push_str(&decode_text(&text)?),
                Event::CData(cdata) => {
                    let decoded = cdata
                        .decode()
                        .map_err(|error| Error::XmlParse(error.into()))?;
                    value.push_str(&normalize_xml_line_endings(&decoded));
                }
                Event::Comment(_) => {}
                Event::End(end) if end.name().as_ref() == element_name => return Ok(value),
                Event::Eof => {
                    return Err(Error::InvalidResource(format!(
                        "unexpected end of file while reading text at {path}"
                    )));
                }
                event => {
                    return Err(Error::InvalidResource(format!(
                        "text value at {path} contains unsupported {}",
                        event_description(&event)
                    )));
                }
            }
        }
    }

    fn next_content(&mut self) -> Result<Event<'static>, Error> {
        loop {
            match self.next_raw()? {
                Event::Comment(_) => {}
                Event::Text(text) if is_xml_whitespace(&decode_text(&text)?) => {}
                event => return Ok(event),
            }
        }
    }

    fn next_raw(&mut self) -> Result<Event<'static>, Error> {
        self.buffer.clear();
        self.reader
            .read_event_into(&mut self.buffer)
            .map(Event::into_owned)
            .map_err(Error::XmlParse)
    }
}

fn decode_entries(root: PlistValue) -> Result<Vec<PluralEntry>, Error> {
    let PlistValue::Dict(entries) = root else {
        return Err(Error::InvalidResource(
            ".stringsdict root value must be a dictionary".to_string(),
        ));
    };

    entries
        .into_iter()
        .map(|(key, value)| decode_entry(key, value))
        .collect()
}

fn decode_entry(key: String, value: PlistValue) -> Result<PluralEntry, Error> {
    if key.is_empty() {
        return Err(Error::InvalidResource(
            ".stringsdict contains an empty top-level key".to_string(),
        ));
    }
    let PlistValue::Dict(members) = value else {
        return Err(invalid_entry(&key, "top-level value must be a dictionary"));
    };

    let mut localized_format = None;
    let mut variable = None;
    for (member_key, member_value) in members {
        match member_key.as_str() {
            LOCALIZED_FORMAT_KEY => {
                localized_format = Some(expect_string(member_value, &key, LOCALIZED_FORMAT_KEY)?);
            }
            _ => {
                let PlistValue::Dict(rule) = member_value else {
                    return Err(Error::UnsupportedFormat(format!(
                        ".stringsdict key '{key}' contains unsupported member '{member_key}'"
                    )));
                };
                if variable.is_some() {
                    return Err(Error::UnsupportedFormat(format!(
                        ".stringsdict key '{key}' defines multiple variables; only one plural variable is supported"
                    )));
                }
                variable = Some(decode_rule(&key, member_key, rule)?);
            }
        }
    }

    let localized_format = localized_format
        .ok_or_else(|| invalid_entry(&key, format!("missing required '{LOCALIZED_FORMAT_KEY}'")))?;
    let (variable_name, value_type, forms) = variable.ok_or_else(|| {
        invalid_entry(
            &key,
            "missing its NSStringPluralRuleType variable dictionary",
        )
    })?;

    let entry = PluralEntry {
        key,
        localized_format,
        variable_name,
        value_type,
        forms,
    };
    validate_entry(&entry)?;
    Ok(entry)
}

fn decode_rule(
    entry_key: &str,
    variable_name: String,
    members: Vec<(String, PlistValue)>,
) -> Result<(String, String, BTreeMap<PluralCategory, String>), Error> {
    let mut spec_type = None;
    let mut value_type = None;
    let mut forms = BTreeMap::new();

    for (key, value) in members {
        match key.as_str() {
            SPEC_TYPE_KEY => {
                spec_type = Some(expect_string(value, entry_key, SPEC_TYPE_KEY)?);
            }
            VALUE_TYPE_KEY => {
                value_type = Some(expect_string(value, entry_key, VALUE_TYPE_KEY)?);
            }
            category => {
                let category = parse_plural_category(category).ok_or_else(|| {
                    Error::UnsupportedFormat(format!(
                        ".stringsdict key '{entry_key}' variable '{variable_name}' has unknown plural category '{category}'"
                    ))
                })?;
                let value = expect_string(value, entry_key, plural_category_name(&category))?;
                forms.insert(category, value);
            }
        }
    }

    let spec_type = spec_type.ok_or_else(|| {
        invalid_entry(
            entry_key,
            format!("variable '{variable_name}' is missing required '{SPEC_TYPE_KEY}'"),
        )
    })?;
    if spec_type != PLURAL_RULE_TYPE {
        return Err(Error::UnsupportedFormat(format!(
            ".stringsdict key '{entry_key}' variable '{variable_name}' uses unsupported rule type '{spec_type}'; only '{PLURAL_RULE_TYPE}' is supported"
        )));
    }
    let value_type = value_type.ok_or_else(|| {
        invalid_entry(
            entry_key,
            format!("variable '{variable_name}' is missing required '{VALUE_TYPE_KEY}'"),
        )
    })?;

    Ok((variable_name, value_type, forms))
}

fn validate_format(format: &Format) -> Result<(), Error> {
    let mut keys = HashSet::new();
    for entry in &format.entries {
        if !keys.insert(entry.key.as_str()) {
            return Err(invalid_entry(
                &entry.key,
                "duplicate top-level .stringsdict key",
            ));
        }
        validate_entry(entry)?;
    }
    Ok(())
}

fn validate_entry(entry: &PluralEntry) -> Result<(), Error> {
    if entry.key.is_empty() {
        return Err(Error::InvalidResource(
            ".stringsdict contains an empty top-level key".to_string(),
        ));
    }
    if entry.variable_name.is_empty() {
        return Err(invalid_entry(&entry.key, "plural variable name is empty"));
    }
    if entry.variable_name.contains('@') {
        return Err(invalid_entry(
            &entry.key,
            format!(
                "plural variable '{}' contains the reserved '@' delimiter",
                entry.variable_name
            ),
        ));
    }
    if entry.variable_name == LOCALIZED_FORMAT_KEY {
        return Err(invalid_entry(
            &entry.key,
            format!(
                "plural variable '{}' collides with a reserved entry key",
                entry.variable_name
            ),
        ));
    }
    if !is_supported_plural_value_type(&entry.value_type) {
        return Err(invalid_entry(
            &entry.key,
            format!(
                "variable '{}' has invalid '{VALUE_TYPE_KEY}' '{}'; expected a numeric printf value-type token such as 'd', 'ld', 'lld', 'u', or 'f'",
                entry.variable_name, entry.value_type
            ),
        ));
    }
    if entry.forms.is_empty() {
        return Err(invalid_entry(
            &entry.key,
            format!("variable '{}' has no plural forms", entry.variable_name),
        ));
    }
    if !entry.forms.contains_key(&PluralCategory::Other) {
        return Err(invalid_entry(
            &entry.key,
            format!(
                "variable '{}' is missing required 'other' plural form",
                entry.variable_name
            ),
        ));
    }

    validate_xml_text(&entry.key, "top-level key", &entry.key)?;
    validate_xml_text(&entry.key, LOCALIZED_FORMAT_KEY, &entry.localized_format)?;
    validate_xml_text(&entry.key, "plural variable name", &entry.variable_name)?;
    validate_xml_text(&entry.key, VALUE_TYPE_KEY, &entry.value_type)?;
    for (category, value) in &entry.forms {
        validate_xml_text(&entry.key, plural_category_name(category), value)?;
        if contains_stringsdict_rule_reference(value) {
            return Err(Error::UnsupportedFormat(format!(
                ".stringsdict key '{}' plural form '{}' contains a nested rule selector; nested select/plural rules are not supported",
                entry.key,
                plural_category_name(category)
            )));
        }
    }

    let selector = bare_localized_format_reference(&entry.localized_format).map_err(|message| {
        invalid_entry(
            &entry.key,
            format!("invalid '{LOCALIZED_FORMAT_KEY}': {message}"),
        )
    })?;
    if selector.variable_name != entry.variable_name {
        return Err(invalid_entry(
            &entry.key,
            format!(
                "'{LOCALIZED_FORMAT_KEY}' references variable '{}', but the entry defines '{}'",
                selector.variable_name, entry.variable_name
            ),
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct BareSelector {
    variable_name: String,
}

fn bare_localized_format_reference(format: &str) -> Result<BareSelector, String> {
    let bytes = format.as_bytes();
    if bytes.first() != Some(&b'%') {
        return Err(
            "only a bare '%#@variable@' or '%n$#@variable@' selector is losslessly supported"
                .to_string(),
        );
    }

    let mut marker_start = 1;
    let position_start = marker_start;
    while bytes
        .get(marker_start)
        .is_some_and(|byte| byte.is_ascii_digit())
    {
        marker_start += 1;
    }
    if marker_start > position_start {
        if bytes.get(marker_start) != Some(&b'$') {
            return Err("a positional selector must include '$' after its index".to_string());
        }
        let position = format[position_start..marker_start]
            .parse::<u32>()
            .map_err(|_| "selector position is too large".to_string())?;
        if position == 0 {
            return Err("selector positions are 1-based; '%0$' is invalid".to_string());
        }
        marker_start += 1;
    }
    if !bytes[marker_start..].starts_with(b"#@") {
        return Err(
            "only a bare '%#@variable@' or '%n$#@variable@' selector is losslessly supported"
                .to_string(),
        );
    }

    let name_start = marker_start + 2;
    let Some(name_end) = format[name_start..].find('@').map(|end| name_start + end) else {
        return Err("unterminated '%#@variable@' reference".to_string());
    };
    if name_end == name_start {
        return Err("empty '%#@variable@' reference".to_string());
    }
    if name_end + 1 != format.len() {
        return Err(
            "only a bare selector without wrapper text or additional selectors is losslessly supported"
                .to_string(),
        );
    }

    Ok(BareSelector {
        variable_name: format[name_start..name_end].to_string(),
    })
}

fn contains_stringsdict_rule_reference(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        if bytes.get(index + 1) == Some(&b'%') {
            index += 2;
            continue;
        }

        let mut marker_start = index + 1;
        let position_start = marker_start;
        while bytes
            .get(marker_start)
            .is_some_and(|byte| byte.is_ascii_digit())
        {
            marker_start += 1;
        }
        if marker_start > position_start && bytes.get(marker_start) == Some(&b'$') {
            marker_start += 1;
        } else {
            marker_start = index + 1;
        }
        if bytes[marker_start..].starts_with(b"#@") {
            return true;
        }
        index += 1;
    }
    false
}

fn is_supported_plural_value_type(value: &str) -> bool {
    const INTEGER_LENGTHS: [&str; 9] = ["", "hh", "h", "l", "ll", "q", "z", "t", "j"];
    const FLOAT_LENGTHS: [&str; 3] = ["", "l", "L"];

    let Some(conversion) = value.chars().last() else {
        return false;
    };
    if !conversion.is_ascii() {
        return false;
    }
    let length = &value[..value.len() - conversion.len_utf8()];

    match conversion {
        'd' | 'i' | 'u' | 'o' | 'x' | 'X' => INTEGER_LENGTHS.contains(&length),
        'a' | 'A' | 'e' | 'E' | 'f' | 'F' | 'g' | 'G' => FLOAT_LENGTHS.contains(&length),
        _ => false,
    }
}

fn validate_xml_text(entry_key: &str, field: &str, value: &str) -> Result<(), Error> {
    if let Some(character) = value.chars().find(|character| {
        !matches!(
            *character,
            '\u{9}'
                | '\u{A}'
                | '\u{D}'
                | '\u{20}'..='\u{D7FF}'
                | '\u{E000}'..='\u{FFFD}'
                | '\u{10000}'..='\u{10FFFF}'
        )
    }) {
        return Err(invalid_entry(
            entry_key,
            format!(
                "{field} contains XML 1.0-illegal character U+{:04X}",
                character as u32
            ),
        ));
    }
    Ok(())
}

fn expect_string(value: PlistValue, entry_key: &str, member_key: &str) -> Result<String, Error> {
    match value {
        PlistValue::String(value) => Ok(value),
        PlistValue::Dict(_) => Err(invalid_entry(
            entry_key,
            format!("'{member_key}' must contain a <string> value"),
        )),
    }
}

fn invalid_entry(key: &str, message: impl AsRef<str>) -> Error {
    Error::InvalidResource(format!(".stringsdict key '{key}': {}", message.as_ref()))
}

fn validate_resource_metadata(metadata: &Metadata) -> Result<(), Error> {
    let mut unsupported = metadata
        .custom
        .keys()
        .filter(|key| {
            !matches!(key.as_str(), "source_language" | "version" | "format")
                && !key.starts_with(PROVENANCE_PREFIX)
        })
        .cloned()
        .collect::<Vec<_>>();
    unsupported.sort();
    if !unsupported.is_empty() {
        return Err(Error::DataMismatch(format!(
            ".stringsdict cannot preserve resource metadata keys: {}",
            unsupported.join(", ")
        )));
    }
    Ok(())
}

fn inferred_status(forms: &BTreeMap<PluralCategory, String>) -> EntryStatus {
    if forms.values().all(String::is_empty) {
        EntryStatus::New
    } else {
        EntryStatus::Translated
    }
}

fn parse_plural_category(value: &str) -> Option<PluralCategory> {
    match value {
        "zero" => Some(PluralCategory::Zero),
        "one" => Some(PluralCategory::One),
        "two" => Some(PluralCategory::Two),
        "few" => Some(PluralCategory::Few),
        "many" => Some(PluralCategory::Many),
        "other" => Some(PluralCategory::Other),
        _ => None,
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

fn write_key<W: Write>(writer: &mut Writer<W>, key: &str) -> Result<(), Error> {
    writer.write_event(Event::Start(BytesStart::new("key")))?;
    write_xml_text(writer, key)?;
    writer.write_event(Event::End(BytesEnd::new("key")))?;
    Ok(())
}

fn write_string_member<W: Write>(
    writer: &mut Writer<W>,
    key: &str,
    value: &str,
) -> Result<(), Error> {
    write_key(writer, key)?;
    writer.write_event(Event::Start(BytesStart::new("string")))?;
    write_xml_text(writer, value)?;
    writer.write_event(Event::End(BytesEnd::new("string")))?;
    Ok(())
}

fn write_xml_text<W: Write>(writer: &mut Writer<W>, value: &str) -> Result<(), Error> {
    let mut start = 0;
    for (index, _) in value.match_indices('\r') {
        writer.write_event(Event::Text(BytesText::new(&value[start..index])))?;
        writer.write_event(Event::Text(BytesText::from_escaped("&#13;")))?;
        start = index + 1;
    }
    writer.write_event(Event::Text(BytesText::new(&value[start..])))?;
    Ok(())
}

fn validate_xml_declaration(declaration: &BytesDecl<'_>) -> Result<(), Error> {
    let version = declaration.version().map_err(Error::XmlParse)?;
    if version.as_ref() != b"1.0" {
        return Err(Error::UnsupportedFormat(format!(
            "unsupported XML version '{}'; .stringsdict requires XML 1.0",
            String::from_utf8_lossy(&version)
        )));
    }

    if let Some(encoding) = declaration.encoding() {
        let encoding = encoding.map_err(|error| Error::XmlParse(error.into()))?;
        if !encoding.as_ref().eq_ignore_ascii_case(b"UTF-8") {
            return Err(Error::UnsupportedFormat(format!(
                "unsupported XML encoding '{}'; .stringsdict requires UTF-8",
                String::from_utf8_lossy(&encoding)
            )));
        }
    }

    if let Some(standalone) = declaration.standalone() {
        let standalone = standalone.map_err(|error| Error::XmlParse(error.into()))?;
        if !matches!(standalone.as_ref(), b"yes" | b"no") {
            return Err(Error::InvalidResource(format!(
                "invalid XML standalone value '{}'; expected 'yes' or 'no'",
                String::from_utf8_lossy(&standalone)
            )));
        }
    }

    Ok(())
}

fn validate_plist_doctype(doctype: &BytesText<'_>) -> Result<(), Error> {
    let doctype = decode_text(doctype)?;
    if doctype.trim() != APPLE_PLIST_DOCTYPE {
        return Err(Error::UnsupportedFormat(format!(
            "unsupported .stringsdict doctype '{}'; expected the Apple plist 1.0 doctype",
            doctype.trim()
        )));
    }
    Ok(())
}

fn validate_plist_attributes(start: &BytesStart<'_>) -> Result<(), Error> {
    let mut version = None;
    for attribute in start.attributes().with_checks(true) {
        let attribute = attribute.map_err(|error| {
            Error::InvalidResource(format!("invalid <plist> attribute: {error}"))
        })?;
        if attribute.key.as_ref() != b"version" {
            return Err(Error::InvalidResource(format!(
                "unsupported <plist> attribute '{}'",
                String::from_utf8_lossy(attribute.key.as_ref())
            )));
        }
        version = Some(attribute.unescape_value()?.into_owned());
    }
    match version.as_deref() {
        Some("1.0") => Ok(()),
        Some(version) => Err(Error::UnsupportedFormat(format!(
            "unsupported plist version '{version}'; only version 1.0 is supported"
        ))),
        None => Err(Error::InvalidResource(
            "<plist> is missing required version=\"1.0\"".to_string(),
        )),
    }
}

fn ensure_no_attributes(start: &BytesStart<'_>, path: &str) -> Result<(), Error> {
    if let Some(attribute) = start.attributes().with_checks(true).next() {
        let attribute = attribute.map_err(|error| {
            Error::InvalidResource(format!("invalid attribute at {path}: {error}"))
        })?;
        return Err(Error::InvalidResource(format!(
            "element at {path} has unsupported attribute '{}'",
            String::from_utf8_lossy(attribute.key.as_ref())
        )));
    }
    Ok(())
}

fn decode_text(text: &BytesText<'_>) -> Result<String, Error> {
    let raw = std::str::from_utf8(text.as_ref()).map_err(|error| {
        Error::InvalidResource(format!(".stringsdict contains invalid UTF-8 text: {error}"))
    })?;
    let normalized = normalize_xml_line_endings(raw);
    quick_xml::escape::unescape(&normalized)
        .map(|value| value.into_owned())
        .map_err(|error| Error::XmlParse(error.into()))
}

fn normalize_xml_line_endings(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn is_xml_whitespace(value: &str) -> bool {
    value
        .chars()
        .all(|character| matches!(character, ' ' | '\t' | '\n' | '\r'))
}

fn event_description(event: &Event<'_>) -> String {
    match event {
        Event::Start(start) => {
            format!("<{}>", String::from_utf8_lossy(start.name().as_ref()))
        }
        Event::End(end) => {
            format!("</{}>", String::from_utf8_lossy(end.name().as_ref()))
        }
        Event::Empty(start) => {
            format!("<{}/>", String::from_utf8_lossy(start.name().as_ref()))
        }
        Event::Text(_) => "non-whitespace text".to_string(),
        Event::CData(_) => "CDATA".to_string(),
        Event::Comment(_) => "XML comment".to_string(),
        Event::Decl(_) => "XML declaration".to_string(),
        Event::PI(_) => "processing instruction".to_string(),
        Event::DocType(_) => "DOCTYPE declaration".to_string(),
        Event::Eof => "end of file".to_string(),
    }
}
