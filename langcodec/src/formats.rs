//! All supported localization file formats for langcodec.
//!
//! This module re-exports the main types for each format and provides
//! the [`FormatType`] enum for generic format handling across the crate.

pub mod android_strings;
pub mod csv;
pub mod strings;
pub mod stringsdict;
pub mod tsv;
pub mod xcstrings;
pub mod xliff;

use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

// Reexporting the formats for easier access
pub use android_strings::Format as AndroidStringsFormat;
pub use csv::{Format as CSVFormat, MultiLanguageCSVRecord};
pub use strings::Format as StringsFormat;
pub use stringsdict::Format as StringsdictFormat;
pub use tsv::{Format as TSVFormat, MultiLanguageTSVRecord};
pub use xcstrings::Format as XcstringsFormat;
pub use xliff::Format as XliffFormat;

use crate::Error;

/// Represents all supported localization file formats for generic handling.
///
/// This enum allows you to work with any supported file format in a type-safe way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatType {
    /// Android `strings.xml` format, with optional language code.
    AndroidStrings(Option<String>),
    /// Apple `.strings` format, with optional language code.
    Strings(Option<String>),
    /// Apple `.stringsdict` plural format, with optional language code.
    Stringsdict(Option<String>),
    /// Apple `.xcstrings` format (no language code).
    Xcstrings,
    /// Apple/Xcode `.xliff` format, with optional target language hint.
    Xliff(Option<String>),
    /// CSV format (multi-language support built-in).
    CSV,
    /// TSV format (multi-language support built-in).
    TSV,
}

/// Implements [`std::fmt::Display`] for [`FormatType`].
///
/// This provides a human-friendly string for each format type:
/// - `AndroidStrings(_)` → `"android"`
/// - `Strings(_)` → `"strings"`
/// - `Stringsdict(_)` → `"stringsdict"`
/// - `Xcstrings` → `"xcstrings"`
///
/// # Example
/// ```rust
/// use langcodec::formats::FormatType;
/// use std::fmt::Display;
/// assert_eq!(FormatType::AndroidStrings(None).to_string(), "android");
/// assert_eq!(FormatType::Strings(None).to_string(), "strings");
/// assert_eq!(FormatType::Stringsdict(None).to_string(), "stringsdict");
/// assert_eq!(FormatType::Xcstrings.to_string(), "xcstrings");
/// ```
impl Display for FormatType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatType::AndroidStrings(_) => write!(f, "android"),
            FormatType::Strings(_) => write!(f, "strings"),
            FormatType::Stringsdict(_) => write!(f, "stringsdict"),
            FormatType::Xcstrings => write!(f, "xcstrings"),
            FormatType::Xliff(_) => write!(f, "xliff"),
            FormatType::CSV => write!(f, "csv"),
            FormatType::TSV => write!(f, "tsv"),
        }
    }
}

/// Implements [`std::str::FromStr`] for [`FormatType`].
///
/// Accepts the following case-insensitive strings:
/// - `"android"`, `"androidstrings"`, `"xml"` → `FormatType::AndroidStrings(None)`
/// - `"strings"` → `FormatType::Strings(None)`
/// - `"stringsdict"` → `FormatType::Stringsdict(None)`
/// - `"xcstrings"` → `FormatType::Xcstrings`
///
/// Returns [`crate::error::Error::UnknownFormat`] for unknown strings.
///
/// # Example
/// ```rust
/// use langcodec::formats::FormatType;
/// use std::str::FromStr;
/// assert_eq!(FormatType::from_str("android").unwrap(), FormatType::AndroidStrings(None));
/// assert_eq!(FormatType::from_str("strings").unwrap(), FormatType::Strings(None));
/// assert_eq!(FormatType::from_str("stringsdict").unwrap(), FormatType::Stringsdict(None));
/// assert_eq!(FormatType::from_str("xcstrings").unwrap(), FormatType::Xcstrings);
/// assert!(FormatType::from_str("foobar").is_err());
/// ```
impl FromStr for FormatType {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim().to_ascii_lowercase();
        match s.as_str() {
            "android" | "androidstrings" | "xml" => Ok(FormatType::AndroidStrings(None)),
            "strings" => Ok(FormatType::Strings(None)),
            "stringsdict" => Ok(FormatType::Stringsdict(None)),
            "xcstrings" => Ok(FormatType::Xcstrings),
            "xliff" => Ok(FormatType::Xliff(None)),
            "csv" => Ok(FormatType::CSV),
            "tsv" => Ok(FormatType::TSV),
            other => Err(Error::UnknownFormat(other.to_string())),
        }
    }
}

