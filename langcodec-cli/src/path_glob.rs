use std::collections::HashSet;
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSetBuilder};
use ignore::WalkBuilder;
use rayon::prelude::*;

fn has_glob_meta(s: &str) -> bool {
    s.bytes().any(|b| matches!(b, b'*' | b'?' | b'[' | b'{'))
}

// Extract the directory components before the first component containing glob
// metadata. A partial component can name an existing directory (for example,
// `values` in `values*`), but it is not a safe traversal root because matching
// sibling directories such as `values-es` live beside it.
fn static_prefix_dir(pattern: &str) -> PathBuf {
    let mut root = PathBuf::new();

    for component in Path::new(pattern).components() {
        if has_glob_meta(&component.as_os_str().to_string_lossy()) {
            break;
        }
        root.push(component.as_os_str());
    }

    if root.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        root
    }
}

/// Expand possible glob patterns in a list of input strings into concrete file paths.
/// Uses ignore + globset for fast, parallel, .gitignore-aware traversal.
pub fn expand_input_globs(inputs: &Vec<String>) -> Result<Vec<String>, String> {
    // Build one GlobSet for all patterns (literal_separator to avoid '/' matching)
    let mut builder = GlobSetBuilder::new();
    for pat in inputs {
        let glob = GlobBuilder::new(pat)
            .literal_separator(true)
            .build()
            .map_err(|e| format!("Invalid glob pattern '{}': {}", pat, e))?;
        builder.add(glob);
    }
    let set = builder
        .build()
        .map_err(|e| format!("Failed to build glob set: {}", e))?;

    // Collect unique roots to minimize directory walks
    let mut roots: Vec<PathBuf> = Vec::new();
    for pat in inputs {
        let root = if has_glob_meta(pat) {
            static_prefix_dir(pat)
        } else {
            Path::new(pat)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        };
        if !roots.iter().any(|r| r == &root) {
            roots.push(root);
        }
    }

    // Walk roots in parallel and match files against the GlobSet
    let collected: Vec<String> = roots
        .par_iter()
        .map(|root| {
            let mut out: Vec<String> = Vec::new();
            let walker = WalkBuilder::new(root)
                .git_ignore(true)
                .git_global(true)
                .git_exclude(true)
                .hidden(false)
                .ignore(true)
                .parents(true)
                .build();

            for dent in walker {
                let dent = match dent {
                    Ok(d) => d,
                    Err(_e) => continue,
                };
                let ftype = match dent.file_type() {
                    Some(t) => t,
                    None => continue,
                };
                if !ftype.is_file() {
                    continue;
                }
                let s = dent.path().to_string_lossy();
                if set.is_match(s.as_ref()) {
                    out.push(s.to_string());
                }
            }
            out
        })
        .flatten()
        .collect();

    // If nothing matched, preserve original inputs to surface errors later
    if collected.is_empty() {
        return Ok(inputs.clone());
    }

    // Deduplicate while preserving order
    let mut seen: HashSet<String> = HashSet::new();
    let mut results: Vec<String> = Vec::with_capacity(collected.len());
    for s in collected {
        if seen.insert(s.clone()) {
            results.push(s);
        }
    }
    Ok(results)
}

#[derive(Debug)]
pub struct UnfilteredGlobExpansion {
    pub paths: Vec<String>,
    pub errors: Vec<String>,
}

