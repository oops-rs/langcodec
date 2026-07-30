use std::fs;
use std::process::Command;

use langcodec::{Codec, ReadOptions, Translation};
use tempfile::TempDir;

fn langcodec_cmd() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("langcodec"))
}

fn singular_value<'a>(codec: &'a Codec, language: &str, key: &str) -> Option<&'a str> {
    let entry = codec
        .resources
        .iter()
        .find(|resource| resource.metadata.language == language)?
        .find_entry(key)?;
    match &entry.value {
        Translation::Singular(value) => Some(value),
        Translation::Empty | Translation::Plural(_) => None,
    }
}

#[test]
fn test_sync_updates_existing_entries_with_translation_fallback() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source.csv");
    let target = temp_dir.path().join("target.csv");
    let output = temp_dir.path().join("synced.csv");

    let source_content = "\
key,en,fr
welcome_key,Welcome,Bienvenue
goodbye,Goodbye,Au revoir
new_only,Only in source,Seulement source
";
    let target_content = "\
key,en,fr
Welcome,Old Welcome,Ancienne bienvenue
goodbye,Old Goodbye,Ancien au revoir
keep_me,Keep me,Reste pareil
";

    fs::write(&source, source_content).unwrap();
    fs::write(&target, target_content).unwrap();

    let out = langcodec_cmd()
        .args([
            "sync",
            "--source",
            source.to_str().unwrap(),
            "--target",
            target.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--match-lang",
            "en",
        ])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(output.exists());

    let mut synced = Codec::new();
    synced
        .read_file_by_extension_with_options(&output, &ReadOptions::new())
        .unwrap();
    assert_eq!(singular_value(&synced, "en", "Welcome"), Some("Welcome"));
    assert_eq!(singular_value(&synced, "fr", "Welcome"), Some("Bienvenue"));
    assert_eq!(singular_value(&synced, "en", "goodbye"), Some("Goodbye"));
    assert_eq!(singular_value(&synced, "fr", "goodbye"), Some("Au revoir"));
    assert_eq!(singular_value(&synced, "en", "keep_me"), Some("Keep me"));
    assert_eq!(
        singular_value(&synced, "fr", "keep_me"),
        Some("Reste pareil")
    );
    assert!(
        synced
            .resources
            .iter()
            .all(|resource| resource.find_entry("new_only").is_none())
    );
}

#[test]
fn test_sync_dry_run_does_not_write_target() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source.csv");
    let target = temp_dir.path().join("target.csv");

    let source_content = "\
key,en
welcome,Welcome
";
    let target_content = "\
key,en
welcome,Old Welcome
";

    fs::write(&source, source_content).unwrap();
    fs::write(&target, target_content).unwrap();
    let before = fs::read_to_string(&target).unwrap();

    let out = langcodec_cmd()
        .args([
            "sync",
            "--source",
            source.to_str().unwrap(),
            "--target",
            target.to_str().unwrap(),
            "--dry-run",
            "--match-lang",
            "en",
        ])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let after = fs::read_to_string(&target).unwrap();
    assert_eq!(before, after);
}

#[test]
fn test_sync_report_json_written() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source.csv");
    let target = temp_dir.path().join("target.csv");
    let report = temp_dir.path().join("sync_report.json");

    fs::write(&source, "key,en\nwelcome,Welcome\n").unwrap();
    fs::write(&target, "key,en\nwelcome,Old Welcome\n").unwrap();

    let out = langcodec_cmd()
        .args([
            "sync",
            "--source",
            source.to_str().unwrap(),
            "--target",
            target.to_str().unwrap(),
            "--report-json",
            report.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(report.exists());
}

#[test]
fn test_sync_fail_on_unmatched_exits_nonzero() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source.csv");
    let target = temp_dir.path().join("target.csv");

    fs::write(&source, "key,en\nwelcome,Welcome\n").unwrap();
    fs::write(&target, "key,en\nnot_in_source,Old\n").unwrap();

    let out = langcodec_cmd()
        .args([
            "sync",
            "--source",
            source.to_str().unwrap(),
            "--target",
            target.to_str().unwrap(),
            "--fail-on-unmatched",
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn test_sync_strict_fails_on_unmatched_by_default() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source.csv");
    let target = temp_dir.path().join("target.csv");

    fs::write(&source, "key,en\nwelcome,Welcome\n").unwrap();
    fs::write(&target, "key,en\nnot_in_source,Old\n").unwrap();

    let out = langcodec_cmd()
        .args([
            "--strict",
            "sync",
            "--source",
            source.to_str().unwrap(),
            "--target",
            target.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
