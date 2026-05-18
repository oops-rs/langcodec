# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project follows Semantic Versioning.

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
