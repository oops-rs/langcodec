use crate::convert::read_resources_from_any_input;
use crate::ui;
use crate::validation::{validate_file_path, validate_language_code, validate_output_path};
use langcodec::{
    Codec, ReadOptions, SyncOptions as LibSyncOptions, SyncReport, formats::FormatType,
    sync_existing_entries,
};
use serde_json::json;

#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub source: String,
    pub target: String,
    pub output: Option<String>,
    pub lang: Option<String>,
    pub match_lang: Option<String>,
    pub report_json: Option<String>,
    pub fail_on_unmatched: bool,
    pub fail_on_ambiguous: bool,
    pub strict: bool,
    pub dry_run: bool,
}

fn normalize_lang(lang: &str) -> String {
    lang.trim().replace('_', "-").to_ascii_lowercase()
}

fn lang_base(lang: &str) -> &str {
    lang.split('-').next().unwrap_or(lang)
}

fn lang_matches(resource_lang: &str, requested_lang: &str) -> bool {
    let res = normalize_lang(resource_lang);
    let req = normalize_lang(requested_lang);
    res == req || lang_base(&res) == lang_base(&req)
}

fn infer_output_format_from_path(path: &str) -> Result<FormatType, String> {
    langcodec::infer_format_from_extension(path)
        .ok_or_else(|| format!("Cannot infer format from path: {}", path))
}

fn reject_xliff_sync_paths(
    source: &str,
    target: &str,
    output: Option<&String>,
) -> Result<(), String> {
    if target.ends_with(".stringsdict") || output.is_some_and(|path| path.ends_with(".stringsdict"))
    {
        return Err(
            ".stringsdict cannot be a `sync` target or output because sync provenance is not representable. It may still be used as the source."
                .to_string(),
        );
    }
    if source.ends_with(".xliff")
        || target.ends_with(".xliff")
        || output.is_some_and(|path| path.ends_with(".xliff"))
    {
        return Err(
            ".xliff is not supported by `sync` in v1. Convert XLIFF into a standard project format first."
                .to_string(),
        );
    }
    Ok(())
}

fn pick_single_resource<'a>(
    codec: &'a Codec,
    lang: &Option<String>,
) -> Result<&'a langcodec::Resource, String> {
    if let Some(l) = lang {
        codec
            .resources
            .iter()
            .find(|r| lang_matches(&r.metadata.language, l))
            .ok_or_else(|| format!("Language '{}' not found in output resources", l))
    } else if codec.resources.len() == 1 {
        Ok(&codec.resources[0])
    } else {
        Err("Multiple languages present; specify --lang".to_string())
    }
}

fn write_back(
    codec: &Codec,
    target_path: &str,
    output_path: &Option<String>,
    lang: &Option<String>,
) -> Result<(), String> {
    let target_owned = target_path.to_string();
    let out = output_path.as_ref().unwrap_or(&target_owned);
    let fmt = infer_output_format_from_path(out)?;

    match fmt {
        FormatType::Strings(_) | FormatType::AndroidStrings(_) => {
            let res = pick_single_resource(codec, lang)?;
            langcodec::Codec::write_resource_to_file(res, out)
                .map_err(|e| format!("Error writing output: {}", e))
        }
        FormatType::Stringsdict(_) => Err(
            ".stringsdict cannot be a `sync` target or output because sync provenance is not representable."
                .to_string(),
        ),
        FormatType::Xcstrings | FormatType::CSV | FormatType::TSV => {
            langcodec::converter::convert_resources_to_format(codec.resources.clone(), out, fmt)
                .map_err(|e| format!("Error writing output: {}", e))
        }
        FormatType::Xliff(_) => Err(
            ".xliff is not supported by `sync` in v1. Convert XLIFF into a standard project format first."
                .to_string(),
        ),
    }
}

fn write_report(path: &str, options: &SyncOptions, report: &SyncReport) -> Result<(), String> {
    let payload = json!({
        "source": options.source,
        "target": options.target,
        "output": options.output,
        "lang": options.lang,
        "match_lang": report.match_language,
        "strict": options.strict,
        "fail_on_unmatched": options.fail_on_unmatched,
        "fail_on_ambiguous": options.fail_on_ambiguous,
        "dry_run": options.dry_run,
        "summary": {
            "total_entries": report.total_entries,
            "updated": report.updated,
            "unchanged": report.unchanged,
            "fallback_matches": report.fallback_matches,
            "skipped_unmatched": report.skipped_unmatched,
            "skipped_missing_language": report.skipped_missing_language,
            "skipped_ambiguous_fallback": report.skipped_ambiguous_fallback,
            "skipped_type_mismatch": report.skipped_type_mismatch
        },
        "issues": report.issues
    });

    let text = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("Failed to serialize report JSON: {}", e))?;
    std::fs::write(path, text).map_err(|e| format!("Failed to write report JSON '{}': {}", path, e))
}

