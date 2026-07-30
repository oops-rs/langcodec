use crate::convert::read_resources_from_any_input;
use crate::ui;
use crate::validation::{validate_file_path, validate_language_code, validate_output_path};
use langcodec::{DiffOptions as LibDiffOptions, DiffReport, Translation, diff_resources};

#[derive(Debug, Clone)]
pub struct DiffOptions {
    pub source: String,
    pub target: String,
    pub lang: Option<String>,
    pub json: bool,
    pub output: Option<String>,
    pub strict: bool,
}

fn translation_as_text(value: &Translation) -> String {
    match value {
        Translation::Empty => String::new(),
        Translation::Singular(v) => v.clone(),
        Translation::Plural(p) => {
            let mut parts = Vec::new();
            for (category, text) in &p.forms {
                parts.push(format!("{:?}={}", category, text));
            }
            parts.join(" | ")
        }
    }
}

fn print_or_write(output: Option<&String>, content: &str) -> Result<(), String> {
    if let Some(path) = output {
        std::fs::write(path, content).map_err(|e| format!("Failed to write {}: {}", path, e))?;
        println!(
            "{}",
            ui::status_line_stdout(ui::Tone::Success, &format!("Report written: {}", path))
        );
    } else {
        println!("{}", content);
    }
    Ok(())
}

fn render_human(report: &DiffReport) -> String {
    if ui::stdout_styled() {
        let mut lines = vec![
            ui::header("Diff"),
            ui::key_value("Languages", report.summary.languages),
            ui::key_value("Added", report.summary.added),
            ui::key_value("Removed", report.summary.removed),
            ui::key_value("Changed", report.summary.changed),
            ui::key_value("Unchanged", report.summary.unchanged),
        ];

        for lang in &report.languages {
            lines.push(ui::section(&format!("Language {}", lang.language)));
            lines.push(ui::divider(28));
            lines.push(ui::key_value("added", lang.added.len()));
            lines.push(ui::key_value("removed", lang.removed.len()));
            lines.push(ui::key_value("changed", lang.changed.len()));
            lines.push(ui::key_value("unchanged", lang.unchanged));
            if !lang.added.is_empty() {
                lines.push(ui::key_value("added keys", lang.added.join(", ")));
            }
            if !lang.removed.is_empty() {
                lines.push(ui::key_value("removed keys", lang.removed.join(", ")));
            }
            if !lang.changed.is_empty() {
                let mut changed_lines = Vec::new();
                for item in &lang.changed {
                    changed_lines.push(format!(
                        "{} ({} → {})",
                        ui::accent(&item.key),
                        translation_as_text(&item.target),
                        translation_as_text(&item.source)
                    ));
                }
                lines.push(ui::key_value("changed keys", changed_lines.join(", ")));
            }
        }

        return lines.join("\n");
    }

    let mut lines = Vec::new();
    lines.push("=== Diff ===".to_string());
    lines.push(format!("Languages: {}", report.summary.languages));
    lines.push(format!(
        "Totals: added={}, removed={}, changed={}, unchanged={}",
        report.summary.added,
        report.summary.removed,
        report.summary.changed,
        report.summary.unchanged
    ));

    for lang in &report.languages {
        lines.push(format!("\nLanguage: {}", lang.language));
        lines.push(format!("  added: {}", lang.added.len()));
        lines.push(format!("  removed: {}", lang.removed.len()));
        lines.push(format!("  changed: {}", lang.changed.len()));
        lines.push(format!("  unchanged: {}", lang.unchanged));
        if !lang.added.is_empty() {
            lines.push(format!("  added keys: {}", lang.added.join(", ")));
        }
        if !lang.removed.is_empty() {
            lines.push(format!("  removed keys: {}", lang.removed.join(", ")));
        }
        if !lang.changed.is_empty() {
            let mut changed_lines = Vec::new();
            for item in &lang.changed {
                changed_lines.push(format!(
                    "{} ('{}' -> '{}')",
                    item.key,
                    translation_as_text(&item.target),
                    translation_as_text(&item.source)
                ));
            }
            lines.push(format!("  changed keys: {}", changed_lines.join(", ")));
        }
    }

    lines.join("\n")
}

fn render_json(report: &DiffReport) -> Result<String, String> {
    let languages_json: Vec<_> = report
        .languages
        .iter()
        .map(|lang| {
            let changed: Vec<_> = lang
                .changed
                .iter()
                .map(|item| {
                    serde_json::json!({
                        "key": item.key,
                        "source": item.source,
                        "target": item.target,
                    })
                })
                .collect();

            serde_json::json!({
                "language": lang.language,
                "counts": {
                    "added": lang.added.len(),
                    "removed": lang.removed.len(),
                    "changed": lang.changed.len(),
                    "unchanged": lang.unchanged,
                },
                "added": lang.added,
                "removed": lang.removed,
                "changed": changed,
            })
        })
        .collect();

    let payload = serde_json::json!({
        "summary": {
            "languages": report.summary.languages,
            "added": report.summary.added,
            "removed": report.summary.removed,
            "changed": report.summary.changed,
            "unchanged": report.summary.unchanged,
        },
        "languages": languages_json,
    });

    serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("Failed to serialize diff report JSON: {}", e))
}

pub fn run_diff_command(opts: DiffOptions) -> Result<(), String> {
    validate_file_path(&opts.source)?;
    validate_file_path(&opts.target)?;
    if let Some(lang) = &opts.lang {
        validate_language_code(lang)?;
    }
    if let Some(output) = &opts.output {
        validate_output_path(output)?;
    }

    let source_resources = read_resources_from_any_input(&opts.source, None, None, opts.strict)?;
    let target_resources = read_resources_from_any_input(&opts.target, None, None, opts.strict)?;

    let report = diff_resources(
        &source_resources,
        &target_resources,
        &LibDiffOptions {
            language_filter: opts.lang.clone(),
        },
    );

    if opts.json {
        let rendered = render_json(&report)?;
        print_or_write(opts.output.as_ref(), &rendered)?;
    } else {
        let rendered = render_human(&report);
        print_or_write(opts.output.as_ref(), &rendered)?;
    }

    Ok(())
}
