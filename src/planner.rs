//! Build rename plans and detect conflicts before anything is modified.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::ordering::compare;
use crate::parser::parse;

/// A single proposed rename within one directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    pub from: OsString,
    pub to: OsString,
}

/// The normalized sequence for one set of sibling names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NamePlan {
    /// Renames needed, in sequence order. Names already correct are omitted.
    pub renames: Vec<Rename>,
    /// Padding width used for the sequence.
    pub width: usize,
    /// Number of names that matched the prefix grammar.
    pub matched: usize,
    /// Names that did not match the prefix grammar and are left unchanged.
    pub unmatched: Vec<OsString>,
}

/// The plan for one directory on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirPlan {
    pub dir: PathBuf,
    pub plan: NamePlan,
}

/// A target name that would clash with another entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub dir: PathBuf,
    pub from: OsString,
    pub to: OsString,
    /// The existing or planned name it clashes with.
    pub with: OsString,
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} -> {} would collide with {}",
            self.dir.join(&self.from).display(),
            self.dir.join(&self.to).display(),
            self.dir.join(&self.with).display()
        )
    }
}

#[derive(Debug)]
pub enum PlanError {
    Io { path: PathBuf, source: io::Error },
    Conflicts(Vec<Conflict>),
    Overflow { dir: PathBuf },
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            PlanError::Conflicts(c) => write!(f, "{} naming conflict(s) detected", c.len()),
            PlanError::Overflow { dir } => {
                write!(f, "{}: sequence numbers would overflow", dir.display())
            }
        }
    }
}

impl std::error::Error for PlanError {}

/// Minimum zero-padding width for a sequence whose largest number is `max`.
pub fn width_for(max: u64) -> usize {
    max.to_string().len().max(2)
}

/// Computes the normalized names for a set of sibling names. Pure: no I/O.
///
/// Returns `None` if the sequence numbers would overflow `u64`.
pub fn plan_names(names: &[OsString], start: u64) -> Option<NamePlan> {
    let mut parsed = Vec::new();
    let mut unmatched = Vec::new();
    for name in names {
        match parse(name) {
            Some(p) => parsed.push(p),
            None => unmatched.push(name.clone()),
        }
    }
    parsed.sort_by(compare);
    unmatched.sort();

    let width = match parsed.len() as u64 {
        0 => 2,
        n => width_for(start.checked_add(n - 1)?),
    };

    let renames = parsed
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let to = p.renamed(start + i as u64, width);
            (to != p.original).then(|| Rename {
                from: p.original.clone(),
                to,
            })
        })
        .collect();

    Some(NamePlan {
        renames,
        width,
        matched: parsed.len(),
        unmatched,
    })
}

/// Case-folded form of a name, used to detect clashes on case-insensitive
/// filesystems. Deliberately conservative: it may report clashes that a
/// case-sensitive filesystem would allow.
pub fn fold(name: &OsStr) -> String {
    name.to_string_lossy().to_lowercase()
}

/// Finds targets that would clash (case-insensitively) with a name that is
/// not being renamed away, or with another target.
pub fn find_conflicts(dir: &Path, all_names: &[OsString], renames: &[Rename]) -> Vec<Conflict> {
    let leaving: std::collections::HashSet<&OsStr> =
        renames.iter().map(|r| r.from.as_os_str()).collect();
    let mut taken: HashMap<String, &OsString> = all_names
        .iter()
        .filter(|n| !leaving.contains(n.as_os_str()))
        .map(|n| (fold(n), n))
        .collect();

    let mut conflicts = Vec::new();
    for r in renames {
        let key = fold(&r.to);
        if let Some(with) = taken.get(&key) {
            conflicts.push(Conflict {
                dir: dir.to_path_buf(),
                from: r.from.clone(),
                to: r.to.clone(),
                with: (*with).clone(),
            });
        } else {
            taken.insert(key, &r.to);
        }
    }
    conflicts
}

