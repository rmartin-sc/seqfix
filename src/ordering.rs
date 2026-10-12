//! Ordering rules for parsed names.
//!
//! Entries are ordered by:
//!
//! 1. Primary number, compared as an integer.
//! 2. Within the same primary number: numeric suffixes first (by value),
//!    then alphabetic suffixes (case-insensitively, then uppercase before
//!    lowercase), then entries without a suffix.
//! 3. The remaining name, compared case-insensitively, then by exact bytes.
//! 4. Finally the full original name by exact bytes, so the order is total
//!    and deterministic.

use std::cmp::Ordering;
use std::ffi::OsStr;

use crate::parser::{ParsedName, Suffix};

/// Compares two parsed names according to the ordering rules.
pub fn compare(a: &ParsedName, b: &ParsedName) -> Ordering {
    compare_digits(&a.primary, &b.primary)
        .then_with(|| compare_suffix(a.suffix.as_ref(), b.suffix.as_ref()))
        .then_with(|| compare_text(&a.rest, &b.rest))
        .then_with(|| {
            a.original
                .as_encoded_bytes()
                .cmp(b.original.as_encoded_bytes())
        })
}

/// Compares two ASCII digit strings by integer value, without overflow.
pub fn compare_digits(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

fn compare_suffix(a: Option<&Suffix>, b: Option<&Suffix>) -> Ordering {
    fn rank(s: Option<&Suffix>) -> u8 {
        match s {
            Some(Suffix::Numeric(_)) => 0,
            Some(Suffix::Alpha(_)) => 1,
            None => 2,
        }
    }
    match (a, b) {
        (Some(Suffix::Numeric(x)), Some(Suffix::Numeric(y))) => compare_digits(x, y),
        (Some(Suffix::Alpha(x)), Some(Suffix::Alpha(y))) => x
            .to_ascii_lowercase()
            .cmp(&y.to_ascii_lowercase())
            .then_with(|| x.cmp(y)),
        _ => rank(a).cmp(&rank(b)),
    }
}

/// Case-insensitive comparison with an exact-bytes tiebreak.
fn compare_text(a: &OsStr, b: &OsStr) -> Ordering {
    let fa = a.to_string_lossy().to_lowercase();
    let fb = b.to_string_lossy().to_lowercase();
    fa.cmp(&fb)
        .then_with(|| a.as_encoded_bytes().cmp(b.as_encoded_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn sorted(names: &[&str]) -> Vec<String> {
        let mut parsed: Vec<_> = names
            .iter()
            .map(|n| parse(OsStr::new(n)).unwrap())
            .collect();
        parsed.sort_by(compare);
        parsed
            .into_iter()
            .map(|p| p.original.into_string().unwrap())
            .collect()
    }

    #[test]
    fn digits_compare_numerically() {
        assert_eq!(compare_digits("2", "10"), Ordering::Less);
        assert_eq!(compare_digits("010", "9"), Ordering::Greater);
        assert_eq!(compare_digits("007", "7"), Ordering::Equal);
        assert_eq!(compare_digits("0", "000"), Ordering::Equal);
        assert_eq!(
            compare_digits("99999999999999999999999", "100000000000000000000000"),
            Ordering::Less
        );
    }

    #[test]
    fn primary_is_numeric_not_lexicographic() {
        assert_eq!(
            sorted(&[
                "100-hundredth.txt",
                "10-tenth.txt",
                "2-second.txt",
                "1-first.txt"
            ]),
            [
                "1-first.txt",
                "2-second.txt",
                "10-tenth.txt",
                "100-hundredth.txt"
            ]
        );
    }

    #[test]
    fn leading_zeros_do_not_affect_order() {
        assert_eq!(sorted(&["003-c", "2-b", "01-a"]), ["01-a", "2-b", "003-c"]);
    }

    #[test]
    fn numeric_suffixes_before_alpha() {
        assert_eq!(
            sorted(&[
                "01a-this.txt",
                "01b-that.txt",
                "01.1-those.txt",
                "01.2-these.txt"
            ]),
            [
                "01.1-those.txt",
                "01.2-these.txt",
                "01a-this.txt",
                "01b-that.txt"
            ]
        );
    }

    #[test]
    fn numeric_suffixes_compare_numerically() {
        assert_eq!(
            sorted(&["1.10-x", "1.2-y", "1.1-z"]),
            ["1.1-z", "1.2-y", "1.10-x"]
        );
    }

    #[test]
    fn alpha_suffixes_compare_alphabetically() {
        assert_eq!(
            sorted(&["1c-x", "1aa-y", "1b-z", "1a-w"]),
            ["1a-w", "1aa-y", "1b-z", "1c-x"]
        );
    }

    #[test]
    fn alpha_suffixes_ignore_case_then_uppercase_first() {
        assert_eq!(sorted(&["1b-x", "1A-y", "1a-z"]), ["1A-y", "1a-z", "1b-x"]);
    }

    #[test]
    fn unsuffixed_sort_by_rest() {
        assert_eq!(
            sorted(&["01-this.txt", "01-that.txt"]),
            ["01-that.txt", "01-this.txt"]
        );
    }

    #[test]
    fn rest_is_case_insensitive_with_exact_tiebreak() {
        assert_eq!(
            sorted(&["1-banana", "1-Apple", "1-apple", "1-Cherry"]),
            ["1-Apple", "1-apple", "1-banana", "1-Cherry"]
        );
    }

    #[test]
    fn separator_is_not_part_of_key() {
        assert_eq!(sorted(&["1_b", "1-a"]), ["1-a", "1_b"]);
    }

    #[test]
    fn explicit_suffixes_precede_unsuffixed() {
        assert_eq!(
            sorted(&[
                "01-zebra.txt",
                "01b-beta.txt",
                "01-apple.txt",
                "01.2-second.txt",
                "01a-alpha.txt",
                "01.1-first.txt",
            ]),
            [
                "01.1-first.txt",
                "01.2-second.txt",
                "01a-alpha.txt",
                "01b-beta.txt",
                "01-apple.txt",
                "01-zebra.txt",
            ]
        );
    }

    #[test]
    fn primary_dominates_suffix_and_rest() {
        assert_eq!(
            sorted(&["03.1-a", "02-z", "02b-y", "03-a", "02.9-x"]),
            ["02.9-x", "02b-y", "02-z", "03.1-a", "03-a"]
        );
    }

    #[test]
    fn identical_keys_fall_back_to_original_name() {
        assert_eq!(sorted(&["1-a", "01-a", "001_a"]), ["001_a", "01-a", "1-a"]);
    }
}
