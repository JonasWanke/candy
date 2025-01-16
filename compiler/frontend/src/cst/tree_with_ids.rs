use super::{Cst, CstKind, Id};
use crate::position::Offset;

pub trait TreeWithIds {
    fn first_id(&self) -> Option<Id>;
    fn find(&self, id: Id) -> Option<&Cst>;

    fn first_offset(&self) -> Option<Offset>;
    fn find_by_offset(&self, offset: Offset) -> Option<&Cst>;
}
impl TreeWithIds for Cst {
    fn first_id(&self) -> Option<Id> {
        Some(self.data.id)
    }
    fn find(&self, id: Id) -> Option<&Cst> {
        if id == self.data.id {
            return Some(self);
        };

        match &self.kind {
            CstKind::EqualsSign
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
            | CstKind::Newline(_) => None,
            CstKind::Comment {
                octothorpe,
                comment: _,
            } => octothorpe.find(id),
            CstKind::TrailingWhitespace { child, whitespace } => {
                child.find(id).or_else(|| whitespace.find(id))
            }
            CstKind::Identifier(_)
            | CstKind::Symbol(_)
            | CstKind::Int {
                radix_prefix: _,
                value: _,
                string: _,
            } => None,
            CstKind::OpeningText {
                opening_single_quotes,
                opening_double_quote,
            } => opening_single_quotes
                .find(id)
                .or_else(|| opening_double_quote.find(id)),
            CstKind::ClosingText {
                closing_double_quote,
                closing_single_quotes,
            } => closing_double_quote
                .find(id)
                .or_else(|| closing_single_quotes.find(id)),
            CstKind::Text {
                opening,
                parts,
                closing,
            } => opening
                .find(id)
                .or_else(|| parts.find(id))
                .or_else(|| closing.find(id)),
            CstKind::TextNewline(_) | CstKind::TextPart(_) => None,
            CstKind::TextInterpolation {
                opening_curly_braces,
                expression,
                closing_curly_braces,
            } => opening_curly_braces
                .find(id)
                .or_else(|| expression.find(id))
                .or_else(|| closing_curly_braces.find(id)),
            CstKind::BinaryBar { left, bar, right } => left
                .find(id)
                .or_else(|| bar.find(id))
                .or_else(|| right.find(id)),
            CstKind::Parenthesized {
                opening_parenthesis,
                inner,
                closing_parenthesis,
            } => opening_parenthesis
                .find(id)
                .or_else(|| inner.find(id))
                .or_else(|| closing_parenthesis.find(id)),
            CstKind::Call {
                receiver,
                arguments,
            } => receiver.find(id).or_else(|| arguments.find(id)),
            CstKind::List {
                opening_parenthesis,
                items,
                closing_parenthesis,
            } => opening_parenthesis
                .find(id)
                .or_else(|| items.find(id))
                .or_else(|| closing_parenthesis.find(id)),
            CstKind::ListItem { value, comma } => value
                .find(id)
                .or_else(|| comma.as_ref().and_then(|comma| comma.find(id))),
            CstKind::Struct {
                opening_bracket,
                fields,
                closing_bracket,
            } => opening_bracket
                .find(id)
                .or_else(|| fields.find(id))
                .or_else(|| closing_bracket.find(id)),
            CstKind::StructField {
                key_and_colon,
                value,
                comma,
            } => key_and_colon
                .as_deref()
                .and_then(|(key, colon)| key.find(id).or_else(|| colon.find(id)))
                .or_else(|| value.find(id))
                .or_else(|| comma.as_ref().and_then(|comma| comma.find(id))),
            CstKind::StructAccess { struct_, dot, key } => struct_
                .find(id)
                .or_else(|| dot.find(id))
                .or_else(|| key.find(id)),
            CstKind::Match {
                expression,
                percent,
                cases,
            } => expression
                .find(id)
                .or_else(|| percent.find(id))
                .or_else(|| cases.find(id)),
            CstKind::MatchCase {
                pattern,
                condition,
                arrow,
                body,
            } => pattern
                .find(id)
                .or_else(|| {
                    condition.as_deref().and_then(|(comma, condition)| {
                        comma.find(id).or_else(|| condition.find(id))
                    })
                })
                .or_else(|| arrow.find(id))
                .or_else(|| body.find(id)),
            CstKind::Function {
                opening_curly_brace,
                parameters_and_arrow,
                body,
                closing_curly_brace,
            } => opening_curly_brace
                .find(id)
                .or_else(|| {
                    parameters_and_arrow
                        .as_ref()
                        .and_then(|(parameters, arrow)| {
                            parameters.find(id).or_else(|| arrow.find(id))
                        })
                })
                .or_else(|| body.find(id))
                .or_else(|| closing_curly_brace.find(id)),
            CstKind::Assignment {
                left,
                assignment_sign,
                body,
            } => left
                .find(id)
                .or_else(|| assignment_sign.find(id))
                .or_else(|| body.find(id)),
            CstKind::Error { .. } => None,
        }
    }

