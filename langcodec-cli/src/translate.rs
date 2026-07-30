use crate::validation::{validate_language_code, validate_output_path};
use crate::{
    ai::{ProviderKind, build_provider, resolve_model, resolve_provider},
    config::{LoadedConfig, load_config, resolve_config_relative_path},
    path_glob,
    tolgee::{
        TranslateTolgeeContext, TranslateTolgeeSettings, prefill_translate_from_tolgee,
        push_translate_results_to_tolgee,
    },
    tui::{
        DashboardEvent, DashboardInit, DashboardItem, DashboardItemStatus, DashboardKind,
        DashboardLogTone, PlainReporter, ResolvedUiMode, RunReporter, SummaryRow, TuiReporter,
        UiMode, resolve_ui_mode_for_current_terminal,
    },
};
use async_trait::async_trait;
use langcodec::{
    Codec, Entry, EntryStatus, FormatType, Metadata, ReadOptions, Resource, Translation,
    convert_resources_to_format,
    formats::{AndroidStringsFormat, CSVFormat, StringsFormat, TSVFormat, XcstringsFormat},
    infer_format_from_extension, infer_language_from_path,
    placeholder::signature,
    traits::Parser,
};
use mentra::provider::{
    self, ContentBlock, Message, Provider, ProviderError, ProviderRequestOptions, Request,
};
use serde::Deserialize;
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
};
use tokio::{
    runtime::Builder,
    sync::{Mutex as AsyncMutex, mpsc},
    task::JoinSet,
};

const DEFAULT_STATUSES: [&str; 2] = ["new", "stale"];
const DEFAULT_CONCURRENCY: usize = 4;
const SYSTEM_PROMPT: &str = "You translate application localization strings. Return JSON only with the shape {\"translation\":\"...\"}. Preserve placeholders, escapes, newline markers, surrounding punctuation, HTML/XML tags, Markdown, and product names exactly unless the target language grammar requires adjacent spacing changes. Never add explanations or extra keys.";

#[derive(Debug, Clone)]
pub struct TranslateOptions {
    pub source: Option<String>,
    pub target: Option<String>,
    pub output: Option<String>,
    pub source_lang: Option<String>,
    pub target_langs: Vec<String>,
    pub status: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub concurrency: Option<usize>,
    pub config: Option<String>,
    pub use_tolgee: bool,
    pub tolgee_config: Option<String>,
    pub tolgee_namespaces: Vec<String>,
    pub dry_run: bool,
    pub strict: bool,
    pub ui_mode: UiMode,
}

#[derive(Debug, Clone)]
struct ResolvedOptions {
    source: String,
    target: Option<String>,
    output: Option<String>,
    source_lang: Option<String>,
    target_langs: Vec<String>,
    statuses: Vec<EntryStatus>,
    output_status: EntryStatus,
    provider: Option<ProviderKind>,
    model: Option<String>,
    provider_error: Option<String>,
    model_error: Option<String>,
    concurrency: usize,
    use_tolgee: bool,
    tolgee_config: Option<String>,
    tolgee_namespaces: Vec<String>,
    dry_run: bool,
    strict: bool,
    ui_mode: ResolvedUiMode,
}

#[derive(Debug, Clone)]
struct SelectedResource {
    language: String,
    resource: Resource,
}

#[derive(Debug, Clone)]
struct TranslationJob {
    key: String,
    source_lang: String,
    target_lang: String,
    source_value: String,
    source_comment: Option<String>,
    existing_comment: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct TranslationSummary {
    total_entries: usize,
    queued: usize,
    translated: usize,
    skipped_do_not_translate: usize,
    skipped_plural: usize,
    skipped_status: usize,
    skipped_empty_source: usize,
    failed: usize,
}

#[derive(Debug, Clone)]
struct TranslationResult {
    key: String,
    target_lang: String,
    translated_value: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TranslateOutcome {
    pub translated: usize,
    pub skipped: usize,
    pub failed: usize,
    pub output_path: Option<String>,
}

#[derive(Debug, Clone)]
struct PreparedTranslation {
    opts: ResolvedOptions,
    source_path: String,
    target_path: String,
    output_path: String,
    output_format: FormatType,
    config_path: Option<PathBuf>,
    source_resource: SelectedResource,
    target_codec: Codec,
    tolgee_context: Option<TranslateTolgeeContext>,
    jobs: Vec<TranslationJob>,
    summary: TranslationSummary,
}

#[derive(Clone)]
struct MentraBackend {
    provider: Arc<dyn Provider>,
    model: String,
}

#[derive(Debug, Clone)]
struct BackendRequest {
    key: String,
    source_lang: String,
    target_lang: String,
    source_value: String,
    source_comment: Option<String>,
}

enum TranslationWorkerUpdate {
    Started {
        id: String,
    },
    Finished {
        id: String,
        result: Result<TranslationResult, String>,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct ModelTranslationPayload {
    translation: String,
}

#[async_trait]
trait TranslationBackend: Send + Sync {
    async fn translate(&self, request: BackendRequest) -> Result<String, String>;
}

#[async_trait]
impl TranslationBackend for MentraBackend {
    async fn translate(&self, request: BackendRequest) -> Result<String, String> {
        let prompt = build_prompt(&request);
        let response = self
            .provider
            .send(Request {
                model: Cow::Borrowed(self.model.as_str()),
                system: Some(Cow::Borrowed(SYSTEM_PROMPT)),
                messages: Cow::Owned(vec![Message::user(ContentBlock::text(prompt))]),
                tools: Cow::Owned(Vec::new()),
                tool_choice: None,
                temperature: Some(0.2),
                max_output_tokens: Some(512),
                metadata: Cow::Owned(BTreeMap::new()),
                provider_request_options: ProviderRequestOptions::default(),
            })
            .await
            .map_err(format_provider_error)?;

        let text = collect_text_blocks(&response);
        parse_translation_response(&text)
    }
}

pub fn run_translate_command(opts: TranslateOptions) -> Result<TranslateOutcome, String> {
    let runs = expand_translate_invocations(&opts)?;
    if runs.len() > 1 && matches!(opts.ui_mode, UiMode::Tui) {
        return Err("TUI mode supports only one translate run at a time".to_string());
    }
    if runs.len() == 1 {
        return run_single_translate_command(runs.into_iter().next().unwrap());
    }

    eprintln!(
        "Running {} translate jobs in parallel from config",
        runs.len()
    );

    let mut handles = Vec::new();
    for mut run in runs {
        run.ui_mode = UiMode::Plain;
        handles.push(thread::spawn(move || run_single_translate_command(run)));
    }

    let mut translated = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut first_error = None;

    for handle in handles {
        match handle.join() {
            Ok(Ok(outcome)) => {
                translated += outcome.translated;
                skipped += outcome.skipped;
                failed += outcome.failed;
            }
            Ok(Err(err)) => {
                failed += 1;
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
            Err(_) => {
                failed += 1;
                if first_error.is_none() {
                    first_error = Some("Parallel translate worker panicked".to_string());
                }
            }
        }
    }

    if let Some(err) = first_error {
        return Err(format!(
            "{} (translated={}, skipped={}, failed_jobs={})",
            err, translated, skipped, failed
        ));
    }

    Ok(TranslateOutcome {
        translated,
        skipped,
        failed,
        output_path: None,
    })
}

fn run_single_translate_command(opts: TranslateOptions) -> Result<TranslateOutcome, String> {
    let prepared = prepare_translation(&opts)?;
    if prepared.jobs.is_empty() {
        return run_prepared_translation(prepared, None);
    }
    let backend = create_mentra_backend(&prepared.opts)?;
    run_prepared_translation(prepared, Some(Arc::new(backend)))
}

fn expand_translate_invocations(opts: &TranslateOptions) -> Result<Vec<TranslateOptions>, String> {
    let loaded_config = load_config(opts.config.as_deref())?;
    let cfg = loaded_config.as_ref().map(|item| &item.data.translate);
    let config_path = loaded_config
        .as_ref()
        .map(|item| item.path.to_string_lossy().to_string())
        .or_else(|| opts.config.clone());
    let config_dir = loaded_config
        .as_ref()
        .and_then(|item| item.path.parent())
        .map(Path::to_path_buf);

    if cfg
        .and_then(|item| item.resolved_source())
        .is_some_and(|_| cfg.and_then(|item| item.resolved_sources()).is_some())
    {
        return Err(
            "Config translate.input.source/translate.source and translate.input.sources/translate.sources cannot both be set"
                .to_string(),
        );
    }

    let sources = resolve_config_sources(opts, cfg, config_dir.as_deref())?;
    if sources.is_empty() {
        return Err(
            "--source is required unless translate.input.source/translate.source or translate.input.sources/translate.sources is set in langcodec.toml"
                .to_string(),
        );
    }

    let target = if let Some(path) = &opts.target {
        Some(path.clone())
    } else {
        cfg.and_then(|item| item.resolved_target())
            .map(|path| resolve_config_relative_path(config_dir.as_deref(), path))
    };
    let output = if let Some(path) = &opts.output {
        Some(path.clone())
    } else {
        cfg.and_then(|item| item.resolved_output_path())
            .map(|path| resolve_config_relative_path(config_dir.as_deref(), path))
    };

    if sources.len() > 1 && (target.is_some() || output.is_some()) {
        return Err(
            "translate.input.sources/translate.sources cannot be combined with translate.output.target/translate.target, translate.output.path/translate.output, or CLI --target/--output; use in-place multi-language sources or invoke files individually"
                .to_string(),
        );
    }

    Ok(sources
        .into_iter()
        .map(|source| TranslateOptions {
            source: Some(source),
            target: target.clone(),
            output: output.clone(),
            source_lang: opts
                .source_lang
                .clone()
                .or_else(|| cfg.and_then(|item| item.resolved_source_lang().map(str::to_string))),
            target_langs: if opts.target_langs.is_empty() {
                Vec::new()
            } else {
                opts.target_langs.clone()
            },
            status: opts.status.clone(),
            provider: opts.provider.clone(),
            model: opts.model.clone(),
            concurrency: opts.concurrency,
            config: config_path.clone(),
            use_tolgee: opts.use_tolgee,
            tolgee_config: opts.tolgee_config.clone(),
            tolgee_namespaces: opts.tolgee_namespaces.clone(),
            dry_run: opts.dry_run,
            strict: opts.strict,
            ui_mode: opts.ui_mode,
        })
        .collect())
}

fn resolve_config_sources(
    opts: &TranslateOptions,
    cfg: Option<&crate::config::TranslateConfig>,
    config_dir: Option<&Path>,
) -> Result<Vec<String>, String> {
    fn has_glob_meta(path: &str) -> bool {
        path.bytes().any(|b| matches!(b, b'*' | b'?' | b'[' | b'{'))
    }

    if let Some(source) = &opts.source {
        return Ok(vec![source.clone()]);
    }

    if let Some(source) = cfg.and_then(|item| item.resolved_source()) {
        let resolved = vec![resolve_config_relative_path(config_dir, source)];
        return if resolved.iter().any(|path| has_glob_meta(path)) {
            path_glob::expand_input_globs(&resolved)
        } else {
            Ok(resolved)
        };
    }

    if let Some(sources) = cfg.and_then(|item| item.resolved_sources()) {
        let resolved = sources
            .iter()
            .map(|source| resolve_config_relative_path(config_dir, source))
            .collect::<Vec<_>>();
        return if resolved.iter().any(|path| has_glob_meta(path)) {
            path_glob::expand_input_globs(&resolved)
        } else {
            Ok(resolved)
        };
    }

    Ok(Vec::new())
}

fn run_prepared_translation(
    prepared: PreparedTranslation,
    backend: Option<Arc<dyn TranslationBackend>>,
) -> Result<TranslateOutcome, String> {
    let runtime = Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to create async runtime: {}", e))?;
    runtime.block_on(async_run_translation(prepared, backend))
}

async fn async_run_translation(
    mut prepared: PreparedTranslation,
    backend: Option<Arc<dyn TranslationBackend>>,
) -> Result<TranslateOutcome, String> {
    validate_translation_preflight(&prepared)?;
    if matches!(prepared.opts.ui_mode, ResolvedUiMode::Plain) {
        print_preamble(&prepared);
    }

    if prepared.jobs.is_empty() {
        print_summary(&prepared.summary);
        if prepared.opts.dry_run {
            println!("Dry-run mode: no files were written");
        } else {
            write_back(
                &prepared.target_codec,
                &prepared.output_path,
                &prepared.output_format,
                single_output_language(&prepared.opts.target_langs),
            )?;
            println!("✅ Translate complete: {}", prepared.output_path);
        }
        return Ok(TranslateOutcome {
            translated: 0,
            skipped: count_skipped(&prepared.summary),
            failed: 0,
            output_path: Some(prepared.output_path),
        });
    }

    let worker_count = prepared.opts.concurrency.min(prepared.jobs.len()).max(1);
    let backend = backend.ok_or_else(|| {
        "Translation backend was not configured even though jobs remain".to_string()
    })?;
    let mut reporter = create_translate_reporter(&prepared)?;
    reporter.emit(DashboardEvent::Log {
        tone: DashboardLogTone::Info,
        message: "Preflight validation passed".to_string(),
    });
    reporter.emit(DashboardEvent::Log {
        tone: DashboardLogTone::Info,
        message: format!("Starting {} worker(s)", worker_count),
    });
    let queue = Arc::new(AsyncMutex::new(VecDeque::from(prepared.jobs.clone())));
    let (tx, mut rx) = mpsc::unbounded_channel::<TranslationWorkerUpdate>();
    let mut join_set = JoinSet::new();
    for _ in 0..worker_count {
        let backend = Arc::clone(&backend);
        let queue = Arc::clone(&queue);
        let tx = tx.clone();
        join_set.spawn(async move {
            loop {
                let job = {
                    let mut queue = queue.lock().await;
                    queue.pop_front()
                };

                let Some(job) = job else {
                    break;
                };

                let id = translation_job_id(&job);
                let _ = tx.send(TranslationWorkerUpdate::Started { id: id.clone() });
                let result = backend
                    .translate(BackendRequest {
                        key: job.key.clone(),
                        source_lang: job.source_lang.clone(),
                        target_lang: job.target_lang.clone(),
                        source_value: job.source_value.clone(),
                        source_comment: job.source_comment.clone(),
                    })
                    .await
                    .and_then(|translated_value| {
                        validate_generated_translation(&job, &translated_value)?;
                        Ok(TranslationResult {
                            key: job.key.clone(),
                            target_lang: job.target_lang.clone(),
                            translated_value,
                        })
                    });
                let _ = tx.send(TranslationWorkerUpdate::Finished { id, result });
            }

            Ok::<(), String>(())
        });
    }
    drop(tx);

    let mut results: HashMap<(String, String), String> = HashMap::new();

    while let Some(update) = rx.recv().await {
        match update {
            TranslationWorkerUpdate::Started { id } => {
                reporter.emit(DashboardEvent::UpdateItem {
                    id,
                    status: Some(DashboardItemStatus::Running),
                    subtitle: None,
                    source_text: None,
                    output_text: None,
                    note_text: None,
                    error_text: None,
                    extra_rows: None,
                });
            }
            TranslationWorkerUpdate::Finished { id, result } => match result {
                Ok(item) => {
                    prepared.summary.translated += 1;
                    let translated_value = item.translated_value.clone();
                    results.insert((item.key, item.target_lang), item.translated_value);
                    reporter.emit(DashboardEvent::UpdateItem {
                        id,
                        status: Some(DashboardItemStatus::Succeeded),
                        subtitle: None,
                        source_text: None,
                        output_text: Some(translated_value),
                        note_text: None,
                        error_text: None,
                        extra_rows: None,
                    });
                }
                Err(err) => {
                    prepared.summary.failed += 1;
                    reporter.emit(DashboardEvent::UpdateItem {
                        id,
                        status: Some(DashboardItemStatus::Failed),
                        subtitle: None,
                        source_text: None,
                        output_text: None,
                        note_text: None,
                        error_text: Some(err.clone()),
                        extra_rows: None,
                    });
                    reporter.emit(DashboardEvent::Log {
                        tone: DashboardLogTone::Error,
                        message: err,
                    });
                }
            },
        }
        reporter.emit(DashboardEvent::SummaryRows {
            rows: translation_summary_rows(&prepared.summary),
        });
    }

    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                prepared.summary.failed += 1;
                reporter.emit(DashboardEvent::Log {
                    tone: DashboardLogTone::Error,
                    message: format!("Translation worker failed: {}", err),
                });
            }
            Err(err) => {
                prepared.summary.failed += 1;
                reporter.emit(DashboardEvent::Log {
                    tone: DashboardLogTone::Error,
                    message: format!("Translation task failed to join: {}", err),
                });
            }
        }
        reporter.emit(DashboardEvent::SummaryRows {
            rows: translation_summary_rows(&prepared.summary),
        });
    }

