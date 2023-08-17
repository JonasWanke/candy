#![feature(let_chains, round_char_boundary)]
#![warn(clippy::nursery, clippy::pedantic)]
#![allow(clippy::missing_panics_doc, clippy::module_name_repetitions)]

mod coverage;
mod fuzzer;
mod input;
mod input_pool;
mod runner;
mod utils;
mod values;

use self::input::Input;
pub use self::{
    fuzzer::{Fuzzer, Status},
    input_pool::InputPool,
    runner::RunResult,
    utils::FuzzablesFinder,
};
use candy_frontend::{
    ast_to_hir::AstToHir,
    cst::CstDb,
    mir_optimize::OptimizeMir,
    module::Module,
    position::PositionConversionDb,
    {hir::Id, TracingConfig, TracingMode},
};
use candy_vm::{
    heap::Heap, mir_to_lir::compile_lir, tracer::stack_trace::StackTracer, Panic, Vm, VmFinished,
};
use std::rc::Rc;
use tracing::{debug, error, info};

pub fn fuzz<DB>(db: &DB, module: Module) -> Vec<FailingFuzzCase>
where
    DB: AstToHir + CstDb + OptimizeMir + PositionConversionDb,
{
    let tracing = TracingConfig {
        register_fuzzables: TracingMode::All,
        calls: TracingMode::Off,
        evaluated_expressions: TracingMode::Off,
    };
    let (lir, _) = compile_lir(db, module, tracing);
    let lir = Rc::new(lir);

    let VmFinished {
        tracer: FuzzablesFinder { fuzzables },
        ..
    } = Vm::for_module(lir.clone(), FuzzablesFinder::default()).run_forever_without_handles();

    info!(
        "Now, the fuzzing begins. So far, we have {} functions to fuzz.",
        fuzzables.len(),
    );

    let mut failing_cases = vec![];

    for (id, function) in fuzzables {
        info!("Fuzzing {id}.");
        let mut fuzzer = Fuzzer::new(lir.clone(), function, id.clone());
        fuzzer.run(100_000);

        match fuzzer.into_status() {
            Status::StillFuzzing { total_coverage, .. } => {
                let coverage = total_coverage
                    .in_range(&lir.range_of_function(&id))
                    .relative_coverage();
                debug!("Achieved a coverage of {:.1} %.", coverage * 100.0);
            }
            Status::FoundPanic {
                input,
                panic,
                heap,
                tracer,
            } => {
                error!("The fuzzer discovered an input that crashes {id}:");
                let case = FailingFuzzCase {
                    function: id,
                    input,
                    panic,
                    heap,
                    tracer,
                };
                case.dump(db);
                failing_cases.push(case);
            }
        }
    }

    failing_cases
}

pub struct FailingFuzzCase {
    function: Id,
    input: Input,
    panic: Panic,
    #[allow(dead_code)]
    heap: Heap,
    #[allow(dead_code)]
    tracer: StackTracer,
}

impl FailingFuzzCase {
    #[allow(unused_variables)]
    pub fn dump<DB>(&self, db: &DB)
    where
        DB: AstToHir + PositionConversionDb,
    {
        error!(
            "Calling `{} {}` panics: {}",
            self.function, self.input, self.panic.reason,
        );
        error!("{} is responsible.", self.panic.responsible);
        // Segfaults: https://github.com/candy-lang/candy/issues/458
        // error!(
        //     "This is the stack trace:\n{}",
        //     self.tracer.format_panic_stack_trace_to_root_fiber(db),
        // );
    }
}