    fn first_offset(&self) -> Option<Offset> {
        Some(self.data.span.start)
    }
    fn find_by_offset(&self, offset: Offset) -> Option<&Cst> {
        let (inner, is_end_inclusive) = match &self.kind {
            CstKind::EqualsSign
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
            | CstKind::Newline(_) => (None, false),
            CstKind::Comment {
                octothorpe,
                comment: _,
            } => (octothorpe.find_by_offset(offset), true),
            CstKind::TrailingWhitespace {
                child,
                whitespace: _,
            } => (child.find_by_offset(offset), false),
            CstKind::Identifier(_)
            | CstKind::Symbol(_)
            | CstKind::Int {
                radix_prefix: _,
                value: _,
                string: _,
            } => (None, true),
            CstKind::Text {
                opening: _,
                parts,
                closing: _,
            } => {
                let interpolation_index = parts
                    .binary_search_by_key(&offset, |it| it.first_offset().unwrap())
                    .or_else(|err| if err == 0 { Err(()) } else { Ok(err - 1) })
                    .ok();

                if let Some(part) = interpolation_index.map(|index| &parts[index])
                    && part.kind.is_text_interpolation()
                    && let Some(child) = part.find_by_offset(offset)
                    && !child.kind.is_text_interpolation()
                {
                    (Some(child), false)
                } else {
                    (None, false)
                }
            }
            CstKind::OpeningText {
                opening_single_quotes: _,
                opening_double_quote: _,
            }
            | CstKind::ClosingText {
                closing_double_quote: _,
                closing_single_quotes: _,
            }
            | CstKind::TextNewline(_)
            | CstKind::TextPart(_) => (None, false),
            CstKind::TextInterpolation {
                opening_curly_braces: _,
                expression,
                closing_curly_braces: _,
            } => (expression.find_by_offset(offset), false),
            CstKind::BinaryBar { left, bar, right } => (
                left.find_by_offset(offset)
                    .or_else(|| bar.find_by_offset(offset))
                    .or_else(|| right.find_by_offset(offset)),
                false,
            ),
            CstKind::Parenthesized {
                opening_parenthesis: _,
                inner,
                closing_parenthesis: _,
            } => (inner.find_by_offset(offset), false),
            CstKind::Call {
                receiver,
                arguments,
            } => (
                receiver
                    .find_by_offset(offset)
                    .or_else(|| arguments.find_by_offset(offset)),
                false,
            ),
            CstKind::List {
                opening_parenthesis,
                items,
                closing_parenthesis,
            } => (
                opening_parenthesis
                    .find_by_offset(offset)
                    .or_else(|| items.find_by_offset(offset))
                    .or_else(|| closing_parenthesis.find_by_offset(offset)),
                false,
            ),
            CstKind::ListItem { value, comma } => (
                value
                    .find_by_offset(offset)
                    .or_else(|| comma.find_by_offset(offset)),
                false,
            ),
            CstKind::Struct {
                opening_bracket,
                fields,
                closing_bracket,
            } => (
                opening_bracket
                    .find_by_offset(offset)
                    .or_else(|| fields.find_by_offset(offset))
                    .or_else(|| closing_bracket.find_by_offset(offset)),
                false,
            ),
            CstKind::StructField {
                key_and_colon,
                value,
                comma,
            } => (
                key_and_colon
                    .as_deref()
                    .and_then(|(key, colon)| {
                        key.find_by_offset(offset)
                            .or_else(|| colon.find_by_offset(offset))
                    })
                    .or_else(|| value.find_by_offset(offset))
                    .or_else(|| comma.find_by_offset(offset)),
                false,
            ),
            CstKind::StructAccess { struct_, dot, key } => (
                struct_
                    .find_by_offset(offset)
                    .or_else(|| dot.find_by_offset(offset))
                    .or_else(|| key.find_by_offset(offset)),
                false,
            ),
            CstKind::Match {
                expression,
                percent,
                cases,
            } => (
                expression
                    .find_by_offset(offset)
                    .or_else(|| percent.find_by_offset(offset))
                    .or_else(|| cases.find_by_offset(offset)),
                false,
            ),
            CstKind::MatchCase {
                pattern,
                condition,
                arrow,
                body,
            } => (
                pattern
                    .find_by_offset(offset)
                    .or_else(|| {
                        condition.as_deref().and_then(|(comma, condition)| {
                            comma
                                .find_by_offset(offset)
                                .or_else(|| condition.find_by_offset(offset))
                        })
                    })
                    .or_else(|| arrow.find_by_offset(offset))
                    .or_else(|| body.find_by_offset(offset)),
                false,
            ),
            CstKind::Function {
                opening_curly_brace,
                parameters_and_arrow,
                body,
                closing_curly_brace,
            } => (
                opening_curly_brace
                    .find_by_offset(offset)
                    .or_else(|| {
                        parameters_and_arrow
                            .as_ref()
                            .and_then(|(parameters, arrow)| {
                                parameters
                                    .find_by_offset(offset)
                                    .or_else(|| arrow.find_by_offset(offset))
                            })
                    })
                    .or_else(|| body.find_by_offset(offset))
                    .or_else(|| closing_curly_brace.find_by_offset(offset)),
                false,
            ),
            CstKind::Assignment {
                left,
                assignment_sign,
                body,
            } => (
                left.find_by_offset(offset)
                    .or_else(|| assignment_sign.find_by_offset(offset))
                    .or_else(|| body.find_by_offset(offset)),
                false,
            ),
            CstKind::Error { .. } => (None, false),
        };

        inner.or_else(|| {
            if self.data.span.contains(&offset)
                || (is_end_inclusive && self.data.span.end == offset)
            {
                Some(self)
            } else {
                None
            }
        })
    }
}
impl<T: TreeWithIds> TreeWithIds for Option<T> {
    fn first_id(&self) -> Option<Id> {
        self.as_ref().and_then(TreeWithIds::first_id)
    }
    fn find(&self, id: Id) -> Option<&Cst> {
        self.as_ref().and_then(|it| it.find(id))
    }

