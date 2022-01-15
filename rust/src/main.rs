mod builtin_functions;
mod compiler;
mod interpreter;

use crate::compiler::ast_to_hir::CompileVecAstsToHir;
use crate::compiler::cst_to_ast::LowerCstToAst;
use crate::compiler::string_to_cst::StringToCst;
use crate::interpreter::fiber::FiberStatus;
use crate::interpreter::*;
use log;
use simplelog::{ColorChoice, Config, LevelFilter, TermLogger, TerminalMode};
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "candy", about = "The 🍭 Candy CLI.")]
struct CandyRunOptions {
    #[structopt(long)]
    print_cst: bool,

    #[structopt(long)]
    print_ast: bool,

    #[structopt(long)]
    print_hir: bool,

    #[structopt(long)]
    no_run: bool,

    #[structopt(parse(from_os_str))]
    file: PathBuf,
}

fn main() {
    TermLogger::init(
        LevelFilter::Debug,
        Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .unwrap();

    let options = CandyRunOptions::from_args();
    log::debug!("{:#?}", options);
    run(options)
}

fn run(options: CandyRunOptions) {
    log::debug!("Running test.candy.\n");

    let test_code = std::fs::read_to_string(options.file).expect("File test.candy not found.");

    log::info!("Parsing string to CST…");
    let cst = test_code.parse_cst();
    if options.print_cst {
        log::info!("CST: {:?}", cst);
    }

    log::info!("Lowering CST to AST…");
    let (asts, errors) = cst.into_ast();
    if options.print_ast {
        log::info!("AST: {:?}", asts);
    }
    if !errors.is_empty() {
        log::error!("Errors occurred while lowering CST to AST:\n{:?}", errors);
        return;
    }

    log::info!("Compiling AST to HIR…");
    let lambda = asts.compile_to_hir();
    if options.print_hir {
        log::info!("HIR: {:?}", lambda);
    }

    if !options.no_run {
        log::info!("Executing code…");
        let mut fiber = fiber::Fiber::new(lambda);
        fiber.run();
        match fiber.status() {
            FiberStatus::Running => log::info!("Fiber is still running."),
            FiberStatus::Done(value) => log::info!("Fiber is done: {:?}", value),
            FiberStatus::Panicked(value) => log::error!("Fiber panicked: {:?}", value),
        }
    }

    // let code = {
    //     let core_code = std::fs::read_to_string("core.candy").expect("File core.candy not found.");
    //     let test_code = std::fs::read_to_string("test.candy").expect("File test.candy not found.");
    //     format!("{}\n{}", core_code, test_code)
    // };

    // let ast = match code.parse_to_asts() {
    //     Ok(it) => it,
    //     Err(err) => panic!("Couldn't parse ASTs of core.candy: {}", err),
    // };
    // debug!("AST: {}\n", &ast);

    // let mut hir = ast.compile_to_hir();
    // hir.optimize();
    // debug!("HIR: {}", hir);

    // let mut lir = hir.compile_to_lir();
    // lir.optimize();
    // debug!("LIR: {}", lir);

    // debug!("Compiling to byte code...");
    // let byte_code = lir.compile_to_byte_code();
    // debug!("Byte code: {:?}", byte_code);

    // debug!("Running in VM...");
    // let mut ambients = HashMap::new();
    // ambients.insert("stdout".into(), Value::ChannelSendEnd(0));
    // ambients.insert("stdin".into(), Value::ChannelReceiveEnd(1));
    // let mut fiber = Fiber::new(byte_code, ambients, Value::unit());
    // loop {
    //     fiber.run(30);
    //     match fiber.status() {
    //         FiberStatus::Running => {}
    //         FiberStatus::Done(value) => {
    //             println!("{}", format!("Done running: {}", value).green());
    //             break;
    //         }
    //         FiberStatus::Panicked(value) => {
    //             println!("{}", format!("Panicked: {}", value).red());
    //             break;
    //         }
    //         FiberStatus::Sending(channel_id, message) => match channel_id {
    //             0 => {
    //                 let mut out = stdout();
    //                 out.write(
    //                     if let Value::String(string) = message {
    //                         string
    //                     } else {
    //                         message.to_string()
    //                     }
    //                     .as_bytes(),
    //                 )
    //                 .unwrap();
    //                 out.flush().unwrap();
    //                 fiber.resolve_sending();
    //             }
    //             _ => panic!("Unknown channel id {}.", channel_id),
    //         },
    //         FiberStatus::Receiving(channel_id) => match channel_id {
    //             1 => {
    //                 let mut input = String::new();
    //                 std::io::stdin()
    //                     .read_line(&mut input)
    //                     .expect("Couldn't read line.");
    //                 fiber.resolve_receiving(Value::String(input));
    //             }
    //             _ => panic!("Unknown channel id {}.", channel_id),
    //         },
    //     }
    // }
    // debug!("{:?}", fiber);
}
