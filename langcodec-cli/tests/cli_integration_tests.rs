use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn langcodec_cmd() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("langcodec"))
}

#[test]
fn test_convert_command_basic() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");
    let output_file = temp_dir.path().join("output.xcstrings");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!",
        "fr": "Bonjour, le monde!"
    }"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "Command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(output_file.exists());
    let output_content = fs::read_to_string(&output_file).unwrap();
    assert!(output_content.contains("hello_world"));
}

#[test]
fn test_convert_command_with_explicit_format() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");
    let output_file = temp_dir.path().join("output.xcstrings");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!",
        "fr": "Bonjour, le monde!"
    }"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--input-format",
            "json-language-map",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());
}

#[test]
fn test_convert_command_with_language_code() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");
    let output_file = temp_dir.path().join("output.xcstrings");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!",
        "fr": "Bonjour, le monde!"
    }"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());
}

#[test]
fn test_convert_command_invalid_input_file() {
    let temp_dir = TempDir::new().unwrap();
    let output_file = temp_dir.path().join("output.xcstrings");

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            "nonexistent.json",
            "-o",
            output_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("File does not exist") || stderr.contains("Input validation failed"));
}

#[test]
fn test_convert_command_invalid_output_path() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!"
    }"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            "/nonexistent/path/output.xcstrings",
        ])
        .output()
        .unwrap();

    // This might succeed if the system allows creating the directory
    // or fail if it doesn't - both are acceptable behaviors
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Cannot write") || stderr.contains("Output validation failed"));
    }
}

#[test]
fn test_convert_command_missing_arguments() {
    let output = langcodec_cmd().args(["convert"]).output().unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("required") || stderr.contains("missing"));
}

#[test]
fn test_convert_command_help() {
    let output = langcodec_cmd()
        .args(["convert", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Convert localization files"));
    assert!(stdout.contains("--input"));
    assert!(stdout.contains("--output"));
}

#[test]
fn test_merge_command_basic() {
    let temp_dir = TempDir::new().unwrap();
    let input_file1 = temp_dir.path().join("file1.json");
    let input_file2 = temp_dir.path().join("file2.json");
    let output_file = temp_dir.path().join("merged.xcstrings");

    let json_content1 = r#"{
        "key": "hello_world",
        "en": "Hello, World!"
    }"#;

    let json_content2 = r#"{
        "key": "goodbye_world",
        "en": "Goodbye, World!"
    }"#;

    fs::write(&input_file1, json_content1).unwrap();
    fs::write(&input_file2, json_content2).unwrap();

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            input_file1.to_str().unwrap(),
            "-i",
            input_file2.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());
}

#[test]
fn test_merge_android_locales_writes_ios_locale_identifiers() {
    let temp_dir = TempDir::new().unwrap();
    let output_file = temp_dir.path().join("merged.xcstrings");
    let fixtures = [
        ("values", "Hello"),
        ("values-in", "Halo"),
        ("values-tl", "Kumusta"),
        ("values-zh", "你好"),
        ("values-zh-rTW", "您好"),
    ];

    for (directory, translation) in fixtures {
        let values_directory = temp_dir.path().join(directory);
        fs::create_dir_all(&values_directory).unwrap();
        let input = values_directory.join("strings.xml");
        fs::write(
            &input,
            format!(
                r#"<?xml version="1.0" encoding="utf-8"?>
<resources><string name="greeting">{translation}</string></resources>"#
            ),
        )
        .unwrap();
    }

    let pattern = format!("{}/values*/strings.xml", temp_dir.path().display());
    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            &pattern,
            "-o",
            output_file.to_str().unwrap(),
            "--source-language",
            "en",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "Command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let document: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_file).unwrap()).unwrap();
    assert_eq!(document["sourceLanguage"], "en");
    let localizations = document["strings"]["greeting"]["localizations"]
        .as_object()
        .unwrap();
    let mut languages = localizations.keys().map(String::as_str).collect::<Vec<_>>();
    languages.sort_unstable();
    assert_eq!(languages, ["en", "fil", "id", "zh-Hans", "zh-Hant"]);
}

