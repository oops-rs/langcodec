use crate::formats::{self, parse_custom_format};
use crate::transformers::custom_format_to_resource;
use crate::ui;
use crate::validation::{self, validate_custom_format_file};

use langcodec::{Codec, ReadOptions, convert_auto, formats::FormatType};
use std::fs::File;
use std::io::BufWriter;

#[derive(Debug, Clone)]
pub struct ConvertOptions {
    pub input_format: Option<String>,
    pub output_format: Option<String>,
    pub source_language: Option<String>,
    pub version: Option<String>,
    pub output_lang: Option<String>,
    pub exclude_lang: Vec<String>,
    pub include_lang: Vec<String>,
}

fn parse_standard_input_format(format: &str) -> Option<FormatType> {
    match format.trim().to_ascii_lowercase().as_str() {
        "strings" => Some(FormatType::Strings(None)),
        "stringsdict" => Some(FormatType::Stringsdict(None)),
        "android" | "androidstrings" => Some(FormatType::AndroidStrings(None)),
        "xcstrings" => Some(FormatType::Xcstrings),
        "xliff" => Some(FormatType::Xliff(None)),
        "csv" => Some(FormatType::CSV),
        "tsv" => Some(FormatType::TSV),
        _ => None,
    }
}

fn is_single_language_format(format: &FormatType) -> bool {
    matches!(
        format,
        FormatType::Strings(_) | FormatType::Stringsdict(_) | FormatType::AndroidStrings(_)
    )
}

fn read_standard_resources(
    input: &str,
    format: FormatType,
    input_language_hint: Option<&String>,
    strict: bool,
    explicit_format: bool,
) -> Result<Vec<langcodec::Resource>, String> {
    let language_hint = if is_single_language_format(&format) {
        input_language_hint
            .map(|language| language.trim())
            .map(|language| {
                if language.is_empty() {
                    Err(
                        "--source-language cannot be empty when used as an input language hint"
                            .to_string(),
                    )
                } else {
                    Ok(language.to_string())
                }
            })
            .transpose()?
    } else {
        None
    };
    let read_options = ReadOptions::new()
        .with_language_hint(language_hint)
        .with_strict(strict);
    let mut codec = Codec::new();
    codec
        .read_file_by_type_with_options(input, format, &read_options)
        .map_err(|error| {
            if explicit_format {
                format!("Failed to read input with explicit format: {error}")
            } else {
                format!("Failed to read input: {error}")
            }
        })?;
    Ok(codec.resources)
}

fn parse_standard_output_format(format: &str) -> Result<FormatType, String> {
    match format.trim().to_ascii_lowercase().as_str() {
        "strings" => Ok(FormatType::Strings(None)),
        "stringsdict" => Ok(FormatType::Stringsdict(None)),
        "android" | "androidstrings" => Ok(FormatType::AndroidStrings(None)),
        "xcstrings" => Ok(FormatType::Xcstrings),
        "xliff" => Ok(FormatType::Xliff(None)),
        "csv" => Ok(FormatType::CSV),
        "tsv" => Ok(FormatType::TSV),
        _ => Err(format!(
            "Unsupported output format: '{}'. Supported formats: strings, stringsdict, android, xcstrings, xliff, csv, tsv",
            format
        )),
    }
}

fn wants_named_output(
    output: &str,
    output_format_hint: Option<&String>,
    extension: &str,
    format_name: &str,
) -> bool {
    output_format_hint.map_or_else(
        || output.ends_with(extension),
        |hint| hint.trim().eq_ignore_ascii_case(format_name),
    )
}

fn wants_xcstrings_output(output: &str, output_format_hint: Option<&String>) -> bool {
    wants_named_output(output, output_format_hint, ".xcstrings", "xcstrings")
}

fn wants_xliff_output(output: &str, output_format_hint: Option<&String>) -> bool {
    wants_named_output(output, output_format_hint, ".xliff", "xliff")
}

