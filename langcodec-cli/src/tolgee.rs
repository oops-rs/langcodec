use langcodec::{
    Codec, FormatType, Metadata, ReadOptions, Resource, Translation, convert_resources_to_format,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::config::{
    LoadedConfig, TolgeeConfig, UserConfig, load_config, load_user_config,
    resolve_config_relative_path,
};

const DEFAULT_TOLGEE_CONFIG: &str = ".tolgeerc.json";
const TOLGEE_FORMAT_APPLE_XCSTRINGS: &str = "APPLE_XCSTRINGS";
const DEFAULT_PULL_TEMPLATE: &str = "/{namespace}/Localizable.{extension}";

#[derive(Debug, Clone)]
pub struct TolgeePullOptions {
    pub config: Option<String>,
    pub namespaces: Vec<String>,
    pub dry_run: bool,
    pub strict: bool,
}

#[derive(Debug, Clone)]
pub struct TolgeePushOptions {
    pub config: Option<String>,
    pub namespaces: Vec<String>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TranslateTolgeeSettings {
    pub enabled: bool,
    pub config: Option<String>,
    pub namespaces: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TranslateTolgeeContext {
    project: TolgeeProject,
    namespace: String,
}

impl TranslateTolgeeContext {
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct TolgeePushFileConfig {
    path: String,
    namespace: String,
}

#[derive(Debug, Clone)]
struct TolgeeMappedFile {
    namespace: String,
    relative_path: String,
    absolute_path: PathBuf,
}

#[derive(Debug, Clone)]
struct TolgeeProject {
    config_path: PathBuf,
    project_root: PathBuf,
    raw: Value,
    user_api_key: Option<String>,
    pull_template: String,
    mappings: Vec<TolgeeMappedFile>,
}

#[derive(Debug, Default, Clone)]
struct MergeReport {
    merged: usize,
    skipped_new_keys: usize,
}

#[derive(Debug, Clone)]
struct AllowedLanguageMapping {
    pulled_language: String,
    local_language: String,
}

#[derive(Debug, Clone)]
enum TolgeeCliInvocation {
    Direct(PathBuf),
    PnpmExec,
    NpmExec,
}

pub fn run_tolgee_pull_command(opts: TolgeePullOptions) -> Result<(), String> {
    let project = load_tolgee_project(opts.config.as_deref())?;
    let mappings = select_mappings(&project, &opts.namespaces)?;
    let selected_namespaces = mappings
        .iter()
        .map(|mapping| mapping.namespace.clone())
        .collect::<Vec<_>>();

    println!(
        "Preparing Tolgee pull for {} namespace(s): {}",
        selected_namespaces.len(),
        describe_namespaces(&selected_namespaces)
    );
    println!("Pulling Tolgee catalogs into a temporary workspace before merging locally...");
    let pulled = pull_catalogs(&project, &selected_namespaces, opts.strict)?;

    let mut changed_files = 0usize;
    for mapping in mappings {
        if !mapping.absolute_path.is_file() {
            return Err(format!(
                "Mapped xcstrings file does not exist: {}",
                mapping.absolute_path.display()
            ));
        }

        println!(
            "Merging namespace '{}' into {}",
            mapping.namespace, mapping.relative_path
        );
        let mut local_codec = read_xcstrings_codec(&mapping.absolute_path, opts.strict)?;
        let pulled_codec = pulled
            .get(&mapping.namespace)
            .ok_or_else(|| format!("Tolgee did not export namespace '{}'", mapping.namespace))?;
        let report = merge_tolgee_catalog(&mut local_codec, pulled_codec, &[])?;

        println!(
            "Namespace {} -> {} merged={} skipped_new_keys={}",
            mapping.namespace, mapping.relative_path, report.merged, report.skipped_new_keys
        );

        if report.merged > 0 {
            changed_files += 1;
            if !opts.dry_run {
                println!("Writing merged catalog to {}", mapping.relative_path);
                write_xcstrings_codec(&local_codec, &mapping.absolute_path)?;
            }
        }
    }

    if opts.dry_run {
        println!("Dry-run mode: no files were written");
    } else {
        println!("Tolgee pull complete: updated {} file(s)", changed_files);
    }
    Ok(())
}

pub fn run_tolgee_push_command(opts: TolgeePushOptions) -> Result<(), String> {
    let project = load_tolgee_project(opts.config.as_deref())?;
    let mappings = select_mappings(&project, &opts.namespaces)?;
    if mappings.is_empty() {
        return Err("No Tolgee namespaces matched the request".to_string());
    }

    for mapping in &mappings {
        if !mapping.absolute_path.is_file() {
            return Err(format!(
                "Mapped xcstrings file does not exist: {}",
                mapping.absolute_path.display()
            ));
        }
    }

    let namespaces = mappings
        .iter()
        .map(|mapping| mapping.namespace.clone())
        .collect::<Vec<_>>();

    println!(
        "Preparing Tolgee push for {} namespace(s): {}",
        namespaces.len(),
        describe_namespaces(&namespaces)
    );
    println!("Validating mapped xcstrings files before upload...");

    if opts.dry_run {
        println!(
            "Dry-run mode: would push namespaces {}",
            namespaces.join(", ")
        );
        return Ok(());
    }

    println!("Uploading catalogs to Tolgee...");
    invoke_tolgee(&project, "push", &namespaces, None)?;
    println!("Tolgee push complete: {}", namespaces.join(", "));
    Ok(())
}

pub fn prefill_translate_from_tolgee(
    settings: &TranslateTolgeeSettings,
    local_catalog_path: &str,
    target_codec: &mut Codec,
    target_langs: &[String],
    strict: bool,
) -> Result<Option<TranslateTolgeeContext>, String> {
    if !settings.enabled {
        return Ok(None);
    }

    let project = load_tolgee_project(settings.config.as_deref())?;
    let Some(mapping) = find_mapping_for_catalog(&project, local_catalog_path)? else {
        if settings.namespaces.is_empty() {
            return Ok(None);
        }
        return Err(format!(
            "Catalog '{}' is not configured in Tolgee push.files",
            local_catalog_path
        ));
    };

    if !settings.namespaces.is_empty()
        && !settings
            .namespaces
            .iter()
            .any(|namespace| namespace == &mapping.namespace)
    {
        return Err(format!(
            "Catalog '{}' maps to Tolgee namespace '{}' which is not included in --tolgee-namespace/[tolgee].namespaces",
            local_catalog_path, mapping.namespace
        ));
    }

    let pulled = pull_catalogs(&project, std::slice::from_ref(&mapping.namespace), strict)?;
    let pulled_codec = pulled
        .get(&mapping.namespace)
        .ok_or_else(|| format!("Tolgee did not export namespace '{}'", mapping.namespace))?;
    merge_tolgee_catalog(target_codec, pulled_codec, target_langs)?;

    Ok(Some(TranslateTolgeeContext {
        project,
        namespace: mapping.namespace,
    }))
}

pub fn push_translate_results_to_tolgee(
    context: &TranslateTolgeeContext,
    dry_run: bool,
) -> Result<(), String> {
    if dry_run {
        return Ok(());
    }

    invoke_tolgee(
        &context.project,
        "push",
        std::slice::from_ref(&context.namespace),
        None,
    )
}

fn load_tolgee_project(explicit_path: Option<&str>) -> Result<TolgeeProject, String> {
    if let Some(path) = explicit_path {
        return load_tolgee_project_from_path(path);
    }

    if let Some(loaded) = load_config(None)? {
        let tolgee = &loaded.data.tolgee;
        if let Some(source_path) = tolgee.config.as_deref() {
            let resolved = resolve_config_relative_path(loaded.config_dir(), source_path);
            return load_tolgee_project_from_path(&resolved);
        }
        if tolgee.has_inline_runtime_config() {
            return load_tolgee_project_from_langcodec(&loaded);
        }
    }

    let config_path = resolve_default_tolgee_json_path()?;
    load_tolgee_project_from_json(config_path)
}

fn load_tolgee_project_from_path(path: &str) -> Result<TolgeeProject, String> {
    let resolved = absolute_from_current_dir(path)?;
    let extension = resolved
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    match extension.as_deref() {
        Some("json") => load_tolgee_project_from_json(resolved),
        Some("toml") => {
            let loaded = load_config(Some(&resolved.to_string_lossy()))?
                .ok_or_else(|| format!("Config file does not exist: {}", resolved.display()))?;
            let tolgee = &loaded.data.tolgee;
            if let Some(source_path) = tolgee.config.as_deref() {
                let nested = resolve_config_relative_path(loaded.config_dir(), source_path);
                load_tolgee_project_from_path(&nested)
            } else {
                load_tolgee_project_from_langcodec(&loaded)
            }
        }
        _ => Err(format!(
            "Unsupported Tolgee config source '{}'. Expected .json or .toml",
            resolved.display()
        )),
    }
}

fn load_tolgee_project_from_json(config_path: PathBuf) -> Result<TolgeeProject, String> {
    let text = fs::read_to_string(&config_path).map_err(|e| {
        format!(
            "Failed to read Tolgee config '{}': {}",
            config_path.display(),
            e
        )
    })?;
    let raw: Value = serde_json::from_str(&text).map_err(|e| {
        format!(
            "Failed to parse Tolgee config '{}': {}",
            config_path.display(),
            e
        )
    })?;
    let project_root = config_path
        .parent()
        .ok_or_else(|| {
            format!(
                "Tolgee config path has no parent: {}",
                config_path.display()
            )
        })?
        .to_path_buf();
    build_tolgee_project_from_raw(config_path, project_root, raw)
}

fn load_tolgee_project_from_langcodec(loaded: &LoadedConfig) -> Result<TolgeeProject, String> {
    let project_root = loaded
        .config_dir()
        .ok_or_else(|| format!("Config path has no parent: {}", loaded.path.display()))?
        .to_path_buf();
    let tolgee = &loaded.data.tolgee;
    if !tolgee.has_inline_runtime_config() {
        return Err(format!(
            "Config '{}' does not contain inline [tolgee] runtime settings",
            loaded.path.display()
        ));
    }

    let raw = build_tolgee_json_from_toml(tolgee)?;
    build_tolgee_project_from_raw(loaded.path.clone(), project_root, raw)
}

fn build_tolgee_json_from_toml(tolgee: &TolgeeConfig) -> Result<Value, String> {
    if tolgee.push.files.is_empty() {
        return Err("Tolgee [push.files] must contain at least one mapping".to_string());
    }

    let push_files = tolgee
        .push
        .files
        .iter()
        .map(|file| {
            json!({
                "path": file.path,
                "namespace": file.namespace,
            })
        })
        .collect::<Vec<_>>();

    let mut root = json!({
        "format": tolgee.format.as_deref().unwrap_or(TOLGEE_FORMAT_APPLE_XCSTRINGS),
        "push": {
            "files": push_files,
        },
        "pull": {
            "path": tolgee.pull.path.as_deref().unwrap_or("./tolgee-temp"),
            "fileStructureTemplate": tolgee.pull.file_structure_template.as_deref().unwrap_or(DEFAULT_PULL_TEMPLATE),
        }
    });

    if let Some(schema) = tolgee.schema.as_deref() {
        set_nested_string(&mut root, &["$schema"], schema);
    }
    if let Some(project_id) = tolgee.project_id {
        set_nested_value(&mut root, &["projectId"], json!(project_id));
    }
    if let Some(api_url) = tolgee.api_url.as_deref() {
        set_nested_string(&mut root, &["apiUrl"], api_url);
    }
    if let Some(api_key) = tolgee.api_key.as_deref() {
        set_nested_string(&mut root, &["apiKey"], api_key);
    }
    if let Some(languages) = tolgee.push.languages.as_ref() {
        set_nested_array(&mut root, &["push", "languages"], languages);
    }
    if let Some(force_mode) = tolgee.push.force_mode.as_deref() {
        set_nested_string(&mut root, &["push", "forceMode"], force_mode);
    }

    Ok(root)
}

fn build_tolgee_project_from_raw(
    config_path: PathBuf,
    project_root: PathBuf,
    mut raw: Value,
) -> Result<TolgeeProject, String> {
    normalize_tolgee_raw(&mut raw);

    let format = raw
        .get("format")
        .and_then(Value::as_str)
        .ok_or_else(|| "Tolgee config is missing 'format'".to_string())?;
    if format != TOLGEE_FORMAT_APPLE_XCSTRINGS {
        return Err(format!(
            "Unsupported Tolgee format '{}'. v1 supports only {}",
            format, TOLGEE_FORMAT_APPLE_XCSTRINGS
        ));
    }

    let push_files_value = raw
        .get("push")
        .and_then(|value| value.get("files"))
        .cloned()
        .ok_or_else(|| "Tolgee config is missing push.files".to_string())?;
    let push_files: Vec<TolgeePushFileConfig> = serde_json::from_value(push_files_value)
        .map_err(|e| format!("Tolgee config push.files is invalid: {}", e))?;
    if push_files.is_empty() {
        return Err("Tolgee config push.files is empty".to_string());
    }

    let mappings = push_files
        .into_iter()
        .map(|file| TolgeeMappedFile {
            absolute_path: normalize_path(project_root.join(&file.path)),
            relative_path: file.path,
            namespace: file.namespace,
        })
        .collect::<Vec<_>>();

    let pull_template = raw
        .get("pull")
        .and_then(|value| value.get("fileStructureTemplate"))
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_PULL_TEMPLATE)
        .to_string();
    let user_api_key = resolve_tolgee_user_api_key(&raw)?;

    Ok(TolgeeProject {
        config_path,
        project_root,
        raw,
        user_api_key,
        pull_template,
        mappings,
    })
}

fn resolve_tolgee_user_api_key(raw: &Value) -> Result<Option<String>, String> {
    if tolgee_has_project_api_key(raw) {
        return Ok(None);
    }

    let Some(user_config) = load_user_config()? else {
        return Ok(None);
    };
    Ok(resolve_tolgee_user_api_key_from_config(
        raw,
        Some(&user_config.data),
    ))
}

fn resolve_tolgee_user_api_key_from_config(
    raw: &Value,
    user_config: Option<&UserConfig>,
) -> Option<String> {
    if tolgee_has_project_api_key(raw) {
        return None;
    }

    user_config.and_then(|config| {
        config
            .tolgee
            .api_key_for_project(tolgee_project_id(raw))
            .map(str::to_string)
    })
}

fn tolgee_has_project_api_key(raw: &Value) -> bool {
    raw.get("apiKey").is_some()
}

fn tolgee_project_id(raw: &Value) -> Option<u64> {
    raw.get("projectId").and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn normalize_tolgee_raw(raw: &mut Value) {
    let Some(push) = raw.get_mut("push").and_then(Value::as_object_mut) else {
        return;
    };

    if push.contains_key("languages") {
        return;
    }

    if let Some(language) = push.remove("language") {
        push.insert("languages".to_string(), language);
    }
}

fn describe_namespaces(namespaces: &[String]) -> String {
    match namespaces {
        [] => "(none)".to_string(),
        [namespace] => namespace.clone(),
        _ => namespaces.join(", "),
    }
}

fn resolve_default_tolgee_json_path() -> Result<PathBuf, String> {
    let mut current =
        env::current_dir().map_err(|e| format!("Failed to determine current directory: {}", e))?;
    loop {
        let candidate = current.join(DEFAULT_TOLGEE_CONFIG);
        if candidate.is_file() {
            return Ok(candidate);
        }
        if !current.pop() {
            return Err(format!(
                "Could not find {} in the current directory or any parent",
                DEFAULT_TOLGEE_CONFIG
            ));
        }
    }
}

fn absolute_from_current_dir(path: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Ok(normalize_path(candidate.to_path_buf()));
    }

    let current_dir =
        env::current_dir().map_err(|e| format!("Failed to determine current directory: {}", e))?;
    Ok(normalize_path(current_dir.join(candidate)))
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        normalized.push(component);
    }
    normalized
}

fn select_mappings(
    project: &TolgeeProject,
    namespaces: &[String],
) -> Result<Vec<TolgeeMappedFile>, String> {
    if namespaces.is_empty() {
        return Ok(project.mappings.clone());
    }

    let mut selected = Vec::new();
    for namespace in namespaces {
        let mapping = project
            .mappings
            .iter()
            .find(|mapping| mapping.namespace == *namespace)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "Tolgee namespace '{}' is not configured in push.files",
                    namespace
                )
            })?;
        if !selected
            .iter()
            .any(|existing: &TolgeeMappedFile| existing.namespace == mapping.namespace)
        {
            selected.push(mapping);
        }
    }
    Ok(selected)
}

