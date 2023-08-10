#![feature(lazy_cell)]

use clap::Parser;
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
mod inkwell;
mod lsp;
mod run;
mod services;
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

    Inkwell(inkwell::Options),
}

#[tokio::main]
async fn main() -> ProgramResult {
    let options = CandyOptions::parse();

    let should_log_to_stdout = !matches!(options, CandyOptions::Lsp);
    init_logger(should_log_to_stdout);

    match options {
        CandyOptions::Run(options) => run::run(options),
        CandyOptions::Check(options) => check::check(options),
        CandyOptions::Fuzz(options) => fuzz::fuzz(options),
        CandyOptions::Debug(options) => debug::debug(options),
        CandyOptions::Lsp => lsp::lsp().await,
        CandyOptions::Inkwell(options) => inkwell::compile(options),
    }
}

type ProgramResult = Result<(), Exit>;
#[derive(Debug)]
enum Exit {
    CodePanicked,
    DirectoryNotFound,
    ExternalError,
    FileNotFound,
    FuzzingFoundFailingCases,
    NotInCandyPackage,
    CodeContainsErrors,
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
            "candy_language_server",
            Level::TRACE,
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
