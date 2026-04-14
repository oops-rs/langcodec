mod ai;
mod annotate;
mod config;
mod convert;
mod debug;
mod diff;
mod edit;
mod editor;
mod formats;
mod merge;
mod normalize;
mod path_glob;
mod stats;
mod sync;
mod tolgee;
mod transformers;
mod translate;
mod tui;
mod ui;
mod validation;
mod view;

use crate::annotate::{AnnotateOptions, run_annotate_command};
use crate::convert::{ConvertOptions, run_unified_convert_command, try_custom_format_view};
use crate::editor::{BrowseOptions, run_browse_command};
use crate::debug::run_debug_command;
use crate::diff::{DiffOptions, run_diff_command};
use crate::edit::{EditSetOptions, run_edit_set_command};
use crate::merge::{ConflictStrategy, run_merge_command};
use crate::normalize::{NormalizeCliOptions, run_normalize_command};
use crate::sync::{SyncOptions, run_sync_command};
use crate::tolgee::{
    TolgeePullOptions, TolgeePushOptions, run_tolgee_pull_command, run_tolgee_push_command,
};
use crate::translate::{TranslateOptions, run_translate_command};
use crate::tui::UiMode;
use crate::validation::{ValidationContext, validate_context, validate_language_code};
use crate::view::{ViewOptions, print_view, validate_status_filter};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};

use langcodec::Codec;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None, styles = ui::clap_styles())]
struct Args {
    /// Enable strict mode (disables parser fallbacks and enforces stricter failures)
    #[arg(long, global = true, default_value_t = false)]
    strict: bool,

    #[command(subcommand)]
    commands: Commands,
}

/// Supported subcommands.
#[derive(Subcommand, Debug)]
enum Commands {
    /// Convert localization files between formats.
    ///
    /// This command automatically detects input and output formats from file extensions.
    /// For JSON files, it will try multiple parsing strategies:
    /// - Standard Resource format (if supported by langcodec)
    /// - JSON key-value pairs (for custom JSON formats)
    Convert {
        /// The input file to process
        #[arg(short, long)]
        input: String,
        /// The output file to write the results to
        #[arg(short, long)]
        output: String,
        /// Optional input format hint (e.g., "json-language-map", "json-array-language-map", "yaml-language-map", "strings", "android", "xliff")
        #[arg(long)]
        input_format: Option<String>,
        /// Optional output format hint (e.g., "xcstrings", "xliff", "strings", "android")
        #[arg(long)]
        output_format: Option<String>,
        /// For xcstrings or xliff output: override source language (default: inferred or en for xcstrings)
        #[arg(long)]
        source_language: Option<String>,
        /// For xcstrings output: override version (default: 1.0)
        #[arg(long)]
        version: Option<String>,
        /// Select the output language for `.strings`, `strings.xml`, or `.xliff` output
        #[arg(long, value_name = "LANG")]
        output_lang: Option<String>,
        /// Language codes to exclude from output (e.g., "en", "fr"). Can be specified multiple times or as comma-separated values (e.g., "--exclude-lang en,fr,zh-hans"). Only affects .langcodec output format.
        #[arg(long, value_name = "LANG", value_delimiter = ',')]
        exclude_lang: Vec<String>,
        /// Language codes to include in output (e.g., "en", "fr"). Can be specified multiple times or as comma-separated values (e.g., "--include-lang en,fr,zh-hans"). If specified, only these languages will be included. Only affects .langcodec output format.
        #[arg(long, value_name = "LANG", value_delimiter = ',')]
        include_lang: Vec<String>,
    },

    /// Edit localization files in-place.
    ///
    /// The `set` action unifies add/update/remove:
    /// - If the key does not exist, it is added
    /// - If `--value` is an empty string or omitted, the key is removed
    /// - Otherwise the key is updated
    Edit {
        #[command(subcommand)]
        command: EditCommands,
    },

