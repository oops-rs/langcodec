use std::collections::{BTreeMap, BTreeSet};

use langcodec::{Codec, PluralCategory, ReadOptions, Translation, signature};
use serde::Serialize;

use crate::path_glob;

#[derive(Debug)]
pub struct CheckOptions {
    pub inputs: Vec<String>,
    pub lang: Option<String>,
    pub json: bool,
    pub continue_on_error: bool,
    pub strict: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct CheckIssue {
    /// Primary path retained for backwards-compatible, single-path consumers.
    path: String,
    /// Every input path implicated in this issue, sorted and deduplicated.
    paths: Vec<String>,
    kind: &'static str,
    message: String,
}

impl CheckIssue {
    fn new(paths: impl IntoIterator<Item = String>, kind: &'static str, message: String) -> Self {
        let mut paths: Vec<String> = paths.into_iter().collect();
        paths.sort();
        paths.dedup();
        let path = paths
            .first()
            .cloned()
            .unwrap_or_else(|| "<inputs>".to_string());
        Self {
            path,
            paths,
            kind,
            message,
        }
    }

    fn for_path(path: &str, kind: &'static str, message: String) -> Self {
        Self::new([path.to_string()], kind, message)
    }
}

#[derive(Debug, Serialize)]
struct CheckReport {
    valid: bool,
    files_checked: usize,
    issues: Vec<CheckIssue>,
}

#[derive(Debug)]
struct PlaceholderObservation {
    path: String,
    signature: Option<Vec<String>>,
}

#[derive(Default)]
struct PlaceholderValidator {
    groups: BTreeMap<(String, String), BTreeMap<String, Vec<PlaceholderObservation>>>,
    issues: BTreeMap<(String, String), Vec<CheckIssue>>,
}

impl PlaceholderValidator {
    fn add_file(&mut self, path: &str, codec: &Codec) -> bool {
        let mut touched_groups = BTreeSet::new();

        for resource in &codec.resources {
            let Some(language) = resource
                .parse_language_identifier()
                .map(|language| language.to_string())
            else {
                continue;
            };
            let domain = resource.metadata.domain.clone();

            for entry in &resource.entries {
                let group = (domain.clone(), entry.id.clone());
                touched_groups.insert(group.clone());
                self.groups
                    .entry(group)
                    .or_default()
                    .entry(language.clone())
                    .or_default()
                    .push(PlaceholderObservation {
                        path: path.to_string(),
                        signature: placeholder_signature(&entry.value),
                    });
            }
        }

        let mut failed = false;
        for group in touched_groups {
            let group_issues = collect_group_placeholder_issues(
                &group.0,
                &group.1,
                self.groups
                    .get(&group)
                    .expect("a touched placeholder group must exist"),
            );
            failed |= !group_issues.is_empty();
            if group_issues.is_empty() {
                self.issues.remove(&group);
            } else {
                self.issues.insert(group, group_issues);
            }
        }
        failed
    }

