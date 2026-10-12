//! Recognize and parse numeric filename prefixes.
//!
//! The supported grammar is:
//!
//! ```text
//! name      = primary [ suffix ] separator rest
//! primary   = 1*DIGIT
//! suffix    = "." 1*DIGIT        ; numeric ordering suffix, e.g. "03.1-"
//!           | 1*ALPHA            ; alphabetic ordering suffix, e.g. "02a-"
//! separator = "-" | "_"
//! rest      = 1*OCTET            ; anything, must be non-empty
//! ```
//!
//! `DIGIT` and `ALPHA` are ASCII only. Names that do not match are left alone.

use std::ffi::{OsStr, OsString};

/// An optional ordering suffix between the primary number and the separator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Suffix {
    /// A numeric suffix such as `.2`. Holds the digits without the dot.
    Numeric(String),
    /// An alphabetic suffix such as `a` or `B`.
    Alpha(String),
}

/// A name that matched the prefix grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedName {
    /// The full original name.
    pub original: OsString,
    /// Digits of the primary prefix, exactly as written (may have leading zeros).
    pub primary: String,
    /// The optional ordering suffix.
    pub suffix: Option<Suffix>,
    /// The separator, `-` or `_`.
    pub separator: char,
    /// Everything after the separator (never empty).
    pub rest: OsString,
}

impl ParsedName {
    /// Builds the normalized name for sequence number `seq` padded to `width` digits.
    pub fn renamed(&self, seq: u64, width: usize) -> OsString {
        let mut name = OsString::from(format!("{seq:0width$}{}", self.separator));
        name.push(&self.rest);
        name
    }
}

/// Parses `name`, returning `None` if it does not match the prefix grammar.
pub fn parse(name: &OsStr) -> Option<ParsedName> {
    let bytes = name.as_encoded_bytes();

    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let mut pos = digits;

    let suffix = match bytes.get(pos) {
        Some(b'.') => {
            let n = count(&bytes[pos + 1..], u8::is_ascii_digit);
            if n == 0 {
                return None;
            }
            let s = ascii(&bytes[pos + 1..pos + 1 + n]);
            pos += 1 + n;
            Some(Suffix::Numeric(s))
        }
        Some(b) if b.is_ascii_alphabetic() => {
            let n = count(&bytes[pos..], u8::is_ascii_alphabetic);
            let s = ascii(&bytes[pos..pos + n]);
            pos += n;
            Some(Suffix::Alpha(s))
        }
        _ => None,
    };

    let separator = match bytes.get(pos) {
        Some(b'-') => '-',
        Some(b'_') => '_',
        _ => return None,
    };
    pos += 1;

    if pos >= bytes.len() {
        return None;
    }

    // SAFETY: everything before `pos` is ASCII, so `pos` falls immediately
    // after an ASCII character, which is a valid split point for the
    // platform's encoded representation of `OsStr`.
    let rest = unsafe { OsStr::from_encoded_bytes_unchecked(&bytes[pos..]) }.to_os_string();

    Some(ParsedName {
        original: name.to_os_string(),
        primary: ascii(&bytes[..digits]),
        suffix,
        separator,
        rest,
    })
}

fn count(bytes: &[u8], pred: fn(&u8) -> bool) -> usize {
    bytes.iter().take_while(|b| pred(b)).count()
}

fn ascii(bytes: &[u8]) -> String {
    // Only ever called on runs of ASCII digits or letters.
    String::from_utf8(bytes.to_vec()).expect("ASCII bytes are valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str) -> Option<ParsedName> {
        parse(OsStr::new(name))
    }

    #[test]
    fn parses_plain_prefixes() {
        let n = p("1-introduction.md").unwrap();
        assert_eq!(n.primary, "1");
        assert_eq!(n.suffix, None);
        assert_eq!(n.separator, '-');
        assert_eq!(n.rest, "introduction.md");

        let n = p("01_introduction.md").unwrap();
        assert_eq!(n.primary, "01");
        assert_eq!(n.separator, '_');

        assert_eq!(p("100-final.txt").unwrap().primary, "100");
    }

    #[test]
    fn parses_alpha_suffixes() {
        let n = p("02a-example.rs").unwrap();
        assert_eq!(n.suffix, Some(Suffix::Alpha("a".into())));
        assert_eq!(n.rest, "example.rs");

        let n = p("02b_exercise.rs").unwrap();
        assert_eq!(n.suffix, Some(Suffix::Alpha("b".into())));
        assert_eq!(n.separator, '_');

        let n = p("5AB-x").unwrap();
        assert_eq!(n.suffix, Some(Suffix::Alpha("AB".into())));
    }

    #[test]
    fn parses_numeric_suffixes() {
        let n = p("03.1-first.txt").unwrap();
        assert_eq!(n.primary, "03");
        assert_eq!(n.suffix, Some(Suffix::Numeric("1".into())));
        assert_eq!(n.rest, "first.txt");

        let n = p("03.10_second.txt").unwrap();
        assert_eq!(n.suffix, Some(Suffix::Numeric("10".into())));
        assert_eq!(n.separator, '_');
    }

    #[test]
    fn keeps_rest_verbatim() {
        let n = p("07-archive.tar.gz").unwrap();
        assert_eq!(n.rest, "archive.tar.gz");
        let n = p("07--double").unwrap();
        assert_eq!(n.rest, "-double");
        let n = p("07-01-nested").unwrap();
        assert_eq!(n.rest, "01-nested");
    }

    #[test]
    fn rejects_non_matching_names() {
        for name in [
            "README.md",
            "notes-1.txt",
            "-1-x",
            "1",
            "1-",
            "1_",
            "1.txt",
            "1 x.txt",
            "1.-x",
            "1.a-x",
            "1.2.3-x",
            "1a1-x",
            "1a",
            "a1-x",
            ".1-x",
            "١-arabic-digit",
            "",
        ] {
            assert_eq!(p(name), None, "{name:?} should not match");
        }
    }

    #[test]
    fn renamed_preserves_separator_and_rest() {
        let n = p("7b_thing.tar.gz").unwrap();
        assert_eq!(n.renamed(3, 2), "03_thing.tar.gz");
        assert_eq!(n.renamed(42, 4), "0042_thing.tar.gz");
        assert_eq!(n.renamed(123, 2), "123_thing.tar.gz");
    }

    #[test]
    fn handles_unicode_rest() {
        let n = p("3-café ☕.txt").unwrap();
        assert_eq!(n.rest, "café ☕.txt");
    }

    #[cfg(unix)]
    #[test]
    fn handles_non_utf8_rest() {
        use std::os::unix::ffi::OsStrExt;
        let name = OsStr::from_bytes(b"4-\xff\xfe.bin");
        let n = parse(name).unwrap();
        assert_eq!(n.rest.as_bytes(), b"\xff\xfe.bin");
        assert_eq!(n.renamed(1, 2).as_bytes(), b"01-\xff\xfe.bin");
    }
}