fn resolve_xliff_source_language(
    resources: &[langcodec::Resource],
    explicit_source_language: Option<&String>,
    target_language: &str,
) -> Result<String, String> {
    if let Some(explicit_source_language) = explicit_source_language {
        let trimmed = explicit_source_language.trim();
        if trimmed.is_empty() {
            return Err("--source-language cannot be empty for .xliff output".to_string());
        }
        return Ok(trimmed.to_string());
    }

    let metadata_source_languages = resources
        .iter()
        .filter_map(|resource| resource.metadata.custom.get("source_language"))
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<std::collections::BTreeSet<_>>();

    let available_languages = resources
        .iter()
        .map(|resource| resource.metadata.language.trim())
        .filter(|language| !language.is_empty())
        .collect::<std::collections::BTreeSet<_>>();

    if metadata_source_languages.len() > 1 {
        return Err(format!(
            "Conflicting source_language metadata found for .xliff output: {}. Pass --source-language.",
            metadata_source_languages
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(source_language) = metadata_source_languages.iter().next() {
        let extras = available_languages
            .iter()
            .filter(|language| **language != *source_language && **language != target_language)
            .cloned()
            .collect::<Vec<_>>();

        if *source_language != target_language && extras.is_empty() {
            return Ok((*source_language).to_string());
        }

        return Err(format!(
            "source_language metadata '{}' is ambiguous for .xliff output with available languages ({}). Pass --source-language.",
            source_language,
            available_languages
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if available_languages.is_empty() {
        return Err("XLIFF output requires language metadata on the input resources".to_string());
    }

    if available_languages.len() == 1 {
        return Ok(available_languages.iter().next().unwrap().to_string());
    }

    let non_target_languages = available_languages
        .iter()
        .filter(|language| **language != target_language)
        .cloned()
        .collect::<Vec<_>>();

    match non_target_languages.as_slice() {
        [source_language] => Ok((*source_language).to_string()),
        _ => Err(format!(
            "Could not infer the XLIFF source language from available languages ({}). Pass --source-language.",
            available_languages
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn infer_output_path_language(path: &str) -> Option<String> {
    match langcodec::infer_format_from_path(path) {
        Some(FormatType::Strings(Some(lang)))
        | Some(FormatType::Stringsdict(Some(lang)))
        | Some(FormatType::AndroidStrings(Some(lang))) => Some(lang),
        _ => None,
    }
}

fn resolve_convert_output_format(
    output: &str,
    output_format_hint: Option<&String>,
    output_lang: Option<&String>,
) -> Result<FormatType, String> {
    let mut output_format = if let Some(format_hint) = output_format_hint {
        parse_standard_output_format(format_hint)?
    } else {
        langcodec::infer_format_from_path(output)
            .or_else(|| langcodec::infer_format_from_extension(output))
            .ok_or_else(|| format!("Cannot infer output format from extension: {}", output))?
    };

    let path_language = infer_output_path_language(output);

    match &output_format {
        FormatType::Strings(_) | FormatType::Stringsdict(_) | FormatType::AndroidStrings(_) => {
            if let Some(language) = output_lang {
                if let Some(path_language) = path_language
                    && path_language != *language
                {
                    return Err(format!(
                        "--output-lang '{}' conflicts with language '{}' implied by output path '{}'",
                        language, path_language, output
                    ));
                }
                output_format = output_format.with_language(Some(language.clone()));
            } else if let Some(path_language) = path_language {
                output_format = output_format.with_language(Some(path_language));
            }
            Ok(output_format)
        }
        FormatType::Xliff(_) => {
            if let Some(language) = output_lang {
                output_format = output_format.with_language(Some(language.clone()));
                Ok(output_format)
            } else {
                Err(
                    ".xliff output requires --output-lang to select the target language"
                        .to_string(),
                )
            }
        }
        FormatType::Xcstrings | FormatType::CSV | FormatType::TSV => {
            if let Some(language) = output_lang {
                Err(format!(
                    "--output-lang '{}' is only supported for .strings, .stringsdict, strings.xml, or .xliff output",
                    language
                ))
            } else {
                Ok(output_format)
            }
        }
    }
}

pub fn run_unified_convert_command(
    input: String,
    output: String,
    options: ConvertOptions,
    strict: bool,
) {
    let explicit_standard_input = options
        .input_format
        .as_deref()
        .and_then(parse_standard_input_format);
    let inferred_standard_input = langcodec::infer_format_from_path(&input);
    let resolved_standard_input = if options.input_format.is_some() {
        explicit_standard_input.as_ref()
    } else {
        inferred_standard_input.as_ref()
    };
    let missing_source_language = options
        .source_language
        .as_ref()
        .is_none_or(|language| language.trim().is_empty());

    if let Some(format) = resolved_standard_input
        && matches!(format, FormatType::Stringsdict(_))
        && missing_source_language
        && langcodec::converter::infer_language_from_path(&input, format)
            .ok()
            .flatten()
            .is_none()
    {
        println!(
            "{}",
            ui::status_line_stdout(ui::Tone::Error, "Conversion failed")
        );
        eprintln!(
            "Error: Could not infer the input language for '{}'. Pass --source-language <LANG> or place the .stringsdict file under a <LANG>.lproj directory.",
            input
        );
        std::process::exit(1);
    }

    let wants_xliff = wants_xliff_output(&output, options.output_format.as_ref());
    if wants_xliff {
        println!(
            "{}",
            ui::status_line_stdout(
                ui::Tone::Info,
                "Converting to XLIFF 1.2 with explicit source/target language selection...",
            )
        );
        match read_resources_from_any_input(
            &input,
            options.input_format.as_ref(),
            options.source_language.as_ref(),
            strict,
        )
        .and_then(|mut resources| {
            let output_format = resolve_convert_output_format(
                &output,
                options.output_format.as_ref(),
                options.output_lang.as_ref(),
            )?;
            let target_language = match &output_format {
                FormatType::Xliff(Some(target_language)) => target_language.clone(),
                _ => {
                    return Err(
                        ".xliff output requires --output-lang to select the target language"
                            .to_string(),
                    );
                }
            };
            let source_language = resolve_xliff_source_language(
                &resources,
                options.source_language.as_ref(),
                &target_language,
            )?;

            for resource in &mut resources {
                resource
                    .metadata
                    .custom
                    .insert("source_language".to_string(), source_language.clone());
            }

            convert_resources_to_format(resources, &output, output_format)
                .map_err(|e| format!("Error converting to xliff: {}", e))
        }) {
            Ok(()) => {
                println!(
                    "{}",
                    ui::status_line_stdout(ui::Tone::Success, "Successfully converted to xliff",)
                );
                return;
            }
            Err(e) => {
                println!(
                    "{}",
                    ui::status_line_stdout(ui::Tone::Error, "Conversion to xliff failed")
                );
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }

    if let Some(output_lang) = options.output_lang.as_ref() {
        if options.output_format.is_none() && output.ends_with(".langcodec") {
            eprintln!(
                "Error: --output-lang '{}' is not supported for .langcodec output. Use --include-lang instead.",
                output_lang
            );
            std::process::exit(1);
        }

        println!(
            "{}",
            ui::status_line_stdout(
                ui::Tone::Info,
                &format!("Converting with explicit output language '{}'", output_lang),
            )
        );
        match read_resources_from_any_input(
            &input,
            options.input_format.as_ref(),
            options.source_language.as_ref(),
            strict,
        )
        .and_then(|resources| {
            let output_format = resolve_convert_output_format(
                &output,
                options.output_format.as_ref(),
                options.output_lang.as_ref(),
            )?;
            convert_resources_to_format(resources, &output, output_format)
                .map_err(|e| format!("Error converting to output format: {}", e))
        }) {
            Ok(()) => {
                println!(
                    "{}",
                    ui::status_line_stdout(
                        ui::Tone::Success,
                        "Successfully converted with explicit output language",
                    )
                );
                return;
            }
            Err(e) => {
                println!(
                    "{}",
                    ui::status_line_stdout(ui::Tone::Error, "Conversion failed")
                );
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }

    // Special handling: when targeting xcstrings, ensure required metadata exists.
    // If source_language/version are missing, default to en/1.0 respectively.
    let wants_xcstrings = wants_xcstrings_output(&output, options.output_format.as_ref());
    if wants_xcstrings {
        println!(
            "{}",
            ui::status_line_stdout(
                ui::Tone::Info,
                "Converting to xcstrings with default sourceLanguage if missing...",
            )
        );
        match read_resources_from_any_input(
            &input,
            options.input_format.as_ref(),
            options.source_language.as_ref(),
            strict,
        )
        .and_then(|mut resources| {
            // Determine source_language priority: explicit flag > metadata > default
            let source_language = options
                .source_language
                .as_ref()
                .and_then(|s| {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    resources.first().and_then(|r| {
                        r.metadata
                            .custom
                            .get("source_language")
                            .cloned()
                            .filter(|s| !s.trim().is_empty())
                    })
                })
                .unwrap_or_else(|| "en".to_string());
            // Determine version: keep existing if present; otherwise default to "1.0"
            let version = options
                .version
                .clone()
                .or_else(|| {
                    resources
                        .first()
                        .and_then(|r| r.metadata.custom.get("version").cloned())
                })
                .unwrap_or_else(|| "1.0".to_string());

            // Apply to all resources so the writer has consistent metadata
            for r in &mut resources {
                r.metadata
                    .custom
                    .insert("source_language".to_string(), source_language.clone());
                r.metadata
                    .custom
                    .insert("version".to_string(), version.clone());
            }

            convert_resources_to_format(resources, &output, FormatType::Xcstrings)
                .map_err(|e| format!("Error converting to xcstrings: {}", e))
        }) {
            Ok(()) => {
                println!(
                    "{}",
                    ui::status_line_stdout(
                        ui::Tone::Success,
                        "Successfully converted to xcstrings",
                    )
                );
                return;
            }
            Err(e) => {
                println!(
                    "{}",
                    ui::status_line_stdout(ui::Tone::Error, "Conversion to xcstrings failed")
                );
                // Preserve legacy expectation for invalid JSON: surface an inference hint
                if input.ends_with(".json") {
                    eprintln!("Cannot infer input format");
                }
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }

    // If the desired output is .langcodec, handle via resource serialization
    if options.output_format.is_none() && output.ends_with(".langcodec") {
        let filter_msg = if !options.include_lang.is_empty() || !options.exclude_lang.is_empty() {
            let mut parts = Vec::new();
            if !options.include_lang.is_empty() {
                parts.push(format!("including: {}", options.include_lang.join(", ")));
            }
            if !options.exclude_lang.is_empty() {
                parts.push(format!("excluding: {}", options.exclude_lang.join(", ")));
            }
            format!(" with language filtering ({})", parts.join(", "))
        } else {
            String::new()
        };

        println!(
            "{}",
            ui::status_line_stdout(
                ui::Tone::Info,
                &format!(
                    "Converting input to .langcodec (Resource JSON array){}...",
                    filter_msg
                ),
            )
        );
        match read_resources_from_any_input(
            &input,
            options.input_format.as_ref(),
            options.source_language.as_ref(),
            strict,
        )
        .and_then(|resources| {
            // Apply language filtering
            let filtered_resources = resources
                .into_iter()
                .filter(|resource| {
                    let lang = &resource.metadata.language;

                    // If include_lang is specified, only include those languages
                    if !options.include_lang.is_empty() && !options.include_lang.contains(lang) {
                        return false;
                    }

                    // If exclude_lang is specified, exclude those languages
                    if !options.exclude_lang.is_empty() && options.exclude_lang.contains(lang) {
                        return false;
                    }

                    true
                })
                .collect();

            write_resources_as_langcodec(&filtered_resources, &output)
        }) {
            Ok(()) => {
                let filter_msg =
                    if !options.include_lang.is_empty() || !options.exclude_lang.is_empty() {
                        let mut parts = Vec::new();
                        if !options.include_lang.is_empty() {
                            parts.push(format!("including: {}", options.include_lang.join(", ")));
                        }
                        if !options.exclude_lang.is_empty() {
                            parts.push(format!("excluding: {}", options.exclude_lang.join(", ")));
                        }
                        format!(" with language filtering ({})", parts.join(", "))
                    } else {
                        String::new()
                    };

                println!(
                    "{}",
                    ui::status_line_stdout(
                        ui::Tone::Success,
                        &format!(
                            "Successfully converted to .langcodec (Resource JSON array){}",
                            filter_msg
                        ),
                    )
                );
                return;
            }
            Err(e) => {
                let filter_msg =
                    if !options.include_lang.is_empty() || !options.exclude_lang.is_empty() {
                        let mut parts = Vec::new();
                        if !options.include_lang.is_empty() {
                            parts.push(format!("including: {}", options.include_lang.join(", ")));
                        }
                        if !options.exclude_lang.is_empty() {
                            parts.push(format!("excluding: {}", options.exclude_lang.join(", ")));
                        }
                        format!(" with language filtering ({})", parts.join(", "))
                    } else {
                        String::new()
                    };

                println!(
                    "{}",
                    ui::status_line_stdout(
                        ui::Tone::Error,
                        &format!("Conversion to .langcodec failed{}", filter_msg),
                    )
                );
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }

    let has_explicit_format = options.input_format.is_some() || options.output_format.is_some();
    if has_explicit_format {
        println!(
            "{}",
            ui::status_line_stdout(
                ui::Tone::Info,
                "Converting with authoritative format selection...",
            )
        );
        match read_resources_from_any_input(
            &input,
            options.input_format.as_ref(),
            options.source_language.as_ref(),
            strict,
        )
        .and_then(|resources| {
            let output_format = resolve_convert_output_format(
                &output,
                options.output_format.as_ref(),
                options.output_lang.as_ref(),
            )?;
            convert_resources_to_format(resources, &output, output_format)
                .map_err(|error| format!("Error converting to output format: {error}"))
        }) {
            Ok(()) => {
                println!(
                    "{}",
                    ui::status_line_stdout(
                        ui::Tone::Success,
                        "Successfully converted with authoritative format selection",
                    )
                );
                return;
            }
            Err(error) => {
                println!(
                    "{}",
                    ui::status_line_stdout(ui::Tone::Error, "Conversion failed")
                );
                eprintln!("Error: {error}");
                std::process::exit(1);
            }
        }
    }

    let hinted_single_language_input = options.source_language.is_some()
        && resolved_standard_input.is_some_and(is_single_language_format);
    if hinted_single_language_input {
        println!(
            "{}",
            ui::status_line_stdout(
                ui::Tone::Info,
                "Converting using the resolved standard input format...",
            )
        );
        match read_resources_from_any_input(
            &input,
            options.input_format.as_ref(),
            options.source_language.as_ref(),
            strict,
        )
        .and_then(|resources| {
            let output_format = resolve_convert_output_format(
                &output,
                options.output_format.as_ref(),
                options.output_lang.as_ref(),
            )?;
            convert_resources_to_format(resources, &output, output_format)
                .map_err(|error| format!("Error converting to output format: {error}"))
        }) {
            Ok(()) => {
                println!(
                    "{}",
                    ui::status_line_stdout(
                        ui::Tone::Success,
                        "Successfully converted using the resolved standard input format",
                    )
                );
                return;
            }
            Err(error) => {
                println!(
                    "{}",
                    ui::status_line_stdout(ui::Tone::Error, "Conversion failed")
                );
                eprintln!("Error: {error}");
                std::process::exit(1);
            }
        }
    }

    if strict {
        if let (Some(input_fmt), Some(output_fmt)) = (
            options.input_format.as_deref(),
            options.output_format.as_deref(),
        ) {
            println!(
                "{}",
                ui::status_line_stdout(
                    ui::Tone::Info,
                    "Strict mode: converting with explicit format hints only...",
                )
            );
            if let Err(e) = try_explicit_format_conversion(
                &input,
                &output,
                input_fmt,
                output_fmt,
                options.output_lang.as_ref(),
            ) {
                println!(
                    "{}",
                    ui::status_line_stdout(ui::Tone::Error, "Strict conversion failed")
                );
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            println!(
                "{}",
                ui::status_line_stdout(ui::Tone::Success, "Successfully converted in strict mode",)
            );
            return;
        }

        if input.ends_with(".json")
            || input.ends_with(".yaml")
            || input.ends_with(".yml")
            || input.ends_with(".langcodec")
        {
            println!(
                "{}",
                ui::status_line_stdout(
                    ui::Tone::Info,
                    "Strict mode: converting custom format without fallback...",
                )
            );
            if let Err(e) = try_custom_format_conversion(
                &input,
                &output,
                &options.input_format,
                options.output_format.as_ref(),
                options.output_lang.as_ref(),
            ) {
                println!(
                    "{}",
                    ui::status_line_stdout(ui::Tone::Error, "Strict conversion failed")
                );
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            println!(
                "{}",
                ui::status_line_stdout(ui::Tone::Success, "Successfully converted in strict mode",)
            );
            return;
        }

        println!(
            "{}",
            ui::status_line_stdout(
                ui::Tone::Info,
                "Strict mode: converting using extension-based standard formats only...",
            )
        );
        if let Err(e) = convert_auto(&input, &output) {
            println!(
                "{}",
                ui::status_line_stdout(ui::Tone::Error, "Strict conversion failed")
            );
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
        println!(
            "{}",
            ui::status_line_stdout(ui::Tone::Success, "Successfully converted in strict mode",)
        );
        return;
    }

    // Recognized standard extensions are authoritative. A parse or
    // representability failure is a real conversion error, not evidence that
    // the input should be reinterpreted as an unrelated custom format.
    if inferred_standard_input.is_some() && langcodec::infer_format_from_path(&output).is_some() {
        println!(
            "{}",
            ui::status_line_stdout(
                ui::Tone::Info,
                "Converting using standard format detection from file extensions...",
            )
        );
        match convert_auto(&input, &output) {
            Ok(()) => {
                println!(
                    "{}",
                    ui::status_line_stdout(
                        ui::Tone::Success,
                        "Successfully converted using standard format detection",
                    )
                );
                return;
            }
            Err(error) => {
                println!(
                    "{}",
                    ui::status_line_stdout(ui::Tone::Error, "Standard format conversion failed")
                );
                eprintln!("Error: {error}");
                std::process::exit(1);
            }
        }
    }

    // Try custom formats for JSON/YAML/langcodec files.
    if input.ends_with(".json")
        || input.ends_with(".yaml")
        || input.ends_with(".yml")
        || input.ends_with(".langcodec")
    {
        // For JSON files without explicit format, try standard format detection first
        if input.ends_with(".json") && options.input_format.is_none() {
            println!(
                "{}",
                ui::status_line_stdout(ui::Tone::Info, "Trying standard JSON format detection...",)
            );
            // Try to use the standard format detection which will show proper JSON parsing errors
            if convert_auto(&input, &output).is_err() {
                println!(
                    "{}",
                    ui::status_line_stdout(
                        ui::Tone::Info,
                        "Trying custom JSON format conversion...",
                    )
                );
                // If standard detection fails, try custom formats
                match try_custom_format_conversion(
                    &input,
                    &output,
                    &options.input_format,
                    options.output_format.as_ref(),
                    options.output_lang.as_ref(),
                ) {
                    Ok(()) => {
                        println!(
                            "{}",
                            ui::status_line_stdout(
                                ui::Tone::Success,
                                "Successfully converted using custom JSON format",
                            )
                        );
                        return;
                    }
                    Err(custom_error) => {
                        // If both fail, show the custom conversion error because it is usually
                        // more actionable than the initial extension-based failure.
                        println!(
                            "{}",
                            ui::status_line_stdout(ui::Tone::Error, "Conversion failed")
                        );
                        eprintln!("Error: {}", custom_error);
                        std::process::exit(1);
                    }
                }
            }
        } else {
            // For YAML and langcodec files, try custom formats directly
            println!(
                "{}",
                ui::status_line_stdout(ui::Tone::Info, "Converting using custom format...")
            );
            if let Err(e) = try_custom_format_conversion(
                &input,
                &output,
                &options.input_format,
                options.output_format.as_ref(),
                options.output_lang.as_ref(),
            ) {
                println!(
                    "{}",
                    ui::status_line_stdout(ui::Tone::Error, "Custom format conversion failed",)
                );
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            println!(
                "{}",
                ui::status_line_stdout(
                    ui::Tone::Success,
                    "Successfully converted using custom format",
                )
            );
            return;
        }
    }

    // Strategy 3: If we have format hints, try with explicit formats
    if let (Some(input_fmt), Some(output_fmt)) =
        (options.input_format.clone(), options.output_format.clone())
    {
        println!(
            "{}",
            ui::status_line_stdout(ui::Tone::Info, "Converting with explicit format hints...")
        );
        if let Err(e) = try_explicit_format_conversion(
            &input,
            &output,
            &input_fmt,
            &output_fmt,
            options.output_lang.as_ref(),
        ) {
            println!(
                "{}",
                ui::status_line_stdout(ui::Tone::Error, "Explicit format conversion failed",)
            );
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
        println!(
            "{}",
            ui::status_line_stdout(
                ui::Tone::Success,
                "Successfully converted with explicit formats",
            )
        );
        return;
    }

    // If all strategies failed, provide helpful error message
    println!(
        "{}",
        ui::status_line_stdout(ui::Tone::Error, "All conversion strategies failed")
    );
    print_conversion_error(&input, &output);
    std::process::exit(1);
}

fn try_custom_format_conversion(
    input: &str,
    output: &str,
    input_format: &Option<String>,
    output_format: Option<&String>,
    output_lang: Option<&String>,
) -> Result<(), String> {
    // Validate custom format file
    validate_custom_format_file(input)?;

    let custom_format = if let Some(format_str) = input_format {
        parse_custom_format(format_str)?
    } else {
        // Auto-detect format based on file content
        let file_content = std::fs::read_to_string(input)
            .map_err(|e| format!("Error reading file {}: {}", input, e))?;

        // Validate file content
        formats::validate_custom_format_content(input, &file_content)?
    };

    // Convert custom format to Resource
    let resources = custom_format_to_resource(input.to_string(), custom_format)?;

    // If output is .langcodec, serialize resources as JSON array
    if output_format.is_none() && output.ends_with(".langcodec") {
        write_resources_as_langcodec(&resources, output)?;
        return Ok(());
    }

    // Get output format type
    let output_format_type = resolve_convert_output_format(output, output_format, output_lang)?;

    // Convert to target format
    convert_resources_to_format(resources, output, output_format_type)
        .map_err(|e| format!("Error converting to output format: {}", e))?;

    Ok(())
}

fn print_conversion_error(input: &str, output: &str) {
    eprintln!("Error: Could not convert '{}' to '{}'", input, output);
    eprintln!();
    eprintln!("Tried the following strategies:");
    eprintln!("1. Standard format detection from file extensions");
    if input.ends_with(".json") {
        eprintln!("2. Custom JSON format conversion");
    }
    if input.ends_with(".yaml") || input.ends_with(".yml") {
        eprintln!("2. Custom YAML format conversion");
    }
    if input.ends_with(".langcodec") {
        eprintln!("2. Custom langcodec Resource array format conversion");
    }
    eprintln!();
    eprintln!("Supported input formats:");
    eprintln!("- .strings (Apple strings files)");
    eprintln!("- .stringsdict (Apple plural strings files)");
    eprintln!("- .xml (Android strings files)");
    eprintln!("- .xcstrings (Apple xcstrings files)");
    eprintln!("- .xliff (Apple/Xcode XLIFF 1.2 files)");
    eprintln!("- .csv (CSV files)");
    eprintln!("- .tsv (TSV files)");
    eprintln!("- .langcodec (Resource JSON array)");
    eprintln!("- .json (JSON key-value pairs or Resource format)");
    eprintln!("- .yaml/.yml (YAML language map format)");
    eprintln!();
    eprintln!("Supported output formats:");
    eprintln!("- .strings (Apple strings files)");
    eprintln!("- .stringsdict (Apple plural strings files)");
    eprintln!("- .xml (Android strings files)");
    eprintln!("- .xcstrings (Apple xcstrings files)");
    eprintln!("- .xliff (Apple/Xcode XLIFF 1.2 files)");
    eprintln!("- .csv (CSV files)");
    eprintln!("- .tsv (TSV files)");
    eprintln!("- .langcodec (Resource JSON array)");
    eprintln!();
    eprintln!(
        "For JSON files, the command will try both standard Resource format and key-value pairs."
    );
    eprintln!("For YAML files, the command will try YAML language map format.");
    eprintln!(
        "Custom formats: {}",
        formats::get_supported_custom_formats()
    );
}

/// Convert a Vec<Resource> to a specific output format using the lib crate
fn convert_resources_to_format(
    resources: Vec<langcodec::Resource>,
    output: &str,
    output_format: FormatType,
) -> Result<(), langcodec::Error> {
    langcodec::converter::convert_resources_to_format(resources, output, output_format)
}

/// Try explicit format conversion with specified input and output formats
fn try_explicit_format_conversion(
    input: &str,
    output: &str,
    input_format: &str,
    output_format: &str,
    output_lang: Option<&String>,
) -> Result<(), String> {
    // Validate input file exists
    validation::validate_file_path(input)?;

    // Validate output path
    validation::validate_output_path(output)?;

    // Parse input format
    let input_format_type = match input_format.to_lowercase().as_str() {
        "strings" => langcodec::formats::FormatType::Strings(None),
        "stringsdict" => langcodec::formats::FormatType::Stringsdict(None),
        "android" | "androidstrings" => langcodec::formats::FormatType::AndroidStrings(None),
        "xcstrings" => langcodec::formats::FormatType::Xcstrings,
        "xliff" => langcodec::formats::FormatType::Xliff(None),
        "csv" => langcodec::formats::FormatType::CSV,
        "tsv" => langcodec::formats::FormatType::TSV,
        _ => {
            return Err(format!(
                "Unsupported input format: '{}'. Supported formats: strings, stringsdict, android, xcstrings, xliff, csv, tsv",
                input_format
            ));
        }
    };

    // Handle .langcodec output specially by reading resources then serializing
    if output_format.trim().eq_ignore_ascii_case("langcodec") {
        // Read resources using explicit input format
        let mut codec = Codec::new();
        codec
            .read_file_by_type(input, input_format_type)
            .map_err(|e| format!("Failed to read input with explicit format: {}", e))?;
        write_resources_as_langcodec(&codec.resources, output)
    } else {
        // Parse output format
        let output_format_type =
            resolve_convert_output_format(output, Some(&output_format.to_string()), output_lang)?;

        // Use the lib crate's convert function
        langcodec::convert(input, input_format_type, output, output_format_type)
            .map_err(|e| format!("Conversion error: {}", e))
    }
}

/// Try to read a custom format file and add it to the codec for view
pub fn try_custom_format_view(
    input: &str,
    _lang: Option<String>,
    codec: &mut Codec,
) -> Result<(), String> {
    // Validate custom format file
    validation::validate_custom_format_file(input)?;

    // Auto-detect format based on file content
    let file_content = std::fs::read_to_string(input)
        .map_err(|e| format!("Error reading file {}: {}", input, e))?;

    // Validate file content
    let custom_format = formats::validate_custom_format_content(input, &file_content)?;

    // Convert custom format to Resource
    let resources = custom_format_to_resource(input.to_string(), custom_format)?;

    // Add resources to codec
    for resource in resources {
        codec.add_resource(resource);
    }

    Ok(())
}

/// Serialize resources as a .langcodec (Resource JSON array) file
fn write_resources_as_langcodec(
    resources: &Vec<langcodec::Resource>,
    output: &str,
) -> Result<(), String> {
    let file = File::create(output).map_err(|e| format!("Failed to create {}: {}", output, e))?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, resources)
        .map_err(|e| format!("Failed to write .langcodec JSON: {}", e))
}

/// Read resources from any supported input (standard or custom formats)
pub fn read_resources_from_any_input(
    input: &str,
    input_format_hint: Option<&String>,
    input_language_hint: Option<&String>,
    strict: bool,
) -> Result<Vec<langcodec::Resource>, String> {
    if strict {
        if let Some(fmt) = input_format_hint {
            if let Some(format) = parse_standard_input_format(fmt) {
                return read_standard_resources(input, format, input_language_hint, true, true);
            }

            let custom_format = parse_custom_format(fmt)?;
            let resources = custom_format_to_resource(input.to_string(), custom_format)?;
            return Ok(resources);
        }

        if input.ends_with(".strings")
            || input.ends_with(".stringsdict")
            || input.ends_with(".xml")
            || input.ends_with(".xcstrings")
            || input.ends_with(".xliff")
            || input.ends_with(".csv")
            || input.ends_with(".tsv")
        {
            let format = langcodec::infer_format_from_path(input)
                .expect("recognized standard extension must infer a format");
            return read_standard_resources(input, format, input_language_hint, true, false);
        }

        if input.ends_with(".json")
            || input.ends_with(".yaml")
            || input.ends_with(".yml")
            || input.ends_with(".langcodec")
        {
            validate_custom_format_file(input)?;
            let file_content = std::fs::read_to_string(input)
                .map_err(|e| format!("Error reading file {}: {}", input, e))?;
            let custom_format = formats::validate_custom_format_content(input, &file_content)?;
            let resources = custom_format_to_resource(input.to_string(), custom_format)?;
            return Ok(resources);
        }

        return Err(format!(
            "Unsupported input format or file extension: '{}'. Supported formats: .strings, .stringsdict, .xml, .xcstrings, .xliff, .csv, .tsv, .json, .yaml, .yml, .langcodec",
            input
        ));
    }

    // First: an explicit input format is authoritative, regardless of the path extension.
    if let Some(fmt) = input_format_hint {
        if let Some(format) = parse_standard_input_format(fmt) {
            return read_standard_resources(input, format, input_language_hint, false, true);
        }

        let custom_format = parse_custom_format(fmt)?;
        return custom_format_to_resource(input.to_string(), custom_format);
    }

    // Second: read a standard file using its extension and the explicit language hint.
    if let Some(format) = langcodec::infer_format_from_path(input) {
        return read_standard_resources(input, format, input_language_hint, false, false);
    }

    // Third: try custom formats (json, yaml, langcodec)
    if input.ends_with(".json")
        || input.ends_with(".yaml")
        || input.ends_with(".yml")
        || input.ends_with(".langcodec")
    {
        // Validate custom format file
        validate_custom_format_file(input)?;

        // Auto-detect format based on file content
        let file_content = std::fs::read_to_string(input)
            .map_err(|e| format!("Error reading file {}: {}", input, e))?;

        // Validate file content
        let custom_format = formats::validate_custom_format_content(input, &file_content)?;

        // Convert custom format to Resource
        let resources = custom_format_to_resource(input.to_string(), custom_format)?;
        return Ok(resources);
    }

    Err(format!(
        "Unsupported input format or file extension: '{}'. Supported formats: .strings, .stringsdict, .xml, .xcstrings, .xliff, .csv, .tsv, .json, .yaml, .yml, .langcodec",
        input
    ))
}
