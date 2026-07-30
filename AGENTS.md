# AGENTS.md — Automation and Navigation Guide

This document helps automation agents quickly understand, build, test, and operate this repository.

## Overview

- **Library crate**: `langcodec/` — universal localization toolkit (parse/convert Apple `.strings`, `.stringsdict`, `.xcstrings`, `.xliff`, Android `strings.xml`, CSV, TSV)
- **CLI crate**: `langcodec-cli/` — end-user command-line tool built on the library
- **Binary name**: `langcodec`

## Philosophy

Agents must reason from first principles. Do not rely on conventions, copied patterns, or assumptions without verification. Every task should begin by identifying the fundamental facts, constraints, and invariants of the system (e.g., API contracts, type rules, data models, performance limits). Decompose problems until they reach irreducible components, then derive solutions logically from those facts. Prefer the simplest design that satisfies all constraints, and explicitly verify assumptions using available evidence (code, documentation, tests, or tools). Avoid guesswork, pattern imitation, or speculative implementations. Solutions should be the result of facts → constraints → reasoning → implementation.

## Repository layout (key paths)

- `langcodec/src/lib.rs`: library entry; re-exports `Codec`, `convert_auto`, `FormatType`, `types::*`
- `langcodec/src/formats/`: parsers/writers for `strings`, `stringsdict`, `xcstrings`, `xliff`, `android_strings`, `csv`, `tsv`
- `langcodec/src/traits.rs`: `Parser` trait for format-agnostic IO
- `langcodec-cli/src/main.rs`: CLI entry with subcommands
- `langcodec-cli/src/transformers/`: one-way converters for custom JSON/YAML formats
- `langcodec-cli/tests/` and `langcodec-cli/tests/fixtures/`: integration tests and sample inputs

## Prerequisites

- Rust toolchain with Edition 2024 support (install via rustup)
- macOS/Linux shell environment for examples below

Optional tooling for contributors:

- `cargo fmt`, `cargo clippy`

## Absolute path convention in this guide

Replace `<repo>` with the absolute path to the repository root. Example:

```bash
REPO="/Users/wendell/Developer/oops-rs/langcodec"
```

When running commands non-interactively, prefer absolute paths like `"$REPO/target/release/langcodec"` and explicit input/output file paths.

## Build

```bash
cd "$REPO"
cargo build --release -p langcodec-cli
```

- Output binary: `"$REPO/target/release/langcodec"`

Build just the library:

```bash
cargo build --release -p langcodec
```

## Test

```bash
cd "$REPO"
cargo test --all
```

Run only CLI tests:

```bash
cargo test -p langcodec-cli
```

## CLI quick reference

Binary: `"$REPO/target/release/langcodec"`

- `convert`: Convert localization files between formats (auto-detect by extension)
- `edit set`: Add/update/remove entries in-place (or to `--output`)
- `diff`: Compare two localization files and report added/removed/changed keys
- `sync`: Sync existing entries from a source file into a target file
- `view`: Pretty-print entries, filter by `--lang`, optional `--check-plurals`
- `merge`: Merge multiple inputs to one output with conflict strategy
- `normalize`: Normalize files and optionally fail on drift with `--check`
- `check`: Validate files for CI without modifying them
- `stats`: Coverage and per-status counts (text or `--json`)
- `debug`: Read file and emit JSON (to stdout or `--output`)
- `completions`: Generate shell completion scripts

Show help for any subcommand:

```bash
"$REPO/target/release/langcodec" --help | cat
"$REPO/target/release/langcodec" convert --help | cat
```

## Supported formats

Standard (read/write):

- **Apple**: `.strings`, `.stringsdict` (XML/binary input and canonical XML plural output), `.xcstrings`, `.xliff`
- **Android**: `strings.xml`
- **CSV**, **TSV**

Custom inputs (one-way into internal Resources via CLI):

- `json-language-map`, `json-array-language-map`, `yaml-language-map`, `langcodec-resource-array` (`.langcodec`)

## Common automation recipes (absolute paths)

- Convert `.strings` → Android XML:

```bash
"$REPO/target/release/langcodec" convert \
  --input "/abs/path/Localizable.strings" \
  --output "/abs/path/values/strings.xml"
```

- Convert `.xcstrings` → CSV:

```bash
"$REPO/target/release/langcodec" convert \
  --input "/abs/path/Localizable.xcstrings" \
  --output "/abs/path/translations.csv"
```

- Convert a standalone `.stringsdict` whose filename does not identify its
  locale:

```bash
"$REPO/target/release/langcodec" convert \
  --input "/abs/path/Localizable.catalog" \
  --input-format stringsdict \
  --source-language en \
  --output "/abs/path/values-en/strings.xml"
```

- Convert custom JSON language map → `.xcstrings` with overrides:

```bash
"$REPO/target/release/langcodec" convert \
  --input "/abs/path/translations.json" \
  --output "/abs/path/Localizable.xcstrings" \
  --input-format json-language-map \
  --output-format xcstrings \
  --source-language en \
  --version 1.0
```

