use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn langcodec_cmd() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("langcodec"))
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn write_fake_tolgee(
    project_root: &Path,
    payload_path: &Path,
    capture_path: &Path,
    log_path: &Path,
) -> PathBuf {
    let bin_dir = project_root.join("node_modules/.bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script_path = bin_dir.join("tolgee");
    let script = format!(
        r#"#!/bin/sh
config=""
subcommand=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --config)
      config="$2"
      shift 2
      ;;
    pull|push)
      subcommand="$1"
      shift
      ;;
    *)
      shift
      ;;
  esac
done

if [ -z "$config" ] || [ -z "$subcommand" ]; then
  echo "missing config or subcommand" >&2
  exit 1
fi

echo "$subcommand|$config" >> "{log_path}"
echo "env|${{TOLGEE_API_KEY-}}" >> "{log_path}"
cp "$config" "{capture_path}"

if [ "$subcommand" = "push" ]; then
  exit 0
fi

eval "$(
python3 - "$config" <<'PY'
import json
import shlex
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    data = json.load(fh)

pull_path = data.get("pull", {{}}).get("path", "")
namespaces = data.get("pull", {{}}).get("namespaces") or data.get("push", {{}}).get("namespaces") or []
if namespaces:
    namespace = namespaces[0]
else:
    files = data.get("push", {{}}).get("files") or []
    namespace = files[0]["namespace"] if files else ""

print(f"pull_path={{shlex.quote(pull_path)}}")
print(f"namespace={{shlex.quote(namespace)}}")
PY
)"

mkdir -p "$pull_path/$namespace"
cp "{payload_path}" "$pull_path/$namespace/Localizable.xcstrings"
"#,
        payload_path = payload_path.display(),
        capture_path = capture_path.display(),
        log_path = log_path.display(),
    );
    fs::write(&script_path, script).unwrap();
    #[cfg(unix)]
    make_executable(&script_path);
    script_path
}

fn write_tolgee_config(project_root: &Path, file_path: &str) -> PathBuf {
    let config_path = project_root.join(".tolgeerc.json");
    fs::write(
        &config_path,
        format!(
            r#"{{
  "format": "APPLE_XCSTRINGS",
  "push": {{
    "files": [
      {{
        "path": "{file_path}",
        "namespace": "Core"
      }}
    ]
  }},
  "pull": {{
    "path": "./tolgee-temp",
    "fileStructureTemplate": "/{{namespace}}/Localizable.{{extension}}"
  }}
}}"#
        ),
    )
    .unwrap();
    config_path
}

fn write_langcodec_tolgee_config(project_root: &Path, file_path: &str) -> PathBuf {
    let config_path = project_root.join("langcodec.toml");
    fs::write(
        &config_path,
        format!(
            r#"[tolgee]
project_id = 36
api_url = "https://tolgee.example/api"
api_key = "tgpak_example"
namespaces = ["Core"]

[tolgee.push]
languages = ["en"]
force_mode = "KEEP"

[[tolgee.push.files]]
path = "{file_path}"
namespace = "Core"

[tolgee.pull]
path = "./tolgee-temp"
file_structure_template = "/{{namespace}}/Localizable.{{extension}}"
"#
        ),
    )
    .unwrap();
    config_path
}

fn write_langcodec_tolgee_config_without_api_key(project_root: &Path, file_path: &str) -> PathBuf {
    let config_path = project_root.join("langcodec.toml");
    fs::write(
        &config_path,
        format!(
            r#"[tolgee]
project_id = 36
api_url = "https://tolgee.example/api"
namespaces = ["Core"]

[tolgee.push]
languages = ["en"]
force_mode = "KEEP"

[[tolgee.push.files]]
path = "{file_path}"
namespace = "Core"

[tolgee.pull]
path = "./tolgee-temp"
file_structure_template = "/{{namespace}}/Localizable.{{extension}}"
"#
        ),
    )
    .unwrap();
    config_path
}

fn write_user_tolgee_config(config_home: &Path, contents: &str) -> PathBuf {
    let config_path = config_home.join("langcodec").join("config.toml");
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    fs::write(&config_path, contents).unwrap();
    config_path
}

fn write_local_catalog(path: &Path) {
    fs::write(
        path,
        r#"{
  "sourceLanguage" : "en",
  "version" : "1.0",
  "strings" : {
    "welcome" : {
      "localizations" : {
        "en" : {
          "stringUnit" : {
            "state" : "translated",
            "value" : "Welcome"
          }
        }
      }
    }
  }
}"#,
    )
    .unwrap();
}

