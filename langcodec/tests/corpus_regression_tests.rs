use langcodec::types::Translation;
use langcodec::{Codec, convert_auto};
use std::path::{Path, PathBuf};

#[derive(Clone)]
struct ExpectedValue {
    language: &'static str,
    key: &'static str,
    value: &'static str,
}

struct ParseCase {
    name: &'static str,
    input_relative_path: &'static str,
    lang_hint: Option<&'static str>,
    expected_values: Vec<ExpectedValue>,
}

struct ConvertCase {
    name: &'static str,
    input_relative_path: &'static str,
    output_file_name: &'static str,
    output_lang_hint: Option<&'static str>,
    expected_values: Vec<ExpectedValue>,
}

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join("data")
        .join("lib")
        .join("corpus")
}

fn expected_en_stable_values() -> Vec<ExpectedValue> {
    vec![
        ExpectedValue {
            language: "en",
            key: "welcome_message",
            value: "Hello, World!",
        },
        ExpectedValue {
            language: "en",
            key: "xml_entities",
            value: "Use <tag> & value",
        },
        ExpectedValue {
            language: "en",
            key: "comma_text",
            value: "alpha, beta, gamma",
        },
        ExpectedValue {
            language: "en",
            key: "accent_text",
            value: "Café crème brûlée",
        },
    ]
}

fn expected_fr_stable_values() -> Vec<ExpectedValue> {
    vec![
        ExpectedValue {
            language: "fr",
            key: "welcome_message",
            value: "Bonjour, le monde !",
        },
        ExpectedValue {
            language: "fr",
            key: "xml_entities",
            value: "Utiliser <tag> & valeur",
        },
        ExpectedValue {
            language: "fr",
            key: "comma_text",
            value: "alpha, bêta, gamma",
        },
        ExpectedValue {
            language: "fr",
            key: "accent_text",
            value: "Café crème brûlée",
        },
    ]
}

fn read_codec(path: &Path, lang_hint: Option<&str>) -> Codec {
    let mut codec = Codec::new();
    codec
        .read_file_by_extension(path, lang_hint.map(|l| l.to_string()))
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    codec
}

fn assert_expected_values(codec: &Codec, expected_values: &[ExpectedValue], case_name: &str) {
    for expected in expected_values {
        let entry = codec
            .find_entry(expected.key, expected.language)
            .unwrap_or_else(|| {
                panic!(
                    "{case_name}: missing key '{}' for language '{}'",
                    expected.key, expected.language
                )
            });

        match &entry.value {
            Translation::Singular(actual) => {
                assert_eq!(
                    actual, expected.value,
                    "{case_name}: value mismatch for {}:{}",
                    expected.language, expected.key
                );
            }
            other => panic!(
                "{case_name}: expected singular value for {}:{}, got {:?}",
                expected.language, expected.key, other
            ),
        }
    }
}

#[test]
fn parse_edge_case_corpora_table_driven() {
    let root = corpus_root();
    let mut csv_and_tsv_expected = expected_en_stable_values();
    csv_and_tsv_expected.extend(expected_fr_stable_values());

    let parse_cases = vec![
        ParseCase {
            name: "strings corpus parse",
            input_relative_path: "en.lproj/Localizable.strings",
            lang_hint: None,
            expected_values: {
                let mut expected = expected_en_stable_values();
                expected.push(ExpectedValue {
                    language: "en",
                    key: "quoted_text",
                    value: "He said \\\"Hello\\\"",
                });
                expected.push(ExpectedValue {
                    language: "en",
                    key: "apostrophe_text",
                    value: "Don't stop",
                });
                expected
            },
        },
        ParseCase {
            name: "android corpus parse",
            input_relative_path: "values-en/strings.xml",
            lang_hint: None,
            expected_values: {
                let mut expected = expected_en_stable_values();
                expected.push(ExpectedValue {
                    language: "en",
                    key: "quoted_text",
                    value: "He said \"Hello\"",
                });
                expected.push(ExpectedValue {
                    language: "en",
                    key: "apostrophe_text",
                    value: "Don't stop",
                });
                expected
            },
        },
        ParseCase {
            name: "csv corpus parse",
            input_relative_path: "corpus.csv",
            lang_hint: None,
            expected_values: csv_and_tsv_expected.clone(),
        },
        ParseCase {
            name: "tsv corpus parse",
            input_relative_path: "corpus.tsv",
            lang_hint: None,
            expected_values: csv_and_tsv_expected,
        },
    ];

    for case in parse_cases {
        let input_path = root.join(case.input_relative_path);
        let codec = read_codec(&input_path, case.lang_hint);
        assert_expected_values(&codec, &case.expected_values, case.name);
    }
}

