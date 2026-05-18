# langcodec (Library)

Universal localization file toolkit for Rust. Parse, write, convert, merge.

- Formats: Apple `.strings`, `.xcstrings`, `.xliff`, Android `strings.xml`, CSV, TSV
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
- `.xliff`: Apple/Xcode XLIFF 1.2 bilingual exchange files
- Android `strings.xml`: `<plurals>` supported (one/two/few/many/other/zero)

## Error Handling

All APIs return `langcodec::Error` with variants for parse, I/O, validation, conversion.

## License

MIT
