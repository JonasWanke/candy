use crate::{
    ast::{AstError, AstResult, AstString},
    position::Offset,
};
use extension_trait::extension_trait;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Parser<'s> {
    file: &'s Path,
    source: &'s str,
    offset: Offset,
}
impl<'s> Parser<'s> {
    #[must_use]
    pub const fn new(file: &'s Path, source: &'s str) -> Self {
        Self {
            file,
            source,
            offset: Offset(0),
        }
    }

    #[must_use]
    pub const fn file(self) -> &'s Path {
        self.file
    }
    #[must_use]
    pub const fn source(self) -> &'s str {
        self.source
    }
    #[must_use]
    pub const fn offset(self) -> Offset {
        self.offset
    }

    #[must_use]
    pub fn rest(self) -> &'s str {
        &self.source[*self.offset..]
    }
    #[must_use]
    pub fn next_char(self) -> Option<char> {
        self.rest().chars().next()
    }
    #[must_use]
    pub fn is_at_end(self) -> bool {
        self.offset == Offset(self.source.len())
    }

    #[must_use]
    pub fn string(self, start_offset: Offset) -> AstString {
        assert!(start_offset <= self.offset);
        AstString {
            string: self.str(start_offset).into(),
            file: self.file.to_path_buf(),
            span: start_offset..self.offset,
        }
    }
    #[must_use]
    pub fn str(self, start_offset: Offset) -> &'s str {
        assert!(start_offset <= self.offset);
        &self.source[*start_offset..*self.offset]
    }

    #[must_use]
    pub fn error_at_current_offset(self, message: impl Into<String>) -> AstError {
        AstError {
            unparsable_input: AstString {
                string: "".into(),
                file: self.file.to_path_buf(),
                span: self.offset..self.offset,
            },
            error: message.into(),
        }
    }

    #[must_use]
    pub fn consume_literal(mut self, literal: &'static str) -> Option<Self> {
        if self.rest().starts_with(literal) {
            self.offset = Offset(*self.offset + literal.len());
            Some(self)
        } else {
            None
        }
    }
    #[must_use]
    pub fn consume_while_not_empty(
        self,
        predicate: impl FnMut(char) -> bool,
    ) -> Option<(Self, AstString)> {
        let (parser, string) = self.consume_while(predicate);
        if string.is_empty() {
            None
        } else {
            Some((parser, string))
        }
    }
    #[must_use]
    pub fn consume_while(mut self, mut predicate: impl FnMut(char) -> bool) -> (Self, AstString) {
        let start_offset = self.offset();
        while let Some((new_parser, c)) = self.consume_char()
            && predicate(c)
        {
            self = new_parser;
        }
        (self, self.string(start_offset))
    }
    #[must_use]
    pub fn consume_char(self) -> Option<(Self, char)> {
        self.next_char().map(|c| {
            (
                Parser {
                    file: self.file,
                    source: self.source,
                    offset: Offset(*self.offset + c.len_utf8()),
                },
                c,
            )
        })
    }
}

#[extension_trait]
pub impl<'s> OptionOfParser<'s> for Option<Parser<'s>> {
    #[must_use]
    fn unwrap_or_ast_error(
        self,
        original_parser: Parser<'s>,
        error_message: impl Into<String>,
    ) -> (Parser<'s>, Option<AstError>) {
        self.map_or_else(
            || {
                (
                    original_parser,
                    Some(original_parser.error_at_current_offset(error_message)),
                )
            },
            |parser| (parser, None),
        )
    }
}
#[extension_trait]
pub impl<T, 's> OptionOfParserWithValue<T, 's> for Option<(Parser<'s>, T)> {
    #[must_use]
    fn optional(self, original_parser: Parser<'s>) -> (Parser<'s>, Option<T>) {
        if let Some((parser, value)) = self {
            (parser, Some(value))
        } else {
            (original_parser, None)
        }
    }
    #[must_use]
    fn unwrap_or_ast_error(
        self,
        original_parser: Parser<'s>,
        error_message: impl Into<String>,
    ) -> (Parser<'s>, AstResult<T>) {
        if let Some((parser, value)) = self {
            (parser, AstResult::ok(value))
        } else {
            (
                original_parser,
                AstResult::error(None, original_parser.error_at_current_offset(error_message)),
            )
        }
    }
}
#[extension_trait]
pub impl<T, 's> OptionOfParserWithResult<T, 's> for Option<(Parser<'s>, AstResult<T>)> {
    #[must_use]
    fn unwrap_or_ast_error_result(
        self,
        original_parser: Parser<'s>,
        error_message: impl Into<String>,
    ) -> (Parser<'s>, AstResult<T>) {
        if let Some((parser, value)) = self {
            (parser, value)
        } else {
            (
                original_parser,
                AstResult::error(None, original_parser.error_at_current_offset(error_message)),
            )
        }
    }
}
