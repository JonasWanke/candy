mod fuzzer;
mod generator;
mod utils;

pub use self::{
    fuzzer::{Fuzzer, Status},
    utils::FuzzablesFinder,
};
use crate::{
    compiler::{hir::Id, TracingConfig, TracingMode},
    database::Database,
    module::Module,
    vm::{
        context::{DbUseProvider, RunForever, RunLimitedNumberOfInstructions},
        tracer::full::FullTracer,
        Closure, Heap, Packet, Pointer, Vm,
    },
};
use itertools::Itertools;
use std::collections::HashMap;
use tracing::{error, info};

pub async fn fuzz(db: &Database, module: Module) -> Vec<FailingFuzzCase> {
    let tracing = TracingConfig {
        register_fuzzables: TracingMode::All,
        calls: TracingMode::Off,
        evaluated_expressions: TracingMode::Off,
    };

    let (fuzzables_heap, fuzzables): (Heap, HashMap<Id, Pointer>) = {
        let mut tracer = FuzzablesFinder::default();
        let mut vm = Vm::default();
        vm.set_up_for_running_module_closure(
            module.clone(),
            Closure::of_module(db, module, tracing.clone()).unwrap(),
        );
        vm.run(
            &DbUseProvider {
                db,
                tracing: tracing.clone(),
            },
            &mut RunForever,
            &mut tracer,
        );
        (tracer.heap, tracer.fuzzables)
    };

    info!(
        "Now, the fuzzing begins. So far, we have {} closures to fuzz.",
        fuzzables.len()
    );

    let mut failing_cases = vec![];

    for (id, closure) in fuzzables {
        info!("Fuzzing {id}.");
        let mut fuzzer = Fuzzer::new(&fuzzables_heap, closure, id.clone());
        fuzzer.run(
            &mut DbUseProvider {
                db,
                tracing: tracing.clone(),
            },
            &mut RunLimitedNumberOfInstructions::new(100000),
        );
        match fuzzer.into_status() {
            Status::StillFuzzing { .. } => {}
            Status::PanickedForArguments {
                arguments,
                reason,
                responsible,
                tracer,
            } => {
                error!("The fuzzer discovered an input that crashes {id}:");
                let case = FailingFuzzCase {
                    closure: id,
                    arguments,
                    reason,
                    responsible,
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
    closure: Id,
    arguments: Vec<Packet>,
    reason: String,
    responsible: Id,
    tracer: FullTracer,
}

impl FailingFuzzCase {
    pub fn dump(&self, db: &Database) {
        error!(
            "Calling `{} {}` panics: {}",
            self.closure,
            self.arguments
                .iter()
                .map(|arg| format!("{arg:?}"))
                .join(" "),
            self.reason,
        );
        error!("{} is responsible.", self.responsible,);
        error!(
            "This is the stack trace:\n{}",
            self.tracer.format_panic_stack_trace_to_root_fiber(db)
        );
    }
}
