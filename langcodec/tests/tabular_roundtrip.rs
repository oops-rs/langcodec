use std::{
    collections::{BTreeMap, HashMap},
    io::Cursor,
};

use langcodec::{
    Codec, Entry, EntryStatus, Metadata, Plural, PluralCategory, ReadOptions, Resource,
    Translation,
    formats::{CSVFormat, MultiLanguageCSVRecord, MultiLanguageTSVRecord, TSVFormat},
    traits::Parser,
};

fn custom(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn lossless_fixture() -> Vec<Resource> {
    let mut plural_forms = BTreeMap::new();
    plural_forms.insert(PluralCategory::One, String::new());
    plural_forms.insert(
        PluralCategory::Other,
        "%d files,\tquoted \"value\"\nnext line".to_string(),
    );

    vec![
        Resource {
            metadata: Metadata {
                language: "en-US".to_string(),
                domain: "App,Core".to_string(),
                custom: custom(&[
                    ("zeta", "last"),
                    ("alpha", "comma, tab\t quote\" newline\n"),
                ]),
            },
            entries: vec![
                Entry {
                    id: "same-key".to_string(),
                    value: Translation::Empty,
                    comment: None,
                    status: EntryStatus::New,
                    custom: custom(&[("empty", "preserved")]),
                },
                Entry {
                    id: "same-key".to_string(),
                    value: Translation::Singular(String::new()),
                    comment: Some(String::new()),
                    status: EntryStatus::NeedsReview,
                    custom: custom(&[("singular", "")]),
                },
                Entry {
                    id: "outdated".to_string(),
                    value: Translation::Singular("Old value".to_string()),
                    comment: Some("stale translation".to_string()),
                    status: EntryStatus::Stale,
                    custom: HashMap::new(),
                },
                Entry {
                    id: "files".to_string(),
                    value: Translation::Plural(Plural {
                        id: "files_id".to_string(),
                        forms: plural_forms,
                    }),
                    comment: Some("translator,\tnote\nline 2".to_string()),
                    status: EntryStatus::Translated,
                    custom: custom(&[("platform", "all")]),
                },
                Entry {
                    id: "brand".to_string(),
                    value: Translation::Singular("Langcodec".to_string()),
                    comment: None,
                    status: EntryStatus::DoNotTranslate,
                    custom: HashMap::new(),
                },
            ],
        },
        Resource {
            metadata: Metadata {
                language: "en-US".to_string(),
                domain: String::new(),
                custom: custom(&[("duplicate-language", "yes")]),
            },
            entries: Vec::new(),
        },
    ]
}

fn csv_round_trip(resources: &[Resource]) -> (String, Vec<Resource>) {
    let format = CSVFormat::try_from(resources.to_vec()).expect("encode resources as CSV");
    assert!(format.is_extended());

    let mut bytes = Vec::new();
    format.to_writer(&mut bytes).expect("write extended CSV");
    let text = String::from_utf8(bytes).expect("CSV is UTF-8");

    let parsed = CSVFormat::from_reader(Cursor::new(text.as_bytes())).expect("parse extended CSV");
    assert!(parsed.is_extended());
    let decoded = Vec::<Resource>::try_from(parsed).expect("decode extended CSV resources");
    (text, decoded)
}

fn tsv_round_trip(resources: &[Resource]) -> (String, Vec<Resource>) {
    let format = TSVFormat::try_from(resources.to_vec()).expect("encode resources as TSV");
    assert!(format.is_extended());

    let mut bytes = Vec::new();
    format.to_writer(&mut bytes).expect("write extended TSV");
    let text = String::from_utf8(bytes).expect("TSV is UTF-8");

    let parsed = TSVFormat::from_reader(Cursor::new(text.as_bytes())).expect("parse extended TSV");
    assert!(parsed.is_extended());
    let decoded = Vec::<Resource>::try_from(parsed).expect("decode extended TSV resources");
    (text, decoded)
}

#[test]
fn extended_csv_and_tsv_round_trip_the_entire_resource_model() {
    let expected = lossless_fixture();

    let (csv, csv_resources) = csv_round_trip(&expected);
    let (tsv, tsv_resources) = tsv_round_trip(&expected);

    assert_eq!(
        csv_resources, expected,
        "CSV must preserve exact model equality"
    );
    assert_eq!(
        tsv_resources, expected,
        "TSV must preserve exact model equality"
    );
    assert!(
        csv.starts_with("__langcodec_extended_v1,row_kind,"),
        "CSV must advertise the exact extended schema"
    );
    assert!(
        tsv.starts_with("__langcodec_extended_v1\trow_kind\t"),
        "TSV must advertise the exact extended schema"
    );
    assert!(csv.contains("plural_form"));
    assert!(tsv.contains("plural_form"));
}

#[test]
fn empty_and_singular_empty_remain_distinct() {
    let expected = lossless_fixture();
    let (_, decoded) = csv_round_trip(&expected);

    assert_eq!(decoded[0].entries[0].value, Translation::Empty);
    assert_eq!(
        decoded[0].entries[1].value,
        Translation::Singular(String::new())
    );
    assert_eq!(decoded[0].entries[0].comment, None);
    assert_eq!(decoded[0].entries[1].comment, Some(String::new()));
}

#[test]
fn extended_output_is_independent_of_hash_map_insertion_order() {
    let left = lossless_fixture();
    let mut right = left.clone();
    right[0].metadata.custom.clear();
    right[0].metadata.custom.insert(
        "alpha".to_string(),
        "comma, tab\t quote\" newline\n".to_string(),
    );
    right[0]
        .metadata
        .custom
        .insert("zeta".to_string(), "last".to_string());

    assert_eq!(left, right, "fixtures differ only in map insertion order");

    let (left_csv, _) = csv_round_trip(&left);
    let (right_csv, _) = csv_round_trip(&right);
    let (left_tsv, _) = tsv_round_trip(&left);
    let (right_tsv, _) = tsv_round_trip(&right);

    assert_eq!(left_csv, right_csv);
    assert_eq!(left_tsv, right_tsv);
}

#[test]
fn basic_schema_preserves_declared_language_order_and_source_language() {
    let csv = "key,fr,en\nwelcome,Bienvenue,Welcome\nempty,,\n";
    let parsed = CSVFormat::from_reader(Cursor::new(csv)).expect("parse legacy CSV");
    assert!(!parsed.is_extended());
    let resources = Vec::<Resource>::try_from(parsed.clone()).expect("decode legacy CSV");

    assert_eq!(
        resources
            .iter()
            .map(|resource| resource.metadata.language.as_str())
            .collect::<Vec<_>>(),
        vec!["fr", "en"]
    );
    for resource in &resources {
        assert_eq!(
            resource
                .metadata
                .custom
                .get("source_language")
                .map(String::as_str),
            Some("fr")
        );
    }

    let round_trip = CSVFormat::try_from(resources).expect("re-encode basic CSV");
    assert!(!round_trip.is_extended());
    let mut bytes = Vec::new();
    round_trip.to_writer(&mut bytes).expect("write basic CSV");
    assert_eq!(String::from_utf8(bytes).unwrap(), csv);
}

#[test]
fn header_only_basic_tables_preserve_declared_languages() {
    let csv_input = "key,fr,en\n";
    let csv = CSVFormat::from_reader(Cursor::new(csv_input)).expect("parse header-only CSV");
    assert!(!csv.is_extended());
    let mut csv_bytes = Vec::new();
    csv.to_writer(&mut csv_bytes)
        .expect("write header-only CSV");
    assert_eq!(String::from_utf8(csv_bytes).unwrap(), csv_input);
    let csv_resources = Vec::<Resource>::try_from(csv).expect("decode header-only CSV");

    let tsv_input = "key\tfr\ten\n";
    let tsv = TSVFormat::from_reader(Cursor::new(tsv_input)).expect("parse header-only TSV");
    assert!(!tsv.is_extended());
    let mut tsv_bytes = Vec::new();
    tsv.to_writer(&mut tsv_bytes)
        .expect("write header-only TSV");
    assert_eq!(String::from_utf8(tsv_bytes).unwrap(), tsv_input);
    let tsv_resources = Vec::<Resource>::try_from(tsv).expect("decode header-only TSV");

    for resources in [&csv_resources, &tsv_resources] {
        assert_eq!(
            resources
                .iter()
                .map(|resource| resource.metadata.language.as_str())
                .collect::<Vec<_>>(),
            vec!["fr", "en"]
        );
        assert!(resources.iter().all(|resource| resource.entries.is_empty()));
        assert!(resources.iter().all(|resource| {
            resource.metadata.domain.is_empty()
                && resource.metadata.custom
                    == custom(&[("source_language", "fr"), ("version", "1.0")])
        }));
    }

    let csv = CSVFormat::try_from(csv_resources).expect("re-encode header-only CSV resources");
    assert!(!csv.is_extended());
    let mut csv_bytes = Vec::new();
    csv.to_writer(&mut csv_bytes)
        .expect("write re-encoded header-only CSV");
    assert_eq!(String::from_utf8(csv_bytes).unwrap(), csv_input);

    let tsv = TSVFormat::try_from(tsv_resources).expect("re-encode header-only TSV resources");
    assert!(!tsv.is_extended());
    let mut tsv_bytes = Vec::new();
    tsv.to_writer(&mut tsv_bytes)
        .expect("write re-encoded header-only TSV");
    assert_eq!(String::from_utf8(tsv_bytes).unwrap(), tsv_input);
}

#[test]
fn parsed_basic_tables_follow_public_language_removals_and_renames() {
    let csv_input = "key,fr,en\nwelcome,Bienvenue,Welcome\nbye,Au revoir,Goodbye\n";
    let mut csv = CSVFormat::from_reader(Cursor::new(csv_input)).unwrap();
    for record in csv.get_records_mut() {
        record.translations.remove("fr");
    }
    let mut csv_bytes = Vec::new();
    csv.to_writer(&mut csv_bytes).unwrap();
    assert_eq!(
        String::from_utf8(csv_bytes).unwrap(),
        "key,en\nwelcome,Welcome\nbye,Goodbye\n"
    );

    let mut csv = CSVFormat::from_reader(Cursor::new(csv_input)).unwrap();
    for record in csv.get_records_mut() {
        let french = record.translations.remove("fr").unwrap();
        let english = record.translations.remove("en").unwrap();
        record.translations.insert("es".to_string(), french);
        record.translations.insert("de".to_string(), english);
    }
    let mut csv_bytes = Vec::new();
    csv.to_writer(&mut csv_bytes).unwrap();
    assert_eq!(
        String::from_utf8(csv_bytes).unwrap(),
        "key,de,es\nwelcome,Welcome,Bienvenue\nbye,Goodbye,Au revoir\n"
    );

    let tsv_input = "key\tfr\ten\nwelcome\tBienvenue\tWelcome\nbye\tAu revoir\tGoodbye\n";
    let mut tsv = TSVFormat::from_reader(Cursor::new(tsv_input)).unwrap();
    for record in tsv.get_records_mut() {
        record.translations.remove("fr");
    }
    let mut tsv_bytes = Vec::new();
    tsv.to_writer(&mut tsv_bytes).unwrap();
    assert_eq!(
        String::from_utf8(tsv_bytes).unwrap(),
        "key\ten\nwelcome\tWelcome\nbye\tGoodbye\n"
    );

    let mut tsv = TSVFormat::from_reader(Cursor::new(tsv_input)).unwrap();
    for record in tsv.get_records_mut() {
        let french = record.translations.remove("fr").unwrap();
        let english = record.translations.remove("en").unwrap();
        record.translations.insert("es".to_string(), french);
        record.translations.insert("de".to_string(), english);
    }
    let mut tsv_bytes = Vec::new();
    tsv.to_writer(&mut tsv_bytes).unwrap();
    assert_eq!(
        String::from_utf8(tsv_bytes).unwrap(),
        "key\tde\tes\nwelcome\tWelcome\tBienvenue\nbye\tGoodbye\tAu revoir\n"
    );
}

#[test]
fn truly_empty_tabular_input_remains_empty() {
    let csv = CSVFormat::from_reader(Cursor::new("")).expect("parse empty CSV");
    let mut csv_bytes = Vec::new();
    csv.to_writer(&mut csv_bytes).expect("write empty CSV");
    assert!(csv_bytes.is_empty());
    assert!(Vec::<Resource>::try_from(csv).unwrap().is_empty());

    let tsv = TSVFormat::from_reader(Cursor::new("")).expect("parse empty TSV");
    let mut tsv_bytes = Vec::new();
    tsv.to_writer(&mut tsv_bytes).expect("write empty TSV");
    assert!(tsv_bytes.is_empty());
    assert!(Vec::<Resource>::try_from(tsv).unwrap().is_empty());
}

#[test]
fn noncanonical_empty_resource_metadata_uses_extended_schema() {
    let resources = vec![Resource {
        metadata: Metadata {
            language: "en".to_string(),
            domain: String::new(),
            custom: HashMap::new(),
        },
        entries: Vec::new(),
    }];

    let csv = CSVFormat::try_from(resources.clone()).expect("encode empty resource as CSV");
    assert!(csv.is_extended());
    let mut csv_bytes = Vec::new();
    csv.to_writer(&mut csv_bytes).unwrap();
    let decoded_csv =
        Vec::<Resource>::try_from(CSVFormat::from_reader(Cursor::new(csv_bytes)).unwrap()).unwrap();
    assert_eq!(decoded_csv, resources);

    let tsv = TSVFormat::try_from(resources.clone()).expect("encode empty resource as TSV");
    assert!(tsv.is_extended());
    let mut tsv_bytes = Vec::new();
    tsv.to_writer(&mut tsv_bytes).unwrap();
    let decoded_tsv =
        Vec::<Resource>::try_from(TSVFormat::from_reader(Cursor::new(tsv_bytes)).unwrap()).unwrap();
    assert_eq!(decoded_tsv, resources);
}

#[test]
fn legacy_headerless_and_wide_files_remain_readable() {
    let headerless = CSVFormat::from_reader(Cursor::new("hello,Hello\nbye,Goodbye\n")).unwrap();
    let headerless_resources = Vec::<Resource>::try_from(headerless).unwrap();
    assert_eq!(headerless_resources.len(), 1);
    assert_eq!(headerless_resources[0].metadata.language, "default");

    let wide = TSVFormat::from_reader(Cursor::new("key\ten\tfr\nhello\tHello\tBonjour\n")).unwrap();
    let wide_resources = Vec::<Resource>::try_from(wide).unwrap();
    assert_eq!(
        wide_resources
            .iter()
            .map(|resource| resource.metadata.language.as_str())
            .collect::<Vec<_>>(),
        vec!["en", "fr"]
    );
    assert!(
        wide_resources.iter().all(|resource| {
            resource
                .metadata
                .custom
                .get("source_language")
                .map(String::as_str)
                == Some("en")
        }),
        "TSV source language must be the first declared language"
    );
}

#[test]
fn programmatic_basic_records_keep_the_simple_wide_output() {
    let mut csv_record = MultiLanguageCSVRecord::new("welcome".to_string());
    csv_record.add_translation("fr".to_string(), "Bienvenue".to_string());
    csv_record.add_translation("en".to_string(), "Welcome".to_string());
    let mut csv_bytes = Vec::new();
    CSVFormat::with_records(vec![csv_record])
        .to_writer(&mut csv_bytes)
        .unwrap();
    assert_eq!(
        String::from_utf8(csv_bytes).unwrap(),
        "key,en,fr\nwelcome,Welcome,Bienvenue\n"
    );

    let mut tsv_record = MultiLanguageTSVRecord::new("welcome".to_string());
    tsv_record.add_translation("fr".to_string(), "Bienvenue".to_string());
    tsv_record.add_translation("en".to_string(), "Welcome".to_string());
    let mut tsv_bytes = Vec::new();
    TSVFormat::with_records(vec![tsv_record])
        .to_writer(&mut tsv_bytes)
        .unwrap();
    assert_eq!(
        String::from_utf8(tsv_bytes).unwrap(),
        "key\ten\tfr\nwelcome\tWelcome\tBienvenue\n"
    );
}

#[test]
fn domain_and_recognized_format_metadata_force_lossless_extended_output() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("Localizable.strings");
    std::fs::write(&input, "\"welcome\" = \"Welcome\";\n").unwrap();

    let mut codec = Codec::new();
    codec
        .read_file_by_type(
            &input,
            langcodec::formats::FormatType::Strings(Some("en".to_string())),
        )
        .unwrap();
    assert_eq!(codec.resources[0].metadata.domain, "Localizable");
    assert_eq!(
        codec.resources[0]
            .metadata
            .custom
            .get("format")
            .map(String::as_str),
        Some("strings")
    );

    let expected = codec.resources;

    let csv = CSVFormat::try_from(expected.clone()).unwrap();
    assert!(csv.is_extended());
    let mut csv_bytes = Vec::new();
    csv.to_writer(&mut csv_bytes).unwrap();
    let parsed_csv = CSVFormat::from_reader(Cursor::new(csv_bytes)).unwrap();
    assert_eq!(Vec::<Resource>::try_from(parsed_csv).unwrap(), expected);

    let tsv = TSVFormat::try_from(expected.clone()).unwrap();
    assert!(tsv.is_extended());
    let mut tsv_bytes = Vec::new();
    tsv.to_writer(&mut tsv_bytes).unwrap();
    let parsed_tsv = TSVFormat::from_reader(Cursor::new(tsv_bytes)).unwrap();
    assert_eq!(Vec::<Resource>::try_from(parsed_tsv).unwrap(), expected);
}

