#![feature(
    anonymous_lifetime_in_impl_trait,
    box_patterns,
    if_let_guard,
    let_chains,
    try_blocks,
    unsigned_is_multiple_of
)]
#![warn(clippy::nursery, clippy::pedantic, unused_crate_dependencies)]
#![allow(
    clippy::cognitive_complexity,
    clippy::match_same_arms,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::similar_names,
    clippy::too_many_lines
)]

// Allows macros to refer to this crate as `::candy_compiler_v4`
extern crate self as candy_compiler_v4;

use ast::CollectAstErrors;
use ast_to_hir::ast_to_hir;
use clap::{arg, Parser, Subcommand, ValueHint};
use error::CompilerError;
use hir::Hir;
use hir_to_mono::hir_to_mono;
use itertools::Itertools;
use mono_to_c::mono_to_c;
use position::{Position, RangeOfOffset};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process,
    time::{Duration, Instant},
};
use to_text::ToText;
use tracing::{debug, error, info, warn, Level, Metadata};
use tracing_subscriber::{
    filter, fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt, Layer,
};

mod ast;
mod ast_to_hir;
mod error;
mod hir;
mod hir_to_mono;
mod id;
mod memory_layout;
mod mono;
mod mono_to_c;
mod position;
mod string_to_ast;
mod to_text;
mod type_solver;
mod utils;

#[derive(Parser, Debug)]
#[command(name = "candy", about = "The 🍭 Candy CLI.")]
enum CandyOptions {
    #[command(subcommand)]
    Debug(DebugOptions),
    Check(CheckOptions),
    Compile(CompileOptions),
    ToolingAnalyze(ToolingAnalyzeOptions),
}

fn main() -> ProgramResult {
    let options = CandyOptions::parse();

    init_logger();

    match options {
        CandyOptions::Debug(options) => debug(options),
        CandyOptions::Check(options) => check(options),
        CandyOptions::Compile(options) => compile(options),
        CandyOptions::ToolingAnalyze(options) => {
            tooling_analyze(options);
            Ok(())
        }
    }
}
pub type ProgramResult = Result<(), Exit>;
#[derive(Debug)]
pub enum Exit {
    FileNotFound,
    CodeContainsErrors,
}

#[derive(Subcommand, Debug)]
enum DebugOptions {
    Ast(DebugStageOptions),
    Hir(DebugStageOptions),
    Mono(DebugStageOptions),
}
#[derive(Parser, Debug)]
struct DebugStageOptions {
    #[arg(value_hint = ValueHint::FilePath)]
    path: PathBuf,
}

#[allow(clippy::needless_pass_by_value)]
fn debug(options: DebugOptions) -> ProgramResult {
    match options {
        DebugOptions::Ast(options) => {
            let source = fs::read_to_string(&options.path).unwrap();
            let ast = string_to_ast::string_to_ast(&options.path, &source);
            println!("{ast:#?}");
        }
        DebugOptions::Hir(options) => {
            let source = fs::read_to_string(&options.path).unwrap();
            let (hir, errors) = compile_hir(&options.path, &source);
            println!("{}", hir.to_text(true));

            if !errors.is_empty() {
                for error in errors {
                    error!("{}", error.to_string_with_location(&source));
                }
                return Err(Exit::CodeContainsErrors);
            }
        }
        DebugOptions::Mono(options) => {
            let source = fs::read_to_string(&options.path).unwrap();
            let (hir, errors) = compile_hir(&options.path, &source);
            if !errors.is_empty() {
                for error in errors {
                    error!("{}", error.to_string_with_location(&source));
                }
                return Err(Exit::CodeContainsErrors);
            }

            let mono = hir_to_mono(&hir);
            println!("{}", mono.to_text(true));
        }
    }
    Ok(())
}

#[derive(Parser, Debug)]
struct CheckOptions {
    /// The file or package to check.
    #[arg(value_hint = ValueHint::FilePath)]
    path: PathBuf,
}