fn find_mapping_for_catalog(
    project: &TolgeeProject,
    local_catalog_path: &str,
) -> Result<Option<TolgeeMappedFile>, String> {
    let resolved = absolute_from_current_dir(local_catalog_path)?;
    Ok(project
        .mappings
        .iter()
        .find(|mapping| mapping.absolute_path == resolved)
        .cloned())
}

fn discover_tolgee_cli(project_root: &Path) -> Result<TolgeeCliInvocation, String> {
    let local_name = if cfg!(windows) {
        "node_modules/.bin/tolgee.cmd"
    } else {
        "node_modules/.bin/tolgee"
    };
    let local_cli = project_root.join(local_name);
    if local_cli.is_file() {
        return Ok(TolgeeCliInvocation::Direct(local_cli));
    }

    match Command::new("tolgee").arg("--version").output() {
        Ok(output) if output.status.success() => {
            return Ok(TolgeeCliInvocation::Direct(PathBuf::from("tolgee")));
        }
        Ok(_) | Err(_) => {}
    }

    if let Ok(output) = Command::new("pnpm")
        .args(["exec", "tolgee", "--version"])
        .current_dir(project_root)
        .output()
        && output.status.success()
    {
        return Ok(TolgeeCliInvocation::PnpmExec);
    }

    if let Ok(output) = Command::new("npm")
        .args(["exec", "--", "tolgee", "--version"])
        .current_dir(project_root)
        .output()
        && output.status.success()
    {
        return Ok(TolgeeCliInvocation::NpmExec);
    }

    Err(
        "Tolgee CLI not found. Install @tolgee/cli locally in node_modules, make 'tolgee' available on PATH, or ensure 'pnpm exec tolgee'/'npm exec -- tolgee' works in the project"
            .to_string(),
    )
}