#[test]
fn recognized_format_custom_is_preserved_even_without_a_domain() {
    let resources = vec![Resource {
        metadata: Metadata {
            language: "en".to_string(),
            domain: String::new(),
            custom: custom(&[
                ("format", "strings"),
                ("source_language", "en"),
                ("version", "1.0"),
            ]),
        },
        entries: vec![Entry {
            id: "welcome".to_string(),
            value: Translation::Singular("Welcome".to_string()),
            comment: None,
            status: EntryStatus::Translated,
            custom: HashMap::new(),
        }],
    }];

    let csv = CSVFormat::try_from(resources.clone()).unwrap();
    assert!(csv.is_extended());
    let mut csv_bytes = Vec::new();
    csv.to_writer(&mut csv_bytes).unwrap();
    assert_eq!(
        Vec::<Resource>::try_from(CSVFormat::from_reader(Cursor::new(csv_bytes)).unwrap()).unwrap(),
        resources
    );

    let tsv = TSVFormat::try_from(resources.clone()).unwrap();
    assert!(tsv.is_extended());
    let mut tsv_bytes = Vec::new();
    tsv.to_writer(&mut tsv_bytes).unwrap();
    assert_eq!(
        Vec::<Resource>::try_from(TSVFormat::from_reader(Cursor::new(tsv_bytes)).unwrap()).unwrap(),
        resources
    );
}

