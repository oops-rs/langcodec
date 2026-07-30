# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project follows Semantic Versioning.

## [Unreleased]

### Added

- Added XML and binary `.stringsdict` parsing and canonical XML writing for one
  bare `NSStringPluralRuleType` selector per key. Wrapper text, nested or
  select/gender rules, and multiple variables remain unsupported.
- Added the read-only `check` command for parsing, normalized locale identity,
  resource structure, CLDR plural completeness, and normalized placeholder
  consistency across singular translations sharing a domain and key. Plural
  branches are not placeholder-compared because `Resource` does not identify
  the quantity argument.
- Added the deterministic `__langcodec_extended_v1` CSV/TSV schema. It is
  selected automatically when the conventional wide schema cannot preserve the
  complete `Resource` model.

### Changed

- Generic `Resource` output to `.stringsdict` now requires all three structural
  `stringsdict.*` entry custom fields and rejects unsupported metadata instead
  of guessing a selector from plural-form text.
- Standalone single-language conversion now uses `--source-language` as the
  input hint, and an explicit standard `--input-format` is authoritative even
  when the extension is absent or different. Explicit output format selections
  are likewise authoritative over destination extensions.
- Path-based parser reads and writes now retain caller-visible path context.
  Writes use a flushed and synced same-directory temporary file before atomic
  replacement. Unix mode bits are preserved, symlink referents are replaced
  without replacing the link, and Unix hard-linked destinations are rejected;
  ownership, ACLs, and extended attributes are not copied.
- Translation and Tolgee matching now normalize spelling while comparing the
  complete locale identity. Script and region variants remain distinct, bare
  language fallback must be unambiguous, and duplicate normalized identities
  are rejected.
- AI translation now rejects blank output and placeholder-corrupt output for
  each generated value before any destination is written, independently of
  parser strictness.
- **Breaking:** the public `Error` enum adds `Error::WithPath`; exhaustive
  downstream matches must handle it.
- **Breaking:** CSV and TSV `Format` values now contain private schema state.
  Downstream callers must use `new` or `with_records` instead of struct
  literals.

## [0.13.0] - 2026-05-18

### Added

- Added user-scoped Tolgee credential support with project-specific API key overrides.
- Added CLI coverage for Tolgee credential resolution through user config.

### Changed

- Serialized `.xcstrings` plural variations in a stable CLDR category order.

### Fixed

- Fixed Tolgee-related Clippy warnings so CI remains clean under denied warnings.

## [0.12.0] - 2026-04-15

### Added

- Added Apple/Xcode XLIFF 1.2 parsing, writing, and conversion support.
- Added an interactive TUI browser/editor for localization files.
- Added CLI coverage for XLIFF conversions and merge workflows.

### Changed

- Expanded the release workflow with manual tag dispatch and packaged binary uploads.
- Added Homebrew tap update automation after GitHub releases.
- Improved CLI editor navigation and UI behavior.

### Fixed

- Tightened single-language output handling in conversion flows.
- Skipped Tolgee sync for unmapped catalogs.
- Fixed release workflow secret handling and Clippy warnings.

## [0.11.0] - 2026-03-26

### Added

- Added CLI translate coverage for Apple `.strings` and Android `strings.xml` workflows.
- Extended `annotate` to support Apple `.strings` and Android XML inputs, not just `.xcstrings`.
- Added Android strings comment round-tripping support so comment metadata survives parse/write cycles.
- Added config-driven glob expansion coverage for `translate` and `annotate` command invocations.

### Changed

- Refreshed the root README and package presentation for the current CLI and library workflows.
- Updated the GitHub release workflow triggers and build matrix configuration.

### Fixed

- Fixed config-relative glob expansion for `translate` and `annotate` so `langcodec.toml` can target multiple matching files.
- Documented the broadened annotate format support in the user-facing docs.
