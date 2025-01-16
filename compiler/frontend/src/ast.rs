use super::{cst, cst_to_ast::CstToAst, error::CompilerError};
use crate::{
    module::Module,
    rich_ir::{RichIrBuilder, ToRichIr, TokenType},
};
use derive_more::{Deref, From};
use enumset::EnumSet;
use num_bigint::{BigInt, BigUint};
use rustc_hash::FxHashMap;
use std::{
    fmt::{self, Display, Formatter},
    num::NonZeroUsize,
};
use strum_macros::EnumIs;

#[salsa::query_group(AstDbStorage)]
pub trait AstDb: CstToAst {
    fn find_ast(&self, id: Id) -> Option<Ast>;
}
#[allow(clippy::needless_pass_by_value)]
fn find_ast(db: &dyn AstDb, id: Id) -> Option<Ast> {
    let (ast, _) = db.ast(id.module.clone()).ok()?;
    ast.find(&id).cloned()
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id {
    pub module: Module,
    pub local: usize,
}
impl Id {
    #[must_use]
    pub const fn new(module: Module, local: usize) -> Self {
        Self { module, local }
    }
    #[must_use]
    pub fn to_short_debug_string(&self) -> String {
        format!("${}", self.local)
    }
}
impl Display for Id {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "AstId({}:{})", self.module, self.local)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Hash, Deref)]
pub struct Ast {
    pub id: Id,
    #[deref]
    pub kind: AstKind,
}