/// Lists the names in `dir` and the subdirectories to descend into.
/// Symbolic links are never treated as directories.
fn read_dir(dir: &Path) -> Result<(Vec<OsString>, Vec<OsString>), PlanError> {
    let io_err = |source| PlanError::Io {
        path: dir.to_path_buf(),
        source,
    };
    let mut names = Vec::new();
    let mut subdirs = Vec::new();
    for entry in fs::read_dir(dir).map_err(io_err)? {
        let entry = entry.map_err(io_err)?;
        let name = entry.file_name();
        // `DirEntry::file_type` does not follow symlinks.
        if entry.file_type().map_err(io_err)?.is_dir() {
            subdirs.push(name.clone());
        }
        names.push(name);
    }
    names.sort();
    subdirs.sort();
    Ok((names, subdirs))
}

/// Plans renames for `root` (and, if `recursive`, every real subdirectory).
///
/// Plans are returned children-first, so executing them in order never
/// invalidates a path that a later plan relies on. Every directory is
/// returned, even those needing no renames. If any conflict is found
/// anywhere, all conflicts are reported and nothing is returned.
pub fn plan_tree(root: &Path, recursive: bool, start: u64) -> Result<Vec<DirPlan>, PlanError> {
    let mut plans = Vec::new();
    let mut conflicts = Vec::new();
    visit(root, recursive, start, &mut plans, &mut conflicts)?;
    if conflicts.is_empty() {
        Ok(plans)
    } else {
        Err(PlanError::Conflicts(conflicts))
    }
}