    if prepared.summary.failed > 0 {
        reporter.finish()?;
        print_summary(&prepared.summary);
        return Err("Translation failed; no files were written".to_string());
    }

    if let Err(err) = apply_translation_results(&mut prepared, &results) {
        reporter.emit(DashboardEvent::Log {
            tone: DashboardLogTone::Error,
            message: err.clone(),
        });
        reporter.finish()?;
        print_summary(&prepared.summary);
        return Err(err);
    }
    reporter.emit(DashboardEvent::Log {
        tone: DashboardLogTone::Info,
        message: "Applying translated values".to_string(),
    });
    reporter.emit(DashboardEvent::Log {
        tone: DashboardLogTone::Success,
        message: "Generated placeholder validation passed".to_string(),
    });

    if prepared.opts.dry_run {
        reporter.emit(DashboardEvent::Log {
            tone: DashboardLogTone::Info,
            message: "Dry-run mode: no files were written".to_string(),
        });
        reporter.finish()?;
        print_summary(&prepared.summary);
        println!("Dry-run mode: no files were written");
    } else {
        reporter.emit(DashboardEvent::Log {
            tone: DashboardLogTone::Info,
            message: format!("Writing {}", prepared.output_path),
        });
        if let Err(err) = write_back(
            &prepared.target_codec,
            &prepared.output_path,
            &prepared.output_format,
            single_output_language(&prepared.opts.target_langs),
        ) {
            reporter.emit(DashboardEvent::Log {
                tone: DashboardLogTone::Error,
                message: err.clone(),
            });
            reporter.finish()?;
            print_summary(&prepared.summary);
            return Err(err);
        }
        reporter.emit(DashboardEvent::Log {
            tone: DashboardLogTone::Success,
            message: format!("Wrote {}", prepared.output_path),
        });
        if prepared.summary.translated > 0
            && let Some(context) = prepared.tolgee_context.as_ref()
        {
            reporter.emit(DashboardEvent::Log {
                tone: DashboardLogTone::Info,
                message: format!("Pushing namespace '{}' back to Tolgee", context.namespace()),
            });
            if let Err(err) = push_translate_results_to_tolgee(context, false) {
                reporter.emit(DashboardEvent::Log {
                    tone: DashboardLogTone::Error,
                    message: err.clone(),
                });
                reporter.finish()?;
                print_summary(&prepared.summary);
                return Err(err);
            }
            reporter.emit(DashboardEvent::Log {
                tone: DashboardLogTone::Success,
                message: "Tolgee sync complete".to_string(),
            });
        }
        reporter.finish()?;
        print_summary(&prepared.summary);
        println!("✅ Translate complete: {}", prepared.output_path);
    }
    print_translation_results(&prepared, &results);