    fn first_offset(&self) -> Option<Offset> {
        self.as_ref().and_then(TreeWithIds::first_offset)
    }
    fn find_by_offset(&self, offset: Offset) -> Option<&Cst> {
        self.as_ref().and_then(|it| it.find_by_offset(offset))
    }
}
impl<T: TreeWithIds> TreeWithIds for Box<T> {
    fn first_id(&self) -> Option<Id> {
        self.as_ref().first_id()
    }
    fn find(&self, id: Id) -> Option<&Cst> {
        self.as_ref().find(id)
    }

    fn first_offset(&self) -> Option<Offset> {
        self.as_ref().first_offset()
    }
    fn find_by_offset(&self, offset: Offset) -> Option<&Cst> {
        self.as_ref().find_by_offset(offset)
    }
}
impl<T: TreeWithIds> TreeWithIds for [T] {
    fn first_id(&self) -> Option<Id> {
        self.iter().find_map(TreeWithIds::first_id)
    }
    fn find(&self, id: Id) -> Option<&Cst> {
        let child_index = self
            .binary_search_by_key(&id, |it| it.first_id().unwrap())
            .or_else(|err| if err == 0 { Err(()) } else { Ok(err - 1) })
            .ok()?;
        self[child_index].find(id)
    }

    fn first_offset(&self) -> Option<Offset> {
        self.iter().find_map(TreeWithIds::first_offset)
    }
    fn find_by_offset(&self, offset: Offset) -> Option<&Cst> {
        let child_index = self
            .binary_search_by_key(&offset, |it| it.first_offset().unwrap())
            .or_else(|err| if err == 0 { Err(()) } else { Ok(err - 1) })
            .ok()?;
        self[child_index].find_by_offset(offset)
    }
}