fn invoke_tolgee(
    project: &TolgeeProject,
    subcommand: &str,
    namespaces: &[String],
    pull_root_override: Option<&Path>,
) -> Result<(), String> {
    let cli = discover_tolgee_cli(&project.project_root)?;
    let config_path = if project_uses_json_config(project)
        && namespaces.is_empty()
        && pull_root_override.is_none()
    {
        project.config_path.clone()
    } else {
        write_overlay_config(project, namespaces, pull_root_override)?
    };

    let mut command = match cli {
        TolgeeCliInvocation::Direct(path) => Command::new(path),
        TolgeeCliInvocation::PnpmExec => {
            let mut command = Command::new("pnpm");
            command.args(["exec", "tolgee"]);
            command
        }
        TolgeeCliInvocation::NpmExec => {
            let mut command = Command::new("npm");
            command.args(["exec", "--", "tolgee"]);
            command
        }
    };

    if let Some(api_key) = &project.user_api_key {
        command.env("TOLGEE_API_KEY", api_key);
    }

    let output = command
        .arg("--config")
        .arg(&config_path)
        .arg(subcommand)
        .arg("--verbose")
        .current_dir(&project.project_root)
        .output()
        .map_err(|e| format!("Failed to run Tolgee CLI: {}", e))?;

    if config_path != project.config_path {
        let _ = fs::remove_file(&config_path);
    }

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "Tolgee CLI {} failed (status={}): stdout={} stderr={}",
        subcommand,
        output.status,
        stdout.trim(),
        stderr.trim()
    ))
}

