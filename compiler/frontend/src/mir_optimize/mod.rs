//! Optimizations are a necessity for Candy code to run reasonably fast. For
//! example, without optimizations, if two modules import a third module using
//! `use "..foo"`, then the `foo` module is instantiated twice completely
//! separately. Because this module can in turn depend on other modules, this
//! approach would lead to exponential code blowup.
//!
//! When optimizing code in general, there are two main objectives:
//!
//! - Making the code fast.
//! - Making the code small.
//!
//! Some optimizations benefit both of these objectives. For example, removing
//! ignored computations from the program makes it smaller, but also means
//! there's less code to be executed. Other optimizations further one objective,
//! but harm the other. For example, inlining functions (basically copying their
//! code to where they're used), can make the code bigger, but also potentially
//! faster because there are less function calls to be performed.
//!
//! Depending on the use case, the tradeoff between both objectives changes. To
//! put you in the right mindset, here are just two use cases:
//!
//! - Programming for a microcontroller with 1 MB of ROM available for the
//!   program. In this case, you want your code to be as fast as possible while
//!   still fitting in 1 MB. Interestingly, the importance of code size is a
//!   step function: There's no benefit in only using 0.5 MB, but 1.1 MB makes
//!   the program completely unusable.
//!
//! - Programming for a WASM module to be downloaded. In this case, you might
//!   have some concrete measurements on how performance and download size
//!   affect user retention.
//!
//! It should be noted that we can't judge performance statically. Although some
//! optimizations such as inlining typically improve performance, there are rare
//! cases where they don't. For example, inlining a function that's used in
//! multiple places means the CPU's branch predictor can't benefit from the
//! knowledge gained by previous function executions. Inlining might also make
//! your program bigger, causing more cache misses. Thankfully, Candy is not yet
//! optimized enough for us to care about such details.
//!
//! This module contains several optimizations. All of them operate on the MIR.
//! Some are called "obvious". Those are optimizations that typically improve
//! both performance and code size. Whenever they can be applied, they should be
//! applied.

use self::{
    current_expression::{Context, CurrentExpression},
    inlining::InliningState,
    log::OptimizationLogger,
    pure::PurenessInsights,
};
use super::{hir, hir_to_mir::HirToMir, mir::Mir, tracing::TracingConfig};
use crate::{
    error::CompilerError,
    hir_to_mir::ExecutionTarget,
    mir::{Body, Expression, MirError, VisibleExpressions},
    string_to_rcst::ModuleError,
    utils::DoHash,
};
use rustc_hash::FxHashSet;
use std::{mem, sync::Arc};
use tracing::debug;

mod after_panic;
mod call_tracing;
mod cleanup;
mod common_subtree_elimination;
mod complexity;
mod constant_folding;
mod constant_lifting;
mod current_expression;
mod inlining;
mod log;
mod module_folding;
mod pure;
mod reference_following;
mod tail_calls;
mod tree_shaking;
mod utils;
mod validate;

#[salsa::query_group(OptimizeMirStorage)]
pub trait OptimizeMir: HirToMir {
    fn optimized_mir(&self, target: ExecutionTarget, tracing: TracingConfig) -> OptimizedMirResult;

    #[salsa::cycle(recover_from_cycle)]
    fn optimized_mir_without_tail_calls(
        &self,
        target: ExecutionTarget,
        tracing: TracingConfig,
    ) -> OptimizedMirWithoutTailCallsResult;
}

pub type OptimizedMirResult = Result<(Arc<Mir>, Arc<FxHashSet<CompilerError>>), ModuleError>;

pub type OptimizedMirWithoutTailCallsResult = Result<
    (
        Arc<Mir>,
        Arc<PurenessInsights>,
        Arc<FxHashSet<CompilerError>>,
    ),
    ModuleError,
>;

#[allow(clippy::needless_pass_by_value)]
fn optimized_mir(
    db: &dyn OptimizeMir,
    target: ExecutionTarget,
    tracing: TracingConfig,
) -> OptimizedMirResult {
    let (mir, _, errors) = db.optimized_mir_without_tail_calls(target, tracing)?;
    let mut mir = (*mir).clone();

    tail_calls::simplify_tail_call_tracing(&mut mir);

    Ok((Arc::new(mir), errors))
}

#[allow(clippy::needless_pass_by_value)]
fn optimized_mir_without_tail_calls(
    db: &dyn OptimizeMir,
    target: ExecutionTarget,
    tracing: TracingConfig,
) -> OptimizedMirWithoutTailCallsResult {
    let module = target.module();
    debug!("{module}: Compiling.");
    OptimizationLogger::log_optimized_mir_without_tail_calls_start(&target, tracing);
    let (mir, errors) = db.mir(target.clone(), tracing)?;
    let mut mir = (*mir).clone();
    let mut pureness = PurenessInsights::default();
    let mut errors = (*errors).clone();

    let complexity_before = mir.complexity();
    mir.optimize(db, &tracing, &mut pureness, &mut errors);
    let complexity_after = mir.complexity();

    debug!("{module}: Done. Optimized from {complexity_before} to {complexity_after}");
    OptimizationLogger::log_optimized_mir_without_tail_calls_end(
        complexity_before,
        complexity_after,
    );
    Ok((Arc::new(mir), Arc::new(pureness), Arc::new(errors)))
}