#[test]
fn singular_resources_use_extended_when_basic_would_invent_metadata() {
    let resources = vec![Resource {
        metadata: Metadata {
            language: "en".to_string(),
            domain: String::new(),
            custom: HashMap::new(),
        },
        entries: vec![Entry {
            id: "welcome".to_string(),
            value: Translation::Singular("Welcome".to_string()),
            comment: None,
            status: EntryStatus::Translated,
            custom: HashMap::new(),
        }],
    }];

    let format = CSVFormat::try_from(resources.clone()).unwrap();
    assert!(
        format.is_extended(),
        "basic decoding would inject source_language and version"
    );
    let mut bytes = Vec::new();
    format.to_writer(&mut bytes).unwrap();
    let parsed = CSVFormat::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(Vec::<Resource>::try_from(parsed).unwrap(), resources);
}

#[test]
fn noncanonical_resource_languages_fall_back_to_lossless_extended_schema() {
    for language in ["", " en "] {
        let resources = vec![Resource {
            metadata: Metadata {
                language: language.to_string(),
                domain: String::new(),
                custom: custom(&[("source_language", language), ("version", "1.0")]),
            },
            entries: vec![Entry {
                id: "welcome".to_string(),
                value: Translation::Singular("Welcome".to_string()),
                comment: None,
                status: EntryStatus::Translated,
                custom: HashMap::new(),
            }],
        }];

        let csv = CSVFormat::try_from(resources.clone()).unwrap();
        assert!(csv.is_extended());
        let mut csv_bytes = Vec::new();
        csv.to_writer(&mut csv_bytes).unwrap();
        let parsed_csv = CSVFormat::from_reader(Cursor::new(csv_bytes)).unwrap();
        assert_eq!(Vec::<Resource>::try_from(parsed_csv).unwrap(), resources);

        let tsv = TSVFormat::try_from(resources.clone()).unwrap();
        assert!(tsv.is_extended());
        let mut tsv_bytes = Vec::new();
        tsv.to_writer(&mut tsv_bytes).unwrap();
        let parsed_tsv = TSVFormat::from_reader(Cursor::new(tsv_bytes)).unwrap();
        assert_eq!(Vec::<Resource>::try_from(parsed_tsv).unwrap(), resources);
    }
}