fn project_uses_json_config(project: &TolgeeProject) -> bool {
    project
        .config_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

fn write_overlay_config(
    project: &TolgeeProject,
    namespaces: &[String],
    pull_root_override: Option<&Path>,
) -> Result<PathBuf, String> {
    let mut raw = project.raw.clone();
    if !namespaces.is_empty() {
        set_nested_array(&mut raw, &["pull", "namespaces"], namespaces);
        set_nested_array(&mut raw, &["push", "namespaces"], namespaces);
    }
    if let Some(pull_root) = pull_root_override {
        set_nested_string(
            &mut raw,
            &["pull", "path"],
            pull_root.to_string_lossy().as_ref(),
        );
    }

    let unique = format!(
        ".langcodec-tolgee-{}-{}.json",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("System clock error: {}", e))?
            .as_nanos()
    );
    let overlay_path = project.project_root.join(unique);
    fs::write(
        &overlay_path,
        serde_json::to_vec_pretty(&raw)
            .map_err(|e| format!("Failed to serialize Tolgee overlay config: {}", e))?,
    )
    .map_err(|e| format!("Failed to write Tolgee overlay config: {}", e))?;
    Ok(overlay_path)
}

fn set_nested_array(root: &mut Value, path: &[&str], values: &[String]) {
    set_nested_value(
        root,
        path,
        Value::Array(values.iter().map(|value| json!(value)).collect()),
    );
}

fn set_nested_string(root: &mut Value, path: &[&str], value: &str) {
    set_nested_value(root, path, Value::String(value.to_string()));
}