    fn into_issues(self) -> Vec<CheckIssue> {
        self.issues.into_values().flatten().collect()
    }
}

/// Checks localization files without modifying them.
///
/// Returns `true` only when every inspected file parses and passes structural,
/// plural, and placeholder validation.
pub fn run_check_command(options: CheckOptions) -> bool {
    let (inputs, mut issues) = expand_inputs_independently(&options.inputs);
    let mut report = CheckReport {
        valid: issues.is_empty(),
        files_checked: 0,
        issues: Vec::new(),
    };

    if !issues.is_empty() && !options.continue_on_error {
        sort_issues(&mut issues);
        report.issues = issues;
        report.valid = false;
        print_report(&report, options.json);
        return false;
    }
    report.issues.append(&mut issues);

    let mut placeholder_validator = PlaceholderValidator::default();

    for input in inputs {
        report.files_checked += 1;
        let issue_count_before = report.issues.len();
        let mut placeholder_failure = false;

        let mut codec = Codec::new();
        let read_options = ReadOptions::new()
            .with_language_hint(options.lang.clone())
            .with_strict(options.strict);

        match codec.read_file_by_extension_with_options(&input, &read_options) {
            Ok(()) => {
                collect_file_validation_issues(&input, &codec, &mut report.issues);
                placeholder_failure = placeholder_validator.add_file(&input, &codec);
            }
            Err(error) => {
                report
                    .issues
                    .push(CheckIssue::for_path(&input, "parse", error.to_string()))
            }
        }

        let file_failed = report.issues.len() != issue_count_before || placeholder_failure;
        if file_failed && !options.continue_on_error {
            break;
        }
    }

    report.issues.extend(placeholder_validator.into_issues());
    sort_issues(&mut report.issues);
    report.valid = report.issues.is_empty();
    print_report(&report, options.json);
    report.valid
}

fn expand_inputs_independently(inputs: &[String]) -> (Vec<String>, Vec<CheckIssue>) {
    let mut expanded = BTreeSet::new();
    let mut issues = Vec::new();

    for input in inputs {
        let expansion = path_glob::expand_input_glob_unfiltered(input);
        expanded.extend(expansion.paths);
        for error in expansion.errors {
            issues.push(CheckIssue::for_path(input, "input", error));
        }
    }

    (expanded.into_iter().collect(), issues)
}

fn collect_file_validation_issues(path: &str, codec: &Codec, issues: &mut Vec<CheckIssue>) {
    if codec.resources.is_empty() {
        issues.push(CheckIssue::for_path(
            path,
            "structure",
            "No resources found".to_string(),
        ));
    }

    // A resource represents one normalized locale within one domain. The same
    // locale may legitimately occur more than once in a multi-domain catalog.
    let mut identities: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for resource in &codec.resources {
        if resource.entries.is_empty() {
            issues.push(CheckIssue::for_path(
                path,
                "structure",
                format!(
                    "Resource for language '{}' in domain '{}' has no entries",
                    resource.metadata.language, resource.metadata.domain
                ),
            ));
        }

        match resource.parse_language_identifier() {
            Some(language) => identities
                .entry((language.to_string(), resource.metadata.domain.clone()))
                .or_default()
                .push(resource.metadata.language.clone()),
            None => issues.push(CheckIssue::for_path(
                path,
                "locale",
                format!(
                    "invalid locale identity '{}'; expected a Unicode language identifier",
                    resource.metadata.language
                ),
            )),
        }
    }

    for ((language, domain), mut spellings) in identities {
        if spellings.len() < 2 {
            continue;
        }
        spellings.sort();
        issues.push(CheckIssue::for_path(
            path,
            "locale",
            format!(
                "catalog contains {} resources with duplicate identity (locale '{}', domain '{}'): [{}]",
                spellings.len(),
                language,
                domain,
                spellings.join(", "),
            ),
        ));
    }

    let mut plural_issues = codec.collect_plural_issues();
    plural_issues.sort_by(|left, right| {
        (&left.language, &left.key, &left.missing, &left.have).cmp(&(
            &right.language,
            &right.key,
            &right.missing,
            &right.have,
        ))
    });
    for issue in plural_issues {
        issues.push(CheckIssue::for_path(
            path,
            "plural",
            format!(
                "language '{}' key '{}': missing [{}] (have [{}])",
                issue.language,
                issue.key,
                plural_categories(&issue.missing),
                plural_categories(&issue.have),
            ),
        ));
    }
}

fn collect_group_placeholder_issues(
    domain: &str,
    key: &str,
    languages: &BTreeMap<String, Vec<PlaceholderObservation>>,
) -> Vec<CheckIssue> {
    let mut issues = Vec::new();
    let mut comparable = Vec::new();

    for (language, observations) in languages {
        if observations.len() > 1 {
            let paths: Vec<String> = observations
                .iter()
                .map(|observation| observation.path.clone())
                .collect();
            issues.push(CheckIssue::new(
                paths.clone(),
                "placeholder",
                format!(
                    "domain '{}' key '{}' has {} duplicate entries for language '{}' in [{}]",
                    domain,
                    key,
                    observations.len(),
                    language,
                    sorted_unique_paths(&paths).join(", "),
                ),
            ));
            continue;
        }

        if let Some(observation) = observations.first()
            && let Some(entry_signature) = &observation.signature
        {
            comparable.push((language, observation.path.as_str(), entry_signature));
        }
    }

    // Report every differing language pair. This remains deterministic for
    // 3+ languages and does not depend on a hash-map-selected baseline.
    for left_index in 0..comparable.len() {
        for right_index in (left_index + 1)..comparable.len() {
            let (left_language, left_path, left_signature) = &comparable[left_index];
            let (right_language, right_path, right_signature) = &comparable[right_index];
            if left_signature == right_signature {
                continue;
            }

            issues.push(CheckIssue::new(
                [left_path.to_string(), right_path.to_string()],
                "placeholder",
                format!(
                    "domain '{}' key '{}' placeholder mismatch: language '{}' at '{}' has {} vs language '{}' at '{}' has {}",
                    domain,
                    key,
                    left_language,
                    left_path,
                    render_signature(left_signature),
                    right_language,
                    right_path,
                    render_signature(right_signature),
                ),
            ));
        }
    }

    issues
}

fn placeholder_signature(value: &Translation) -> Option<Vec<String>> {
    match value {
        // Empty entries have no translation contract to compare. Plural
        // branches are intentionally excluded: Resource identifies categories
        // but not the quantity argument, and a valid branch may render "One
        // file" while another renders "%d files".
        Translation::Empty | Translation::Plural(_) => None,
        Translation::Singular(value) => Some(signature(value)),
    }
}

fn sorted_unique_paths(paths: &[String]) -> Vec<String> {
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    paths
}

fn render_signature(signature: &[String]) -> String {
    format!("{signature:?}")
}

fn plural_category(category: &PluralCategory) -> String {
    format!("{category:?}").to_lowercase()
}

fn plural_categories(categories: &BTreeSet<PluralCategory>) -> String {
    categories
        .iter()
        .map(plural_category)
        .collect::<Vec<_>>()
        .join(", ")
}

fn sort_issues(issues: &mut Vec<CheckIssue>) {
    fn kind_rank(kind: &str) -> u8 {
        match kind {
            "input" => 0,
            "parse" => 1,
            "structure" => 2,
            "locale" => 3,
            "plural" => 4,
            "placeholder" => 5,
            _ => u8::MAX,
        }
    }

    issues.sort_by(|left, right| {
        (&left.path, kind_rank(left.kind), &left.paths, &left.message).cmp(&(
            &right.path,
            kind_rank(right.kind),
            &right.paths,
            &right.message,
        ))
    });
    issues.dedup();
}

fn print_report(report: &CheckReport, json: bool) {
    if json {
        let rendered =
            serde_json::to_string_pretty(report).expect("check report serialization is infallible");
        println!("{rendered}");
        return;
    }

    for issue in &report.issues {
        println!(
            "ERROR {} [{}]: {}",
            issue.paths.join(", "),
            issue.kind,
            issue.message
        );
    }

    if report.valid {
        println!(
            "OK: checked {} file(s); no issues found",
            report.files_checked
        );
    } else {
        println!(
            "FAILED: checked {} file(s); found {} issue(s)",
            report.files_checked,
            report.issues.len()
        );
    }
}
