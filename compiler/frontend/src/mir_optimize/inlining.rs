//! Inlining means inserting a function's code at the caller site.
//!
//! Here's a before-and-after example of a `use "Core"` call being inlined:
//!
//! ```mir
//! # before:
//! $0 = { $1 ($2 responsible) ->
//!   $3 = use $1 relative to here, $2 responsible
//! }
//! $4 = "Core"
//! $5 = HirId(the `use "Core"` expression)
//! $6 = call $0 with $4 ($5 is responsible)
//!
//! # after:
//! $0 = { $1 ($2 responsible) ->
//!   $3 = use $1 relative to here, $2 responsible
//! }
//! $4 = "Core"
//! $5 = HirId(the `use "Core"` expression)
//! $6 =
//!   $7 = use $4 relative to here, $5 responsible
//! ```
//!
//! Inlining makes lots of other optimizations more effective, in partiuclar
//! [tree shaking] of functions that were inlined into all call sites. Because
//! at the call sites, more information about arguments exist,
//! [constant folding] and [module folding] can be more effective.
//!
//! TODO: When we have a metric for judging performance vs. code size, also
//! speculatively inline more call sites, such as smallish functions and
//! functions only used once.
//!
//! [constant folding]: super::constant_folding
//! [module folding]: super::module_folding
//! [tree shaking]: super::tree_shaking

use super::{
    complexity::Complexity,
    current_expression::{Context, CurrentExpression},
    pure::PurenessInsights,
};
use crate::{
    hir,
    mir::{Expression, Id},
};
use rustc_hash::FxHashMap;
use std::{collections::hash_map::Entry, num::NonZeroUsize};

const NAME: &str = "Inlining";

pub fn inline_tiny_functions(context: &mut Context, expression: &mut CurrentExpression) {
    inline_functions_of_maximum_complexity(
        context,
        expression,
        Complexity {
            is_self_contained: true,
            expressions: 100,
        },
    );
}
fn inline_functions_of_maximum_complexity(
    context: &mut Context,
    expression: &mut CurrentExpression,
    complexity: Complexity,
) {
    if let Expression::Call { function, .. } = **expression
        && let Expression::Function { body, .. } = context.visible.get(function)
        && body.complexity() <= complexity
    {
        context.inline_call(expression);
    }
}

pub fn inline_needs_function(context: &mut Context, expression: &mut CurrentExpression) {
    if let Expression::Call {
        function,
        arguments,
        ..
    } = &**expression
        && arguments.iter().all(|it| {
            context
                .pureness
                .is_definition_const(context.visible.get(*it))
        })
        && let Expression::Function { original_hirs, .. } = context.visible.get(*function)
        && original_hirs.contains(&hir::Id::needs())
    {
        context.inline_call(expression);
    }
}

pub fn inline_functions_containing_use(context: &mut Context, expression: &mut CurrentExpression) {
    if let Expression::Call { function, .. } = **expression
        && let Expression::Function { body, .. } = context.visible.get(function)
        && body
            .iter()
            .any(|(_, expression)| expression.is_use_module())
    {
        context.inline_call(expression);
    }
}
pub fn inline_calls_with_constant_arguments(
    context: &mut Context,
    expression: &mut CurrentExpression,
) {
    if let Expression::Call { arguments, .. } = &**expression
        && arguments.iter().all(|arg| {
            context
                .pureness
                .is_definition_const(context.visible.get(*arg))
        })
    {
        context.inline_call(expression);
    }
}

#[derive(Clone, Debug, Default)]
pub struct InliningState {
    recursive_inlining_counts: FxHashMap<Id, NonZeroUsize>,
}
impl InliningState {
    /// To avoid infinite recursion, we limit the number of times a function can
    /// be inlined into itself in a single module.
    const MAX_RECURSION_INLINING_COUNT_IN_MODULE: usize = 1;
}

impl Context<'_> {
    fn inline_call(&mut self, expression: &mut CurrentExpression) {
        let Expression::Call {
            function,
            arguments,
            responsible: responsible_argument,
        } = &**expression
        else {
            // Expression is not a call.
            return;
        };
        if arguments.contains(function) {
            // Callee is used as an argument → recursion
            match self
                .inlining_state
                .recursive_inlining_counts
                .entry(*function)
            {
                Entry::Occupied(mut entry) => {
                    let count = entry.get_mut();
                    if count.get() >= InliningState::MAX_RECURSION_INLINING_COUNT_IN_MODULE {
                        return;
                    }
                    *count = count.saturating_add(1);
                }
                Entry::Vacant(entry) => {
                    entry.insert(NonZeroUsize::new(1).unwrap());
                }
            }
        }

        let Expression::Function {
            original_hirs: _,
            parameters,
            responsible_parameter,
            body,
        } = self.visible.get(*function)
        else {
            // Callee is not a function.
            return;
        };
        if arguments.len() != parameters.len() {
            // Number of arguments doesn't match the expected parameter count.
            return;
        }

        let id_mapping: FxHashMap<Id, Id> = parameters
            .iter()
            .zip(arguments.iter())
            .map(|(parameter, argument)| (*parameter, *argument))
            .chain([(*responsible_parameter, *responsible_argument)])
            .chain(
                body.defined_ids()
                    .into_iter()
                    .map(|id| (id, self.id_generator.generate())),
            )
            .collect();

        expression.replace_with_multiple(
            NAME,
            body.iter().map(|(id, expression)| {
                let mut expression = expression.clone();
                expression.replace_ids(&mut |id| {
                    if let Some(replacement) = id_mapping.get(id) {
                        *id = *replacement;
                    }
                });
                (id_mapping[&id], expression)
            }),
            // The replaced expression is definitely a call, which means it
            // doesn't define any expressions that need to be removed from the
            // pureness insights.
            &mut PurenessInsights::default(),
        );
    }
}