#[test]
fn convert_edge_case_corpora_table_driven() {
    let root = corpus_root();
    let output_dir = tempfile::tempdir().expect("create temp output dir");

    let csv_single_lang_expected = expected_en_stable_values();

    let convert_cases = vec![
        ConvertCase {
            name: "strings -> android",
            input_relative_path: "en.lproj/Localizable.strings",
            output_file_name: "from_strings.xml",
            output_lang_hint: Some("en"),
            expected_values: expected_en_stable_values(),
        },
        ConvertCase {
            name: "strings -> xcstrings",
            input_relative_path: "en.lproj/Localizable.strings",
            output_file_name: "from_strings.xcstrings",
            output_lang_hint: None,
            expected_values: expected_en_stable_values(),
        },
        ConvertCase {
            name: "strings -> csv",
            input_relative_path: "en.lproj/Localizable.strings",
            output_file_name: "from_strings.csv",
            output_lang_hint: None,
            expected_values: csv_single_lang_expected.clone(),
        },
        ConvertCase {
            name: "strings -> tsv",
            input_relative_path: "en.lproj/Localizable.strings",
            output_file_name: "from_strings.tsv",
            output_lang_hint: None,
            expected_values: csv_single_lang_expected,
        },
        ConvertCase {
            name: "android -> strings",
            input_relative_path: "values-en/strings.xml",
            output_file_name: "from_android.strings",
            output_lang_hint: Some("en"),
            expected_values: expected_en_stable_values(),
        },
        ConvertCase {
            name: "android -> xcstrings",
            input_relative_path: "values-en/strings.xml",
            output_file_name: "from_android.xcstrings",
            output_lang_hint: None,
            expected_values: {
                let mut expected = expected_en_stable_values();
                expected.push(ExpectedValue {
                    language: "en",
                    key: "apostrophe_text",
                    value: "Don't stop",
                });
                expected
            },
        },
        ConvertCase {
            name: "csv -> xcstrings",
            input_relative_path: "corpus.csv",
            output_file_name: "from_csv.xcstrings",
            output_lang_hint: None,
            expected_values: {
                let mut expected = expected_en_stable_values();
                expected.extend(expected_fr_stable_values());
                expected
            },
        },
        ConvertCase {
            name: "tsv -> xcstrings",
            input_relative_path: "corpus.tsv",
            output_file_name: "from_tsv.xcstrings",
            output_lang_hint: None,
            expected_values: {
                let mut expected = expected_en_stable_values();
                expected.extend(expected_fr_stable_values());
                expected
            },
        },
    ];

    for case in convert_cases {
        let input_path = root.join(case.input_relative_path);
        let output_path = output_dir.path().join(case.output_file_name);

        convert_auto(&input_path, &output_path).unwrap_or_else(|e| {
            panic!(
                "{}: conversion failed from {} to {}: {}",
                case.name,
                input_path.display(),
                output_path.display(),
                e
            )
        });

        let codec = read_codec(&output_path, case.output_lang_hint);
        assert_expected_values(&codec, &case.expected_values, case.name);
    }
}