#[test]
fn test_merge_command_with_glob_pattern() {
    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    // Create multiple .strings files that the glob should match
    let input_file1 = dir.join("a.strings");
    let input_file2 = dir.join("b.strings");
    let output_file = dir.join("merged.strings");

    let strings_content1 = r#"/* Greeting */
"hello" = "Hello";"#;
    let strings_content2 = r#"/* Farewell */
"goodbye" = "Goodbye";"#;

    fs::write(&input_file1, strings_content1).unwrap();
    fs::write(&input_file2, strings_content2).unwrap();

    // Use a glob pattern for inputs
    let pattern = format!("{}/*.strings", dir.to_string_lossy());

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            &pattern,
            "-o",
            output_file.to_str().unwrap(),
            "--strategy",
            "last",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_file.exists());
    let merged_content = fs::read_to_string(&output_file).unwrap();
    assert!(merged_content.contains("hello"));
    assert!(merged_content.contains("goodbye"));
}

#[test]
fn test_merge_command_with_recursive_glob_pattern() {
    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    // Create nested directories with .strings files
    let nested = dir.join("nested");
    fs::create_dir_all(&nested).unwrap();

    let input_file1 = dir.join("root.strings");
    let input_file2 = nested.join("nested.strings");
    let output_file = dir.join("merged.strings");

    let strings_content1 = r#"/* Greeting */
"hello" = "Hello";"#;
    let strings_content2 = r#"/* Welcome */
"welcome" = "Welcome";"#;

    fs::write(&input_file1, strings_content1).unwrap();
    fs::write(&input_file2, strings_content2).unwrap();

    // Use a recursive glob pattern for inputs
    let pattern = format!("{}/**/*.strings", dir.to_string_lossy());

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            &pattern,
            "-o",
            output_file.to_str().unwrap(),
            "--strategy",
            "last",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_file.exists());
    let merged_content = fs::read_to_string(&output_file).unwrap();
    assert!(merged_content.contains("hello"));
    assert!(merged_content.contains("welcome"));
}

#[test]
fn test_merge_command_with_conflict_strategy() {
    let temp_dir = TempDir::new().unwrap();
    let input_file1 = temp_dir.path().join("file1.json");
    let input_file2 = temp_dir.path().join("file2.json");
    let output_file = temp_dir.path().join("merged.xcstrings");

    let json_content1 = r#"{
        "key": "hello_world",
        "en": "Hello, World!"
    }"#;

    let json_content2 = r#"{
        "key": "hello_world",
        "en": "Hi, World!"
    }"#;

    fs::write(&input_file1, json_content1).unwrap();
    fs::write(&input_file2, json_content2).unwrap();

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            input_file1.to_str().unwrap(),
            "-i",
            input_file2.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--strategy",
            "last",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());
}

#[test]
fn test_view_command_basic() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!",
        "fr": "Bonjour, le monde!"
    }"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args(["view", "-i", input_file.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello_world"));
    assert!(stdout.contains("Hello, World!"));
    assert!(stdout.contains("Bonjour, le monde!"));
}

#[test]
fn test_view_command_with_format() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!",
        "fr": "Bonjour, le monde!"
    }"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args(["view", "-i", input_file.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello_world"));
}

#[test]
fn test_view_help_lists_new_filter_flags() {
    let output = langcodec_cmd().args(["view", "--help"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--status"));
    assert!(stdout.contains("--keys-only"));
    assert!(stdout.contains("--json"));
}

#[test]
fn test_debug_command_basic() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!",
        "fr": "Bonjour, le monde!"
    }"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args(["debug", "-i", input_file.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Debug output") || stdout.contains("Debug Summary"));
}

#[test]
fn test_main_help_command() {
    let output = langcodec_cmd().args(["--help"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("langcodec"));
    assert!(stdout.contains("convert"));
    assert!(stdout.contains("diff"));
    assert!(stdout.contains("merge"));
    assert!(stdout.contains("sync"));
    assert!(stdout.contains("view"));
    assert!(stdout.contains("debug"));
}

#[test]
fn test_invalid_command() {
    let output = langcodec_cmd().args(["invalid-command"]).output().unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error") || stderr.contains("Unknown"));
}

#[test]
fn test_version_command() {
    let output = langcodec_cmd().args(["--version"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("langcodec"));
}

#[test]
fn test_convert_command_with_yaml() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.yaml");
    let output_file = temp_dir.path().join("output.xcstrings");

    let yaml_content = r#"key: hello_world
en: Hello, World!
fr: Bonjour, le monde!"#;

    fs::write(&input_file, yaml_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--input-format",
            "yaml-language-map",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());
}

#[test]
fn test_convert_command_with_json_array() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");
    let output_file = temp_dir.path().join("output.xcstrings");

    let json_content = r#"[
        {
            "key": "hello_world",
            "en": "Hello, World!",
            "fr": "Bonjour, le monde!"
        },
        {
            "key": "goodbye_world",
            "en": "Goodbye, World!",
            "fr": "Au revoir, le monde!"
        }
    ]"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--input-format",
            "json-array-language-map",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());
}