pub fn run_sync_command(opts: SyncOptions) -> Result<(), String> {
    reject_xliff_sync_paths(&opts.source, &opts.target, opts.output.as_ref())?;

    validate_file_path(&opts.source)?;
    validate_file_path(&opts.target)?;
    if let Some(output) = &opts.output {
        validate_output_path(output)?;
    }
    if let Some(lang) = &opts.lang {
        validate_language_code(lang)?;
    }
    if let Some(match_lang) = &opts.match_lang {
        validate_language_code(match_lang)?;
    }
    if let Some(report_path) = &opts.report_json {
        validate_output_path(report_path)?;
    }

    let source_resources = read_resources_from_any_input(&opts.source, None, None, opts.strict)?;
    let mut target_codec = Codec::new();
    target_codec
        .read_file_by_extension_with_options(
            &opts.target,
            &ReadOptions::new()
                .with_strict(opts.strict)
                .with_provenance(true),
        )
        .map_err(|e| format!("Failed to read target '{}': {}", opts.target, e))?;

    let report = sync_existing_entries(
        &source_resources,
        &mut target_codec.resources,
        &LibSyncOptions {
            language_filter: opts.lang.clone(),
            match_language: opts.match_lang.clone(),
            fail_on_unmatched: false,
            fail_on_ambiguous: false,
            record_provenance: true,
        },
    )
    .map_err(|e| e.to_string())?;

    if opts.lang.is_some() && report.processed_languages == 0 {
        return Err(format!(
            "Language '{}' not found in target file",
            opts.lang.clone().unwrap_or_default()
        ));
    }

    if ui::stdout_styled() {
        println!("{}", ui::header("Sync"));
        println!(
            "{}",
            ui::key_value("Sync match language", &report.match_language)
        );
        println!(
            "{}",
            ui::key_value("Total target entries considered", report.total_entries)
        );
        println!("{}", ui::key_value("Updated", report.updated));
        println!("{}", ui::key_value("Unchanged", report.unchanged));
        println!(
            "{}",
            ui::key_value("Fallback matches used", report.fallback_matches)
        );
        println!(
            "{}",
            ui::key_value("Skipped unmatched", report.skipped_unmatched)
        );
        println!(
            "{}",
            ui::key_value(
                "Skipped missing source language value",
                report.skipped_missing_language
            )
        );
        println!(
            "{}",
            ui::key_value(
                "Skipped ambiguous fallback",
                report.skipped_ambiguous_fallback
            )
        );
        println!(
            "{}",
            ui::key_value("Skipped type mismatch", report.skipped_type_mismatch)
        );
    } else {
        println!("Sync match language: {}", report.match_language);
        println!("Total target entries considered: {}", report.total_entries);
        println!("Updated: {}", report.updated);
        println!("Unchanged: {}", report.unchanged);
        println!("Fallback matches used: {}", report.fallback_matches);
        println!("Skipped (unmatched): {}", report.skipped_unmatched);
        println!(
            "Skipped (missing source language value): {}",
            report.skipped_missing_language
        );
        println!(
            "Skipped (ambiguous fallback): {}",
            report.skipped_ambiguous_fallback
        );
        println!("Skipped (type mismatch): {}", report.skipped_type_mismatch);
    }

    if let Some(report_path) = &opts.report_json {
        write_report(report_path, &opts, &report)?;
        println!(
            "{}",
            ui::status_line_stdout(
                ui::Tone::Success,
                &format!("Report JSON written: {}", report_path),
            )
        );
    }

    let fail_on_unmatched = opts.fail_on_unmatched || opts.strict;
    let fail_on_ambiguous = opts.fail_on_ambiguous || opts.strict;
    if (fail_on_unmatched && report.skipped_unmatched > 0)
        || (fail_on_ambiguous && report.skipped_ambiguous_fallback > 0)
    {
        let mut reasons = Vec::new();
        if fail_on_unmatched && report.skipped_unmatched > 0 {
            reasons.push(format!("unmatched={}", report.skipped_unmatched));
        }
        if fail_on_ambiguous && report.skipped_ambiguous_fallback > 0 {
            reasons.push(format!("ambiguous={}", report.skipped_ambiguous_fallback));
        }
        return Err(format!("Sync policy failure ({})", reasons.join(", ")));
    }

    if opts.dry_run {
        println!(
            "{}",
            ui::status_line_stdout(ui::Tone::Warning, "Dry-run mode: no files were written")
        );
        return Ok(());
    }

    write_back(&target_codec, &opts.target, &opts.output, &opts.lang)?;
    println!(
        "{}",
        ui::status_line_stdout(
            ui::Tone::Success,
            &format!(
                "Sync complete: {}",
                opts.output.as_deref().unwrap_or(&opts.target)
            ),
        )
    );
    Ok(())
}
