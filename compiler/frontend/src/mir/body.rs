use super::{expression::Expression, id::Id};
use crate::{
    builtin_functions::BuiltinFunction,
    hir,
    id::{CountableId, IdGenerator},
    impl_display_via_richir,
    rich_ir::{RichIrBuilder, ToRichIr, TokenType},
};
use derive_more::{Deref, DerefMut};
use enumset::EnumSet;
use itertools::Itertools;
use num_bigint::BigInt;
use rustc_hash::FxHashMap;
use std::{
    cmp::Ordering,
    fmt::{self, Debug, Formatter},
    mem, vec,
};

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct Body {
    pub expressions: Vec<(Id, Expression)>,
}
impl Body {
    #[must_use]
    pub fn new(expressions: Vec<(Id, Expression)>) -> Self {
        Self { expressions }
    }
    #[must_use]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (Id, &Expression)> {
        self.expressions
            .iter()
            .map(|(id, expression)| (*id, expression))
    }
    #[must_use]
    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = (Id, &mut Expression)> {
        self.expressions
            .iter_mut()
            .map(|(id, expression)| (*id, expression))
    }
}
impl IntoIterator for Body {
    type Item = (Id, Expression);
    type IntoIter = vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.expressions.into_iter()
    }
}
impl Body {
    #[must_use]
    pub fn return_value(&self) -> Id {
        let (id, _) = self.expressions.last().unwrap();
        *id
    }

    pub fn push(&mut self, id: Id, expression: impl Into<Expression>) {
        self.expressions.push((id, expression.into()));
    }
    pub fn push_with_new_id(
        &mut self,
        id_generator: &mut IdGenerator<Id>,
        expression: impl Into<Expression>,
    ) -> Id {
        let id = id_generator.generate();
        self.push(id, expression.into());
        id
    }
    pub fn insert_at_front(&mut self, mut expressions: Vec<(Id, Expression)>) {
        let mut old_expressions = mem::take(&mut self.expressions);
        self.expressions.append(&mut expressions);
        self.expressions.append(&mut old_expressions);
    }
    pub fn remove_all<F>(&mut self, mut predicate: F) -> Vec<(Id, Expression)>
    where
        F: FnMut(Id, &Expression) -> bool,
    {
        self.expressions
            .extract_if(|(id, expression)| predicate(*id, expression))
            .collect()
    }
    pub fn sort_by<F>(&mut self, predicate: F)
    where
        F: FnMut(&(Id, Expression), &(Id, Expression)) -> Ordering,
    {
        self.expressions.sort_by(predicate);
    }
}

pub enum VisitorResult {
    Continue,
    Abort,
}

#[derive(Clone)]
pub struct VisibleExpressions {
    expressions: FxHashMap<Id, Expression>,
}
impl VisibleExpressions {
    #[must_use]
    pub fn none_visible() -> Self {
        Self {
            expressions: FxHashMap::default(),
        }
    }
    pub fn insert(&mut self, id: Id, expression: Expression) {
        self.expressions.insert(id, expression);
    }
    pub fn remove(&mut self, id: Id) -> Expression {
        self.expressions.remove(&id).unwrap_or_else(|| {
            panic!("Expression with ID {id} is not visible in this scope. Visible expressions: {self:?}")
        })
    }
    #[must_use]
    pub fn get(&self, id: Id) -> &Expression {
        self.expressions.get(&id).unwrap_or_else(|| {
            panic!("Expression with ID {id} is not visible in this scope. Visible expressions: {self:?}")
        })
    }
    #[must_use]
    pub fn contains(&self, id: Id) -> bool {
        self.expressions.contains_key(&id)
    }
}
impl Debug for VisibleExpressions {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.expressions
                .keys()
                .sorted()
                .map(ToString::to_string)
                .join(", "),
        )
    }
}