/// Expands one check input without applying ignore files or hidden-file rules.
///
/// Unlike [`expand_input_globs`], this preserves traversal failures so callers
/// performing validation can report incomplete filesystem coverage. Globset is
/// retained here because the CLI documents brace patterns such as
/// `*.{strings,xcstrings}`.
pub fn expand_input_glob_unfiltered(input: &str) -> UnfilteredGlobExpansion {
    if !has_glob_meta(input) {
        return UnfilteredGlobExpansion {
            paths: vec![input.to_string()],
            errors: Vec::new(),
        };
    }

    let matcher = match GlobBuilder::new(input).literal_separator(true).build() {
        Ok(glob) => glob.compile_matcher(),
        Err(error) => {
            return UnfilteredGlobExpansion {
                paths: Vec::new(),
                errors: vec![format!("Invalid glob pattern '{}': {}", input, error)],
            };
        }
    };

    let root = static_prefix_dir(input);
    let walker = WalkBuilder::new(root)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .hidden(false)
        .ignore(false)
        .parents(false)
        .build();
    let mut paths = Vec::new();
    let mut errors = Vec::new();

    for entry in walker {
        match entry {
            Ok(entry) if entry.file_type().is_some_and(|kind| kind.is_file()) => {
                let path = entry.path().to_string_lossy();
                if matcher.is_match(path.as_ref()) {
                    paths.push(path.into_owned());
                }
            }
            Ok(_) => {}
            Err(error) => errors.push(format!(
                "Failed to traverse glob pattern '{}': {}",
                input, error
            )),
        }
    }

    paths.sort();
    paths.dedup();
    errors.sort();
    errors.dedup();
    if paths.is_empty() && errors.is_empty() {
        paths.push(input.to_string());
    }

    UnfilteredGlobExpansion { paths, errors }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{expand_input_glob_unfiltered, expand_input_globs};

    #[test]
    fn glob_component_uses_its_parent_as_the_walk_root() {
        let temp_dir = TempDir::new().unwrap();
        let values = temp_dir.path().join("res/values/strings.xml");
        let values_es = temp_dir.path().join("res/values-es/strings.xml");
        fs::create_dir_all(values.parent().unwrap()).unwrap();
        fs::create_dir_all(values_es.parent().unwrap()).unwrap();
        fs::write(&values, "<resources />").unwrap();
        fs::write(&values_es, "<resources />").unwrap();

        let pattern = format!("{}/res/values*/strings.xml", temp_dir.path().display());
        let mut expanded = expand_input_globs(&vec![pattern]).unwrap();
        expanded.sort();

        let mut expected = vec![
            values.to_string_lossy().into_owned(),
            values_es.to_string_lossy().into_owned(),
        ];
        expected.sort();
        assert_eq!(expanded, expected);
    }

    #[test]
    fn literal_input_behavior_is_unchanged() {
        let temp_dir = TempDir::new().unwrap();
        let input = temp_dir.path().join("res/values/strings.xml");
        fs::create_dir_all(input.parent().unwrap()).unwrap();
        fs::write(&input, "<resources />").unwrap();
        let input = input.to_string_lossy().into_owned();

        assert_eq!(
            expand_input_globs(&vec![input.clone()]).unwrap(),
            vec![input]
        );
    }

    #[test]
    fn unfiltered_glob_includes_ignored_and_brace_matches() {
        let temp_dir = TempDir::new().unwrap();
        let strings = temp_dir.path().join("ignored.strings");
        let catalog = temp_dir.path().join("ignored.xcstrings");
        fs::write(
            temp_dir.path().join(".gitignore"),
            "ignored.strings\nignored.xcstrings\n",
        )
        .unwrap();
        fs::write(&strings, "").unwrap();
        fs::write(&catalog, "").unwrap();

        let pattern = format!(
            "{}/*.{{strings,xcstrings}}",
            temp_dir.path().to_string_lossy()
        );
        let expansion = expand_input_glob_unfiltered(&pattern);

        assert!(expansion.errors.is_empty());
        assert_eq!(
            expansion.paths,
            vec![
                strings.to_string_lossy().into_owned(),
                catalog.to_string_lossy().into_owned(),
            ]
        );
    }

    #[test]
    fn unfiltered_glob_surfaces_traversal_errors() {
        let temp_dir = TempDir::new().unwrap();
        let missing_root = temp_dir.path().join("missing");
        let pattern = format!("{}/*.xcstrings", missing_root.to_string_lossy());

        let expansion = expand_input_glob_unfiltered(&pattern);

        assert!(expansion.paths.is_empty());
        assert!(
            expansion
                .errors
                .iter()
                .any(|error| error.contains("Failed to traverse"))
        );
    }
}
