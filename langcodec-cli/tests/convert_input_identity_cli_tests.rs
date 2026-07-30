use std::{fs, process::Command};

use tempfile::TempDir;

fn langcodec_cmd() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("langcodec"))
}

fn canonical_stringsdict() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>file_count</key>
  <dict>
    <key>NSStringLocalizedFormatKey</key>
    <string>%#@count@</string>
    <key>count</key>
    <dict>
      <key>NSStringFormatSpecTypeKey</key>
      <string>NSStringPluralRuleType</string>
      <key>NSStringFormatValueTypeKey</key>
      <string>d</string>
      <key>one</key>
      <string>One file</string>
      <key>other</key>
      <string>%d files</string>
    </dict>
  </dict>
</dict>
</plist>
"#
}

fn assert_android_plural(path: &std::path::Path) {
    let android = fs::read_to_string(path).expect("generated Android XML");
    assert!(android.contains("<plurals name=\"file_count\""));
    assert!(android.contains("quantity=\"one\">One file"));
    assert!(android.contains("quantity=\"other\">%d files"));
}

#[test]
fn standalone_stringsdict_uses_source_language_as_its_input_identity() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("Localizable.stringsdict");
    let output = temporary.path().join("values-en").join("strings.xml");
    fs::create_dir_all(output.parent().expect("output parent")).expect("output directory");
    fs::write(&input, canonical_stringsdict()).expect("stringsdict fixture");

    let converted = langcodec_cmd()
        .args([
            "convert",
            "--input",
            input.to_str().expect("UTF-8 path"),
            "--output",
            output.to_str().expect("UTF-8 path"),
            "--source-language",
            "en",
        ])
        .output()
        .expect("run convert");

    assert!(
        converted.status.success(),
        "conversion failed: {}{}",
        String::from_utf8_lossy(&converted.stdout),
        String::from_utf8_lossy(&converted.stderr)
    );
    assert_android_plural(&output);
}

#[test]
fn standalone_stringsdict_without_an_input_identity_fails_actionably() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("Localizable.stringsdict");
    let output = temporary.path().join("values-en").join("strings.xml");
    fs::create_dir_all(output.parent().expect("output parent")).expect("output directory");
    fs::write(&input, canonical_stringsdict()).expect("stringsdict fixture");

    let converted = langcodec_cmd()
        .args([
            "convert",
            "--input",
            input.to_str().expect("UTF-8 path"),
            "--output",
            output.to_str().expect("UTF-8 path"),
        ])
        .output()
        .expect("run convert");

    assert!(!converted.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&converted.stdout),
        String::from_utf8_lossy(&converted.stderr)
    );
    assert!(
        combined.contains("--source-language") && combined.contains("<LANG>.lproj"),
        "expected an actionable input-language error, got: {combined}"
    );
    assert!(!output.exists(), "failed conversion must not create output");
}

#[test]
fn explicit_stringsdict_input_format_reads_an_extensionless_file_in_both_modes() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("catalog");
    fs::write(&input, canonical_stringsdict()).expect("extensionless stringsdict fixture");

    for strict in [false, true] {
        let mode = if strict { "strict" } else { "non-strict" };
        let output = temporary
            .path()
            .join(mode)
            .join("values-en")
            .join("strings.xml");
        fs::create_dir_all(output.parent().expect("output parent")).expect("output directory");

        let mut command = langcodec_cmd();
        if strict {
            command.arg("--strict");
        }
        let converted = command
            .args([
                "convert",
                "--input",
                input.to_str().expect("UTF-8 path"),
                "--output",
                output.to_str().expect("UTF-8 path"),
                "--input-format",
                "stringsdict",
                "--source-language",
                "en",
            ])
            .output()
            .expect("run explicit conversion");

        assert!(
            converted.status.success(),
            "{mode} explicit conversion failed: {}{}",
            String::from_utf8_lossy(&converted.stdout),
            String::from_utf8_lossy(&converted.stderr)
        );
        assert_android_plural(&output);
    }
}