impl Body {
    pub fn visit(
        &self,
        visitor: &mut dyn FnMut(Id, &Expression, bool) -> VisitorResult,
    ) -> VisitorResult {
        let length = self.expressions.len();
        for i in 0..length {
            let (id, expression) = self.expressions.get(i).unwrap();
            match Self::visit_expression(*id, expression, i == length - 1, visitor) {
                VisitorResult::Continue => {}
                VisitorResult::Abort => return VisitorResult::Abort,
            }
        }
        VisitorResult::Continue
    }
    fn visit_expression(
        id: Id,
        expression: &Expression,
        is_returned: bool,
        visitor: &mut dyn FnMut(Id, &Expression, bool) -> VisitorResult,
    ) -> VisitorResult {
        if let Expression::Function { body, .. } = expression {
            match body.visit(visitor) {
                VisitorResult::Continue => {}
                VisitorResult::Abort => return VisitorResult::Abort,
            }
        }
        visitor(id, expression, is_returned)
    }

    pub fn visit_mut(
        &mut self,
        visitor: &mut dyn FnMut(Id, &mut Expression, bool) -> VisitorResult,
    ) -> VisitorResult {
        let length = self.expressions.len();
        for i in 0..length {
            let (id, expression) = self.expressions.get_mut(i).unwrap();
            match Self::visit_expression_mut(*id, expression, i == length - 1, visitor) {
                VisitorResult::Continue => {}
                VisitorResult::Abort => return VisitorResult::Abort,
            }
        }
        VisitorResult::Continue
    }
    fn visit_expression_mut(
        id: Id,
        expression: &mut Expression,
        is_returned: bool,
        visitor: &mut dyn FnMut(Id, &mut Expression, bool) -> VisitorResult,
    ) -> VisitorResult {
        if let Expression::Function { body, .. } = expression {
            match body.visit_mut(visitor) {
                VisitorResult::Continue => {}
                VisitorResult::Abort => return VisitorResult::Abort,
            }
        }
        visitor(id, expression, is_returned)
    }

    /// Calls the visitor for each contained expression, even expressions in
    /// functions or multiples.
    ///
    /// The visitor is called in inside-out order, so if the body contains a
    /// function, the visitor is first called for its body expressions and only
    /// then for the function expression itself.
    ///
    /// The visitor takes the ID of the current expression as well as the
    /// expression itself. It also takes `VisibleExpressions`, which allows it
    /// to inspect all expressions currently in scope. Finally, the visitor also
    /// receives whether the current expression is returned from the surrounding
    /// body.
    pub fn visit_with_visible(
        &mut self,
        visitor: &mut dyn FnMut(Id, &mut Expression, &VisibleExpressions, bool),
    ) {
        self.visit_with_visible_rec(&mut VisibleExpressions::none_visible(), visitor);
    }
    fn visit_with_visible_rec(
        &mut self,
        visible: &mut VisibleExpressions,
        visitor: &mut dyn FnMut(Id, &mut Expression, &VisibleExpressions, bool),
    ) {
        let expressions_in_this_body = self.expressions.iter().map(|(id, _)| *id).collect_vec();
        let length = expressions_in_this_body.len();

        for index in 0..length {
            let (id, mut expression) = mem::replace(
                self.expressions.get_mut(index).unwrap(),
                (Id::from_usize(0), Expression::Parameter),
            );
            let is_returned = index == length - 1;
            Self::visit_expression_with_visible(id, &mut expression, visible, is_returned, visitor);
            visible.insert(id, expression);
        }

        for (index, id) in expressions_in_this_body.iter().enumerate() {
            *self.expressions.get_mut(index).unwrap() = (*id, visible.remove(*id));
        }
    }
    fn visit_expression_with_visible(
        id: Id,
        expression: &mut Expression,
        visible: &mut VisibleExpressions,
        is_returned: bool,
        visitor: &mut dyn FnMut(Id, &mut Expression, &VisibleExpressions, bool),
    ) {
        if let Expression::Function {
            parameters,
            responsible_parameter,
            body,
            ..
        } = expression
        {
            for parameter in &*parameters {
                visible.insert(*parameter, Expression::Parameter);
            }
            visible.insert(*responsible_parameter, Expression::Parameter);
            body.visit_with_visible_rec(visible, visitor);
            for parameter in &*parameters {
                visible.remove(*parameter);
            }
            visible.remove(*responsible_parameter);
        }

        visitor(id, expression, visible, is_returned);
    }

