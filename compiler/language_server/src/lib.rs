#![feature(
    anonymous_lifetime_in_impl_trait,
    async_closure,
    box_patterns,
    let_chains
)]
#![allow(clippy::all)]
// #![warn(clippy::nursery, clippy::pedantic, unused_crate_dependencies)]
// #![allow(
//     clippy::future_not_send, // TODO: Fix clippy::future_not_send occurrences
//     clippy::large_enum_variant,
//     clippy::match_same_arms,
//     clippy::missing_errors_doc,
//     clippy::missing_panics_doc,
//     clippy::module_name_repetitions,
//     clippy::too_many_lines
// )]

pub mod database;
pub mod debug_adapter;
pub mod features;
pub mod features_candy;
pub mod features_ir;
mod semantic_tokens;
pub mod server;
pub mod utils;