#[allow(clippy::needless_pass_by_value)]
fn check(options: CheckOptions) -> ProgramResult {
    let source = fs::read_to_string(&options.path).unwrap();

    let started_at = Instant::now();
    let (_, errors) = compile_hir(&options.path, &source);
    debug!("Check took {}.", format_duration(started_at.elapsed()));

    if errors.is_empty() {
        info!("No errors found 🎉");
        Ok(())
    } else {
        for error in errors {
            error!("{}", error.to_string_with_location(&source));
        }
        Err(Exit::CodeContainsErrors)
    }
}

#[derive(Parser, Debug)]
struct CompileOptions {
    /// The file or package to compile to C.
    #[arg(value_hint = ValueHint::FilePath)]
    path: PathBuf,
}

#[allow(clippy::needless_pass_by_value)]
fn compile(options: CompileOptions) -> ProgramResult {
    let source = fs::read_to_string(&options.path).unwrap();

    let started_at = Instant::now();
    let (hir, errors) = compile_hir(&options.path, &source);

    if !errors.is_empty() {
        for error in errors {
            error!("{}", error.to_string_with_location(&source));
        }
        return Err(Exit::CodeContainsErrors);
    }

    let mono = hir_to_mono(&hir);

    let c_code = mono_to_c(&mono);
    debug!(
        "Compilation to C took {}.",
        format_duration(started_at.elapsed())
    );

    let c_path = options.path.with_extension("c");
    fs::write(&c_path, c_code).unwrap();
    process::Command::new("clang-format")
        .args(["-i", c_path.to_str().unwrap()])
        .spawn()
        .unwrap()
        .wait()
        .unwrap();

    let executable_path = options.path.with_extension("");
    process::Command::new("gcc")
        .args([
            c_path.to_str().unwrap(),
            "-O3",
            "-o",
            executable_path.to_str().unwrap(),
        ])
        .spawn()
        .unwrap()
        .wait()
        .unwrap();

    info!("Done 🎉");
    Ok(())
}

#[derive(Parser, Debug)]
struct ToolingAnalyzeOptions {
    /// The file to analyze.
    #[arg(value_hint = ValueHint::FilePath)]
    path: PathBuf,
}
#[allow(clippy::needless_pass_by_value)]
fn tooling_analyze(options: ToolingAnalyzeOptions) {
    let path_str = options.path.to_string_lossy();
    let path = path_str.strip_prefix("file:/").unwrap();
    let source = fs::read_to_string(Path::new(path))
        .unwrap_or_else(|err| panic!("Couldn't open file `{path}`: {err:?}"));

    let (_, errors) = compile_hir(&options.path, &source);

    // TODO: request file content from VS Code extension

    let diagnostics = errors
        .into_iter()
        .map(|error| {
            let span = error.span.to_positions(&source);
            Diagnostic {
                message: error.message.into_boxed_str(),
                source: DiagnosticSource {
                    file: error.path.to_string_lossy().into_owned().into_boxed_str(),
                    start: span.start,
                    end: span.end,
                },
            }
        })
        .collect_vec();
    println!(
        "{}",
        serde_json::to_string(&Message::Diagnostics { diagnostics }).unwrap(),
    );
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Message {
    #[allow(dead_code)]
    ReadFile {
        path: String,
    },
    Diagnostics {
        diagnostics: Vec<Diagnostic>,
    },
}
#[derive(Serialize)]
struct Diagnostic {
    pub message: Box<str>,
    pub source: DiagnosticSource,
}
#[derive(Serialize)]
struct DiagnosticSource {
    pub file: Box<str>,
    pub start: Position,
    pub end: Position,
}

fn compile_hir(path: &Path, source: &str) -> (Hir, Vec<CompilerError>) {
    let asts = string_to_ast::string_to_ast(path, source);
    let mut errors = asts.collect_errors();

    let (hir, mut hir_errors) = ast_to_hir(path, &asts);
    errors.append(&mut hir_errors);

    (hir, errors)
}

fn format_duration(duration: Duration) -> String {
    if duration < Duration::from_millis(1) {
        format!("{} µs", duration.as_micros())
    } else {
        format!("{} ms", duration.as_millis())
    }
}

fn init_logger() {
    let console_log = tracing_subscriber::fmt::layer()
        .compact()
        .with_writer(std::io::stderr)
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
            "candy_v4::string_to_ast",
            Level::DEBUG,
        )));
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
