#![feature(
    allocator_api,
    anonymous_lifetime_in_impl_trait,
    let_chains,
    slice_ptr_get,
    strict_provenance
)]

use crate::heap::{Struct, Tag};
use context::RunForever;
use fiber::{ExecutionEnded, ExecutionEndedReason};
use heap::{Function, Heap, HeapObject, InlineObject};
use lir::Lir;
use rustc_hash::FxHashMap;
use std::borrow::Borrow;
use tracer::{FiberTracer, Tracer};
use tracing::{debug, error};
use vm::{Status, Vm};

mod builtin_functions;
pub mod channel;
pub mod context;
pub mod fiber;
pub mod heap;
pub mod lir;
pub mod mir_to_lir;
pub mod tracer;
mod utils;
pub mod vm;

impl<'c, 'h, L: Borrow<Lir<'c>>, T: Tracer<'h>> Vm<'c, 'h, L, T> {
    pub fn run_until_completion(mut self, tracer: &mut T) -> ExecutionEnded<'c, 'h, T::ForFiber> {
        self.run(&mut RunForever, tracer);
        if let Status::WaitingForOperations = self.status() {
            error!("The module waits on channel operations. Perhaps, the code tried to read from a channel without sending a packet into it.");
            // TODO: Show stack traces of all fibers?
        }
        self.tear_down()
    }
}

impl<'c, 'h, T: FiberTracer<'h>> ExecutionEnded<'c, 'h, T> {
    pub fn into_main_function(
        mut self,
    ) -> Result<
        (
            Heap<'h>,
            Function<'h>,
            FxHashMap<HeapObject<'c>, HeapObject<'h>>,
        ),
        String,
    > {
        match self.reason {
            ExecutionEndedReason::Finished(return_value) => {
                match return_value_into_main_function(&mut self.heap, return_value) {
                    Ok(main) => Ok((self.heap, main, self.constant_mapping)),
                    Err(err) => Err(err.to_string()),
                }
            }
            ExecutionEndedReason::Panicked(panic) => Err(format!(
                "The module panicked at {}: {}",
                panic.responsible, panic.reason,
            )),
        }
    }
}

pub fn return_value_into_main_function<'h>(
    heap: &mut Heap<'h>,
    return_value: InlineObject<'h>,
) -> Result<Function<'h>, &'static str> {
    let exported_definitions: Struct = return_value.try_into().unwrap();
    debug!("The module exports these definitions: {exported_definitions}");

    let main = Tag::create_from_str(heap, "Main", None);
    exported_definitions
        .get(main)
        .ok_or("The module doesn't export a main function.")
        .and_then(|main| {
            main.try_into()
                .map_err(|_| "The exported main object is not a function.")
        })
}