    Ok(TranslateOutcome {
        translated: prepared.summary.translated,
        skipped: count_skipped(&prepared.summary),
        failed: 0,
        output_path: Some(prepared.output_path),
    })
}

fn prepare_translation(opts: &TranslateOptions) -> Result<PreparedTranslation, String> {
    let config = load_config(opts.config.as_deref())?;
    let mut resolved = resolve_options(opts, config.as_ref())?;

    validate_path_inputs(&resolved)?;

    let has_explicit_target = resolved.target.is_some();
    let source_path = resolved.source.clone();
    let target_path = resolved
        .target
        .clone()
        .or_else(|| resolved.output.clone())
        .unwrap_or_else(|| resolved.source.clone());
    let output_path = resolved
        .output
        .clone()
        .unwrap_or_else(|| target_path.clone());

    let output_format = infer_format_from_extension(&output_path)
        .ok_or_else(|| format!("Cannot infer output format from path: {}", output_path))?;
    let source_format = infer_format_from_extension(&source_path);
    let target_format = if has_explicit_target {
        infer_format_from_extension(&target_path)
    } else {
        None
    };
    let target_exists = Path::new(&target_path).is_file();
    if [&source_path, &target_path, &output_path]
        .into_iter()
        .any(|path| {
            matches!(
                infer_format_from_extension(path),
                Some(FormatType::Stringsdict(_))
            )
        })
    {
        return Err(
            ".stringsdict is not supported by `translate` until plural-aware translation is implemented."
                .to_string(),
        );
    }
    let source_path_lang_hint = source_format
        .as_ref()
        .map(|format| infer_path_language(&source_path, format))
        .transpose()?
        .flatten();
    let output_lang_hint = infer_path_language(&output_path, &output_format)?;
    let target_lang_hint = if let Some(target_format) = target_format.as_ref() {
        infer_path_language(&target_path, target_format)?
    } else {
        None
    };
    let source_intrinsic_lang_hint = if source_path_lang_hint.is_none() {
        source_format
            .as_ref()
            .map(|format| infer_intrinsic_language(&source_path, format))
            .transpose()?
            .flatten()
    } else {
        None
    };
    let target_intrinsic_lang_hint = if target_exists && target_lang_hint.is_none() {
        target_format
            .as_ref()
            .map(|format| infer_intrinsic_language(&target_path, format))
            .transpose()?
            .flatten()
    } else {
        None
    };
    let target_identity_hint = target_lang_hint
        .as_deref()
        .or(target_intrinsic_lang_hint.as_deref());
    let target_is_source = Path::new(&target_path) == Path::new(&source_path);
    let target_is_distinct = has_explicit_target && !target_is_source;

    if target_is_distinct
        && Path::new(&target_path) != Path::new(&output_path)
        && let (Some(target_lang), Some(output_lang)) =
            (target_identity_hint, output_lang_hint.as_deref())
        && !language_identity_eq(target_lang, output_lang)
    {
        return Err(format!(
            "Target path '{}' identifies language '{}' but output path '{}' identifies language '{}'; refusing to retag the target locale",
            target_path, target_lang, output_path, output_lang
        ));
    }

    if !is_multi_language_format(&output_format) && resolved.target_langs.len() > 1 {
        return Err(
            "Multiple --target-lang values are only supported for multi-language output formats"
                .to_string(),
        );
    }

    if !has_explicit_target
        && output_path == source_path
        && !is_multi_language_format(&output_format)
    {
        return Err(
            "Omitting --target is only supported for in-place multi-language files; use --target or --output for single-language formats"
                .to_string(),
        );
    }

    let source_read_hint = if source_format
        .as_ref()
        .is_some_and(format_requires_external_language)
        && source_path_lang_hint.is_none()
    {
        source_intrinsic_lang_hint
            .clone()
            .or_else(|| resolved.source_lang.clone())
    } else {
        None
    };
    let source_codec = read_codec(&source_path, source_read_hint, resolved.strict)?;
    validate_unique_language_identities(&source_codec, "Source catalog")?;
    let source_resource = select_source_resource(&source_codec, &resolved.source_lang)?;

    let target_read_hint = if target_is_distinct && target_exists {
        resolve_existing_target_read_hint(
            &target_path,
            target_format.as_ref(),
            target_identity_hint,
            &output_path,
            &output_format,
            output_lang_hint.as_deref(),
            &resolved.target_langs,
        )?
    } else {
        target_lang_hint.clone()
    };
    let mut target_codec = if target_is_distinct && target_exists {
        read_codec(&target_path, target_read_hint, resolved.strict)?
    } else if (has_explicit_target && target_is_source)
        || (!has_explicit_target && is_multi_language_format(&output_format))
    {
        source_codec.clone()
    } else {
        Codec::new()
    };
    validate_unique_language_identities(&target_codec, "Target catalog")?;
    let explicit_target_lang_hint = if target_is_distinct {
        target_identity_hint
    } else {
        None
    };

    let target_languages = resolve_target_languages(
        &target_codec,
        &resolved.target_langs,
        explicit_target_lang_hint,
        output_lang_hint.as_deref(),
    )?;
    if !is_multi_language_format(&output_format)
        && let (Some(target_lang), Some(output_lang)) =
            (target_languages.first(), output_lang_hint.as_deref())
        && !language_identity_eq(target_lang, output_lang)
    {
        return Err(format!(
            "Target language '{}' is incompatible with output path '{}' language '{}'; use a matching output locale",
            target_lang, output_path, output_lang
        ));
    }
    if let Some(target_language) = target_languages
        .iter()
        .find(|language| language_identity_eq(&source_resource.language, language))
    {
        return Err(format!(
            "Source language '{}' and target language '{}' must differ",
            source_resource.language, target_language
        ));
    }
    resolved.target_langs = target_languages;

    if target_is_distinct && !target_exists && is_multi_language_format(&output_format) {
        ensure_resource_exists(
            &mut target_codec,
            &source_resource.resource,
            &source_resource.language,
            true,
        );
    }
    for target_lang in &resolved.target_langs {
        ensure_target_resource(&mut target_codec, target_lang)?;
    }
    propagate_xcstrings_metadata(&mut target_codec, &source_resource.resource);

    let tolgee_context = prefill_translate_from_tolgee(
        &TranslateTolgeeSettings {
            enabled: resolved.use_tolgee,
            config: resolved.tolgee_config.clone(),
            namespaces: resolved.tolgee_namespaces.clone(),
        },
        &output_path,
        &mut target_codec,
        &resolved.target_langs,
        resolved.strict,
    )?;
    validate_unique_language_identities(&target_codec, "Target catalog")?;

    let (jobs, summary) = build_jobs(
        &source_resource.resource,
        &target_codec,
        &resolved.target_langs,
        &resolved.statuses,
        target_supports_explicit_status(&target_path),
    )?;

    Ok(PreparedTranslation {
        opts: resolved,
        source_path,
        target_path,
        output_path,
        output_format,
        config_path: config.map(|cfg| cfg.path),
        source_resource,
        target_codec,
        tolgee_context,
        jobs,
        summary,
    })
}

fn print_preamble(prepared: &PreparedTranslation) {
    println!(
        "Translating {} -> {} using {}",
        prepared.source_resource.language,
        prepared.opts.target_langs.join(", "),
        translate_engine_label(&prepared.opts)
    );
    println!("Source: {}", prepared.source_path);
    println!("Target: {}", prepared.target_path);
    if let Some(config_path) = &prepared.config_path {
        println!("Config: {}", config_path.display());
    }
    if prepared.opts.dry_run {
        println!("Mode: dry-run");
    }
}

fn create_translate_reporter(
    prepared: &PreparedTranslation,
) -> Result<Box<dyn RunReporter>, String> {
    let init = DashboardInit {
        kind: DashboardKind::Translate,
        title: format!(
            "{} -> {}",
            prepared.source_resource.language,
            prepared.opts.target_langs.join(", ")
        ),
        metadata: translate_metadata_rows(prepared),
        summary_rows: translation_summary_rows(&prepared.summary),
        items: prepared.jobs.iter().map(translate_dashboard_item).collect(),
    };
    match prepared.opts.ui_mode {
        ResolvedUiMode::Plain => Ok(Box::new(PlainReporter::new(init))),
        ResolvedUiMode::Tui => Ok(Box::new(TuiReporter::new(init)?)),
    }
}

fn translate_metadata_rows(prepared: &PreparedTranslation) -> Vec<SummaryRow> {
    let mut rows = vec![
        SummaryRow::new("Provider", translate_engine_label(&prepared.opts)),
        SummaryRow::new("Source", prepared.source_path.clone()),
        SummaryRow::new("Target", prepared.target_path.clone()),
        SummaryRow::new("Output", prepared.output_path.clone()),
        SummaryRow::new("Concurrency", prepared.opts.concurrency.to_string()),
    ];
    if prepared.opts.dry_run {
        rows.push(SummaryRow::new("Mode", "dry-run"));
    }
    if let Some(config_path) = &prepared.config_path {
        rows.push(SummaryRow::new("Config", config_path.display().to_string()));
    }
    rows
}

fn translate_dashboard_item(job: &TranslationJob) -> DashboardItem {
    let mut item = DashboardItem::new(
        translation_job_id(job),
        job.key.clone(),
        job.target_lang.clone(),
        DashboardItemStatus::Queued,
    );
    item.source_text = Some(job.source_value.clone());
    item.note_text = job
        .existing_comment
        .clone()
        .or_else(|| job.source_comment.clone());
    item
}

fn translation_job_id(job: &TranslationJob) -> String {
    format!("{}:{}", job.target_lang, job.key)
}

fn translation_summary_rows(summary: &TranslationSummary) -> Vec<SummaryRow> {
    vec![
        SummaryRow::new("Total candidates", summary.total_entries.to_string()),
        SummaryRow::new("Queued", summary.queued.to_string()),
        SummaryRow::new("Translated", summary.translated.to_string()),
        SummaryRow::new("Skipped total", count_skipped(summary).to_string()),
        SummaryRow::new("Skipped plural", summary.skipped_plural.to_string()),
        SummaryRow::new(
            "Skipped do_not_translate",
            summary.skipped_do_not_translate.to_string(),
        ),
        SummaryRow::new("Skipped status", summary.skipped_status.to_string()),
        SummaryRow::new(
            "Skipped empty source",
            summary.skipped_empty_source.to_string(),
        ),
        SummaryRow::new("Failed", summary.failed.to_string()),
    ]
}

fn print_summary(summary: &TranslationSummary) {
    println!("Total candidate translations: {}", summary.total_entries);
    println!("Queued for translation: {}", summary.queued);
    println!("Translated: {}", summary.translated);
    println!("Skipped (plural): {}", summary.skipped_plural);
    println!(
        "Skipped (do_not_translate): {}",
        summary.skipped_do_not_translate
    );
    println!("Skipped (status): {}", summary.skipped_status);
    println!("Skipped (empty source): {}", summary.skipped_empty_source);
    println!("Failed: {}", summary.failed);
}

fn count_skipped(summary: &TranslationSummary) -> usize {
    summary.skipped_plural
        + summary.skipped_do_not_translate
        + summary.skipped_status
        + summary.skipped_empty_source
}

fn print_translation_results(
    prepared: &PreparedTranslation,
    results: &HashMap<(String, String), String>,
) {
    if results.is_empty() {
        return;
    }

    println!("Translation results:");
    for job in &prepared.jobs {
        if let Some(translated_value) = results.get(&(job.key.clone(), job.target_lang.clone())) {
            println!(
                "{}\t{}\t{} => {}",
                job.target_lang,
                job.key,
                format_inline_value(&job.source_value),
                format_inline_value(translated_value)
            );
        }
    }
}

fn format_inline_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn apply_translation_results(
    prepared: &mut PreparedTranslation,
    results: &HashMap<(String, String), String>,
) -> Result<(), String> {
    for job in &prepared.jobs {
        let Some(translated_value) = results.get(&(job.key.clone(), job.target_lang.clone()))
        else {
            continue;
        };

        if let Some(existing) = find_entry_mut_by_language_identity(
            &mut prepared.target_codec,
            &job.key,
            &job.target_lang,
        ) {
            existing.value = Translation::Singular(translated_value.clone());
            existing.status = prepared.opts.output_status.clone();
        } else {
            prepared
                .target_codec
                .add_entry(
                    &job.key,
                    &job.target_lang,
                    Translation::Singular(translated_value.clone()),
                    job.existing_comment
                        .clone()
                        .or_else(|| job.source_comment.clone()),
                    Some(prepared.opts.output_status.clone()),
                )
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn validate_generated_translation(
    job: &TranslationJob,
    translated_value: &str,
) -> Result<(), String> {
    if translated_value.trim().is_empty() {
        return Err(format!(
            "Translation for key '{}' and language '{}' was blank",
            job.key, job.target_lang
        ));
    }

    let source_signature = signature(&job.source_value);
    let translated_signature = signature(translated_value);
    if source_signature != translated_signature {
        return Err(format!(
            "Translation for key '{}' and language '{}' changed placeholders: source {:?}, translated {:?}",
            job.key, job.target_lang, source_signature, translated_signature
        ));
    }

    Ok(())
}

fn validate_translation_preflight(prepared: &PreparedTranslation) -> Result<(), String> {
    validate_output_serialization(
        &prepared.target_codec,
        &prepared.output_format,
        &prepared.output_path,
        single_output_language(&prepared.opts.target_langs),
    )
    .map_err(|e| format!("Preflight output validation failed: {}", e))
}

fn validate_output_serialization(
    codec: &Codec,
    output_format: &FormatType,
    output_path: &str,
    target_lang: Option<&str>,
) -> Result<(), String> {
    match output_format {
        FormatType::Strings(_) => {
            let target_lang = target_lang.ok_or_else(|| {
                "Single-language outputs require exactly one target language".to_string()
            })?;
            let resource = find_resource_by_language_identity(codec, target_lang)
                .ok_or_else(|| format!("Target language '{}' not found in output", target_lang))?;
            let format = StringsFormat::try_from(resource.clone())
                .map_err(|e| format!("Error building Strings output: {}", e))?;
            let mut out = Vec::new();
            format
                .to_writer(&mut out)
                .map_err(|e| format!("Error serializing Strings output: {}", e))
        }
        FormatType::AndroidStrings(_) => {
            let target_lang = target_lang.ok_or_else(|| {
                "Single-language outputs require exactly one target language".to_string()
            })?;
            let resource = find_resource_by_language_identity(codec, target_lang)
                .ok_or_else(|| format!("Target language '{}' not found in output", target_lang))?;
            let format = AndroidStringsFormat::from(resource.clone());
            let mut out = Vec::new();
            format
                .to_writer(&mut out)
                .map_err(|e| format!("Error serializing Android output: {}", e))
        }
        FormatType::Stringsdict(_) => Err(
            ".stringsdict is not supported by `translate` until plural-aware translation is implemented."
                .to_string(),
        ),
        FormatType::Xcstrings => {
            let format = XcstringsFormat::try_from(codec.resources.clone())
                .map_err(|e| format!("Error building Xcstrings output: {}", e))?;
            let mut out = Vec::new();
            format
                .to_writer(&mut out)
                .map_err(|e| format!("Error serializing Xcstrings output: {}", e))
        }
        FormatType::Xliff(_) => Err(
            "XLIFF output is not supported by `translate` in v1. Translate into .xcstrings, .strings, or strings.xml first."
                .to_string(),
        ),
        FormatType::CSV => {
            let format = CSVFormat::try_from(codec.resources.clone())
                .map_err(|e| format!("Error building CSV output: {}", e))?;
            let mut out = Vec::new();
            format
                .to_writer(&mut out)
                .map_err(|e| format!("Error serializing CSV output: {}", e))
        }
        FormatType::TSV => {
            let format = TSVFormat::try_from(codec.resources.clone())
                .map_err(|e| format!("Error building TSV output: {}", e))?;
            let mut out = Vec::new();
            format
                .to_writer(&mut out)
                .map_err(|e| format!("Error serializing TSV output: {}", e))
        }
    }
    .map_err(|err| format!("{} ({})", err, output_path))
}

fn build_jobs(
    source: &Resource,
    target_codec: &Codec,
    target_langs: &[String],
    statuses: &[EntryStatus],
    explicit_target_status: bool,
) -> Result<(Vec<TranslationJob>, TranslationSummary), String> {
    let mut jobs = Vec::new();
    let mut summary = TranslationSummary {
        total_entries: source.entries.len() * target_langs.len(),
        ..TranslationSummary::default()
    };

    for target_lang in target_langs {
        for entry in &source.entries {
            if entry.status == EntryStatus::DoNotTranslate {
                summary.skipped_do_not_translate += 1;
                continue;
            }

            let source_text = match &entry.value {
                Translation::Plural(_) => {
                    summary.skipped_plural += 1;
                    continue;
                }
                Translation::Empty => {
                    summary.skipped_empty_source += 1;
                    continue;
                }
                Translation::Singular(text) if text.trim().is_empty() => {
                    summary.skipped_empty_source += 1;
                    continue;
                }
                Translation::Singular(text) => text,
            };

            let target_entry =
                find_entry_by_language_identity(target_codec, &entry.id, target_lang);

            if target_entry.is_some_and(|item| item.status == EntryStatus::DoNotTranslate) {
                summary.skipped_do_not_translate += 1;
                continue;
            }

            let effective_status = target_entry
                .map(|item| effective_target_status(item, explicit_target_status))
                .unwrap_or(EntryStatus::New);

            if !statuses.contains(&effective_status) {
                summary.skipped_status += 1;
                continue;
            }

            jobs.push(TranslationJob {
                key: entry.id.clone(),
                source_lang: source.metadata.language.clone(),
                target_lang: target_lang.clone(),
                source_value: source_text.clone(),
                source_comment: entry.comment.clone(),
                existing_comment: target_entry.and_then(|item| item.comment.clone()),
            });
            summary.queued += 1;
        }
    }

    Ok((jobs, summary))
}

fn effective_target_status(entry: &Entry, explicit_target_status: bool) -> EntryStatus {
    if explicit_target_status {
        return entry.status.clone();
    }

    match &entry.value {
        Translation::Empty => EntryStatus::New,
        Translation::Singular(text) if text.trim().is_empty() => EntryStatus::New,
        _ => EntryStatus::Translated,
    }
}

fn ensure_target_resource(codec: &mut Codec, language: &str) -> Result<(), String> {
    if find_resource_by_language_identity(codec, language).is_none() {
        codec.add_resource(Resource {
            metadata: Metadata {
                language: language.to_string(),
                domain: String::new(),
                custom: HashMap::new(),
            },
            entries: Vec::new(),
        });
    }
    Ok(())
}

fn ensure_resource_exists(
    codec: &mut Codec,
    resource: &Resource,
    language: &str,
    clone_entries: bool,
) {
    if find_resource_by_language_identity(codec, language).is_some() {
        return;
    }

    codec.add_resource(Resource {
        metadata: resource.metadata.clone(),
        entries: if clone_entries {
            resource.entries.clone()
        } else {
            Vec::new()
        },
    });
}

fn propagate_xcstrings_metadata(codec: &mut Codec, source_resource: &Resource) {
    let source_language = source_resource
        .metadata
        .custom
        .get("source_language")
        .cloned()
        .unwrap_or_else(|| source_resource.metadata.language.clone());
    let version = source_resource
        .metadata
        .custom
        .get("version")
        .cloned()
        .unwrap_or_else(|| "1.0".to_string());

    for resource in &mut codec.resources {
        resource
            .metadata
            .custom
            .entry("source_language".to_string())
            .or_insert_with(|| source_language.to_string());
        resource
            .metadata
            .custom
            .entry("version".to_string())
            .or_insert_with(|| version.clone());
    }
}

fn validate_path_inputs(opts: &ResolvedOptions) -> Result<(), String> {
    if !Path::new(&opts.source).is_file() {
        return Err(format!("Source file does not exist: {}", opts.source));
    }

    if let Some(target) = &opts.target {
        if Path::new(target).exists() && !Path::new(target).is_file() {
            return Err(format!("Target path is not a file: {}", target));
        }
        validate_output_path(target)?;
    }

    if let Some(output) = &opts.output {
        validate_output_path(output)?;
    }

    Ok(())
}

fn resolve_options(
    opts: &TranslateOptions,
    config: Option<&LoadedConfig>,
) -> Result<ResolvedOptions, String> {
    let cfg = config.map(|item| &item.data.translate);
    let tolgee_cfg = config.map(|item| &item.data.tolgee);
    let config_dir = config.and_then(LoadedConfig::config_dir);
    let source_lang = opts
        .source_lang
        .clone()
        .or_else(|| cfg.and_then(|item| item.resolved_source_lang().map(str::to_string)));
    let target_langs = if !opts.target_langs.is_empty() {
        parse_language_list(opts.target_langs.iter().map(String::as_str))?
    } else {
        parse_language_list(
            cfg.and_then(|item| item.resolved_target_langs())
                .into_iter()
                .flatten()
                .flat_map(|value| value.split(',')),
        )?
    };
    if target_langs.is_empty() {
        return Err(
            "--target-lang is required (or set translate.output.lang/translate.target_lang in langcodec.toml)"
                .to_string(),
        );
    }
    if let Some(lang) = &source_lang {
        validate_language_code(lang)?;
    }

    let use_tolgee = opts.use_tolgee
        || opts.tolgee_config.is_some()
        || !opts.tolgee_namespaces.is_empty()
        || cfg.and_then(|item| item.use_tolgee).unwrap_or(false);

    let tolgee_config = opts.tolgee_config.clone().or_else(|| {
        tolgee_cfg
            .and_then(|item| item.config.as_deref())
            .map(|path| resolve_config_relative_path(config_dir, path))
    });
    let tolgee_namespaces = if !opts.tolgee_namespaces.is_empty() {
        opts.tolgee_namespaces.clone()
    } else {
        tolgee_cfg
            .and_then(|item| item.namespaces.clone())
            .unwrap_or_default()
    };

    let provider_resolution = resolve_provider(
        opts.provider.as_deref(),
        config.map(|item| &item.data),
        cfg.and_then(|item| item.provider.as_deref()),
    );
    let (provider, provider_error) = match provider_resolution {
        Ok(provider) => (Some(provider), None),
        Err(err) if use_tolgee => (None, Some(err)),
        Err(err) => return Err(err),
    };
    let (model, model_error) = if let Some(provider) = provider.as_ref() {
        match resolve_model(
            opts.model.as_deref(),
            config.map(|item| &item.data),
            provider,
            cfg.and_then(|item| item.model.as_deref()),
        ) {
            Ok(model) => (Some(model), None),
            Err(err) if use_tolgee => (None, Some(err)),
            Err(err) => return Err(err),
        }
    } else {
        (None, None)
    };

    let concurrency = opts
        .concurrency
        .or_else(|| cfg.and_then(|item| item.concurrency))
        .unwrap_or(DEFAULT_CONCURRENCY);
    if concurrency == 0 {
        return Err("Concurrency must be greater than zero".to_string());
    }

    let statuses = parse_status_filter(
        opts.status.as_deref(),
        cfg.and_then(|item| item.resolved_filter_status()),
    )?;
    let output_status = parse_output_status(cfg.and_then(|item| item.resolved_output_status()))?;
    let ui_mode = resolve_ui_mode_for_current_terminal(opts.ui_mode)?;

    Ok(ResolvedOptions {
        source: opts
            .source
            .clone()
            .ok_or_else(|| "--source is required".to_string())?,
        target: opts.target.clone(),
        output: opts.output.clone(),
        source_lang,
        target_langs,
        statuses,
        output_status,
        provider,
        model,
        provider_error,
        model_error,
        concurrency,
        use_tolgee,
        tolgee_config,
        tolgee_namespaces,
        dry_run: opts.dry_run,
        strict: opts.strict,
        ui_mode,
    })
}

fn parse_status_filter(
    cli: Option<&str>,
    cfg: Option<&Vec<String>>,
) -> Result<Vec<EntryStatus>, String> {
    let raw_values: Vec<String> = if let Some(cli) = cli {
        cli.split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    } else if let Some(cfg) = cfg {
        cfg.clone()
    } else {
        DEFAULT_STATUSES
            .iter()
            .map(|value| value.to_string())
            .collect()
    };

    let mut statuses = Vec::new();
    for raw in raw_values {
        let normalized = raw.replace(['-', ' '], "_");
        let parsed = normalized
            .parse::<EntryStatus>()
            .map_err(|e| format!("Invalid translate status '{}': {}", raw, e))?;
        if !statuses.contains(&parsed) {
            statuses.push(parsed);
        }
    }
    Ok(statuses)
}

fn parse_output_status(raw: Option<&str>) -> Result<EntryStatus, String> {
    let Some(raw) = raw else {
        return Ok(EntryStatus::NeedsReview);
    };

    let normalized = raw.trim().replace(['-', ' '], "_");
    let parsed = normalized
        .parse::<EntryStatus>()
        .map_err(|e| format!("Invalid translate output_status '{}': {}", raw, e))?;

    match parsed {
        EntryStatus::NeedsReview | EntryStatus::Translated => Ok(parsed),
        _ => Err(format!(
            "translate output status must be either 'needs_review' or 'translated', got '{}'",
            raw
        )),
    }
}

fn parse_language_list<'a, I>(values: I) -> Result<Vec<String>, String>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut parsed: Vec<String> = Vec::new();
    for raw in values {
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        validate_language_code(value)?;
        if !parsed
            .iter()
            .any(|existing| normalize_lang(existing) == normalize_lang(value))
        {
            parsed.push(value.to_string());
        }
    }
    Ok(parsed)
}

fn infer_path_language(path: &str, format: &FormatType) -> Result<Option<String>, String> {
    infer_language_from_path(path, format)
        .map_err(|err| format!("Failed to infer language from '{}': {}", path, err))
}

fn infer_intrinsic_language(path: &str, format: &FormatType) -> Result<Option<String>, String> {
    let language = match format {
        FormatType::Strings(_) => {
            StringsFormat::read_from(path)
                .map_err(|err| format!("Failed to inspect language in '{}': {}", path, err))?
                .language
        }
        _ => return Ok(None),
    };

    if language.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(language))
    }
}

fn format_requires_external_language(format: &FormatType) -> bool {
    matches!(
        format,
        FormatType::Strings(_) | FormatType::Stringsdict(_) | FormatType::AndroidStrings(_)
    )
}

fn resolve_existing_target_read_hint(
    target_path: &str,
    target_format: Option<&FormatType>,
    target_path_language: Option<&str>,
    output_path: &str,
    output_format: &FormatType,
    output_path_language: Option<&str>,
    requested_languages: &[String],
) -> Result<Option<String>, String> {
    if let Some(language) = target_path_language {
        return Ok(Some(language.to_string()));
    }

    if !target_format.is_some_and(format_requires_external_language) {
        return Ok(None);
    }

    let [requested_language] = requested_languages else {
        return Err(format!(
            "Existing single-language target '{}' has no locale identity and cannot be assigned unambiguously to requested target languages [{}]; use a locale-identifying target path or exactly one --target-lang",
            target_path,
            requested_languages.join(", ")
        ));
    };

    let separate_output_language = if Path::new(target_path) != Path::new(output_path)
        && format_requires_external_language(output_format)
    {
        output_path_language
    } else {
        None
    };

    if let Some(output_language) = separate_output_language {
        let compatible = language_identity_eq(requested_language, output_language)
            || (is_bare_language(requested_language)
                && primary_language(requested_language) == primary_language(output_language));
        if !compatible {
            return Err(format!(
                "Requested target language '{}' is incompatible with separate output path '{}' language '{}'; refusing to retag the identity-less target '{}'",
                requested_language, output_path, output_language, target_path
            ));
        }
        return Ok(Some(output_language.to_string()));
    }

    Ok(Some(requested_language.clone()))
}

fn read_codec(path: &str, language_hint: Option<String>, strict: bool) -> Result<Codec, String> {
    let mut codec = Codec::new();
    codec
        .read_file_by_extension_with_options(
            path,
            &ReadOptions::new()
                .with_language_hint(language_hint)
                .with_strict(strict),
        )
        .map_err(|e| format!("Failed to read '{}': {}", path, e))?;
    Ok(codec)
}

fn select_source_resource(
    codec: &Codec,
    requested_lang: &Option<String>,
) -> Result<SelectedResource, String> {
    if let Some(lang) = requested_lang {
        let resolved = resolve_language_tag(
            lang,
            codec
                .resources
                .iter()
                .map(|resource| resource.metadata.language.as_str()),
            "Source",
            "--source-lang",
        )?
        .ok_or_else(|| format!("Source language '{}' not found", lang))?;
        let resource = find_resource_by_language_identity(codec, &resolved)
            .cloned()
            .ok_or_else(|| format!("Source language '{}' not found", lang))?;
        return Ok(SelectedResource {
            language: resource.metadata.language.clone(),
            resource,
        });
    }

    if codec.resources.len() == 1 {
        let resource = codec.resources[0].clone();
        return Ok(SelectedResource {
            language: resource.metadata.language.clone(),
            resource,
        });
    }

    Err("Multiple source languages present; specify --source-lang".to_string())
}

fn resolve_target_languages(
    codec: &Codec,
    requested_langs: &[String],
    inferred_from_target: Option<&str>,
    inferred_from_output: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut resolved: Vec<String> = Vec::new();
    let available_languages = codec
        .resources
        .iter()
        .map(|resource| resource.metadata.language.as_str())
        .chain(inferred_from_output)
        .collect::<Vec<_>>();
    let target_binding_index = inferred_from_target.and_then(|target_language| {
        requested_langs
            .iter()
            .position(|requested| language_identity_eq(requested, target_language))
            .or_else(|| {
                requested_langs.iter().position(|requested| {
                    is_bare_language(requested)
                        && primary_language(requested) == primary_language(target_language)
                })
            })
    });

    for (requested_index, requested_lang) in requested_langs.iter().enumerate() {
        let canonical = if target_binding_index == Some(requested_index) {
            let target_language =
                inferred_from_target.expect("target binding requires an inferred target language");
            find_resource_by_language_identity(codec, target_language)
                .map(|resource| resource.metadata.language.clone())
                .unwrap_or_else(|| target_language.to_string())
        } else if let Some(target_language) = inferred_from_target {
            if target_binding_index.is_none()
                && !is_bare_language(requested_lang)
                && primary_language(requested_lang) == primary_language(target_language)
            {
                return Err(format!(
                    "Requested target language '{}' conflicts with explicit target path language '{}'; refusing to retag the target locale",
                    requested_lang, target_language
                ));
            } else {
                resolve_language_tag(
                    requested_lang,
                    available_languages.iter().copied(),
                    "Target",
                    "--target-lang",
                )?
                .unwrap_or_else(|| requested_lang.to_string())
            }
        } else {
            resolve_language_tag(
                requested_lang,
                available_languages.iter().copied(),
                "Target",
                "--target-lang",
            )?
            .unwrap_or_else(|| requested_lang.to_string())
        };

        if !resolved
            .iter()
            .any(|existing| language_identity_eq(existing, &canonical))
        {
            resolved.push(canonical);
        }
    }

    Ok(resolved)
}

fn is_bare_language(language: &str) -> bool {
    !normalize_lang(language).contains('-')
}

fn primary_language(language: &str) -> String {
    normalize_lang(language)
        .split('-')
        .next()
        .unwrap_or_default()
        .to_string()
}

fn resolve_language_tag<'a, I>(
    requested: &str,
    available: I,
    subject: &str,
    option: &str,
) -> Result<Option<String>, String>
where
    I: IntoIterator<Item = &'a str>,
{
    let available = available.into_iter().collect::<Vec<_>>();

    if let Some(exact) = available
        .iter()
        .copied()
        .find(|candidate| language_identity_eq(candidate, requested))
    {
        return Ok(Some(exact.to_string()));
    }

    let normalized_requested = normalize_lang(requested);
    if normalized_requested.contains('-') {
        return Ok(None);
    }

    let mut variants = available
        .iter()
        .copied()
        .filter(|candidate| {
            normalize_lang(candidate)
                .split('-')
                .next()
                .is_some_and(|language| language == normalized_requested.as_str())
        })
        .fold(Vec::<&str>::new(), |mut unique, candidate| {
            if !unique
                .iter()
                .any(|existing| language_identity_eq(existing, candidate))
            {
                unique.push(candidate);
            }
            unique
        });

    match variants.len() {
        0 => Ok(None),
        1 => Ok(Some(variants[0].to_string())),
        _ => {
            variants.sort_by_key(|candidate| normalize_lang(candidate));
            Err(format!(
                "{} language '{}' is ambiguous; matching variants: {}. Specify {} with a fully-qualified language tag",
                subject,
                requested,
                variants.join(", "),
                option
            ))
        }
    }
}

fn language_identity_eq(left: &str, right: &str) -> bool {
    normalize_lang(left) == normalize_lang(right)
}

fn validate_unique_language_identities(codec: &Codec, label: &str) -> Result<(), String> {
    let mut languages_by_identity = BTreeMap::<String, Vec<String>>::new();
    for resource in &codec.resources {
        languages_by_identity
            .entry(normalize_lang(&resource.metadata.language))
            .or_default()
            .push(resource.metadata.language.clone());
    }

    let duplicates = languages_by_identity
        .into_iter()
        .filter_map(|(identity, mut languages)| {
            if languages.len() < 2 {
                return None;
            }
            languages.sort();
            Some(format!("{} ({})", identity, languages.join(", ")))
        })
        .collect::<Vec<_>>();

    if duplicates.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} contains duplicate normalized language identities: {}",
            label,
            duplicates.join("; ")
        ))
    }
}

