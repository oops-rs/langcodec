#![forbid(unsafe_code)]
//! Universal localization file toolkit for Rust.
//!
//! Supports parsing, writing, and converting between Apple `.strings`, `.stringsdict`,
//! `.xcstrings`, `.xliff`, Android `strings.xml`, CSV, and TSV files.
//! All conversion happens through the unified `Resource` model.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use langcodec::{Codec, convert_auto};
//!
//! // Convert between formats automatically
//! convert_auto("en.lproj/Localizable.strings", "strings.xml")?;
//!
//! // Or work with the unified Resource model
//! let mut codec = Codec::new();
//! codec.read_file_by_extension("en.lproj/Localizable.strings", None)?;
//! codec.write_to_file()?;
//!
//! // Or use the builder pattern for fluent construction
//! let codec = Codec::builder()
//!     .add_file("en.lproj/Localizable.strings")?
//!     .add_file("fr.lproj/Localizable.strings")?
//!     .add_file("values-es/strings.xml")?
//!     .read_file_by_extension("de.strings", Some("de".to_string()))?
//!     .build();
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Supported Formats
//!
//! - **Apple `.strings`**: Traditional iOS/macOS localization files
//! - **Apple `.stringsdict`**: XML and binary input plus canonical XML output for one bare plural selector
//! - **Apple `.xcstrings`**: Modern Xcode localization format with plural support
//! - **Apple `.xliff`**: Xcode localization exchange files (XLIFF 1.2)
//! - **Android `strings.xml`**: Android resource files
//! - **CSV**: Conventional wide data plus an automatic versioned schema for lossless model data
//! - **TSV**: Conventional wide data plus the same automatic versioned schema
//!
//! # Features
//!
//! - ✨ Parse, write, convert, and merge multiple localization file formats
//! - 🦀 Idiomatic, modular, and ergonomic Rust API
//! - 📦 Designed for CLI tools, CI/CD pipelines, and library integration
//! - 🔄 Unified internal model (`Resource`) with explicit errors for non-representable output
//! - 📖 Well-documented, robust error handling and extensible codebase
//!
//! # Examples
//!
//! ## Basic Format Conversion
//! ```rust,no_run
//! use langcodec::convert_auto;
//!
//! // Convert Apple .strings to Android XML
//! convert_auto("en.lproj/Localizable.strings", "values-en/strings.xml")?;
//!
//! // Convert to CSV for analysis
//! convert_auto("Localizable.xcstrings", "translations.csv")?;
//! # Ok::<(), langcodec::Error>(())
//! ```
//!
//! ## Working with Resources
//! ```rust,no_run
//! use langcodec::{Codec, types::Entry};
//!
//! // Load multiple files with the builder pattern
//! let codec = Codec::builder()
//!     .add_file("en.lproj/Localizable.strings")?
//!     .add_file("fr.lproj/Localizable.strings")?
//!     .add_file("values-es/strings.xml")?
//!     .build();
//!
//! // Find specific translations
//! if let Some(en_resource) = codec.get_by_language("en") {
//!     if let Some(entry) = en_resource.entries.iter().find(|e| e.id == "welcome") {
//!         println!("Welcome message: {}", entry.value);
//!     }
//! }
//! # Ok::<(), langcodec::Error>(())
//! ```
//!
//! ## Modifying Translations
//! ```rust,no_run
//! use langcodec::{Codec, types::{Translation, EntryStatus}};
//!
//! let mut codec = Codec::builder()
//!     .add_file("en.lproj/Localizable.strings")?
//!     .add_file("fr.lproj/Localizable.strings")?
//!     .build();
//!
//! // Update an existing translation
//! codec.update_translation(
//!     "welcome_message",
//!     "en",
//!     Translation::Singular("Hello, World!".to_string()),
//!     Some(EntryStatus::Translated)
//! )?;
//!
//! // Add a new translation
//! codec.add_entry(
//!     "new_feature",
//!     "en",
//!     Translation::Singular("Check out our new feature!".to_string()),
//!     Some("Promotional message for new feature".to_string()),
//!     Some(EntryStatus::New)
//! )?;
//!
//! // Copy a translation from one language to another
//! codec.copy_entry("welcome_message", "en", "fr", true)?;
//!
//! // Find all translations for a key
//! for (resource, entry) in codec.find_entries("welcome_message") {
//!     println!("{}: {}", resource.metadata.language, entry.value);
//! }
//!
//! // Validate the codec
//! if let Err(validation_error) = codec.validate() {
//!     eprintln!("Validation failed: {}", validation_error);
//! }
//! # Ok::<(), langcodec::Error>(())
//! ```
//!
//! ## Batch Processing
//! ```rust,no_run
//! use langcodec::Codec;
//! use std::path::Path;
//!
//! let mut codec = Codec::new();
//!
//! // Load all localization files in a directory
//! for entry in std::fs::read_dir("locales")? {
//!     let path = entry?.path();
//!     if path.extension().and_then(|s| s.to_str()) == Some("strings") {
//!         codec.read_file_by_extension(&path, None)?;
//!     }
//! }
//!
//! // Write all resources back to their original formats
//! codec.write_to_file()?;
//! # Ok::<(), langcodec::Error>(())
//! ```

pub mod builder;
pub mod codec;
pub mod converter;
pub mod error;
pub mod formats;
pub mod normalize;
pub mod operations;
pub mod placeholder;
pub mod plural_rules;
pub mod provenance;
pub mod read_options;
pub mod traits;
pub mod types;

// Re-export most used types for easy consumption
pub use crate::{
    builder::CodecBuilder,
    codec::Codec,
    converter::{
        convert, convert_auto, convert_auto_with_normalization, convert_resources_to_format,
        convert_with_normalization, infer_format_from_extension, infer_format_from_path,
        infer_language_from_path, merge_resources,
    },
    error::{Error, ErrorCode, ErrorContext, StructuredError},
    formats::FormatType,
    normalize::{KeyStyle, NormalizeOptions, NormalizeReport, normalize_codec},
    operations::{
        DiffChangedItem, DiffOptions, DiffReport, DiffSummary, LanguageDiff, SyncIssue,
        SyncIssueKind, SyncOptions, SyncReport, diff_resources, sync_existing_entries,
    },
    placeholder::{extract_placeholders, normalize_placeholders, signature},
    plural_rules::{
        PluralValidationReport, autofix_fill_missing_from_other_resource,
        collect_resource_plural_issues, required_categories_for_str, validate_resource_plurals,
    },
    provenance::{
        PROVENANCE_PREFIX, ProvenanceRecord, entry_provenance, resource_provenance,
        set_entry_provenance, set_resource_provenance,
    },
    read_options::ReadOptions,
    types::{
        ConflictStrategy, Entry, EntryStatus, Metadata, Plural, PluralCategory, Resource,
        Translation,
    },
};