    pub fn visit_bodies(&mut self, visitor: &mut dyn FnMut(&mut Self)) {
        for (_, expression) in self.iter_mut() {
            expression.visit_bodies(visitor);
        }
        visitor(self);
    }
}
impl Expression {
    pub fn visit_bodies(&mut self, visitor: &mut dyn FnMut(&mut Body)) {
        if let Self::Function { body, .. } = self {
            body.visit_bodies(visitor);
        }
    }
}

#[derive(Deref, DerefMut)]
pub struct FunctionBodyBuilder {
    hir_id: hir::Id,
    #[deref]
    #[deref_mut]
    body_builder: BodyBuilder,
    // PERF: These are numbered sequentially, so avoid the vec
    parameters: Vec<Id>,
    responsible_parameter: Id,
}
impl FunctionBodyBuilder {
    fn new(hir_id: hir::Id, mut id_generator: IdGenerator<Id>) -> Self {
        let responsible_parameter = id_generator.generate();
        Self {
            hir_id,
            body_builder: BodyBuilder::new(id_generator),
            parameters: vec![],
            responsible_parameter,
        }
    }

    pub fn new_parameter(&mut self) -> Id {
        let id = self.body_builder.id_generator.generate();
        self.parameters.push(id);
        id
    }

    fn finish(self) -> (IdGenerator<Id>, Expression) {
        let (id_generator, body) = self.body_builder.finish();
        let function = Expression::Function {
            original_hirs: vec![self.hir_id].into_iter().collect(),
            parameters: self.parameters,
            responsible_parameter: self.responsible_parameter,
            body,
        };
        (id_generator, function)
    }
}

pub struct BodyBuilder {
    id_generator: IdGenerator<Id>,
    body: Body,
}
impl BodyBuilder {
    #[must_use]
    pub fn new(id_generator: IdGenerator<Id>) -> Self {
        Self {
            id_generator,
            body: Body::default(),
        }
    }

    pub fn push(&mut self, expression: Expression) -> Id {
        self.body
            .push_with_new_id(&mut self.id_generator, expression)
    }

    pub fn push_int(&mut self, value: impl Into<BigInt>) -> Id {
        self.push(Expression::Int(value.into()))
    }
    pub fn push_text(&mut self, value: String) -> Id {
        self.push(Expression::Text(value))
    }

    pub fn push_tag(&mut self, symbol: String, value: impl Into<Option<Id>>) -> Id {
        self.push(Expression::Tag {
            symbol,
            value: value.into(),
        })
    }
    pub fn push_nothing(&mut self) -> Id {
        self.push(Expression::nothing())
    }
    pub fn push_bool(&mut self, value: bool) -> Id {
        self.push(value.into())
    }

    pub fn push_builtin(&mut self, function: BuiltinFunction) -> Id {
        self.push(Expression::Builtin(function))
    }
    pub fn push_list(&mut self, list: Vec<Id>) -> Id {
        self.push(Expression::List(list))
    }
    pub fn push_struct(&mut self, struct_: Vec<(Id, Id)>) -> Id {
        self.push(Expression::Struct(struct_))
    }
    pub fn push_reference(&mut self, reference: Id) -> Id {
        self.push(Expression::Reference(reference))
    }
    pub fn push_hir_id(&mut self, id: hir::Id) -> Id {
        self.push(Expression::HirId(id))
    }

    /// The builder function takes the builder and the responsible parameter.
    pub fn push_function<F>(&mut self, hir_id: hir::Id, function: F) -> Id
    where
        F: FnOnce(&mut FunctionBodyBuilder, Id),
    {
        let mut builder = FunctionBodyBuilder::new(hir_id, mem::take(&mut self.id_generator));
        let responsible_parameter = builder.responsible_parameter;
        function(&mut builder, responsible_parameter);
        let (id_generator, function) = builder.finish();
        self.id_generator = id_generator;
        self.push(function)
    }