fn set_nested_value(root: &mut Value, path: &[&str], value: Value) {
    let mut current = root;
    for key in &path[..path.len() - 1] {
        if !current.is_object() {
            *current = Value::Object(Map::new());
        }
        let object = current.as_object_mut().expect("object");
        current = object
            .entry((*key).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }

    if !current.is_object() {
        *current = Value::Object(Map::new());
    }
    current
        .as_object_mut()
        .expect("object")
        .insert(path[path.len() - 1].to_string(), value);
}

fn pull_catalogs(
    project: &TolgeeProject,
    namespaces: &[String],
    strict: bool,
) -> Result<HashMap<String, Codec>, String> {
    let selected = select_mappings(project, namespaces)?;
    let temp_root = create_temp_dir("langcodec-tolgee-pull")?;
    let result = (|| {
        invoke_tolgee(project, "pull", namespaces, Some(&temp_root))?;
        let mut pulled = HashMap::new();
        for mapping in selected {
            let pulled_path = pulled_path_for_namespace(project, &temp_root, &mapping.namespace)?;
            if !pulled_path.is_file() {
                return Err(format!(
                    "Tolgee pull did not produce '{}'",
                    pulled_path.display()
                ));
            }
            pulled.insert(
                mapping.namespace,
                read_xcstrings_codec(&pulled_path, strict)?,
            );
        }
        Ok(pulled)
    })();
    let _ = fs::remove_dir_all(&temp_root);
    result
}

fn create_temp_dir(prefix: &str) -> Result<PathBuf, String> {
    let dir = env::temp_dir().join(format!(
        "{}-{}-{}",
        prefix,
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("System clock error: {}", e))?
            .as_nanos()
    ));
    fs::create_dir_all(&dir).map_err(|e| {
        format!(
            "Failed to create temporary directory '{}': {}",
            dir.display(),
            e
        )
    })?;
    Ok(dir)
}

fn pulled_path_for_namespace(
    project: &TolgeeProject,
    pull_root: &Path,
    namespace: &str,
) -> Result<PathBuf, String> {
    if project.pull_template.contains("{languageTag}") {
        return Err(
            "Tolgee pull.fileStructureTemplate with {languageTag} is not supported for APPLE_XCSTRINGS in v1"
                .to_string(),
        );
    }

    let relative = project
        .pull_template
        .replace("{namespace}", namespace)
        .replace("{extension}", "xcstrings");
    if relative.contains('{') {
        return Err(format!(
            "Unsupported placeholders in Tolgee pull.fileStructureTemplate: {}",
            project.pull_template
        ));
    }
    Ok(pull_root.join(relative.trim_start_matches('/')))
}

fn merge_tolgee_catalog(
    local_codec: &mut Codec,
    pulled_codec: &Codec,
    allowed_langs: &[String],
) -> Result<MergeReport, String> {
    validate_unique_language_identities(local_codec, "Local catalog")?;
    validate_unique_language_identities(pulled_codec, "Pulled Tolgee catalog")?;
    let allowed_languages = resolve_allowed_languages(local_codec, pulled_codec, allowed_langs)?;

    let existing_keys = local_codec
        .resources
        .iter()
        .flat_map(|resource| resource.entries.iter().map(|entry| entry.id.clone()))
        .collect::<BTreeSet<_>>();
    let mut report = MergeReport::default();

    for pulled_resource in &pulled_codec.resources {
        let local_languages = if allowed_langs.is_empty() {
            vec![pulled_resource.metadata.language.clone()]
        } else {
            allowed_languages
                .iter()
                .filter(|mapping| {
                    language_identity_eq(
                        &mapping.pulled_language,
                        &pulled_resource.metadata.language,
                    )
                })
                .map(|mapping| mapping.local_language.clone())
                .collect::<Vec<_>>()
        };

        for destination_language in local_languages {
            let mut local_metadata = pulled_resource.metadata.clone();
            local_metadata.language = destination_language;
            let local_language = ensure_resource(local_codec, &local_metadata);

            for pulled_entry in &pulled_resource.entries {
                if !existing_keys.contains(&pulled_entry.id) {
                    report.skipped_new_keys += 1;
                    continue;
                }
                if translation_is_empty(&pulled_entry.value) {
                    continue;
                }

                if let Some(existing) = find_entry_mut_by_language_identity(
                    local_codec,
                    &pulled_entry.id,
                    &local_language,
                ) {
                    if existing.value != pulled_entry.value
                        || existing.status != pulled_entry.status
                        || existing.comment != pulled_entry.comment
                    {
                        existing.value = pulled_entry.value.clone();
                        existing.status = pulled_entry.status.clone();
                        existing.comment = pulled_entry.comment.clone();
                        report.merged += 1;
                    }
                    continue;
                }

                let _ = local_codec.add_entry(
                    &pulled_entry.id,
                    &local_language,
                    pulled_entry.value.clone(),
                    pulled_entry.comment.clone(),
                    Some(pulled_entry.status.clone()),
                );
                report.merged += 1;
            }
        }
    }

    Ok(report)
}