fn find_resource_by_language_identity<'a>(
    codec: &'a Codec,
    language: &str,
) -> Option<&'a Resource> {
    codec
        .resources
        .iter()
        .find(|resource| language_identity_eq(&resource.metadata.language, language))
}

fn find_entry_by_language_identity<'a>(
    codec: &'a Codec,
    key: &str,
    language: &str,
) -> Option<&'a Entry> {
    find_resource_by_language_identity(codec, language)
        .and_then(|resource| resource.find_entry(key))
}

fn find_entry_mut_by_language_identity<'a>(
    codec: &'a mut Codec,
    key: &str,
    language: &str,
) -> Option<&'a mut Entry> {
    codec
        .resources
        .iter_mut()
        .find(|resource| language_identity_eq(&resource.metadata.language, language))
        .and_then(|resource| resource.find_entry_mut(key))
}

fn normalize_lang(lang: &str) -> String {
    let normalized = lang.trim().replace('_', "-");
    normalized
        .parse::<unic_langid::LanguageIdentifier>()
        .map(|language| language.to_string().to_ascii_lowercase())
        .unwrap_or_else(|_| normalized.to_ascii_lowercase())
}

fn is_multi_language_format(format: &FormatType) -> bool {
    matches!(
        format,
        FormatType::Xcstrings | FormatType::CSV | FormatType::TSV
    )
}

fn target_supports_explicit_status(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("xcstrings"))
}

fn single_output_language(target_langs: &[String]) -> Option<&str> {
    if target_langs.len() == 1 {
        Some(target_langs[0].as_str())
    } else {
        None
    }
}

fn write_back(
    codec: &Codec,
    output_path: &str,
    output_format: &FormatType,
    target_lang: Option<&str>,
) -> Result<(), String> {
    match output_format {
        FormatType::Strings(_) | FormatType::AndroidStrings(_) => {
            let target_lang = target_lang.ok_or_else(|| {
                "Single-language outputs require exactly one target language".to_string()
            })?;
            let resource = find_resource_by_language_identity(codec, target_lang)
                .ok_or_else(|| format!("Target language '{}' not found in output", target_lang))?;
            Codec::write_resource_to_file(resource, output_path)
                .map_err(|e| format!("Error writing output: {}", e))
        }
        FormatType::Xcstrings | FormatType::CSV | FormatType::TSV => {
            convert_resources_to_format(codec.resources.clone(), output_path, output_format.clone())
                .map_err(|e| format!("Error writing output: {}", e))
        }
        FormatType::Xliff(_) => Err(
            "XLIFF output is not supported by `translate` in v1. Translate into .xcstrings, .strings, or strings.xml first."
                .to_string(),
        ),
        FormatType::Stringsdict(_) => Err(
            ".stringsdict is not supported by `translate` until plural-aware translation is implemented."
                .to_string(),
        ),
    }
}

