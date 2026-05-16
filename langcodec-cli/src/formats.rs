use std::str::FromStr;

/// Custom format types that are not supported by the lib crate.
/// These are one-way conversions only (to Resource format).
#[derive(Debug, Clone, PartialEq, clap::ValueEnum)]
#[allow(clippy::enum_variant_names)]
pub enum CustomFormat {
    /// A JSON file which contains a map of language codes to translations.
    ///
    /// The key is the localization code, and the value is the translation:
    ///
    /// ```json
    /// {
    ///     "key": "hello_world",
    ///     "en": "Hello, World!",
    ///     "fr": "Bonjour, le monde!"
    /// }
    /// ```
    JSONLanguageMap,

    /// A YAML file which contains a map of language codes to translations.
    ///
    /// The key is the localization code, and the value is the translation:
    ///
    /// ```yaml
    /// key: hello_world
    /// en: Hello, World!
    /// fr: Bonjour, le monde!
    /// ```
    YAMLLanguageMap,

    /// A JSON file which contains an array of language map objects.
    ///
    /// Each object contains a key and translations for different languages:
    ///
    /// ```json
    /// [
    ///     {
    ///         "key": "hello_world",
    ///         "en": "Hello, World!",
    ///         "fr": "Bonjour, le monde!"
    ///     },
    ///     {
    ///         "key": "welcome_message",
    ///         "en": "Welcome to our app!",
    ///         "fr": "Bienvenue dans notre application!"
    ///     }
    /// ]
    /// ```
    JSONArrayLanguageMap,

    /// A JSON file which contains an array of langcodec::Resource objects.
    ///
    /// Each object is a complete Resource with metadata and entries:
    ///
    /// ```json
    /// [
    ///     {
    ///         "metadata": {
    ///             "language": "en",
    ///             "domain": "MyApp"
    ///         },
    ///         "entries": [
    ///             {
    ///                 "id": "hello_world",
    ///                 "value": "Hello, World!",
    ///                 "comment": "Welcome message"
    ///             }
    ///         ]
    ///     },
    ///     {
    ///         "metadata": {
    ///             "language": "fr",
    ///             "domain": "MyApp"
    ///         },
    ///         "entries": [
    ///             {
    ///                 "id": "hello_world",
    ///                 "value": "Bonjour, le monde!",
    ///                 "comment": "Welcome message"
    ///             }
    ///         ]
    ///     }
    /// ]
    /// ```
    LangcodecResourceArray,
}

impl FromStr for CustomFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_ascii_lowercase().replace(['-', '_'], "");
        //: cspell:disable
        match normalized.as_str() {
            "jsonlanguagemap" => Ok(CustomFormat::JSONLanguageMap),
            "jsonarraylanguagemap" => Ok(CustomFormat::JSONArrayLanguageMap),
            "yamllanguagemap" => Ok(CustomFormat::YAMLLanguageMap),
            "langcodecresourcearray" => Ok(CustomFormat::LangcodecResourceArray),
            // "csvlanguages" => Ok(CustomFormat::CSVLanguages),
            _ => Err(format!(
                "Unknown custom format: '{}'. Supported formats: json-language-map, json-array-language-map, yaml-language-map, langcodec-resource-array",
                s
            )),
        }
        //: cspell:enable
    }
}

/// Parse a custom format from a string, with helpful error messages.
pub fn parse_custom_format(s: &str) -> Result<CustomFormat, String> {
    CustomFormat::from_str(s)
}

/// Detect if a file is a custom format based on its content and extension.
/// Returns the detected custom format if found, None otherwise.
pub fn detect_custom_format(file_path: &str, file_content: &str) -> Option<CustomFormat> {
    let extension = std::path::Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    match extension.as_str() {
        "langcodec" if is_langcodec_resource_array(file_content) => {
            Some(CustomFormat::LangcodecResourceArray)
        }
        "json" if is_json_language_map(file_content) => Some(CustomFormat::JSONLanguageMap),
        "json" if serde_json::from_str::<Vec<serde_json::Value>>(file_content).is_ok() => {
            Some(CustomFormat::JSONArrayLanguageMap)
        }
        "yaml" | "yml" if serde_yaml::from_str::<serde_yaml::Value>(file_content).is_ok() => {
            Some(CustomFormat::YAMLLanguageMap)
        }
        _ => None,
    }
}

fn is_langcodec_resource_array(file_content: &str) -> bool {
    let Ok(array) = serde_json::from_str::<Vec<serde_json::Value>>(file_content) else {
        return false;
    };
    let Some(first) = array.first() else {
        return false;
    };

    first
        .as_object()
        .is_some_and(|obj| obj.contains_key("metadata") && obj.contains_key("entries"))
}

fn is_json_language_map(file_content: &str) -> bool {
    serde_json::from_str::<std::collections::HashMap<String, serde_json::Value>>(file_content)
        .is_ok_and(|obj| !obj.is_empty())
}

/// Validate custom format file content
pub fn validate_custom_format_content(
    file_path: &str,
    file_content: &str,
) -> Result<CustomFormat, String> {
    if file_content.trim().is_empty() {
        return Err("File content is empty".to_string());
    }

    if let Some(format) = detect_custom_format(file_path, file_content) {
        Ok(format)
    } else {
        Err(format!(
            "Could not detect custom format from file content. Supported formats: {}",
            get_supported_custom_formats()
        ))
    }
}

/// Get a list of all supported custom formats for help messages.
pub fn get_supported_custom_formats() -> &'static str {
    "json-language-map, json-array-language-map, yaml-language-map, langcodec-resource-array"
}