#[derive(Clone, Debug, EnumIs, Eq, From, Hash, PartialEq)]
pub enum AstKind {
    Int(Int),
    Text(Text),
    TextPart(TextPart),
    Identifier(Identifier),
    Symbol(Symbol),
    List(List),
    Struct(Struct),
    StructAccess(StructAccess),
    Function(Function),
    Call(Call),
    Assignment(Assignment),
    Match(Match),
    MatchCase(MatchCase),
    OrPattern(OrPattern),
    Error { errors: Vec<CompilerError> },
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Int(pub BigUint);

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Text(pub Vec<Ast>);

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct TextPart(pub AstString);

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Identifier(pub AstString);

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Symbol(pub AstString);

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct List(pub Vec<Ast>);

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Struct {
    pub fields: Vec<(Option<Ast>, Ast)>,
}
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct StructAccess {
    pub struct_: Box<Ast>,
    pub key: AstString,
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Function {
    pub parameters: Vec<Ast>,
    pub body: Vec<Ast>,
    pub fuzzable: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Call {
    pub receiver: Box<Ast>,
    pub arguments: Vec<Ast>,
    pub is_from_pipe: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Assignment {
    pub is_public: bool,
    pub body: AssignmentBody,
}
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum AssignmentBody {
    Function { name: AstString, function: Function },
    Body { pattern: Box<Ast>, body: Vec<Ast> },
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Match {
    pub expression: Box<Ast>,
    pub cases: Vec<Ast>,
}
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct MatchCase {
    pub pattern: Box<Ast>,
    pub condition: Option<Box<Ast>>,
    pub body: Vec<Ast>,
}
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct OrPattern(pub Vec<Ast>);

#[derive(Debug, PartialEq, Eq, Clone, Hash, Deref)]
pub struct AstString {
    pub id: Id,
    #[deref]
    pub value: String,
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum AstError {
    ExpectedNameOrPatternInAssignment,
    ExpectedParameter,
    FunctionMissesClosingCurlyBrace,
    ListItemMissesComma,
    ListMissesClosingParenthesis,
    ListWithNonListItem,
    OrPatternIsMissingIdentifiers {
        identifier: String,
        number_of_missing_captures: NonZeroUsize,
        all_captures: Vec<cst::Id>,
    },
    ParenthesizedInPattern,
    ParenthesizedMissesClosingParenthesis,
    PatternContainsInvalidExpression,
    PatternLiteralPartContainsInvalidExpression,
    PipeInPattern,
    StructKeyMissesColon,
    StructMissesClosingBrace,
    StructShorthandWithNotIdentifier,
    StructValueMissesComma,
    StructWithNonStructField,
    TextInterpolationMissesClosingCurlyBraces,
    TextMissesClosingQuote,
    UnexpectedPunctuation,
}

pub trait FindAst {
    fn find(&self, id: &Id) -> Option<&Ast>;
}
impl FindAst for Ast {
    fn find(&self, id: &Id) -> Option<&Ast> {
        if id == &self.id {
            return Some(self);
        };

        match &self.kind {
            AstKind::Int(_) => None,
            AstKind::Text(_) => None,
            AstKind::TextPart(_) => None,
            AstKind::Identifier(_) => None,
            AstKind::Symbol(_) => None,
            AstKind::List(list) => list.find(id),
            AstKind::Struct(struct_) => struct_.find(id),
            AstKind::StructAccess(access) => access.find(id),
            AstKind::Function(function) => function.find(id),
            AstKind::Call(call) => call.find(id),
            AstKind::Assignment(assignment) => assignment.find(id),
            AstKind::Match(match_) => match_.find(id),
            AstKind::MatchCase(match_case) => match_case.find(id),
            AstKind::OrPattern(or_pattern) => or_pattern.find(id),
            AstKind::Error { .. } => None,
        }
    }
}
impl FindAst for List {
    fn find(&self, id: &Id) -> Option<&Ast> {
        self.0.find(id)
    }
}
impl FindAst for Struct {
    fn find(&self, id: &Id) -> Option<&Ast> {
        self.fields.iter().find_map(|(key, value)| {
            key.as_ref()
                .and_then(|key| key.find(id))
                .or_else(|| value.find(id))
        })
    }
}
impl FindAst for StructAccess {
    fn find(&self, id: &Id) -> Option<&Ast> {
        self.struct_.find(id)
    }
}
impl FindAst for Function {
    fn find(&self, id: &Id) -> Option<&Ast> {
        self.body.find(id)
    }
}
impl FindAst for Call {
    fn find(&self, id: &Id) -> Option<&Ast> {
        self.receiver.find(id).or_else(|| self.arguments.find(id))
    }
}
impl FindAst for Assignment {
    fn find(&self, id: &Id) -> Option<&Ast> {
        self.body.find(id)
    }
}
impl FindAst for AssignmentBody {
    fn find(&self, id: &Id) -> Option<&Ast> {
        match self {
            Self::Function { name: _, function } => function.find(id),
            Self::Body { pattern, body } => pattern.find(id).or_else(|| body.find(id)),
        }
    }
}
impl FindAst for Match {
    fn find(&self, id: &Id) -> Option<&Ast> {
        self.expression.find(id).or_else(|| self.cases.find(id))
    }
}
impl FindAst for MatchCase {
    fn find(&self, id: &Id) -> Option<&Ast> {
        self.pattern
            .find(id)
            .or_else(|| {
                self.condition
                    .as_ref()
                    .and_then(|box condition| condition.find(id))
            })
            .or_else(|| self.body.find(id))
    }
}
impl FindAst for OrPattern {
    fn find(&self, id: &Id) -> Option<&Ast> {
        self.0.find(id)
    }
}
impl FindAst for Vec<Ast> {
    fn find(&self, id: &Id) -> Option<&Ast> {
        self.iter().find_map(|ast| ast.find(id))
    }
}

impl AstKind {
    #[must_use]
    pub fn captured_identifiers(&self) -> FxHashMap<String, Vec<Id>> {
        let mut captured_identifiers = FxHashMap::default();
        self.captured_identifiers_helper(&mut captured_identifiers);
        captured_identifiers
    }
    fn captured_identifiers_helper(&self, captured_identifiers: &mut FxHashMap<String, Vec<Id>>) {
        match self {
            Self::Int(_) | Self::Text(_) | Self::TextPart(_) => {}
            Self::Identifier(Identifier(identifier)) => {
                let entry = captured_identifiers
                    .entry(identifier.value.clone())
                    .or_default();
                entry.push(identifier.id.clone());
            }
            Self::Symbol(_) => {}
            Self::List(List(list)) => {
                for item in list {
                    item.captured_identifiers_helper(captured_identifiers);
                }
            }
            Self::Struct(Struct { fields }) => {
                for (key, value) in fields {
                    if let Some(key) = key {
                        key.captured_identifiers_helper(captured_identifiers);
                    }
                    value.captured_identifiers_helper(captured_identifiers);
                }
            }
            Self::StructAccess(_)
            | Self::Function(_)
            | Self::Call(_)
            | Self::Assignment(_)
            | Self::Match(_)
            | Self::MatchCase(_) => {}
            Self::OrPattern(OrPattern(patterns)) => {
                for pattern in patterns {
                    pattern.captured_identifiers_helper(captured_identifiers);
                }
            }
            Self::Error { .. } => {}
        }
    }
}

pub trait CollectErrors {
    fn collect_errors(self, errors: &mut Vec<CompilerError>);
}
impl CollectErrors for Ast {
    fn collect_errors(self, errors: &mut Vec<CompilerError>) {
        match self.kind {
            AstKind::Int(_) => {}
            AstKind::Text(Text(parts)) => parts.collect_errors(errors),
            AstKind::TextPart(_) => {}
            AstKind::Identifier(_) => {}
            AstKind::Symbol(_) => {}
            AstKind::List(List(items)) => {
                for item in items {
                    item.collect_errors(errors);
                }
            }
            AstKind::Struct(struct_) => {
                for (key, value) in struct_.fields {
                    if let Some(key) = key {
                        key.collect_errors(errors);
                    }
                    value.collect_errors(errors);
                }
            }
            AstKind::StructAccess(StructAccess { struct_, key: _ }) => {
                struct_.collect_errors(errors);
            }
            AstKind::Function(function) => function.body.collect_errors(errors),
            AstKind::Call(call) => call.arguments.collect_errors(errors),
            AstKind::Assignment(assignment) => match assignment.body {
                AssignmentBody::Function { name: _, function } => {
                    function.body.collect_errors(errors);
                }
                AssignmentBody::Body { pattern, body } => {
                    pattern.collect_errors(errors);
                    for ast in body {
                        ast.collect_errors(errors);
                    }
                }
            },
            AstKind::Match(Match { expression, cases }) => {
                expression.collect_errors(errors);
                cases.collect_errors(errors);
            }
            AstKind::MatchCase(MatchCase {
                pattern,
                condition,
                body,
            }) => {
                pattern.collect_errors(errors);
                if let Some(box condition) = condition {
                    condition.collect_errors(errors);
                }
                body.collect_errors(errors);
            }
            AstKind::OrPattern(OrPattern(patterns)) => {
                for pattern in patterns {
                    pattern.collect_errors(errors);
                }
            }
            AstKind::Error {
                errors: mut ast_errors,
            } => {
                errors.append(&mut ast_errors);
            }
        }
    }
}
impl CollectErrors for Vec<Ast> {
    fn collect_errors(self, errors: &mut Vec<CompilerError>) {
        for ast in self {
            ast.collect_errors(errors);
        }
    }
}

impl ToRichIr for Ast {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        match &self.kind {
            AstKind::Int(int) => int.build_rich_ir(builder),
            AstKind::Text(text) => text.build_rich_ir(builder),
            AstKind::TextPart(part) => part.build_rich_ir(builder),
            AstKind::Identifier(identifier) => identifier.build_rich_ir(builder),
            AstKind::Symbol(symbol) => symbol.build_rich_ir(builder),
            AstKind::List(list) => list.build_rich_ir(builder),
            AstKind::Struct(struct_) => struct_.build_rich_ir(builder),
            AstKind::StructAccess(struct_access) => struct_access.build_rich_ir(builder),
            AstKind::Function(function) => function.build_rich_ir(builder),
            AstKind::Call(call) => call.build_rich_ir(builder),
            AstKind::Assignment(assignment) => assignment.build_rich_ir(builder),
            AstKind::Match(match_) => match_.build_rich_ir(builder),
            AstKind::MatchCase(match_case) => match_case.build_rich_ir(builder),
            AstKind::OrPattern(or_pattern) => or_pattern.build_rich_ir(builder),
            AstKind::Error { errors } => {
                builder.push("error:", None, EnumSet::empty());
                builder.push_children_multiline(errors);
            }
        }
    }
}
impl ToRichIr for Int {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        let range = builder.push(format!("int {}", self.0), TokenType::Int, EnumSet::empty());
        builder.push_reference(BigInt::from(self.0.clone()), range);
    }
}
impl ToRichIr for Text {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push("text", None, EnumSet::empty());
        builder.push_foldable(|builder| builder.push_children_multiline(&self.0));
    }
}
impl ToRichIr for TextPart {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push("textPart ", None, EnumSet::empty());
        self.0.build_rich_ir(builder);
    }
}
impl ToRichIr for Identifier {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push("identifier ", None, EnumSet::empty());
        self.0.build_rich_ir(builder);
    }
}
impl ToRichIr for Symbol {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push("symbol ", None, EnumSet::empty());
        self.0.build_rich_ir(builder);
    }
}
impl ToRichIr for List {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push("list", None, EnumSet::empty());
        builder.push_foldable(|builder| builder.push_children_multiline(&self.0));
    }
}
impl ToRichIr for Struct {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push("struct", None, EnumSet::empty());
        builder.push_foldable(|builder| {
            builder.push_children_custom_multiline(&self.fields, |builder, (key, value)| {
                if let Some(key) = key {
                    key.build_rich_ir(builder);
                    builder.push(": ", None, EnumSet::empty());
                }

                value.build_rich_ir(builder);
            });
        });
    }
}
impl ToRichIr for StructAccess {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push("struct access ", None, EnumSet::empty());
        self.struct_.build_rich_ir(builder);
        builder.push(".", None, EnumSet::empty());
        self.key.build_rich_ir(builder); // TODO: `lowercase_first_letter()`?
    }
}
impl ToRichIr for Function {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push(
            format!(
                "function ({}) {{",
                if self.fuzzable {
                    "fuzzable"
                } else {
                    "non-fuzzable"
                },
            ),
            None,
            EnumSet::empty(),
        );

