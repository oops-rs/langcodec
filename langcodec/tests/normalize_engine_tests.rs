use langcodec::{
    Codec, ErrorCode,
    formats::stringsdict::{
        LOCALIZED_FORMAT_CUSTOM_KEY, VALUE_TYPE_CUSTOM_KEY, VARIABLE_NAME_CUSTOM_KEY,
    },
    normalize::{KeyStyle, NormalizeOptions},
    types::{Entry, EntryStatus, Metadata, Resource, Translation},
};
use std::collections::HashMap;

#[test]
fn normalize_sorts_entries_and_is_idempotent() {
    let mut codec = Codec {
        resources: vec![Resource {
            metadata: Metadata {
                language: "en".to_string(),
                domain: "Localizable".to_string(),
                custom: HashMap::new(),
            },
            entries: vec![
                Entry {
                    id: "z_key".to_string(),
                    value: Translation::Singular("Z".to_string()),
                    comment: None,
                    status: EntryStatus::Translated,
                    custom: HashMap::new(),
                },
                Entry {
                    id: "a_key".to_string(),
                    value: Translation::Singular("A".to_string()),
                    comment: None,
                    status: EntryStatus::Translated,
                    custom: HashMap::new(),
                },
            ],
        }],
    };

    let report1 = langcodec::normalize::normalize_codec(&mut codec, &Default::default()).unwrap();
    let ids: Vec<_> = codec.resources[0]
        .entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    assert_eq!(ids, vec!["a_key", "z_key"]);
    assert!(report1.changed);

    let report2 = langcodec::normalize::normalize_codec(&mut codec, &Default::default()).unwrap();
    assert!(!report2.changed);
}

#[test]
fn normalize_applies_placeholder_normalization_by_default() {
    let mut codec = Codec {
        resources: vec![Resource {
            metadata: Metadata {
                language: "en".to_string(),
                domain: "Localizable".to_string(),
                custom: HashMap::new(),
            },
            entries: vec![Entry {
                id: "summary".to_string(),
                value: Translation::Singular("%@ has %ld items".to_string()),
                comment: None,
                status: EntryStatus::Translated,
                custom: HashMap::new(),
            }],
        }],
    };

    let report = langcodec::normalize::normalize_codec(&mut codec, &Default::default()).unwrap();
    let value = match &codec.resources[0].entries[0].value {
        Translation::Singular(value) => value.clone(),
        _ => unreachable!("test fixture uses singular translation"),
    };

    assert_eq!(value, "%s has %d items");
    assert!(report.changed);
}

#[test]
fn normalize_errors_on_key_style_collision_after_transform() {
    let mut codec = Codec {
        resources: vec![Resource {
            metadata: Metadata {
                language: "en".to_string(),
                domain: "Localizable".to_string(),
                custom: HashMap::new(),
            },
            entries: vec![
                Entry {
                    id: "welcome-title".to_string(),
                    value: Translation::Singular("Welcome".to_string()),
                    comment: None,
                    status: EntryStatus::Translated,
                    custom: HashMap::new(),
                },
                Entry {
                    id: "welcome_title".to_string(),
                    value: Translation::Singular("Welcome again".to_string()),
                    comment: None,
                    status: EntryStatus::Translated,
                    custom: HashMap::new(),
                },
            ],
        }],
    };

    let options = NormalizeOptions {
        normalize_placeholders: false,
        key_style: KeyStyle::Snake,
    };

    let error = langcodec::normalize::normalize_codec(&mut codec, &options).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("collision"));
    assert!(message.contains("welcome-title"));
    assert!(message.contains("welcome_title"));
}

#[test]
fn normalize_preserves_unicode_letters_in_key_style_transform() {
    let mut codec = Codec {
        resources: vec![Resource {
            metadata: Metadata {
                language: "en".to_string(),
                domain: "Localizable".to_string(),
                custom: HashMap::new(),
            },
            entries: vec![Entry {
                id: "CrèmeBrûlée_你好_weißKey".to_string(),
                value: Translation::Singular("Dessert".to_string()),
                comment: None,
                status: EntryStatus::Translated,
                custom: HashMap::new(),
            }],
        }],
    };

    let options = NormalizeOptions {
        normalize_placeholders: false,
        key_style: KeyStyle::Snake,
    };

    let report = langcodec::normalize::normalize_codec(&mut codec, &options).unwrap();
    assert_eq!(
        codec.resources[0].entries[0].id,
        "crème_brûlée_你好_weiß_key"
    );
    assert!(report.changed);
}