#[test]
fn codec_and_builder_preserve_extended_metadata_but_still_decorate_basic_tables() {
    let expected = lossless_fixture();
    let directory = tempfile::tempdir().unwrap();
    let csv_path = directory.path().join("catalog.csv");
    let tsv_path = directory.path().join("catalog.tsv");
    CSVFormat::try_from(expected.clone())
        .unwrap()
        .write_to(&csv_path)
        .unwrap();
    TSVFormat::try_from(expected.clone())
        .unwrap()
        .write_to(&tsv_path)
        .unwrap();

    let mut codec = Codec::new();
    codec
        .read_file_by_type(&csv_path, langcodec::formats::FormatType::CSV)
        .unwrap();
    assert_eq!(
        codec.resources, expected,
        "default Codec reads must not decorate extended metadata"
    );

    let inferred_builder = Codec::builder().add_file(&tsv_path).unwrap().build();
    assert_eq!(
        inferred_builder.resources, expected,
        "inferred CodecBuilder reads must preserve extended metadata"
    );

    let explicit_builder = Codec::builder()
        .add_file_with_format(&csv_path, langcodec::formats::FormatType::CSV)
        .unwrap()
        .build();
    assert_eq!(
        explicit_builder.resources, expected,
        "explicit CodecBuilder reads must preserve extended metadata"
    );

    let basic_path = directory.path().join("basic.csv");
    std::fs::write(&basic_path, "key,en\nwelcome,Welcome\n").unwrap();
    let mut basic_codec = Codec::new();
    basic_codec
        .read_file_by_type(&basic_path, langcodec::formats::FormatType::CSV)
        .unwrap();
    assert_eq!(basic_codec.resources[0].metadata.domain, "basic");
    assert_eq!(
        basic_codec.resources[0]
            .metadata
            .custom
            .get("format")
            .map(String::as_str),
        Some("csv")
    );
}

#[test]
fn explicit_provenance_is_applied_without_legacy_metadata_decoration() {
    let expected = lossless_fixture();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("catalog.csv");
    CSVFormat::try_from(expected.clone())
        .unwrap()
        .write_to(&path)
        .unwrap();

    let mut codec = Codec::new();
    codec
        .read_file_by_type_with_options(
            &path,
            langcodec::formats::FormatType::CSV,
            &ReadOptions::new().with_provenance(true),
        )
        .unwrap();

    assert_eq!(
        codec.resources[0].metadata.domain,
        expected[0].metadata.domain
    );
    assert_eq!(
        codec.resources[0].metadata.custom.get("alpha"),
        expected[0].metadata.custom.get("alpha")
    );
    assert!(!codec.resources[0].metadata.custom.contains_key("format"));
    assert!(
        codec.resources[0]
            .metadata
            .custom
            .contains_key("langcodec.provenance.source_path")
    );
}
