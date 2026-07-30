use std::{
    collections::{BTreeMap, HashMap},
    fs,
};

use langcodec::{
    Error, convert, convert_auto, convert_auto_with_normalization, convert_resources_to_format,
    convert_with_normalization,
    formats::{
        AndroidStringsFormat, FormatType, StringsdictFormat, XcstringsFormat,
        stringsdict::{
            LOCALIZED_FORMAT_CUSTOM_KEY, VALUE_TYPE_CUSTOM_KEY, VARIABLE_NAME_CUSTOM_KEY,
        },
    },
    traits::Parser,
    types::{Entry, EntryStatus, Metadata, Plural, PluralCategory, Resource, Translation},
};

fn write_stringsdict_fixture(path: &std::path::Path) {
    fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture directory");
    fs::write(
        path,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>file_count</key>
  <dict>
    <key>NSStringLocalizedFormatKey</key>
    <string>%#@files@</string>
    <key>files</key>
    <dict>
      <key>NSStringFormatSpecTypeKey</key>
      <string>NSStringPluralRuleType</string>
      <key>NSStringFormatValueTypeKey</key>
      <string>ld</string>
      <key>one</key><string>One file</string>
      <key>other</key><string>%ld files</string>
    </dict>
  </dict>
</dict>
</plist>
"#,
    )
    .expect("stringsdict fixture");
}

fn assert_normalization_is_unsupported(result: Result<(), Error>) {
    match result.expect_err("stringsdict normalization must be rejected") {
        Error::UnsupportedFormat(message) => {
            assert!(message.contains(".stringsdict"));
            assert!(message.contains("disable normalization"));
        }
        error => panic!("expected UnsupportedFormat, got {error}"),
    }
}

