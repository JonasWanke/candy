use enumset::EnumSet;

use super::{ast::AstError, cst, cst::CstError, hir::HirError};
use crate::{
    mir::MirError,
    module::Module,
    position::{Offset, PositionConversionDb, RangeOfPosition},
    rich_ir::{ReferenceKey, RichIrBuilder, ToRichIr},
    string_to_rcst::ModuleError,
};
use derive_more::From;
use itertools::Itertools;
use std::{fmt::Display, hash::Hash, ops::Range};

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct CompilerError {
    pub module: Module,
    pub span: Range<Offset>,
    pub payload: CompilerErrorPayload,
}

#[derive(Clone, Debug, Eq, From, Hash, PartialEq)]
pub enum CompilerErrorPayload {
    Module(ModuleError),
    Cst(CstError),
    Ast(AstError),
    Hir(HirError),
    Mir(MirError),
}
impl CompilerError {
    pub fn for_whole_module(module: Module, payload: impl Into<CompilerErrorPayload>) -> Self {
        Self {
            module,
            span: Offset(0)..Offset(0),
            payload: payload.into(),
        }
    }
    pub fn to_string_with_location(&self, db: &impl PositionConversionDb) -> String {
        let range = db.range_to_positions(self.module.clone(), self.span.clone());
        format!("{}:{}: {}", self.module, range.format(), self.payload)
    }
}
impl Display for CompilerErrorPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Module(error) => match error {
                ModuleError::DoesNotExist => "The module doesn't exist.".to_string(),
                ModuleError::InvalidUtf8 => "The module contains invalid UTF-8.".to_string(),
                ModuleError::IsNotCandy => "The module is not Candy.".to_string(),
                ModuleError::IsToolingModule => "The module is a tooling module.".to_string(),
            },
            Self::Cst(error) => match error {
                CstError::BinaryBarMissesRight => "There should be a right side after this bar.",
                CstError::CurlyBraceNotClosed => "The curly brace is not closed.",
                CstError::IdentifierContainsNonAlphanumericAscii => {
                    "This identifier contains non-alphanumeric ASCII characters."
                }
                CstError::IntContainsNonDigits => {
                    "This integer contains characters that are not digits."
                }
                CstError::ListItemMissesValue => "This list item is missing a value.",
                CstError::ListNotClosed => "The list is not closed.",
                CstError::MatchMissesCases => "This match misses cases to match against.",
                CstError::MatchCaseMissesArrow => "This match case misses an arrow.",
                CstError::MatchCaseMissesBody => "This match case misses a body to run.",
                CstError::MatchCaseMissesCondition => "This match case condition is empty.",
                CstError::OpeningParenthesisMissesExpression => {
                    "Here's an opening parenthesis without an expression after it."
                }
                CstError::OrPatternMissesRight => "This or-pattern misses a right-hand side.",
                CstError::ParenthesisNotClosed => "This parenthesis isn't closed.",
                CstError::StructFieldMissesColon => "This struct field misses a colon.",
                CstError::StructFieldMissesKey => "This struct field misses a key.",
                CstError::StructFieldMissesValue => "This struct field misses a value.",
                CstError::StructNotClosed => "This struct is not closed.",
                CstError::SymbolContainsNonAlphanumericAscii => {
                    "This symbol contains non-alphanumeric ASCII characters."
                }
                CstError::TextNotClosed => "This text isn't closed.",
                CstError::TextNotSufficientlyIndented => "This text isn't sufficiently indented.",
                CstError::TextInterpolationNotClosed => "This text interpolation isn't closed.",
                CstError::TextInterpolationMissesExpression => {
                    "Here's a start of a text interpolation without an expression after it."
                }
                CstError::TooMuchWhitespace => "There is too much whitespace here.",
                CstError::UnexpectedCharacters => "This is an unexpected character.",
                CstError::UnparsedRest => "The parser couldn't parse this rest.",
                CstError::WeirdWhitespace => "This is weird whitespace.",
                CstError::WeirdWhitespaceInIndentation => {
                    "This is weird whitespace. Make sure to use indent using two spaces."
                }
            }
            .to_string(),
            Self::Ast(error) => match error {
                AstError::ExpectedNameOrPatternInAssignment => {
                    "An assignment should have a name or pattern on the left side.".to_string()
                }
                AstError::ExpectedParameter => "A parameter should come here.".to_string(),
                AstError::FunctionMissesClosingCurlyBrace => {
                    "This function doesn't have a closing curly brace.".to_string()
                }
                AstError::ListItemMissesComma => {
                    "This list item should be followed by a comma.".to_string()
                }
                AstError::ListMissesClosingParenthesis => {
                    "This list doesn't have a closing parenthesis.".to_string()
                }
                AstError::ListWithNonListItem => "This is not a list item.".to_string(),
                AstError::OrPatternIsMissingIdentifiers {
                    identifier,
                    number_of_missing_captures,
                    ..
                } => {
                    format!(
                        "`{identifier}` is missing in {number_of_missing_captures} {} of this or-pattern.",
                        if number_of_missing_captures.get() == 1 { "sub-pattern" } else { "sub-patterns" },
                    )
                }
                AstError::ParenthesizedInPattern => {
                    "Parentheses are not allowed in patterns.".to_string()
                }
                AstError::ParenthesizedMissesClosingParenthesis => {
                    "This expression is parenthesized, but the closing parenthesis is missing."
                        .to_string()
                }
                AstError::PatternContainsInvalidExpression => {
                    "This type of expression is not allowed in patterns.".to_string()
                }
                AstError::PatternLiteralPartContainsInvalidExpression => {
                    "This type of expression is not allowed in this part of a pattern.".to_string()
                }
                AstError::PipeInPattern => "Pipes are not allowed in patterns.".to_string(),
                AstError::StructKeyMissesColon => {
                    "This struct key should be followed by a colon.".to_string()
                }
                AstError::StructMissesClosingBrace => {
                    "This struct doesn't have a closing bracket.".to_string()
                }
                AstError::StructShorthandWithNotIdentifier => {
                    "Shorthand syntax in structs only supports identifiers.".to_string()
                }
                AstError::StructValueMissesComma => {
                    "This struct value should be followed by a comma.".to_string()
                }
                AstError::StructWithNonStructField => {
                    "Structs should only contain struct key.".to_string()
                }
                AstError::TextInterpolationMissesClosingCurlyBraces => {
                    "This text interpolation never ends.".to_string()
                }
                AstError::TextMissesClosingQuote => "This text never ends.".to_string(),
                AstError::UnexpectedPunctuation => "This punctuation was unexpected.".to_string(),
            },
            Self::Hir(error) => match error {
                HirError::NeedsWithWrongNumberOfArguments { num_args } => {
                    format!("`needs` accepts one or two arguments, but was called with {num_args} arguments. Its parameters are the `condition` and an optional `message`.")
                }
                HirError::PatternContainsCall => "Calls in patterns are not allowed.".to_string(),
                HirError::PublicAssignmentInNotTopLevel => {
                    "Public assignments (:=) can only be used in top-level code.".to_string()
                }
                HirError::PublicAssignmentWithSameName { name } => {
                    format!("There already exists a public assignment (:=) named `{name}`.")
                }
                HirError::UnknownReference { name } => format!("`{name}` is not in scope."),
            },
            Self::Mir(error) => match error {
                MirError::UseWithInvalidPath { module, path } => {
                    format!(
                        "{module} tries to `use` {path:?}, but that's an invalid path.",
                    )
                }
                MirError::UseHasTooManyParentNavigations { module, path } => format!("{module} tries to `use` {path:?}, but that has too many parent navigations. You can't navigate out of the current package (the module that also contains a `_package.candy` file)."),
                MirError::ModuleNotFound { module, path } => format!(
                    "{module} tries to use {path:?}, but that module is not found.",
                ),
                MirError::UseNotStaticallyResolvable { containing_module } => format!(
                    "A `use` in {containing_module} is not statically resolvable.",
                ),
                MirError::ModuleHasCycle { cycle } => {
                    format!(
                        "There's a cycle in the used modules: {}",
                        cycle.iter().join(" → "),
                    )
                }
            },
        };
        write!(f, "{message}")
    }
}

impl CompilerError {
    #[must_use]
    pub fn to_related_information(&self) -> Vec<(Module, cst::Id, String)> {
        match &self.payload {
            CompilerErrorPayload::Ast(AstError::OrPatternIsMissingIdentifiers {
                all_captures,
                ..
            }) => all_captures
                .iter()
                .map(|capture| {
                    (
                        self.module.clone(),
                        *capture,
                        "The identifier is bound here.".to_string(),
                    )
                })
                .collect(),
            _ => vec![],
        }
    }
}

impl ToRichIr for CompilerError {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        let range = builder.push(
            format!(
                "{} (span: {} – {})",
                self.module, *self.span.start, *self.span.end,
            ),
            None,
            EnumSet::empty(),
        );
        builder.push_reference(
            ReferenceKey::ModuleWithSpan(self.module.clone(), self.span.clone()),
            range,
        );
        builder.push(format!(": {}", self.payload), None, EnumSet::empty());
    }
}
