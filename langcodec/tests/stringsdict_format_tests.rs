use std::collections::{BTreeMap, HashMap};

use langcodec::{
    Error,
    formats::stringsdict::{
        Format, LOCALIZED_FORMAT_CUSTOM_KEY, VALUE_TYPE_CUSTOM_KEY, VARIABLE_NAME_CUSTOM_KEY,
    },
    traits::Parser,
    types::{Entry, EntryStatus, Metadata, Plural, PluralCategory, Resource, Translation},
};
use plist::{Dictionary as BinaryDictionary, Value as BinaryValue};

// Generated independently with macOS `plutil -convert binary1` from the
// canonical `file_count` XML fixture used by this test module.
const CANONICAL_BINARY_STRINGSDICT: &[u8] = &[
    0x62, 0x70, 0x6c, 0x69, 0x73, 0x74, 0x30, 0x30, 0xd1, 0x01, 0x02, 0x5a, 0x66, 0x69, 0x6c, 0x65,
    0x5f, 0x63, 0x6f, 0x75, 0x6e, 0x74, 0xd2, 0x03, 0x04, 0x05, 0x0e, 0x55, 0x66, 0x69, 0x6c, 0x65,
    0x73, 0x5f, 0x10, 0x1a, 0x4e, 0x53, 0x53, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x4c, 0x6f, 0x63, 0x61,
    0x6c, 0x69, 0x7a, 0x65, 0x64, 0x46, 0x6f, 0x72, 0x6d, 0x61, 0x74, 0x4b, 0x65, 0x79, 0xd4, 0x06,
    0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x53, 0x6f, 0x6e, 0x65, 0x55, 0x6f, 0x74, 0x68, 0x65,
    0x72, 0x5f, 0x10, 0x19, 0x4e, 0x53, 0x53, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x46, 0x6f, 0x72, 0x6d,
    0x61, 0x74, 0x53, 0x70, 0x65, 0x63, 0x54, 0x79, 0x70, 0x65, 0x4b, 0x65, 0x79, 0x5f, 0x10, 0x1a,
    0x4e, 0x53, 0x53, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x46, 0x6f, 0x72, 0x6d, 0x61, 0x74, 0x56, 0x61,
    0x6c, 0x75, 0x65, 0x54, 0x79, 0x70, 0x65, 0x4b, 0x65, 0x79, 0x58, 0x25, 0x6c, 0x64, 0x20, 0x66,
    0x69, 0x6c, 0x65, 0x59, 0x25, 0x6c, 0x64, 0x20, 0x66, 0x69, 0x6c, 0x65, 0x73, 0x5f, 0x10, 0x16,
    0x4e, 0x53, 0x53, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x50, 0x6c, 0x75, 0x72, 0x61, 0x6c, 0x52, 0x75,
    0x6c, 0x65, 0x54, 0x79, 0x70, 0x65, 0x52, 0x6c, 0x64, 0x59, 0x25, 0x23, 0x40, 0x66, 0x69, 0x6c,
    0x65, 0x73, 0x40, 0x08, 0x0b, 0x16, 0x1b, 0x21, 0x3e, 0x47, 0x4b, 0x51, 0x6d, 0x8a, 0x93, 0x9d,
    0xb6, 0xb9, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x0f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0xc3,
];

fn plist(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
{body}
</dict>
</plist>
"#
    )
}

fn canonical_entry(
    key: &str,
    localized_format: &str,
    variable: &str,
    value_type: &str,
    rule_type: &str,
    forms: &str,
) -> String {
    format!(
        r#"<key>{key}</key>
<dict>
  <key>NSStringLocalizedFormatKey</key>
  <string>{localized_format}</string>
  <key>{variable}</key>
  <dict>
    <key>NSStringFormatSpecTypeKey</key>
    <string>{rule_type}</string>
    <key>NSStringFormatValueTypeKey</key>
    <string>{value_type}</string>
    {forms}
  </dict>
</dict>"#
    )
}

