//! End-to-end tests that run the `seqfix` binary against temporary directories.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn seqfix(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_seqfix"))
        .args(args)
        .arg(dir)
        .output()
        .expect("failed to run seqfix")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Creates a temporary directory containing files (and directories, for
/// names ending in `/`). File contents are the original name.
fn setup(entries: &[&str]) -> TempDir {
    let tmp = tempfile::tempdir().unwrap();
    for e in entries {
        let path = tmp.path().join(e);
        if e.ends_with('/') {
            fs::create_dir_all(path).unwrap();
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, e).unwrap();
        }
    }
    tmp
}

fn listing(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn apply(dir: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["--apply"];
    args.extend_from_slice(extra);
    let out = seqfix(dir, &args);
    assert!(out.status.success(), "seqfix failed: {}", stderr(&out));
    out
}

#[test]
fn dry_run_is_the_default_and_changes_nothing() {
    let tmp = setup(&["3-c.txt", "1-a.txt"]);
    for args in [&[][..], &["--dry-run"][..]] {
        let out = seqfix(tmp.path(), args);
        assert!(out.status.success());
        assert_eq!(stdout(&out), "1-a.txt -> 01-a.txt\n3-c.txt -> 02-c.txt\n");
        assert!(stderr(&out).contains("--apply"));
        assert_eq!(listing(tmp.path()), ["1-a.txt", "3-c.txt"]);
    }
}

#[test]
fn apply_renames_and_keeps_contents() {
    let tmp = setup(&["3-c.txt", "1-a.txt"]);
    let out = apply(tmp.path(), &[]);
    assert_eq!(stdout(&out), "1-a.txt -> 01-a.txt\n3-c.txt -> 02-c.txt\n");
    assert_eq!(listing(tmp.path()), ["01-a.txt", "02-c.txt"]);
    assert_eq!(
        fs::read_to_string(tmp.path().join("02-c.txt")).unwrap(),
        "3-c.txt"
    );
}

#[test]
fn dry_run_and_apply_are_mutually_exclusive() {
    let tmp = setup(&["1-a"]);
    let out = seqfix(tmp.path(), &["--dry-run", "--apply"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(listing(tmp.path()), ["1-a"]);
}

#[test]
fn gaps_and_numeric_order() {
    let tmp = setup(&["100-h.txt", "10-t.txt", "2-s.txt", "1-f.txt"]);
    apply(tmp.path(), &[]);
    assert_eq!(
        listing(tmp.path()),
        ["01-f.txt", "02-s.txt", "03-t.txt", "04-h.txt"]
    );
}

#[test]
fn spec_rule_examples() {
    let tmp = setup(&[
        "01-zebra.txt",
        "01b-beta.txt",
        "01-apple.txt",
        "01.2-second.txt",
        "01a-alpha.txt",
        "01.1-first.txt",
        "02-next.txt",
    ]);
    apply(tmp.path(), &[]);
    assert_eq!(
        listing(tmp.path()),
        [
            "01-first.txt",
            "02-second.txt",
            "03-alpha.txt",
            "04-beta.txt",
            "05-apple.txt",
            "06-zebra.txt",
            "07-next.txt",
        ]
    );
}

#[test]
fn numeric_suffix_two_before_ten() {
    let tmp = setup(&["4.10-ten", "4.2-two"]);
    apply(tmp.path(), &[]);
    assert_eq!(listing(tmp.path()), ["01-two", "02-ten"]);
}

#[test]
fn multiple_extensions_and_separators_preserved() {
    let tmp = setup(&["5_backup.tar.gz", "2-notes.final.md"]);
    apply(tmp.path(), &[]);
    assert_eq!(
        listing(tmp.path()),
        ["01-notes.final.md", "02_backup.tar.gz"]
    );
}

#[test]
fn non_matching_names_are_untouched() {
    let tmp = setup(&["README.md", "notes-1.txt", "1.txt", "2-b", "x/"]);
    apply(tmp.path(), &[]);
    assert_eq!(
        listing(tmp.path()),
        ["01-b", "1.txt", "README.md", "notes-1.txt", "x"]
    );
}

#[test]
fn directories_are_renamed_in_the_same_sequence() {
    let tmp = setup(&["2-chapter/inner.txt", "1-intro.md"]);
    apply(tmp.path(), &[]);
    assert_eq!(listing(tmp.path()), ["01-intro.md", "02-chapter"]);
    assert!(tmp.path().join("02-chapter/inner.txt").is_file());
}

#[test]
fn non_recursive_leaves_subdirectories_alone() {
    let tmp = setup(&["5-dir/7-x", "5-dir/9-y"]);
    apply(tmp.path(), &[]);
    assert_eq!(listing(&tmp.path().join("01-dir")), ["7-x", "9-y"]);
}

#[test]
fn recursive_sequences_and_widths_are_independent() {
    let mut entries: Vec<String> = (1..=100).map(|i| format!("3-big/{i}-f")).collect();
    entries.extend([
        "3-big/7-sub/5-deep".into(),
        "9-small/4-a".into(),
        "9-small/8-b".into(),
    ]);
    let refs: Vec<&str> = entries.iter().map(String::as_str).collect();
    let tmp = setup(&refs);

    let dry = seqfix(tmp.path(), &["-r"]);
    let text = stdout(&dry);
    assert!(text.contains(&format!(
        "{} -> ",
        Path::new("3-big").join("7-sub").join("5-deep").display()
    )));
    assert!(text.contains("3-big -> 01-big"));

    apply(tmp.path(), &["--recursive"]);
    assert_eq!(listing(tmp.path()), ["01-big", "02-small"]);
    assert_eq!(listing(&tmp.path().join("02-small")), ["01-a", "02-b"]);
    let big = listing(&tmp.path().join("01-big"));
    assert_eq!(big.len(), 101);
    assert_eq!(big[0], "001-f");
    // "7-sub" sorts after "7-f" (s > f) and shifts the rest along.
    assert!(big.contains(&"008-sub".to_string()));
    assert!(big.contains(&"101-f".to_string()));
    assert_eq!(listing(&tmp.path().join("01-big/008-sub")), ["01-deep"]);
}

#[test]
fn start_option() {
    let tmp = setup(&["1-a", "2-b"]);
    apply(tmp.path(), &["--start", "99"]);
    assert_eq!(listing(tmp.path()), ["099-a", "100-b"]);
}

#[test]
fn width_option_is_rejected() {
    let tmp = setup(&["1-a"]);
    assert_eq!(seqfix(tmp.path(), &["--width", "3"]).status.code(), Some(2));
}

#[test]
fn idempotent() {
    let tmp = setup(&["9b-x", "9a-y", "3_z", "12.1-w", "12-v/"]);
    apply(tmp.path(), &[]);
    let first = listing(tmp.path());
    let out = apply(tmp.path(), &["-r"]);
    assert_eq!(stdout(&out), "");
    assert!(stderr(&out).contains("Nothing to rename"));
    assert_eq!(listing(tmp.path()), first);
}

#[test]
fn swaps_and_cycles_on_disk() {
    // Sorted: 01-a, 1-a (same key, original bytes "01-a" < "1-a"), 1-b, 02-a.
    // So 1-a -> 02-a while 02-a -> 04-a: a target occupied by another source.
    let tmp = setup(&["1-b", "1-a", "01-a", "02-a"]);
    apply(tmp.path(), &[]);
    assert_eq!(listing(tmp.path()), ["01-a", "02-a", "03-b", "04-a"]);
    assert_eq!(fs::read_to_string(tmp.path().join("01-a")).unwrap(), "01-a");
    assert_eq!(fs::read_to_string(tmp.path().join("02-a")).unwrap(), "1-a");
    assert_eq!(fs::read_to_string(tmp.path().join("03-b")).unwrap(), "1-b");
    assert_eq!(fs::read_to_string(tmp.path().join("04-a")).unwrap(), "02-a");
}

#[test]
fn names_differing_only_in_case() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("5-Notes.txt"), "upper").unwrap();
    // On a case-insensitive filesystem this overwrites the same file, so
    // only check the two-entry ordering where both names really exist.
    fs::write(tmp.path().join("5-notes.txt"), "lower").unwrap();
    apply(tmp.path(), &[]);
    let names = listing(tmp.path());
    if names.len() == 2 {
        assert_eq!(names, ["01-Notes.txt", "02-notes.txt"]);
        assert_eq!(
            fs::read_to_string(tmp.path().join("01-Notes.txt")).unwrap(),
            "upper"
        );
    } else {
        assert_eq!(names.len(), 1);
        assert!(names[0].eq_ignore_ascii_case("01-notes.txt"));
    }
}

#[test]
fn verbose_reports_details() {
    let tmp = setup(&["README", "3-a"]);
    let out = seqfix(tmp.path(), &["-v"]);
    let err = stderr(&out);
    assert!(
        err.contains("1 numbered entry, width 2, 1 to rename"),
        "{err}"
    );
    assert!(err.contains("skipped (no numeric prefix): README"), "{err}");
}

#[test]
fn missing_path_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let out = seqfix(&tmp.path().join("nope"), &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("not a directory"));
}