#[test]
fn explicit_stringsdict_output_format_overrides_a_conflicting_xml_extension() {
    let temporary = TempDir::new().expect("temporary directory");
    let input_directory = temporary.path().join("en.lproj");
    fs::create_dir_all(&input_directory).expect("input directory");
    let input = input_directory.join("Localizable.stringsdict");
    fs::write(&input, canonical_stringsdict()).expect("stringsdict fixture");

    for strict in [false, true] {
        let mode = if strict { "strict" } else { "non-strict" };
        let output = temporary.path().join(format!("{mode}-result.xml"));

        let mut command = langcodec_cmd();
        if strict {
            command.arg("--strict");
        }
        let converted = command
            .args([
                "convert",
                "--input",
                input.to_str().expect("UTF-8 path"),
                "--output",
                output.to_str().expect("UTF-8 path"),
                "--output-format",
                "stringsdict",
            ])
            .output()
            .expect("run explicit output conversion");

        assert!(
            converted.status.success(),
            "{mode} explicit output conversion failed: {}{}",
            String::from_utf8_lossy(&converted.stdout),
            String::from_utf8_lossy(&converted.stderr)
        );
        let plist = fs::read_to_string(&output).expect("generated stringsdict");
        assert!(plist.contains("<plist"));
        assert!(plist.contains("NSStringPluralRuleType"));
        assert!(
            !plist.contains("<resources>"),
            "the conflicting .xml extension selected Android output"
        );
    }
}

#[test]
fn explicit_custom_input_format_overrides_a_conflicting_stringsdict_extension() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("translations.stringsdict");
    fs::write(&input, r#"{"key":"welcome","en":"Hello"}"#).expect("JSON language map fixture");

    for strict in [false, true] {
        let mode = if strict { "strict" } else { "non-strict" };
        let output = temporary.path().join(format!("{mode}.csv"));

        let mut command = langcodec_cmd();
        if strict {
            command.arg("--strict");
        }
        let converted = command
            .args([
                "convert",
                "--input",
                input.to_str().expect("UTF-8 path"),
                "--output",
                output.to_str().expect("UTF-8 path"),
                "--input-format",
                "json-language-map",
            ])
            .output()
            .expect("run explicit custom conversion");

        assert!(
            converted.status.success(),
            "{mode} explicit custom conversion failed: {}{}",
            String::from_utf8_lossy(&converted.stdout),
            String::from_utf8_lossy(&converted.stderr)
        );
        let csv = fs::read_to_string(&output).expect("generated CSV");
        assert!(csv.contains("welcome"));
        assert!(csv.contains("Hello"));
    }
}

#[test]
fn explicit_custom_input_failure_never_falls_back_or_mutates_output() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("Localizable.stringsdict");
    fs::write(&input, canonical_stringsdict()).expect("stringsdict fixture");

    for strict in [false, true] {
        let mode = if strict { "strict" } else { "non-strict" };
        let output = temporary.path().join(format!("{mode}.csv"));
        fs::write(&output, "keep this output intact\n").expect("output sentinel");

        let mut command = langcodec_cmd();
        if strict {
            command.arg("--strict");
        }
        let converted = command
            .args([
                "convert",
                "--input",
                input.to_str().expect("UTF-8 path"),
                "--output",
                output.to_str().expect("UTF-8 path"),
                "--input-format",
                "json-language-map",
            ])
            .output()
            .expect("run explicit custom conversion");

        assert!(
            !converted.status.success(),
            "{mode} conversion silently fell back to the stringsdict extension"
        );
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&converted.stdout),
            String::from_utf8_lossy(&converted.stderr)
        );
        assert!(
            combined.contains("Error parsing JSON"),
            "expected the authoritative JSON parser error, got: {combined}"
        );
        assert_eq!(
            fs::read_to_string(&output).expect("output sentinel remains readable"),
            "keep this output intact\n",
            "{mode} failure mutated the existing output"
        );
    }
}
