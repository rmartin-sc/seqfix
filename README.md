# seqfix

[![CI](https://github.com/rmartin-sc/seqfix/actions/workflows/ci.yml/badge.svg)](https://github.com/rmartin-sc/seqfix/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/rmartin-sc/seqfix/blob/main/LICENSE)

`seqfix` is a small command-line tool that normalizes numbered file and
directory names, such as course materials, chapters or lessons, into a clean,
gap-free, zero-padded sequence.

```text
$ ls
01-intro.md  02a-setup.md  02b-tools.md  10-wrap-up.md
$ seqfix
02a-setup.md -> 02-setup.md
02b-tools.md -> 03-tools.md
10-wrap-up.md -> 04-wrap-up.md
Dry run: 3 rename(s) proposed. Re-run with --apply to perform them.
```

## Why?

Numbered names drift over time. Lessons are inserted as `02a` or `03.1`,
items are removed and leave gaps, and a jump from `9` to `10` makes `10`
sort before `2` in most file browsers. `seqfix` repairs all of that in one
predictable step while keeping each name's descriptive part intact.

- Numbers are compared as **integers**, never as text, so `2` precedes `10`.
- Insertions like `02a`/`02b` and `03.1`/`03.2` are folded into the sequence.
- Gaps are removed and every entry gets the same zero-padding width.
- It shows what it would do by default and only renames with `--apply`.

## Installation

### Pre-built binaries

Binaries for Linux (x86-64, ARM64), macOS (Intel, Apple silicon) and Windows
(x86-64, ARM64) are attached to each release on the
[Releases page](https://github.com/rmartin-sc/seqfix/releases), together
with a `SHA256SUMS` file. Download the archive for your platform, extract it
and place `seqfix` (or `seqfix.exe`) somewhere on your `PATH`.

```sh
# Example for Linux x86-64; substitute the version and target you need.
curl -LO https://github.com/rmartin-sc/seqfix/releases/download/v0.1.0/seqfix-v0.1.0-x86_64-unknown-linux-musl.tar.gz
tar xzf seqfix-v0.1.0-x86_64-unknown-linux-musl.tar.gz
sudo install seqfix-v0.1.0-x86_64-unknown-linux-musl/seqfix /usr/local/bin/
```

If no release is listed yet, build from source as below.

### From source with Cargo

Requires Rust 1.85 or newer ([rustup.rs](https://rustup.rs)).

```sh
cargo install --git https://github.com/rmartin-sc/seqfix
```

Or from a local clone:

```sh
git clone https://github.com/rmartin-sc/seqfix
cd seqfix
cargo install --path .
```

`seqfix` is not currently published on crates.io.

## Usage

```text
seqfix [OPTIONS] [PATH]
```

`PATH` is the directory to process and defaults to the current directory.

```sh
seqfix                      # preview renames in the current directory
seqfix lessons/             # preview renames in lessons/
seqfix --apply lessons/     # perform them
seqfix -r --apply course/   # every directory under course/, independently
seqfix --start 0 slides/    # number from 00 instead of 01
```

### Options

| Option | Description |
|---|---|
| `--dry-run` | Show proposed renames without making changes. This is the default; the flag exists to make intent explicit in scripts. Cannot be combined with `--apply`. |
| `--apply` | Perform the renames. |
| `-r`, `--recursive` | Also process every subdirectory. Each directory gets its own independent sequence and padding width. Symbolic links are never followed. |
| `--start N` | First sequence number (default `1`). Also counts towards the padding width. |
| `-v`, `--verbose` | Show, per directory, how many entries were recognized, the padding width, and which names were skipped. |
| `-h`, `--help` | Show help. |
| `-V`, `--version` | Show the version. |

Proposed renames are written to standard output, one per line, as
`old-name -> new-name`. Entries that already have the correct name are not
listed. With `--recursive`, paths are shown relative to
`PATH`. Summaries, details and errors go to standard error.

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Success, including when there is nothing to rename. |
| `1` | An error occurred (for example, unreadable directory or failed rename). Any renames already made were rolled back. |
| `2` | Invalid command-line arguments. |
| `3` | Naming conflicts were detected. Nothing was changed. |
| `4` | A rename failed **and** rollback was incomplete. The affected paths are listed and need manual attention. |

## Examples

**Gaps are removed**

```text
Before            After
01-first.txt      01-first.txt
02-second.txt     02-second.txt
03-third.txt      03-third.txt
07-fourth.txt     04-fourth.txt
```

**Numbers compare as integers, and padding is recomputed**

```text
Before              After
1-first.txt         01-first.txt
2-second.txt        02-second.txt
10-tenth.txt        03-tenth.txt
100-hundredth.txt   04-hundredth.txt
```

**Ordering suffixes: numeric before alphabetic**

```text
Before            After
01.1-those.txt    01-those.txt
01.2-these.txt    02-these.txt
01a-this.txt      03-this.txt
01b-that.txt      04-that.txt
```

**Duplicate numbers without suffixes sort by the rest of the name**

```text
Before          After
01-this.txt     02-this.txt
01-that.txt     01-that.txt
```

**Explicit suffixes come before unsuffixed duplicates**

```text
Before             After
01-zebra.txt       06-zebra.txt
01b-beta.txt       04-beta.txt
01-apple.txt       05-apple.txt
01.2-second.txt    02-second.txt
01a-alpha.txt      03-alpha.txt
01.1-first.txt     01-first.txt
```

**Separators and extensions are preserved; other names are untouched**

```text
Before                  After
README.md               README.md
5_backup.tar.gz         02_backup.tar.gz
2-notes.final.md        01-notes.final.md
```

## Naming grammar

A name is processed only if it matches this grammar exactly:

```text
name      = primary [ suffix ] separator rest
primary   = one or more ASCII digits           e.g. 1, 01, 100
suffix    = "." one or more ASCII digits       e.g. .1, .10   (numeric)
          | one or more ASCII letters          e.g. a, B, aa  (alphabetic)
separator = "-" or "_"
rest      = one or more characters of any kind (must not be empty)
```

Matching: `1-intro.md`, `01_intro.md`, `02a-example.rs`, `03.1-first.txt`,
`100-final.txt`.

Not matching (left unchanged): `README.md`, `notes-1.txt`, `1.txt`,
`1 intro.txt`, `1-` (empty rest), `1.2.3-x`, `1.a-x`, `a1-x`.

Files, directories and symbolic links in the same directory share **one**
sequence. Hidden files cannot match, because names must start with a digit.

## Ordering rules

Within each directory, matching entries are sorted by the following keys, in
order, and then numbered consecutively from `--start`:

1. **Primary number**, compared as an integer. Leading zeros are ignored, and
   numbers of any length are supported. Entries with different primary
   numbers always keep their relative order, regardless of suffixes or names.
2. **Ordering suffix**, for entries that share a primary number:
   1. numeric suffixes first, compared as integers (`.2` before `.10`;
      `.1` and `.01` are equal here);
   2. then alphabetic suffixes, compared alphabetically **ignoring case**,
      with uppercase before lowercase when they differ only in case
      (`A` < `a` < `B` < `b`); multi-letter suffixes compare letter by
      letter (`a` < `aa` < `b`);
   3. then entries without a suffix.
3. **Rest of the name** (everything after the separator), compared
   **ignoring case**; names that differ only in case are ordered by their
   exact bytes, which puts uppercase before lowercase (`Apple` < `apple` <
   `banana`).
4. **The full original name**, by exact bytes, as a final tiebreak so the
   result is always deterministic (`01-a` < `1-a`).

The separator is not part of the ordering key; each entry keeps its own
separator when renamed. The ordering suffix is dropped from the new name.

### Padding width

All entries in a directory share one width:

```text
width = max(2, number of digits in the largest resulting sequence number)
```

The largest resulting number is `start + count - 1`, so 9 entries become
`01`–`09`, 99 become `01`–`99`, 100 become `001`–`100`, and 1,000 become
`0001`–`1000`. The width depends on the result, not on the original names:
`1-this.txt` and `10-some.txt` become `01-this.txt` and `02-some.txt`. With
`--recursive`, the width is computed separately for each directory.

Running `seqfix` on an already-normalized directory makes no changes.

## Safety

- **Dry run by default.** Nothing is renamed unless `--apply` is given.
- **Plan first, then act.** All directories are scanned and every rename is
  planned before anything is changed. If any directory cannot be read, or any
  conflict is found, `seqfix` stops without modifying anything.
- **Never overwrites.** A rename whose destination already exists is refused.
  The destination is checked again immediately before every rename.
- **Case-insensitive filesystems.** Conflicts are detected by comparing names
  case-insensitively on every platform, so a plan that is safe on Linux is
  also safe on Windows and default macOS volumes. This is deliberately
  conservative.
- **Swaps, cycles and case-only changes.** Each directory is renamed in two
  phases: every entry is first moved to a unique temporary name
  (`.seqfix-tmp-<pid>-<n>`) in the same directory, then to its final name.
- **Rollback.** If any rename fails, all completed renames are undone in
  reverse order. If rollback itself fails, `seqfix` exits with code `4` and
  lists each affected path with its original name.
- **Symbolic links** are renamed like any other entry when they match, but
  are never followed during recursive traversal.
- **Contents are never touched.** Only names change.
- **Non-UTF-8 names** are supported on platforms that allow them; they are
  compared using a lossy UTF-8 view for case folding and by exact bytes for
  tiebreaks, and their bytes are preserved exactly.

Note that renaming is not atomic across a whole directory. Avoid running
`seqfix --apply` while other programs are creating or renaming files in the
same directories.

### Things to be aware of

- Date-style names such as `2024-01-15-notes.md` match the grammar (primary
  `2024`) and would be renumbered. Preview first.
- Names such as `1st-place.txt` match with the alphabetic suffix `st`.
- With `--recursive`, all subdirectories are visited, including hidden ones
  such as `.git`.

## Development

```sh
cargo build
cargo test                                  # unit and integration tests
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

The code is organized into small modules:

| Module | Responsibility |
|---|---|
| `parser` | Recognize the naming grammar and split names into parts. |
| `ordering` | Compare parsed names according to the ordering rules. |
| `planner` | Compute new names and widths, detect conflicts, and walk directories. |
| `executor` | Perform two-phase renames with collision checks and rollback. |
| `cli` | Argument parsing, output and exit codes. |

Parsing, ordering and `planner::plan_names` are pure functions with no
filesystem access and are covered by unit tests. The `tests/` directory
contains end-to-end tests that run the binary against temporary directories.

### Releases

Pushing a tag such as `v0.1.0` runs the release workflow, which builds
binaries for all supported platforms and attaches them to a GitHub release.
Update `version` in `Cargo.toml` before tagging.

### Documentation site

The site at <https://rmartin-sc.github.io/seqfix/> is generated from this
README by the Pages workflow whenever it changes on `main`. In the repository
settings, set **Pages → Source** to **GitHub Actions**.

## Contributing

Issues and pull requests are welcome at
<https://github.com/rmartin-sc/seqfix>. Please run the formatting, lint and
test commands above before submitting, and add tests for any behaviour change.

## License

Released under the [MIT License](https://github.com/rmartin-sc/seqfix/blob/main/LICENSE).
Copyright © 2026 Rodney Martin.