#[cfg(unix)]
#[test]
fn symlinks_are_renamed_but_not_followed() {
    let tmp = setup(&["target/3-x", "target/5-y"]);
    std::os::unix::fs::symlink(tmp.path().join("target"), tmp.path().join("4-link")).unwrap();
    apply(tmp.path(), &["-r"]);
    assert_eq!(listing(tmp.path()), ["01-link", "target"]);
    assert!(
        fs::symlink_metadata(tmp.path().join("01-link"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    // Reached through the real directory, not through the link.
    assert_eq!(listing(&tmp.path().join("target")), ["01-x", "02-y"]);

    // A link pointing outside the tree must not be traversed.
    let outside = setup(&["7-keep"]);
    let tmp2 = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), tmp2.path().join("1-out")).unwrap();
    apply(tmp2.path(), &["-r"]);
    assert_eq!(listing(outside.path()), ["7-keep"]);
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_names() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join(OsStr::from_bytes(b"7-\xff.bin")), "x").unwrap();
    fs::write(tmp.path().join("3-a.txt"), "y").unwrap();
    let out = apply(tmp.path(), &[]);
    assert!(stdout(&out).contains("3-a.txt -> 01-a.txt"));

    let mut names: Vec<Vec<u8>> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_vec())
        .collect();
    names.sort();
    assert_eq!(names, [b"01-a.txt".to_vec(), b"02-\xff.bin".to_vec()]);
}

#[cfg(unix)]
#[test]
fn unreadable_subdirectory_aborts_before_changes() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = setup(&["2-a", "5-locked/1-x"]);
    let locked = tmp.path().join("5-locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    // Root ignores permissions; skip the assertion in that case.
    if fs::read_dir(&locked).is_ok() {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let out = seqfix(tmp.path(), &["--apply", "-r"]);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(listing(tmp.path()), ["2-a", "5-locked"]);
}
