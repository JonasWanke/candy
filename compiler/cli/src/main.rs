mod database;

use candy_frontend::{
    ast_to_hir::AstToHir,
    cst_to_ast::CstToAst,
    error::CompilerError,
    hir,
    hir_to_mir::HirToMir,
    mir_optimize::OptimizeMir,
    module::{Module, ModuleFromPathError, ModuleKind, PackagesPath},
    position::PositionConversionDb,
    rcst_to_cst::RcstToCst,
    rich_ir::ToRichIr,
    string_to_rcst::StringToRcst,
    TracingConfig, TracingMode,
};
use candy_language_server::server::Server;
use candy_vm::{
    channel::{ChannelId, Packet},
    context::RunForever,
    fiber::{ExecutionResult, FiberId},
    heap::{Data, Heap, HirId, SendPort, Struct, Text},
    lir::Lir,
    mir_to_lir::compile_lir,
    tracer::{full::FullTracer, DummyTracer, Tracer},
    vm::{CompletedOperation, OperationId, Status, Vm},
};
use clap::{Parser, ValueHint};
use database::Database;
use itertools::Itertools;
use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;
use std::{
    borrow::Borrow,
    convert::TryInto,
    env::{current_dir, current_exe},
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::{mpsc::channel, Arc},
    time::Duration,
};
use tracing::{debug, error, info, warn, Level, Metadata};
use tracing_subscriber::{
    filter,
    fmt::{format::FmtSpan, writer::BoxMakeWriter},
    prelude::*,
};

#[derive(Parser, Debug)]
#[command(name = "candy", about = "The 🍭 Candy CLI.")]
enum CandyOptions {
    Build(CandyBuildOptions),
    Run(CandyRunOptions),
    Fuzz(CandyFuzzOptions),

    /// Start a Language Server.
    Lsp,
}

#[derive(Parser, Debug)]
struct CandyBuildOptions {
    #[arg(long)]
    debug: bool,

    #[arg(long)]
    watch: bool,

    #[arg(long)]
    tracing: bool,

    #[arg(value_hint = ValueHint::FilePath)]
    path: Option<PathBuf>,
}

/// Run a Candy program.
///
/// This command runs the given file, or, if no file is provided, the package of
/// your current working directory. The module should export a `main` function.
/// This function is then called with an environment.
#[derive(Parser, Debug)]
struct CandyRunOptions {
    #[arg(long)]
    debug: bool,

    #[arg(long)]
    tracing: bool,

    /// The file or package to run. If none is provided, the package of your
    /// current working directory will be run.
    #[arg(value_hint = ValueHint::FilePath)]
    path: Option<PathBuf>,
}

/// Fuzz a Candy module.
///
/// This command runs the given file or, if no file is provided, the package of
/// your current working directory. It finds all fuzzable functions and then
/// fuzzes them.
///
/// Fuzzable functions are functions written without curly braces.
#[derive(Parser, Debug)]
struct CandyFuzzOptions {
    #[arg(long)]
    debug: bool,

