/// Builder for creating a `Codec` instance with a fluent interface.
///
/// This builder allows you to chain method calls to add resources from files
/// and then build the final `Codec` instance.
///
/// # Example
///
/// ```rust,no_run
/// use langcodec::{Codec, formats::FormatType};
///
/// let codec = Codec::builder()
///     .add_file("en.strings")?
///     .add_file("fr.strings")?
///     .add_file_with_format("de.xml", FormatType::AndroidStrings(Some("de".to_string())))?
///     .read_file_by_extension("es.strings", Some("es".to_string()))?
///     .build();
/// # Ok::<(), langcodec::Error>(())
/// ```
use crate::formats::{CSVFormat, TSVFormat, XliffFormat};
use crate::{error::Error, formats::*, traits::Parser, types::Resource};
use std::path::Path;

pub struct CodecBuilder {
    resources: Vec<Resource>,
}

impl CodecBuilder {
    /// Creates a new `CodecBuilder` with no resources.
    pub fn new() -> Self {
        Self {
            resources: Vec::new(),
        }
    }

    /// Adds a resource file by inferring its format from the file extension.
    ///
    /// The language will be automatically inferred from the file path if possible.
    /// For example, `en.lproj/Localizable.strings` will be detected as English.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the resource file
    ///
    /// # Returns
    ///
    /// Returns `self` for method chaining, or an `Error` if the file cannot be read.
    pub fn add_file<P: AsRef<Path>>(mut self, path: P) -> Result<Self, Error> {
        let path = path.as_ref();
        let format_type = super::converter::infer_format_from_path(path).ok_or_else(|| {
            Error::UnknownFormat(format!(
                "Cannot infer format from file extension: {:?}",
                path.extension()
            ))
        })?;

        let language = super::converter::infer_language_from_path(path, &format_type)?;
        let domain = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        let mut preserve_parsed_metadata = false;
        let mut new_resources = match &format_type {
            FormatType::Strings(_) => {
                vec![Resource::from(StringsFormat::read_from(path)?)]
            }
            FormatType::Stringsdict(_) => {
                vec![Resource::from(StringsdictFormat::read_from(path)?)]
            }
            FormatType::AndroidStrings(_) => {
                vec![Resource::from(AndroidStringsFormat::read_from(path)?)]
            }
            FormatType::Xcstrings => Vec::<Resource>::try_from(XcstringsFormat::read_from(path)?)?,
            FormatType::Xliff(_) => Vec::<Resource>::try_from(XliffFormat::read_from(path)?)?,
            FormatType::CSV => {
                // Parse CSV format and convert to resources
                let csv_format = CSVFormat::read_from(path)?;
                preserve_parsed_metadata = csv_format.is_extended();
                Vec::<Resource>::try_from(csv_format)?
            }
            FormatType::TSV => {
                // Parse TSV format and convert to resources
                let tsv_format = TSVFormat::read_from(path)?;
                preserve_parsed_metadata = tsv_format.is_extended();
                Vec::<Resource>::try_from(tsv_format)?
            }
        };

        let should_override_language = matches!(
            format_type,
            FormatType::Strings(_) | FormatType::Stringsdict(_) | FormatType::AndroidStrings(_)
        );

        for new_resource in &mut new_resources {
            if should_override_language && let Some(ref lang) = language {
                new_resource.metadata.language = lang.clone();
            }
            if !preserve_parsed_metadata {
                new_resource.metadata.domain = domain.clone();
                new_resource
                    .metadata
                    .custom
                    .insert("format".to_string(), format_type.to_string());
            }
        }

        self.resources.extend(new_resources);
        Ok(self)
    }

    /// Adds a resource file with a specific format and optional language override.
    ///
    /// This method allows you to specify the format explicitly and optionally
    /// override the language that would be inferred from the file path.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the resource file
    /// * `format_type` - The format type to use for parsing
    ///
    /// # Returns
    ///
    /// Returns `self` for method chaining, or an `Error` if the file cannot be read.
    pub fn add_file_with_format<P: AsRef<Path>>(
        mut self,
        path: P,
        format_type: FormatType,
    ) -> Result<Self, Error> {
        let language = format_type.language().cloned().or_else(|| {
            super::converter::infer_language_from_path(&path, &format_type)
                .ok()
                .flatten()
        });
        let domain = path
            .as_ref()
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let path = path.as_ref();

        let mut preserve_parsed_metadata = false;
        let mut new_resources = match &format_type {
            FormatType::Strings(_) => {
                vec![Resource::from(StringsFormat::read_from(path)?)]
            }
            FormatType::Stringsdict(_) => {
                vec![Resource::from(StringsdictFormat::read_from(path)?)]
            }
            FormatType::AndroidStrings(_) => {
                vec![Resource::from(AndroidStringsFormat::read_from(path)?)]
            }
            FormatType::Xcstrings => Vec::<Resource>::try_from(XcstringsFormat::read_from(path)?)?,
            FormatType::Xliff(_) => Vec::<Resource>::try_from(XliffFormat::read_from(path)?)?,
            FormatType::CSV => {
                // Parse CSV format and convert to resources
                let csv_format = CSVFormat::read_from(path)?;
                preserve_parsed_metadata = csv_format.is_extended();
                Vec::<Resource>::try_from(csv_format)?
            }
            FormatType::TSV => {
                // Parse TSV format and convert to resources
                let tsv_format = TSVFormat::read_from(path)?;
                preserve_parsed_metadata = tsv_format.is_extended();
                Vec::<Resource>::try_from(tsv_format)?
            }
        };

        let should_override_language = matches!(
            format_type,
            FormatType::Strings(_) | FormatType::Stringsdict(_) | FormatType::AndroidStrings(_)
        );

        for new_resource in &mut new_resources {
            if should_override_language && let Some(ref lang) = language {
                new_resource.metadata.language = lang.clone();
            }
            if !preserve_parsed_metadata {
                new_resource.metadata.domain = domain.clone();
                new_resource
                    .metadata
                    .custom
                    .insert("format".to_string(), format_type.to_string());
            }
        }

        self.resources.extend(new_resources);
        Ok(self)
    }

