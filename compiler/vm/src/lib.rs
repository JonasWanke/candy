#![feature(
    addr_parse_ascii,
    allocator_api,
    anonymous_lifetime_in_impl_trait,
    box_patterns,
    iterator_try_collect,
    let_chains,
    nonzero_ops,
    slice_ptr_get,
    step_trait,
    try_blocks
)]
// We can't enable `unused_crate_dependencies` since it reports false positives about
// dev-dependencies used in our benchmarks.
// https://github.com/rust-lang/rust/issues/57274
// https://github.com/rust-lang/rust/issues/95513
// https://github.com/rust-lang/rust-clippy/issues/4341
#![allow(clippy::all)]
// #![warn(clippy::nursery, clippy::pedantic)]
// #![allow(
//     clippy::large_enum_variant,
//     clippy::match_same_arms,
//     clippy::missing_errors_doc,
//     clippy::missing_panics_doc,
//     clippy::module_name_repetitions,
//     clippy::similar_names,
//     clippy::too_many_lines
// )]

pub use builtin_functions::CAN_USE_STDOUT;
pub use instruction_pointer::InstructionPointer;
pub use utils::PopulateInMemoryProviderFromFileSystem;
pub use vm::{Panic, StateAfterRun, StateAfterRunForever, Vm, VmFinished, VmHandleCall};

mod builtin_functions;
pub mod byte_code;
pub mod environment;
mod handle_id;
pub mod heap;
mod instruction_pointer;
mod instructions;
pub mod lir_to_byte_code;
pub mod tracer;
mod utils;
mod vm;