- Edit in place (add/update). For single-language formats, specify `--lang` as needed:

```bash
"$REPO/target/release/langcodec" edit set \
  --inputs "/abs/path/en.lproj/Localizable.strings" \
  --lang en \
  --key welcome_message \
  --value "Hello, World!"
```

- Remove a key (omit or empty `--value`):

```bash
"$REPO/target/release/langcodec" edit set \
  --inputs "/abs/path/values/strings.xml" \
  --lang en \
  --key obsolete_key \
  --value ""
```

- Preview changes without writing:

```bash
"$REPO/target/release/langcodec" edit set \
  --inputs "/abs/path/en.lproj/Localizable.strings" \
  --lang en \
  --key welcome_message \
  --value "Hello" \
  --dry-run
```

- View entries (full values) and check plurals:

```bash
"$REPO/target/release/langcodec" view \
  --input "/abs/path/Localizable.xcstrings" \
  --lang en \
  --full \
  --check-plurals
```

- Validate files without writing:

```bash
"$REPO/target/release/langcodec" check \
  --inputs "/abs/path/**/Localizable.{strings,stringsdict,xcstrings}" \
  --continue-on-error \
  --json
```

- Merge multiple files (quote globs to avoid shell-side expansion):

```bash
"$REPO/target/release/langcodec" merge \
  --inputs "/abs/path/**/Localizable.strings" \
  --output "/abs/path/merged.xcstrings" \
  --strategy last \
  --lang en \
  --source-language en \
  --version 1.0
```

- Stats (machine-readable):

```bash
"$REPO/target/release/langcodec" stats \
  --input "/abs/path/Localizable.xcstrings" \
  --lang en \
  --json
```

- Debug (emit JSON to file):

```bash
"$REPO/target/release/langcodec" debug \
  --input "/abs/path/values/strings.xml" \
  --lang en \
  --output "/abs/path/out.json"
```

## Exit codes (for CI/non-interactive use)

- `0`: success
- `1`: validation or runtime failure (including issues reported by `check`)
- `2`: plural validation failed (when `view --check-plurals` is used)

## Behavior notes for agents

- All commands are non-interactive. Always pass explicit absolute paths.
- Input/output formats are inferred from file extensions unless `--input-format` / `--output-format` is provided.
- For single-language formats, pass `--lang` when required (e.g., ambiguous inputs).
- For `convert`, use `--source-language` as the input language hint when a single-language `.strings`, `.stringsdict`, or Android XML path does not supply one; other commands use their documented `--lang` flag.
- An explicit standard `convert --input-format` is authoritative even when the input extension is missing or disagrees.
- Writing `.stringsdict` from a generic `Resource` requires all three structural entry custom keys (`stringsdict.localized_format`, `stringsdict.variable_name`, `stringsdict.value_type`); never infer the selector from printf occurrences.
- Basic CSV/TSV uses the wide `key,<language>...` schema. Conversion automatically selects deterministic `__langcodec_extended_v1` output when plurals or metadata would otherwise be lost.
- Locale identity matching normalizes case and `_`/`-` spelling but keeps script and region variants distinct. A bare language may match a qualified variant only when catalog/path context makes the match unique.
- `check` compares normalized placeholder signatures only for singular translations sharing a domain and key; plural branches receive CLDR completeness checks only.
- Path-based `Parser` writes use a same-directory temporary file and atomic replacement. They preserve Unix mode bits but not ownership, ACLs, or extended attributes; symlink referents are replaced in place and Unix hard-linked destinations are rejected.
- Quote glob patterns provided to `merge --inputs` to avoid slow shell-side expansion.

## Library usage (Rust)

The library exposes a high-level API. Minimal example:

```rust
use langcodec::convert_auto;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    convert_auto("/abs/path/Localizable.strings", "/abs/path/values/strings.xml")?;
    Ok(())
}
```

Builder pattern and direct `Codec` manipulation are also available; see `langcodec/src/lib.rs` for more examples and re-exports.

## Extension points (for contributors/agents)

- Add/modify formats: edit files under `langcodec/src/formats/` and wire into `langcodec/src/formats.rs`
- Implement parsing/writing: implement `Parser` in `langcodec/src/traits.rs`
- Add CLI subcommands/options: edit `langcodec-cli/src/main.rs` and corresponding modules
- Support new custom one-way formats: add a transformer under `langcodec-cli/src/transformers/` and register in `langcodec-cli/src/transformers.rs` and `langcodec-cli/src/formats.rs`

## Reproducible CI example

```bash
set -euo pipefail
REPO="/abs/path/to/langcodec"
cargo build --release -p langcodec-cli --manifest-path "$REPO/Cargo.toml"
"$REPO/target/release/langcodec" --version | cat
"$REPO/target/release/langcodec" check \
  --inputs "/abs/path/Localizable.xcstrings" \
  --json
"$REPO/target/release/langcodec" convert \
  --input "/abs/path/Localizable.xcstrings" \
  --output "/abs/path/translations.csv"
```
