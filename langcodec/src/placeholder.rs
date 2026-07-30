//! Placeholder parsing, normalization and validation utilities.
//!
//! Goals:
//! - Normalize common iOS vs Android placeholder variants to a canonical form.
//! - Extract a placeholder "signature" for comparison across languages.
//! - Preserve printf argument identity, including positional and dynamic arguments.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceholderToken {
    /// One-based explicit argument index. During extraction, index `0` marks
    /// Java Formatter's `<` reuse of a prior unindexed argument; [`signature`]
    /// resolves that marker to the consumed argument's canonical index.
    pub index: Option<usize>,
    /// Canonical conversion kind (`s`, `d`, `f`, etc.). `*` represents an
    /// argument consumed by dynamic width or precision.
    pub kind: char,
}

impl PlaceholderToken {
    pub fn to_signature(&self) -> String {
        match self.index {
            Some(i) => format!("{}${}", i, self.kind),
            None => format!("{}", self.kind),
        }
    }
}

/// Extracts printf-style argument tokens in argument-consumption order.
///
/// Recognizes positional arguments, flags, static or dynamic width, precision,
/// and the `hh`, `h`, `l`, `ll`, `j`, `z`, `t`, and `L` length modifiers.
/// Android/Java Formatter additions are also recognized: boolean/hash `b`/`h`,
/// comma/parentheses/previous-argument flags, and `t`/`T` plus a date/time
/// suffix (canonically represented as kind `t`). In the dialect-ambiguous
/// `%td` shape, C's `t` length modifier takes precedence; Java date/time is
/// recognized only for unambiguous suffixes such as `%tY`. Bare `%n` is treated
/// as Java Formatter's non-consuming newline conversion. An explicitly indexed
/// `%1$n` remains a consuming C-style conversion.
/// Static formatting controls do not consume arguments. Dynamic `*` width and
/// precision do, so each is represented by a `PlaceholderToken` with kind `*`.
/// Escaped percent `%%` consumes no argument.
pub fn extract_placeholders(input: &str) -> Vec<PlaceholderToken> {
    const FLAGS: &[u8] = b"-+ #0',(<";
    const CONVERSIONS: &[u8] = b"bBhHdiuoxXfFeEgGaAcCsSpn@";
    const DATE_TIME_SUFFIXES: &[u8] = b"HIklMSLNpzZsQBbhAaCYyjmdeRTrDFc";

    let bytes = input.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    let mut previous_value_index: Option<Option<usize>> = None;

    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        // Handle escaped percent
        if i + 1 < bytes.len() && bytes[i + 1] == b'%' {
            i += 2;
            continue;
        }
        // Java Formatter's bare `%n` emits a platform newline and consumes no
        // argument. Preserve explicitly indexed `%1$n` as the C-style
        // consuming conversion because its argument identity is unambiguous.
        if bytes.get(i + 1) == Some(&b'n') {
            i += 2;
            continue;
        }

        let mut cursor = i + 1;
        let value_index = parse_positional_index(bytes, &mut cursor);
        let mut dynamic_arguments = Vec::new();
        let mut reuse_previous = false;

        while cursor < bytes.len() && FLAGS.contains(&bytes[cursor]) {
            reuse_previous |= bytes[cursor] == b'<';
            cursor += 1;
        }

        if bytes.get(cursor) == Some(&b'*') {
            cursor += 1;
            let width_index = parse_positional_index(bytes, &mut cursor);
            dynamic_arguments.push(PlaceholderToken {
                index: width_index,
                kind: '*',
            });
        } else {
            while bytes.get(cursor).is_some_and(|byte| byte.is_ascii_digit()) {
                cursor += 1;
            }
        }

        if bytes.get(cursor) == Some(&b'.') {
            cursor += 1;
            if bytes.get(cursor) == Some(&b'*') {
                cursor += 1;
                let precision_index = parse_positional_index(bytes, &mut cursor);
                dynamic_arguments.push(PlaceholderToken {
                    index: precision_index,
                    kind: '*',
                });
            } else {
                while bytes.get(cursor).is_some_and(|byte| byte.is_ascii_digit()) {
                    cursor += 1;
                }
            }
        }

        let java_simple_conversion = match bytes.get(cursor).copied() {
            Some(b'b' | b'B') => Some('b'),
            Some(b'H') => Some('h'),
            Some(b'h')
                if !(bytes
                    .get(cursor + 1)
                    .is_some_and(|conversion| b"diuoxXn".contains(conversion))
                    || bytes.get(cursor + 1) == Some(&b'h')
                        && bytes
                            .get(cursor + 2)
                            .is_some_and(|conversion| b"diuoxXn".contains(conversion))) =>
            {
                Some('h')
            }
            _ => None,
        };

        let conversion = if let Some(kind) = java_simple_conversion {
            Some(kind)
        } else if bytes
            .get(cursor)
            .is_some_and(|byte| matches!(byte, b't' | b'T'))
            && bytes
                .get(cursor + 1)
                .is_some_and(|suffix| DATE_TIME_SUFFIXES.contains(suffix))
            && !(bytes.get(cursor) == Some(&b't')
                && bytes
                    .get(cursor + 1)
                    .is_some_and(|suffix| b"diuoxXn".contains(suffix)))
        {
            cursor += 1;
            Some('t')
        } else {
            if bytes.get(cursor..).is_some_and(|remaining| {
                remaining.starts_with(b"hh") || remaining.starts_with(b"ll")
            }) {
                cursor += 2;
            } else if bytes
                .get(cursor)
                .is_some_and(|byte| b"hljztL".contains(byte))
            {
                cursor += 1;
            }

            bytes
                .get(cursor)
                .copied()
                .filter(|conversion| CONVERSIONS.contains(conversion))
                .map(|conversion| canonical_kind_char(conversion as char))
        };

        if let Some(kind) = conversion {
            let effective_index = if reuse_previous {
                if value_index.is_some() {
                    i += 1;
                    continue;
                }
                match previous_value_index {
                    Some(Some(index)) => Some(index),
                    Some(None) => Some(0),
                    None => {
                        i += 1;
                        continue;
                    }
                }
            } else {
                value_index
            };

            out.extend(dynamic_arguments);
            out.push(PlaceholderToken {
                index: effective_index,
                kind,
            });
            i = cursor + 1;
            if !reuse_previous {
                previous_value_index = Some(value_index);
            }
            continue;
        }

        // Not a recognized placeholder; skip this '%'
        i += 1;
    }

    out
}

