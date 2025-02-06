use super::{inlining::InliningState, pure::PurenessInsights, OptimizeMir};
use crate::{
    error::CompilerError,
    id::IdGenerator,
    mir::{Body, Expression, Id, VisibleExpressions},
    mir_optimize::log::OptimizationLogger,
    TracingConfig,
};
use rustc_hash::FxHashSet;
use std::ops::Deref;

pub struct Context<'a> {
    pub db: &'a dyn OptimizeMir,
    pub tracing: &'a TracingConfig,
    pub errors: &'a mut FxHashSet<CompilerError>,
    pub visible: &'a mut VisibleExpressions,
    pub id_generator: &'a mut IdGenerator<Id>,
    pub pureness: &'a mut PurenessInsights,
    pub inlining_state: InliningState,
}

pub struct CurrentExpression<'a> {
    body: &'a mut Body,
    index: usize,
}
impl<'a> CurrentExpression<'a> {
    pub fn new(body: &'a mut Body, index: usize) -> Self {
        Self { body, index }
    }

    pub const fn index(&self) -> usize {
        self.index
    }
    pub fn id(&self) -> Id {
        self.body.expressions[self.index].0
    }

    /// When you modify the expression, you need to also update the pureness
    /// insights.
    pub fn get_mut_carefully(&mut self) -> &mut Expression {
        &mut self.body.expressions[self.index].1
    }
    pub fn replace_id_references(
        &mut self,
        optimization_name: &str,
        replacer: &mut impl FnMut(&mut Id),
    ) {
        self.get_mut_carefully().replace_id_references(replacer);
        OptimizationLogger::log_replace_id_references(optimization_name, self.id());
    }
    pub fn prepend_optimized(
        &mut self,
        optimization_name: &str,
        visible: &mut VisibleExpressions,
        optimized_expressions: impl IntoIterator<Item = (Id, Expression)>,
    ) {
        let mut new_formatted = OptimizationLogger::is_enabled().then(String::new);
        self.body.expressions.splice(
            self.index..self.index,
            optimized_expressions.into_iter().map(|(id, expression)| {
                if let Some(new_formatted) = &mut new_formatted {
                    *new_formatted += &format!("{id} = {expression}\n");
                }
                visible.insert(id, expression);
                self.index += 1;
                (id, Expression::Parameter)
            }),
        );

        if let Some(new_formatted) = new_formatted {
            OptimizationLogger::log_prepend_optimized(
                optimization_name,
                self.id(),
                new_formatted.strip_suffix('\n').unwrap_or_default(),
            );
        }
    }
    pub fn replace_with(
        &mut self,
        optimization_name: &str,
        expression: Expression,
        pureness: &mut PurenessInsights,
    ) -> Expression {
        let id = self.body.expressions[self.index].0;
        self.replace_with_multiple(optimization_name, [(id, expression)], pureness)
    }
    pub fn replace_with_multiple<I: DoubleEndedIterator<Item = (Id, Expression)>>(
        &mut self,
        optimization_name: &str,
        expressions: impl IntoIterator<Item = (Id, Expression), IntoIter = I>,
        pureness: &mut PurenessInsights,
    ) -> Expression {
        let mut new_formatted = OptimizationLogger::is_enabled().then(String::new);
        let mut expressions = expressions.into_iter();
        let id = self.id();
        let (_, last_expression) = expressions.next_back().unwrap();
        let mut removed = self.body.expressions.splice(
            self.index..=self.index,
            expressions
                .chain([(id, last_expression)])
                .map(|(id, expression)| {
                    if let Some(new_formatted) = &mut new_formatted {
                        *new_formatted += &format!("{id} = {expression}\n");
                    }
                    (id, expression)
                }),
        );

        let (_, removed_expression) = removed.next().unwrap();
        assert!(removed.next().is_none());
        drop(removed);
        for id in removed_expression.defined_ids() {
            pureness.on_remove(id);
        }

        if let Some(new_formatted) = new_formatted {
            OptimizationLogger::log_replace_with(
                optimization_name,
                &format!("{id} = {removed_expression}"),
                // There's at least one new expression, so there's always a
                // trailing newline we have to remove.
                &new_formatted[..new_formatted.len() - 1],
            );
        }

        removed_expression
    }
}

impl Deref for CurrentExpression<'_> {
    type Target = Expression;

    fn deref(&self) -> &Self::Target {
        &self.body.expressions[self.index].1
    }
}