fn resolve_allowed_languages(
    local_codec: &Codec,
    pulled_codec: &Codec,
    requested_languages: &[String],
) -> Result<Vec<AllowedLanguageMapping>, String> {
    if requested_languages.is_empty() {
        return Ok(Vec::new());
    }

    let available = pulled_codec
        .resources
        .iter()
        .map(|resource| resource.metadata.language.as_str())
        .collect::<Vec<_>>();
    let mut resolved = Vec::<AllowedLanguageMapping>::new();

    for requested in requested_languages {
        let pulled_language = if let Some(exact) = available
            .iter()
            .copied()
            .find(|candidate| language_identity_eq(candidate, requested))
        {
            exact.to_string()
        } else {
            let normalized_requested = normalize_lang(requested);
            if normalized_requested.contains('-') {
                requested.clone()
            } else {
                let mut variants = available
                    .iter()
                    .copied()
                    .filter(|candidate| {
                        normalize_lang(candidate)
                            .split('-')
                            .next()
                            .is_some_and(|language| language == normalized_requested.as_str())
                    })
                    .collect::<Vec<_>>();
                match variants.len() {
                    0 => requested.clone(),
                    1 => variants[0].to_string(),
                    _ => {
                        variants.sort_by_key(|candidate| normalize_lang(candidate));
                        return Err(format!(
                            "Tolgee target language '{}' is ambiguous; matching variants: {}. Specify a fully-qualified target language",
                            requested,
                            variants.join(", ")
                        ));
                    }
                }
            }
        };
        let local_language = find_resource_by_language_identity(local_codec, requested)
            .map(|resource| resource.metadata.language.clone())
            .unwrap_or_else(|| requested.clone());

        if !resolved.iter().any(|existing| {
            language_identity_eq(&existing.pulled_language, &pulled_language)
                && language_identity_eq(&existing.local_language, &local_language)
        }) {
            resolved.push(AllowedLanguageMapping {
                pulled_language,
                local_language,
            });
        }
    }

    Ok(resolved)
}

fn ensure_resource(codec: &mut Codec, metadata: &Metadata) -> String {
    if let Some(existing) = find_resource_by_language_identity(codec, &metadata.language) {
        return existing.metadata.language.clone();
    }

    codec.add_resource(Resource {
        metadata: metadata.clone(),
        entries: Vec::new(),
    });
    metadata.language.clone()
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

fn find_entry_mut_by_language_identity<'a>(
    codec: &'a mut Codec,
    key: &str,
    language: &str,
) -> Option<&'a mut langcodec::Entry> {
    codec
        .resources
        .iter_mut()
        .find(|resource| language_identity_eq(&resource.metadata.language, language))
        .and_then(|resource| resource.find_entry_mut(key))
}

fn translation_is_empty(translation: &Translation) -> bool {
    match translation {
        Translation::Empty => true,
        Translation::Singular(value) => value.trim().is_empty(),
        Translation::Plural(_) => false,
    }
}

fn read_xcstrings_codec(path: &Path, strict: bool) -> Result<Codec, String> {
    let format = path
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| ext.eq_ignore_ascii_case("xcstrings"))
        .ok_or_else(|| {
            format!(
                "Tolgee v1 supports only .xcstrings files, got '{}'",
                path.display()
            )
        })?;
    let _ = format;

    let mut codec = Codec::new();
    codec
        .read_file_by_extension_with_options(path, &ReadOptions::new().with_strict(strict))
        .map_err(|e| format!("Failed to read '{}': {}", path.display(), e))?;
    Ok(codec)
}

fn write_xcstrings_codec(codec: &Codec, path: &Path) -> Result<(), String> {
    convert_resources_to_format(
        codec.resources.clone(),
        &path.to_string_lossy(),
        FormatType::Xcstrings,
    )
    .map_err(|e| format!("Failed to write '{}': {}", path.display(), e))
}

fn language_identity_eq(left: &str, right: &str) -> bool {
    normalize_lang(left) == normalize_lang(right)
}

