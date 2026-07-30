use crate::path_glob;
use crate::validation::{validate_file_path, validate_output_path};
use langcodec::{
    Codec, FormatType, KeyStyle, NormalizeOptions as EngineNormalizeOptions, ReadOptions,
    normalize_codec,
};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct NormalizeCliOptions {
    pub inputs: Vec<String>,
    pub output: Option<String>,
    pub dry_run: bool,
    pub check: bool,
    pub no_placeholders: bool,
    pub key_style: String,
    pub continue_on_error: bool,
    pub strict: bool,
}

fn parse_key_style(input: &str) -> Result<KeyStyle, String> {
    match input.trim().to_ascii_lowercase().as_str() {
        "none" => Ok(KeyStyle::None),
        "snake" => Ok(KeyStyle::Snake),
        "kebab" => Ok(KeyStyle::Kebab),
        "camel" => Ok(KeyStyle::Camel),
        other => Err(format!(
            "Invalid --key-style '{}'. Expected one of: none, snake, kebab, camel",
            other
        )),
    }
}

fn infer_output_format_from_path(path: &str) -> Result<FormatType, String> {
    langcodec::infer_format_from_extension(path)
        .ok_or_else(|| format!("Cannot infer format from path: {}", path))
}

fn reject_xliff_normalize_paths(input: &str, output: Option<&String>) -> Result<(), String> {
    if input.ends_with(".stringsdict") || output.is_some_and(|path| path.ends_with(".stringsdict"))
    {
        return Err(
            ".stringsdict is not supported by `normalize`: placeholder or key rewriting can invalidate plural selector metadata. Use `convert`, `view`, or `check` instead."
                .to_string(),
        );
    }
    if input.ends_with(".xliff") || output.is_some_and(|path| path.ends_with(".xliff")) {
        return Err(
            ".xliff is not supported by `normalize` in v1. Use `convert`, `view`, or `debug` instead."
                .to_string(),
        );
    }
    Ok(())
}

fn pick_single_resource(codec: &Codec) -> Result<&langcodec::Resource, String> {
    if codec.resources.len() == 1 {
        Ok(&codec.resources[0])
    } else {
        Err(
            "Multiple languages present; single-language output requires exactly one resource"
                .to_string(),
        )
    }
}

fn write_back(codec: &Codec, input_path: &str, output_path: &Option<String>) -> Result<(), String> {
    let input_owned = input_path.to_string();
    let out = output_path.as_ref().unwrap_or(&input_owned);
    let fmt = infer_output_format_from_path(out)?;

    match fmt {
        FormatType::Strings(_) | FormatType::AndroidStrings(_) => {
            let resource = pick_single_resource(codec)?;
            Codec::write_resource_to_file(resource, out)
                .map_err(|e| format!("Error writing output: {}", e))
        }
        FormatType::Stringsdict(_) => Err(
            ".stringsdict is not supported by `normalize`. Use `convert`, `view`, or `check` instead."
                .to_string(),
        ),
        FormatType::Xcstrings | FormatType::CSV | FormatType::TSV => {
            langcodec::converter::convert_resources_to_format(codec.resources.clone(), out, fmt)
                .map_err(|e| format!("Error writing output: {}", e))
        }
        FormatType::Xliff(_) => Err(
            ".xliff is not supported by `normalize` in v1. Use `convert`, `view`, or `debug` instead."
                .to_string(),
        ),
    }
}

fn has_distinct_output_path(input_path: &str, output_path: &Option<String>) -> bool {
    output_path
        .as_ref()
        .is_some_and(|output| Path::new(output) != Path::new(input_path))
}

fn has_glob_meta(input: &str) -> bool {
    input
        .bytes()
        .any(|b| matches!(b, b'*' | b'?' | b'[' | b'{'))
}