fn visit(
    dir: &Path,
    recursive: bool,
    start: u64,
    plans: &mut Vec<DirPlan>,
    conflicts: &mut Vec<Conflict>,
) -> Result<(), PlanError> {
    let (names, subdirs) = read_dir(dir)?;
    if recursive {
        for sub in &subdirs {
            visit(&dir.join(sub), recursive, start, plans, conflicts)?;
        }
    }
    let plan = plan_names(&names, start).ok_or_else(|| PlanError::Overflow {
        dir: dir.to_path_buf(),
    })?;
    conflicts.extend(find_conflicts(dir, &names, &plan.renames));
    plans.push(DirPlan {
        dir: dir.to_path_buf(),
        plan,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    /// Returns the full resulting directory listing in sequence order.
    fn result(list: &[&str], start: u64) -> Vec<String> {
        let input = names(list);
        let plan = plan_names(&input, start).unwrap();
        let mut out: Vec<OsString> = input
            .iter()
            .filter(|n| parse(n).is_some())
            .map(|n| {
                plan.renames
                    .iter()
                    .find(|r| &r.from == n)
                    .map_or(n.clone(), |r| r.to.clone())
            })
            .collect();
        out.sort();
        out.into_iter().map(|s| s.into_string().unwrap()).collect()
    }

    #[test]
    fn width_rules() {
        assert_eq!(width_for(1), 2);
        assert_eq!(width_for(9), 2);
        assert_eq!(width_for(99), 2);
        assert_eq!(width_for(100), 3);
        assert_eq!(width_for(1000), 4);
    }

    #[test]
    fn simple_sequence() {
        assert_eq!(result(&["1-a", "2-b", "3-c"], 1), ["01-a", "02-b", "03-c"]);
    }

    #[test]
    fn removes_gaps() {
        assert_eq!(
            result(
                &[
                    "03-third.txt",
                    "01-first.txt",
                    "07-fourth.txt",
                    "02-second.txt"
                ],
                1
            ),
            [
                "01-first.txt",
                "02-second.txt",
                "03-third.txt",
                "04-fourth.txt"
            ]
        );
    }

    #[test]
    fn width_from_result_not_original() {
        assert_eq!(
            result(&["1-this.txt", "10-some.txt"], 1),
            ["01-this.txt", "02-some.txt"]
        );
        assert_eq!(result(&["001-a", "100-b"], 1), ["01-a", "02-b"]);
    }

    #[test]
    fn widths_for_counts() {
        for (count, first, last) in [
            (9, "01-x", "09-x"),
            (10, "01-x", "10-x"),
            (99, "01-x", "99-x"),
            (100, "001-x", "100-x"),
            (1000, "0001-x", "1000-x"),
        ] {
            let list: Vec<String> = (1..=count).map(|i| format!("{i}-x")).collect();
            let refs: Vec<&str> = list.iter().map(String::as_str).collect();
            let out = result(&refs, 1);
            assert_eq!(out.first().unwrap(), first);
            assert_eq!(out.last().unwrap(), last);
        }
    }

    #[test]
    fn ten_entries_one_to_ten() {
        let list: Vec<String> = (1..=10).map(|i| format!("{i}-x")).collect();
        let refs: Vec<&str> = list.iter().map(String::as_str).collect();
        let out = result(&refs, 1);
        assert_eq!(out[0], "01-x");
        assert_eq!(out[9], "10-x");
    }

    #[test]
    fn start_affects_width() {
        assert_eq!(result(&["1-a", "2-b"], 99), ["099-a", "100-b"]);
        assert_eq!(result(&["1-a"], 0), ["00-a"]);
    }

    #[test]
    fn start_overflow_is_detected() {
        assert!(plan_names(&names(&["1-a", "2-b"]), u64::MAX).is_none());
        assert!(plan_names(&names(&["1-a"]), u64::MAX).is_some());
    }

    #[test]
    fn suffix_examples() {
        assert_eq!(
            result(
                &[
                    "01a-this.txt",
                    "01b-that.txt",
                    "01.1-those.txt",
                    "01.2-these.txt"
                ],
                1
            ),
            ["01-those.txt", "02-these.txt", "03-this.txt", "04-that.txt"]
        );
        assert_eq!(
            result(&["01-this.txt", "01-that.txt"], 1),
            ["01-that.txt", "02-this.txt"]
        );
        assert_eq!(
            result(
                &[
                    "01-zebra.txt",
                    "01b-beta.txt",
                    "01-apple.txt",
                    "01.2-second.txt",
                    "01a-alpha.txt",
                    "01.1-first.txt",
                ],
                1
            ),
            [
                "01-first.txt",
                "02-second.txt",
                "03-alpha.txt",
                "04-beta.txt",
                "05-apple.txt",
                "06-zebra.txt",
            ]
        );
    }

    #[test]
    fn preserves_separator() {
        assert_eq!(result(&["5_a", "9-b"], 1), ["01_a", "02-b"]);
    }

    #[test]
    fn unmatched_names_are_ignored() {
        let plan = plan_names(&names(&["README.md", "3-a", "notes"]), 1).unwrap();
        assert_eq!(plan.unmatched, names(&["README.md", "notes"]));
        assert_eq!(plan.matched, 1);
        assert_eq!(
            plan.renames,
            [Rename {
                from: "3-a".into(),
                to: "01-a".into()
            }]
        );
    }

    #[test]
    fn already_normalized_is_idempotent() {
        let plan = plan_names(&names(&["01-a", "02-b", "03-c", "x"]), 1).unwrap();
        assert!(plan.renames.is_empty());
    }

    #[test]
    fn normalizing_twice_changes_nothing() {
        let first = result(&["5b-x", "5a-y", "2-z", "5.3_w"], 1);
        let again: Vec<&str> = first.iter().map(String::as_str).collect();
        assert!(plan_names(&names(&again), 1).unwrap().renames.is_empty());
    }

    #[test]
    fn conflicts_with_unrelated_entries() {
        let all = names(&["3-a", "01-A"]);
        // Construct a rename that clashes case-insensitively with a name that stays.
        let renames = [Rename {
            from: "3-a".into(),
            to: "01-a".into(),
        }];
        let c = find_conflicts(Path::new("d"), &all, &renames);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].with, "01-A");
    }

    #[test]
    fn conflicts_between_targets() {
        let all = names(&["x", "y"]);
        let renames = [
            Rename {
                from: "x".into(),
                to: "01-A".into(),
            },
            Rename {
                from: "y".into(),
                to: "01-a".into(),
            },
        ];
        assert_eq!(find_conflicts(Path::new("d"), &all, &renames).len(), 1);
    }

    #[test]
    fn swaps_are_not_conflicts() {
        let all = names(&["01-b", "02-a"]);
        let renames = [
            Rename {
                from: "02-a".into(),
                to: "01-a".into(),
            },
            Rename {
                from: "01-b".into(),
                to: "02-b".into(),
            },
        ];
        assert!(find_conflicts(Path::new("d"), &all, &renames).is_empty());
        // Targets that only differ in case from a departing source are fine too.
        let all = names(&["1-A"]);
        let renames = [Rename {
            from: "1-A".into(),
            to: "01-A".into(),
        }];
        assert!(find_conflicts(Path::new("d"), &all, &renames).is_empty());
    }
}