fn create_mentra_backend(opts: &ResolvedOptions) -> Result<MentraBackend, String> {
    let provider = opts.provider.as_ref().ok_or_else(|| {
        opts.provider_error.clone().unwrap_or_else(|| {
            "--provider is required when Tolgee prefill does not satisfy all translations"
                .to_string()
        })
    })?;
    let model = opts.model.as_ref().ok_or_else(|| {
        opts.model_error.clone().unwrap_or_else(|| {
            "--model is required when Tolgee prefill does not satisfy all translations".to_string()
        })
    })?;
    let setup = build_provider(provider)?;
    if setup.provider_kind != *provider {
        return Err("Resolved provider mismatch".to_string());
    }
    Ok(MentraBackend {
        provider: setup.provider,
        model: model.clone(),
    })
}

fn translate_engine_label(opts: &ResolvedOptions) -> String {
    let ai_label = opts
        .provider
        .as_ref()
        .zip(opts.model.as_ref())
        .map(|(provider, model)| format!("{}:{}", provider.display_name(), model));

    match (opts.use_tolgee, ai_label) {
        (true, Some(ai_label)) => format!("tolgee + {}", ai_label),
        (true, None) => "tolgee".to_string(),
        (false, Some(ai_label)) => ai_label,
        (false, None) => "unconfigured".to_string(),
    }
}

fn build_prompt(request: &BackendRequest) -> String {
    let mut prompt = format!(
        "Translate the following localization value from {} to {}.\nKey: {}\nSource value:\n{}\n",
        request.source_lang, request.target_lang, request.key, request.source_value
    );
    if let Some(comment) = &request.source_comment {
        prompt.push_str("\nComment:\n");
        prompt.push_str(comment);
        prompt.push('\n');
    }
    prompt.push_str(
        "\nReturn JSON only in this exact shape: {\"translation\":\"...\"}. Do not wrap in markdown fences unless necessary.",
    );
    prompt
}

fn collect_text_blocks(response: &provider::Response) -> String {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn parse_translation_response(text: &str) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Model returned an empty translation".to_string());
    }

    if let Ok(payload) = serde_json::from_str::<ModelTranslationPayload>(trimmed) {
        return non_blank_model_translation(payload.translation);
    }

    if let Some(json_body) = extract_json_body(trimmed)
        && let Ok(payload) = serde_json::from_str::<ModelTranslationPayload>(&json_body)
    {
        return non_blank_model_translation(payload.translation);
    }

    Err(format!(
        "Model response was not valid translation JSON: {}",
        trimmed
    ))
}

fn non_blank_model_translation(translation: String) -> Result<String, String> {
    if translation.trim().is_empty() {
        return Err("Model returned a blank translation value".to_string());
    }
    Ok(translation)
}

fn extract_json_body(text: &str) -> Option<String> {
    let fenced = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .map(str::trim_start)?;
    let unfenced = fenced.strip_suffix("```")?.trim();
    Some(unfenced.to_string())
}

fn format_provider_error(err: ProviderError) -> String {
    format!("Provider request failed: {}", err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, fs, path::PathBuf, sync::Mutex};
    use tempfile::TempDir;

    type MockResponseKey = (String, String);
    type MockResponse = Result<String, String>;
    type MockResponseQueue = VecDeque<MockResponse>;
    type MockResponseMap = HashMap<MockResponseKey, MockResponseQueue>;
    type MockResponseSeed = ((&'static str, &'static str), MockResponse);

    #[derive(Clone)]
    struct MockBackend {
        responses: Arc<Mutex<MockResponseMap>>,
    }

    impl MockBackend {
        fn new(responses: Vec<MockResponseSeed>) -> Self {
            let mut mapped = HashMap::new();
            for ((key, target_lang), value) in responses {
                mapped
                    .entry((key.to_string(), target_lang.to_string()))
                    .or_insert_with(VecDeque::new)
                    .push_back(value);
            }
            Self {
                responses: Arc::new(Mutex::new(mapped)),
            }
        }
    }

    #[async_trait]
    impl TranslationBackend for MockBackend {
        async fn translate(&self, request: BackendRequest) -> Result<String, String> {
            self.responses
                .lock()
                .unwrap()
                .get_mut(&(request.key.clone(), request.target_lang.clone()))
                .and_then(|values| values.pop_front())
                .unwrap_or_else(|| Err("missing mock response".to_string()))
        }
    }

    fn base_options(source: &Path, target: Option<&Path>) -> TranslateOptions {
        TranslateOptions {
            source: Some(source.to_string_lossy().to_string()),
            target: target.map(|path| path.to_string_lossy().to_string()),
            output: None,
            source_lang: Some("en".to_string()),
            target_langs: vec!["fr".to_string()],
            status: None,
            provider: Some("openai".to_string()),
            model: Some("gpt-4.1-mini".to_string()),
            concurrency: Some(2),
            config: None,
            use_tolgee: false,
            tolgee_config: None,
            tolgee_namespaces: Vec::new(),
            dry_run: false,
            strict: false,
            ui_mode: UiMode::Plain,
        }
    }

    fn test_resource(language: &str, entries: Vec<Entry>) -> Resource {
        Resource {
            metadata: Metadata {
                language: language.to_string(),
                domain: String::new(),
                custom: HashMap::new(),
            },
            entries,
        }
    }

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    fn write_fake_tolgee(
        project_root: &Path,
        payload_path: &Path,
        capture_path: &Path,
        log_path: &Path,
    ) {
        let bin_dir = project_root.join("node_modules/.bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let script_path = bin_dir.join("tolgee");
        let script = format!(
            r#"#!/bin/sh
config=""
subcommand=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --config)
      config="$2"
      shift 2
      ;;
    pull|push)
      subcommand="$1"
      shift
      ;;
    *)
      shift
      ;;
  esac
done

echo "$subcommand|$config" >> "{log_path}"
cp "$config" "{capture_path}"

if [ "$subcommand" = "push" ]; then
  exit 0
fi

eval "$(
python3 - "$config" <<'PY'
import json
import shlex
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    data = json.load(fh)

pull_path = data.get("pull", {{}}).get("path", "")
namespaces = data.get("pull", {{}}).get("namespaces") or data.get("push", {{}}).get("namespaces") or []
if namespaces:
    namespace = namespaces[0]
else:
    files = data.get("push", {{}}).get("files") or []
    namespace = files[0]["namespace"] if files else ""

print(f"pull_path={{shlex.quote(pull_path)}}")
print(f"namespace={{shlex.quote(namespace)}}")
PY
)"
mkdir -p "$pull_path/$namespace"
cp "{payload_path}" "$pull_path/$namespace/Localizable.xcstrings"
"#,
            payload_path = payload_path.display(),
            capture_path = capture_path.display(),
            log_path = log_path.display(),
        );
        fs::write(&script_path, script).unwrap();
        #[cfg(unix)]
        make_executable(&script_path);
    }

    fn write_translate_tolgee_config(project_root: &Path) -> PathBuf {
        let config_path = project_root.join(".tolgeerc.json");
        fs::write(
            &config_path,
            r#"{
  "format": "APPLE_XCSTRINGS",
  "push": {
    "files": [
      {
        "path": "Localizable.xcstrings",
        "namespace": "Core"
      }
    ]
  },
  "pull": {
    "path": "./tolgee-temp",
    "fileStructureTemplate": "/{namespace}/Localizable.{extension}"
  }
}"#,
        )
        .unwrap();
        config_path
    }

    fn write_translate_source_catalog(path: &Path) {
        fs::write(
            path,
            r#"{
  "sourceLanguage" : "en",
  "version" : "1.0",
  "strings" : {
    "welcome" : {
      "localizations" : {
        "en" : {
          "stringUnit" : {
            "state" : "translated",
            "value" : "Welcome"
          }
        }
      }
    },
    "bye" : {
      "localizations" : {
        "en" : {
          "stringUnit" : {
            "state" : "translated",
            "value" : "Goodbye"
          }
        }
      }
    }
  }
}"#,
        )
        .unwrap();
    }

    fn write_translate_tolgee_payload(path: &Path) {
        fs::write(
            path,
            r#"{
  "sourceLanguage" : "en",
  "version" : "1.0",
  "strings" : {
    "welcome" : {
      "localizations" : {
        "fr" : {
          "stringUnit" : {
            "state" : "translated",
            "value" : "Bienvenue"
          }
        }
      }
    }
  }
}"#,
        )
        .unwrap();
    }

    #[test]
    fn translates_missing_entries_into_target_file() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("en.strings");
        let target = temp_dir.path().join("fr.strings");

        fs::write(
            &source,
            "\"welcome\" = \"Welcome\";\n\"bye\" = \"Goodbye\";\n",
        )
        .unwrap();

        let prepared = prepare_translation(&base_options(&source, Some(&target))).unwrap();
        let outcome = run_prepared_translation(
            prepared,
            Some(Arc::new(MockBackend::new(vec![
                (("welcome", "fr"), Ok("Bienvenue".to_string())),
                (("bye", "fr"), Ok("Au revoir".to_string())),
            ]))),
        )
        .unwrap();

        assert_eq!(outcome.translated, 2);
        let written = fs::read_to_string(&target).unwrap();
        assert!(written.contains("\"welcome\" = \"Bienvenue\";"));
        assert!(written.contains("\"bye\" = \"Au revoir\";"));
    }

    #[test]
    fn translates_strings_source_into_android_target_file() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("en.strings");
        let target_dir = temp_dir.path().join("values-fr");
        let target = target_dir.join("strings.xml");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(
            &source,
            "\"welcome\" = \"Welcome\";\n\"bye\" = \"Goodbye\";\n",
        )
        .unwrap();

        let prepared = prepare_translation(&base_options(&source, Some(&target))).unwrap();
        let outcome = run_prepared_translation(
            prepared,
            Some(Arc::new(MockBackend::new(vec![
                (("welcome", "fr"), Ok("Bienvenue".to_string())),
                (("bye", "fr"), Ok("Au revoir".to_string())),
            ]))),
        )
        .unwrap();

        assert_eq!(outcome.translated, 2);
        let written = fs::read_to_string(&target).unwrap();
        assert!(written.contains("<string name=\"welcome\">Bienvenue</string>"));
        assert!(written.contains("<string name=\"bye\">Au revoir</string>"));
    }

    #[test]
    fn translates_android_source_into_strings_target_file() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("values");
        let source = source_dir.join("strings.xml");
        let target = temp_dir.path().join("fr.strings");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            &source,
            r#"<resources>
