use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CliConfig {
    #[serde(default)]
    pub openai: ProviderConfig,
    #[serde(default)]
    pub anthropic: ProviderConfig,
    #[serde(default)]
    pub gemini: ProviderConfig,
    #[serde(default)]
    pub tolgee: TolgeeConfig,
    #[serde(default)]
    pub translate: TranslateConfig,
    #[serde(default)]
    pub annotate: AnnotateConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProviderConfig {
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TranslateConfig {
    pub source: Option<String>,
    pub sources: Option<Vec<String>>,
    pub target: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub source_lang: Option<String>,
    pub use_tolgee: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_vec")]
    pub target_lang: Option<Vec<String>>,
    pub concurrency: Option<usize>,
    pub status: Option<Vec<String>>,
    pub output_status: Option<String>,
    #[serde(default)]
    pub input: TranslateInputConfig,
    pub output: Option<TranslateOutputScope>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TolgeeConfig {
    pub config: Option<String>,
    pub project_id: Option<u64>,
    pub api_url: Option<String>,
    pub api_key: Option<String>,
    pub format: Option<String>,
    pub schema: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_vec")]
    pub namespaces: Option<Vec<String>>,
    #[serde(default)]
    pub push: TolgeePushConfig,
    #[serde(default)]
    pub pull: TolgeePullConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TolgeePushConfig {
    #[serde(
        default,
        alias = "language",
        deserialize_with = "deserialize_optional_string_or_vec"
    )]
    pub languages: Option<Vec<String>>,
    pub force_mode: Option<String>,
    #[serde(default)]
    pub files: Vec<TolgeePushFileConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TolgeePushFileConfig {
    pub path: String,
    pub namespace: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TolgeePullConfig {
    pub path: Option<String>,
    pub file_structure_template: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserConfig {
    #[serde(default)]
    pub tolgee: UserTolgeeConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserTolgeeConfig {
    pub api_key: Option<String>,
    #[serde(default)]
    pub projects: HashMap<String, UserTolgeeProjectConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserTolgeeProjectConfig {
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TranslateInputConfig {
    pub source: Option<String>,
    pub sources: Option<Vec<String>>,
    pub lang: Option<String>,
    pub status: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TranslateOutputScope {
    Path(String),
    Config(TranslateOutputConfig),
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TranslateOutputConfig {
    pub target: Option<String>,
    pub path: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_vec")]
    pub lang: Option<Vec<String>>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AnnotateConfig {
    pub input: Option<String>,
    pub inputs: Option<Vec<String>>,
    pub source_roots: Option<Vec<String>>,
    pub output: Option<String>,
    pub source_lang: Option<String>,
    pub concurrency: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub path: PathBuf,
    pub data: CliConfig,
}

#[derive(Debug, Clone)]
pub struct LoadedUserConfig {
    pub data: UserConfig,
}

impl LoadedConfig {
    pub fn config_dir(&self) -> Option<&Path> {
        self.path.parent()
    }
}

impl CliConfig {
    pub fn provider_model(&self, provider: &str) -> Option<&str> {
        match provider.trim().to_ascii_lowercase().as_str() {
            "openai" => self.openai.model.as_deref(),
            "anthropic" => self.anthropic.model.as_deref(),
            "gemini" => self.gemini.model.as_deref(),
            _ => None,
        }
    }

    pub fn configured_provider_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.openai.model.is_some() {
            names.push("openai");
        }
        if self.anthropic.model.is_some() {
            names.push("anthropic");
        }
        if self.gemini.model.is_some() {
            names.push("gemini");
        }
        names
    }
}

impl TolgeeConfig {
    pub fn has_inline_runtime_config(&self) -> bool {
        self.project_id.is_some()
            || self.api_url.is_some()
            || self.api_key.is_some()
            || self.format.is_some()
            || self.schema.is_some()
            || self.push.languages.is_some()
            || self.push.force_mode.is_some()
            || !self.push.files.is_empty()
            || self.pull.path.is_some()
            || self.pull.file_structure_template.is_some()
    }
}

impl UserTolgeeConfig {
    pub fn api_key_for_project(&self, project_id: Option<u64>) -> Option<&str> {
        project_id
            .and_then(|id| self.projects.get(&id.to_string()))
            .and_then(|project| project.api_key.as_deref())
            .or(self.api_key.as_deref())
    }
}

impl TranslateConfig {
    pub fn resolved_source(&self) -> Option<&str> {
        self.input.source.as_deref().or(self.source.as_deref())
    }

    pub fn resolved_sources(&self) -> Option<&Vec<String>> {
        self.input.sources.as_ref().or(self.sources.as_ref())
    }

    pub fn resolved_source_lang(&self) -> Option<&str> {
        self.input.lang.as_deref().or(self.source_lang.as_deref())
    }

    pub fn resolved_filter_status(&self) -> Option<&Vec<String>> {
        self.input.status.as_ref().or(self.status.as_ref())
    }

    pub fn resolved_target(&self) -> Option<&str> {
        match self.output.as_ref() {
            Some(TranslateOutputScope::Config(config)) => {
                config.target.as_deref().or(self.target.as_deref())
            }
            _ => self.target.as_deref(),
        }
    }

    pub fn resolved_output_path(&self) -> Option<&str> {
        match self.output.as_ref() {
            Some(TranslateOutputScope::Path(path)) => Some(path.as_str()),
            Some(TranslateOutputScope::Config(config)) => config.path.as_deref(),
            None => None,
        }
    }

    pub fn resolved_target_langs(&self) -> Option<&Vec<String>> {
        match self.output.as_ref() {
            Some(TranslateOutputScope::Config(config)) => {
                config.lang.as_ref().or(self.target_lang.as_ref())
            }
            _ => self.target_lang.as_ref(),
        }
    }

    pub fn resolved_output_status(&self) -> Option<&str> {
        match self.output.as_ref() {
            Some(TranslateOutputScope::Config(config)) => {
                config.status.as_deref().or(self.output_status.as_deref())
            }
            _ => self.output_status.as_deref(),
        }
    }
}

pub fn load_config(explicit_path: Option<&str>) -> Result<Option<LoadedConfig>, String> {
    let path = match explicit_path {
        Some(path) => {
            let resolved = PathBuf::from(path);
            if !resolved.exists() {
                return Err(format!(
                    "Config file does not exist: {}",
                    resolved.display()
                ));
            }
            resolved
        }
        None => match discover_config_path()? {
            Some(path) => path,
            None => return Ok(None),
        },
    };

    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read config '{}': {}", path.display(), e))?;
    let data: CliConfig = toml::from_str(&text)
        .map_err(|e| format!("Failed to parse config '{}': {}", path.display(), e))?;
    Ok(Some(LoadedConfig { path, data }))
}

pub fn load_user_config() -> Result<Option<LoadedUserConfig>, String> {
    let Some(path) = discover_user_config_path() else {
        return Ok(None);
    };

    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read user config '{}': {}", path.display(), e))?;
    let data: UserConfig = toml::from_str(&text)
        .map_err(|e| format!("Failed to parse user config '{}': {}", path.display(), e))?;
    Ok(Some(LoadedUserConfig { data }))
}

fn discover_config_path() -> Result<Option<PathBuf>, String> {
    let mut current = std::env::current_dir()
        .map_err(|e| format!("Failed to determine current directory: {}", e))?;

    loop {
        let candidate = current.join("langcodec.toml");
        if candidate.is_file() {
            return Ok(Some(candidate));
        }

        if !current.pop() {
            return Ok(None);
        }
    }
}

fn discover_user_config_path() -> Option<PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })?;

    let candidate = config_home.join("langcodec").join("config.toml");
    candidate.is_file().then_some(candidate)
}

pub fn resolve_config_relative_path(config_dir: Option<&Path>, path: &str) -> String {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return candidate.to_string_lossy().to_string();
    }

    match config_dir {
        Some(dir) => dir.join(candidate).to_string_lossy().to_string(),
        None => candidate.to_string_lossy().to_string(),
    }
}

fn deserialize_optional_string_or_vec<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<String>),
    }

    let value = Option::<StringOrVec>::deserialize(deserializer)?;
    Ok(value.map(|value| match value {
        StringOrVec::String(value) => vec![value],
        StringOrVec::Vec(values) => values,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn cli_config_lists_provider_sections() {
        let config: CliConfig = toml::from_str(
            r#"
[openai]
model = "gpt-5.4"

[anthropic]
model = "claude-sonnet"
"#,
        )
        .expect("parse config");

        assert_eq!(
            config.configured_provider_names(),
            vec!["openai", "anthropic"]
        );
    }

    #[test]
    fn cli_config_reads_provider_specific_models() {
        let config: CliConfig = toml::from_str(
            r#"
[openai]
model = "gpt-5.4"

[anthropic]
model = "claude-sonnet"
"#,
        )
        .expect("parse config");

        assert_eq!(config.provider_model("openai"), Some("gpt-5.4"));
        assert_eq!(config.provider_model("anthropic"), Some("claude-sonnet"));
        assert_eq!(config.provider_model("gemini"), None);
    }

    #[test]
    fn resolve_config_relative_path_uses_config_dir() {
        let resolved = resolve_config_relative_path(
            Some(Path::new("/tmp/project")),
            "locales/Localizable.xcstrings",
        );
        assert_eq!(resolved, "/tmp/project/locales/Localizable.xcstrings");
    }

    #[test]
    fn load_config_parses_annotate_section() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let config_path = temp_dir.path().join("langcodec.toml");
        fs::write(
            &config_path,
            r#"
[openai]
model = "gpt-5.4"

[annotate]
input = "locales/Localizable.xcstrings"
source_roots = ["Sources", "Modules"]
concurrency = 2
"#,
        )
        .expect("write config");

        let loaded = load_config(Some(config_path.to_str().expect("config path")))
            .expect("load config")
            .expect("config present");

        assert_eq!(
            loaded.data.annotate.input.as_deref(),
            Some("locales/Localizable.xcstrings")
        );
        assert_eq!(
            loaded.data.annotate.source_roots,
            Some(vec!["Sources".to_string(), "Modules".to_string()])
        );
        assert_eq!(loaded.data.annotate.concurrency, Some(2));
    }

    #[test]
    fn load_config_parses_annotate_inputs_section() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let config_path = temp_dir.path().join("langcodec.toml");
        fs::write(
            &config_path,
            r#"
[openai]
model = "gpt-5.4"

[annotate]
inputs = ["locales/A.xcstrings", "locales/B.xcstrings"]
source_roots = ["Sources"]
concurrency = 2
"#,
        )
        .expect("write config");

        let loaded = load_config(Some(config_path.to_str().expect("config path")))
            .expect("load config")
            .expect("config present");

        assert_eq!(
            loaded.data.annotate.inputs,
            Some(vec![
                "locales/A.xcstrings".to_string(),
                "locales/B.xcstrings".to_string()
            ])
        );
    }

    #[test]
    fn load_config_parses_translate_target_lang_array() {
        let config: CliConfig = toml::from_str(
            r#"
[translate]
target_lang = ["fr", "de"]
"#,
        )
        .expect("parse config");

        assert_eq!(
            config.translate.target_lang,
            Some(vec!["fr".to_string(), "de".to_string()])
        );
    }

    #[test]
    fn load_config_preserves_legacy_translate_target_lang_string() {
        let config: CliConfig = toml::from_str(
            r#"
[translate]
target_lang = "fr,de"
"#,
        )
        .expect("parse config");

        assert_eq!(
            config.translate.target_lang,
            Some(vec!["fr,de".to_string()])
        );
    }

    #[test]
    fn load_config_parses_nested_translate_input_output_sections() {
        let config: CliConfig = toml::from_str(
            r#"
[translate.input]
source = "locales/Localizable.xcstrings"
lang = "en"
status = ["new", "stale"]

[translate.output]
target = "locales/Translated.xcstrings"
path = "build/Translated.xcstrings"
lang = ["fr", "de"]
status = "translated"
"#,
        )
        .expect("parse config");

        assert_eq!(
            config.translate.resolved_source(),
            Some("locales/Localizable.xcstrings")
        );
        assert_eq!(config.translate.resolved_source_lang(), Some("en"));
        assert_eq!(
            config.translate.resolved_filter_status(),
            Some(&vec!["new".to_string(), "stale".to_string()])
        );
        assert_eq!(
            config.translate.resolved_target(),
            Some("locales/Translated.xcstrings")
        );
        assert_eq!(
            config.translate.resolved_output_path(),
            Some("build/Translated.xcstrings")
        );
        assert_eq!(
            config.translate.resolved_target_langs(),
            Some(&vec!["fr".to_string(), "de".to_string()])
        );
        assert_eq!(
            config.translate.resolved_output_status(),
            Some("translated")
        );
    }

    #[test]
    fn load_config_parses_tolgee_defaults() {
        let config: CliConfig = toml::from_str(
            r#"
[tolgee]
config = ".tolgeerc.json"
project_id = 36
api_url = "https://tolgee.example/api"
api_key = "tgpak_example"
format = "APPLE_XCSTRINGS"
schema = "https://docs.tolgee.io/cli-schema.json"
namespaces = ["WebGame"]

[tolgee.push]
languages = ["en"]
force_mode = "KEEP"

[[tolgee.push.files]]
path = "Modules/WebGame/Localizable.xcstrings"
namespace = "WebGame"

[tolgee.pull]
path = "./tolgee-temp"
file_structure_template = "/{namespace}/Localizable.{extension}"

[translate]
use_tolgee = true
"#,
        )
        .expect("parse config");

        assert_eq!(config.tolgee.config.as_deref(), Some(".tolgeerc.json"));
        assert_eq!(config.tolgee.project_id, Some(36));
        assert_eq!(
            config.tolgee.api_url.as_deref(),
            Some("https://tolgee.example/api")
        );
        assert_eq!(config.tolgee.api_key.as_deref(), Some("tgpak_example"));
        assert_eq!(config.tolgee.format.as_deref(), Some("APPLE_XCSTRINGS"));
        assert_eq!(
            config.tolgee.schema.as_deref(),
            Some("https://docs.tolgee.io/cli-schema.json")
        );
        assert_eq!(config.tolgee.namespaces, Some(vec!["WebGame".to_string()]));
        assert_eq!(config.tolgee.push.languages, Some(vec!["en".to_string()]));
        assert_eq!(config.tolgee.push.force_mode.as_deref(), Some("KEEP"));
        assert_eq!(config.tolgee.push.files.len(), 1);
        assert_eq!(
            config.tolgee.push.files[0].path,
            "Modules/WebGame/Localizable.xcstrings"
        );
        assert_eq!(config.tolgee.pull.path.as_deref(), Some("./tolgee-temp"));
        assert_eq!(
            config.tolgee.pull.file_structure_template.as_deref(),
            Some("/{namespace}/Localizable.{extension}")
        );
        assert!(config.tolgee.has_inline_runtime_config());
        assert_eq!(config.translate.use_tolgee, Some(true));
    }

    #[test]
    fn load_config_parses_legacy_tolgee_language_alias() {
        let config: CliConfig = toml::from_str(
            r#"
[tolgee]
project_id = 36

[tolgee.push]
language = ["en"]

[[tolgee.push.files]]
path = "Modules/WebGame/Localizable.xcstrings"
namespace = "WebGame"
"#,
        )
        .expect("parse config");

        assert_eq!(config.tolgee.push.languages, Some(vec!["en".to_string()]));
    }

    #[test]
    fn user_config_parses_tolgee_global_api_key() {
        let config: UserConfig = toml::from_str(
            r#"
[tolgee]
api_key = "tgpak_user_default_key"
"#,
        )
        .expect("parse user config");

        assert_eq!(
            config.tolgee.api_key.as_deref(),
            Some("tgpak_user_default_key")
        );
        assert_eq!(
            config.tolgee.api_key_for_project(Some(36)),
            Some("tgpak_user_default_key")
        );
    }

    #[test]
    fn user_config_parses_tolgee_project_api_key() {
        let config: UserConfig = toml::from_str(
            r#"
[tolgee]
api_key = "tgpak_user_default_key"

[tolgee.projects.36]
api_key = "tgpak_project_specific_key"
"#,
        )
        .expect("parse user config");

        assert_eq!(
            config.tolgee.api_key_for_project(Some(36)),
            Some("tgpak_project_specific_key")
        );
        assert_eq!(
            config.tolgee.api_key_for_project(Some(37)),
            Some("tgpak_user_default_key")
        );
    }
}