#[test]
fn test_convert_command_output_to_csv() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");
    let output_file = temp_dir.path().join("output.csv");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!",
        "fr": "Bonjour, le monde!"
    }"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--output-format",
            "csv",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());
}

#[test]
fn test_convert_command_output_to_tsv() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");
    let output_file = temp_dir.path().join("output.tsv");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!",
        "fr": "Bonjour, le monde!"
    }"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--output-format",
            "tsv",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());

    let output_content = fs::read_to_string(&output_file).unwrap();
    assert!(output_content.contains("key\ten\tfr"));
}

#[test]
fn test_convert_command_output_to_strings() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");
    let output_file = temp_dir.path().join("output.strings");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!",
        "fr": "Bonjour, le monde!"
    }"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--output-format",
            "strings",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("single-language"));
    assert!(stderr.contains("--output-lang"));
}

#[test]
fn test_convert_command_output_to_android() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");
    let output_file = temp_dir.path().join("output.xml");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!",
        "fr": "Bonjour, le monde!"
    }"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--output-format",
            "android",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("single-language"));
    assert!(stderr.contains("--output-lang"));
}

#[test]
fn test_convert_command_output_to_strings_with_output_lang() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");
    let output_file = temp_dir.path().join("output.strings");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!",
        "fr": "Bonjour, le monde!"
    }"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--output-lang",
            "fr",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let output_content = fs::read_to_string(&output_file).unwrap();
    assert!(output_content.contains("Bonjour, le monde!"));
    assert!(!output_content.contains("Hello, World!"));
}

#[test]
fn test_convert_command_output_to_android_with_output_lang() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");
    let output_file = temp_dir.path().join("strings.xml");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!",
        "fr": "Bonjour, le monde!"
    }"#;

    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--output-lang",
            "fr",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let output_content = fs::read_to_string(&output_file).unwrap();
    assert!(output_content.contains("Bonjour, le monde!"));
    assert!(!output_content.contains("Hello, World!"));
}

#[test]
fn test_convert_command_output_lang_conflicts_with_output_path_language() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("test.json");
    let output_dir = temp_dir.path().join("en.lproj");
    let output_file = output_dir.join("Localizable.strings");

    let json_content = r#"{
        "key": "hello_world",
        "en": "Hello, World!",
        "fr": "Bonjour, le monde!"
    }"#;

    fs::create_dir_all(&output_dir).unwrap();
    fs::write(&input_file, json_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--output-lang",
            "fr",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("conflicts with language"));
}

#[test]
fn test_convert_command_with_tsv_input_format() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("input.tsv");
    let output_file = temp_dir.path().join("output.xcstrings");

    let tsv_content = "key\ten\tfr\nhello_world\tHello, World!\tBonjour, le monde!\n";
    fs::write(&input_file, tsv_content).unwrap();

    let output = langcodec_cmd()
        .args([
            "convert",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--input-format",
            "tsv",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "Command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_file.exists());

    let output_content = fs::read_to_string(&output_file).unwrap();
    assert!(output_content.contains("hello_world"));
}

#[test]
fn test_merge_command_updated_behavior() {
    let temp_dir = TempDir::new().unwrap();
    let input_file1 = temp_dir.path().join("file1.strings");
    let input_file2 = temp_dir.path().join("file2.strings");
    let output_file = temp_dir.path().join("merged.strings");

    // Create two .strings files with the same language but different keys
    let strings_content1 = r#"/* Greeting */
"hello" = "Hello";"#;

    let strings_content2 = r#"/* Farewell */
"goodbye" = "Goodbye";"#;

    fs::write(&input_file1, strings_content1).unwrap();
    fs::write(&input_file2, strings_content2).unwrap();

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            input_file1.to_str().unwrap(),
            "-i",
            input_file2.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--strategy",
            "last",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());

    // Verify the output contains the expected merge count message
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Merged 1 language groups"),
        "Expected merge count message, got: {}",
        stdout
    );

    // Verify the output contains the success message
    assert!(
        stdout.contains("✅ Successfully merged 2 files into"),
        "Expected success message, got: {}",
        stdout
    );

    // Verify the merged file contains both entries
    let merged_content = fs::read_to_string(&output_file).unwrap();
    assert!(merged_content.contains("hello"));
    assert!(merged_content.contains("goodbye"));
}

