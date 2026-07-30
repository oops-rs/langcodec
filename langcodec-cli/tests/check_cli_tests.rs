use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

fn langcodec_cmd() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("langcodec"))
}

fn json_output(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "JSON mode must not emit non-JSON diagnostics; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not one byte-pure JSON document: {error}; stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn write_clean_xcstrings(path: &Path) {
    fs::write(
        path,
        r#"{
  "sourceLanguage": "en",
  "version": "1.0",
  "strings": {
    "welcome": {
      "localizations": {
        "en": {
          "stringUnit": {
            "state": "translated",
            "value": "Welcome"
          }
        }
      }
    }
  }
}
"#,
    )
    .unwrap();
}

const EXTENDED_HEADER: [&str; 16] = [
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

fn extended_csv(rows: &[[&str; 16]]) -> String {
    let mut output = EXTENDED_HEADER.join(",");
    output.push('\n');
    for row in rows {
        output.push_str(&row.join(","));
        output.push('\n');
    }
    output
}

#[test]
fn check_clean_input_succeeds_with_human_output() {
    let temp_dir = TempDir::new().unwrap();
    let input = temp_dir.path().join("Localizable.xcstrings");
    write_clean_xcstrings(&input);

    let output = langcodec_cmd()
        .args(["check", "--inputs", input.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("OK: checked 1 file(s); no issues found"));
}

#[test]
fn check_json_reports_plural_issue_and_exits_nonzero() {
    let temp_dir = TempDir::new().unwrap();
    let input = temp_dir.path().join("strings.xml");
    fs::write(
        &input,
        r#"<resources>
  <plurals name="apples">
    <item quantity="other">%d apples</item>
  </plurals>
</resources>
"#,
    )
    .unwrap();

    let output = langcodec_cmd()
        .args([
            "check",
            "--inputs",
            input.to_str().unwrap(),
            "--lang",
            "en",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report = json_output(&output);
    assert_eq!(report["valid"], false);
    assert_eq!(report["files_checked"], 1);
    assert_eq!(report["issues"].as_array().unwrap().len(), 1);
    assert_eq!(report["issues"][0]["kind"], "plural");
    assert!(
        report["issues"][0]["message"]
            .as_str()
            .unwrap()
            .contains("missing [one]")
    );
}

#[test]
fn check_reports_structurally_empty_resource() {
    let temp_dir = TempDir::new().unwrap();
    let input = temp_dir.path().join("empty.strings");
    fs::write(&input, "").unwrap();

    let output = langcodec_cmd()
        .args([
            "check",
            "--inputs",
            input.to_str().unwrap(),
            "--lang",
            "en",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report = json_output(&output);
    assert_eq!(report["issues"].as_array().unwrap().len(), 1);
    assert_eq!(report["issues"][0]["kind"], "structure");
}

#[test]
fn check_glob_expansion_is_deterministic() {
    let temp_dir = TempDir::new().unwrap();
    let first = temp_dir.path().join("a.xcstrings");
    let second = temp_dir.path().join("b.xcstrings");
    write_clean_xcstrings(&first);
    write_clean_xcstrings(&second);
    let pattern = format!("{}/*.xcstrings", temp_dir.path().display());

    let first_run = langcodec_cmd()
        .args(["check", "--inputs", &pattern, "--json"])
        .output()
        .unwrap();
    let second_run = langcodec_cmd()
        .args(["check", "--inputs", &pattern, "--json"])
        .output()
        .unwrap();

    assert!(first_run.status.success());
    assert_eq!(first_run.stdout, second_run.stdout);
    let report = json_output(&first_run);
    assert_eq!(report["files_checked"], 2);
    assert_eq!(report["issues"].as_array().unwrap().len(), 0);
}

#[test]
fn check_glob_includes_gitignored_inputs() {
    let temp_dir = TempDir::new().unwrap();
    fs::create_dir(temp_dir.path().join(".git")).unwrap();
    fs::write(temp_dir.path().join(".gitignore"), "ignored.xcstrings\n").unwrap();
    let ignored = temp_dir.path().join("ignored.xcstrings");
    write_clean_xcstrings(&ignored);
    let pattern = format!("{}/*.xcstrings", temp_dir.path().display());

    let output = langcodec_cmd()
        .args(["check", "--inputs", &pattern, "--json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "ignored input was not checked: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report = json_output(&output);
    assert_eq!(report["files_checked"], 1);
    assert_eq!(report["issues"].as_array().unwrap().len(), 0);
}

#[test]
fn check_glob_preserves_brace_alternation() {
    let temp_dir = TempDir::new().unwrap();
    let strings = temp_dir.path().join("Localizable.strings");
    let catalog = temp_dir.path().join("Localizable.xcstrings");
    // Keep the two catalogs structurally independent: this test proves brace
    // expansion, not duplicate `(locale, domain, key)` detection.
    fs::write(&strings, r#""standalone" = "Standalone";"#).unwrap();
    write_clean_xcstrings(&catalog);
    let pattern = format!("{}/*.{{strings,xcstrings}}", temp_dir.path().display());

    let output = langcodec_cmd()
        .args(["check", "--inputs", &pattern, "--lang", "en", "--json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "brace glob failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(json_output(&output)["files_checked"], 2);
}

#[test]
fn check_invalid_glob_is_an_input_issue() {
    let temp_dir = TempDir::new().unwrap();
    let pattern = format!("{}/[unterminated", temp_dir.path().display());

    let output = langcodec_cmd()
        .args(["check", "--inputs", &pattern, "--json"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report = json_output(&output);
    assert_eq!(report["files_checked"], 0);
    let issues = report["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0]["kind"], "input");
    assert_eq!(issues[0]["path"], pattern);
    assert!(
        issues[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("glob pattern"))
    );
}

#[test]
fn check_mixed_matched_missing_and_unmatched_inputs_are_all_retained() {
    let temp_dir = TempDir::new().unwrap();
    let existing = temp_dir.path().join("existing.xcstrings");
    let missing = temp_dir.path().join("missing.xcstrings");
    let unmatched = format!("{}/*.strings", temp_dir.path().display());
    write_clean_xcstrings(&existing);

    let output = langcodec_cmd()
        .args([
            "check",
            "--inputs",
            existing.to_str().unwrap(),
            missing.to_str().unwrap(),
            &unmatched,
            "--continue-on-error",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report = json_output(&output);
    assert_eq!(report["files_checked"], 3);
    let issues = report["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 2);
    let issue_paths: Vec<_> = issues
        .iter()
        .map(|issue| issue["path"].as_str().unwrap())
        .collect();
    assert!(issue_paths.contains(&missing.to_str().unwrap()));
    assert!(issue_paths.contains(&unmatched.as_str()));
}

#[test]
fn check_continue_on_error_inspects_remaining_files_once() {
    let temp_dir = TempDir::new().unwrap();
    let malformed = temp_dir.path().join("a-malformed.xcstrings");
    let plural = temp_dir.path().join("b-plural.xml");
    fs::write(&malformed, "{not json").unwrap();
    fs::write(
        &plural,
        r#"<resources>
  <plurals name="items">
    <item quantity="other">%d items</item>
  </plurals>
</resources>
"#,
    )
    .unwrap();

    let without_continue = langcodec_cmd()
        .args([
            "check",
            "--inputs",
            plural.to_str().unwrap(),
            malformed.to_str().unwrap(),
            "--lang",
            "en",
            "--json",
        ])
        .output()
        .unwrap();
    let stopped_report = json_output(&without_continue);
    assert_eq!(stopped_report["files_checked"], 1);
    assert_eq!(stopped_report["issues"].as_array().unwrap().len(), 1);
    assert_eq!(stopped_report["issues"][0]["kind"], "parse");

    let with_continue = langcodec_cmd()
        .args([
            "check",
            "--inputs",
            plural.to_str().unwrap(),
            malformed.to_str().unwrap(),
            "--lang",
            "en",
            "--continue-on-error",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!with_continue.status.success());
    let report = json_output(&with_continue);
    assert_eq!(report["files_checked"], 2);
    let issues = report["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0]["path"], malformed.to_str().unwrap());
    assert_eq!(issues[0]["kind"], "parse");
    assert_eq!(issues[1]["path"], plural.to_str().unwrap());
    assert_eq!(issues[1]["kind"], "plural");
}

#[test]
fn check_non_strict_mode_rejects_placeholders_across_input_files() {
    let temp_dir = TempDir::new().unwrap();
    let en_dir = temp_dir.path().join("en.lproj");
    let fr_dir = temp_dir.path().join("fr.lproj");
    fs::create_dir_all(&en_dir).unwrap();
    fs::create_dir_all(&fr_dir).unwrap();
    let en = en_dir.join("Localizable.strings");
    let fr = fr_dir.join("Localizable.strings");
    fs::write(&en, r#""greeting" = "Hello %@";"#).unwrap();
    fs::write(&fr, r#""greeting" = "Bonjour";"#).unwrap();

    let output = langcodec_cmd()
        .args([
            "check",
            "--inputs",
            fr.to_str().unwrap(),
            en.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report = json_output(&output);
    let issues = report["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0]["kind"], "placeholder");
    assert!(issues[0]["message"].as_str().unwrap().contains("greeting"));
    let paths = issues[0]["paths"].as_array().unwrap();
    assert_eq!(paths.len(), 2);
    assert!(paths.iter().any(|path| path == en.to_str().unwrap()));
    assert!(paths.iter().any(|path| path == fr.to_str().unwrap()));
}

#[test]
fn check_does_not_compare_same_key_from_unrelated_domains() {
    let temp_dir = TempDir::new().unwrap();
    let first_dir = temp_dir.path().join("module-a/en.lproj");
    let second_dir = temp_dir.path().join("module-b/en.lproj");
    fs::create_dir_all(&first_dir).unwrap();
    fs::create_dir_all(&second_dir).unwrap();
    let auth = first_dir.join("Auth.strings");
    let profile = second_dir.join("Profile.strings");
    fs::write(&auth, r#""title" = "Account %@";"#).unwrap();
    fs::write(&profile, r#""title" = "Profil";"#).unwrap();

    let output = langcodec_cmd()
        .args([
            "check",
            "--inputs",
            profile.to_str().unwrap(),
            auth.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "unrelated domains produced a false placeholder issue: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report = json_output(&output);
    assert_eq!(report["valid"], true);
    assert_eq!(report["issues"].as_array().unwrap().len(), 0);
}

#[test]
fn check_accepts_plural_languages_with_different_category_counts() {
    let temp_dir = TempDir::new().unwrap();
    let en_dir = temp_dir.path().join("values-en");
    let ar_dir = temp_dir.path().join("values-ar");
    fs::create_dir_all(&en_dir).unwrap();
    fs::create_dir_all(&ar_dir).unwrap();
    let en = en_dir.join("strings.xml");
    let ar = ar_dir.join("strings.xml");
    fs::write(
        &en,
        r#"<resources>
  <plurals name="items">
    <item quantity="one">One item</item>
    <item quantity="other">%d items</item>
  </plurals>
</resources>
"#,
    )
    .unwrap();
    fs::write(
        &ar,
        r#"<resources>
  <plurals name="items">
    <item quantity="zero">No items</item>
    <item quantity="one">One item</item>
    <item quantity="two">Two items</item>
    <item quantity="few">%d items</item>
    <item quantity="many">%d items</item>
    <item quantity="other">%d items</item>
  </plurals>
</resources>
"#,
    )
    .unwrap();

    let output = langcodec_cmd()
        .args([
            "--strict",
            "check",
            "--inputs",
            ar.to_str().unwrap(),
            en.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "valid plural signatures failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(json_output(&output)["valid"], true);
}

#[test]
fn check_accepts_plural_forms_with_independent_placeholder_signatures() {
    let temp_dir = TempDir::new().unwrap();
    let values = temp_dir.path().join("values-en");
    fs::create_dir_all(&values).unwrap();
    let input = values.join("strings.xml");
    fs::write(
        &input,
        r#"<resources>
  <plurals name="items">
    <item quantity="one">One item</item>
    <item quantity="other">%d items</item>
  </plurals>
</resources>
"#,
    )
    .unwrap();

    let output = langcodec_cmd()
        .args(["check", "--inputs", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "valid independent plural branches failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report = json_output(&output);
    assert_eq!(report["valid"], true);
    assert_eq!(report["issues"].as_array().unwrap().len(), 0);
}

#[test]
fn check_three_language_placeholder_json_is_byte_stable() {
    let temp_dir = TempDir::new().unwrap();
    let de_dir = temp_dir.path().join("de.lproj");
    let en_dir = temp_dir.path().join("en.lproj");
    let fr_dir = temp_dir.path().join("fr.lproj");
    fs::create_dir_all(&de_dir).unwrap();
    fs::create_dir_all(&en_dir).unwrap();
    fs::create_dir_all(&fr_dir).unwrap();
    let de = de_dir.join("Localizable.strings");
    let en = en_dir.join("Localizable.strings");
    let fr = fr_dir.join("Localizable.strings");
    fs::write(&de, r#""value" = "Wert %d";"#).unwrap();
    fs::write(&en, r#""value" = "Value %@";"#).unwrap();
    fs::write(&fr, r#""value" = "Valeur";"#).unwrap();

    let run = || {
        langcodec_cmd()
            .args([
                "check",
                "--inputs",
                fr.to_str().unwrap(),
                de.to_str().unwrap(),
                en.to_str().unwrap(),
                "--continue-on-error",
                "--json",
            ])
            .output()
            .unwrap()
    };
    let first = run();
    let second = run();

    assert!(!first.status.success());
    assert_eq!(first.stdout, second.stdout);
    let report = json_output(&first);
    let issues = report["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 3);
    assert!(issues.iter().all(|issue| issue["kind"] == "placeholder"));
    assert!(issues.iter().all(|issue| {
        issue["paths"]
            .as_array()
            .is_some_and(|paths| paths.len() == 2)
    }));
}

#[test]
fn check_reports_duplicate_entries_for_the_same_language() {
    let temp_dir = TempDir::new().unwrap();
    let first_dir = temp_dir.path().join("module-a/en.lproj");
    let second_dir = temp_dir.path().join("module-b/en.lproj");
    fs::create_dir_all(&first_dir).unwrap();
    fs::create_dir_all(&second_dir).unwrap();
    let first = first_dir.join("Localizable.strings");
    let second = second_dir.join("Localizable.strings");
    fs::write(&first, r#""value" = "Value %@";"#).unwrap();
    fs::write(&second, r#""value" = "Other %@";"#).unwrap();

    let output = langcodec_cmd()
        .args([
            "check",
            "--inputs",
            first.to_str().unwrap(),
            second.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report = json_output(&output);
    let issues = report["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0]["kind"], "placeholder");
    assert!(
        issues[0]["message"]
            .as_str()
            .unwrap()
            .contains("duplicate entries")
    );
    assert_eq!(issues[0]["paths"].as_array().unwrap().len(), 2);
}

#[test]
fn check_normalizes_positional_placeholder_argument_identity() {
    let temp_dir = TempDir::new().unwrap();
    let en_dir = temp_dir.path().join("en.lproj");
    let fr_dir = temp_dir.path().join("fr.lproj");
    fs::create_dir_all(&en_dir).unwrap();
    fs::create_dir_all(&fr_dir).unwrap();
    let en = en_dir.join("Localizable.strings");
    let fr = fr_dir.join("Localizable.strings");
    fs::write(&en, r#""value" = "%2$d files for %1$@";"#).unwrap();
    fs::write(&fr, r#""value" = "%1$s : %2$d fichiers";"#).unwrap();

    let output = langcodec_cmd()
        .args([
            "check",
            "--inputs",
            en.to_str().unwrap(),
            fr.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "reordered positional arguments failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(json_output(&output)["valid"], true);
}

#[test]
fn check_equates_implicit_and_explicit_placeholder_identity() {
    let temp_dir = TempDir::new().unwrap();
    let en_dir = temp_dir.path().join("en.lproj");
    let fr_dir = temp_dir.path().join("fr.lproj");
    fs::create_dir_all(&en_dir).unwrap();
    fs::create_dir_all(&fr_dir).unwrap();
    let en = en_dir.join("Localizable.strings");
    let fr = fr_dir.join("Localizable.strings");
    fs::write(&en, r#""value" = "%@ has %d files";"#).unwrap();
    fs::write(&fr, r#""value" = "%1$s : %2$d fichiers";"#).unwrap();

    let output = langcodec_cmd()
        .args([
            "check",
            "--inputs",
            en.to_str().unwrap(),
            fr.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "equivalent implicit and explicit arguments failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(json_output(&output)["valid"], true);
}

#[test]
fn check_accepts_same_locale_in_distinct_domains() {
    let temp_dir = TempDir::new().unwrap();
    let input = temp_dir.path().join("catalog.csv");
    fs::write(
        &input,
        extended_csv(&[
            [
                "v1", "resource", "0", "", "en", "Auth", "[]", "", "", "", "", "", "", "", "", "",
            ],
            [
                "v1",
                "entry",
                "0",
                "0",
                "",
                "",
                "",
                "title",
                "singular",
                "",
                "",
                "Account %@",
                "translated",
                "none",
                "",
                "[]",
            ],
            [
                "v1", "resource", "1", "", "en", "Profile", "[]", "", "", "", "", "", "", "", "",
                "",
            ],
            [
                "v1",
                "entry",
                "1",
                "0",
                "",
                "",
                "",
                "title",
                "singular",
                "",
                "",
                "Profile",
                "translated",
                "none",
                "",
                "[]",
            ],
        ]),
    )
    .unwrap();

    let output = langcodec_cmd()
        .args(["check", "--inputs", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "distinct domains were treated as duplicate resources: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report = json_output(&output);
    assert_eq!(report["files_checked"], 1);
    assert_eq!(report["issues"].as_array().unwrap().len(), 0);
}

#[test]
fn check_rejects_catalog_with_no_resources() {
    let temp_dir = TempDir::new().unwrap();
    let input = temp_dir.path().join("empty.csv");
    fs::write(&input, extended_csv(&[])).unwrap();

    let output = langcodec_cmd()
        .args(["check", "--inputs", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report = json_output(&output);
    assert_eq!(report["issues"].as_array().unwrap().len(), 1);
    assert_eq!(report["issues"][0]["kind"], "structure");
}

#[test]
fn check_rejects_invalid_resource_locale() {
    let temp_dir = TempDir::new().unwrap();
    let input = temp_dir.path().join("invalid-locale.csv");
    fs::write(
        &input,
        extended_csv(&[
            [
                "v1",
                "resource",
                "0",
                "",
                "not_a_locale_!",
                "App",
                "[]",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
            ],
            [
                "v1",
                "entry",
                "0",
                "0",
                "",
                "",
                "",
                "title",
                "singular",
                "",
                "",
                "Title",
                "translated",
                "none",
                "",
                "[]",
            ],
        ]),
    )
    .unwrap();

    let output = langcodec_cmd()
        .args(["check", "--inputs", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report = json_output(&output);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["kind"] == "locale")
    );
}

#[test]
fn check_rejects_duplicate_normalized_locales_within_one_catalog() {
    let temp_dir = TempDir::new().unwrap();
    let input = temp_dir.path().join("Localizable.xcstrings");
    fs::write(
        &input,
        r#"{
  "sourceLanguage": "en",
  "version": "1.0",
  "strings": {
    "value": {
      "localizations": {
        "fr-CA": {
          "stringUnit": { "state": "translated", "value": "Valeur" }
        },
        "FR_ca": {
          "stringUnit": { "state": "translated", "value": "Autre valeur" }
        }
      }
    }
  }
}
"#,
    )
    .unwrap();

    let run = || {
        langcodec_cmd()
            .args(["check", "--inputs", input.to_str().unwrap(), "--json"])
            .output()
            .unwrap()
    };
    let first = run();
    let second = run();

    assert!(!first.status.success());
    assert_eq!(first.stdout, second.stdout);
    let report = json_output(&first);
    assert!(report["issues"].as_array().unwrap().iter().any(|issue| {
        issue["kind"] == "locale"
            && issue["message"]
                .as_str()
                .is_some_and(|message| message.contains("fr-CA"))
    }));
}

#[test]
fn check_plural_validation_accepts_underscore_locale_identity() {
    let temp_dir = TempDir::new().unwrap();
    let input = temp_dir.path().join("Localizable.xcstrings");
    fs::write(
        &input,
        r#"{
  "sourceLanguage": "en",
  "version": "1.0",
  "strings": {
    "items": {
      "localizations": {
        "pt_BR": {
          "variations": {
            "plural": {
              "one": {
                "stringUnit": { "state": "translated", "value": "Um item" }
              },
              "other": {
                "stringUnit": { "state": "translated", "value": "%d itens" }
              }
            }
          }
        }
      }
    }
  }
}
"#,
    )
    .unwrap();

    let output = langcodec_cmd()
        .args(["check", "--inputs", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "underscore locale plural validation failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(json_output(&output)["valid"], true);
}

#[test]
fn check_honors_global_strict_read_options() {
    let temp_dir = TempDir::new().unwrap();
    let input = temp_dir.path().join("Localizable.strings");
    fs::write(&input, r#""welcome" = "Welcome";"#).unwrap();

    let missing_language = langcodec_cmd()
        .args([
            "--strict",
            "check",
            "--inputs",
            input.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!missing_language.status.success());
    let report = json_output(&missing_language);
    assert_eq!(report["issues"][0]["kind"], "parse");

    let explicit_language = langcodec_cmd()
        .args([
            "--strict",
            "check",
            "--inputs",
            input.to_str().unwrap(),
            "--lang",
            "en",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        explicit_language.status.success(),
        "strict check with a language hint failed: {}",
        String::from_utf8_lossy(&explicit_language.stderr)
    );
    assert_eq!(json_output(&explicit_language)["valid"], true);
}