fn write_pulled_payload(path: &Path, include_extra_key: bool) {
    let extra = if include_extra_key {
        r#",
    "new_only" : {
      "localizations" : {
        "fr" : {
          "stringUnit" : {
            "state" : "translated",
            "value" : "Nouveau"
          }
        }
      }
    }"#
    } else {
        ""
    };
    fs::write(
        path,
        format!(
            r#"{{
  "sourceLanguage" : "en",
  "version" : "1.0",
  "strings" : {{
    "welcome" : {{
      "localizations" : {{
        "fr" : {{
          "stringUnit" : {{
            "state" : "translated",
            "value" : "Bienvenue"
          }}
        }}
      }}
    }}{extra}
  }}
}}"#
        ),
    )
    .unwrap();
}

#[test]
fn tolgee_pull_merges_only_existing_keys() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();
    let local_catalog = project_root.join("Localizable.xcstrings");
    let payload = project_root.join("pull_payload.xcstrings");
    let capture = project_root.join("captured_config.json");
    let log = project_root.join("tolgee.log");

    write_local_catalog(&local_catalog);
    write_pulled_payload(&payload, true);
    let config = write_tolgee_config(project_root, "Localizable.xcstrings");
    write_fake_tolgee(project_root, &payload, &capture, &log);

    let output = langcodec_cmd()
        .current_dir(project_root)
        .args(["tolgee", "pull", "--config", config.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Preparing Tolgee pull for 1 namespace(s): Core"));
    assert!(
        stdout
            .contains("Pulling Tolgee catalogs into a temporary workspace before merging locally")
    );
    assert!(stdout.contains("Merging namespace 'Core' into Localizable.xcstrings"));

    let written = fs::read_to_string(&local_catalog).unwrap();
    assert!(written.contains("\"fr\""));
    assert!(written.contains("\"Bienvenue\""));
    assert!(!written.contains("new_only"));
}

#[test]
fn tolgee_push_uses_filtered_overlay_config() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();
    let local_catalog = project_root.join("Localizable.xcstrings");
    let payload = project_root.join("pull_payload.xcstrings");
    let capture = project_root.join("captured_config.json");
    let log = project_root.join("tolgee.log");

    write_local_catalog(&local_catalog);
    write_pulled_payload(&payload, false);
    let config = write_tolgee_config(project_root, "Localizable.xcstrings");
    write_fake_tolgee(project_root, &payload, &capture, &log);

    let output = langcodec_cmd()
        .current_dir(project_root)
        .args([
            "tolgee",
            "push",
            "--config",
            config.to_str().unwrap(),
            "--namespace",
            "Core",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Preparing Tolgee push for 1 namespace(s): Core"));
    assert!(stdout.contains("Validating mapped xcstrings files before upload"));
    assert!(stdout.contains("Uploading catalogs to Tolgee"));
    assert!(stdout.contains("Tolgee push complete: Core"));

    let captured = fs::read_to_string(&capture).unwrap();
    assert!(captured.contains("\"namespaces\""));
    assert!(captured.contains("\"Core\""));

    let log_line = fs::read_to_string(&log).unwrap();
    assert!(log_line.contains("push|"));
    assert!(!log_line.contains(".tolgeerc.json\n"));
}

#[test]
fn translate_with_tolgee_prefill_succeeds_without_provider_when_fully_filled() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();
    let local_catalog = project_root.join("Localizable.xcstrings");
    let payload = project_root.join("pull_payload.xcstrings");
    let capture = project_root.join("captured_config.json");
    let log = project_root.join("tolgee.log");

    write_local_catalog(&local_catalog);
    write_pulled_payload(&payload, false);
    let config = write_tolgee_config(project_root, "Localizable.xcstrings");
    write_fake_tolgee(project_root, &payload, &capture, &log);

    let output = langcodec_cmd()
        .current_dir(project_root)
        .args([
            "translate",
            "--source",
            local_catalog.to_str().unwrap(),
            "--source-lang",
            "en",
            "--target-lang",
            "fr",
            "--tolgee",
            "--tolgee-config",
            config.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let written = fs::read_to_string(&local_catalog).unwrap();
    assert!(written.contains("\"fr\""));
    assert!(written.contains("\"Bienvenue\""));
}

#[test]
fn tolgee_pull_uses_langcodec_toml_without_tolgeerc_json() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();
    let local_catalog = project_root.join("Localizable.xcstrings");
    let payload = project_root.join("pull_payload.xcstrings");
    let capture = project_root.join("captured_config.json");
    let log = project_root.join("tolgee.log");

    write_local_catalog(&local_catalog);
    write_pulled_payload(&payload, false);
    write_langcodec_tolgee_config(project_root, "Localizable.xcstrings");
    write_fake_tolgee(project_root, &payload, &capture, &log);

    let output = langcodec_cmd()
        .current_dir(project_root)
        .args(["tolgee", "pull"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let written = fs::read_to_string(&local_catalog).unwrap();
    assert!(written.contains("\"Bienvenue\""));

    let captured = fs::read_to_string(&capture).unwrap();
    assert!(captured.contains("\"projectId\""));
    assert!(captured.contains("\"apiUrl\""));
    assert!(captured.contains("\"tgpak_example\""));
    assert!(captured.contains("\"push\""));
    assert!(captured.contains("\"pull\""));
}

#[test]
fn tolgee_user_project_api_key_is_passed_via_env_not_overlay() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path().join("project");
    fs::create_dir_all(&project_root).unwrap();
    let config_home = temp_dir.path().join("xdg");
    let local_catalog = project_root.join("Localizable.xcstrings");
    let payload = project_root.join("pull_payload.xcstrings");
    let capture = project_root.join("captured_config.json");
    let log = project_root.join("tolgee.log");

    write_local_catalog(&local_catalog);
    write_pulled_payload(&payload, false);
    write_langcodec_tolgee_config_without_api_key(&project_root, "Localizable.xcstrings");
    write_user_tolgee_config(
        &config_home,
        r#"[tolgee]
api_key = "tgpak_user_global"

[tolgee.projects.36]
api_key = "tgpak_user_project"
"#,
    );
    write_fake_tolgee(&project_root, &payload, &capture, &log);

    let output = langcodec_cmd()
        .current_dir(&project_root)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("TOLGEE_API_KEY", "tgpak_env_should_not_win")
        .args(["tolgee", "push", "--namespace", "Core"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let log_contents = fs::read_to_string(&log).unwrap();
    assert!(log_contents.contains("env|tgpak_user_project"));

    let captured = fs::read_to_string(&capture).unwrap();
    assert!(captured.contains("\"namespaces\""));
    assert!(!captured.contains("tgpak_user_project"));
    assert!(!captured.contains("tgpak_user_global"));
    assert!(!captured.contains("\"apiKey\""));
}

#[test]
fn tolgee_project_api_key_wins_over_user_config() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path().join("project");
    fs::create_dir_all(&project_root).unwrap();
    let config_home = temp_dir.path().join("xdg");
    let local_catalog = project_root.join("Localizable.xcstrings");
    let payload = project_root.join("pull_payload.xcstrings");
    let capture = project_root.join("captured_config.json");
    let log = project_root.join("tolgee.log");

    write_local_catalog(&local_catalog);
    write_pulled_payload(&payload, false);
    write_langcodec_tolgee_config(&project_root, "Localizable.xcstrings");
    write_user_tolgee_config(
        &config_home,
        r#"[tolgee]
api_key = "tgpak_user_global"

[tolgee.projects.36]
api_key = "tgpak_user_project"
"#,
    );
    write_fake_tolgee(&project_root, &payload, &capture, &log);

    let output = langcodec_cmd()
        .current_dir(&project_root)
        .env("XDG_CONFIG_HOME", &config_home)
        .env_remove("TOLGEE_API_KEY")
        .args(["tolgee", "push", "--namespace", "Core"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let log_contents = fs::read_to_string(&log).unwrap();
    assert!(log_contents.contains("env|\n"));

    let captured = fs::read_to_string(&capture).unwrap();
    assert!(captured.contains("tgpak_example"));
    assert!(!captured.contains("tgpak_user_project"));
    assert!(!captured.contains("tgpak_user_global"));
}

#[test]
fn tolgee_env_api_key_remains_fallback_without_user_config() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path().join("project");
    fs::create_dir_all(&project_root).unwrap();
    let config_home = temp_dir.path().join("empty-xdg");
    fs::create_dir_all(&config_home).unwrap();
    let local_catalog = project_root.join("Localizable.xcstrings");
    let payload = project_root.join("pull_payload.xcstrings");
    let capture = project_root.join("captured_config.json");
    let log = project_root.join("tolgee.log");

    write_local_catalog(&local_catalog);
    write_pulled_payload(&payload, false);
    let config = write_tolgee_config(&project_root, "Localizable.xcstrings");
    write_fake_tolgee(&project_root, &payload, &capture, &log);

    let output = langcodec_cmd()
        .current_dir(&project_root)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("TOLGEE_API_KEY", "tgpak_env_fallback")
        .args(["tolgee", "push", "--config", config.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let log_contents = fs::read_to_string(&log).unwrap();
    assert!(log_contents.contains("env|tgpak_env_fallback"));
}