#[test]
fn test_merge_command_with_language_override() {
    let temp_dir = TempDir::new().unwrap();
    let input_file1 = temp_dir.path().join("file1.strings");
    let input_file2 = temp_dir.path().join("file2.strings");
    let output_file = temp_dir.path().join("merged.strings");

    // Create two .strings files with different content
    let strings_content1 = r#"/* Greeting */
"hello" = "Hello";"#;

    let strings_content2 = r#"/* Farewell */
"goodbye" = "Goodbye";"#;

    fs::write(&input_file1, strings_content1).unwrap();
    fs::write(&input_file2, strings_content2).unwrap();

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            input_file1.to_str().unwrap(),
            "-i",
            input_file2.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "-l",
            "en",
            "--strategy",
            "first",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());

    // Verify the output contains the expected merge count message
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Merged 1 language groups"),
        "Expected merge count message, got: {}",
        stdout
    );

    // Verify the output contains the success message
    assert!(
        stdout.contains("✅ Successfully merged 2 files into"),
        "Expected success message, got: {}",
        stdout
    );

    // Verify the merged file contains both entries
    let merged_content = fs::read_to_string(&output_file).unwrap();
    assert!(merged_content.contains("hello"));
    assert!(merged_content.contains("goodbye"));
}

#[test]
fn test_merge_command_skip_strategy_removes_triply_conflicted_key() {
    let temp_dir = TempDir::new().unwrap();
    let input_file1 = temp_dir.path().join("file1.json");
    let input_file2 = temp_dir.path().join("file2.json");
    let input_file3 = temp_dir.path().join("file3.json");
    let output_file = temp_dir.path().join("merged.xcstrings");

    fs::write(&input_file1, r#"{"key":"hello","en":"one"}"#).unwrap();
    fs::write(&input_file2, r#"{"key":"hello","en":"two"}"#).unwrap();
    fs::write(&input_file3, r#"{"key":"hello","en":"three"}"#).unwrap();

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            input_file1.to_str().unwrap(),
            "-i",
            input_file2.to_str().unwrap(),
            "-i",
            input_file3.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--strategy",
            "skip",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let merged_content = fs::read_to_string(&output_file).unwrap();
    assert!(!merged_content.contains("\"hello\""));
}

#[test]
fn test_merge_command_single_language_output_requires_selection() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("input.csv");
    let output_file = temp_dir.path().join("merged.strings");

    fs::write(&input_file, "key,en,fr\nhello,Hello,Bonjour\n").unwrap();

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("single-language"));
}

#[test]
fn test_merge_command_single_language_output_uses_lang_selector() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("input.csv");
    let output_file = temp_dir.path().join("merged.strings");

    fs::write(&input_file, "key,en,fr\nhello,Hello,Bonjour\n").unwrap();

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--lang",
            "fr",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let merged_content = fs::read_to_string(&output_file).unwrap();
    assert!(merged_content.contains("Bonjour"));
    assert!(!merged_content.contains("\"hello\" = \"Hello\";"));
}

#[test]
fn test_merge_command_multiple_languages_no_merges() {
    let temp_dir = TempDir::new().unwrap();
    let input_file1 = temp_dir.path().join("file1.strings");
    let input_file2 = temp_dir.path().join("file2.strings");
    let output_file = temp_dir.path().join("merged.strings");

    // Create two .strings files with different keys (no conflicts)
    let content1 = r#"/* Greeting */
"hello" = "Hello";"#;

    let content2 = r#"/* Farewell */
"goodbye" = "Goodbye";"#;

    fs::write(&input_file1, content1).unwrap();
    fs::write(&input_file2, content2).unwrap();

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            input_file1.to_str().unwrap(),
            "-i",
            input_file2.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--strategy",
            "last",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());

    // Since both files have the same language (empty, inferred from path), they should merge
    // Verify the output contains the expected merge count message (1 merge since same language)
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Merged 1 language groups"),
        "Expected 1 merge count message, got: {}",
        stdout
    );

    // Verify the output contains the success message
    assert!(
        stdout.contains("✅ Successfully merged 2 files into"),
        "Expected success message, got: {}",
        stdout
    );

    // Verify the merged file contains both entries
    let merged_content = fs::read_to_string(&output_file).unwrap();
    assert!(merged_content.contains("hello"));
    assert!(merged_content.contains("goodbye"));
}

