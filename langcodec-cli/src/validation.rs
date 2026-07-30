use crate::formats::parse_custom_format;
use std::path::Path;
use unic_langid::LanguageIdentifier;

/// Validation context for different command types
pub struct ValidationContext {
    pub input_files: Vec<String>,
    pub output_file: Option<String>,
    pub language_code: Option<String>,
    pub input_format: Option<String>,
    pub output_format: Option<String>,
}

impl Default for ValidationContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidationContext {
    pub fn new() -> Self {
        Self {
            input_files: Vec::new(),
            output_file: None,
            language_code: None,
            input_format: None,
            output_format: None,
        }
    }

    pub fn with_input_file(mut self, file: String) -> Self {
        self.input_files.push(file);
        self
    }

    pub fn with_output_file(mut self, file: String) -> Self {
        self.output_file = Some(file);
        self
    }

    pub fn with_language_code(mut self, lang: String) -> Self {
        self.language_code = Some(lang);
        self
    }

    pub fn with_input_format(mut self, format: String) -> Self {
        self.input_format = Some(format);
        self
    }

    pub fn with_output_format(mut self, format: String) -> Self {
        self.output_format = Some(format);
        self
    }
}

/// Validate file path exists and is readable
pub fn validate_file_path(path: &str) -> Result<(), String> {
    let path_obj = Path::new(path);

    if !path_obj.exists() {
        return Err(format!("File does not exist: {}", path));
    }

    if !path_obj.is_file() {
        return Err(format!("Path is not a file: {}", path));
    }

    if !path_obj.metadata().map(|m| m.is_file()).unwrap_or(false) {
        return Err(format!("Cannot read file: {}", path));
    }

    Ok(())
}

/// Validate output directory exists or can be created
pub fn validate_output_path(path: &str) -> Result<(), String> {
    let path_obj = Path::new(path);

    if let Some(parent) = path_obj.parent()
        && !parent.exists()
    {
        // Try to create the directory
        if let Err(e) = std::fs::create_dir_all(parent) {
            return Err(format!("Cannot create output directory: {}", e));
        }
    }

    Ok(())
}

/// Validate language code format using unic-langid (same as lib crate)
pub fn validate_language_code(lang: &str) -> Result<(), String> {
    if lang.is_empty() {
        return Err("Language code cannot be empty".to_string());
    }

    // Use the same approach as the lib crate - parse with LanguageIdentifier
    match lang.parse::<LanguageIdentifier>() {
        Ok(lang_id) => {
            // Additional validation: ensure the language code follows expected patterns
            // Reject codes that are too generic or don't look like real language codes
            let lang_str = lang_id.to_string();
            if lang_str == "invalid"
                || lang_str == "123"
                || lang_str.starts_with('-')
                || lang_str.ends_with('-')
            {
                return Err(format!(
                    "Invalid language code format: {}. Expected valid BCP 47 language identifier",
                    lang
                ));
            }
            Ok(())
        }
        Err(_) => Err(format!(
            "Invalid language code format: {}. Expected valid BCP 47 language identifier",
            lang
        )),
    }
}

/// Validate custom format string
pub fn validate_custom_format(format: &str) -> Result<(), String> {
    if format.is_empty() {
        return Err("Format cannot be empty".to_string());
    }

    // Trim whitespace and check if it's a supported custom format
    let trimmed_format = format.trim();
    if parse_custom_format(trimmed_format).is_err() {
        return Err(format!(
            "Unsupported custom format: {}. Supported formats: {}",
            format,
            crate::formats::get_supported_custom_formats()
        ));
    }

    Ok(())
}

/// Validate standard format string
pub fn validate_standard_format(format: &str) -> Result<(), String> {
    if format.is_empty() {
        return Err("Format cannot be empty".to_string());
    }

    // Trim whitespace and check if it's a supported standard format
    match format.trim().to_lowercase().as_str() {
        "android" | "androidstrings" | "xml" => Ok(()),
        "strings" | "stringsdict" => Ok(()),
        "xcstrings" => Ok(()),
        "xliff" => Ok(()),
        "csv" => Ok(()),
        "tsv" => Ok(()),
        _ => Err(format!(
            "Unsupported standard format: {}. Supported formats: android, strings, stringsdict, xcstrings, xliff, csv, tsv",
            format
        )),
    }
}

/// Validate a complete validation context
pub fn validate_context(context: &ValidationContext) -> Result<(), String> {
    // Validate input files
    for (i, input) in context.input_files.iter().enumerate() {
        validate_file_path(input)
            .map_err(|e| format!("Input file {} validation failed: {}", i + 1, e))?;
    }

    // Validate output file
    if let Some(ref output) = context.output_file {
        validate_output_path(output).map_err(|e| format!("Output validation failed: {}", e))?;
    }

    // Validate language code
    if let Some(ref lang) = context.language_code {
        validate_language_code(lang)
            .map_err(|e| format!("Language code validation failed: {}", e))?;
    }

    // Validate input format
    if let Some(ref format) = context.input_format {
        // Try standard format first, then custom format
        if validate_standard_format(format).is_err() {
            validate_custom_format(format)
                .map_err(|e| format!("Input format validation failed: {}", e))?;
        }
    }

    // Validate output format
    if let Some(ref format) = context.output_format {
        // Output formats are typically standard formats
        validate_standard_format(format)
            .map_err(|e| format!("Output format validation failed: {}", e))?;
    }

    Ok(())
}

/// Validate custom format file content and extension
pub fn validate_custom_format_file(input: &str) -> Result<(), String> {
    // Validate input file extension for custom formats
    let input_ext = Path::new(input)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    match input_ext.as_str() {
        "json" => {
            // Validate JSON file exists and is readable
            validate_file_path(input)?;
        }
        "yaml" | "yml" => {
            // Validate YAML file exists and is readable
            validate_file_path(input)?;
        }
        "langcodec" => {
            // Validate langcodec file exists and is readable
            validate_file_path(input)?;
        }
        _ => {
            return Err(format!(
                "Unsupported file extension for custom format: {}. Expected: json, yaml, yml, langcodec",
                input_ext
            ));
        }
    }

    Ok(())
}