<string name="welcome">Welcome</string>
<string name="bye">Goodbye</string>
</resources>
"#,
        )
        .unwrap();

        let prepared = prepare_translation(&base_options(&source, Some(&target))).unwrap();
        let outcome = run_prepared_translation(
            prepared,
            Some(Arc::new(MockBackend::new(vec![
                (("welcome", "fr"), Ok("Bienvenue".to_string())),
                (("bye", "fr"), Ok("Au revoir".to_string())),
            ]))),
        )
        .unwrap();

        assert_eq!(outcome.translated, 2);
        let written = fs::read_to_string(&target).unwrap();
        assert!(written.contains("\"welcome\" = \"Bienvenue\";"));
        assert!(written.contains("\"bye\" = \"Au revoir\";"));
    }

    #[test]
    fn dry_run_does_not_write_target() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("en.strings");
        let target = temp_dir.path().join("fr.strings");

        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();
        fs::write(&target, "\"welcome\" = \"\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.dry_run = true;

        let before = fs::read_to_string(&target).unwrap();
        let prepared = prepare_translation(&options).unwrap();
        let outcome = run_prepared_translation(
            prepared,
            Some(Arc::new(MockBackend::new(vec![(
                ("welcome", "fr"),
                Ok("Bienvenue".to_string()),
            )]))),
        )
        .unwrap();
        let after = fs::read_to_string(&target).unwrap();

        assert_eq!(outcome.translated, 1);
        assert_eq!(before, after);
    }

    #[test]
    fn fails_without_writing_when_any_translation_fails() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("en.strings");
        let target = temp_dir.path().join("fr.strings");

        fs::write(
            &source,
            "\"welcome\" = \"Welcome\";\n\"bye\" = \"Goodbye\";\n",
        )
        .unwrap();
        fs::write(&target, "\"welcome\" = \"\";\n\"bye\" = \"\";\n").unwrap();
        let before = fs::read_to_string(&target).unwrap();

        let prepared = prepare_translation(&base_options(&source, Some(&target))).unwrap();
        let err = run_prepared_translation(
            prepared,
            Some(Arc::new(MockBackend::new(vec![
                (("welcome", "fr"), Ok("Bienvenue".to_string())),
                (("bye", "fr"), Err("boom".to_string())),
            ]))),
        )
        .unwrap_err();

        assert!(err.contains("no files were written"));
        let after = fs::read_to_string(&target).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn default_mode_rejects_missing_and_extra_placeholders_without_writing() {
        for (case, translated_value) in [("missing", "Bienvenue"), ("extra", "Bienvenue %1$s %2$d")]
        {
            let temp_dir = TempDir::new().unwrap();
            let source = temp_dir.path().join("en.strings");
            let target = temp_dir.path().join("fr.strings");
            fs::write(&source, "\"welcome\" = \"Welcome %1$@\";\n").unwrap();

            let options = base_options(&source, Some(&target));
            assert!(!options.strict);
            let prepared = prepare_translation(&options).unwrap();
            let err = run_prepared_translation(
                prepared,
                Some(Arc::new(MockBackend::new(vec![(
                    ("welcome", "fr"),
                    Ok(translated_value.to_string()),
                )]))),
            )
            .unwrap_err();

            assert!(
                err.contains("no files were written"),
                "{case}: unexpected error: {err}"
            );
            assert!(!target.exists(), "{case}: target must not be created");
        }
    }

    #[test]
    fn mock_backend_cannot_bypass_blank_translation_validation() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("en.strings");
        let target = temp_dir.path().join("fr.strings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();

        let prepared = prepare_translation(&base_options(&source, Some(&target))).unwrap();
        let err = run_prepared_translation(
            prepared,
            Some(Arc::new(MockBackend::new(vec![(
                ("welcome", "fr"),
                Ok(" \n\t ".to_string()),
            )]))),
        )
        .unwrap_err();

        assert!(err.contains("no files were written"));
        assert!(!target.exists());
    }

    #[test]
    fn valid_fully_positional_placeholder_reordering_succeeds() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("en.strings");
        let target = temp_dir.path().join("fr.strings");
        fs::write(
            &source,
            "\"message\" = \"Hello %1$@, you have %2$d items\";\n",
        )
        .unwrap();

        let prepared = prepare_translation(&base_options(&source, Some(&target))).unwrap();
        let outcome = run_prepared_translation(
            prepared,
            Some(Arc::new(MockBackend::new(vec![(
                ("message", "fr"),
                Ok("Vous avez %2$d elements, %1$s".to_string()),
            )]))),
        )
        .unwrap();

        assert_eq!(outcome.translated, 1);
        let written = fs::read_to_string(target).unwrap();
        assert!(written.contains("\"message\" = \"Vous avez %2$d elements, %1$@\";"));
    }

    #[test]
    fn strict_mode_ignores_unrelated_preexisting_placeholder_mismatch() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("en.strings");
        let target = temp_dir.path().join("fr.strings");
        fs::write(
            &source,
            "\"welcome\" = \"Welcome %1$@\";\n\"legacy\" = \"Legacy %@\";\n",
        )
        .unwrap();
        fs::write(&target, "\"legacy\" = \"Heritage %d\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.strict = true;
        let prepared = prepare_translation(&options).unwrap();
        assert_eq!(prepared.jobs.len(), 1);
        assert_eq!(prepared.jobs[0].key, "welcome");

        let outcome = run_prepared_translation(
            prepared,
            Some(Arc::new(MockBackend::new(vec![(
                ("welcome", "fr"),
                Ok("Bienvenue %1$s".to_string()),
            )]))),
        )
        .unwrap();

        assert_eq!(outcome.translated, 1);
        let written = fs::read_to_string(target).unwrap();
        assert!(written.contains("\"welcome\" = \"Bienvenue %1$@\";"));
        assert!(written.contains("\"legacy\" = \"Heritage %d\";"));
    }

    #[test]
    fn uses_config_defaults_when_flags_are_missing() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source.csv");
        let config = temp_dir.path().join("langcodec.toml");
        fs::write(&source, "key,en,fr\nwelcome,Welcome,\n").unwrap();
        fs::write(
            &config,
            r#"[openai]
model = "gpt-5.4"

[translate]
source_lang = "en"
target_lang = ["fr"]
concurrency = 2
status = ["new", "stale"]
"#,
        )
        .unwrap();

        let options = TranslateOptions {
            source: Some(source.to_string_lossy().to_string()),
            target: None,
            output: None,
            source_lang: None,
            target_langs: Vec::new(),
            status: None,
            provider: None,
            model: None,
            concurrency: None,
            config: Some(config.to_string_lossy().to_string()),
            use_tolgee: false,
            tolgee_config: None,
            tolgee_namespaces: Vec::new(),
            dry_run: true,
            strict: false,
            ui_mode: UiMode::Plain,
        };

        let prepared = prepare_translation(&options).unwrap();
        assert_eq!(prepared.opts.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(prepared.opts.target_langs, vec!["fr".to_string()]);
        assert_eq!(prepared.summary.queued, 1);
    }

    #[test]
    fn uses_array_target_langs_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source.csv");
        let config = temp_dir.path().join("langcodec.toml");
        fs::write(&source, "key,en,fr,de\nwelcome,Welcome,,\n").unwrap();
        fs::write(
            &config,
            r#"[openai]
model = "gpt-5.4"

[translate.input]
lang = "en"

[translate.output]
lang = ["fr", "de"]
"#,
        )
        .unwrap();

        let options = TranslateOptions {
            source: Some(source.to_string_lossy().to_string()),
            target: None,
            output: None,
            source_lang: None,
            target_langs: Vec::new(),
            status: None,
            provider: None,
            model: None,
            concurrency: None,
            config: Some(config.to_string_lossy().to_string()),
            use_tolgee: false,
            tolgee_config: None,
            tolgee_namespaces: Vec::new(),
            dry_run: true,
            strict: false,
            ui_mode: UiMode::Plain,
        };

        let prepared = prepare_translation(&options).unwrap();
        assert_eq!(
            prepared.opts.target_langs,
            vec!["fr".to_string(), "de".to_string()]
        );
        assert_eq!(prepared.summary.queued, 2);
    }

    #[test]
    fn uses_translated_output_status_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("Localizable.xcstrings");
        let config = temp_dir.path().join("langcodec.toml");
        fs::write(
            &source,
            r#"{
  "sourceLanguage" : "en",
  "version" : "1.0",
  "strings" : {
    "welcome" : {
      "localizations" : {
        "en" : {
          "stringUnit" : {
            "state" : "new",
            "value" : "Welcome"
          }
        }
      }
    }
  }
}"#,
        )
        .unwrap();
        fs::write(
            &config,
            r#"[openai]
model = "gpt-5.4"

[translate.input]
source = "Localizable.xcstrings"
lang = "en"

[translate.output]
lang = ["fr"]
status = "translated"
"#,
        )
        .unwrap();

        let options = TranslateOptions {
            source: None,
            target: None,
            output: None,
            source_lang: None,
            target_langs: Vec::new(),
            status: None,
            provider: None,
            model: None,
            concurrency: None,
            config: Some(config.to_string_lossy().to_string()),
            use_tolgee: false,
            tolgee_config: None,
            tolgee_namespaces: Vec::new(),
            dry_run: false,
            strict: false,
            ui_mode: UiMode::Plain,
        };

        let runs = expand_translate_invocations(&options).unwrap();
        let prepared = prepare_translation(&runs[0]).unwrap();
        let output_path = prepared.output_path.clone();
        run_prepared_translation(
            prepared,
            Some(Arc::new(MockBackend::new(vec![(
                ("welcome", "fr"),
                Ok("Bienvenue".to_string()),
            )]))),
        )
        .unwrap();

        let written = fs::read_to_string(output_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&written).unwrap();
        assert_eq!(
            parsed["strings"]["welcome"]["localizations"]["fr"]["stringUnit"]["state"],
            "translated"
        );
    }

    #[test]
    fn rejects_invalid_output_status_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source.csv");
        let config = temp_dir.path().join("langcodec.toml");
        fs::write(&source, "key,en,fr\nwelcome,Welcome,\n").unwrap();
        fs::write(
            &config,
            r#"[openai]
model = "gpt-5.4"

[translate.input]
lang = "en"

[translate.output]
lang = ["fr"]
status = "new"
"#,
        )
        .unwrap();

        let options = TranslateOptions {
            source: Some(source.to_string_lossy().to_string()),
            target: None,
            output: None,
            source_lang: None,
            target_langs: Vec::new(),
            status: None,
            provider: None,
            model: None,
            concurrency: None,
            config: Some(config.to_string_lossy().to_string()),
            use_tolgee: false,
            tolgee_config: None,
            tolgee_namespaces: Vec::new(),
            dry_run: true,
            strict: false,
            ui_mode: UiMode::Plain,
        };

        let err = prepare_translation(&options).unwrap_err();
        assert!(err.contains("translate output status must be either"));
    }

    #[test]
    fn expands_single_source_from_config_relative_to_config_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join("project");
        fs::create_dir_all(config_dir.join("locales")).unwrap();
        fs::create_dir_all(config_dir.join("output")).unwrap();
        let config = config_dir.join("langcodec.toml");
        fs::write(
            &config,
            r#"[translate]
source = "locales/Localizable.xcstrings"
target = "output/Translated.xcstrings"
"#,
        )
        .unwrap();

        let runs = expand_translate_invocations(&TranslateOptions {
            source: None,
            target: None,
            output: None,
            source_lang: None,
            target_langs: Vec::new(),
            status: None,
            provider: None,
            model: None,
            concurrency: None,
            config: Some(config.to_string_lossy().to_string()),
            use_tolgee: false,
            tolgee_config: None,
            tolgee_namespaces: Vec::new(),
            dry_run: true,
            strict: false,
            ui_mode: UiMode::Plain,
        })
        .unwrap();

        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs[0].source,
            Some(
                config_dir
                    .join("locales/Localizable.xcstrings")
                    .to_string_lossy()
                    .to_string()
            )
        );
        assert_eq!(
            runs[0].target,
            Some(
                config_dir
                    .join("output/Translated.xcstrings")
                    .to_string_lossy()
                    .to_string()
            )
        );
    }

    #[test]
    fn expands_multiple_sources_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join("project");
        fs::create_dir_all(&config_dir).unwrap();
        let config = config_dir.join("langcodec.toml");
        fs::write(
            &config,
            r#"[translate]
sources = ["one.xcstrings", "two.xcstrings"]
"#,
        )
        .unwrap();

        let runs = expand_translate_invocations(&TranslateOptions {
            source: None,
            target: None,
            output: None,
            source_lang: None,
            target_langs: Vec::new(),
            status: None,
            provider: None,
            model: None,
            concurrency: None,
            config: Some(config.to_string_lossy().to_string()),
            use_tolgee: false,
            tolgee_config: None,
            tolgee_namespaces: Vec::new(),
            dry_run: true,
            strict: false,
            ui_mode: UiMode::Plain,
        })
        .unwrap();

        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs[0].source,
            Some(
                config_dir
                    .join("one.xcstrings")
                    .to_string_lossy()
                    .to_string()
            )
        );
        assert_eq!(
            runs[1].source,
            Some(
                config_dir
                    .join("two.xcstrings")
                    .to_string_lossy()
                    .to_string()
            )
        );
    }

    #[test]
    fn expands_globbed_sources_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join("project");
        let feature_a = config_dir.join("Modules").join("FeatureA");
        let feature_b = config_dir.join("Modules").join("FeatureB");
        fs::create_dir_all(&feature_a).unwrap();
        fs::create_dir_all(&feature_b).unwrap();

        let first = feature_a.join("Localizable.xcstrings");
        let second = feature_b.join("Localizable.xcstrings");
        fs::write(
            &first,
            r#"{"sourceLanguage":"en","version":"1.0","strings":{}}"#,
        )
        .unwrap();
        fs::write(
            &second,
            r#"{"sourceLanguage":"en","version":"1.0","strings":{}}"#,
        )
        .unwrap();

        let config = config_dir.join("langcodec.toml");
        fs::write(
            &config,
            r#"[translate.input]
sources = ["Modules/*/Localizable.xcstrings"]
"#,
        )
        .unwrap();

        let runs = expand_translate_invocations(&TranslateOptions {
            source: None,
            target: None,
            output: None,
            source_lang: None,
            target_langs: Vec::new(),
            status: None,
            provider: None,
            model: None,
            concurrency: None,
            config: Some(config.to_string_lossy().to_string()),
            use_tolgee: false,
            tolgee_config: None,
            tolgee_namespaces: Vec::new(),
            dry_run: true,
            strict: false,
            ui_mode: UiMode::Plain,
        })
        .unwrap();

        let mut sources = runs
            .into_iter()
            .map(|run| run.source.expect("source"))
            .collect::<Vec<_>>();
        sources.sort();

        let mut expected = vec![
            first.to_string_lossy().to_string(),
            second.to_string_lossy().to_string(),
        ];
        expected.sort();

        assert_eq!(sources, expected);
    }

    #[test]
    fn rejects_target_with_multiple_sources_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config = temp_dir.path().join("langcodec.toml");
        fs::write(
            &config,
            r#"[translate]
sources = ["one.xcstrings", "two.xcstrings"]
target = "translated.xcstrings"
"#,
        )
        .unwrap();

        let err = expand_translate_invocations(&TranslateOptions {
            source: None,
            target: None,
            output: None,
            source_lang: None,
            target_langs: Vec::new(),
            status: None,
            provider: None,
            model: None,
            concurrency: None,
            config: Some(config.to_string_lossy().to_string()),
            use_tolgee: false,
            tolgee_config: None,
            tolgee_namespaces: Vec::new(),
            dry_run: true,
            strict: false,
            ui_mode: UiMode::Plain,
        })
        .unwrap_err();

        assert!(err.contains("translate.input.sources/translate.sources cannot be combined"));
    }

    #[test]
    fn skips_plural_entries() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("Localizable.xcstrings");
        let target = temp_dir.path().join("translated.xcstrings");
        fs::write(
            &source,
            r#"{
  "sourceLanguage" : "en",
  "version" : "1.0",
  "strings" : {
    "welcome" : {
      "localizations" : {
        "en" : {
          "stringUnit" : {
            "state" : "new",
            "value" : "Welcome"
          }
        }
      }
    },
    "item_count" : {
      "localizations" : {
        "en" : {
          "variations" : {
            "plural" : {
              "one" : {
                "stringUnit" : {
                  "state" : "new",
                  "value" : "%#@items@"
                }
              },
              "other" : {
                "stringUnit" : {
                  "state" : "new",
                  "value" : "%#@items@"
                }
              }
            }
          }
        }
      }
    }
  }
}"#,
        )
        .unwrap();

        let prepared = prepare_translation(&base_options(&source, Some(&target))).unwrap();
        assert_eq!(prepared.summary.skipped_plural, 1);
        assert_eq!(prepared.summary.queued, 1);
    }

    #[test]
    fn rejects_in_place_single_language_translation_without_target() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("en.strings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();

        let options = base_options(&source, None);
        let err = prepare_translation(&options).unwrap_err();
        assert!(err.contains("Omitting --target is only supported"));
    }

    #[test]
    fn bare_target_language_resolves_sole_variant() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("translations.csv");
        let target = temp_dir.path().join("target.csv");
        fs::write(&source, "key,en\nwelcome,Welcome\n").unwrap();
        fs::write(&target, "key,fr-CA\nwelcome,\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.target_langs = vec!["fr".to_string()];
        options.source_lang = Some("en".to_string());

        let prepared = prepare_translation(&options).unwrap();
        assert_eq!(prepared.opts.target_langs, vec!["fr-CA".to_string()]);
        assert_eq!(prepared.summary.queued, 1);
    }

    #[test]
    fn exact_target_language_wins_over_same_base_variant() {
        let mut codec = Codec::new();
        codec.add_resource(test_resource("pt-BR", Vec::new()));
        codec.add_resource(test_resource("pt-PT", Vec::new()));

        let resolved =
            resolve_target_languages(&codec, &["pt-PT".to_string()], None, None).unwrap();

        assert_eq!(resolved, vec!["pt-PT".to_string()]);
    }

    #[test]
    fn bare_target_language_reports_ambiguous_variants() {
        let mut codec = Codec::new();
        codec.add_resource(test_resource("fr-FR", Vec::new()));
        codec.add_resource(test_resource("fr-CA", Vec::new()));

        let err = resolve_target_languages(&codec, &["fr".to_string()], None, None).unwrap_err();

        assert!(err.contains("Target language 'fr' is ambiguous"));
        assert!(err.contains("fr-FR"));
        assert!(err.contains("fr-CA"));
        assert!(err.contains("--target-lang"));
    }

    #[test]
    fn bare_source_language_reports_ambiguous_variants() {
        let mut codec = Codec::new();
        codec.add_resource(test_resource("fr-FR", Vec::new()));
        codec.add_resource(test_resource("fr-CA", Vec::new()));

        let err = select_source_resource(&codec, &Some("fr".to_string())).unwrap_err();

        assert!(err.contains("Source language 'fr' is ambiguous"));
        assert!(err.contains("fr-FR"));
        assert!(err.contains("fr-CA"));
        assert!(err.contains("--source-lang"));
    }

    #[test]
    fn fully_qualified_source_does_not_bind_different_variant() {
        let mut codec = Codec::new();
        codec.add_resource(test_resource("pt-BR", Vec::new()));

        let err = select_source_resource(&codec, &Some("pt-PT".to_string())).unwrap_err();

        assert_eq!(err, "Source language 'pt-PT' not found");
    }

    #[test]
    fn source_lang_does_not_retag_path_qualified_source_or_write_target() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("pt-BR.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let target = temp_dir.path().join("fr.strings");
        fs::write(&source, "\"welcome\" = \"Bem-vindo\";\n").unwrap();
        fs::write(&target, "\"existing\" = \"Conserver\";\n").unwrap();
        let target_before = fs::read_to_string(&target).unwrap();

        let mut options = base_options(&source, Some(&target));
        options.source_lang = Some("pt-PT".to_string());

        let err = prepare_translation(&options).unwrap_err();

        assert_eq!(err, "Source language 'pt-PT' not found");
        assert_eq!(fs::read_to_string(target).unwrap(), target_before);
    }

    #[test]
    fn bare_source_lang_preserves_path_qualified_source_identity() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("fr-CA.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let target = temp_dir.path().join("de.strings");
        fs::write(&source, "\"welcome\" = \"Bienvenue\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.source_lang = Some("fr".to_string());
        options.target_langs = vec!["de".to_string()];

        let prepared = prepare_translation(&options).unwrap();

        assert_eq!(prepared.source_resource.language, "fr-CA");
        assert_eq!(prepared.jobs[0].source_lang, "fr-CA");
    }

    #[test]
    fn identityless_single_language_source_uses_source_lang_as_read_hint() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("Localizable.strings");
        let target = temp_dir.path().join("fr.strings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.strict = true;

        let prepared = prepare_translation(&options).unwrap();

        assert_eq!(prepared.source_resource.language, "en");
        assert_eq!(prepared.summary.queued, 1);
    }

    #[test]
    fn standalone_strings_header_identity_is_preserved_in_strict_mode() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("Catalog.strings");
        let target = temp_dir.path().join("de.strings");
        fs::write(
            &source,
            "//: Language: fr-CA\n\"welcome\" = \"Bienvenue\";\n",
        )
        .unwrap();

        let mut options = base_options(&source, Some(&target));
        options.source_lang = Some("fr".to_string());
        options.target_langs = vec!["de".to_string()];
        options.strict = true;

        let prepared = prepare_translation(&options).unwrap();

        assert_eq!(prepared.source_resource.language, "fr-CA");
        assert_eq!(prepared.jobs[0].source_lang, "fr-CA");
    }

    #[test]
    fn source_lang_selects_without_retagging_intrinsic_xcstrings_locale() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("Catalog.xcstrings");
        let target = temp_dir.path().join("de.strings");
        fs::write(
            &source,
            r#"{
  "sourceLanguage" : "fr-CA",
  "version" : "1.0",
  "strings" : {
    "welcome" : {
      "localizations" : {
        "fr-CA" : {
          "stringUnit" : {
            "state" : "translated",
            "value" : "Bienvenue"
          }
        }
      }
    }
  }
}"#,
        )
        .unwrap();

        let mut options = base_options(&source, Some(&target));
        options.source_lang = Some("fr".to_string());
        options.target_langs = vec!["de".to_string()];

        let prepared = prepare_translation(&options).unwrap();

        assert_eq!(prepared.source_resource.language, "fr-CA");
        assert_eq!(prepared.jobs[0].source_lang, "fr-CA");
    }

    #[test]
    fn script_variants_are_distinct_source_and_target_languages() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("translations.csv");
        fs::write(&source, "key,zh-Hans,zh-Hant\nwelcome,欢迎,\n").unwrap();

        let mut options = base_options(&source, None);
        options.source_lang = Some("zh-Hans".to_string());
        options.target_langs = vec!["zh-Hant".to_string()];

        let prepared = prepare_translation(&options).unwrap();

        assert_eq!(prepared.source_resource.language, "zh-Hans");
        assert_eq!(prepared.opts.target_langs, vec!["zh-Hant".to_string()]);
        assert_eq!(prepared.summary.queued, 1);
    }

    #[test]
    fn existing_target_lookup_uses_normalized_full_tag_identity() {
        let source = test_resource(
            "en",
            vec![Entry {
                id: "welcome".to_string(),
                value: Translation::Singular("Welcome".to_string()),
                comment: None,
                status: EntryStatus::Translated,
                custom: HashMap::new(),
            }],
        );
        let mut target_codec = Codec::new();
        target_codec.add_resource(test_resource(
            "fr_CA",
            vec![Entry {
                id: "welcome".to_string(),
                value: Translation::Singular("Bienvenue".to_string()),
                comment: None,
                status: EntryStatus::Translated,
                custom: HashMap::new(),
            }],
        ));

        let (jobs, summary) = build_jobs(
            &source,
            &target_codec,
            &["FR-ca".to_string()],
            &[EntryStatus::New, EntryStatus::Stale],
            false,
        )
        .unwrap();

        assert!(jobs.is_empty());
        assert_eq!(summary.skipped_status, 1);
    }

    #[test]
    fn prepare_rejects_duplicate_normalized_target_identities() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source.csv");
        let target = temp_dir.path().join("target.csv");
        fs::write(&source, "key,en\nwelcome,Welcome\n").unwrap();
        fs::write(&target, "key,fr_CA,fr-CA\nwelcome,,\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.target_langs = vec!["fr-CA".to_string()];

        let err = prepare_translation(&options).unwrap_err();

        assert_eq!(
            err,
            "Target catalog contains duplicate normalized language identities: fr-ca (fr-CA, fr_CA)"
        );
    }

    #[test]
    fn prepare_rejects_duplicate_normalized_source_identities() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source.csv");
        fs::write(&source, "key,fr_CA,fr-CA\nwelcome,Bienvenue,Salut\n").unwrap();

        let mut options = base_options(&source, None);
        options.source_lang = Some("fr-CA".to_string());
        options.target_langs = vec!["de".to_string()];

        let err = prepare_translation(&options).unwrap_err();

        assert_eq!(
            err,
            "Source catalog contains duplicate normalized language identities: fr-ca (fr-CA, fr_CA)"
        );
    }

    #[test]
    fn output_only_single_language_starts_with_empty_target() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("en.lproj");
        let output_dir = temp_dir.path().join("fr.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let output = output_dir.join("Localizable.strings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();

        let mut options = base_options(&source, None);
        options.output = Some(output.to_string_lossy().to_string());

        let prepared = prepare_translation(&options).unwrap();

        assert_eq!(prepared.target_path, output.to_string_lossy().to_string());
        assert_eq!(prepared.opts.target_langs, vec!["fr".to_string()]);
        assert_eq!(prepared.summary.queued, 1);
        assert!(prepared.target_codec.find_entry("welcome", "fr").is_none());
    }

    #[test]
    fn output_only_multilanguage_preserves_source_resource() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("en.strings");
        let output = temp_dir.path().join("translations.csv");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();

        let mut options = base_options(&source, None);
        options.output = Some(output.to_string_lossy().to_string());

        let prepared = prepare_translation(&options).unwrap();

        assert_eq!(prepared.summary.queued, 1);
        assert_eq!(
            prepared
                .target_codec
                .find_entry("welcome", "en")
                .map(|entry| &entry.value),
            Some(&Translation::Singular("Welcome".to_string()))
        );
    }

    #[test]
    fn distinct_target_infers_language_from_target_path() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("en.lproj");
        let target_dir = temp_dir.path().join("fr-CA.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&target_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let target = target_dir.join("Localizable.strings");
        let output = temp_dir.path().join("translated.xcstrings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();
        fs::write(&target, "\"welcome\" = \"\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.target_langs = vec!["fr".to_string()];
        options.output = Some(output.to_string_lossy().to_string());

        let prepared = prepare_translation(&options).unwrap();

        assert_eq!(prepared.opts.target_langs, vec!["fr-CA".to_string()]);
        assert_eq!(prepared.summary.queued, 1);
        assert!(prepared.target_codec.get_by_language("fr-CA").is_some());
    }

    #[test]
    fn identityless_existing_target_uses_separate_output_locale_and_preserves_entries() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("en.lproj");
        let output_dir = temp_dir.path().join("fr-CA.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&output_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let target = temp_dir.path().join("Existing.strings");
        let output = output_dir.join("Localizable.strings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();
        fs::write(
            &target,
            "\"welcome\" = \"\";\n\"target_only\" = \"Conserver\";\n",
        )
        .unwrap();
        let target_before = fs::read_to_string(&target).unwrap();

        let mut options = base_options(&source, Some(&target));
        options.output = Some(output.to_string_lossy().to_string());
        options.strict = true;

        let prepared = prepare_translation(&options).unwrap();
        assert_eq!(prepared.opts.target_langs, vec!["fr-CA".to_string()]);
        assert!(
            prepared
                .target_codec
                .find_entry("target_only", "fr-CA")
                .is_some()
        );

        let outcome = run_prepared_translation(
            prepared,
            Some(Arc::new(MockBackend::new(vec![(
                ("welcome", "fr-CA"),
                Ok("Bienvenue".to_string()),
            )]))),
        )
        .unwrap();

        assert_eq!(outcome.translated, 1);
        let written = fs::read_to_string(output).unwrap();
        assert!(written.contains("\"welcome\" = \"Bienvenue\";"));
        assert!(written.contains("\"target_only\" = \"Conserver\";"));
        assert_eq!(fs::read_to_string(target).unwrap(), target_before);
    }

    #[test]
    fn strict_mode_binds_identityless_existing_target_from_sole_requested_locale() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("en.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let target = temp_dir.path().join("Existing.strings");
        let output = temp_dir.path().join("translated.xcstrings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();
        fs::write(&target, "\"target_only\" = \"Conserver\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.target_langs = vec!["fr-CA".to_string()];
        options.output = Some(output.to_string_lossy().to_string());
        options.strict = true;

        let prepared = prepare_translation(&options).unwrap();

        assert_eq!(prepared.opts.target_langs, vec!["fr-CA".to_string()]);
        assert!(
            prepared
                .target_codec
                .find_entry("target_only", "fr-CA")
                .is_some()
        );
        assert_eq!(prepared.summary.queued, 1);
    }

    #[test]
    fn identityless_existing_target_rejects_multiple_requested_locales_as_ambiguous() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("en.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let target = temp_dir.path().join("Existing.strings");
        let output = temp_dir.path().join("translated.xcstrings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();
        fs::write(&target, "\"target_only\" = \"Conserver\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.target_langs = vec!["fr".to_string(), "de".to_string()];
        options.output = Some(output.to_string_lossy().to_string());
        options.strict = true;

        let err = prepare_translation(&options).unwrap_err();

        assert!(err.contains("has no locale identity"));
        assert!(err.contains("cannot be assigned unambiguously"));
        assert!(err.contains("[fr, de]"));
        assert!(err.contains("exactly one --target-lang"));
    }

    #[test]
    fn missing_target_path_language_is_authoritative_for_multilanguage_output() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("en.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let target = temp_dir
            .path()
            .join("fr-CA.lproj")
            .join("Localizable.strings");
        let output = temp_dir.path().join("translated.xcstrings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.target_langs = vec!["fr".to_string()];
        options.output = Some(output.to_string_lossy().to_string());

        let prepared = prepare_translation(&options).unwrap();

        assert_eq!(prepared.opts.target_langs, vec!["fr-CA".to_string()]);
        assert_eq!(prepared.summary.queued, 1);
        assert!(prepared.target_codec.get_by_language("en").is_some());
        assert!(prepared.target_codec.get_by_language("fr-CA").is_some());
        assert!(prepared.target_codec.get_by_language("fr").is_none());
    }

    #[test]
    fn missing_same_base_target_hint_precedes_source_seed() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("en-US.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let target = temp_dir
            .path()
            .join("en-GB.lproj")
            .join("Localizable.strings");
        let output = temp_dir.path().join("translated.xcstrings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.source_lang = Some("en-US".to_string());
        options.target_langs = vec!["en".to_string()];
        options.output = Some(output.to_string_lossy().to_string());

        let prepared = prepare_translation(&options).unwrap();

        assert_eq!(prepared.source_resource.language, "en-US");
        assert_eq!(prepared.opts.target_langs, vec!["en-GB".to_string()]);
        assert_eq!(prepared.summary.queued, 1);
        assert!(prepared.target_codec.get_by_language("en-US").is_some());
        assert!(prepared.target_codec.get_by_language("en-GB").is_some());
    }

    #[test]
    fn rejects_qualified_target_conflicting_with_explicit_target_path() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("en.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let target = temp_dir
            .path()
            .join("fr-CA.lproj")
            .join("Localizable.strings");
        let output = temp_dir.path().join("translated.xcstrings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.target_langs = vec!["fr-FR".to_string()];
        options.output = Some(output.to_string_lossy().to_string());

        let err = prepare_translation(&options).unwrap_err();

        assert!(err.contains("Requested target language 'fr-FR'"));
        assert!(err.contains("explicit target path language 'fr-CA'"));
        assert!(err.contains("refusing to retag"));
    }

    #[test]
    fn explicit_target_hint_does_not_block_unrelated_multilanguage_targets() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("en.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let target = temp_dir
            .path()
            .join("fr-CA.lproj")
            .join("Localizable.strings");
        let output = temp_dir.path().join("translated.xcstrings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.target_langs = vec!["fr-CA".to_string(), "de-DE".to_string()];
        options.output = Some(output.to_string_lossy().to_string());

        let prepared = prepare_translation(&options).unwrap();

        assert_eq!(
            prepared.opts.target_langs,
            vec!["fr-CA".to_string(), "de-DE".to_string()]
        );
        assert_eq!(prepared.summary.queued, 2);
        assert!(prepared.target_codec.get_by_language("fr-CA").is_some());
        assert!(prepared.target_codec.get_by_language("de-DE").is_some());
    }

    #[test]
    fn explicit_script_target_hint_allows_sibling_variant_target() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("en.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let target = temp_dir
            .path()
            .join("zh-Hans.lproj")
            .join("Localizable.strings");
        let output = temp_dir.path().join("translated.xcstrings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.target_langs = vec!["zh-Hans".to_string(), "zh-Hant".to_string()];
        options.output = Some(output.to_string_lossy().to_string());

        let prepared = prepare_translation(&options).unwrap();

        assert_eq!(
            prepared.opts.target_langs,
            vec!["zh-Hans".to_string(), "zh-Hant".to_string()]
        );
        assert_eq!(prepared.summary.queued, 2);
        assert!(prepared.target_codec.get_by_language("zh-Hans").is_some());
        assert!(prepared.target_codec.get_by_language("zh-Hant").is_some());
    }

    #[test]
    fn explicit_target_path_binding_is_order_independent() {
        let resolved = resolve_target_languages(
            &Codec::new(),
            &["zh-Hant".to_string(), "zh-Hans".to_string()],
            Some("zh-Hans"),
            None,
        )
        .unwrap();

        assert_eq!(resolved, vec!["zh-Hant".to_string(), "zh-Hans".to_string()]);
    }

    #[test]
    fn rejects_incompatible_qualified_target_and_output_paths() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("en.lproj");
        let target_dir = temp_dir.path().join("pt-BR.lproj");
        let output_dir = temp_dir.path().join("pt-PT.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&target_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let target = target_dir.join("Localizable.strings");
        let output = output_dir.join("Localizable.strings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();
        fs::write(&target, "\"welcome\" = \"Bem-vindo\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.target_langs = vec!["pt-PT".to_string()];
        options.output = Some(output.to_string_lossy().to_string());

        let err = prepare_translation(&options).unwrap_err();

        assert!(err.contains("Target path"));
        assert!(err.contains("pt-BR"));
        assert!(err.contains("pt-PT"));
        assert!(err.contains("refusing to retag"));
    }

    #[test]
    fn rejects_target_language_incompatible_with_single_language_output_path() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("en.lproj");
        let output_dir = temp_dir.path().join("pt-PT.lproj");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Localizable.strings");
        let output = output_dir.join("Localizable.strings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();

        let mut options = base_options(&source, None);
        options.target_langs = vec!["pt-BR".to_string()];
        options.output = Some(output.to_string_lossy().to_string());

        let err = prepare_translation(&options).unwrap_err();

        assert!(err.contains("Target language 'pt-BR'"));
        assert!(err.contains("pt-PT"));
        assert!(err.contains("matching output locale"));
    }

    #[test]
    fn infers_status_from_target_input_format_not_output_format() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("en.strings");
        let target = temp_dir.path().join("fr.strings");
        let output = temp_dir.path().join("translated.xcstrings");

        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();
        fs::write(&target, "\"welcome\" = \"\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.output = Some(output.to_string_lossy().to_string());

        let prepared = prepare_translation(&options).unwrap();
        assert_eq!(prepared.summary.queued, 1);
    }

    #[test]
    fn parses_fenced_json_translation() {
        let text = "```json\n{\"translation\":\"Bonjour\"}\n```";
        let parsed = parse_translation_response(text).unwrap();
        assert_eq!(parsed, "Bonjour");
    }

    #[test]
    fn rejects_blank_translation_values_in_direct_and_fenced_json() {
        for response in [
            r#"{"translation":" \n\t "}"#,
            "```json\n{\"translation\":\"   \"}\n```",
        ] {
            let err = parse_translation_response(response).unwrap_err();
            assert!(err.contains("blank translation value"));
        }
    }

    #[test]
    fn build_prompt_includes_comment_context() {
        let prompt = build_prompt(&BackendRequest {
            key: "countdown".to_string(),
            source_lang: "zh-Hans".to_string(),
            target_lang: "fr".to_string(),
            source_value: "代码过期倒计时".to_string(),
            source_comment: Some("A label displayed below the code expiration timer.".to_string()),
        });

        assert!(prompt.contains("Comment:"));
        assert!(prompt.contains("A label displayed below the code expiration timer."));
    }

    #[test]
    fn translates_multiple_target_languages_into_multilanguage_output() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("Localizable.xcstrings");
        fs::write(
            &source,
            r#"{
  "sourceLanguage" : "en",
  "version" : "1.0",
  "strings" : {
    "welcome" : {
      "localizations" : {
        "en" : {
          "stringUnit" : {
            "state" : "new",
            "value" : "Welcome"
          }
        }
      }
    }
  }
}"#,
        )
        .unwrap();

        let mut options = base_options(&source, None);
        options.target_langs = vec!["fr".to_string(), "de".to_string()];

        let prepared = prepare_translation(&options).unwrap();
        let output_path = prepared.output_path.clone();
        assert_eq!(
            prepared.opts.target_langs,
            vec!["fr".to_string(), "de".to_string()]
        );
        assert_eq!(prepared.summary.total_entries, 2);
        assert_eq!(prepared.summary.queued, 2);

        let outcome = run_prepared_translation(
            prepared,
            Some(Arc::new(MockBackend::new(vec![
                (("welcome", "fr"), Ok("Bienvenue".to_string())),
                (("welcome", "de"), Ok("Willkommen".to_string())),
            ]))),
        )
        .unwrap();

        assert_eq!(outcome.translated, 2);
        let written = fs::read_to_string(output_path).unwrap();
        assert!(written.contains("\"fr\""));
        assert!(written.contains("\"Bienvenue\""));
        assert!(written.contains("\"de\""));
        assert!(written.contains("\"Willkommen\""));
    }

    #[test]
    fn rejects_multiple_target_languages_for_single_language_output() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("en.strings");
        let target = temp_dir.path().join("fr.strings");
        fs::write(&source, "\"welcome\" = \"Welcome\";\n").unwrap();

        let mut options = base_options(&source, Some(&target));
        options.target_langs = vec!["fr".to_string(), "de".to_string()];

        let err = prepare_translation(&options).unwrap_err();
        assert!(err.contains("Multiple --target-lang values are only supported"));
    }

    #[test]
    fn preserves_catalog_source_language_when_translating_from_non_source_locale() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("Localizable.xcstrings");
        fs::write(
            &source,
            r#"{
  "sourceLanguage" : "en",
  "version" : "1.0",
  "strings" : {
    "countdown" : {
      "comment" : "A label displayed below the code expiration timer.",
      "localizations" : {
        "en" : {
          "stringUnit" : {
            "state" : "translated",
            "value" : "Code expired countdown"
          }
        },
        "zh-Hans" : {
          "stringUnit" : {
            "state" : "translated",
            "value" : "代码过期倒计时"
          }
        }
      }
    }
  }
}"#,
        )
        .unwrap();

        let mut options = base_options(&source, None);
        options.source_lang = Some("zh-Hans".to_string());
        options.target_langs = vec!["fr".to_string()];

        let prepared = prepare_translation(&options).unwrap();
        let output_path = prepared.output_path.clone();
        let outcome = run_prepared_translation(
            prepared,
            Some(Arc::new(MockBackend::new(vec![(
                ("countdown", "fr"),
                Ok("Compte a rebours du code expire".to_string()),
            )]))),
        )
        .unwrap();

        assert_eq!(outcome.translated, 1);
        let written = fs::read_to_string(output_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&written).unwrap();
        assert_eq!(parsed["sourceLanguage"], "en");
        assert_eq!(
            parsed["strings"]["countdown"]["localizations"]["fr"]["stringUnit"]["value"],
            "Compte a rebours du code expire"
        );
    }

    #[test]
    fn fails_preflight_before_translation_when_output_cannot_serialize() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("Localizable.xcstrings");
        fs::write(
            &source,
            r#"{
  "sourceLanguage" : "en",
  "version" : "1.0",
  "strings" : {
    "welcome" : {
      "localizations" : {
        "en" : {
          "stringUnit" : {
            "state" : "translated",
            "value" : "Welcome"
          }
        }
      }
    }
  }
}"#,
        )
        .unwrap();

        let prepared = prepare_translation(&base_options(&source, None)).unwrap();
        let mut broken = prepared.clone();
        broken
            .target_codec
            .get_mut_by_language("fr")
            .unwrap()
            .metadata
            .custom
            .insert("source_language".to_string(), "zh-Hans".to_string());

        let err = run_prepared_translation(
            broken,
            Some(Arc::new(MockBackend::new(vec![(
                ("welcome", "fr"),
                Ok("Bonjour".to_string()),
            )]))),
        )
        .unwrap_err();
        assert!(err.contains("Preflight output validation failed"));
        assert!(err.contains("Source language mismatch"));
    }

    #[test]
    fn tolgee_prefill_uses_ai_fallback_and_pushes_namespace() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();
        let source = project_root.join("Localizable.xcstrings");
        let payload = project_root.join("pull_payload.xcstrings");
        let capture = project_root.join("captured_config.json");
        let log = project_root.join("tolgee.log");

        write_translate_source_catalog(&source);
        write_translate_tolgee_payload(&payload);
        let tolgee_config = write_translate_tolgee_config(project_root);
        write_fake_tolgee(project_root, &payload, &capture, &log);

        let mut options = base_options(&source, None);
        options.target_langs = vec!["fr".to_string()];
        options.provider = Some("openai".to_string());
        options.model = Some("gpt-4.1-mini".to_string());
        options.use_tolgee = true;
        options.tolgee_config = Some(tolgee_config.to_string_lossy().to_string());
        options.tolgee_namespaces = vec!["Core".to_string()];

        let prepared = prepare_translation(&options).unwrap();
        assert_eq!(prepared.jobs.len(), 1);
        assert_eq!(prepared.jobs[0].key, "bye");

        let outcome = run_prepared_translation(
            prepared,
            Some(Arc::new(MockBackend::new(vec![(
                ("bye", "fr"),
                Ok("Au revoir".to_string()),
            )]))),
        )
        .unwrap();

        assert_eq!(outcome.translated, 1);
        let written = fs::read_to_string(&source).unwrap();
        assert!(written.contains("\"Bienvenue\""));
        assert!(written.contains("\"Au revoir\""));

        let log_contents = fs::read_to_string(&log).unwrap();
        assert!(log_contents.contains("pull|"));
        assert!(log_contents.contains("push|"));

        let captured = fs::read_to_string(&capture).unwrap();
        assert!(captured.contains("\"namespaces\""));
        assert!(captured.contains("\"Core\""));
    }

    #[test]
    fn tolgee_translate_ignores_unmapped_catalogs_without_namespace_filter() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();
        let source = project_root.join("ModuleExport.xcstrings");

        fs::write(
            &source,
            r#"{
  "sourceLanguage" : "en",
  "version" : "1.0",
  "strings" : {
    "welcome" : {
      "localizations" : {
        "en" : {
          "stringUnit" : {
            "state" : "translated",
            "value" : "Welcome"
          }
        },
        "fr" : {
          "stringUnit" : {
            "state" : "translated",
            "value" : "Bienvenue"
          }
        }
      }
    }
  }
}"#,
        )
        .unwrap();

        let tolgee_config = write_translate_tolgee_config(project_root);
        let mut options = base_options(&source, None);
        options.target_langs = vec!["fr".to_string()];
        options.provider = None;
        options.model = None;
        options.use_tolgee = true;
        options.tolgee_config = Some(tolgee_config.to_string_lossy().to_string());

        let prepared = prepare_translation(&options).unwrap();
        assert!(prepared.tolgee_context.is_none());
        assert!(prepared.jobs.is_empty());

        let outcome = run_prepared_translation(prepared, None).unwrap();
        assert_eq!(outcome.translated, 0);
        assert_eq!(outcome.failed, 0);
    }

    #[test]
    fn falls_back_to_xcstrings_key_when_source_locale_entry_is_missing() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("Localizable.xcstrings");
        fs::write(
            &source,
            r#"{
  "sourceLanguage" : "en",
  "version" : "1.0",
  "strings" : {
    "99+ users have won tons of blue diamonds here" : {
      "localizations" : {
        "tr" : {
          "stringUnit" : {
            "state" : "translated",
            "value" : "99+ kullanici burada tonlarca mavi elmas kazandi"
          }
        }
      }
    }
  }
}"#,
        )
        .unwrap();

        let mut options = base_options(&source, None);
        options.source_lang = Some("en".to_string());
        options.target_langs = vec!["zh-Hans".to_string()];

        let prepared = prepare_translation(&options).unwrap();
        assert_eq!(prepared.summary.queued, 1);
        assert_eq!(
            prepared.jobs[0].source_value,
            "99+ users have won tons of blue diamonds here"
        );
    }
}