impl FormatType {
    /// Returns the typical file extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            FormatType::AndroidStrings(_) => "xml",
            FormatType::Strings(_) => "strings",
            FormatType::Stringsdict(_) => "stringsdict",
            FormatType::Xcstrings => "xcstrings",
            FormatType::Xliff(_) => "xliff",
            FormatType::CSV => "csv",
            FormatType::TSV => "tsv",
        }
    }

    /// Returns the language code for this format, if available.
    pub fn language(&self) -> Option<&String> {
        match self {
            FormatType::AndroidStrings(lang) => lang.as_ref(),
            FormatType::Strings(lang) => lang.as_ref(),
            FormatType::Stringsdict(lang) => lang.as_ref(),
            FormatType::Xcstrings => None,
            FormatType::Xliff(lang) => lang.as_ref(),
            FormatType::CSV => None,
            FormatType::TSV => None,
        }
    }

    /// Recreates the format type with a new language code, if applicable.
    pub fn with_language(&self, lang: Option<String>) -> Self {
        match self {
            FormatType::AndroidStrings(_) => FormatType::AndroidStrings(lang),
            FormatType::Strings(_) => FormatType::Strings(lang),
            FormatType::Stringsdict(_) => FormatType::Stringsdict(lang),
            FormatType::Xcstrings => FormatType::Xcstrings,
            FormatType::Xliff(_) => FormatType::Xliff(lang),
            FormatType::CSV => FormatType::CSV,
            FormatType::TSV => FormatType::TSV,
        }
    }

    /// Checks if this format matches the language of another format.
    ///
    /// For `Xcstrings`, it always returns `true` since it has no language
    /// and matches any other `Xcstrings`. Note that this does not look at the
    /// actual content of the files, only the format type and its language. So if the
    /// the xcstrings file does not have that language, it will still return true.
    ///
    /// # Example
    /// ```rust
    /// use langcodec::formats::FormatType;
    /// let format1 = FormatType::AndroidStrings(Some("en".to_string()));
    /// let format2 = FormatType::AndroidStrings(Some("en".to_string()));
    /// let format3 = FormatType::Strings(Some("fr".to_string()));
    /// let format4 = FormatType::Xcstrings;
    ///
    /// assert!(format1.matches_language_of(&format2));
    /// assert!(!format1.matches_language_of(&format3));
    /// assert!(format4.matches_language_of(&format4));
    /// assert!(format4.matches_language_of(&format1));
    /// ```
    ///
    /// This is useful for ensuring that two formats can be compared or converted
    /// without language mismatch issues.
    ///
    pub fn matches_language_of(&self, other: &FormatType) -> bool {
        match (self, other) {
            // Multi-language containers match anything (both directions)
            (FormatType::Xcstrings, _) | (_, FormatType::Xcstrings) => true,
            (FormatType::Xliff(_), _) | (_, FormatType::Xliff(_)) => true,
            (FormatType::CSV, _) | (_, FormatType::CSV) => true,
            (FormatType::TSV, _) | (_, FormatType::TSV) => true,
            _ => self.language() == other.language(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_type_display() {
        assert_eq!(FormatType::AndroidStrings(None).to_string(), "android");
        assert_eq!(FormatType::Strings(None).to_string(), "strings");
        assert_eq!(FormatType::Stringsdict(None).to_string(), "stringsdict");
        assert_eq!(FormatType::Xcstrings.to_string(), "xcstrings");
        assert_eq!(FormatType::Xliff(None).to_string(), "xliff");
        assert_eq!(FormatType::CSV.to_string(), "csv");
        assert_eq!(FormatType::TSV.to_string(), "tsv");
    }

    #[test]
    fn test_format_type_from_str() {
        // Android formats
        assert_eq!(
            FormatType::from_str("android").unwrap(),
            FormatType::AndroidStrings(None)
        );
        assert_eq!(
            FormatType::from_str("ANDROID").unwrap(),
            FormatType::AndroidStrings(None)
        );
        assert_eq!(
            FormatType::from_str("androidstrings").unwrap(),
            FormatType::AndroidStrings(None)
        );
        assert_eq!(
            FormatType::from_str("xml").unwrap(),
            FormatType::AndroidStrings(None)
        );

        // Strings format
        assert_eq!(
            FormatType::from_str("strings").unwrap(),
            FormatType::Strings(None)
        );
        assert_eq!(
            FormatType::from_str("STRINGS").unwrap(),
            FormatType::Strings(None)
        );
        assert_eq!(
            FormatType::from_str("stringsdict").unwrap(),
            FormatType::Stringsdict(None)
        );
        assert_eq!(
            FormatType::from_str("STRINGSDICT").unwrap(),
            FormatType::Stringsdict(None)
        );

        // Xcstrings format
        assert_eq!(
            FormatType::from_str("xcstrings").unwrap(),
            FormatType::Xcstrings
        );
        assert_eq!(
            FormatType::from_str("XCSTRINGS").unwrap(),
            FormatType::Xcstrings
        );

        assert_eq!(
            FormatType::from_str("xliff").unwrap(),
            FormatType::Xliff(None)
        );
        assert_eq!(
            FormatType::from_str("XLIFF").unwrap(),
            FormatType::Xliff(None)
        );

        // CSV format
        assert_eq!(FormatType::from_str("csv").unwrap(), FormatType::CSV);
        assert_eq!(FormatType::from_str("CSV").unwrap(), FormatType::CSV);

        // TSV format
        assert_eq!(FormatType::from_str("tsv").unwrap(), FormatType::TSV);
        assert_eq!(FormatType::from_str("TSV").unwrap(), FormatType::TSV);
    }

    #[test]
    fn test_format_type_from_str_with_whitespace() {
        assert_eq!(
            FormatType::from_str("  android  ").unwrap(),
            FormatType::AndroidStrings(None)
        );
        assert_eq!(
            FormatType::from_str("  strings  ").unwrap(),
            FormatType::Strings(None)
        );
    }

    #[test]
    fn test_format_type_from_str_invalid() {
        assert!(FormatType::from_str("invalid").is_err());
        assert!(FormatType::from_str("foobar").is_err());
        assert!(FormatType::from_str("").is_err());
    }

    #[test]
    fn test_format_type_extension() {
        assert_eq!(FormatType::AndroidStrings(None).extension(), "xml");
        assert_eq!(FormatType::Strings(None).extension(), "strings");
        assert_eq!(FormatType::Stringsdict(None).extension(), "stringsdict");
        assert_eq!(FormatType::Xcstrings.extension(), "xcstrings");
        assert_eq!(FormatType::CSV.extension(), "csv");
        assert_eq!(FormatType::TSV.extension(), "tsv");
    }

    #[test]
    fn test_format_type_language() {
        assert_eq!(
            FormatType::AndroidStrings(Some("en".to_string())).language(),
            Some(&"en".to_string())
        );
        assert_eq!(
            FormatType::Strings(Some("fr".to_string())).language(),
            Some(&"fr".to_string())
        );
        assert_eq!(
            FormatType::Stringsdict(Some("ar".to_string())).language(),
            Some(&"ar".to_string())
        );
        assert_eq!(FormatType::Xcstrings.language(), None);
        assert_eq!(FormatType::CSV.language(), None);
        assert_eq!(FormatType::TSV.language(), None);
    }

    #[test]
    fn test_format_type_with_language() {
        let original = FormatType::AndroidStrings(None);
        let with_lang = original.with_language(Some("en".to_string()));
        assert_eq!(
            with_lang,
            FormatType::AndroidStrings(Some("en".to_string()))
        );

        let original = FormatType::Strings(Some("fr".to_string()));
        let without_lang = original.with_language(None);
        assert_eq!(without_lang, FormatType::Strings(None));

        let original = FormatType::TSV;
        let with_lang = original.with_language(Some("fr".to_string()));
        assert_eq!(with_lang, FormatType::TSV);
    }

    #[test]
    fn test_format_type_matches_language_of() {
        let format1 = FormatType::AndroidStrings(Some("en".to_string()));
        let format2 = FormatType::Strings(Some("en".to_string()));
        let format3 = FormatType::CSV;
        let format4 = FormatType::TSV;

        assert!(format1.matches_language_of(&format2));
        // CSV and TSV are multi-language formats that can match any other format
        assert!(format1.matches_language_of(&format3));
        assert!(format1.matches_language_of(&format4));
        // Xcstrings is also multi-language and should match in either direction
        assert!(format1.matches_language_of(&FormatType::Xcstrings));
        assert!(FormatType::Xcstrings.matches_language_of(&format1));
    }

    #[test]
    fn test_format_type_matches_language_of_none() {
        let format1 = FormatType::AndroidStrings(None);
        let format2 = FormatType::Strings(None);
        let format3 = FormatType::CSV;

        // When both have None language, they should match
        assert!(format1.matches_language_of(&format2));
        // CSV is a multi-language format that can match any other format
        assert!(format1.matches_language_of(&format3));
    }

    #[test]
    fn test_format_type_debug() {
        let format = FormatType::AndroidStrings(Some("en".to_string()));
        let debug = format!("{:?}", format);
        assert!(debug.contains("AndroidStrings"));
        assert!(debug.contains("en"));
    }

    #[test]
    fn test_format_type_clone() {
        let original = FormatType::Strings(Some("en".to_string()));
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_format_type_partial_eq() {
        let format1 = FormatType::AndroidStrings(Some("en".to_string()));
        let format2 = FormatType::AndroidStrings(Some("en".to_string()));
        let format3 = FormatType::AndroidStrings(Some("fr".to_string()));

        assert_eq!(format1, format2);
        assert_ne!(format1, format3);
    }
}