fn file_count_entry(localized_format: &str, value_type: &str, forms: &str) -> String {
    canonical_entry(
        "file_count",
        localized_format,
        "files",
        value_type,
        "NSStringPluralRuleType",
        forms,
    )
}

fn assert_error_contains(result: Result<Format, Error>, expected: &[&str]) {
    let error = result.expect_err("fixture must be rejected");
    let message = error.to_string();
    for fragment in expected {
        assert!(
            message.contains(fragment),
            "expected error '{message}' to contain '{fragment}'"
        );
    }
}

fn binary_plist_bytes(value: BinaryValue) -> Vec<u8> {
    let mut bytes = Vec::new();
    value
        .to_writer_binary(&mut bytes)
        .expect("serialize binary plist test fixture");
    bytes
}

fn resource_entry(forms: BTreeMap<PluralCategory, String>) -> Resource {
    Resource {
        metadata: Metadata {
            language: "en".to_string(),
            domain: String::new(),
            custom: HashMap::new(),
        },
        entries: vec![Entry {
            id: "file_count".to_string(),
            value: Translation::Plural(Plural {
                id: "file_count".to_string(),
                forms,
            }),
            comment: None,
            status: EntryStatus::Translated,
            custom: HashMap::new(),
        }],
    }
}

fn set_stringsdict_structure(
    resource: &mut Resource,
    localized_format: &str,
    variable_name: &str,
    value_type: &str,
) {
    resource.entries[0].custom = stringsdict_structure(localized_format, variable_name, value_type);
}

fn stringsdict_structure(
    localized_format: &str,
    variable_name: &str,
    value_type: &str,
) -> HashMap<String, String> {
    HashMap::from([
        (
            LOCALIZED_FORMAT_CUSTOM_KEY.to_string(),
            localized_format.to_string(),
        ),
        (
            VARIABLE_NAME_CUSTOM_KEY.to_string(),
            variable_name.to_string(),
        ),
        (VALUE_TYPE_CUSTOM_KEY.to_string(), value_type.to_string()),
    ])
}

#[test]
fn canonical_plural_parses_without_flattening() {
    let xml = plist(&file_count_entry(
        "%#@files@",
        "ld",
        r#"<key>one</key><string>%ld file</string>
           <key>other</key><string>%ld files</string>"#,
    ));

    let format = Format::from_str(&xml).expect("canonical stringsdict");
    let parsed = &format.entries[0];
    assert_eq!(parsed.key, "file_count");
    assert_eq!(parsed.localized_format, "%#@files@");
    assert_eq!(parsed.variable_name, "files");
    assert_eq!(parsed.value_type, "ld");

    let resource = Resource::from(format);
    let entry = resource.find_entry("file_count").expect("plural entry");
    assert_eq!(entry.comment, None);
    assert_eq!(entry.status, EntryStatus::Translated);
    assert_eq!(
        entry.custom.get(LOCALIZED_FORMAT_CUSTOM_KEY),
        Some(&"%#@files@".to_string())
    );
    assert_eq!(
        entry.custom.get(VARIABLE_NAME_CUSTOM_KEY),
        Some(&"files".to_string())
    );
    assert_eq!(
        entry.custom.get(VALUE_TYPE_CUSTOM_KEY),
        Some(&"ld".to_string())
    );
    let Translation::Plural(plural) = &entry.value else {
        panic!("plural must remain plural");
    };
    assert_eq!(plural.id, "file_count");
    assert_eq!(
        plural.forms.get(&PluralCategory::Other),
        Some(&"%ld files".to_string())
    );

    let round_tripped = Format::try_from(resource).expect("parsed structural metadata round-trips");
    assert_eq!(round_tripped.entries[0].localized_format, "%#@files@");
    assert_eq!(round_tripped.entries[0].variable_name, "files");
    assert_eq!(round_tripped.entries[0].value_type, "ld");
}

