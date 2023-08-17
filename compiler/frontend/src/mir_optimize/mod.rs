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
    pure::PurenessInsights,
};
use super::{hir, hir_to_mir::HirToMir, mir::Mir, tracing::TracingConfig};
use crate::{
    error::CompilerError,
    mir::{Body, Expression, MirError, VisibleExpressions},
    module::Module,
    string_to_rcst::ModuleError,
    utils::DoHash,
};
use rustc_hash::FxHashSet;
use std::{mem, sync::Arc};
use tracing::debug;

mod cleanup;
mod common_subtree_elimination;
mod complexity;
mod constant_folding;
mod constant_lifting;
mod current_expression;
mod inlining;
mod module_folding;
mod pure;
mod reference_following;
mod tree_shaking;
mod utils;
mod validate;

#[salsa::query_group(OptimizeMirStorage)]
pub trait OptimizeMir: HirToMir {
    #[salsa::cycle(recover_from_cycle)]
    fn optimized_mir(&self, module: Module, tracing: TracingConfig) -> OptimizedMirResult;
}

pub type OptimizedMirResult = Result<
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
    module: Module,
    tracing: TracingConfig,
) -> OptimizedMirResult {
    debug!("{module}: Compiling.");
    let (mir, errors) = db.mir(module.clone(), tracing.clone())?;
    let mut mir = (*mir).clone();
    let mut pureness = PurenessInsights::default();
    let mut errors = (*errors).clone();

    let complexity_before = mir.complexity();
    mir.optimize(db, &tracing, &mut pureness, &mut errors);
    let complexity_after = mir.complexity();

    debug!("{module}: Done. Optimized from {complexity_before} to {complexity_after}");
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
        let mut index = 0;
        while index < body.expressions.len() {
            // Thoroughly optimize the expression.
            let mut expression = CurrentExpression::new(body, index);
            self.optimize_expression(&mut expression);
            if cfg!(debug_assertions) {
                expression.validate(self.visible);
            }

            let id = expression.id();
            self.pureness.visit_optimized(id, &expression);

            module_folding::apply(self, &mut expression);

            index = expression.index() + 1;
            let expression = mem::replace(&mut *expression, Expression::Parameter);
            self.visible.insert(id, expression);
        }

        for (id, expression) in &mut body.expressions {
            *expression = self.visible.remove(*id);
        }

        common_subtree_elimination::eliminate_common_subtrees(body, self.pureness);
        tree_shaking::tree_shake(body, self.pureness);
        reference_following::remove_redundant_return_references(body);
    }

    fn optimize_expression(&mut self, expression: &mut CurrentExpression) {
        'outer: loop {
            if let Expression::Function {
                parameters, body, ..
            } = &mut **expression
            {
                for parameter in &*parameters {
                    self.visible.insert(*parameter, Expression::Parameter);
                }
                self.pureness.enter_function(parameters);

                self.optimize_body(body);

                for parameter in &*parameters {
                    self.visible.remove(*parameter);
                }
            }

            loop {
                let hashcode_before = expression.do_hash();

                reference_following::follow_references(self, expression);
                constant_folding::fold_constants(self, expression);

                let is_call = matches!(**expression, Expression::Call { .. });
                inlining::inline_tiny_functions(self, expression);
                inlining::inline_functions_containing_use(self, expression);
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

        // TODO: If this is a call to the `needs` function with `True` as the
        // first argument, optimize it away. This is not correct – calling
        // `needs True 3 4` should panic instead. But we figured this is
        // temporarily fine until we have data flow.
        if let Expression::Call { function, arguments, .. } = &**expression
            && let Expression::Function { original_hirs, .. } = self.visible.get(*function)
            && original_hirs.contains(&hir::Id::needs())
            && arguments.len() == 4
            && let Expression::Tag { symbol, value: None  } = self.visible.get(arguments[0])
            && symbol == "True" {
            **expression = Expression::nothing();
        }
    }
}

#[allow(clippy::unnecessary_wraps)]
fn recover_from_cycle(
    _db: &dyn OptimizeMir,
    cycle: &[String],
    module: &Module,
    _tracing: &TracingConfig,
) -> OptimizedMirResult {
    let error = CompilerError::for_whole_module(
        module.clone(),
        MirError::ModuleHasCycle {
            cycle: cycle.to_vec(),
        },
    );

    let mir = Mir::build(|body| {
        let reason = body.push_text(error.payload.to_string());
        let responsible = body.push_hir_id(hir::Id::new(module.clone(), vec![]));
        body.push_panic(reason, responsible);
    });

    Ok((
        Arc::new(mir),
        Arc::default(),
        Arc::new(FxHashSet::from_iter([error])),
    ))
}
