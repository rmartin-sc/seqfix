#!/bin/sh
# The same checks as CI. Run by cargo-release before a release commit.
set -e
cargo fmt --all --check
cargo clippy --all-targets --locked --quiet -- -D warnings
cargo test --locked --quiet