    /// Compare two localization files and report added/removed/changed keys by language.
    Diff {
        /// Source localization file (A)
        #[arg(short = 's', long)]
        source: String,

        /// Target localization file (B)
        #[arg(short = 't', long)]
        target: String,

        /// Optional language code to filter by
        #[arg(short, long)]
        lang: Option<String>,

        /// Output JSON instead of human-readable text
        #[arg(long)]
        json: bool,

        /// Optional output file path (defaults to stdout)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Sync existing entries from a source file into a target file.
    ///
    /// Behavior:
    /// - Only updates entries that already exist in target
    /// - Never adds new keys to target
    /// - Matches by key first
    /// - Fallback matching by source-language translation (`--match-lang`, default: inferred/en)
    #[command(verbatim_doc_comment)]
    Sync {
        /// Source localization file (A): values are copied from here
        #[arg(short = 's', long)]
        source: String,

        /// Target localization file (B): existing entries are updated here
        #[arg(short = 't', long)]
        target: String,

        /// Optional output path (default: write back to --target)
        #[arg(short, long)]
        output: Option<String>,

        /// Restrict updates to a single target language (e.g., "fr")
        #[arg(short, long)]
        lang: Option<String>,

        /// Language used for translation-based fallback matching (e.g., "en")
        #[arg(long)]
        match_lang: Option<String>,

        /// Write machine-readable sync report JSON to a file
        #[arg(long)]
        report_json: Option<String>,

        /// Fail when target entries cannot be matched to source
        #[arg(long, default_value_t = false)]
        fail_on_unmatched: bool,

        /// Fail when fallback matching is ambiguous
        #[arg(long, default_value_t = false)]
        fail_on_ambiguous: bool,

        /// Preview changes without writing
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// View localization files.
    View {
        /// The input file to view
        #[arg(short, long)]
        input: String,

        /// Optional language code to filter entries by
        #[arg(short, long)]
        lang: Option<String>,

        /// Display full value without truncation (even in terminal)
        #[arg(long)]
        full: bool,

        /// Filter entries by status (e.g. translated, needs_review, stale, new, do_not_translate)
        #[arg(long)]
        status: Option<String>,

        /// Print keys only
        #[arg(long, default_value_t = false)]
        keys_only: bool,

        /// Output JSON instead of human-readable text
        #[arg(long, default_value_t = false)]
        json: bool,

        /// Validate plural completeness against CLDR category sets
        #[arg(long, default_value_t = false)]
        check_plurals: bool,
    },

    /// Merge multiple localization files into one output file with automatic format detection and conversion.
    ///
    /// This command intelligently merges multiple localization files, automatically detecting
    /// input formats and converting to the output format based on the file extension.
    /// Supports merging files with the same language and provides conflict resolution strategies.
    Merge {
        /// The input files to merge (supports multiple formats: .strings, .xml, .csv, .tsv, .xcstrings, .json, .yaml)
        #[arg(short, long, num_args = 1.., help = "Input files. Supports glob patterns. Quote patterns to avoid slow shell-side expansion (e.g., '/path/**/*/strings.xml').")]
        inputs: Vec<String>,
        /// The output file path (format automatically determined from extension)
        #[arg(short, long)]
        output: String,
        /// Strategy for handling conflicts when merging entries with the same key
        #[arg(short, long, default_value = "last")]
        strategy: ConflictStrategy,
        /// Language code to use for all input files (e.g., "en", "fr")
        #[arg(short, long)]
        lang: Option<String>,
        /// For xcstrings output: override source language (default: en)
        #[arg(long)]
        source_language: Option<String>,
        /// For xcstrings output: override version (default: 1.0)
        #[arg(long)]
        version: Option<String>,
    },

    /// Normalize localization files.
    Normalize {
        /// The input files to normalize (supports glob patterns). Quote patterns to avoid shell expansion.
        #[arg(short, long, required = true, num_args = 1.., help = "Input files. Supports glob patterns. Quote patterns to avoid slow shell-side expansion (e.g., '/path/**/*/Localizable.strings').")]
        inputs: Vec<String>,

        /// Optional output file (single-file mode only). If omitted, writes in-place.
        #[arg(short, long)]
        output: Option<String>,

        /// Preview changes without writing
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// Exit non-zero if normalization would change the file
        #[arg(long, default_value_t = false)]
        check: bool,

        /// Disable placeholder normalization
        #[arg(long, default_value_t = false)]
        no_placeholders: bool,

        /// Key renaming style: none|snake|kebab|camel
        #[arg(long, default_value = "none")]
        key_style: String,

        /// Continue processing remaining files when a file fails
        #[arg(long, default_value_t = false)]
        continue_on_error: bool,
    },

    /// Show translation coverage and per-status counts.
    Stats {
        /// The input file to analyze
        #[arg(short, long)]
        input: String,
        /// Optional language code to filter by
        #[arg(short, long)]
        lang: Option<String>,
        /// Output JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },

    /// Translate source entries into a target language using Mentra-backed providers.
    Translate {
        /// Source localization file. Required unless configured in `langcodec.toml`.
        #[arg(short = 's', long)]
        source: Option<String>,

        /// Optional target localization file. If omitted, translates in-place within multi-language files.
        #[arg(short = 't', long)]
        target: Option<String>,

        /// Optional output file. Defaults to in-place write to target or source.
        #[arg(short, long)]
        output: Option<String>,

        /// Source language code. Required when the source file contains multiple languages.
        #[arg(long)]
        source_lang: Option<String>,

        /// Target language code(s). Comma-separated values are supported for multi-language outputs.
        #[arg(long, value_name = "LANG", value_delimiter = ',')]
        target_lang: Vec<String>,

        /// Filter target entries by status before translating (default: new,stale)
        #[arg(long)]
        status: Option<String>,

        /// Mentra provider to use: openai, anthropic, gemini
        #[arg(long)]
        provider: Option<String>,

        /// Model identifier to use with Mentra
        #[arg(long)]
        model: Option<String>,

        /// Number of concurrent translation workers
        #[arg(long)]
        concurrency: Option<usize>,

        /// Optional langcodec.toml path
        #[arg(long)]
        config: Option<String>,

        /// Enable Tolgee prefill and push-back for this translation run
        #[arg(long, default_value_t = false)]
        tolgee: bool,

        /// Optional Tolgee source config path (.tolgeerc.json or langcodec.toml)
        #[arg(long)]
        tolgee_config: Option<String>,

        /// Restrict Tolgee operations to one or more namespaces
        #[arg(long, value_name = "NAMESPACE", value_delimiter = ',')]
        tolgee_namespace: Vec<String>,

        /// Preview the translation run without writing files
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// Terminal UI mode: auto, plain, or tui
        #[arg(long = "ui", value_enum, default_value_t = UiMode::Auto)]
        ui_mode: UiMode,
    },

    /// Generate translator-facing localization comments from source usage with a Mentra agent.
    Annotate {
        /// Localization file to annotate (`.xcstrings`, `.strings`, or Android `strings.xml`). Required unless configured in `langcodec.toml`.
        #[arg(short, long)]
        input: Option<String>,

        /// Source roots to scan and expose to the agent. Repeat for multiple roots.
        #[arg(long = "source-root")]
        source_roots: Vec<String>,

        /// Optional output file in the same format as the input. Defaults to writing back to the input file.
        #[arg(short, long)]
        output: Option<String>,

        /// Override the source language used to resolve source values from the input file.
        #[arg(long)]
        source_lang: Option<String>,

        /// Mentra provider to use: openai, anthropic, gemini
        #[arg(long)]
        provider: Option<String>,

        /// Model identifier to use with Mentra
        #[arg(long)]
        model: Option<String>,

        /// Number of concurrent annotation workers
        #[arg(long)]
        concurrency: Option<usize>,

        /// Optional langcodec.toml path
        #[arg(long)]
        config: Option<String>,

        /// Preview changes without writing files
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// Exit non-zero if comments would be added or refreshed
        #[arg(long, default_value_t = false)]
        check: bool,

        /// Terminal UI mode: auto, plain, or tui
        #[arg(long = "ui", value_enum, default_value_t = UiMode::Auto)]
        ui_mode: UiMode,
    },

    /// Sync xcstrings catalogs with Tolgee using langcodec.toml or .tolgeerc.json mappings.
    Tolgee {
        #[command(subcommand)]
        command: TolgeeCommands,
    },

    /// Interactive TUI browser for localization files.
    ///
    /// Opens an interactive terminal editor to view, search, and edit
    /// localization keys and translations without an IDE.
    ///
    /// Key bindings:
    ///   j/↓ k/↑      Navigate keys
    ///   Tab/Shift+Tab  Select language
    ///   /              Enter search mode
    ///   e              Edit selected translation
    ///   s              Save changes
    ///   q              Quit (prompts if unsaved changes)
    #[command(verbatim_doc_comment)]
    Browse {
        /// Localization file to open (.xcstrings, .strings, .xml, .xliff, .csv, .tsv)
        #[arg(short, long)]
        input: String,
        /// Language hint (required for single-language formats like .strings)
        #[arg(short, long)]
        lang: Option<String>,
    },

    /// Debug: Read a localization file and output as JSON.
    Debug {
        /// The input file to debug
        #[arg(short, long)]
        input: String,
        /// Language code to use (e.g., "en", "fr")
        #[arg(short, long)]
        lang: Option<String>,
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Generate shell completion script and print to stdout.
    ///
    /// Examples:
    /// - langcodec completions bash > /etc/bash_completion.d/langcodec
    /// - langcodec completions zsh > "${fpath[1]}/_langcodec"
    /// - langcodec completions fish > ~/.config/fish/completions/langcodec.fish
    /// - langcodec completions powershell > langcodec.ps1
    Completions {
        /// Shell to generate completions for (bash, zsh, fish, powershell, elvish)
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Subcommand, Debug)]
enum TolgeeCommands {
    /// Pull translations from Tolgee and merge them into mapped local xcstrings files.
    Pull {
        /// Optional Tolgee source config path (.tolgeerc.json or langcodec.toml)
        #[arg(long)]
        config: Option<String>,

        /// Restrict the pull to one or more namespaces
        #[arg(long, value_name = "NAMESPACE", value_delimiter = ',')]
        namespace: Vec<String>,

        /// Preview the pull without writing local files
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Push mapped local xcstrings files to Tolgee.
    Push {
        /// Optional Tolgee source config path (.tolgeerc.json or langcodec.toml)
        #[arg(long)]
        config: Option<String>,

        /// Restrict the push to one or more namespaces
        #[arg(long, value_name = "NAMESPACE", value_delimiter = ',')]
        namespace: Vec<String>,

        /// Preview the push without invoking Tolgee
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug)]
enum EditCommands {
    /// Set a key's value (add/update/remove unified).
    ///
    /// Behavior:
    /// - Missing key → add
    /// - Empty or omitted --value → remove
    /// - Otherwise → update
    Set {
        /// The input files to modify (supports glob patterns). Quote patterns to avoid shell expansion.
        #[arg(short, long, num_args = 1.., help = "Input files. Supports glob patterns. Quote patterns to avoid slow shell-side expansion (e.g., '/path/**/*/Localizable.strings').")]
        inputs: Vec<String>,

        /// Language code (required for single-language formats when multiple resources present)
        #[arg(short, long)]
        lang: Option<String>,

        /// Entry key to set
        #[arg(short, long)]
        key: String,

        /// New value. If omitted or empty, the entry will be removed.
        #[arg(short, long)]
        value: Option<String>,

        /// Optional translator comment
        #[arg(long)]
        comment: Option<String>,

        /// Optional status: translated|needs_review|new|do_not_translate|stale
        #[arg(long)]
        status: Option<String>,

        /// Optional output file; if omitted, writes in-place to input
        #[arg(short, long)]
        output: Option<String>,

        /// Preview changes without writing
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// Continue processing remaining files when a file fails
        #[arg(long, default_value_t = false)]
        continue_on_error: bool,
    },
}

fn is_custom_input_extension(input: &str) -> bool {
    input.ends_with(".json")
        || input.ends_with(".yaml")
        || input.ends_with(".yml")
        || input.ends_with(".langcodec")
}

fn input_supports_explicit_status_metadata(input: &str) -> bool {
    std::path::Path::new(input)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("xcstrings"))
}

fn load_codec_for_readonly_command(
    input: &str,
    lang: &Option<String>,
    strict: bool,
) -> Result<Codec, String> {
    let mut codec = Codec::new();
    let is_custom_ext = is_custom_input_extension(input);

    if strict {
        if is_custom_ext {
            try_custom_format_view(input, lang.clone(), &mut codec)?;
        } else {
            codec
                .read_file_by_extension(input, lang.clone())
                .map_err(|e| format!("{}", e))?;
        }
        return Ok(codec);
    }

    if codec.read_file_by_extension(input, lang.clone()).is_ok() {
        return Ok(codec);
    }

    if is_custom_ext {
        try_custom_format_view(input, lang.clone(), &mut codec)?;
        return Ok(codec);
    }

    Err("unsupported format".to_string())
}

fn main() {
    let args = Args::parse();
    let strict = args.strict;

    match args.commands {
        Commands::Convert {
            input,
            output,
            input_format,
            output_format,
            output_lang,
            exclude_lang,
            include_lang,
            source_language,
            version,
        } => {
            // Create validation context
            let mut context = ValidationContext::new()
                .with_input_file(input.clone())
                .with_output_file(output.clone());

            if let Some(format) = &input_format {
                context = context.with_input_format(format.clone());
            }
            if let Some(format) = &output_format {
                context = context.with_output_format(format.clone());
            }
            if let Some(lang) = &output_lang {
                context = context.with_language_code(lang.clone());
            }

            // Validate all inputs
            if let Err(e) = validate_context(&context) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Validation failed: {}", e))
                );
                std::process::exit(1);
            }

            run_unified_convert_command(
                input,
                output,
                ConvertOptions {
                    input_format,
                    output_format,
                    source_language,
                    version,
                    output_lang,
                    exclude_lang,
                    include_lang,
                },
                strict,
            );
        }
        Commands::Edit { command } => match command {
            EditCommands::Set {
                inputs,
                lang,
                key,
                value,
                comment,
                status,
                output,
                dry_run,
                continue_on_error,
            } => {
                let opts = EditSetOptions {
                    inputs,
                    lang,
                    key,
                    value,
                    comment,
                    status,
                    output,
                    dry_run,
                    continue_on_error,
                };

                if let Err(e) = run_edit_set_command(opts) {
                    eprintln!(
                        "{}",
                        ui::status_line_stderr(ui::Tone::Error, &format!("Edit failed: {}", e))
                    );
                    std::process::exit(1);
                }
            }
        },
        Commands::Diff {
            source,
            target,
            lang,
            json,
            output,
        } => {
            let mut context = ValidationContext::new()
                .with_input_file(source.clone())
                .with_input_file(target.clone());
            if let Some(lang_code) = &lang {
                context = context.with_language_code(lang_code.clone());
            }
            if let Some(output_path) = &output {
                context = context.with_output_file(output_path.clone());
            }
            if let Err(e) = validate_context(&context) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Validation failed: {}", e))
                );
                std::process::exit(1);
            }

            if let Err(e) = run_diff_command(DiffOptions {
                source,
                target,
                lang,
                json,
                output,
                strict,
            }) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Diff failed: {}", e))
                );
                std::process::exit(1);
            }
        }
        Commands::Sync {
            source,
            target,
            output,
            lang,
            match_lang,
            report_json,
            fail_on_unmatched,
            fail_on_ambiguous,
            dry_run,
        } => {
            let mut context = ValidationContext::new()
                .with_input_file(source.clone())
                .with_input_file(target.clone());
            if let Some(output_path) = &output {
                context = context.with_output_file(output_path.clone());
            }
            if let Some(lang_code) = &lang {
                context = context.with_language_code(lang_code.clone());
            }
            if let Err(e) = validate_context(&context) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Validation failed: {}", e))
                );
                std::process::exit(1);
            }
            if let Some(match_lang_code) = &match_lang
                && let Err(e) = validate_language_code(match_lang_code)
            {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Validation failed: {}", e))
                );
                std::process::exit(1);
            }

            if let Err(e) = run_sync_command(SyncOptions {
                source,
                target,
                output,
                lang,
                match_lang,
                report_json,
                fail_on_unmatched,
                fail_on_ambiguous,
                strict,
                dry_run,
            }) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Sync failed: {}", e))
                );
                std::process::exit(1);
            }
        }
        Commands::View {
            input,
            lang,
            full,
            status,
            keys_only,
            json,
            check_plurals,
        } => {
            // Create validation context
            let mut context = ValidationContext::new().with_input_file(input.clone());

            if let Some(lang_code) = &lang {
                context = context.with_language_code(lang_code.clone());
            }

            // Validate all inputs
            if let Err(e) = validate_context(&context) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Validation failed: {}", e))
                );
                std::process::exit(1);
            }

            if let Err(e) = validate_status_filter(&status) {
                eprintln!("{}", ui::status_line_stderr(ui::Tone::Error, &e));
                std::process::exit(1);
            }

            if strict && status.is_some() && !input_supports_explicit_status_metadata(&input) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(
                        ui::Tone::Error,
                        "Strict mode with --status requires explicit status metadata. Supported in v1: .xcstrings",
                    )
                );
                std::process::exit(1);
            }

            let codec = match load_codec_for_readonly_command(&input, &lang, strict) {
                Ok(codec) => codec,
                Err(e) => {
                    eprintln!(
                        "{}",
                        ui::status_line_stderr(
                            ui::Tone::Error,
                            &format!("Failed to read file: {}", e)
                        )
                    );
                    std::process::exit(1);
                }
            };

            let view_options = ViewOptions {
                full,
                status,
                keys_only,
                json,
            };

            print_view(&codec, &lang, &view_options);

            if check_plurals {
                match codec.validate_plurals() {
                    Ok(()) => {
                        if json || keys_only {
                            eprintln!(
                                "{}",
                                ui::status_line_stderr(
                                    ui::Tone::Success,
                                    "Plural validation passed",
                                )
                            );
                        } else {
                            println!(
                                "\n{}",
                                ui::status_line_stdout(
                                    ui::Tone::Success,
                                    "Plural validation passed",
                                )
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "\n{}",
                            ui::status_line_stderr(
                                ui::Tone::Error,
                                &format!("Plural validation failed: {}", e),
                            )
                        );
                        std::process::exit(2);
                    }
                }
            }
        }
        Commands::Merge {
            inputs,
            output,
            strategy,
            lang,
            source_language,
            version,
        } => {
            // Expand any glob patterns in inputs (e.g., *.strings, **/*.xml)
            println!(
                "{}",
                ui::status_line_stdout(
                    ui::Tone::Info,
                    &format!("Expanding glob patterns in inputs: {:?}", inputs),
                )
            );
            let expanded_inputs = match path_glob::expand_input_globs(&inputs) {
                Ok(list) => list,
                Err(e) => {
                    eprintln!(
                        "{}",
                        ui::status_line_stderr(
                            ui::Tone::Error,
                            &format!("Failed to expand input patterns: {}", e),
                        )
                    );
                    std::process::exit(1);
                }
            };

            if expanded_inputs.is_empty() {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(
                        ui::Tone::Error,
                        "No input files matched the provided patterns",
                    )
                );
                std::process::exit(1);
            }

            // Create validation context
            let mut context = ValidationContext::new().with_output_file(output.clone());

            for input in &expanded_inputs {
                context = context.with_input_file(input.clone());
            }

            if let Some(lang_code) = &lang {
                context = context.with_language_code(lang_code.clone());
            }

            // Validate all inputs
            if let Err(e) = validate_context(&context) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Validation failed: {}", e))
                );
                std::process::exit(1);
            }

            run_merge_command(
                expanded_inputs,
                output,
                strategy,
                lang,
                source_language,
                version,
                strict,
            );
        }
        Commands::Translate {
            source,
            target,
            output,
            source_lang,
            target_lang,
            status,
            provider,
            model,
            concurrency,
            config,
            tolgee,
            tolgee_config,
            tolgee_namespace,
            dry_run,
            ui_mode,
        } => {
            let mut context = ValidationContext::new();
            if let Some(source_path) = &source {
                context = context.with_input_file(source_path.clone());
            }
            if let Some(target_path) = &target
                && std::path::Path::new(target_path).exists()
            {
                context = context.with_input_file(target_path.clone());
            }
            if let Some(output_path) = &output {
                context = context.with_output_file(output_path.clone());
            } else if let Some(target_path) = &target {
                context = context.with_output_file(target_path.clone());
            }
            if let Some(lang_code) = &source_lang {
                context = context.with_language_code(lang_code.clone());
            }
            if let Err(e) = validate_context(&context) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Validation failed: {}", e))
                );
                std::process::exit(1);
            }
            for lang_code in &target_lang {
                if let Err(e) = validate_language_code(lang_code) {
                    eprintln!(
                        "{}",
                        ui::status_line_stderr(
                            ui::Tone::Error,
                            &format!("Validation failed: {}", e),
                        )
                    );
                    std::process::exit(1);
                }
            }

            if let Err(e) = run_translate_command(TranslateOptions {
                source,
                target,
                output,
                source_lang,
                target_langs: target_lang,
                status,
                provider,
                model,
                concurrency,
                config,
                use_tolgee: tolgee,
                tolgee_config,
                tolgee_namespaces: tolgee_namespace,
                dry_run,
                strict,
                ui_mode,
            }) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Translate failed: {}", e),)
                );
                std::process::exit(1);
            }
        }
        Commands::Tolgee { command } => match command {
            TolgeeCommands::Pull {
                config,
                namespace,
                dry_run,
            } => {
                if let Err(e) = run_tolgee_pull_command(TolgeePullOptions {
                    config,
                    namespaces: namespace,
                    dry_run,
                    strict,
                }) {
                    eprintln!(
                        "{}",
                        ui::status_line_stderr(
                            ui::Tone::Error,
                            &format!("Tolgee pull failed: {}", e),
                        )
                    );
                    std::process::exit(1);
                }
            }
            TolgeeCommands::Push {
                config,
                namespace,
                dry_run,
            } => {
                if let Err(e) = run_tolgee_push_command(TolgeePushOptions {
                    config,
                    namespaces: namespace,
                    dry_run,
                }) {
                    eprintln!(
                        "{}",
                        ui::status_line_stderr(
                            ui::Tone::Error,
                            &format!("Tolgee push failed: {}", e),
                        )
                    );
                    std::process::exit(1);
                }
            }
        },
        Commands::Annotate {
            input,
            source_roots,
            output,
            source_lang,
            provider,
            model,
            concurrency,
            config,
            dry_run,
            check,
            ui_mode,
        } => {
            let mut context = ValidationContext::new();
            if let Some(input_path) = &input {
                context = context.with_input_file(input_path.clone());
            }
            if let Some(output_path) = &output {
                context = context.with_output_file(output_path.clone());
            }
            if let Some(lang_code) = &source_lang {
                context = context.with_language_code(lang_code.clone());
            }
            if let Err(e) = validate_context(&context) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Validation failed: {}", e))
                );
                std::process::exit(1);
            }

            if let Err(e) = run_annotate_command(AnnotateOptions {
                input,
                source_roots,
                output,
                source_lang,
                provider,
                model,
                concurrency,
                config,
                dry_run,
                check,
                ui_mode,
            }) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Annotate failed: {}", e),)
                );
                std::process::exit(1);
            }
        }
        Commands::Browse { input, lang } => {
            if let Err(e) = run_browse_command(BrowseOptions { input, lang }) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Browse failed: {}", e))
                );
                std::process::exit(1);
            }
        }
        Commands::Debug {
            input,
            lang,
            output,
        } => {
            // Create validation context
            let mut context = ValidationContext::new().with_input_file(input.clone());

            if let Some(lang_code) = &lang {
                context = context.with_language_code(lang_code.clone());
            }
            if let Some(output_path) = &output {
                context = context.with_output_file(output_path.clone());
            }

            // Validate all inputs
            if let Err(e) = validate_context(&context) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Validation failed: {}", e))
                );
                std::process::exit(1);
            }

            run_debug_command(input, lang, output, strict);
        }
        Commands::Completions { shell } => {
            let mut cmd = Args::command();
            cmd = cmd.bin_name("langcodec");
            generate(shell, &mut cmd, "langcodec", &mut std::io::stdout());
        }
        Commands::Normalize {
            inputs,
            output,
            dry_run,
            check,
            no_placeholders,
            key_style,
            continue_on_error,
        } => {
            let opts = NormalizeCliOptions {
                inputs,
                output,
                dry_run,
                check,
                no_placeholders,
                key_style,
                continue_on_error,
                strict,
            };
            if let Err(e) = run_normalize_command(opts) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Normalize failed: {}", e),)
                );
                std::process::exit(1);
            }
        }
        Commands::Stats { input, lang, json } => {
            // Validate
            let mut context = ValidationContext::new().with_input_file(input.clone());
            if let Some(l) = &lang {
                context = context.with_language_code(l.clone());
            }
            if let Err(e) = validate_context(&context) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Validation failed: {}", e))
                );
                std::process::exit(1);
            }

            let codec = match load_codec_for_readonly_command(&input, &lang, strict) {
                Ok(codec) => codec,
                Err(e) => {
                    eprintln!(
                        "{}",
                        ui::status_line_stderr(
                            ui::Tone::Error,
                            &format!("Failed to read file: {}", e)
                        )
                    );
                    std::process::exit(1);
                }
            };

            if let Err(e) = stats::print_stats(&codec, &lang, json) {
                eprintln!(
                    "{}",
                    ui::status_line_stderr(ui::Tone::Error, &format!("Stats failed: {}", e))
                );
                std::process::exit(1);
            }
        }
    }
}
