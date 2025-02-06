use super::{Cst, CstKind};
use std::ops::Deref;

pub trait UnwrapWhitespaceAndComment {
    #[must_use]
    fn unwrap_whitespace_and_comment(&self) -> Self;
}
impl<D: Clone> UnwrapWhitespaceAndComment for Cst<D> {
    fn unwrap_whitespace_and_comment(&self) -> Self {
        let kind = match &self.kind {
            kind @ (CstKind::EqualsSign
            | CstKind::Comma
            | CstKind::Dot
            | CstKind::Colon
            | CstKind::ColonEqualsSign
            | CstKind::Bar
            | CstKind::OpeningParenthesis
            | CstKind::ClosingParenthesis
            | CstKind::OpeningBracket
            | CstKind::ClosingBracket
            | CstKind::OpeningCurlyBrace
            | CstKind::ClosingCurlyBrace
            | CstKind::Arrow
            | CstKind::SingleQuote
            | CstKind::DoubleQuote
            | CstKind::Percent
            | CstKind::Octothorpe
            | CstKind::Whitespace(_)
            | CstKind::Newline(_)
            | CstKind::Comment { .. }) => kind.clone(),
            CstKind::TrailingWhitespace { box child, .. } => {
                return child.unwrap_whitespace_and_comment()
            }
            kind @ (CstKind::Identifier(_) | CstKind::Symbol(_) | CstKind::Int { .. }) => {
                kind.clone()
            }
            CstKind::OpeningText {
                opening_single_quotes,
                opening_double_quote,
            } => CstKind::OpeningText {
                opening_single_quotes: opening_single_quotes.unwrap_whitespace_and_comment(),
                opening_double_quote: opening_double_quote.unwrap_whitespace_and_comment(),
            },
            CstKind::ClosingText {
                closing_double_quote,
                closing_single_quotes,
            } => CstKind::ClosingText {
                closing_double_quote: closing_double_quote.unwrap_whitespace_and_comment(),
                closing_single_quotes: closing_single_quotes.unwrap_whitespace_and_comment(),
            },
            CstKind::Text {
                opening,
                parts,
                closing,
            } => CstKind::Text {
                opening: opening.unwrap_whitespace_and_comment(),
                parts: parts.unwrap_whitespace_and_comment(),
                closing: closing.unwrap_whitespace_and_comment(),
            },
            kind @ (CstKind::TextNewline(_) | CstKind::TextPart(_)) => kind.clone(),
            CstKind::TextInterpolation {
                opening_curly_braces,
                expression,
                closing_curly_braces,
            } => CstKind::TextInterpolation {
                opening_curly_braces: opening_curly_braces.unwrap_whitespace_and_comment(),
                expression: expression.unwrap_whitespace_and_comment(),
                closing_curly_braces: closing_curly_braces.unwrap_whitespace_and_comment(),
            },
            CstKind::BinaryBar { left, bar, right } => CstKind::BinaryBar {
                left: left.unwrap_whitespace_and_comment(),
                bar: bar.unwrap_whitespace_and_comment(),
                right: right.unwrap_whitespace_and_comment(),
            },
            CstKind::Parenthesized {
                opening_parenthesis,
                inner,
                closing_parenthesis,
            } => CstKind::Parenthesized {
                opening_parenthesis: opening_parenthesis.unwrap_whitespace_and_comment(),
                inner: inner.unwrap_whitespace_and_comment(),
                closing_parenthesis: closing_parenthesis.unwrap_whitespace_and_comment(),
            },
            CstKind::Call {
                receiver,
                arguments,
            } => CstKind::Call {
                receiver: receiver.unwrap_whitespace_and_comment(),
                arguments: arguments.unwrap_whitespace_and_comment(),
            },
            CstKind::List {
                opening_parenthesis,
                items,
                closing_parenthesis,
            } => CstKind::List {
                opening_parenthesis: opening_parenthesis.unwrap_whitespace_and_comment(),
                items: items.unwrap_whitespace_and_comment(),
                closing_parenthesis: closing_parenthesis.unwrap_whitespace_and_comment(),
            },
            CstKind::ListItem { value, comma } => CstKind::ListItem {
                value: value.unwrap_whitespace_and_comment(),
                comma: comma
                    .as_ref()
                    .map(UnwrapWhitespaceAndComment::unwrap_whitespace_and_comment),
            },
            CstKind::Struct {
                opening_bracket,
                fields,
                closing_bracket,
            } => CstKind::Struct {
                opening_bracket: opening_bracket.unwrap_whitespace_and_comment(),
                fields: fields.unwrap_whitespace_and_comment(),
                closing_bracket: closing_bracket.unwrap_whitespace_and_comment(),
            },
            CstKind::StructField {
                key_and_colon,
                value,
                comma,
            } => CstKind::StructField {
                key_and_colon: key_and_colon.as_deref().map(|(key, colon)| {
                    Box::new((
                        key.unwrap_whitespace_and_comment(),
                        colon.unwrap_whitespace_and_comment(),
                    ))
                }),
                value: value.unwrap_whitespace_and_comment(),
                comma: comma
                    .as_ref()
                    .map(UnwrapWhitespaceAndComment::unwrap_whitespace_and_comment),
            },
            CstKind::StructAccess { struct_, dot, key } => CstKind::StructAccess {
                struct_: struct_.unwrap_whitespace_and_comment(),
                dot: dot.unwrap_whitespace_and_comment(),
                key: key.unwrap_whitespace_and_comment(),
            },
            CstKind::Match {
                expression,
                percent,
                cases,
            } => CstKind::Match {
                expression: expression.unwrap_whitespace_and_comment(),
                percent: percent.unwrap_whitespace_and_comment(),
                cases: cases.unwrap_whitespace_and_comment(),
            },
            CstKind::MatchCase {
                pattern,
                condition,
                arrow,
                body,
            } => CstKind::MatchCase {
                pattern: pattern.unwrap_whitespace_and_comment(),
                condition: condition.as_deref().map(|(comma, condition)| {
                    Box::new((
                        comma.unwrap_whitespace_and_comment(),
                        condition.unwrap_whitespace_and_comment(),
                    ))
                }),
                arrow: arrow.unwrap_whitespace_and_comment(),
                body: body.unwrap_whitespace_and_comment(),
            },
            CstKind::Function {
                opening_curly_brace,
                parameters_and_arrow,
                body,
                closing_curly_brace,
            } => CstKind::Function {
                opening_curly_brace: opening_curly_brace.unwrap_whitespace_and_comment(),
                parameters_and_arrow: parameters_and_arrow.as_ref().map(|(parameters, arrow)| {
                    (
                        parameters.unwrap_whitespace_and_comment(),
                        arrow.unwrap_whitespace_and_comment(),
                    )
                }),
                body: body.unwrap_whitespace_and_comment(),
                closing_curly_brace: closing_curly_brace.unwrap_whitespace_and_comment(),
            },
            CstKind::Assignment {
                left,
                assignment_sign,
                body,
            } => CstKind::Assignment {
                left: left.unwrap_whitespace_and_comment(),
                assignment_sign: assignment_sign.unwrap_whitespace_and_comment(),
                body: body.unwrap_whitespace_and_comment(),
            },
            kind @ CstKind::Error { .. } => kind.clone(),
        };
        Self {
            data: self.data.clone(),
            kind,
        }
    }
}
impl<C: UnwrapWhitespaceAndComment> UnwrapWhitespaceAndComment for Box<C> {
    fn unwrap_whitespace_and_comment(&self) -> Self {
        Self::new(self.deref().unwrap_whitespace_and_comment())
    }
}
impl<D: Clone> UnwrapWhitespaceAndComment for Vec<Cst<D>> {
    fn unwrap_whitespace_and_comment(&self) -> Self {
        self.iter()
            .filter(|it| !it.is_whitespace_or_comment())
            .map(UnwrapWhitespaceAndComment::unwrap_whitespace_and_comment)
            .collect()
    }
}
