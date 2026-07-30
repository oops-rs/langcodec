# langcodec Roadmap

This document outlines progressive, bite‑sized tasks to enhance langcodec and langcodec‑cli. It’s structured so we can pick items incrementally and track progress over time.

Legend: [ ] todo, [x] done, [~] in progress

## Recently Completed

- [x] Android `<plurals>` parse/write support (library)
- [x] `.strings` writer escaping (quotes, backslashes, control chars)
- [x] Symmetric language matching for multi‑language formats (`xcstrings`, `csv`, `tsv`)
- [x] CLI view prints “Type: Plural” and plural categories
- [x] Conversion tests: CSV→Android, XCStrings→Android (with plurals)
- [x] CLI `stats` subcommand (per-language counts, completion %, JSON output)

---

## M1. Quality & Safety

- [x] Placeholder normalization and validation
  - [x] Mapping between iOS (`%1$@`, `%@`, `%ld`) and Android (`%1$s`, `%s`, `%d/%u`)
  - [x] Detect placeholder mismatches across languages; strict vs non‑strict modes
  - [x] Auto‑fix option for common cases (`normalize_placeholders_in_place`)
  - [x] Tests across singular and plural entries; cross‑language normalization
- [~] Plural rules engine
  - [x] CLDR‑driven required category sets per locale (few/many/etc.)
  - [x] Validation pass: flag missing categories per key+locale
  - [x] CLI: `view --check-plurals` output
- [~] Strict vs. permissive parsing
  - [x] Global setting in lib; CLI `--strict` flag
  - [ ] Consistent error surfaces with actionable context
- [~] Better error context
  - [x] Attach caller-visible file paths to path-based parser reads and writes
  - [ ] Include entry id for parse/convert errors where available
  - [ ] (Optional) capture line/column when parser knows it

## M2. Formats

- [~] Apple `.stringsdict`
  - [x] XML plist parse/write/convert for one bare `NSStringPluralRuleType` variable
  - [x] Binary plist input with canonical XML output
  - [x] Bare positional `%n$#@variable@` references
  - [x] Explicit selector identity for generic `Resource` output
  - [ ] Wrapper text, nested/select/gender rules, and multiple variables
- [ ] Flutter `.arb`
- [ ] Gettext `.po`
- [x] XLIFF 1.2
- [ ] XLIFF 2.0
- [ ] (Later) ICU MessageFormat v2 (exploration)

For each new format:

- [ ] Implement `Parser` and conversions to/from `Resource`
- [ ] Round‑trip tests + cross‑conversion tests
- [ ] CLI convert + view coverage
- [ ] README updates

## M3. CSV/TSV Schema

- [x] Automatically select a versioned extended schema when the wide schema would be lossy
- [x] Round-trip resource/entry order, plurals, comments, status, domains, and custom metadata
- [x] Deterministic output plus strict version/header/row validation
- [ ] CLI: `--schema` flag (e.g., `basic`, `extended`, custom mapping)
- [ ] User-defined column mappings

## M4. CLI UX

- [x] `diff` subcommand
  - [x] Compare two files; output added/removed/changed keys by language
  - [x] Machine‑readable JSON output and pretty mode
- [x] `stats` subcommand
  - [x] Per‑language counts by `EntryStatus`
  - [x] Completion percent (excludes DoNotTranslate)
  - [x] Missing plurals
- [x] `check` subcommand
  - [x] Read-only parse, locale identity, structure, and CLDR plural checks
  - [x] Normalized placeholder comparison for matching singular translations
  - [x] Deterministic text and JSON reports with implicated paths
- [x] `normalize` subcommand
  - [x] Placeholder normalization, optional key style, dry-run, and CI `--check`
- [ ] Filters and export
  - [ ] `view --where 'status=stale and lang in(en,fr)' --format csv`
  - [ ] `--grep` for key/value regex
- [ ] Stdio support: `-` for stdin/stdout across commands
- [~] Config file
  - [x] `langcodec.toml` for translate, annotate, and Tolgee workflows
  - [ ] General command defaults (merge strategy, tabular schema, placeholder policy)

## M5. Developer Experience

- [ ] API ergonomics
  - [ ] Borrowed iterators and helpers: `iter_keys()`, `iter_entries(lang)`
  - [ ] Mutators: `rename_key`, `bulk_rename`, `map_values`
- [ ] Deterministic ordering everywhere (keys, languages)
- [ ] Provenance tracking (source file, optional line) per entry
- [ ] Benchmarks (Criterion) for parse/convert/merge

## M6. Ecosystem & Distribution

- [ ] WASM target (browser/Node) for view/convert/diff in web tools
- [ ] GitHub Action templates
  - [ ] Validate PRs, enforce placeholder policy, fail on regressions
  - [ ] Example workflows in `.github/workflows/examples/`
- [ ] Documentation site
  - [ ] Task‑oriented guides (convert recipes, plural pitfalls, placeholder mapping)
  - [ ] API docs deep links; examples gallery

## Testing Strategy

- [x] Start with unit tests near each format parser/writer
- [x] Add conversion matrix tests for common paths (strings↔android↔xcstrings↔csv/tsv)
- [x] Property tests where feasible (e.g., round‑trip invariants)
- [ ] Large sample corpora in `tests/data/` for regression

## Contribution Guide Enhancements

- [x] Add coding standards and commit message conventions
- [x] Issue templates for formats vs CLI vs core
- [x] Local dev quickstart and common cargo commands

## Release Checklist (per minor)

- [ ] Update README Supported Formats table
- [ ] Update CHANGELOG.md highlights (breaking changes, new formats, CLI flags)
- [ ] Version bumps in workspace `Cargo.toml` and README
- [ ] Tag + GitHub release notes

---

If you pick up an item, feel free to mark it with [~] and open a PR referencing this roadmap.
