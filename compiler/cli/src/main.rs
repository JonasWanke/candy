#![allow(clippy::all)]
// #![warn(clippy::nursery, clippy::pedantic, unused_crate_dependencies)]
// #![allow(
//     clippy::cognitive_complexity,
//     clippy::match_same_arms,
//     clippy::missing_errors_doc,
//     clippy::missing_panics_doc,
//     clippy::module_name_repetitions,
//     clippy::similar_names,
//     clippy::too_many_lines
// )]

use candy_vm::CAN_USE_STDOUT;
use clap::Parser;
use std::sync::atomic::Ordering;
use tracing::{debug, Level, Metadata};
use tracing_subscriber::{
    filter,
    fmt::{format::FmtSpan, writer::BoxMakeWriter},
    prelude::*,
};

mod check;
mod database;
mod debug;
mod fuzz;
#[cfg(feature = "inkwell")]
mod inkwell;
mod lsp;
mod run;
mod utils;

#[derive(Parser, Debug)]
#[command(name = "candy", about = "The 🍭 Candy CLI.")]
enum CandyOptions {
    Run(run::Options),

    Check(check::Options),

    Fuzz(fuzz::Options),

    #[command(subcommand)]
    Debug(debug::Options),

    /// Start a Language Server.
    Lsp,

    #[cfg(feature = "inkwell")]
    Inkwell(inkwell::Options),
}

#[tokio::main]
async fn main() -> ProgramResult {
    let options = CandyOptions::parse();

    let should_log_to_stdout = !matches!(options, CandyOptions::Lsp);
    init_logger(should_log_to_stdout);
    CAN_USE_STDOUT.store(should_log_to_stdout, Ordering::Relaxed);

    match options {
        CandyOptions::Run(options) => run::run(options),
        CandyOptions::Check(options) => check::check(options),
        CandyOptions::Fuzz(options) => fuzz::fuzz(options),
        CandyOptions::Debug(options) => debug::debug(options),
        CandyOptions::Lsp => lsp::lsp().await,
        #[cfg(feature = "inkwell")]
        CandyOptions::Inkwell(options) => inkwell::compile(&options),
    }
}

pub type ProgramResult = Result<(), Exit>;
#[derive(Debug)]
pub enum Exit {
    CodePanicked,
    DirectoryNotFound,
    #[cfg(feature = "inkwell")]
    ExternalError,
    FileNotFound,
    FuzzingFoundFailingCases,
    NotInCandyPackage,
    CodeContainsErrors,
    #[cfg(feature = "inkwell")]
    LlvmError(String),
    GoldOutdated,
}

fn init_logger(use_stdout: bool) {
    let writer = if use_stdout {
        BoxMakeWriter::new(std::io::stdout)
    } else {
        BoxMakeWriter::new(std::io::stderr)
    };
    let console_log = tracing_subscriber::fmt::layer()
        .compact()
        .with_writer(writer)
        .with_span_events(FmtSpan::ENTER)
        .with_filter(filter::filter_fn(|metadata| {
            // For external packages, show only the error logs.
            metadata.level() <= &Level::ERROR
                || metadata
                    .module_path()
                    .unwrap_or_default()
                    .starts_with("candy")
        }))
        .with_filter(filter::filter_fn(level_for(
            "candy_frontend::mir_optimize",
            Level::INFO,
        )))
        .with_filter(filter::filter_fn(level_for(
            "candy_frontend::string_to_rcst",
            Level::WARN,
        )))
        .with_filter(filter::filter_fn(level_for("candy_frontend", Level::DEBUG)))
        .with_filter(filter::filter_fn(level_for("candy_fuzzer", Level::DEBUG)))
        .with_filter(filter::filter_fn(level_for(
            "candy_fuzzer::fuzzer",
            Level::INFO,
        )))
        .with_filter(filter::filter_fn(level_for(
            "candy_language_server",
            Level::TRACE,
        )))
        .with_filter(filter::filter_fn(level_for(
            "candy_language_server::features_candy::analyzer::module_analyzer",
            Level::INFO,
        )))
        .with_filter(filter::filter_fn(level_for("candy_vm", Level::DEBUG)))
        .with_filter(filter::filter_fn(level_for("candy_vm::heap", Level::DEBUG)));
    tracing_subscriber::registry().with(console_log).init();
}
fn level_for(module: &'static str, level: Level) -> impl Fn(&Metadata) -> bool {
    move |metadata| {
        if metadata
            .module_path()
            .unwrap_or_default()
            .starts_with(module)
        {
            metadata.level() <= &level
        } else {
            true
        }
    }
}
