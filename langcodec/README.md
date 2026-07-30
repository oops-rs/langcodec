# langcodec (Library)

Universal localization file toolkit for Rust. Parse, write, convert, merge.

- Formats: Apple `.strings`, `.stringsdict`, `.xcstrings`, `.xliff`, Android `strings.xml`, CSV, TSV
- Unified model: `Resource` with `Entry`, `Translation::Singular|Plural`
- Robust error type, utilities to infer format/language, merge, cache

## Install

```toml
[dependencies]
langcodec = "0.13.0"
```

Docs: <https://docs.rs/langcodec>

## Quick Start

```rust
use langcodec::{Codec, convert_auto};

// Convert between formats automatically
convert_auto("Localizable.strings", "strings.xml")?;

// Build an XLIFF exchange file (target language must be selected explicitly)
langcodec::convert_resources_to_format(
    vec![langcodec::Resource {
        metadata: langcodec::Metadata {
            language: "en".into(),
            domain: "Localizable".into(),
            custom: std::collections::HashMap::from([(
                "source_language".into(),
                "en".into(),
            )]),
        },
        entries: vec![],
    }],
    "Localizable.xliff",
    langcodec::FormatType::Xliff(Some("fr".into())),
)?;

// Load, inspect, and write
let mut codec = Codec::new();
codec.read_file_by_extension("en.lproj/Localizable.strings", None)?;
codec.write_to_file()?;
# Ok::<(), langcodec::Error>(())
```

### Builder Pattern

```rust
use langcodec::Codec;

let codec = Codec::builder()
  .add_file("en.lproj/Localizable.strings")?
  .add_file("values/strings.xml")?
  .build();
# Ok::<(), langcodec::Error>(())
```

### Work with Entries

```rust
use langcodec::{Codec, types::{Translation, EntryStatus}};
let mut codec = Codec::new();
codec.add_entry("welcome", "en", Translation::Singular("Hello".into()), None, None)?;
codec.update_translation("welcome", "en", Translation::Singular("Hello!".into()), Some(EntryStatus::Translated))?;
# Ok::<(), langcodec::Error>(())
```

## Conversion Helpers

- `convert(input, input_format, output, output_format)`
- `convert_auto(input, output)`
- `infer_format_from_path`, `infer_language_from_path`

## Plurals

- `.xcstrings`: plural variations supported via CLDR categories
- `.stringsdict`: XML and binary plist input with canonical XML output, limited to one bare `NSStringPluralRuleType` variable per key
- `.xliff`: Apple/Xcode XLIFF 1.2 bilingual exchange files
- Android `strings.xml`: `<plurals>` supported (one/two/few/many/other/zero)

The `.stringsdict` codec implements the lossless intersection with
`Resource`: plural-only entries, one variable, and a bare
`%#@variable@` or `%n$#@variable@` selector. It rejects wrapper text,
select/gender or nested rules, multiple variables, non-representable entry
metadata, and unsupported plist value kinds. A language is inferred from an
`*.lproj` directory or must be supplied by the caller. Binary input is
accepted, while all writes use canonical XML.

`Format::try_from(Resource)` requires every plural entry to provide
`stringsdict.localized_format`, `stringsdict.variable_name`, and
`stringsdict.value_type` in `Entry.custom`. These fields explicitly identify
the Apple selector. They are mandatory because `Resource` plural forms do not
otherwise say which printf argument controls quantity; inferring from a
numeric placeholder could select an unrelated value. `Resource`s parsed from
`.stringsdict` receive the three fields automatically.

`Resource.metadata.domain` is container/path metadata and is not encoded in a
`.stringsdict` file. Known operational resource metadata (`source_language`,
`version`, `format`, and langcodec provenance keys) is also container-only;
unknown resource metadata and all non-structural entry custom metadata are
rejected.

Binary input is materialized through `plist::Value`. Duplicate dictionary keys
in an adversarial binary plist therefore cannot be detected after the plist
library has materialized the dictionary.

## CSV and TSV Schemas

CSV and TSV support two schemas. The conventional wide
`key,<language>...` schema remains the default for simple singular catalogs.
Conversion automatically selects the versioned `__langcodec_extended_v1`
schema when the wide form would invent or discard `Resource` data.

The extended schema round-trips resource and entry order, languages, domains,
plural identifiers and forms, comments, statuses, custom metadata, and the
difference between `Translation::Empty` and an empty singular string.
Serialization is deterministic, including map-valued metadata. This preserves
the langcodec data model, not the original whitespace or quoting of an imported
file.

`CSVFormat` and `TSVFormat` now carry private schema state. Construct basic
values with `new` or `with_records`, inspect parsed values with `is_extended`,
and convert through `TryFrom<Vec<Resource>>` when automatic schema selection is
required.

## Error Handling

Fallible APIs return `langcodec::Error` with stable error codes and optional
structured context. Path-based `Parser::read_from` and `Parser::write_to`
operations attach the caller-visible path without changing the underlying error
category.

`Parser::write_to` creates missing parent directories, serializes to a temporary
file in the destination directory, flushes and syncs it, and then atomically
replaces the destination. On Unix, temporary content remains owner-only until
serialization succeeds, the destination mode is then applied, and the parent
directory is synced after replacement. Existing symlinks remain in place while
their referent is replaced; dangling symlinks and symlink loops are rejected.
Unix hard-linked destinations are also rejected rather than having their
identity silently broken.

These guarantees assume the destination directory is trusted and is not being
concurrently mutated by an adversary. A parent-directory sync failure can be
reported after replacement is already visible; in that case the new contents
are committed but crash durability is uncertain. Newly created ancestor
directories are not individually synced. Directory syncing and hard-link
protection are Unix-only.

Only file permissions are retained. Ownership, ACLs, extended attributes, and
other platform metadata are not copied. Callers that exhaustively match the
public `Error` enum must handle the new `Error::WithPath` variant.

## License

MIT