impl Mir {
    pub fn optimize(
        &mut self,
        db: &dyn OptimizeMir,
        tracing: &TracingConfig,
        pureness: &mut PurenessInsights,
        errors: &mut FxHashSet<CompilerError>,
    ) {
        let mut context = Context {
            db,
            tracing,
            errors,
            visible: &mut VisibleExpressions::none_visible(),
            id_generator: &mut self.id_generator,
            pureness,
            inlining_state: InliningState::default(),
        };
        context.optimize_body(&mut self.body);
        if cfg!(debug_assertions) {
            self.validate();
        }
        self.cleanup(pureness);
    }
}

impl Context<'_> {
    fn optimize_body(&mut self, body: &mut Body) {
        // Even though `self.visible` is mutable, this function guarantees that
        // the value is the same after returning.
        OptimizationLogger::log_optimize_body_start(body);
        let mut index = 0;
        while index < body.expressions.len() {
            // Thoroughly optimize the expression.
            let mut expression = CurrentExpression::new(body, index);
            self.optimize_expression(&mut expression);
            if cfg!(debug_assertions) {
                expression.validate(self.visible);
            }
            self.pureness.visit_optimized(expression.id(), &expression);

            module_folding::apply(self, &mut expression);

            let new_id = expression.id();
            index = expression.index() + 1;
            let expression = mem::replace(expression.get_mut_carefully(), Expression::Parameter);
            self.visible.insert(new_id, expression);
        }

        for (id, expression) in &mut body.expressions {
            *expression = self.visible.remove(*id);
        }

        after_panic::remove_expressions_after_panic(body, self.pureness);
        common_subtree_elimination::eliminate_common_subtrees(body, self.pureness);
        {
            // Reference following
            let mut index = 0;
            while index < body.expressions.len() {
                // Thoroughly optimize the expression.
                let mut expression = CurrentExpression::new(body, index);
                reference_following::follow_references(self, &mut expression);
                if cfg!(debug_assertions) {
                    expression.validate(self.visible);
                }
                self.pureness.visit_optimized(expression.id(), &expression);

                let new_id = expression.id();
                index = expression.index() + 1;
                let expression =
                    mem::replace(expression.get_mut_carefully(), Expression::Parameter);
                self.visible.insert(new_id, expression);
            }
            for (id, expression) in &mut body.expressions {
                *expression = self.visible.remove(*id);
            }
        }
        call_tracing::remove_unnecessary_call_tracing(body, self.pureness, self.tracing.calls);
        tree_shaking::tree_shake(body, self.pureness);
        reference_following::remove_redundant_return_references(body, self.pureness);
        OptimizationLogger::log_optimize_body_end();
    }

    fn optimize_expression(&mut self, expression: &mut CurrentExpression) {
        OptimizationLogger::log_optimize_expression_start(expression);
        'outer: loop {
            if let Expression::Function {
                parameters,
                responsible_parameter,
                body,
                ..
            } = expression.get_mut_carefully()
            {
                for parameter in &*parameters {
                    self.visible.insert(*parameter, Expression::Parameter);
                }
                self.visible
                    .insert(*responsible_parameter, Expression::Parameter);
                self.pureness
                    .enter_function(parameters, *responsible_parameter);

                self.optimize_body(body);

                for parameter in &*parameters {
                    self.visible.remove(*parameter);
                }
                self.visible.remove(*responsible_parameter);
            }

            loop {
                let hashcode_before = expression.do_hash();

                reference_following::follow_references(self, expression);
                constant_folding::fold_constants(self, expression);

                let is_call = matches!(**expression, Expression::Call { .. });
                inlining::inline_tiny_functions(self, expression);
                inlining::inline_needs_function(self, expression);
                inlining::inline_functions_containing_use(self, expression);
                inlining::inline_calls_with_constant_arguments(self, expression);
                if is_call && matches!(**expression, Expression::Function { .. }) {
                    // We inlined a function call and the resulting code starts with
                    // a function definition. We need to visit that first before
                    // continuing the optimizations.
                    continue 'outer;
                }

                constant_lifting::lift_constants(self, expression);

                if expression.do_hash() == hashcode_before {
                    break 'outer;
                }
            }
        }
        OptimizationLogger::log_optimize_expression_end();
    }
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::unnecessary_wraps)]
fn recover_from_cycle(
    _db: &dyn OptimizeMir,
    cycle: &[String],
    target: &ExecutionTarget,
    _tracing: &TracingConfig,
) -> OptimizedMirWithoutTailCallsResult {
    let error = CompilerError::for_whole_module(
        target.module().clone(),
        MirError::ModuleHasCycle {
            cycle: cycle.to_vec(),
        },
    );

    let mir = Mir::build(|body| {
        let reason = body.push_text(error.payload.to_string());
        let responsible = body.push_hir_id(hir::Id::new(target.module().clone(), vec![]));
        body.push_panic(reason, responsible);
    });

    Ok((
        Arc::new(mir),
        Arc::default(),
        Arc::new(FxHashSet::from_iter([error])),
    ))
}
