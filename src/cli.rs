//! Command-line interface.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

use crate::executor::execute;
use crate::planner::{DirPlan, PlanError, plan_tree};

/// Exit status: renames proposed or applied successfully (or nothing to do).
pub const EXIT_OK: u8 = 0;
/// Exit status: an I/O or rename error occurred; any partial changes were rolled back.
pub const EXIT_ERROR: u8 = 1;
/// Exit status: invalid command-line usage (reported by clap).
pub const EXIT_USAGE: u8 = 2;
/// Exit status: naming conflicts were detected; nothing was changed.
pub const EXIT_CONFLICT: u8 = 3;
/// Exit status: a rename failed and the rollback was incomplete.
pub const EXIT_ROLLBACK_INCOMPLETE: u8 = 4;

/// Normalize numeric filename prefixes into a clean, gap-free sequence.
///
/// By default seqfix only shows the renames it would make. Pass --apply to
/// perform them.
#[derive(Debug, Parser)]
#[command(name = "seqfix", version, about, long_about)]
pub struct Cli {
    /// Directory to process
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Show proposed renames without making changes (the default)
    #[arg(long, conflicts_with = "apply")]
    pub dry_run: bool,

    /// Perform the renames
    #[arg(long)]
    pub apply: bool,

    /// Process subdirectories recursively (symbolic links are not followed)
    #[arg(short, long)]
    pub recursive: bool,

    /// Starting sequence number
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub start: u64,

    /// Display additional details
    #[arg(short, long)]
    pub verbose: bool,
}

/// Parses the process arguments and runs seqfix.
pub fn run() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(if e.use_stderr() { EXIT_USAGE } else { EXIT_OK });
        }
    };
    ExitCode::from(run_with(&cli))
}

/// Runs seqfix with already-parsed options and returns the exit status.
pub fn run_with(cli: &Cli) -> u8 {
    let root = &cli.path;
    if !root.is_dir() {
        eprintln!("seqfix: {}: not a directory", root.display());
        return EXIT_ERROR;
    }

    let plans = match plan_tree(root, cli.recursive, cli.start) {
        Ok(plans) => plans,
        Err(PlanError::Conflicts(conflicts)) => {
            for c in &conflicts {
                eprintln!("seqfix: conflict: {c}");
            }
            eprintln!(
                "seqfix: {} conflict(s) found; nothing was changed",
                conflicts.len()
            );
            return EXIT_CONFLICT;
        }
        Err(e) => {
            eprintln!("seqfix: {e}");
            return EXIT_ERROR;
        }
    };

    for plan in &plans {
        if cli.verbose {
            print_details(root, plan);
        }
        for r in &plan.plan.renames {
            println!(
                "{} -> {}",
                display(root, &plan.dir.join(&r.from)),
                display(root, &plan.dir.join(&r.to))
            );
        }
    }

    let total: usize = plans.iter().map(|p| p.plan.renames.len()).sum();
    if total == 0 {
        eprintln!("Nothing to rename.");
        return EXIT_OK;
    }

    if !cli.apply {
        eprintln!("Dry run: {total} rename(s) proposed. Re-run with --apply to perform them.");
        return EXIT_OK;
    }

    match execute(&plans) {
        Ok(n) => {
            eprintln!("Renamed {n} entr{}.", if n == 1 { "y" } else { "ies" });
            EXIT_OK
        }
        Err(e) => {
            eprintln!("seqfix: {e}");
            if e.fully_rolled_back() {
                eprintln!("seqfix: all completed renames were rolled back");
                EXIT_ERROR
            } else {
                eprintln!("seqfix: rollback incomplete; these entries need manual attention:");
                for (current, original, err) in &e.unrecovered {
                    eprintln!(
                        "  {} (originally {}): {err}",
                        current.display(),
                        original.display()
                    );
                }
                EXIT_ROLLBACK_INCOMPLETE
            }
        }
    }
}

fn print_details(root: &Path, plan: &DirPlan) {
    let p = &plan.plan;
    eprintln!(
        "{}: {} numbered entr{}, width {}, {} to rename",
        display(root, &plan.dir),
        p.matched,
        if p.matched == 1 { "y" } else { "ies" },
        p.width,
        p.renames.len()
    );
    for name in &p.unmatched {
        eprintln!(
            "  skipped (no numeric prefix): {}",
            Path::new(name).display()
        );
    }
}

/// Displays `path` relative to `root`, or `.` for the root itself.
fn display(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Ok(rel) => rel.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn defaults() {
        let cli = Cli::try_parse_from(["seqfix"]).unwrap();
        assert_eq!(cli.path, PathBuf::from("."));
        assert!(!cli.apply && !cli.dry_run && !cli.recursive && !cli.verbose);
        assert_eq!(cli.start, 1);
    }

    #[test]
    fn dry_run_conflicts_with_apply() {
        assert!(Cli::try_parse_from(["seqfix", "--dry-run", "--apply"]).is_err());
    }

    #[test]
    fn width_option_is_not_supported() {
        assert!(Cli::try_parse_from(["seqfix", "--width", "3"]).is_err());
    }
}
