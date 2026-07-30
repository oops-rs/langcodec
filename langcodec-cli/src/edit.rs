use crate::path_glob;
use crate::ui;
use crate::validation::{ValidationContext, validate_context};
use langcodec::{Codec, Resource, Translation, formats::FormatType};
use std::path::Path;

fn parse_status_opt(
    status: &Option<String>,
) -> Result<Option<langcodec::types::EntryStatus>, String> {
    if let Some(s) = status {
        let normalized = s.replace(['-', ' '], "_");
        normalized
            .parse::<langcodec::types::EntryStatus>()
            .map(Some)
            .map_err(|e| e.to_string())
    } else {
        Ok(None)
    }
}

fn infer_output_format_from_path(path: &str) -> Result<FormatType, String> {
    langcodec::infer_format_from_extension(path)
        .ok_or_else(|| format!("Cannot infer format from path: {}", path))
}

fn reject_unsafe_edit_paths(input_path: &str, output_path: Option<&String>) -> Result<(), String> {
    if input_path.ends_with(".xliff") {
        return Err(
            ".xliff is not supported by `edit` in v1. Use `convert`, `view`, or `debug` instead."
                .to_string(),
        );
    }
    if output_path.is_some_and(|path| path.ends_with(".xliff")) {
        return Err(
            ".xliff is not supported as an `edit` output in v1. Use `convert` instead.".to_string(),
        );
    }
    if input_path.ends_with(".stringsdict")
        || output_path.is_some_and(|path| path.ends_with(".stringsdict"))
    {
        return Err(
            ".stringsdict is not supported by `edit set`: singular updates and removals are not representable in this plural-only format."
                .to_string(),
        );
    }
    Ok(())
}

fn pick_single_resource<'a>(
    codec: &'a Codec,
    lang: &Option<String>,
) -> Result<&'a Resource, String> {
    if let Some(l) = lang {
        codec
            .get_by_language(l)
            .ok_or_else(|| format!("Language '{}' not found in input", l))
    } else if codec.resources.len() == 1 {
        Ok(&codec.resources[0])
    } else {
        Err("Multiple languages present; specify --lang".to_string())
    }
}

fn pick_single_resource_mut<'a>(
    codec: &'a mut Codec,
    lang: &Option<String>,
) -> Result<&'a mut Resource, String> {
    if let Some(l) = lang {
        codec
            .get_mut_by_language(l)
            .ok_or_else(|| format!("Language '{}' not found in input", l))
    } else if codec.resources.len() == 1 {
        Ok(&mut codec.resources[0])
    } else {
        Err("Multiple languages present; specify --lang".to_string())
    }
}

fn write_back(
    codec: &Codec,
    input_path: &str,
    output_path: &Option<String>,
    lang: &Option<String>,
) -> Result<(), String> {
    let input_owned = input_path.to_string();
    let out = output_path.as_ref().unwrap_or(&input_owned);
    let fmt = infer_output_format_from_path(out)?;

    match fmt {
        FormatType::Strings(_) | FormatType::AndroidStrings(_) => {
            // Single-language per file formats: write only one resource
            let res = pick_single_resource(codec, lang)?;
            langcodec::Codec::write_resource_to_file(res, out)
                .map_err(|e| format!("Error writing output: {}", e))
        }
        FormatType::Xcstrings | FormatType::CSV | FormatType::TSV => {
            // Multi-language formats: write all resources
            let resources = codec.resources.clone();
            langcodec::converter::convert_resources_to_format(resources, out, fmt)
                .map_err(|e| format!("Error writing output: {}", e))
        }
        FormatType::Xliff(_) => Err(
            ".xliff is not supported as an `edit` output in v1. Use `convert` instead.".to_string(),
        ),
        FormatType::Stringsdict(_) => Err(
            ".stringsdict is not supported by `edit set`: singular updates and removals are not representable in this plural-only format."
                .to_string(),
        ),
    }
}

#[derive(Debug, Clone)]
pub struct EditSetOptions {
    pub inputs: Vec<String>,
    pub lang: Option<String>,
    pub key: String,
    pub value: Option<String>,
    pub comment: Option<String>,
    pub status: Option<String>,
    pub output: Option<String>,
    pub dry_run: bool,
    pub continue_on_error: bool,
}

