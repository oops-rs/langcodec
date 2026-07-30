use std::{collections::BTreeMap, io::Cursor};

use langcodec::{
    Entry, EntryStatus, Error, Metadata, Plural, Resource, Translation,
    formats::{CSVFormat, MultiLanguageCSVRecord, MultiLanguageTSVRecord, TSVFormat},
    traits::Parser,
};

const HEADER: [&str; 16] = [
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

fn encoded(delimiter: u8, rows: &[Vec<&str>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut writer = csv::WriterBuilder::new()
            .delimiter(delimiter)
            .from_writer(&mut bytes);
        writer.write_record(HEADER).unwrap();
        for row in rows {
            writer.write_record(row).unwrap();
        }
        writer.flush().unwrap();
    }
    bytes
}

fn assert_data_mismatch(error: Error) {
    assert!(
        matches!(error, Error::DataMismatch(_)),
        "expected DataMismatch, got {error:?}"
    );
}

fn resource_row(custom: &str) -> Vec<&str> {
    vec![
        "v1", "resource", "0", "", "en", "", custom, "", "", "", "", "", "", "", "", "",
    ]
}

fn plural_entry_row() -> Vec<&'static str> {
    vec![
        "v1",
        "entry",
        "0",
        "0",
        "",
        "",
        "",
        "files",
        "plural",
        "files_id",
        "",
        "",
        "translated",
        "none",
        "",
        "[]",
    ]
}