    /// Adds a resource file by inferring its format from the file extension with optional language override.
    ///
    /// This method is similar to `add_file` but allows you to specify a language
    /// that will override any language inferred from the file path.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the resource file
    /// * `lang` - Optional language code to use (overrides path inference)
    ///
    /// # Returns
    ///
    /// Returns `self` for method chaining, or an `Error` if the file cannot be read.
    pub fn read_file_by_extension<P: AsRef<Path>>(
        self,
        path: P,
        lang: Option<String>,
    ) -> Result<Self, Error> {
        let format_type = match path.as_ref().extension().and_then(|s| s.to_str()) {
            Some("xml") => FormatType::AndroidStrings(lang),
            Some("strings") => FormatType::Strings(lang),
            Some("stringsdict") => FormatType::Stringsdict(lang),
            Some("xcstrings") => FormatType::Xcstrings,
            Some("xliff") => FormatType::Xliff(lang),
            Some("csv") => FormatType::CSV,
            Some("tsv") => FormatType::TSV,
            extension => {
                return Err(Error::UnsupportedFormat(format!(
                    "Unsupported file extension: {:?}.",
                    extension
                )));
            }
        };

        self.add_file_with_format(path, format_type)
    }

    /// Adds a resource directly to the builder.
    ///
    /// This method allows you to add a `Resource` instance directly, which is useful
    /// when you have resources that were created programmatically or loaded from
    /// other sources.
    ///
    /// # Arguments
    ///
    /// * `resource` - The resource to add
    ///
    /// # Returns
    ///
    /// Returns `self` for method chaining.
    pub fn add_resource(mut self, resource: Resource) -> Self {
        self.resources.push(resource);
        self
    }

    /// Adds multiple resources directly to the builder.
    ///
    /// This method allows you to add multiple `Resource` instances at once.
    ///
    /// # Arguments
    ///
    /// * `resources` - Iterator of resources to add
    ///
    /// # Returns
    ///
    /// Returns `self` for method chaining.
    pub fn add_resources<I>(mut self, resources: I) -> Self
    where
        I: IntoIterator<Item = Resource>,
    {
        self.resources.extend(resources);
        self
    }

    /// Loads resources from a JSON cache file.
    ///
    /// This method loads resources that were previously cached using `Codec::cache_to_file`.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the JSON cache file
    ///
    /// # Returns
    ///
    /// Returns `self` for method chaining, or an `Error` if the file cannot be read.
    pub fn load_from_cache<P: AsRef<Path>>(mut self, path: P) -> Result<Self, Error> {
        let mut reader = std::fs::File::open(path).map_err(Error::Io)?;
        let cached_resources: Vec<Resource> =
            serde_json::from_reader(&mut reader).map_err(Error::Parse)?;
        self.resources.extend(cached_resources);
        Ok(self)
    }

    /// Builds the final `Codec` instance.
    ///
    /// This method consumes the builder and returns the constructed `Codec`.
    ///
    /// # Returns
    ///
    /// Returns the constructed `Codec` instance.
    pub fn build(self) -> super::codec::Codec {
        super::codec::Codec {
            resources: self.resources,
        }
    }

    /// Builds the final `Codec` instance and validates it.
    ///
    /// This method is similar to `build()` but performs additional validation
    /// on the resources before returning the `Codec`.
    ///
    /// # Returns
    ///
    /// Returns the constructed `Codec` instance, or an `Error` if validation fails.
    pub fn build_and_validate(self) -> Result<super::codec::Codec, Error> {
        let codec = self.build();

        // Validate that all resources have a language
        for (i, resource) in codec.resources.iter().enumerate() {
            if resource.metadata.language.is_empty() {
                return Err(Error::Validation(format!(
                    "Resource at index {} has no language specified",
                    i
                )));
            }
        }

        // Check for duplicate languages
        let mut languages = std::collections::HashSet::new();
        for resource in &codec.resources {
            if !languages.insert(&resource.metadata.language) {
                return Err(Error::Validation(format!(
                    "Duplicate language found: {}",
                    resource.metadata.language
                )));
            }
        }

        Ok(codec)
    }
}

impl Default for CodecBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_builder_read_file_by_extension() {
        // Create a temporary strings file using tempfile
        let temp_file = NamedTempFile::new().unwrap();
        let test_file = temp_file.path().with_extension("strings");

        let content = r#"/* English localization */
"hello" = "Hello";
"goodbye" = "Goodbye";
"thanks" = "Thank you!";
"#;

        // Write the test file
        std::fs::write(&test_file, content).unwrap();

        // Test the builder with read_file_by_extension
        let result = CodecBuilder::new()
            .read_file_by_extension(&test_file, Some("en".to_string()))
            .unwrap()
            .build();

        // Verify the result
        assert_eq!(result.resources.len(), 1);
        let resource = &result.resources[0];
        assert_eq!(resource.metadata.language, "en");
        assert_eq!(resource.entries.len(), 3);

        // Clean up - tempfile will automatically clean up when temp_file goes out of scope
        let _ = std::fs::remove_file(&test_file);
    }
}