#[test]
fn test_merge_command_format_inference_strings() {
    let temp_dir = TempDir::new().unwrap();
    let input_file1 = temp_dir.path().join("file1.strings");
    let input_file2 = temp_dir.path().join("file2.strings");
    let output_file = temp_dir.path().join("merged.strings");

    // Create two .strings files with different keys
    let content1 = r#"/* Greeting */
"hello" = "Hello";"#;

    let content2 = r#"/* Farewell */
"goodbye" = "Goodbye";"#;

    fs::write(&input_file1, content1).unwrap();
    fs::write(&input_file2, content2).unwrap();

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            input_file1.to_str().unwrap(),
            "-i",
            input_file2.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--strategy",
            "last",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());

    // Verify the output contains the format inference message
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Converting resources to format:"),
        "Expected format inference message, got: {}",
        stdout
    );

    // Verify the output contains the success message
    assert!(
        stdout.contains("✅ Successfully merged 2 files into"),
        "Expected success message, got: {}",
        stdout
    );

    // Verify the merged file contains both entries
    let merged_content = fs::read_to_string(&output_file).unwrap();
    assert!(merged_content.contains("hello"));
    assert!(merged_content.contains("goodbye"));
}

#[test]
fn test_merge_command_format_inference_xml() {
    let temp_dir = TempDir::new().unwrap();
    let input_file1 = temp_dir.path().join("file1.strings");
    let input_file2 = temp_dir.path().join("file2.strings");
    let output_file = temp_dir.path().join("merged.xml");

    // Create two .strings files with different keys
    let content1 = r#"/* Greeting */
"hello" = "Hello";"#;

    let content2 = r#"/* Farewell */
"goodbye" = "Goodbye";"#;

    fs::write(&input_file1, content1).unwrap();
    fs::write(&input_file2, content2).unwrap();

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            input_file1.to_str().unwrap(),
            "-i",
            input_file2.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--strategy",
            "last",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());

    // Verify the output contains the format inference message
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Converting resources to format:"),
        "Expected format inference message, got: {}",
        stdout
    );

    // Verify the output contains the success message
    assert!(
        stdout.contains("✅ Successfully merged 2 files into"),
        "Expected success message, got: {}",
        stdout
    );

    // Verify the merged file contains XML content
    let merged_content = fs::read_to_string(&output_file).unwrap();
    assert!(merged_content.contains("<?xml"));
    assert!(merged_content.contains("<resources>"));
}

#[test]
fn test_merge_command_single_resource_fallback() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("file.strings");
    let output_file = temp_dir.path().join("output.strings");

    // Create a single .strings file
    let content = r#"/* Greeting */
"hello" = "Hello";"#;

    fs::write(&input_file, content).unwrap();

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--strategy",
            "last",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output_file.exists());

    // Since .strings extension can be inferred, it should use the format conversion path
    // Verify the output contains the format inference message
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Converting resources to format:"),
        "Expected format inference message, got: {}",
        stdout
    );

    // Verify the output contains the success message
    assert!(
        stdout.contains("✅ Successfully merged 1 files into"),
        "Expected success message, got: {}",
        stdout
    );

    // Verify the output file contains the expected content
    let output_content = fs::read_to_string(&output_file).unwrap();
    assert!(output_content.contains("hello"));
}

#[test]
fn test_merge_command_actual_fallback_behavior() {
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("file.strings");
    let output_file = temp_dir.path().join("output.unknown");

    // Create a single .strings file
    let content = r#"/* Greeting */
"hello" = "Hello";"#;

    fs::write(&input_file, content).unwrap();

    let output = langcodec_cmd()
        .args([
            "merge",
            "-i",
            input_file.to_str().unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--strategy",
            "last",
        ])
        .output()
        .unwrap();

    // This should fail because the format cannot be inferred from .unknown extension
    // and the fallback also requires a valid extension
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Cannot infer format from output path"),
        "Expected format inference error, got: {}",
        stderr
    );
}
