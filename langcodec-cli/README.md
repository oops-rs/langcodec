# langcodec-cli

`langcodec` is a localization CLI for teams shipping real apps, not demo files.

It handles the annoying parts of localization work in one place: format conversion, catalog cleanup, AI-assisted translation, translator-facing comment generation, and Tolgee sync for Apple string catalogs.

Supported formats:

- Apple `.strings`
- Apple `.stringsdict` (XML/binary read, canonical XML write, single-variable plural subset)
- Apple `.xcstrings`
- Apple/Xcode `.xliff`
- Android `strings.xml`
- CSV
- TSV

## Why It Feels Useful

Most localization tooling does one small thing. `langcodec` is designed to cover the loop teams actually run:

1. Convert strings between iOS, Android, and spreadsheet formats.
2. Inspect what is missing, stale, or still needs review.
3. Normalize files so diffs stop being noisy.
4. Draft translations with AI.
5. Generate better comments for translators from real source usage.
6. Pull from and push back to Tolgee without custom glue scripts.

## The Cool Stuff

### AI translation with real workflow support

```sh
langcodec translate \
  --source Localizable.xcstrings \
  --source-lang en \
  --target-lang fr,de,ja \
  --provider openai \
  --model gpt-5.4
```

`translate` is built for app catalogs, not just raw text:

- updates multi-language files like `.xcstrings` in place
- supports single-language Apple `.strings` and Android `strings.xml` files too
- supports multiple target languages in one run
- can prefill from Tolgee before using AI fallback
- shows live progress with `--ui auto|plain|tui`
- validates output before model requests
- prints a clear result summary at the end

Locale matching normalizes case and treats `_` and `-` as equivalent spelling,
but it compares the complete locale identity. `zh-Hans` is not `zh-Hant`, and
`pt-BR` is not `pt-PT`. A bare request such as `pt` resolves to a qualified
catalog locale only when the surrounding catalog or target path makes the
choice unambiguous; otherwise `--source-lang` or `--target-lang` must name the
fully qualified locale.
Duplicate normalized identities such as `fr-CA` and `fr_CA` are rejected.

### AI-generated translator comments

```sh
langcodec annotate \
  --input Localizable.xcstrings \
  --source-root Sources \
  --source-root Modules \
  --provider openai \
  --model gpt-5.4
```

`annotate` looks through your codebase and writes better translator comments for `.xcstrings`, Apple `.strings`, and Android `strings.xml` files while preserving manual comments.

```sh
langcodec annotate \
  --input en.lproj/Localizable.strings \
  --source-root Sources \
  --provider openai \
  --model gpt-5.4
```

### Tolgee sync without a pile of project scripts

```sh
langcodec tolgee pull
langcodec tolgee push --namespace WebGame
```

Tolgee support in v1 is intentionally focused on Apple `.xcstrings`. `langcodec.toml` can now be the source of truth, and `langcodec` will synthesize the Tolgee CLI JSON config at runtime.
Tolgee filtering and merge use the same complete normalized locale identity as
`translate`, so sibling script or region variants are never merged implicitly.

## Install

```sh
brew tap oops-rs/tap
brew install langcodec-cli
```

```sh
cargo install langcodec-cli
```

## Quick Start

Use the CLI help for exact flags:

```sh
langcodec --help
langcodec check --help
langcodec translate --help
langcodec annotate --help
langcodec tolgee --help
```

## Core Workflows

### Convert between ecosystems

```sh
langcodec convert -i Localizable.xcstrings -o translations.csv
langcodec convert -i en.lproj/Localizable.stringsdict -o values-en/strings.xml
langcodec convert -i translations.csv -o values/strings.xml
langcodec convert -i Localizable.xcstrings -o Localizable.xliff --output-lang fr
langcodec convert -i Localizable.xliff -o Localizable.xcstrings
```

For a single-language input whose path does not identify its locale, pass
`--source-language` as the input language hint:

```sh
langcodec convert \
  -i Localizable.catalog \
  -o values-en/strings.xml \
  --input-format stringsdict \
  --source-language en
```

An explicit standard `--input-format` reads that format regardless of the
input extension, including in `--strict` mode. For `.xliff` output, pass
`--output-lang` to choose the target language; `--source-language` continues
to select its source language.