        if !self.parameters.is_empty() {
            if self
                .parameters
                .iter()
                .all(|it| matches!(it.kind, AstKind::Identifier(_)))
            {
                for parameter in &self.parameters {
                    builder.push(" ", None, EnumSet::empty());
                    parameter.build_rich_ir(builder);
                }
                builder.push(" ->", None, EnumSet::empty());
            } else {
                builder.push_children_multiline(&self.parameters);
                builder.push_indented(|builder| {
                    builder.push_newline();
                });
                builder.push("->", None, EnumSet::empty());
            }
        }

        builder.push_foldable(|builder| {
            builder.push_children_multiline(&self.body);
            builder.push_newline();
        });
        builder.push("}", None, EnumSet::empty());
    }
}
impl ToRichIr for Call {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push("call ", None, EnumSet::empty());
        self.receiver.build_rich_ir(builder);
        builder.push(" with these arguments:", None, EnumSet::empty());
        builder.push_foldable(|builder| builder.push_children_multiline(&self.arguments));
    }
}
impl ToRichIr for Assignment {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push("assignment: ", None, EnumSet::empty());
        match &self.body {
            AssignmentBody::Function { name, .. } => name.build_rich_ir(builder),
            AssignmentBody::Body { pattern, .. } => pattern.build_rich_ir(builder),
        }
        builder.push(
            if self.is_public { " := " } else { " = " },
            None,
            EnumSet::empty(),
        );
        match &self.body {
            AssignmentBody::Function { function, .. } => function.build_rich_ir(builder),
            AssignmentBody::Body { body, .. } => {
                builder.push_foldable(|builder| builder.push_children_multiline(body));
            }
        }
    }
}
impl ToRichIr for Match {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push("match ", None, EnumSet::empty());
        self.expression.build_rich_ir(builder);
        builder.push(" %", None, EnumSet::empty());
        builder.push_foldable(|builder| builder.push_children_multiline(&self.cases));
    }
}
impl ToRichIr for MatchCase {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push("case ", None, EnumSet::empty());
        self.pattern.build_rich_ir(builder);
        if let Some(box condition) = &self.condition {
            builder.push(", ", None, EnumSet::empty());
            builder.indent();
            builder.push_newline();
            builder.push("condition ", None, EnumSet::empty());
            condition.build_rich_ir(builder);
        }
        builder.push(" -> ", None, EnumSet::empty());
        builder.indent();
        builder.push_foldable(|builder| builder.push_children_multiline(&self.body));
        if self.condition.is_some() {
            builder.dedent();
        }
        builder.dedent();
    }
}
impl ToRichIr for OrPattern {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        self.0.first().unwrap().build_rich_ir(builder);
        for pattern in self.0.iter().skip(1) {
            builder.push(" | ", None, EnumSet::empty());
            pattern.build_rich_ir(builder);
        }
    }
}
impl ToRichIr for AstString {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push(
            format!(r#"{}@"{}""#, self.id.to_short_debug_string(), self.value),
            TokenType::Text,
            EnumSet::empty(),
        );
    }
}