fn plural_resource(language: &str, forms: BTreeMap<PluralCategory, String>) -> Resource {
    Resource {
        metadata: Metadata {
            language: language.to_string(),
            domain: String::new(),
            custom: HashMap::from([
                ("source_language".to_string(), "en".to_string()),
                ("version".to_string(), "1.0".to_string()),
            ]),
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

fn with_stringsdict_structure(
    mut resource: Resource,
    localized_format: &str,
    variable_name: &str,
    value_type: &str,
) -> Resource {
    resource.entries[0].custom = HashMap::from([
        (
            LOCALIZED_FORMAT_CUSTOM_KEY.to_string(),
            localized_format.to_string(),
        ),
        (
            VARIABLE_NAME_CUSTOM_KEY.to_string(),
            variable_name.to_string(),
        ),
        (VALUE_TYPE_CUSTOM_KEY.to_string(), value_type.to_string()),
    ]);
    resource
}

fn plural_forms(resource: &Resource) -> &BTreeMap<PluralCategory, String> {
    let entry = resource
        .find_entry("file_count")
        .expect("file_count entry must exist");
    let Translation::Plural(plural) = &entry.value else {
        panic!("file_count must remain plural");
    };
    &plural.forms
}

#[test]
fn android_to_stringsdict_rejects_missing_selector_identity_without_touching_output() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let android_input = temporary.path().join("values-en").join("strings.xml");
    let output = temporary
        .path()
        .join("en.lproj")
        .join("Localizable.stringsdict");
    fs::create_dir_all(android_input.parent().expect("input parent")).expect("input directory");
    fs::write(
        &android_input,
        r#"<?xml version="1.0" encoding="utf-8"?>
<resources>
  <plurals name="file_count">
    <item quantity="one">One download (%lld bytes)</item>
    <item quantity="other">Downloads (%lld bytes)</item>
  </plurals>
</resources>
"#,
    )
    .expect("Android fixture");

    let error = convert_auto(&android_input, &output)
        .expect_err("Android has no plural selector identity metadata");
    let message = error.to_string();
    assert!(message.contains("does not identify which printf argument drives plural selection"));
    assert!(message.contains(LOCALIZED_FORMAT_CUSTOM_KEY));
    assert!(!output.exists(), "rejection must not create output");

    fs::create_dir_all(output.parent().expect("output parent")).expect("output directory");
    fs::write(&output, "existing output").expect("output sentinel");
    convert_auto(&android_input, &output)
        .expect_err("unsafe conversion must fail before replacing output");
    assert_eq!(
        fs::read_to_string(&output).expect("unchanged output"),
        "existing output"
    );
}

#[test]
fn stringsdict_to_android_preserves_all_plural_categories() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let stringsdict = temporary
        .path()
        .join("ar.lproj")
        .join("Localizable.stringsdict");
    let android_output = temporary.path().join("values-ar").join("strings.xml");
    fs::create_dir_all(stringsdict.parent().expect("input parent")).expect("input directory");
    fs::write(
        &stringsdict,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>file_count</key><dict>
    <key>NSStringLocalizedFormatKey</key><string>%#@files@</string>
    <key>files</key><dict>
      <key>NSStringFormatSpecTypeKey</key><string>NSStringPluralRuleType</string>
      <key>NSStringFormatValueTypeKey</key><string>d</string>
      <key>zero</key><string>لا ملفات</string>
      <key>one</key><string>ملف واحد</string>
      <key>two</key><string>ملفان</string>
      <key>few</key><string>%d ملفات</string>
      <key>many</key><string>%d ملفًا</string>
      <key>other</key><string>%d ملف</string>
    </dict>
  </dict>
</dict></plist>
"#,
    )
    .expect("stringsdict fixture");

    convert_auto(&stringsdict, &android_output).expect("stringsdict -> Android");
    let mut parsed_android =
        AndroidStringsFormat::read_from(&android_output).expect("parse converted Android");
    parsed_android.language = "ar".to_string();
    let converted = Resource::from(parsed_android);

    let expected = BTreeMap::from([
        (PluralCategory::Zero, "لا ملفات".to_string()),
        (PluralCategory::One, "ملف واحد".to_string()),
        (PluralCategory::Two, "ملفان".to_string()),
        (PluralCategory::Few, "%d ملفات".to_string()),
        (PluralCategory::Many, "%d ملفًا".to_string()),
        (PluralCategory::Other, "%d ملف".to_string()),
    ]);
    assert_eq!(plural_forms(&converted), &expected);
    assert_eq!(converted.entries[0].comment, None);
}

#[test]
fn explicit_selector_metadata_enables_language_selection_and_xcstrings_round_trip() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let stringsdict = temporary
        .path()
        .join("fr.lproj")
        .join("Localizable.stringsdict");
    let xcstrings_output = temporary.path().join("French.xcstrings");

    let english_forms = BTreeMap::from([
        (PluralCategory::One, "One file".to_string()),
        (PluralCategory::Other, "%lld files".to_string()),
    ]);
    let french_forms = BTreeMap::from([
        (PluralCategory::One, "Un fichier".to_string()),
        (PluralCategory::Many, "%lld fichiers".to_string()),
        (PluralCategory::Other, "%lld fichier".to_string()),
    ]);
    convert_resources_to_format(
        vec![
            with_stringsdict_structure(
                plural_resource("en", english_forms),
                "%#@files@",
                "files",
                "lld",
            ),
            with_stringsdict_structure(
                plural_resource("fr", french_forms.clone()),
                "%#@files@",
                "files",
                "lld",
            ),
        ],
        stringsdict.to_str().expect("UTF-8 path"),
        FormatType::Stringsdict(Some("fr".to_string())),
    )
    .expect("explicit metadata permits French stringsdict output");

    let selected = Resource::from(
        StringsdictFormat::read_from(&stringsdict).expect("parse selected stringsdict"),
    );
    assert_eq!(plural_forms(&selected), &french_forms);
    assert_eq!(selected.entries[0].comment, None);

    convert(
        &stringsdict,
        FormatType::Stringsdict(Some("fr".to_string())),
        &xcstrings_output,
        FormatType::Xcstrings,
    )
    .expect("stringsdict -> xcstrings");
    let resources = Vec::<Resource>::try_from(
        XcstringsFormat::read_from(&xcstrings_output).expect("parse round-tripped xcstrings"),
    )
    .expect("decode round-tripped xcstrings");
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].metadata.language, "fr");
    assert_eq!(plural_forms(&resources[0]), &french_forms);
}

#[test]
fn stringsdict_output_requires_an_unambiguous_resource_language() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let output = temporary.path().join("Localizable.stringsdict");
    let resources = vec![
        with_stringsdict_structure(
            plural_resource(
                "en",
                BTreeMap::from([(PluralCategory::Other, "%d files".to_string())]),
            ),
            "%#@files@",
            "files",
            "d",
        ),
        with_stringsdict_structure(
            plural_resource(
                "fr",
                BTreeMap::from([(PluralCategory::Other, "%d fichiers".to_string())]),
            ),
            "%#@files@",
            "files",
            "d",
        ),
    ];

    let error = convert_resources_to_format(
        resources.clone(),
        output.to_str().expect("UTF-8 path"),
        FormatType::Stringsdict(None),
    )
    .expect_err("multi-language stringsdict output must be rejected");
    let message = error.to_string();
    assert!(message.contains("single-language"));
    assert!(message.contains("--output-lang"));

    convert_resources_to_format(
        resources,
        output.to_str().expect("UTF-8 path"),
        FormatType::Stringsdict(Some("fr".to_string())),
    )
    .expect("explicit language selects one resource");
    let selected =
        Resource::from(StringsdictFormat::read_from(&output).expect("selected stringsdict"));
    assert_eq!(
        plural_forms(&selected).get(&PluralCategory::Other),
        Some(&"%d fichiers".to_string())
    );
}