fn plural_form_row(category: &'static str) -> Vec<&'static str> {
    vec![
        "v1",
        "plural_form",
        "0",
        "0",
        "",
        "",
        "",
        "",
        "",
        "",
        category,
        "%d files",
        "",
        "",
        "",
        "",
    ]
}

#[test]
fn malformed_basic_rows_and_duplicate_headers_are_rejected() {
    assert_data_mismatch(
        CSVFormat::from_reader(Cursor::new("key,en,fr\nwelcome,Welcome\n")).unwrap_err(),
    );
    assert_data_mismatch(
        TSVFormat::from_reader(Cursor::new("key\ten\ten\nwelcome\tOne\tTwo\n")).unwrap_err(),
    );
    assert_data_mismatch(
        CSVFormat::from_reader(Cursor::new("key,en\nsame,One\nsame,Two\n")).unwrap_err(),
    );
    assert_data_mismatch(
        CSVFormat::from_reader(Cursor::new("key, en\nwelcome,Welcome\n")).unwrap_err(),
    );
}

#[test]
fn malformed_or_unknown_extended_header_never_falls_back_to_basic() {
    let unknown = "__langcodec_extended_v2,row_kind,resource_index\nv2,resource,0\n";
    assert_data_mismatch(CSVFormat::from_reader(Cursor::new(unknown)).unwrap_err());

    let malformed = "__langcodec_extended_v1,row_kind,resource_index\nv1,resource,0\n";
    assert_data_mismatch(CSVFormat::from_reader(Cursor::new(malformed)).unwrap_err());
}

#[test]
fn extended_rows_must_follow_resource_and_entry_stream_order() {
    let entry_before_resource = vec![vec![
        "v1",
        "entry",
        "0",
        "0",
        "",
        "",
        "",
        "key",
        "singular",
        "",
        "",
        "value",
        "translated",
        "none",
        "",
        "[]",
    ]];
    assert_data_mismatch(
        CSVFormat::from_reader(Cursor::new(encoded(b',', &entry_before_resource))).unwrap_err(),
    );
    assert_data_mismatch(
        TSVFormat::from_reader(Cursor::new(encoded(b'\t', &entry_before_resource))).unwrap_err(),
    );

    let resource_gap = vec![vec![
        "v1", "resource", "1", "", "en", "", "[]", "", "", "", "", "", "", "", "", "",
    ]];
    assert_data_mismatch(
        CSVFormat::from_reader(Cursor::new(encoded(b',', &resource_gap))).unwrap_err(),
    );
}

#[test]
fn duplicate_plural_categories_and_custom_keys_are_rejected() {
    let duplicate_form = vec![
        resource_row("[]"),
        plural_entry_row(),
        plural_form_row("one"),
        plural_form_row("one"),
    ];
    assert_data_mismatch(
        CSVFormat::from_reader(Cursor::new(encoded(b',', &duplicate_form))).unwrap_err(),
    );
    assert_data_mismatch(
        TSVFormat::from_reader(Cursor::new(encoded(b'\t', &duplicate_form))).unwrap_err(),
    );

    let duplicate_custom = vec![resource_row(r#"[["same","one"],["same","two"]]"#)];
    assert_data_mismatch(
        CSVFormat::from_reader(Cursor::new(encoded(b',', &duplicate_custom))).unwrap_err(),
    );
}

#[test]
fn inapplicable_nonblank_fields_are_rejected() {
    let mut row = resource_row("[]");
    row[7] = "resource rows cannot carry keys";
    assert_data_mismatch(CSVFormat::from_reader(Cursor::new(encoded(b',', &[row]))).unwrap_err());
}

#[test]
fn plural_entries_without_forms_are_rejected_on_read_and_write() {
    let rows = vec![resource_row("[]"), plural_entry_row()];
    assert_data_mismatch(CSVFormat::from_reader(Cursor::new(encoded(b',', &rows))).unwrap_err());

    let resources = vec![Resource {
        metadata: Metadata {
            language: "en".to_string(),
            domain: String::new(),
            custom: Default::default(),
        },
        entries: vec![Entry {
            id: "files".to_string(),
            value: Translation::Plural(Plural {
                id: "files_id".to_string(),
                forms: BTreeMap::new(),
            }),
            comment: None,
            status: EntryStatus::Translated,
            custom: Default::default(),
        }],
    }];
    assert_data_mismatch(CSVFormat::try_from(resources).unwrap_err());
}

#[test]
fn public_record_mutation_cannot_hide_languages_or_duplicate_keys() {
    let mut csv =
        CSVFormat::from_reader(Cursor::new("key,en\nwelcome,Welcome\nbye,Goodbye\n")).unwrap();
    csv.records[0].add_translation("fr".to_string(), "Bienvenue".to_string());
    let csv_resources = Vec::<Resource>::try_from(csv.clone()).unwrap();
    assert_eq!(
        csv_resources
            .iter()
            .map(|resource| resource.metadata.language.as_str())
            .collect::<Vec<_>>(),
        vec!["en", "fr"]
    );
    assert_eq!(
        csv_resources[1].entries[1].value,
        Translation::Singular(String::new())
    );
    let mut csv_bytes = Vec::new();
    csv.to_writer(&mut csv_bytes).unwrap();
    let reparsed_csv = CSVFormat::from_reader(Cursor::new(csv_bytes)).unwrap();
    assert_eq!(
        Vec::<Resource>::try_from(reparsed_csv).unwrap(),
        csv_resources
    );

    let mut tsv =
        TSVFormat::from_reader(Cursor::new("key\ten\nwelcome\tWelcome\nbye\tGoodbye\n")).unwrap();
    tsv.records[0].add_translation("fr".to_string(), "Bienvenue".to_string());
    let tsv_resources = Vec::<Resource>::try_from(tsv.clone()).unwrap();
    assert_eq!(
        tsv_resources
            .iter()
            .map(|resource| resource.metadata.language.as_str())
            .collect::<Vec<_>>(),
        vec!["en", "fr"]
    );
    assert_eq!(
        tsv_resources[1].entries[1].value,
        Translation::Singular(String::new())
    );
    let mut tsv_bytes = Vec::new();
    tsv.to_writer(&mut tsv_bytes).unwrap();
    let reparsed_tsv = TSVFormat::from_reader(Cursor::new(tsv_bytes)).unwrap();
    assert_eq!(
        Vec::<Resource>::try_from(reparsed_tsv).unwrap(),
        tsv_resources
    );

    let mut csv_first = MultiLanguageCSVRecord::new("same".to_string());
    csv_first.add_translation("en".to_string(), "one".to_string());
    let mut csv_second = MultiLanguageCSVRecord::new("same".to_string());
    csv_second.add_translation("en".to_string(), "two".to_string());
    let csv_duplicates = CSVFormat::with_records(vec![csv_first, csv_second]);
    assert_data_mismatch(Vec::<Resource>::try_from(csv_duplicates).unwrap_err());

    let mut tsv_first = MultiLanguageTSVRecord::new("same".to_string());
    tsv_first.add_translation("en".to_string(), "one".to_string());
    let mut tsv_second = MultiLanguageTSVRecord::new("same".to_string());
    tsv_second.add_translation("en".to_string(), "two".to_string());
    let tsv_duplicates = TSVFormat::with_records(vec![tsv_first, tsv_second]);
    assert_data_mismatch(Vec::<Resource>::try_from(tsv_duplicates).unwrap_err());

    let mut invalid_language = MultiLanguageCSVRecord::new("welcome".to_string());
    invalid_language.add_translation(" en ".to_string(), "Welcome".to_string());
    assert_data_mismatch(
        CSVFormat::with_records(vec![invalid_language])
            .to_writer(Vec::new())
            .unwrap_err(),
    );
}
