use std::collections::HashSet;
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSetBuilder};
use ignore::WalkBuilder;
use rayon::prelude::*;

/// Expand possible glob patterns in a list of input strings into concrete file paths.
/// Uses ignore + globset for fast, parallel, .gitignore-aware traversal.
pub fn expand_input_globs(inputs: &Vec<String>) -> Result<Vec<String>, String> {
    fn has_glob_meta(s: &str) -> bool {
        s.bytes().any(|b| matches!(b, b'*' | b'?' | b'[' | b'{'))
    }

    // Extract the directory components before the first component containing
    // glob metadata. A partial component can name an existing directory (for
    // example, `values` in `values*`), but it is not a safe traversal root
    // because matching sibling directories such as `values-es` live beside it.
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::expand_input_globs;

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
}