#[test]
fn positional_bare_selector_round_trips_and_positions_are_one_based() {
    let xml = plist(&file_count_entry(
        "%1$#@files@",
        "lld",
        "<key>other</key><string>%1$lld files</string>",
    ));
    let format = Format::from_str(&xml).expect("positional selector");
    let mut encoded = Vec::new();
    format.to_writer(&mut encoded).expect("serialize");
    assert!(String::from_utf8(encoded).unwrap().contains("%1$#@files@"));

    let invalid = plist(&file_count_entry(
        "%0$#@files@",
        "d",
        "<key>other</key><string>%d files</string>",
    ));
    assert_error_contains(Format::from_str(&invalid), &["1-based", "%0$"]);
}

#[test]
fn xcode_style_positional_selector_with_variable_relative_forms_round_trips() {
    let xml = plist(&canonical_entry(
        "attachments",
        "%1$#@attachments@",
        "attachments",
        "ld",
        "NSStringPluralRuleType",
        r#"<key>one</key><string>%ld attachment</string>
           <key>other</key><string>%ld attachments</string>"#,
    ));

    let format = Format::from_str(&xml).expect("Xcode IDEKit-style positional selector");
    let mut encoded = Vec::new();
    format.to_writer(&mut encoded).expect("serialize");
    let reparsed = Format::from_bytes(&encoded).expect("reparse");
    assert_eq!(reparsed.entries[0].localized_format, "%1$#@attachments@");
    assert_eq!(
        reparsed.entries[0].forms.get(&PluralCategory::Other),
        Some(&"%ld attachments".to_string())
    );
}

#[test]
fn wrapper_text_and_additional_selectors_are_rejected_as_lossy() {
    for localized_format in [
        "Found %#@files@.",
        "%#@files@ on %2$#@device@",
        "%#@device@",
    ] {
        let xml = plist(&file_count_entry(
            localized_format,
            "d",
            "<key>other</key><string>%d files</string>",
        ));
        assert_error_contains(
            Format::from_str(&xml),
            &["file_count", "NSStringLocalizedFormatKey"],
        );
    }
}