fn run_normalize_for_file(
    input: &str,
    output: &Option<String>,
    dry_run: bool,
    check: bool,
    no_placeholders: bool,
    key_style: &KeyStyle,
    strict: bool,
) -> Result<bool, String> {
    reject_xliff_normalize_paths(input, output.as_ref())?;

    validate_file_path(input)?;

    let mut codec = Codec::new();
    codec
        .read_file_by_extension_with_options(input, &ReadOptions::new().with_strict(strict))
        .map_err(|e| format!("Failed to read input '{}': {}", input, e))?;

    let report = normalize_codec(
        &mut codec,
        &EngineNormalizeOptions {
            normalize_placeholders: !no_placeholders,
            key_style: *key_style,
        },
    )
    .map_err(|e| e.to_string())?;

    if check {
        if report.changed {
            println!("would change: {}", input);
            return Err(format!("would change: {}", input));
        }

        println!("No changes needed: {}", input);
        return Ok(false);
    }

    if dry_run {
        if report.changed {
            println!("DRY-RUN: would change {}", input);
            return Ok(true);
        } else {
            println!("No changes needed: {}", input);
        }
        return Ok(false);
    }

    if !report.changed {
        if has_distinct_output_path(input, output) {
            if let Some(output) = output {
                validate_output_path(output)?;
            }
            write_back(&codec, input, output)?;
            println!("No changes needed: {}", input);
            println!("✅ Wrote output: {}", output.as_deref().unwrap_or(input));
            return Ok(false);
        }

        println!("No changes needed: {}", input);
        return Ok(false);
    }

    if let Some(output) = output {
        validate_output_path(output)?;
    }

    write_back(&codec, input, output)?;
    println!("✅ Normalized: {}", output.as_deref().unwrap_or(input));
    Ok(true)
}

pub fn run_normalize_command(opts: NormalizeCliOptions) -> Result<(), String> {
    let expanded = path_glob::expand_input_globs(&opts.inputs)
        .map_err(|e| format!("Failed to expand input patterns: {}", e))?;
    let expanded: Vec<String> = expanded
        .into_iter()
        .filter(|path| !has_glob_meta(path) || Path::new(path).is_file())
        .collect();
    if expanded.is_empty() {
        return Err("No input files matched the provided patterns".to_string());
    }

    if expanded.len() > 1 && opts.output.is_some() {
        return Err("--output cannot be used with multiple input files".to_string());
    }

    let key_style = parse_key_style(&opts.key_style)?;

    let mut skip_missing: HashSet<String> = HashSet::new();
    let mut failures: Vec<String> = Vec::new();
    let mut processed_count: usize = 0;
    let mut success_count: usize = 0;
    let mut failed_count: usize = 0;
    let mut changed_count: usize = 0;

    for original in &opts.inputs {
        if !has_glob_meta(original) && !Path::new(original).is_file() {
            let msg = format!("Input file does not exist: {}", original);
            if opts.continue_on_error {
                eprintln!("❌ {}", msg);
                failures.push(msg);
                processed_count += 1;
                failed_count += 1;
                skip_missing.insert(original.clone());
                continue;
            }
            return Err(msg);
        }
    }

    for input in expanded {
        if skip_missing.contains(&input) {
            continue;
        }

        processed_count += 1;

        match run_normalize_for_file(
            &input,
            &opts.output,
            opts.dry_run,
            opts.check,
            opts.no_placeholders,
            &key_style,
            opts.strict,
        ) {
            Ok(changed) => {
                success_count += 1;
                if changed {
                    changed_count += 1;
                }
            }
            Err(err) => {
                failed_count += 1;
                if opts.continue_on_error {
                    eprintln!("❌ {}", err);
                    failures.push(err);
                    continue;
                }

                println!(
                    "Summary: processed {}; success: {}; failed: {}; changed: {}",
                    processed_count, success_count, failed_count, changed_count
                );
                return Err(err);
            }
        }
    }

    println!(
        "Summary: processed {}; success: {}; failed: {}; changed: {}",
        processed_count, success_count, failed_count, changed_count
    );

    if failures.is_empty() {
        return Ok(());
    }

    Err(format!(
        "{} file(s) failed. See errors above.",
        failures.len()
    ))
}