    #[arg(value_hint = ValueHint::FilePath)]
    path: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> ProgramResult {
    match CandyOptions::parse() {
        CandyOptions::Build(options) => build(options),
        CandyOptions::Run(options) => run(options),
        CandyOptions::Fuzz(options) => fuzz(options),
        CandyOptions::Lsp => lsp().await,
    }
}

type ProgramResult = Result<(), Exit>;
#[derive(Debug)]
enum Exit {
    CodePanicked,
    FileNotFound,
    FuzzingFoundFailingCases,
    NotInCandyPackage,
}

fn packages_path() -> PackagesPath {
    // We assume the candy executable lives inside the Candy Git repository at
    // its usual location, `$candy/target/[release or debug]/candy`.
    let candy_exe = current_exe().unwrap();
    let target_dir = candy_exe.parent().unwrap().parent().unwrap();
    let candy_repo = target_dir.parent().unwrap();
    PackagesPath::try_from(candy_repo.join("packages").as_path()).unwrap()
}
fn module_for_path(path: Option<PathBuf>) -> Result<Module, Exit> {
    let packages_path = packages_path();
    match path {
        Some(file) => {
            Module::from_path(&packages_path, &file, ModuleKind::Code).map_err(
                |error| match error {
                    ModuleFromPathError::NotFound(_) => {
                        error!("The given file doesn't exist.");
                        Exit::FileNotFound
                    }
                    ModuleFromPathError::NotInPackage(_) => {
                        error!("The given file is not in a Candy package.");
                        Exit::NotInCandyPackage
                    }
                },
            )
        }
        None => {
            let Some(package) = packages_path.find_surrounding_package(&current_dir().unwrap()) else {
                error!("You are not in a Candy package. Either navigate into a package or specify a Candy file.");
                error!("Candy packages are folders that contain a `_package.candy` file. This file marks the root folder of a package. Relative imports can only happen within the package.");
                return Err(Exit::NotInCandyPackage)
            };
            Ok(Module {
                package,
                path: vec![],
                kind: ModuleKind::Code,
            })
        }
    }
}

fn build(options: CandyBuildOptions) -> ProgramResult {
    init_logger(true);

    let packages_path = packages_path();
    let db = Database::new_with_file_system_module_provider(packages_path.clone());
    let module = module_for_path(options.path.clone())?;

    let tracing = TracingConfig {
        register_fuzzables: TracingMode::Off,
        calls: TracingMode::all_or_off(options.tracing),
        evaluated_expressions: TracingMode::all_or_off(options.tracing),
    };
    raw_build(&db, module.clone(), &tracing, options.debug);

    if options.watch {
        let (tx, rx) = channel();
        let mut debouncer = new_debouncer(Duration::from_secs(1), None, tx).unwrap();
        debouncer
            .watcher()
            .watch(
                &module.package.to_path(&packages_path).unwrap(),
                RecursiveMode::Recursive,
            )
            .unwrap();
        loop {
            match rx.recv() {
                Ok(_) => {
                    raw_build(&db, module.clone(), &tracing, options.debug);
                }
                Err(e) => error!("watch error: {e:#?}"),
            }
        }
    } else {
        Ok(())
    }
}
fn raw_build(db: &Database, module: Module, tracing: &TracingConfig, debug: bool) -> Arc<Lir> {
    debug!("Compiling `{}`.", module.to_rich_ir());

    let packages_path = packages_path();
    let rcst = db
        .rcst(module.clone())
        .unwrap_or_else(|err| panic!("Error parsing file `{}`: {:?}", module.to_rich_ir(), err));
    if debug {
        module.dump_associated_debug_file(
            &packages_path,
            "rcst",
            &format!("{}\n", rcst.to_rich_ir()),
        );
    }

    let cst = db.cst(module.clone()).unwrap();
    if debug {
        module.dump_associated_debug_file(&packages_path, "cst", &format!("{:#?}\n", cst));
    }

    let (asts, ast_cst_id_map) = db.ast(module.clone()).unwrap();
    if debug {
        module.dump_associated_debug_file(
            &packages_path,
            "ast",
            &format!("{}\n", asts.to_rich_ir()),
        );
        module.dump_associated_debug_file(
            &packages_path,
            "ast_to_cst_ids",
            &ast_cst_id_map
                .keys()
                .sorted_by_key(|it| it.local)
                .map(|key| {
                    format!(
                        "{} -> {}\n",
                        key.to_short_debug_string(),
                        ast_cst_id_map[key].0,
                    )
                })
                .join(""),
        );
    }

    let (hir, hir_ast_id_map) = db.hir(module.clone()).unwrap();
    if debug {
        module.dump_associated_debug_file(
            &packages_path,
            "hir",
            &format!("{}\n", hir.to_rich_ir()),
        );
        module.dump_associated_debug_file(
            &packages_path,
            "hir_to_ast_ids",
            &hir_ast_id_map
                .keys()
                .map(|key| {
                    format!(
                        "{} -> {}\n",
                        key.to_short_debug_string(),
                        hir_ast_id_map[key].to_short_debug_string(),
                    )
                })
                .join(""),
        );
    }

    let (mir, _) = db.mir(module.clone(), tracing.clone()).unwrap();
    if debug {
        module.dump_associated_debug_file(
            &packages_path,
            "mir",
            &format!("{}\n", mir.to_rich_ir()),
        );
    }

    let (optimized_mir, _) = db.optimized_mir(module.clone(), tracing.clone()).unwrap();
    if debug {
        module.dump_associated_debug_file(
            &packages_path,
            "optimized_mir",
            &format!("{}\n", optimized_mir.to_rich_ir()),
        );
    }

    let (lir, errors) = compile_lir(db, module.clone(), tracing.clone());
    if debug {
        module.dump_associated_debug_file(
            &packages_path,
            "lir",
            &format!("{}\n", lir.to_rich_ir()),
        );
    }

    for CompilerError {
        module,
        span,
        payload,
    } in errors.iter()
    {
        let range = db.range_to_positions(module.clone(), span.clone());
        warn!(
            "{}:{}:{} – {}:{}: {payload}",
            module.to_rich_ir(),
            range.start.line,
            range.start.character,
            range.end.line,
            range.end.character,
        );
    }

    lir
}

fn run(options: CandyRunOptions) -> ProgramResult {
    init_logger(true);

    let packages_path = packages_path();
    let db = Database::new_with_file_system_module_provider(packages_path.clone());
    let module = module_for_path(options.path.clone())?;

    let tracing = TracingConfig {
        register_fuzzables: TracingMode::Off,
        calls: TracingMode::all_or_off(options.tracing),
        evaluated_expressions: TracingMode::only_current_or_off(options.tracing),
    };
    let lir = raw_build(&db, module.clone(), &tracing, options.debug);

    debug!("Running {}.", module.to_rich_ir());

    let mut tracer = FullTracer::default();
    let result = Vm::for_module(lir.clone()).run_until_completion(&mut tracer);
    if options.debug {
        module.dump_associated_debug_file(&packages_path, "trace", &format!("{tracer:?}"));
    }

    let (mut heap, main) = match result {
        ExecutionResult::Finished(return_value) => return_value.into_main_function().unwrap(),
        ExecutionResult::Panicked {
            reason,
            responsible,
        } => {
            error!("The module panicked: {reason}");
            error!("{responsible} is responsible.");
            if let Some(span) = db.hir_id_to_span(responsible) {
                error!("Responsible is at {span:?}.");
            }
            error!(
                "This is the stack trace:\n{}",
                tracer.format_panic_stack_trace_to_root_fiber(&db),
            );
            return Err(Exit::CodePanicked);
        }
    };

    debug!("Running main function.");
    // TODO: Add more environment stuff.
    let mut vm = Vm::uninitialized(lir);
    let mut stdout = StdoutService::new(&mut vm);
    let mut stdin = StdinService::new(&mut vm);
    let fields = [
        ("Stdout", SendPort::create(&mut heap, stdout.channel)),
        ("Stdin", SendPort::create(&mut heap, stdin.channel)),
    ];
    let environment = Struct::create_with_symbol_keys(&mut heap, fields).into();
    let platform = HirId::create(&mut heap, hir::Id::platform());
    tracer.for_fiber(FiberId::root()).call_started(
        platform,
        main.into(),
        vec![environment],
        platform,
        &heap,
    );
    vm.initialize_for_closure(heap, main, &[environment], platform);
    loop {
        match vm.status() {
            Status::CanRun => {
                vm.run(&mut RunForever, &mut tracer);
            }
            Status::WaitingForOperations => {}
            _ => break,
        }
        stdout.run(&mut vm);
        stdin.run(&mut vm);
        vm.free_unreferenced_channels();
    }
    if options.debug {
        module.dump_associated_debug_file(&packages_path, "trace", &format!("{tracer:?}"));
    }
    match vm.tear_down() {
        ExecutionResult::Finished(return_value) => {
            tracer
                .for_fiber(FiberId::root())
                .call_ended(return_value.object, &return_value.heap);
            debug!("The main function returned:");
            debug!("{return_value:?}");
            Ok(())
        }
        ExecutionResult::Panicked {
            reason,
            responsible,
        } => {
            error!("The main function panicked: {reason}");
            error!("{responsible} is responsible.");
            error!(
                "This is the stack trace:\n{}",
                tracer.format_panic_stack_trace_to_root_fiber(&db)
            );
            Err(Exit::CodePanicked)
        }
    }
}

/// A state machine that corresponds to a loop that always calls `receive` on
/// the stdout channel and then logs that packet.
struct StdoutService {
    channel: ChannelId,
    current_receive: OperationId,
}
impl StdoutService {
    fn new<L: Borrow<Lir>>(vm: &mut Vm<L>) -> Self {
        let channel = vm.create_channel(0);
        let current_receive = vm.receive(channel);
        Self {
            channel,
            current_receive,
        }
    }
    fn run<L: Borrow<Lir>>(&mut self, vm: &mut Vm<L>) {
        while let Some(CompletedOperation::Received { packet }) =
            vm.completed_operations.remove(&self.current_receive)
        {
            match packet.object.into() {
                Data::Text(text) => println!("{}", text.get()),
                _ => info!("Non-text value sent to stdout: {packet:?}"),
            }
            self.current_receive = vm.receive(self.channel);
        }
    }
}
struct StdinService {
    channel: ChannelId,
    current_receive: OperationId,
}
impl StdinService {
    fn new<L: Borrow<Lir>>(vm: &mut Vm<L>) -> Self {
        let channel = vm.create_channel(0);
        let current_receive = vm.receive(channel);
        Self {
            channel,
            current_receive,
        }
    }
    fn run<L: Borrow<Lir>>(&mut self, vm: &mut Vm<L>) {
        while let Some(CompletedOperation::Received { packet }) =
            vm.completed_operations.remove(&self.current_receive)
        {
            let request: SendPort = packet
                .object
                .try_into()
                .expect("Expected a send port to be sent to stdin.");
            print!(">> ");
            io::stdout().flush().unwrap();
            let input = {
                let stdin = io::stdin();
                stdin.lock().lines().next().unwrap().unwrap()
            };
            let packet = {
                let mut heap = Heap::default();
                let object = Text::create(&mut heap, &input).into();
                Packet { heap, object }
            };
            vm.send(&mut DummyTracer, request.channel_id(), packet);

            // Receive the next request
            self.current_receive = vm.receive(self.channel);
        }
    }
}

fn fuzz(options: CandyFuzzOptions) -> ProgramResult {
    init_logger(true);

    let db = Database::new_with_file_system_module_provider(packages_path());
    let module = module_for_path(options.path.clone())?;

    let tracing = TracingConfig {
        register_fuzzables: TracingMode::All,
        calls: TracingMode::Off,
        evaluated_expressions: TracingMode::Off,
    };

    raw_build(&db, module.clone(), &tracing, options.debug);

    debug!("Fuzzing `{}`.", module.to_rich_ir());
    let failing_cases = candy_fuzzer::fuzz(&db, module);

    if failing_cases.is_empty() {
        info!("All found fuzzable closures seem fine.");
        Ok(())
    } else {
        error!("");
        error!("Finished fuzzing.");
        error!("These are the failing cases:");
        for case in failing_cases {
            error!("");
            case.dump(&db);
        }
        Err(Exit::FuzzingFoundFailingCases)
    }
}

async fn lsp() -> ProgramResult {
    init_logger(false);
    info!("Starting language server…");
    let (service, socket) = Server::create(packages_path());
    tower_lsp::Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
    Ok(())
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