#[test]
fn canonical_binary_plist_reads_and_rewrites_as_canonical_xml() {
    let format = Format::from_bytes(CANONICAL_BINARY_STRINGSDICT).expect("binary stringsdict");
    let parsed = &format.entries[0];
    assert_eq!(parsed.key, "file_count");
    assert_eq!(parsed.localized_format, "%#@files@");
    assert_eq!(parsed.variable_name, "files");
    assert_eq!(parsed.value_type, "ld");
    assert_eq!(
        parsed.forms.get(&PluralCategory::One),
        Some(&"%ld file".to_string())
    );
    assert_eq!(
        parsed.forms.get(&PluralCategory::Other),
        Some(&"%ld files".to_string())
    );

    let mut encoded = Vec::new();
    format
        .to_writer(&mut encoded)
        .expect("canonical XML output");
    assert!(encoded.starts_with(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(!encoded.starts_with(b"bplist"));
    assert_eq!(
        Format::from_bytes(&encoded).expect("reparse XML output"),
        format
    );
}

#[test]
fn malformed_binary_plists_have_actionable_errors() {
    let error = Format::from_bytes(b"bplist00not-a-valid-binary-plist")
        .expect_err("malformed binary plist");
    assert!(matches!(error, Error::InvalidResource(_)));
    assert!(
        error
            .to_string()
            .contains("invalid binary .stringsdict plist")
    );
}

#[test]
fn binary_plists_reject_unsupported_value_kinds_with_the_key_path() {
    let mut entry = BinaryDictionary::new();
    entry.insert(
        "NSStringLocalizedFormatKey".to_string(),
        BinaryValue::String("%#@files@".to_string()),
    );
    entry.insert("files".to_string(), BinaryValue::Boolean(true));
    let mut root = BinaryDictionary::new();
    root.insert("file_count".to_string(), BinaryValue::Dictionary(entry));

    assert_error_contains(
        Format::from_bytes(&binary_plist_bytes(BinaryValue::Dictionary(root))),
        &["<root>.file_count.files", "boolean"],
    );
}

#[test]
fn binary_plists_enforce_the_dictionary_depth_limit() {
    let mut too_deep = BinaryDictionary::new();
    too_deep.insert(
        "leaf".to_string(),
        BinaryValue::String("not representable".to_string()),
    );
    let mut rule = BinaryDictionary::new();
    rule.insert("other".to_string(), BinaryValue::Dictionary(too_deep));
    let mut entry = BinaryDictionary::new();
    entry.insert("files".to_string(), BinaryValue::Dictionary(rule));
    let mut root = BinaryDictionary::new();
    root.insert("file_count".to_string(), BinaryValue::Dictionary(entry));

    assert_error_contains(
        Format::from_bytes(&binary_plist_bytes(BinaryValue::Dictionary(root))),
        &[
            "<root>.file_count.files.other",
            "exceeds the supported depth of 3",
        ],
    );
}

#[test]
fn writer_is_canonical_apple_only_and_preserves_all_categories() {
    let forms = BTreeMap::from([
        (PluralCategory::Zero, "No items".to_string()),
        (PluralCategory::One, "One item".to_string()),
        (PluralCategory::Two, "Two items".to_string()),
        (PluralCategory::Few, "%2$lld items (few)".to_string()),
        (PluralCategory::Many, "%2$lld items (many)".to_string()),
        (PluralCategory::Other, "%2$lld items".to_string()),
    ]);
    let mut resource = resource_entry(forms);
    resource.entries[0].custom = HashMap::from([
        (
            LOCALIZED_FORMAT_CUSTOM_KEY.to_string(),
            "%2$#@items@".to_string(),
        ),
        (VARIABLE_NAME_CUSTOM_KEY.to_string(), "items".to_string()),
        (VALUE_TYPE_CUSTOM_KEY.to_string(), "lld".to_string()),
    ]);

    let format = Format::try_from(resource).expect("representable Resource");
    let mut encoded = Vec::new();
    format.to_writer(&mut encoded).expect("serialize");
    let xml = String::from_utf8(encoded).expect("UTF-8");
    assert!(!xml.contains("Langcodec"));
    assert!(xml.contains("<string>%2$#@items@</string>"));
    assert!(xml.contains("<key>many</key>"));

    let round_tripped = Resource::from(Format::from_str(&xml).expect("reparse"));
    let Translation::Plural(plural) = &round_tripped.entries[0].value else {
        panic!("plural");
    };
    assert_eq!(plural.forms.len(), 6);
}

#[test]
fn resource_conversion_rejects_metadata_that_apple_cannot_preserve() {
    let forms = BTreeMap::from([(PluralCategory::Other, "%d files".to_string())]);

    let mut resource = resource_entry(forms.clone());
    resource.entries[0].comment = Some("translator note".to_string());
    assert!(
        Format::try_from(resource)
            .unwrap_err()
            .to_string()
            .contains("comment")
    );

    let mut resource = resource_entry(forms.clone());
    resource.entries[0].status = EntryStatus::NeedsReview;
    assert!(
        Format::try_from(resource)
            .unwrap_err()
            .to_string()
            .contains("status")
    );

    let mut resource = resource_entry(forms.clone());
    let Translation::Plural(plural) = &mut resource.entries[0].value else {
        unreachable!()
    };
    plural.id = "different_id".to_string();
    assert!(
        Format::try_from(resource)
            .unwrap_err()
            .to_string()
            .contains("plural identifier")
    );

    let mut resource = resource_entry(forms.clone());
    resource.entries[0]
        .custom
        .insert("developer_note".to_string(), "keep me".to_string());
    assert!(
        Format::try_from(resource)
            .unwrap_err()
            .to_string()
            .contains("developer_note")
    );

    let mut resource = resource_entry(forms);
    resource
        .metadata
        .custom
        .insert("project".to_string(), "Example".to_string());
    assert!(
        Format::try_from(resource)
            .unwrap_err()
            .to_string()
            .contains("project")
    );
}

#[test]
fn resource_domain_and_operational_metadata_are_container_only() {
    let mut resource = resource_entry(BTreeMap::from([(
        PluralCategory::Other,
        "%d files".to_string(),
    )]));
    resource.metadata.domain = "/tmp/en.lproj/Localizable.stringsdict".to_string();
    resource
        .metadata
        .custom
        .insert("source_language".to_string(), "en".to_string());
    resource
        .metadata
        .custom
        .insert("version".to_string(), "1.0".to_string());
    set_stringsdict_structure(&mut resource, "%#@files@", "files", "d");

    let format = Format::try_from(resource).expect("container metadata is intentionally ignored");
    assert_eq!(format.language, "en");
}

#[test]
fn resource_output_requires_explicit_selector_identity() {
    let resource = resource_entry(BTreeMap::from([
        (
            PluralCategory::One,
            "One download (%1$lld bytes)".to_string(),
        ),
        (
            PluralCategory::Other,
            "Downloads (%1$lld bytes)".to_string(),
        ),
    ]));
    let error = Format::try_from(resource).expect_err("bytes are not known to drive plurality");
    let message = error.to_string();
    assert!(message.contains("does not identify which printf argument drives plural selection"));
    assert!(message.contains(LOCALIZED_FORMAT_CUSTOM_KEY));
    assert!(message.contains(VARIABLE_NAME_CUSTOM_KEY));
    assert!(message.contains(VALUE_TYPE_CUSTOM_KEY));
    assert!(message.contains("explicitly to opt in"));

    let mut resource = resource_entry(BTreeMap::from([(
        PluralCategory::Other,
        "%1$@ across %2$lld runs (%3$.1f MB)".to_string(),
    )]));
    resource.entries[0]
        .custom
        .insert(VALUE_TYPE_CUSTOM_KEY.to_string(), "lld".to_string());
    resource.entries[0].custom.insert(
        LOCALIZED_FORMAT_CUSTOM_KEY.to_string(),
        "%2$#@count@".to_string(),
    );
    resource.entries[0]
        .custom
        .insert(VARIABLE_NAME_CUSTOM_KEY.to_string(), "count".to_string());
    assert_eq!(
        Format::try_from(resource)
            .expect("explicit selector type disambiguates other form arguments")
            .entries[0]
            .value_type,
        "lld"
    );

    let mut resource = resource_entry(BTreeMap::from([(
        PluralCategory::Other,
        "%lld files".to_string(),
    )]));
    resource.entries[0].custom.insert(
        LOCALIZED_FORMAT_CUSTOM_KEY.to_string(),
        "%#@count@".to_string(),
    );
    let partial_error =
        Format::try_from(resource).expect_err("partial structural metadata is unsafe");
    assert!(
        partial_error
            .to_string()
            .contains("all three Entry.custom keys")
    );
}

#[test]
fn nested_rule_selectors_inside_plural_forms_are_rejected() {
    for form in ["%#@gender@", "%2$#@device@"] {
        let xml = plist(&file_count_entry(
            "%#@files@",
            "d",
            &format!("<key>other</key><string>{form}</string>"),
        ));
        assert_error_contains(
            Format::from_str(&xml),
            &["file_count", "nested rule selector"],
        );
    }
}

#[test]
fn value_type_must_be_a_numeric_printf_token() {
    for invalid in ["", " ", "%d", "banana", "@", "llf"] {
        let xml = plist(&file_count_entry(
            "%#@files@",
            invalid,
            "<key>other</key><string>files</string>",
        ));
        assert_error_contains(
            Format::from_str(&xml),
            &["NSStringFormatValueTypeKey", "invalid"],
        );
    }
    for valid in ["d", "ld", "lld", "u", "llu", "f", "Lf"] {
        let xml = plist(&file_count_entry(
            "%#@files@",
            valid,
            "<key>other</key><string>files</string>",
        ));
        Format::from_str(&xml).expect("valid numeric token");
    }
}

#[test]
fn carriage_returns_round_trip_as_character_references() {
    let xml = plist(&file_count_entry(
        "%#@files@",
        "d",
        "<key>other</key><string>first&#13;second</string>",
    ));
    let format = Format::from_str(&xml).expect("entity CR");
    assert_eq!(
        format.entries[0].forms.get(&PluralCategory::Other),
        Some(&"first\rsecond".to_string())
    );
    let mut encoded = Vec::new();
    format.to_writer(&mut encoded).expect("serialize CR");
    let encoded = String::from_utf8(encoded).unwrap();
    assert!(encoded.contains("first&#13;second"));
    let reparsed = Format::from_str(&encoded).expect("reparse CR");
    assert_eq!(
        reparsed.entries[0].forms.get(&PluralCategory::Other),
        Some(&"first\rsecond".to_string())
    );

    let raw_cr = plist(&file_count_entry(
        "%#@files@",
        "d",
        "<key>other</key><string>first\r\nsecond\rthird</string>",
    ));
    let normalized = Format::from_str(&raw_cr).expect("raw XML line endings");
    assert_eq!(
        normalized.entries[0].forms.get(&PluralCategory::Other),
        Some(&"first\nsecond\nthird".to_string())
    );
}

#[test]
fn writer_rejects_xml_illegal_characters_and_is_deterministic() {
    let mut resource = resource_entry(BTreeMap::from([(
        PluralCategory::Other,
        "%d invalid \0 value".to_string(),
    )]));
    set_stringsdict_structure(&mut resource, "%#@count@", "count", "d");
    assert!(
        Format::try_from(resource.clone())
            .unwrap_err()
            .to_string()
            .contains("U+0000")
    );

    resource.entries[0].value = Translation::Plural(Plural {
        id: "file_count".to_string(),
        forms: BTreeMap::from([
            (PluralCategory::One, "%d one".to_string()),
            (PluralCategory::Other, "%d other".to_string()),
        ]),
    });
    resource.entries.push(Entry {
        id: "a_key".to_string(),
        value: Translation::Plural(Plural {
            id: "a_key".to_string(),
            forms: BTreeMap::from([(PluralCategory::Other, "%d other".to_string())]),
        }),
        comment: None,
        status: EntryStatus::Translated,
        custom: stringsdict_structure("%#@count@", "count", "d"),
    });
    let format = Format::try_from(resource).expect("valid resource");
    let mut first = Vec::new();
    format.to_writer(&mut first).expect("first");
    let first_text = String::from_utf8(first.clone()).unwrap();
    assert!(
        first_text.find("<key>a_key</key>").unwrap()
            < first_text.find("<key>file_count</key>").unwrap()
    );
    let mut second = Vec::new();
    Format::from_bytes(&first)
        .expect("reparse")
        .to_writer(&mut second)
        .expect("second");
    assert_eq!(first, second);
}

#[test]
fn singular_empty_multiple_variables_and_nonplural_rules_are_rejected() {
    for value in [
        Translation::Singular("Files".to_string()),
        Translation::Empty,
    ] {
        let mut resource = resource_entry(BTreeMap::from([(
            PluralCategory::Other,
            "files".to_string(),
        )]));
        resource.entries[0].value = value;
        assert!(Format::try_from(resource).is_err());
    }

    let count_rule = r#"<key>count</key><dict>
      <key>NSStringFormatSpecTypeKey</key><string>NSStringPluralRuleType</string>
      <key>NSStringFormatValueTypeKey</key><string>d</string>
      <key>other</key><string>%d items</string></dict>"#;
    let device_rule = r#"<key>device</key><dict>
      <key>NSStringFormatSpecTypeKey</key><string>NSStringPluralRuleType</string>
      <key>NSStringFormatValueTypeKey</key><string>d</string>
      <key>other</key><string>%d devices</string></dict>"#;
    let multiple = plist(&format!(
        r#"<key>inventory</key><dict>
           <key>NSStringLocalizedFormatKey</key><string>%#@count@</string>
           {count_rule}{device_rule}</dict>"#
    ));
    assert_error_contains(Format::from_str(&multiple), &["multiple variables"]);

    let select = plist(&canonical_entry(
        "salutation",
        "%#@gender@",
        "gender",
        "d",
        "NSStringGenderRuleType",
        "<key>other</key><string>They</string>",
    ));
    assert_error_contains(
        Format::from_str(&select),
        &["NSStringGenderRuleType", "unsupported rule type"],
    );
}

#[test]
fn malformed_plist_shapes_duplicates_and_excessive_depth_are_rejected() {
    let duplicate_form = plist(&file_count_entry(
        "%#@files@",
        "d",
        r#"<key>one</key><string>one</string>
           <key>one</key><string>again</string>
           <key>other</key><string>other</string>"#,
    ));
    assert_error_contains(Format::from_str(&duplicate_form), &["duplicate", "one"]);

    let unknown = plist(&file_count_entry(
        "%#@files@",
        "d",
        r#"<key>single</key><string>one</string>
           <key>other</key><string>other</string>"#,
    ));
    assert_error_contains(Format::from_str(&unknown), &["unknown plural category"]);

    assert_error_contains(
        Format::from_str(&plist("<key>orphaned</key>")),
        &["missing its value"],
    );
    assert_error_contains(
        Format::from_str(&plist("<key>messages</key><string>scalar</string>")),
        &["top-level value must be a dictionary"],
    );

    let nested = plist(
        r#"<key>inventory</key><dict>
           <key>NSStringLocalizedFormatKey</key><string>%#@count@</string>
           <key>count</key><dict>
             <key>NSStringFormatSpecTypeKey</key><string>NSStringPluralRuleType</string>
             <key>NSStringFormatValueTypeKey</key><string>d</string>
             <key>other</key><dict><key>deeper</key><dict/></dict>
           </dict></dict>"#,
    );
    assert_error_contains(Format::from_str(&nested), &["nesting", "depth"]);
}

#[test]
fn only_xml_whitespace_is_ignored_between_nodes() {
    let xml = plist(&format!(
        "{}\u{00a0}",
        file_count_entry("%#@files@", "d", "<key>other</key><string>files</string>")
    ));
    assert_error_contains(Format::from_str(&xml), &["expected", "key"]);
}

#[test]
fn xml_declaration_must_be_first_utf8_and_well_formed() {
    let body = file_count_entry(
        "%#@files@",
        "d",
        "<key>other</key><string>%d files</string>",
    );
    let valid = plist(&body);

    let leading_whitespace = format!(" \n{valid}");
    assert_error_contains(
        Format::from_str(&leading_whitespace),
        &["declaration", "first event"],
    );

    let wrong_first_attribute = valid.replacen(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<?xml encoding="UTF-8" version="1.0"?>"#,
        1,
    );
    assert!(
        Format::from_str(&wrong_first_attribute).is_err(),
        "version must be the first XML declaration attribute"
    );

    for encoding in ["UTF-16", "ISO-8859-1"] {
        let declared = valid.replacen(
            r#"encoding="UTF-8""#,
            &format!(r#"encoding="{encoding}""#),
            1,
        );
        assert_error_contains(Format::from_str(&declared), &["encoding", "UTF-8"]);
    }

    let xml_11 = valid.replacen(r#"version="1.0""#, r#"version="1.1""#, 1);
    assert_error_contains(Format::from_str(&xml_11), &["XML version", "1.0"]);

    let invalid_standalone = valid.replacen(
        r#"encoding="UTF-8"?>"#,
        r#"encoding="UTF-8" standalone="maybe"?>"#,
        1,
    );
    assert_error_contains(
        Format::from_str(&invalid_standalone),
        &["standalone", "yes", "no"],
    );
}

#[test]
fn utf8_bom_before_a_valid_declaration_is_accepted() {
    let xml = plist(&file_count_entry(
        "%#@files@",
        "d",
        "<key>other</key><string>%d files</string>",
    ));
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice(xml.as_bytes());

    Format::from_bytes(&bytes).expect("a UTF-8 BOM precedes, but is not before, the declaration");
}

#[test]
fn non_plist_doctype_is_rejected() {
    let valid = plist(&file_count_entry(
        "%#@files@",
        "d",
        "<key>other</key><string>%d files</string>",
    ));
    let invalid = valid.replacen(
        "\n<plist version=\"1.0\">",
        "\n<!DOCTYPE html>\n<plist version=\"1.0\">",
        1,
    );
    assert_error_contains(Format::from_str(&invalid), &["doctype", "Apple plist"]);
}