fn parse_positional_index(bytes: &[u8], cursor: &mut usize) -> Option<usize> {
    let start = *cursor;
    while bytes.get(*cursor).is_some_and(|byte| byte.is_ascii_digit()) {
        *cursor += 1;
    }
    if *cursor == start || bytes.get(*cursor) != Some(&b'$') {
        *cursor = start;
        return None;
    }

    let index = std::str::from_utf8(&bytes[start..*cursor])
        .ok()
        .and_then(|value| value.parse().ok());
    *cursor += 1;
    index
}

/// Normalize a string by converting iOS-specific tokens to canonical ones.
/// - %@  -> %s
/// - %1$@ -> %1$s
/// - %ld, %lu -> %d / %u
pub fn normalize_placeholders(input: &str) -> String {
    // Replace positional iOS object placeholders %<n>$@ -> %<n>$s
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut tmp = String::with_capacity(input.len());
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let mut j = i + 1;
            let start_digits = j;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > start_digits && j + 1 < bytes.len() && bytes[j] == b'$' && bytes[j + 1] == b'@' {
                // Copy prefix, then normalized token
                tmp.push('%');
                tmp.push_str(&input[start_digits..j]); // digits
                tmp.push('$');
                tmp.push('s');
                i = j + 2;
                continue;
            }
        }
        // Copy the next full UTF-8 character, not just one byte
        let ch = input[i..]
            .chars()
            .next()
            .expect("valid UTF-8 slicing while scanning placeholders");
        tmp.push(ch);
        i += ch.len_utf8();
    }

    // Simple iOS object -> string
    let out = tmp.replace("%@", "%s");
    // Long ints to canonical
    let out = out.replace("%ld", "%d");

    out.replace("%lu", "%u")
}