fn normalize_lang(value: &str) -> String {
    let normalized = value.trim().replace('_', "-");
    normalized
        .parse::<unic_langid::LanguageIdentifier>()
        .map(|language| language.to_string().to_ascii_lowercase())
        .unwrap_or_else(|_| normalized.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use langcodec::{Entry, EntryStatus};
    use tempfile::TempDir;

    fn test_entry(id: &str, value: Translation) -> Entry {
        Entry {
            id: id.to_string(),
            value,
            comment: None,
            status: EntryStatus::Translated,
            custom: HashMap::new(),
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

    #[test]
    fn merge_tolgee_catalog_updates_existing_keys_and_skips_new_ones() {
        let mut local = Codec::new();
        local.add_resource(Resource {
            metadata: Metadata {
                language: "en".to_string(),
                domain: String::new(),
                custom: HashMap::new(),
            },
            entries: vec![langcodec::Entry {
                id: "welcome".to_string(),
                value: Translation::Singular("Welcome".to_string()),
                comment: None,
                status: EntryStatus::Translated,
                custom: HashMap::new(),
            }],
        });

        let pulled = Codec {
            resources: vec![Resource {
                metadata: Metadata {
                    language: "fr".to_string(),
                    domain: String::new(),
                    custom: HashMap::new(),
                },
                entries: vec![
                    langcodec::Entry {
                        id: "welcome".to_string(),
                        value: Translation::Singular("Bienvenue".to_string()),
                        comment: Some("Greeting".to_string()),
                        status: EntryStatus::Translated,
                        custom: HashMap::new(),
                    },
                    langcodec::Entry {
                        id: "new_only".to_string(),
                        value: Translation::Singular("Nouveau".to_string()),
                        comment: None,
                        status: EntryStatus::Translated,
                        custom: HashMap::new(),
                    },
                ],
            }],
        };

        let report = merge_tolgee_catalog(&mut local, &pulled, &[]).unwrap();
        assert_eq!(report.merged, 1);
        assert_eq!(report.skipped_new_keys, 1);

        let fr_entry = local.find_entry("welcome", "fr").expect("fr welcome");
        assert_eq!(
            fr_entry.value,
            Translation::Singular("Bienvenue".to_string())
        );
        assert_eq!(fr_entry.comment.as_deref(), Some("Greeting"));
        assert!(local.find_entry("new_only", "fr").is_none());
    }

    #[test]
    fn merge_tolgee_catalog_keeps_script_variants_distinct() {
        let mut local = Codec {
            resources: vec![test_resource(
                "en",
                vec![test_entry(
                    "welcome",
                    Translation::Singular("Welcome".to_string()),
                )],
            )],
        };
        let pulled = Codec {
            resources: vec![
                test_resource(
                    "zh-Hans",
                    vec![test_entry(
                        "welcome",
                        Translation::Singular("欢迎".to_string()),
                    )],
                ),
                test_resource(
                    "zh-Hant",
                    vec![test_entry(
                        "welcome",
                        Translation::Singular("歡迎".to_string()),
                    )],
                ),
            ],
        };

        let report = merge_tolgee_catalog(&mut local, &pulled, &["zh-Hans".to_string()]).unwrap();

        assert_eq!(report.merged, 1);
        assert_eq!(
            local
                .find_entry("welcome", "zh-Hans")
                .map(|entry| &entry.value),
            Some(&Translation::Singular("欢迎".to_string()))
        );
        assert!(local.get_by_language("zh-Hant").is_none());
    }

    #[test]
    fn merge_tolgee_catalog_resolves_bare_language_to_sole_variant() {
        let mut local = Codec {
            resources: vec![
                test_resource(
                    "en",
                    vec![test_entry(
                        "welcome",
                        Translation::Singular("Welcome".to_string()),
                    )],
                ),
                test_resource("fr", vec![test_entry("welcome", Translation::Empty)]),
            ],
        };
        let pulled = Codec {
            resources: vec![test_resource(
                "fr-CA",
                vec![test_entry(
                    "welcome",
                    Translation::Singular("Bienvenue".to_string()),
                )],
            )],
        };

        let report = merge_tolgee_catalog(&mut local, &pulled, &["fr".to_string()]).unwrap();

        assert_eq!(report.merged, 1);
        assert_eq!(
            local.find_entry("welcome", "fr").map(|entry| &entry.value),
            Some(&Translation::Singular("Bienvenue".to_string()))
        );
        assert!(local.get_by_language("fr-CA").is_none());
    }

    #[test]
    fn merge_tolgee_catalog_rejects_ambiguous_bare_language() {
        let mut local = Codec {
            resources: vec![test_resource(
                "en",
                vec![test_entry(
                    "welcome",
                    Translation::Singular("Welcome".to_string()),
                )],
            )],
        };
        let pulled = Codec {
            resources: vec![
                test_resource(
                    "fr-FR",
                    vec![test_entry(
                        "welcome",
                        Translation::Singular("Bienvenue".to_string()),
                    )],
                ),
                test_resource(
                    "fr-CA",
                    vec![test_entry(
                        "welcome",
                        Translation::Singular("Bienvenue".to_string()),
                    )],
                ),
            ],
        };

        let err = merge_tolgee_catalog(&mut local, &pulled, &["fr".to_string()]).unwrap_err();

        assert_eq!(
            err,
            "Tolgee target language 'fr' is ambiguous; matching variants: fr-CA, fr-FR. Specify a fully-qualified target language"
        );
        assert!(local.get_by_language("fr-FR").is_none());
        assert!(local.get_by_language("fr-CA").is_none());
    }

    #[test]
    fn merge_tolgee_catalog_prefers_exact_bare_identity_over_variants() {
        let mut local = Codec {
            resources: vec![test_resource(
                "en",
                vec![test_entry(
                    "welcome",
                    Translation::Singular("Welcome".to_string()),
                )],
            )],
        };
        let pulled = Codec {
            resources: vec![
                test_resource(
                    "fr-CA",
                    vec![test_entry(
                        "welcome",
                        Translation::Singular("Allo".to_string()),
                    )],
                ),
                test_resource(
                    "fr",
                    vec![test_entry(
                        "welcome",
                        Translation::Singular("Bonjour".to_string()),
                    )],
                ),
                test_resource(
                    "fr-FR",
                    vec![test_entry(
                        "welcome",
                        Translation::Singular("Salut".to_string()),
                    )],
                ),
            ],
        };

        let report = merge_tolgee_catalog(&mut local, &pulled, &["fr".to_string()]).unwrap();

        assert_eq!(report.merged, 1);
        assert_eq!(
            local.find_entry("welcome", "fr").map(|entry| &entry.value),
            Some(&Translation::Singular("Bonjour".to_string()))
        );
        assert!(local.get_by_language("fr-CA").is_none());
        assert!(local.get_by_language("fr-FR").is_none());
    }

    #[test]
    fn merge_tolgee_catalog_reuses_normalized_local_identity() {
        let mut local = Codec {
            resources: vec![
                test_resource(
                    "en",
                    vec![test_entry(
                        "welcome",
                        Translation::Singular("Welcome".to_string()),
                    )],
                ),
                test_resource("fr_CA", vec![test_entry("welcome", Translation::Empty)]),
            ],
        };
        let pulled = Codec {
            resources: vec![test_resource(
                "fr-CA",
                vec![test_entry(
                    "welcome",
                    Translation::Singular("Bienvenue".to_string()),
                )],
            )],
        };

        let report = merge_tolgee_catalog(&mut local, &pulled, &["FR-ca".to_string()]).unwrap();

        assert_eq!(report.merged, 1);
        assert_eq!(
            local
                .resources
                .iter()
                .filter(|resource| { language_identity_eq(&resource.metadata.language, "fr-CA") })
                .count(),
            1
        );
        assert_eq!(
            local
                .find_entry("welcome", "fr_CA")
                .map(|entry| &entry.value),
            Some(&Translation::Singular("Bienvenue".to_string()))
        );
    }

    #[test]
    fn merge_tolgee_catalog_rejects_duplicate_normalized_identities_deterministically() {
        let mut local = Codec {
            resources: vec![
                test_resource("fr_CA", Vec::new()),
                test_resource("fr-CA", Vec::new()),
            ],
        };
        let pulled = Codec::new();

        let err = merge_tolgee_catalog(&mut local, &pulled, &[]).unwrap_err();

        assert_eq!(
            err,
            "Local catalog contains duplicate normalized language identities: fr-ca (fr-CA, fr_CA)"
        );
    }

    #[test]
    fn merge_tolgee_catalog_rejects_duplicate_pulled_identities_before_merging() {
        let mut local = Codec {
            resources: vec![test_resource(
                "en",
                vec![test_entry(
                    "welcome",
                    Translation::Singular("Welcome".to_string()),
                )],
            )],
        };
        let pulled = Codec {
            resources: vec![
                test_resource(
                    "zh_Hans",
                    vec![test_entry(
                        "welcome",
                        Translation::Singular("欢迎".to_string()),
                    )],
                ),
                test_resource(
                    "zh-Hans",
                    vec![test_entry(
                        "welcome",
                        Translation::Singular("您好".to_string()),
                    )],
                ),
            ],
        };

        let err = merge_tolgee_catalog(&mut local, &pulled, &[]).unwrap_err();

        assert_eq!(
            err,
            "Pulled Tolgee catalog contains duplicate normalized language identities: zh-hans (zh-Hans, zh_Hans)"
        );
        assert!(local.get_by_language("zh-Hans").is_none());
        assert!(local.get_by_language("zh_Hans").is_none());
    }

    #[test]
    fn pulled_path_rejects_language_tag_templates() {
        let project = TolgeeProject {
            config_path: PathBuf::from("/tmp/.tolgeerc.json"),
            project_root: PathBuf::from("/tmp"),
            raw: json!({}),
            user_api_key: None,
            pull_template: "/{namespace}/{languageTag}.{extension}".to_string(),
            mappings: Vec::new(),
        };
        let err = pulled_path_for_namespace(&project, Path::new("/tmp/pull"), "Core").unwrap_err();
        assert!(err.contains("{languageTag}"));
    }

    #[test]
    fn loads_inline_tolgee_from_langcodec_toml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("langcodec.toml");
        fs::write(
            &config_path,
            r#"
[tolgee]
project_id = 36
api_url = "https://tolgee.example/api"
api_key = "tgpak_example"
namespaces = ["Core"]

[tolgee.push]
languages = ["en"]
force_mode = "KEEP"

[[tolgee.push.files]]
path = "Localizable.xcstrings"
namespace = "Core"

[tolgee.pull]
path = "./tolgee-temp"
file_structure_template = "/{namespace}/Localizable.{extension}"
"#,
        )
        .unwrap();

        let previous_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();
        let project = load_tolgee_project(None).unwrap();
        env::set_current_dir(previous_dir).unwrap();

        assert_eq!(
            project
                .config_path
                .file_name()
                .and_then(|name| name.to_str()),
            Some("langcodec.toml")
        );
        assert_eq!(project.mappings.len(), 1);
        assert_eq!(project.mappings[0].namespace, "Core");
        assert_eq!(project.mappings[0].relative_path, "Localizable.xcstrings");
        assert_eq!(project.raw["projectId"], json!(36));
        assert_eq!(project.raw["apiUrl"], json!("https://tolgee.example/api"));
        assert_eq!(project.raw["apiKey"], json!("tgpak_example"));
        assert_eq!(project.raw["push"]["languages"], json!(["en"]));
        assert_eq!(project.raw["push"]["forceMode"], json!("KEEP"));
        assert_eq!(project.raw["pull"]["path"], json!("./tolgee-temp"));
    }

    #[test]
    fn loads_legacy_tolgee_json_language_key() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(".tolgeerc.json");
        fs::write(
            &config_path,
            r#"{
  "projectId": 36,
  "apiUrl": "https://tolgee.example/api",
  "apiKey": "tgpak_example",
  "format": "APPLE_XCSTRINGS",
  "push": {
    "language": ["en"],
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

        let project = load_tolgee_project_from_json(config_path).unwrap();
        assert_eq!(project.raw["push"]["languages"], json!(["en"]));
        assert!(project.raw["push"].get("language").is_none());
    }

    #[test]
    fn resolves_tolgee_credentials_by_precedence() {
        let user_config: UserConfig = toml::from_str(
            r#"
[tolgee]
api_key = "tgpak_user_global"

[tolgee.projects.36]
api_key = "tgpak_user_project"
"#,
        )
        .unwrap();

        let project_key = resolve_tolgee_user_api_key_from_config(
            &json!({
                "projectId": 36,
                "apiKey": "tgpak_project"
            }),
            Some(&user_config),
        );
        assert_eq!(project_key, None);

        let user_project_key = resolve_tolgee_user_api_key_from_config(
            &json!({ "projectId": 36 }),
            Some(&user_config),
        );
        assert_eq!(user_project_key.as_deref(), Some("tgpak_user_project"));

        let user_global_key = resolve_tolgee_user_api_key_from_config(
            &json!({ "projectId": 37 }),
            Some(&user_config),
        );
        assert_eq!(user_global_key.as_deref(), Some("tgpak_user_global"));
    }
}
