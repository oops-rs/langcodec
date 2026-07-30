# Contributing to langcodec

Thank you for your interest in contributing to langcodec! This document provides guidelines for contributing to the project.

## Getting Started

### Prerequisites

- Rust 1.85 or later (Edition 2024 compatible)
- Git

### Development Setup

**1. Fork and clone the repository:**

```bash
git clone https://github.com/oops-rs/langcodec.git
cd langcodec
```

**2. Install dependencies and run tests:**

```bash
cargo test
cargo clippy --all-targets --all-features
```

## Development Guidelines

### Code Style

- Follow Rust's official style guide
- Use `cargo fmt` to format code
- Use `cargo clippy` to check for common issues
- Write comprehensive tests for new features

### Adding New Formats

To add support for a new localization format:

1. Create a new module in `langcodec/src/formats/`
2. Implement the `Parser` trait for your format
3. Add `From`/`TryFrom` conversions to/from `Resource`
4. Update `FormatType` in `langcodec/src/formats.rs`
5. Add tests in the appropriate test module

### Testing

- Write unit tests for all new functionality
- Add integration tests for format conversions
- Include test data files in `tests/data/`
- Ensure all tests pass before submitting PR

### Documentation

- Document all public APIs
- Update README.md for new features
- Add examples in doc comments
- Update the format support table in README.md

## Commit message style

Follow the Conventional Commits specification to keep history readable and enable automated changelog tooling. See the official docs: <https://www.conventionalcommits.org/en/v1.0.0/>.

- Use the format: `<type>(<scope>): <subject>`
- type: feat, fix, docs, style, refactor, perf, test, build, ci, chore, revert
- scope (optional): one of lib, cli, formats, formats/android, formats/strings, formats/stringsdict, formats/xcstrings, formats/xliff, formats/csv, formats/tsv, transformers, validation, docs, tests
- subject: imperative mood, lowercase, ≤ 72 characters, no trailing period
- Separate body from subject with a blank line; explain motivation and impact
- Reference issues in the body or footer (e.g., Refs #123 or Fixes #123)
- Breaking changes: use `feat!:` or `fix!:` in the subject, and include a `BREAKING CHANGE:` footer

Examples:

```text
feat(cli): add --check_plurals to view command

fix(formats/android): escape apostrophes correctly in strings.xml

refactor(lib): simplify plural rules evaluation

chore: update dependencies

feat!: replace plural category API

feat(cli): add stats --json output

Provide machine-readable stats for CI consumers and human-readable summary by default.

BREAKING CHANGE: stats subcommand no longer prints totals by default; use --json.
```

## Submitting Changes

**1. Create a feature branch:**

 ```bash
 git checkout -b feature/your-feature-name
 ```

**2. Make your changes and commit:**

 ```bash
 git add .
 git commit -m "feat: brief description of changes"
 ```

**3. Push and create a pull request:**

 ```bash
 git push origin feature/your-feature-name
 ```

## Issue Reporting

When reporting issues:

1. Use the issue template
2. Include steps to reproduce
3. Provide sample input files if relevant
4. Include error messages and stack traces
5. Specify your Rust version and platform

## Code Review Process

1. All changes require review
2. Address review comments promptly
3. Ensure CI checks pass
4. Update documentation as needed

## Release Process

1. Update version numbers in `Cargo.toml` files
2. Update CHANGELOG.md
3. Create and push a release tag like `v0.10.0`
4. GitHub Actions will build release binaries and attach them to the GitHub Release automatically
5. Publish to crates.io

Thank you for contributing to langcodec!