    pub fn push_call(&mut self, function: Id, arguments: Vec<Id>, responsible: Id) -> Id {
        self.push(Expression::Call {
            function,
            arguments,
            responsible,
        })
    }
    pub fn push_if_else<T, E>(
        &mut self,
        hir_id: &hir::Id,
        condition: Id,
        then_builder: T,
        else_builder: E,
        responsible: Id,
    ) -> Id
    where
        T: FnOnce(&mut Self),
        E: FnOnce(&mut Self),
    {
        let builtin_if_else = self.push_builtin(BuiltinFunction::IfElse);
        let then_function = self.push_function(hir_id.child("then"), |body, _| then_builder(body));
        let else_function = self.push_function(hir_id.child("else"), |body, _| else_builder(body));
        self.push_call(
            builtin_if_else,
            vec![condition, then_function, else_function],
            responsible,
        )
    }
    pub fn push_if_not(
        &mut self,
        hir_id: &hir::Id,
        condition: Id,
        else_builder: impl FnOnce(&mut Self),
        responsible: Id,
    ) -> Id {
        self.push_if_else(
            hir_id,
            condition,
            |body| {
                body.push_nothing();
            },
            else_builder,
            responsible,
        )
    }
    pub fn push_is_bool(&mut self, hir_id: &hir::Id, value: Id, responsible: Id) -> Id {
        let is_value_true = self.push_equals_value(value, true, responsible);
        self.push_if_else(
            &hir_id.child("isValueTrue"),
            is_value_true,
            |body| {
                body.push_reference(is_value_true);
            },
            |body| {
                body.push_equals_value(value, false, responsible);
            },
            responsible,
        )
    }
    pub fn push_equals(&mut self, a: Id, b: Id, responsible: Id) -> Id {
        let builtin_equals = self.push_builtin(BuiltinFunction::Equals);
        self.push_call(builtin_equals, vec![a, b], responsible)
    }
    pub fn push_equals_value(&mut self, a: Id, b: impl Into<Expression>, responsible: Id) -> Id {
        let b = self.push(b.into());
        self.push_equals(a, b, responsible)
    }

    pub fn push_to_debug_text(&mut self, value: Id, responsible: Id) -> Id {
        let builtin_to_debug_text = self.push_builtin(BuiltinFunction::ToDebugText);
        self.push_call(builtin_to_debug_text, vec![value], responsible)
    }
    pub fn push_panic(&mut self, reason: Id, responsible: Id) -> Id {
        self.push(Expression::Panic {
            reason,
            responsible,
        })
    }
    pub fn push_panic_if_false(
        &mut self,
        hir_id: &hir::Id,
        condition: Id,
        reason: Id,
        responsible: Id,
    ) -> Id {
        self.push_if_else(
            hir_id,
            condition,
            |body| {
                body.push_nothing();
            },
            |body| {
                body.push_panic(reason, responsible);
            },
            responsible,
        )
    }

    pub fn push_text_concatenate(&mut self, parts: &[Id], responsible: Id) -> Id {
        assert!(!parts.is_empty());

        let builtin_text_concatenate = self.push_builtin(BuiltinFunction::TextConcatenate);
        parts
            .iter()
            .copied()
            .reduce(|left, right| {
                self.push_call(builtin_text_concatenate, vec![left, right], responsible)
            })
            .unwrap()
    }

    #[must_use]
    pub fn current_return_value(&self) -> Id {
        self.body.return_value()
    }

    #[must_use]
    pub fn finish(self) -> (IdGenerator<Id>, Body) {
        (self.id_generator, self.body)
    }
}

impl_display_via_richir!(Body);
impl ToRichIr for Body {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push_custom_multiline(&self.expressions, |builder, (id, expression)| {
            if let Expression::Function { original_hirs, .. } = expression {
                builder.push("# ", TokenType::Comment, EnumSet::empty());
                builder.push_children_custom(
                    original_hirs.iter().sorted().collect_vec(),
                    |builder, id| {
                        let range =
                            builder.push(id.to_string(), TokenType::Comment, EnumSet::empty());
                        builder.push_reference((*id).clone(), range);
                    },
                    ", ",
                );
                builder.push_newline();
            }

            let range = builder.push(id.to_string(), TokenType::Comment, EnumSet::empty());
            builder.push_definition(*id, range);
            builder.push(" = ", None, EnumSet::empty());
            expression.build_rich_ir(builder);
        });
    }
}
