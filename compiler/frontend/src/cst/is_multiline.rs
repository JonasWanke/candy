use super::{Cst, CstKind};

pub trait IsMultiline {
    fn is_singleline(&self) -> bool {
        !self.is_multiline()
    }
    fn is_multiline(&self) -> bool;
}
impl<D> IsMultiline for Cst<D> {
    fn is_multiline(&self) -> bool {
        self.kind.is_multiline()
    }
}
impl<D> IsMultiline for CstKind<D> {
    fn is_multiline(&self) -> bool {
        match self {
            Self::EqualsSign => false,
            Self::Comma => false,
            Self::Dot => false,
            Self::Colon => false,
            Self::ColonEqualsSign => false,
            Self::Bar => false,
            Self::OpeningParenthesis => false,
            Self::ClosingParenthesis => false,
            Self::OpeningBracket => false,
            Self::ClosingBracket => false,
            Self::OpeningCurlyBrace => false,
            Self::ClosingCurlyBrace => false,
            Self::Arrow => false,
            Self::SingleQuote => false,
            Self::DoubleQuote => false,
            Self::Percent => false,
            Self::Octothorpe => false,
            Self::Whitespace(_) => false,
            Self::Newline(_) => true,
            Self::Comment { .. } => false,
            Self::TrailingWhitespace { child, whitespace } => {
                child.is_multiline() || whitespace.is_multiline()
            }
            Self::Identifier(_) => false,
            Self::Symbol(_) => false,
            Self::Int { .. } => false,
            Self::OpeningText { .. } => false,
            Self::ClosingText { .. } => false,
            Self::Text {
                opening,
                parts,
                closing,
            } => opening.is_multiline() || parts.is_multiline() || closing.is_multiline(),
            Self::TextNewline(_) => true,
            Self::TextPart(_) => false,
            Self::TextInterpolation { expression, .. } => expression.is_multiline(),
            Self::BinaryBar { left, bar, right } => {
                left.is_multiline() || bar.is_multiline() || right.is_multiline()
            }
            Self::Parenthesized {
                opening_parenthesis,
                inner,
                closing_parenthesis,
            } => {
                opening_parenthesis.is_multiline()
                    || inner.is_multiline()
                    || closing_parenthesis.is_multiline()
            }
            Self::Call {
                receiver,
                arguments,
            } => receiver.is_multiline() || arguments.is_multiline(),
            Self::List {
                opening_parenthesis,
                items,
                closing_parenthesis,
            } => {
                opening_parenthesis.is_multiline()
                    || items.is_multiline()
                    || closing_parenthesis.is_multiline()
            }
            Self::ListItem { value, comma } => {
                value.is_multiline() || comma.as_ref().map_or(false, |comma| comma.is_multiline())
            }
            Self::Struct {
                opening_bracket,
                fields,
                closing_bracket,
            } => {
                opening_bracket.is_multiline()
                    || fields.is_multiline()
                    || closing_bracket.is_multiline()
            }
            Self::StructField {
                key_and_colon,
                value,
                comma,
            } => {
                key_and_colon.as_deref().map_or(false, |(key, colon)| {
                    key.is_multiline() || colon.is_multiline()
                }) || value.is_multiline()
                    || comma.as_ref().map_or(false, |comma| comma.is_multiline())
            }
            Self::StructAccess { struct_, dot, key } => {
                struct_.is_multiline() || dot.is_multiline() || key.is_multiline()
            }
            Self::Match {
                expression,
                percent,
                cases,
            } => expression.is_multiline() || percent.is_multiline() || cases.is_multiline(),
            Self::MatchCase {
                pattern,
                condition,
                arrow,
                body,
            } => {
                pattern.is_multiline()
                    || condition.as_deref().map_or(false, |(comma, condition)| {
                        comma.is_multiline() || condition.is_multiline()
                    })
                    || arrow.is_multiline()
                    || body.is_multiline()
            }
            Self::Function {
                opening_curly_brace,
                parameters_and_arrow,
                body,
                closing_curly_brace,
            } => {
                opening_curly_brace.is_multiline()
                    || parameters_and_arrow
                        .as_ref()
                        .map_or(false, |(parameters, arrow)| {
                            parameters.is_multiline() || arrow.is_multiline()
                        })
                    || body.is_multiline()
                    || closing_curly_brace.is_multiline()
            }
            Self::Assignment {
                left,
                assignment_sign,
                body,
            } => left.is_multiline() || assignment_sign.is_multiline() || body.is_multiline(),
            Self::Error {
                unparsable_input, ..
            } => unparsable_input.is_multiline(),
        }
    }
}

impl IsMultiline for str {
    fn is_multiline(&self) -> bool {
        self.contains('\n')
    }
}

impl<C: IsMultiline> IsMultiline for Vec<C> {
    fn is_multiline(&self) -> bool {
        self.iter().any(IsMultiline::is_multiline)
    }
}

impl<T: IsMultiline> IsMultiline for Option<T> {
    fn is_multiline(&self) -> bool {
        self.as_ref().map_or(false, IsMultiline::is_multiline)
    }
}

impl<A: IsMultiline, B: IsMultiline> IsMultiline for (A, B) {
    fn is_multiline(&self) -> bool {
        self.0.is_multiline() || self.1.is_multiline()
    }
}