pub fn run_edit_set_command(opts: EditSetOptions) -> Result<(), String> {
    // Expand globs for inputs
    let expanded = path_glob::expand_input_globs(&opts.inputs)
        .map_err(|e| format!("Failed to expand input patterns: {}", e))?;
    if expanded.is_empty() {
        return Err("No input files matched the provided patterns".to_string());
    }

    if expanded.len() > 1 && opts.output.is_some() {
        return Err("--output cannot be used with multiple input files".to_string());
    }

    // Report missing literal inputs (patterns without glob meta that don't exist)
    fn has_glob_meta(s: &str) -> bool {
        s.bytes().any(|b| matches!(b, b'*' | b'?' | b'[' | b'{'))
    }
    use std::collections::HashSet;
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut skip_missing: HashSet<String> = HashSet::new();
    for original in &opts.inputs {
        if !has_glob_meta(original) && !Path::new(original).is_file() {
            let msg = format!("Input file does not exist: {}", original);
            if opts.continue_on_error {
                eprintln!("{}", ui::status_line_stderr(ui::Tone::Error, &msg));
                failures.push((original.clone(), msg));
                skip_missing.insert(original.clone());
            } else {
                return Err(msg);
            }
        }
    }

    let mut processed_count: usize = 0;
    let mut success_count: usize = 0;

    for input_path in expanded {
        if skip_missing.contains(&input_path) {
            continue;
        }
        processed_count += 1;
        // Validate per-file
        let mut validation_context = ValidationContext::new().with_input_file(input_path.clone());
        if let Some(l) = &opts.lang {
            validation_context = validation_context.with_language_code(l.clone());
        }
        if let Some(o) = &opts.output {
            validation_context = validation_context.with_output_file(o.clone());
        }
        if let Err(e) = validate_context(&validation_context) {
            let msg = format!("Input validation failed for '{}': {}", input_path, e);
            if opts.continue_on_error {
                eprintln!("{}", ui::status_line_stderr(ui::Tone::Error, &msg));
                failures.push((input_path.clone(), msg));
                continue;
            } else {
                println!(
                    "Summary: processed {}; success: {}; failed: {}",
                    processed_count,
                    success_count,
                    failures.len() + 1
                );
                return Err(msg);
            }
        }

        if let Err(e) = apply_set_to_file(
            &input_path,
            &opts.lang,
            &opts.key,
            &opts.value,
            &opts.comment,
            &opts.status,
            opts.output.as_ref(),
            opts.dry_run,
        ) {
            let msg = e.to_string();
            if opts.continue_on_error {
                eprintln!("{}", ui::status_line_stderr(ui::Tone::Error, &msg));
                failures.push((input_path.clone(), msg));
                continue;
            } else {
                println!(
                    "Summary: processed {}; success: {}; failed: {}",
                    processed_count,
                    success_count,
                    failures.len() + 1
                );
                return Err(msg);
            }
        } else {
            success_count += 1;
        }
    }

    println!(
        "{}",
        ui::status_line_stdout(
            if failures.is_empty() {
                ui::Tone::Success
            } else {
                ui::Tone::Warning
            },
            &format!(
                "Summary: processed {}; success: {}; failed: {}",
                processed_count,
                success_count,
                failures.len()
            ),
        )
    );

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} file(s) failed. See errors above.",
            failures.len()
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_set_to_file(
    input: &str,
    lang: &Option<String>,
    key: &str,
    value: &Option<String>,
    comment: &Option<String>,
    status: &Option<String>,
    output: Option<&String>,
    dry_run: bool,
) -> Result<(), String> {
    reject_unsafe_edit_paths(input, output)?;

    let mut codec = Codec::new();
    if let Err(e) = codec.read_file_by_extension(input, lang.clone()) {
        let ext = Path::new(input)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if matches!(ext, "json" | "yaml" | "yml" | "langcodec") {
            return Err(
                "Edit currently supports standard formats (.strings, .xml, .xcstrings, .csv, .tsv)"
                    .to_string(),
            );
        }
        return Err(format!("Failed to read input '{}': {}", input, e));
    }

    let is_remove = value.as_deref().map(|s| s.is_empty()).unwrap_or(true);
    let status_parsed = parse_status_opt(status)?;

    if is_remove {
        if let Some(l) = lang.as_deref() {
            if codec.has_entry(key, l) {
                if dry_run {
                    println!(
                        "{}",
                        ui::status_line_stdout(
                            ui::Tone::Warning,
                            &format!("DRY-RUN: Would remove '{}' from {} ({})", key, l, input),
                        )
                    );
                } else {
                    codec.remove_entry(key, l).map_err(|e| e.to_string())?;
                    println!(
                        "{}",
                        ui::status_line_stdout(
                            ui::Tone::Success,
                            &format!("Removed '{}' from {} ({})", key, l, input),
                        )
                    );
                }
            } else {
                println!(
                    "{}",
                    ui::status_line_stdout(
                        ui::Tone::Info,
                        &format!(
                            "Key '{}' not found in {} ({}); nothing to remove",
                            key, l, input
                        ),
                    )
                );
            }
        } else {
            let res = pick_single_resource_mut(&mut codec, lang)?;
            let before = res.entries.len();
            let will_remove = res.entries.iter().any(|e| e.id == key);
            if dry_run {
                if will_remove {
                    println!(
                        "{}",
                        ui::status_line_stdout(
                            ui::Tone::Warning,
                            &format!("DRY-RUN: Would remove '{}' ({})", key, input),
                        )
                    );
                } else {
                    println!(
                        "{}",
                        ui::status_line_stdout(
                            ui::Tone::Info,
                            &format!("Key '{}' not present ({}); nothing to remove", key, input),
                        )
                    );
                }
            } else {
                res.entries.retain(|e| e.id != key);
                if res.entries.len() < before {
                    println!(
                        "{}",
                        ui::status_line_stdout(
                            ui::Tone::Success,
                            &format!("Removed '{}' ({})", key, input),
                        )
                    );
                } else {
                    println!(
                        "{}",
                        ui::status_line_stdout(
                            ui::Tone::Info,
                            &format!("Key '{}' not present ({}); nothing to remove", key, input),
                        )
                    );
                }
            }
        }
    } else {
        let resolved_lang_owned: String;
        let lang_ref: &str = if let Some(l) = lang.as_deref() {
            l
        } else if codec.resources.len() == 1 {
            resolved_lang_owned = codec.resources[0].metadata.language.clone();
            resolved_lang_owned.as_str()
        } else {
            return Err(format!(
                "--lang is required for set on multi-language files ({})",
                input
            ));
        };
        let val = value.clone().unwrap_or_default();
        let exists = codec.has_entry(key, lang_ref);
        if exists {
            let old = codec
                .find_entry(key, lang_ref)
                .map(|e| match &e.value {
                    Translation::Empty => String::new(),
                    Translation::Singular(s) => s.clone(),
                    Translation::Plural(p) => p.id.clone(),
                })
                .unwrap_or_default();
            if dry_run {
                println!(
                    "{}",
                    ui::status_line_stdout(
                        ui::Tone::Warning,
                        &format!(
                            "DRY-RUN: Would update '{}' in {}: '{}' -> '{}' ({})",
                            key, lang_ref, old, val, input
                        ),
                    )
                );
            } else {
                codec
                    .update_translation(
                        key,
                        lang_ref,
                        Translation::Singular(val.clone()),
                        status_parsed.clone(),
                    )
                    .map_err(|e| e.to_string())?;
                if comment.is_some()
                    && let Some(entry) = codec.find_entry_mut(key, lang_ref)
                {
                    entry.comment = comment.clone();
                }
                println!(
                    "{}",
                    ui::status_line_stdout(
                        ui::Tone::Success,
                        &format!("Updated '{}' in {} ({})", key, lang_ref, input),
                    )
                );
            }
        } else if dry_run {
            println!(
                "{}",
                ui::status_line_stdout(
                    ui::Tone::Warning,
                    &format!(
                        "DRY-RUN: Would add '{}' to {} with value '{}' ({})",
                        key, lang_ref, val, input
                    ),
                )
            );
        } else {
            codec
                .add_entry(
                    key,
                    lang_ref,
                    Translation::Singular(val.clone()),
                    comment.clone(),
                    status_parsed.clone(),
                )
                .map_err(|e| e.to_string())?;
            println!(
                "{}",
                ui::status_line_stdout(
                    ui::Tone::Success,
                    &format!("Added '{}' to {} ({})", key, lang_ref, input),
                )
            );
        }
    }

    if !dry_run {
        write_back(&codec, input, &output.cloned(), lang)?;
        if let Some(out) = output {
            println!(
                "{}",
                ui::status_line_stdout(ui::Tone::Accent, &format!("Wrote changes to {}", out))
            );
        } else {
            println!(
                "{}",
                ui::status_line_stdout(ui::Tone::Accent, &format!("Updated {} in place", input),)
            );
        }
    }

    Ok(())
}