/// Convert canonical/Android-style string placeholders to iOS-style.
/// - %s   -> %@
/// - %1$s -> %1$@
///   Leaves numeric specifiers (e.g., %d, %u, %ld) unchanged.
pub fn to_ios_placeholders(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut i = 0usize;
    let mut out = String::with_capacity(input.len());
    while i < bytes.len() {
        if bytes[i] != b'%' {
            // Copy the next full UTF-8 character
            let ch = input[i..]
                .chars()
                .next()
                .expect("valid UTF-8 slicing while converting placeholders");
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        // Escaped percent '%%'
        if i + 1 < bytes.len() && bytes[i + 1] == b'%' {
            out.push('%');
            out.push('%');
            i += 2;
            continue;
        }

        // Examine potential placeholder
        let mut j = i + 1;
        // Optional positional index digits+$
        let start_digits = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        let mut had_positional = false;
        if j > start_digits && j < bytes.len() && bytes[j] == b'$' {
            had_positional = true;
            j += 1; // skip '$'
        } else {
            // reset if not positional
            j = i + 1;
        }

        // Optional length modifiers (l/ll). We will drop them when converting %s -> %@.
        let mut k = j;
        while k < bytes.len() && bytes[k] == b'l' {
            k += 1;
        }
        if k >= bytes.len() {
            // not a complete placeholder, copy '%' and advance
            out.push('%');
            i += 1;
            continue;
        }

        let ty = bytes[k] as char;
        if ty == 's' {
            // Emit converted iOS placeholder
            out.push('%');
            if had_positional {
                // copy the digits we saw
                out.push_str(
                    &input[start_digits..(if start_digits < j {
                        j - 1
                    } else {
                        start_digits
                    })],
                );
                out.push('$');
            }
            out.push('@');
            i = k + 1;
            continue;
        }

        // Not a string placeholder, emit '%' and advance one byte; the rest will be handled in next iterations
        out.push('%');
        i += 1;
    }
    out
}

/// Builds a normalized printf argument signature.
///
/// A wholly implicit consumption sequence is assigned the equivalent one-based
/// argument identities, so `%@ %d` and `%1$s %2$d` have the same signature.
/// Fully positional signatures are sorted by identity because textual
/// occurrence order may legitimately change. Genuinely mixed explicit/implicit
/// signatures retain their bare implicit tokens and occurrence order, keeping
/// that ambiguous contract distinct from either wholly addressed form.
pub fn signature(input: &str) -> Vec<String> {
    let mut tokens = extract_placeholders(input);
    let has_explicit = tokens
        .iter()
        .any(|token| token.index.is_some_and(|index| index > 0));
    let has_implicit = tokens.iter().any(|token| token.index.is_none());

    if !has_explicit {
        canonicalize_implicit_indices(&mut tokens, true);
        tokens.sort_by_key(|token| (token.index, token.kind));
    } else if !has_implicit && tokens.iter().all(|token| token.index != Some(0)) {
        tokens.sort_by_key(|token| (token.index, token.kind));
    } else {
        // Keep ordinary implicit tokens bare so a genuinely mixed format
        // cannot compare equal to a wholly implicit or wholly explicit one,
        // but resolve Java `<` markers to the prior implicit identity.
        canonicalize_implicit_indices(&mut tokens, false);
    }
    tokens.into_iter().map(|t| t.to_signature()).collect()
}

fn canonicalize_implicit_indices(tokens: &mut [PlaceholderToken], assign_all: bool) {
    let mut next_index = 1;
    let mut previous_implicit_index = None;

    for token in tokens {
        match token.index {
            None => {
                let index = next_index;
                next_index += 1;
                previous_implicit_index = Some(index);
                if assign_all {
                    token.index = Some(index);
                }
            }
            Some(0) => {
                token.index = previous_implicit_index;
            }
            Some(_) => {}
        }
    }
}

fn canonical_kind_char(ch: char) -> char {
    match ch {
        '@' => 's',
        // Map uppercase to lowercase for type letters where it matters
        c => c.to_ascii_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_android_and_ios() {
        let s = "Hello %1$@, you have %2$d items and %s extra";
        let sig = signature(s);
        assert_eq!(sig, vec!["1$s", "2$d", "s"]);
        let s = "你好,%s";
        let sig = signature(s);
        assert_eq!(sig, vec!["1$s"]);
    }

    #[test]
    fn test_normalize_ios_simple() {
        let s = "Value: %@ and number %ld";
        let n = normalize_placeholders(s);
        assert!(n.contains("%s"));
        assert!(n.contains("%d"));
        assert_eq!(signature(s), vec!["1$s", "2$d"]);
    }

    #[test]
    fn test_normalize_positional_object() {
        let s = "Hello %1$@";
        let n = normalize_placeholders(s);
        assert!(n.contains("%1$s"));
    }

    #[test]
    fn test_ignore_escaped_percent() {
        let s = "Discount: 50%% and value %d";
        let sig = signature(s);
        assert_eq!(sig, vec!["1$d"]);
    }

    #[test]
    fn extracts_flags_width_precision_and_length_modifiers() {
        assert_eq!(signature("%02d %.2f %zu"), vec!["1$d", "2$f", "3$u"]);
        assert_eq!(signature("%1$.2f"), vec!["1$f"]);
        assert_eq!(
            signature("%hhd %hd %ld %lld %jd %tu %Lf"),
            vec!["1$d", "2$d", "3$d", "4$d", "5$d", "6$u", "7$f"]
        );
    }

    #[test]
    fn dynamic_width_and_precision_are_argument_tokens() {
        assert_eq!(signature("%*.*f"), vec!["1$*", "2$*", "3$f"]);
        assert_eq!(signature("%3$*1$.*2$f"), vec!["1$*", "2$*", "3$f"]);
    }

    #[test]
    fn positional_arguments_compare_by_identity_not_occurrence() {
        assert_eq!(signature("%2$d %1$@"), signature("%1$s %2$d"));
        assert_ne!(signature("%d %s"), signature("%s %d"));
    }

    #[test]
    fn wholly_implicit_arguments_equal_explicit_identity_sequence() {
        assert_eq!(signature("%@ %d"), signature("%1$s %2$d"));
        assert_eq!(signature("%@ %d"), vec!["1$s", "2$d"]);
    }

    #[test]
    fn mixed_addressing_remains_order_sensitive_and_distinct() {
        assert_ne!(signature("%1$s %d"), signature("%d %1$s"));
        assert_ne!(signature("%1$s %d"), signature("%1$s %2$d"));
    }

    #[test]
    fn extracts_android_java_boolean_hash_and_date_time_conversions() {
        assert_eq!(signature("%b %B %h %H"), vec!["1$b", "2$b", "3$h", "4$h"]);
        assert_eq!(signature("%2$Tm %1$tY"), vec!["1$t", "2$t"]);
        assert_eq!(signature("%tF %Tc"), vec!["1$t", "2$t"]);
    }

    #[test]
    fn unified_dialect_prefers_c_length_and_java_bare_newline() {
        assert_eq!(signature("%td %tY"), vec!["1$d", "2$t"]);
        assert_eq!(signature("first%nsecond %d"), vec!["1$d"]);
        assert_eq!(signature("%1$n"), vec!["1$n"]);
    }

    #[test]
    fn java_flags_do_not_hide_conversions_or_become_static_width() {
        assert_eq!(signature("%,12d %(08d"), vec!["1$d", "2$d"]);
        assert_eq!(signature("%#b %+d"), vec!["1$b", "2$d"]);
    }

    #[test]
    fn java_previous_argument_reuse_is_not_a_new_unindexed_argument() {
        assert_eq!(signature("%s %<S"), vec!["1$s", "1$s"]);
        assert_eq!(signature("%s %<S"), signature("%1$s %1$s"));
        assert_ne!(signature("%s %<s"), signature("%s %s"));
        assert_eq!(signature("%2$s %<S"), vec!["2$s", "2$s"]);
    }
}