#[test]
fn cross_format_conversion_rejects_wrappers_and_preserves_bare_rule_forms() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let input = temporary
        .path()
        .join("en.lproj")
        .join("Localizable.stringsdict");
    let output = temporary.path().join("values-en").join("strings.xml");
    fs::create_dir_all(input.parent().expect("input parent")).expect("input directory");

    let stringsdict = |localized_format: &str| {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>file_count</key>
  <dict>
    <key>NSStringLocalizedFormatKey</key>
    <string>{localized_format}</string>
    <key>files</key>
    <dict>
      <key>NSStringFormatSpecTypeKey</key>
      <string>NSStringPluralRuleType</string>
      <key>NSStringFormatValueTypeKey</key>
      <string>d</string>
      <key>one</key><string>Exactly one file</string>
      <key>other</key><string>%d files remain</string>
    </dict>
  </dict>
</dict>
</plist>
"#
        )
    };

    fs::write(&input, stringsdict("There are %#@files@ remaining")).expect("wrapper fixture");
    let error = convert_auto(&input, &output).expect_err("wrapper text would be lost");
    assert!(error.to_string().contains("bare"));

    fs::write(&input, stringsdict("%#@files@")).expect("canonical fixture");
    convert_auto(&input, &output).expect("bare selector is cross-format safe");
    let android = fs::read_to_string(&output).expect("Android output");
    assert!(android.contains("Exactly one file"));
    assert!(android.contains("%d files remain"));
    assert!(!android.contains("There are"));
}

#[test]
fn normalization_rejects_stringsdict_input_without_creating_or_truncating_output() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let input = temporary
        .path()
        .join("en.lproj")
        .join("Localizable.stringsdict");
    let output = temporary.path().join("values-en").join("strings.xml");
    write_stringsdict_fixture(&input);
    fs::create_dir_all(output.parent().expect("output parent")).expect("output directory");

    assert_normalization_is_unsupported(convert_with_normalization(
        &input,
        FormatType::Stringsdict(Some("en".to_string())),
        &output,
        FormatType::AndroidStrings(Some("en".to_string())),
        true,
    ));
    assert!(!output.exists(), "rejection must not create the output");

    fs::write(&output, "existing output").expect("output sentinel");
    assert_normalization_is_unsupported(convert_with_normalization(
        &input,
        FormatType::Stringsdict(Some("en".to_string())),
        &output,
        FormatType::AndroidStrings(Some("en".to_string())),
        true,
    ));
    assert_eq!(
        fs::read_to_string(&output).expect("unchanged output"),
        "existing output"
    );
}

#[test]
fn normalization_rejects_stringsdict_output_without_creating_or_truncating_output() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let input = temporary.path().join("values-en").join("strings.xml");
    let output = temporary
        .path()
        .join("en.lproj")
        .join("Localizable.stringsdict");
    fs::create_dir_all(input.parent().expect("input parent")).expect("input directory");
    fs::create_dir_all(output.parent().expect("output parent")).expect("output directory");
    fs::write(
        &input,
        r#"<?xml version="1.0" encoding="utf-8"?>
<resources>
  <plurals name="file_count">
    <item quantity="one">One file</item>
    <item quantity="other">%d files</item>
  </plurals>
</resources>
"#,
    )
    .expect("Android fixture");

    assert_normalization_is_unsupported(convert_auto_with_normalization(&input, &output, true));
    assert!(!output.exists(), "rejection must not create the output");

    fs::write(&output, "existing output").expect("output sentinel");
    assert_normalization_is_unsupported(convert_auto_with_normalization(&input, &output, true));
    assert_eq!(
        fs::read_to_string(&output).expect("unchanged output"),
        "existing output"
    );
}

#[test]
fn stringsdict_conversion_remains_supported_when_normalization_is_disabled() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let input = temporary
        .path()
        .join("en.lproj")
        .join("Localizable.stringsdict");
    let output = temporary
        .path()
        .join("en.lproj")
        .join("RoundTrip.stringsdict");
    write_stringsdict_fixture(&input);

    convert_with_normalization(
        &input,
        FormatType::Stringsdict(Some("en".to_string())),
        &output,
        FormatType::Stringsdict(Some("en".to_string())),
        false,
    )
    .expect("normalization-disabled stringsdict conversion");

    let contents = fs::read_to_string(&output).expect("converted stringsdict");
    assert!(contents.contains("<string>ld</string>"));
    assert!(contents.contains("%ld files"));
}