Simple CSV and TSV files keep the conventional wide `key,<language>...`
layout. If a conversion contains plurals, comments, statuses, domains, custom
metadata, or other distinctions that the wide layout cannot represent,
langcodec automatically emits the deterministic `__langcodec_extended_v1`
schema. That schema round-trips the langcodec data model; it does not preserve
an input file's original whitespace or quoting.

### Find strings that still need work

```sh
langcodec view -i Localizable.xcstrings --status new,needs_review --keys-only
langcodec stats -i Localizable.xcstrings --json
```

### Validate files in CI

```sh
langcodec check \
  -i 'locales/**/*.{strings,stringsdict,xml,xcstrings,xliff,csv,tsv}' \
  --continue-on-error --json
```

`check` is read-only and exits non-zero for input/parse errors, invalid or
duplicate normalized locale identities, invalid resource structure, missing
locale-required plural categories, or placeholder inconsistencies. Placeholder
signatures are normalized and compared only across singular translations that
share the same domain and key. Plural branches receive CLDR category
completeness checks but are not placeholder-compared: the resource model does
not identify which printf argument is the plural quantity, and valid categories
may independently include or omit a displayed count. `check` does not add
missing keys, measure coverage, or modify files.

### Edit files without format-specific tooling

```sh
langcodec edit set -i en.strings -k welcome_title -v "Welcome"
langcodec edit set -i values/strings.xml -k welcome_title -v "Welcome"
```

### Normalize files for cleaner diffs

```sh
langcodec normalize -i 'locales/**/*.{strings,xml,csv,tsv,xcstrings}' --check
```

`normalize`, `edit`, and `sync` intentionally do not operate on `.xliff` in v1; convert XLIFF into a project format first.

`.stringsdict` can be converted, viewed, debugged, checked, and merged when
every entry remains representable. Scalar editing, interactive `browse`,
`normalize`, `annotate`, and `translate` reject it. It may be a `sync` source,
but not a target or output, because sync provenance is not representable.
Generic CLI inputs such as Android XML or `.xcstrings` cannot create a
`.stringsdict`: their plural model does not identify the printf argument that
drives quantity, and the CLI does not guess. Library callers can opt in by
setting all three structural `Entry.custom` keys documented by `langcodec`.

### Sync or merge existing translation assets

```sh
langcodec sync --source source.xcstrings --target target.xcstrings --match-lang en
langcodec merge -i a.xcstrings -i b.xcstrings -o merged.xcstrings --strategy last
```

## Example Config

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

[tolgee]
project_id = 36
api_url = "https://tolgee.example/api"
api_key = "tgpak_example"
namespaces = ["WebGame"]

[tolgee.push]
languages = ["en"]
force_mode = "KEEP"

[[tolgee.push.files]]
path = "locales/Localizable.xcstrings"
namespace = "WebGame"

[tolgee.pull]
path = "./tolgee-temp"
file_structure_template = "/{namespace}/Localizable.{extension}"

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

When exactly one provider section is configured, `translate` and `annotate` use it automatically. If you configure multiple providers, choose one with `--provider` or `translate.provider`.

For larger repos:

- use `translate.input.sources = [...]` to fan out translation runs
- use `annotate.inputs = [...]` to annotate multiple catalogs in place

## Main Commands

- `convert`: convert between localization formats
- `check`: validate files for CI without modifying them
- `view`: inspect entries, statuses, and keys
- `stats`: summarize coverage and completion
- `edit`: add, update, or remove entries
- `normalize`: rewrite files into a stable form
- `diff`: compare two localization files
- `sync`: update existing target entries from a source file
- `merge`: combine multiple inputs into one output
- `translate`: draft translations with AI-backed providers
- `tolgee`: pull and push mapped `.xcstrings` catalogs with Tolgee
- `annotate`: generate translator-facing comments for `.xcstrings`, `.strings`, and Android XML using AI-backed source lookup
- `debug`: inspect parsed output as JSON

## Best Fit

`langcodec` shines when you are:

- shipping both iOS and Android apps
- moving strings through translators, spreadsheets, and app catalogs
- trying to reduce localization drift in CI
- replacing fragile one-off scripts with one repeatable tool

## Related Docs

- Root overview: [README.md](../README.md)
- Rust library crate: [langcodec/README.md](../langcodec/README.md)

## License

MIT
