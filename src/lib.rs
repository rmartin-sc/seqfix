//! `seqfix` normalizes numeric filename prefixes into a clean, gap-free
//! sequence. See the README for the full naming grammar and ordering rules.
//!
//! The pure logic lives in [`parser`], [`ordering`] and [`planner`]
//! (`planner::plan_names`); filesystem access is confined to
//! [`planner::plan_tree`] and [`executor`].

pub mod cli;
pub mod executor;
pub mod ordering;
pub mod parser;
pub mod planner;
