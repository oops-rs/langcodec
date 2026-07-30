use std::{fs, process::Command};

use tempfile::TempDir;

fn langcodec_cmd() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("langcodec"))
}

fn write_stringsdict_fixture(path: &std::path::Path) {
    fs::create_dir_all(path.parent().expect("stringsdict parent")).expect("stringsdict directory");
    fs::write(
        path,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>file_count</key><dict>
    <key>NSStringLocalizedFormatKey</key><string>%#@count@</string>
    <key>count</key><dict>
      <key>NSStringFormatSpecTypeKey</key><string>NSStringPluralRuleType</string>
      <key>NSStringFormatValueTypeKey</key><string>d</string>
      <key>one</key><string>One file</string>
      <key>other</key><string>%d files</string>
    </dict>
  </dict>
</dict></plist>
"#,
    )
    .expect("stringsdict fixture");
}

#[test]
fn cli_rejects_android_to_stringsdict_without_creating_or_truncating_output() {
    let temporary = TempDir::new().expect("temporary directory");
    let android = temporary.path().join("values-en").join("strings.xml");
    let stringsdict = temporary
        .path()
        .join("en.lproj")
        .join("Localizable.stringsdict");
    fs::create_dir_all(android.parent().expect("Android parent")).expect("Android directory");
    fs::write(
        &android,
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

    let converted = langcodec_cmd()
        .args([
            "convert",
            "--input",
            android.to_str().expect("UTF-8 path"),
            "--output",
            stringsdict.to_str().expect("UTF-8 path"),
        ])
        .output()
        .expect("run convert");
    assert!(
        !converted.status.success(),
        "Android -> stringsdict must require explicit selector identity"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&converted.stdout),
        String::from_utf8_lossy(&converted.stderr)
    );
    assert!(combined.contains("does not identify which printf argument drives plural selection"));
    assert!(combined.contains("stringsdict.localized_format"));
    assert!(!stringsdict.exists(), "rejection must not create output");

    fs::create_dir_all(stringsdict.parent().expect("output parent")).expect("output directory");
    fs::write(&stringsdict, "existing output").expect("output sentinel");
    let converted = langcodec_cmd()
        .args([
            "convert",
            "--input",
            android.to_str().expect("UTF-8 path"),
            "--output",
            stringsdict.to_str().expect("UTF-8 path"),
        ])
        .output()
        .expect("run convert against existing output");
    assert!(!converted.status.success());
    assert_eq!(
        fs::read_to_string(&stringsdict).expect("unchanged output"),
        "existing output"
    );
}

#[test]
fn cli_views_checks_and_converts_stringsdict_to_android() {
    let temporary = TempDir::new().expect("temporary directory");
    let stringsdict = temporary
        .path()
        .join("en.lproj")
        .join("Localizable.stringsdict");
    let android = temporary.path().join("values-en").join("strings.xml");
    write_stringsdict_fixture(&stringsdict);

    let viewed = langcodec_cmd()
        .args([
            "view",
            "--input",
            stringsdict.to_str().expect("UTF-8 path"),
            "--lang",
            "en",
            "--full",
        ])
        .output()
        .expect("run view");
    assert!(
        viewed.status.success(),
        "view failed: {}",
        String::from_utf8_lossy(&viewed.stderr)
    );
    let view_stdout = String::from_utf8_lossy(&viewed.stdout);
    assert!(view_stdout.contains("Type: Plural"));
    assert!(view_stdout.contains("One file"));
    assert!(view_stdout.contains("%d files"));

    let checked = langcodec_cmd()
        .args([
            "check",
            "--inputs",
            stringsdict.to_str().expect("UTF-8 path"),
            "--lang",
            "en",
        ])
        .output()
        .expect("run check");
    assert!(
        checked.status.success(),
        "check failed: {}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );

    let converted_back = langcodec_cmd()
        .args([
            "convert",
            "--input",
            stringsdict.to_str().expect("UTF-8 path"),
            "--output",
            android.to_str().expect("UTF-8 path"),
        ])
        .output()
        .expect("run reverse convert");
    assert!(
        converted_back.status.success(),
        "stringsdict -> Android failed: {}{}",
        String::from_utf8_lossy(&converted_back.stdout),
        String::from_utf8_lossy(&converted_back.stderr)
    );
    let android = fs::read_to_string(android).expect("converted Android");
    assert!(android.contains("quantity=\"one\">One file"));
    assert!(android.contains("quantity=\"other\">%d files"));
}

#[test]
fn cli_rejects_scalar_or_rewriting_commands_for_stringsdict() {
    let temporary = TempDir::new().expect("temporary directory");
    let stringsdict = temporary.path().join("Localizable.stringsdict");
    fs::write(
        &stringsdict,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>file_count</key><dict>
    <key>NSStringLocalizedFormatKey</key><string>%#@count@</string>
    <key>count</key><dict>
      <key>NSStringFormatSpecTypeKey</key><string>NSStringPluralRuleType</string>
      <key>NSStringFormatValueTypeKey</key><string>d</string>
      <key>one</key><string>One file</string>
      <key>other</key><string>%d files</string>
    </dict>
  </dict>
</dict></plist>
"#,
    )
    .expect("stringsdict fixture");

    for arguments in [
        vec![
            "edit",
            "set",
            "--inputs",
            stringsdict.to_str().expect("UTF-8 path"),
            "--key",
            "file_count",
            "--value",
            "files",
        ],
        vec![
            "normalize",
            "--inputs",
            stringsdict.to_str().expect("UTF-8 path"),
        ],
    ] {
        let output = langcodec_cmd()
            .args(arguments)
            .output()
            .expect("run guarded command");
        assert!(!output.status.success());
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            combined.contains(".stringsdict") && combined.contains("not supported"),
            "expected explicit stringsdict guard, got: {combined}"
        );
    }
}
