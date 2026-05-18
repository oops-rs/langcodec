<p align="center">
  <img src="./assets/langcodec-icon.svg" alt="langcodec" width="160" height="160" />
</p>

<h1 align="center">langcodec</h1>

<p align="center">
  Universal localization tooling for real product workflows.
</p>

<p align="center">
  Convert, inspect, normalize, translate, annotate, and sync localization assets across Apple, XLIFF, Android, CSV, TSV, and Tolgee-backed pipelines.
</p>

<p align="center">
  <a href="https://crates.io/crates/langcodec-cli">CLI</a> |
  <a href="https://crates.io/crates/langcodec">Library</a> |
  <a href="https://docs.rs/langcodec">docs.rs</a> |
  <a href="./langcodec-cli/README.md">CLI Guide</a> |
  <a href="./langcodec/README.md">Library Guide</a> |
  <a href="./CONTRIBUTING.md">Contributing</a>
</p>

<p align="center">
  <a href="https://github.com/oops-rs/langcodec/actions/workflows/rust.yml">
    <img src="https://img.shields.io/github/actions/workflow/status/oops-rs/langcodec/rust.yml?branch=main&label=ci&logo=github" alt="CI status" />
  </a>
  <a href="https://crates.io/crates/langcodec-cli">
    <img src="https://img.shields.io/crates/v/langcodec-cli?logo=rust" alt="langcodec-cli on crates.io" />
  </a>
  <a href="https://docs.rs/langcodec">
    <img src="https://img.shields.io/docsrs/langcodec?logo=docsdotrs" alt="langcodec docs.rs" />
  </a>
</p>

## Why langcodec?

Most localization workflows are a pile of one-off scripts, format-specific tools, spreadsheet exports, and CI glue. `langcodec` gives you one Rust-native toolkit for the loop teams actually run:

- move between Apple, Android, and tabular formats without losing structure
- inspect stale, missing, or incomplete strings before they ship
- normalize files so diffs stay readable in review and CI
- draft translations with AI-backed providers
- generate better translator comments from real source usage
- sync `.xcstrings` catalogs with Tolgee without custom release scripts

## Highlights

- Unified data model for singular and plural translations
- Read and write support for Apple `.strings`, Apple `.xcstrings`, Apple/Xcode `.xliff`, Android `strings.xml`, CSV, and TSV
- CLI commands for convert, diff, merge, sync, edit, normalize, view, stats, debug, translate, annotate, and Tolgee sync
- Config-driven AI workflows with `langcodec.toml`
- Rust library API for teams building custom localization pipelines

## Quick Start

Install the CLI:

```sh
brew tap oops-rs/tap
brew install langcodec-cli
```

```sh
cargo install langcodec-cli
```

Use the library:

```toml
[dependencies]
langcodec = "0.13.0"
```

Try the workflow:

```sh
# Convert Apple strings to Android XML
langcodec convert -i Localizable.strings -o values/strings.xml

# Export an Apple/Xcode translation exchange file
langcodec convert -i Localizable.xcstrings -o Localizable.xliff --output-lang fr

# Import XLIFF back into an Xcode string catalog
langcodec convert -i Localizable.xliff -o Localizable.xcstrings

# Inspect work that still needs attention
langcodec view -i Localizable.xcstrings --status new,needs_review --keys-only

# Normalize catalogs in CI
langcodec normalize -i 'locales/**/*.{strings,xml,csv,tsv,xcstrings}' --check

# Draft translations into an existing string catalog
langcodec translate \
  --source Localizable.xcstrings \
  --source-lang en \
  --target-lang fr,de,ja \
  --provider openai \
  --model gpt-5.4

# Draft translations between single-language files too
langcodec translate \
  --source en.lproj/Localizable.strings \
  --target values-fr/strings.xml \
  --source-lang en \
  --target-lang fr \
  --provider openai \
  --model gpt-5.4

# Generate translator-facing comments from source usage
langcodec annotate \
  --input Localizable.xcstrings \
  --source-root Sources \
  --source-root Modules \
  --provider openai \
  --model gpt-5.4

# Annotate Apple .strings or Android XML inline
langcodec annotate \
  --input en.lproj/Localizable.strings \
  --source-root Sources \
  --provider openai \
  --model gpt-5.4
```

## Packages

| Package                            | What it is             | Best for                                                                    |
| ---------------------------------- | ---------------------- | --------------------------------------------------------------------------- |
| [`langcodec`](./langcodec)         | Rust library crate     | Building custom localization tooling, validation, and conversions in Rust   |
| [`langcodec-cli`](./langcodec-cli) | Command-line interface | Day-to-day conversion, cleanup, translation, annotation, and sync workflows |

## Format Support

| Format                | Parse | Write | Convert | Merge | Plurals | Comments |
| --------------------- | :---: | :---: | :-----: | :---: | :-----: | :------: |
| Apple `.strings`      |  yes  |  yes  |   yes   |  yes  |   no    |   yes    |
| Apple `.xcstrings`    |  yes  |  yes  |   yes   |  yes  |   yes   |   yes    |
| Apple `.xliff`        |  yes  |  yes  |   yes   |   no  |   no    |   yes    |
| Android `strings.xml` |  yes  |  yes  |   yes   |  yes  |   yes   |   yes    |
| CSV                   |  yes  |  yes  |   yes   |  yes  |   no    |    no    |
| TSV                   |  yes  |  yes  |   yes   |  yes  |   no    |    no    |

## AI Workflows

`langcodec` is built for app localization workflows, not just isolated text snippets. `translate` and `annotate` can be driven from a shared `langcodec.toml`, use supported providers such as OpenAI, Anthropic, and Gemini, and scale from single-language files or `.xcstrings` catalogs to config-driven runs across larger repos.

```toml
[openai]
model = "gpt-5.4"

[translate]
concurrency = 4
use_tolgee = true

[translate.input]
source = "locales/Localizable.xcstrings"
lang = "en"
status = ["new", "stale"]

[translate.output]
lang = ["fr", "de"]
status = "translated"

[annotate]
input = "locales/Localizable.xcstrings"
source_roots = ["Sources", "Modules"]
concurrency = 4
```

Then run:

```sh
langcodec translate
langcodec annotate
langcodec tolgee pull
```

For deeper CLI examples, head to [langcodec-cli/README.md](./langcodec-cli/README.md).

## Rust API

```rust
use langcodec::{Codec, convert_auto};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    convert_auto("Localizable.strings", "strings.xml")?;

    let mut codec = Codec::new();
    codec.read_file_by_extension("Localizable.xcstrings", None)?;

    for language in codec.languages() {
        println!("{language}");
    }

    Ok(())
}
```

The library is a good fit if you want to build custom pipelines, validate assets in CI, or work with a consistent representation instead of format-specific parsers.

## Documentation

- [CLI guide](./langcodec-cli/README.md)
- [Library guide](./langcodec/README.md)
- [Contribution guide](./CONTRIBUTING.md)
- [Project roadmap](./ROADMAP.md)

## License

[MIT](./LICENSE)