#[test]
fn normalize_keeps_codec_unchanged_when_key_style_collision_occurs() {
    let mut codec = Codec {
        resources: vec![
            Resource {
                metadata: Metadata {
                    language: "en".to_string(),
                    domain: "Primary".to_string(),
                    custom: HashMap::new(),
                },
                entries: vec![
                    Entry {
                        id: "zKey".to_string(),
                        value: Translation::Singular("Z".to_string()),
                        comment: None,
                        status: EntryStatus::Translated,
                        custom: HashMap::new(),
                    },
                    Entry {
                        id: "aKey".to_string(),
                        value: Translation::Singular("A".to_string()),
                        comment: None,
                        status: EntryStatus::Translated,
                        custom: HashMap::new(),
                    },
                ],
            },
            Resource {
                metadata: Metadata {
                    language: "en".to_string(),
                    domain: "Colliding".to_string(),
                    custom: HashMap::new(),
                },
                entries: vec![
                    Entry {
                        id: "welcome-title".to_string(),
                        value: Translation::Singular("Welcome".to_string()),
                        comment: None,
                        status: EntryStatus::Translated,
                        custom: HashMap::new(),
                    },
                    Entry {
                        id: "welcome_title".to_string(),
                        value: Translation::Singular("Welcome again".to_string()),
                        comment: None,
                        status: EntryStatus::Translated,
                        custom: HashMap::new(),
                    },
                ],
            },
        ],
    };
    let original_resources = codec.resources.clone();

    let options = NormalizeOptions {
        normalize_placeholders: false,
        key_style: KeyStyle::Snake,
    };

    let error = langcodec::normalize::normalize_codec(&mut codec, &options).unwrap_err();
    assert!(error.to_string().contains("collision"));
    assert_eq!(codec.resources, original_resources);
}

fn stringsdict_structural_custom() -> HashMap<String, String> {
    HashMap::from([
        (
            LOCALIZED_FORMAT_CUSTOM_KEY.to_string(),
            "%#@count@".to_string(),
        ),
        (VARIABLE_NAME_CUSTOM_KEY.to_string(), "count".to_string()),
        (VALUE_TYPE_CUSTOM_KEY.to_string(), "ld".to_string()),
    ])
}

fn codec_with_stringsdict_structural_entry(custom: HashMap<String, String>) -> Codec {
    Codec {
        resources: vec![Resource {
            metadata: Metadata {
                language: "en".to_string(),
                domain: "Localizable".to_string(),
                custom: HashMap::new(),
            },
            entries: vec![
                Entry {
                    id: "ordinaryKey".to_string(),
                    value: Translation::Singular("Hello %@".to_string()),
                    comment: None,
                    status: EntryStatus::Translated,
                    custom: HashMap::new(),
                },
                Entry {
                    id: "itemCount".to_string(),
                    value: Translation::Singular("%ld items".to_string()),
                    comment: None,
                    status: EntryStatus::Translated,
                    custom,
                },
            ],
        }],
    }
}

#[test]
fn normalize_rejects_placeholder_rewrite_for_stringsdict_structural_entry_transactionally() {
    let mut codec = codec_with_stringsdict_structural_entry(stringsdict_structural_custom());
    let original = codec.clone();
    let options = NormalizeOptions {
        normalize_placeholders: true,
        key_style: KeyStyle::None,
    };

    let error = langcodec::normalize::normalize_codec(&mut codec, &options).unwrap_err();

    assert_eq!(error.error_code(), ErrorCode::UnsupportedFormat);
    assert!(
        error
            .to_string()
            .contains("Disable placeholder normalization")
    );
    assert_eq!(codec.resources, original.resources);
}

#[test]
fn normalize_rejects_key_rewrite_for_stringsdict_structural_entry_transactionally() {
    let mut codec = codec_with_stringsdict_structural_entry(stringsdict_structural_custom());
    let original = codec.clone();
    let options = NormalizeOptions {
        normalize_placeholders: false,
        key_style: KeyStyle::Snake,
    };

    let error = langcodec::normalize::normalize_codec(&mut codec, &options).unwrap_err();

    assert_eq!(error.error_code(), ErrorCode::UnsupportedFormat);
    assert!(error.to_string().contains("use KeyStyle::None"));
    assert_eq!(codec.resources, original.resources);
}

#[test]
fn normalize_rejects_partial_stringsdict_structural_metadata_conservatively() {
    let partial = HashMap::from([(VALUE_TYPE_CUSTOM_KEY.to_string(), "ld".to_string())]);
    let mut codec = codec_with_stringsdict_structural_entry(partial);
    let original = codec.clone();

    let error = langcodec::normalize::normalize_codec(&mut codec, &NormalizeOptions::default())
        .unwrap_err();

    assert_eq!(error.error_code(), ErrorCode::DataMismatch);
    assert!(error.to_string().contains("partial .stringsdict"));
    assert!(error.to_string().contains(LOCALIZED_FORMAT_CUSTOM_KEY));
    assert!(error.to_string().contains(VARIABLE_NAME_CUSTOM_KEY));
    assert_eq!(codec.resources, original.resources);
}

#[test]
fn normalize_allows_sort_only_for_stringsdict_structural_entries() {
    let mut codec = codec_with_stringsdict_structural_entry(stringsdict_structural_custom());
    let options = NormalizeOptions {
        normalize_placeholders: false,
        key_style: KeyStyle::None,
    };

    let report = langcodec::normalize::normalize_codec(&mut codec, &options).unwrap();

    assert!(report.changed);
    let ids = codec.resources[0]
        .entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["itemCount", "ordinaryKey"]);
    assert_eq!(
        codec.resources[0].entries[0].value,
        Translation::Singular("%ld items".to_string())
    );
}
