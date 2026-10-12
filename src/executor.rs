//! Perform planned renames safely.
//!
//! Each directory is processed in two phases: every source is first moved to
//! a unique temporary name, then each temporary name is moved to its final
//! name. This makes swaps, cycles and case-only renames safe, including on
//! case-insensitive filesystems. Before each rename the destination is
//! checked so that nothing is ever overwritten. If any step fails, completed
//! renames are undone in reverse order.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::planner::{DirPlan, fold};

/// A rename that was carried out, recorded so it can be undone.
#[derive(Debug, Clone)]
struct Done {
    from: PathBuf,
    to: PathBuf,
}

#[derive(Debug)]
pub struct ExecError {
    /// What went wrong.
    pub message: String,
    /// Renames that could not be undone, as `(current path, original path, error)`.
    pub unrecovered: Vec<(PathBuf, PathBuf, io::Error)>,
}

impl ExecError {
    fn new(message: String) -> Self {
        ExecError {
            message,
            unrecovered: Vec::new(),
        }
    }

    /// Whether every completed rename was successfully undone.
    pub fn fully_rolled_back(&self) -> bool {
        self.unrecovered.is_empty()
    }
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExecError {}

/// Executes `plans` in order. Returns the number of entries renamed.
pub fn execute(plans: &[DirPlan]) -> Result<usize, ExecError> {
    for plan in plans {
        validate(plan)?;
    }

    let mut journal = Vec::new();
    let mut renamed = 0;
    for plan in plans {
        if let Err(message) = execute_dir(plan, &mut journal) {
            let unrecovered = rollback(&journal);
            return Err(ExecError {
                message,
                unrecovered,
            });
        }
        renamed += plan.plan.renames.len();
    }
    Ok(renamed)
}

/// Re-checks a plan against the directory as it is now, before touching it.
fn validate(plan: &DirPlan) -> Result<(), ExecError> {
    if plan.plan.renames.is_empty() {
        return Ok(());
    }
    let dir = &plan.dir;
    let current: Vec<OsString> = fs::read_dir(dir)
        .and_then(|it| it.map(|e| e.map(|e| e.file_name())).collect())
        .map_err(|e| ExecError::new(format!("{}: {e}", dir.display())))?;
    let present: HashSet<&OsString> = current.iter().collect();
    let leaving: HashSet<&OsString> = plan.plan.renames.iter().map(|r| &r.from).collect();
    let staying: HashSet<String> = current
        .iter()
        .filter(|n| !leaving.contains(n))
        .map(|n| fold(n))
        .collect();

    for r in &plan.plan.renames {
        if !present.contains(&r.from) {
            return Err(ExecError::new(format!(
                "{}: no longer exists",
                dir.join(&r.from).display()
            )));
        }
        if staying.contains(&fold(&r.to)) {
            return Err(ExecError::new(format!(
                "{}: destination already exists",
                dir.join(&r.to).display()
            )));
        }
    }
    Ok(())
}

fn execute_dir(plan: &DirPlan, journal: &mut Vec<Done>) -> Result<(), String> {
    let dir = &plan.dir;
    let mut temps = Vec::with_capacity(plan.plan.renames.len());
    let mut counter = 0u64;

    for r in &plan.plan.renames {
        let from = dir.join(&r.from);
        let temp = unique_temp(dir, &mut counter)
            .map_err(|e| format!("{}: cannot choose temporary name: {e}", dir.display()))?;
        rename_no_overwrite(&from, &temp, journal)?;
        temps.push(temp);
    }

    for (r, temp) in plan.plan.renames.iter().zip(&temps) {
        rename_no_overwrite(temp, &dir.join(&r.to), journal)?;
    }
    Ok(())
}

/// Returns a temporary path in `dir` that does not currently exist.
fn unique_temp(dir: &Path, counter: &mut u64) -> io::Result<PathBuf> {
    let pid = std::process::id();
    loop {
        let path = dir.join(format!(".seqfix-tmp-{pid}-{counter}"));
        *counter += 1;
        if !exists(&path)? {
            return Ok(path);
        }
    }
}

/// Whether `path` exists, without following symlinks.
fn exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

fn rename_no_overwrite(from: &Path, to: &Path, journal: &mut Vec<Done>) -> Result<(), String> {
    match exists(to) {
        Ok(false) => {}
        Ok(true) => return Err(format!("{}: destination already exists", to.display())),
        Err(e) => return Err(format!("{}: {e}", to.display())),
    }
    fs::rename(from, to)
        .map_err(|e| format!("cannot rename {} -> {}: {e}", from.display(), to.display()))?;
    journal.push(Done {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
    });
    Ok(())
}

/// Undoes journaled renames, most recent first. Returns those that failed.
fn rollback(journal: &[Done]) -> Vec<(PathBuf, PathBuf, io::Error)> {
    let mut failed = Vec::new();
    for done in journal.iter().rev() {
        let result = match exists(&done.from) {
            Ok(false) => fs::rename(&done.to, &done.from),
            Ok(true) => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "original name is occupied",
            )),
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            failed.push((done.to.clone(), done.from.clone(), e));
        }
    }
    failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{NamePlan, Rename};

    fn plan(dir: &Path, renames: &[(&str, &str)]) -> DirPlan {
        DirPlan {
            dir: dir.to_path_buf(),
            plan: NamePlan {
                renames: renames
                    .iter()
                    .map(|(f, t)| Rename {
                        from: f.into(),
                        to: t.into(),
                    })
                    .collect(),
                ..Default::default()
            },
        }
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        v.sort();
        v
    }

    fn touch(dir: &Path, name: &str, contents: &str) {
        fs::write(dir.join(name), contents).unwrap();
    }

    #[test]
    fn swap() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a", "A");
        touch(tmp.path(), "b", "B");
        let n = execute(&[plan(tmp.path(), &[("a", "b"), ("b", "a")])]).unwrap();
        assert_eq!(n, 2);
        assert_eq!(fs::read_to_string(tmp.path().join("a")).unwrap(), "B");
        assert_eq!(fs::read_to_string(tmp.path().join("b")).unwrap(), "A");
    }

    #[test]
    fn cycle() {
        let tmp = tempfile::tempdir().unwrap();
        for n in ["a", "b", "c"] {
            touch(tmp.path(), n, n);
        }
        execute(&[plan(tmp.path(), &[("a", "b"), ("b", "c"), ("c", "a")])]).unwrap();
        assert_eq!(fs::read_to_string(tmp.path().join("b")).unwrap(), "a");
        assert_eq!(fs::read_to_string(tmp.path().join("c")).unwrap(), "b");
        assert_eq!(fs::read_to_string(tmp.path().join("a")).unwrap(), "c");
        assert_eq!(listing(tmp.path()), ["a", "b", "c"]);
    }

    #[test]
    fn case_only_rename() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "01-readme", "x");
        execute(&[plan(tmp.path(), &[("01-readme", "01-README")])]).unwrap();
        assert_eq!(listing(tmp.path()), ["01-README"]);
    }

    #[test]
    fn refuses_existing_destination_before_changing_anything() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a", "A");
        touch(tmp.path(), "b", "B");
        touch(tmp.path(), "keep", "K");
        let err = execute(&[plan(tmp.path(), &[("a", "x"), ("b", "keep")])]).unwrap_err();
        assert!(err.message.contains("already exists"), "{err}");
        assert!(err.fully_rolled_back());
        assert_eq!(listing(tmp.path()), ["a", "b", "keep"]);
        assert_eq!(fs::read_to_string(tmp.path().join("keep")).unwrap(), "K");
    }

    #[test]
    fn refuses_case_insensitive_destination_clash() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a", "A");
        touch(tmp.path(), "01-KEEP", "K");
        let err = execute(&[plan(tmp.path(), &[("a", "01-keep")])]).unwrap_err();
        assert!(err.message.contains("already exists"), "{err}");
        assert_eq!(listing(tmp.path()), ["01-KEEP", "a"]);
    }

    #[test]
    fn refuses_missing_source() {
        let tmp = tempfile::tempdir().unwrap();
        let err = execute(&[plan(tmp.path(), &[("gone", "x")])]).unwrap_err();
        assert!(err.message.contains("no longer exists"), "{err}");
    }

    #[test]
    fn rolls_back_on_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let ok = tmp.path().join("ok");
        fs::create_dir(&ok).unwrap();
        touch(&ok, "1-a", "A");
        touch(tmp.path(), "2-b", "B");
        let good = plan(&ok, &[("1-a", "01-a")]);
        let mut bad = plan(tmp.path(), &[("2-b", "02-b")]);
        // Validation passes, but the directory vanishes before execution reaches it.
        let missing = tmp.path().join("missing");
        fs::create_dir(&missing).unwrap();
        touch(&missing, "2-b", "B");
        bad.dir = missing.clone();
        let plans = [good, bad];
        for p in &plans {
            validate(p).unwrap();
        }
        fs::remove_dir_all(&missing).unwrap();

        let mut journal = Vec::new();
        execute_dir(&plans[0], &mut journal).unwrap();
        assert_eq!(listing(&ok), ["01-a"]);
        assert!(execute_dir(&plans[1], &mut journal).is_err());
        assert!(rollback(&journal).is_empty());
        assert_eq!(listing(&ok), ["1-a"]);
    }
}
