use super::{Cst, CstData, CstError};
use crate::rich_ir::{RichIrBuilder, ToRichIr, TokenType};
use enumset::EnumSet;
use num_bigint::{BigInt, BigUint};
use std::fmt::{self, Display, Formatter, Pointer};
use strum_macros::EnumIs;

#[derive(Clone, Debug, EnumIs, Eq, Hash, PartialEq)]
pub enum CstKind<D = CstData> {
    EqualsSign,         // =
    Comma,              // ,
    Dot,                // .
    Colon,              // :
    ColonEqualsSign,    // :=
    Bar,                // |
    OpeningParenthesis, // (
    ClosingParenthesis, // )
    OpeningBracket,     // [
    ClosingBracket,     // ]
    OpeningCurlyBrace,  // {
    ClosingCurlyBrace,  // }
    Arrow,              // ->
    SingleQuote,        // '
    DoubleQuote,        // "
    Percent,            // %
    Octothorpe,         // #
    Whitespace(String), // contains only non-multiline whitespace
    Newline(String), // the associated `String` because some systems (such as Windows) have weird newlines
    Comment {
        octothorpe: Box<Cst<D>>,
        comment: String,
    },
    TrailingWhitespace {
        child: Box<Cst<D>>,
        whitespace: Vec<Cst<D>>,
    },
    Identifier(String),
    Symbol(String),
    Int {
        radix_prefix: Option<(IntRadix, String)>,
        value: BigUint,
        string: String,
    },
    OpeningText {
        opening_single_quotes: Vec<Cst<D>>,
        opening_double_quote: Box<Cst<D>>,
    },
    ClosingText {
        closing_double_quote: Box<Cst<D>>,
        closing_single_quotes: Vec<Cst<D>>,
    },
    Text {
        opening: Box<Cst<D>>,
        parts: Vec<Cst<D>>,
        closing: Box<Cst<D>>,
    },
    TextNewline(String), // special newline for text because line breaks have semantic meaning there
    TextPart(String),
    TextInterpolation {
        opening_curly_braces: Vec<Cst<D>>,
        expression: Box<Cst<D>>,
        closing_curly_braces: Vec<Cst<D>>,
    },
    BinaryBar {
        left: Box<Cst<D>>,
        bar: Box<Cst<D>>,
        right: Box<Cst<D>>,
    },
    Parenthesized {
        opening_parenthesis: Box<Cst<D>>,
        inner: Box<Cst<D>>,
        closing_parenthesis: Box<Cst<D>>,
    },
    Call {
        receiver: Box<Cst<D>>,
        arguments: Vec<Cst<D>>,
    },
    List {
        opening_parenthesis: Box<Cst<D>>,
        items: Vec<Cst<D>>,
        closing_parenthesis: Box<Cst<D>>,
    },
    ListItem {
        value: Box<Cst<D>>,
        comma: Option<Box<Cst<D>>>,
    },
    Struct {
        opening_bracket: Box<Cst<D>>,
        fields: Vec<Cst<D>>,
        closing_bracket: Box<Cst<D>>,
    },
    StructField {
        key_and_colon: Option<Box<(Cst<D>, Cst<D>)>>,
        value: Box<Cst<D>>,
        comma: Option<Box<Cst<D>>>,
    },
    StructAccess {
        struct_: Box<Cst<D>>,
        dot: Box<Cst<D>>,
        key: Box<Cst<D>>,
    },
    Match {
        expression: Box<Cst<D>>,
        percent: Box<Cst<D>>,
        cases: Vec<Cst<D>>,
    },
    MatchCase {
        pattern: Box<Cst<D>>,
        condition: Option<MatchCaseWithComma<D>>,
        arrow: Box<Cst<D>>,
        body: Vec<Cst<D>>,
    },
    Function {
        opening_curly_brace: Box<Cst<D>>,
        parameters_and_arrow: Option<FunctionParametersAndArrow<D>>,
        body: Vec<Cst<D>>,
        closing_curly_brace: Box<Cst<D>>,
    },
    Assignment {
        left: Box<Cst<D>>,
        assignment_sign: Box<Cst<D>>,
        body: Vec<Cst<D>>,
    },
    Error {
        unparsable_input: String,
        error: CstError,
    },
}
#[derive(Clone, Debug, EnumIs, Eq, Hash, PartialEq)]
pub enum IntRadix {
    Binary,
    Hexadecimal,
}
pub type MatchCaseWithComma<D> = Box<(Cst<D>, Cst<D>)>;
pub type FunctionParametersAndArrow<D> = (Vec<Cst<D>>, Box<Cst<D>>);

impl<D> CstKind<D> {
    #[must_use]
    pub fn is_whitespace_or_comment(&self) -> bool {
        match self {
            Self::Whitespace(_) | Self::Newline(_) | Self::Comment { .. } => true,
            Self::TrailingWhitespace { child, .. } => (**child).is_whitespace_or_comment(),
            _ => false,
        }
    }

    #[must_use]
    pub fn children(&self) -> Vec<&Cst<D>> {
        match self {
            Self::EqualsSign
            | Self::Comma
            | Self::Dot
            | Self::Colon
            | Self::ColonEqualsSign
            | Self::Bar
            | Self::OpeningParenthesis
            | Self::ClosingParenthesis
            | Self::OpeningBracket
            | Self::ClosingBracket
            | Self::OpeningCurlyBrace
            | Self::ClosingCurlyBrace
            | Self::Arrow
            | Self::SingleQuote
            | Self::DoubleQuote
            | Self::Percent
            | Self::Octothorpe
            | Self::Whitespace(_)
            | Self::Newline(_) => vec![],
            Self::Comment { octothorpe, .. } => vec![octothorpe],
            Self::TrailingWhitespace { child, whitespace } => {
                let mut children = vec![child.as_ref()];
                children.extend(whitespace);
                children
            }
            Self::Identifier(_) | Self::Symbol(_) | Self::Int { .. } => vec![],
            Self::OpeningText {
                opening_single_quotes,
                opening_double_quote,
            } => {
                let mut children = vec![];
                children.extend(opening_single_quotes);
                children.push(opening_double_quote);
                children
            }
            Self::ClosingText {
                closing_double_quote,
                closing_single_quotes,
            } => {
                let mut children = vec![closing_double_quote.as_ref()];
                children.extend(closing_single_quotes);
                children
            }
            Self::Text {
                opening,
                parts,
                closing,
            } => {
                let mut children = vec![opening.as_ref()];
                children.extend(parts);
                children.push(closing);
                children
            }
            Self::TextNewline(_) | Self::TextPart(_) => vec![],
            Self::TextInterpolation {
                opening_curly_braces,
                expression,
                closing_curly_braces,
            } => {
                let mut children = vec![];
                children.extend(opening_curly_braces);
                children.push(expression);
                children.extend(closing_curly_braces);
                children
            }
            Self::BinaryBar { left, bar, right } => {
                let mut children = vec![left.as_ref()];
                children.push(bar);
                children.push(right);
                children
            }
            Self::Parenthesized {
                opening_parenthesis,
                inner,
                closing_parenthesis,
            } => {
                let mut children = vec![opening_parenthesis.as_ref()];
                children.push(inner);
                children.push(closing_parenthesis);
                children
            }
            Self::Call {
                receiver,
                arguments,
            } => {
                let mut children = vec![receiver.as_ref()];
                children.extend(arguments);
                children
            }
            Self::List {
                opening_parenthesis,
                items,
                closing_parenthesis,
            } => {
                let mut children = vec![opening_parenthesis.as_ref()];
                children.extend(items);
                children.push(closing_parenthesis);
                children
            }
            Self::ListItem { value, comma } => {
                let mut children = vec![value.as_ref()];
                if let Some(comma) = comma {
                    children.push(comma);
                }
                children
            }
            Self::Struct {
                opening_bracket,
                fields,
                closing_bracket,
            } => {
                let mut children = vec![opening_bracket.as_ref()];
                children.extend(fields);
                children.push(closing_bracket);
                children
            }
            Self::StructField {
                key_and_colon,
                value,
                comma,
            } => {
                let mut children = vec![];
                if let Some(box (key, colon)) = key_and_colon {
                    children.push(key);
                    children.push(colon);
                }
                children.push(value);
                if let Some(box comma) = comma {
                    children.push(comma);
                }
                children
            }
            Self::StructAccess { struct_, dot, key } => {
                vec![struct_.as_ref(), dot.as_ref(), key.as_ref()]
            }
            Self::Match {
                expression,
                percent,
                cases,
            } => {
                let mut children = vec![expression.as_ref(), percent.as_ref()];
                children.extend(cases);
                children
            }
            Self::MatchCase {
                pattern,
                condition,
                arrow,
                body,
            } => {
                let mut children = vec![pattern.as_ref(), arrow.as_ref()];
                if let Some(box (comma, condition)) = condition {
                    children.extend([&comma, &condition]);
                }
                children.extend(body);
                children
            }
            Self::Function {
                opening_curly_brace,
                parameters_and_arrow,
                body,
                closing_curly_brace,
            } => {
                let mut children = vec![opening_curly_brace.as_ref()];
                if let Some((parameters, arrow)) = parameters_and_arrow {
                    children.extend(parameters);
                    children.push(arrow);
                }
                children.extend(body);
                children.push(closing_curly_brace);
                children
            }
            Self::Assignment {
                left,
                assignment_sign,
                body,
            } => {
                let mut children = vec![left.as_ref()];
                children.push(assignment_sign);
                children.extend(body);
                children
            }
            Self::Error { .. } => vec![],
        }
    }
}

impl<D> Display for CstKind<D> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match &self {
            Self::EqualsSign => '='.fmt(f),
            Self::Comma => ','.fmt(f),
            Self::Dot => '.'.fmt(f),
            Self::Colon => ':'.fmt(f),
            Self::ColonEqualsSign => ":=".fmt(f),
            Self::Bar => '|'.fmt(f),
            Self::OpeningParenthesis => '('.fmt(f),
            Self::ClosingParenthesis => ')'.fmt(f),
            Self::OpeningBracket => '['.fmt(f),
            Self::ClosingBracket => ']'.fmt(f),
            Self::OpeningCurlyBrace => '{'.fmt(f),
            Self::ClosingCurlyBrace => '}'.fmt(f),
            Self::Arrow => "->".fmt(f),
            Self::SingleQuote => '\''.fmt(f),
            Self::DoubleQuote => '"'.fmt(f),
            Self::Percent => '%'.fmt(f),
            Self::Octothorpe => '#'.fmt(f),
            Self::Whitespace(whitespace) => whitespace.fmt(f),
            Self::Newline(newline) => newline.fmt(f),
            Self::Comment {
                octothorpe,
                comment,
            } => {
                octothorpe.fmt(f)?;
                comment.fmt(f)
            }
            Self::TrailingWhitespace { child, whitespace } => {
                child.fmt(f)?;
                for w in whitespace {
                    w.fmt(f)?;
                }
                Ok(())
            }
            Self::Identifier(identifier) => identifier.fmt(f),
            Self::Symbol(symbol) => symbol.fmt(f),
            Self::Int {
                radix_prefix,
                value: _,
                string,
            } => {
                if let Some((_, radix_string)) = radix_prefix {
                    radix_string.fmt(f)?;
                }
                string.fmt(f)
            }
            Self::OpeningText {
                opening_single_quotes,
                opening_double_quote,
            } => {
                for opening_single_quote in opening_single_quotes {
                    opening_single_quote.fmt(f)?;
                }
                opening_double_quote.fmt(f)
            }
            Self::ClosingText {
                closing_double_quote,
                closing_single_quotes,
            } => {
                closing_double_quote.fmt(f)?;
                for closing_single_quote in closing_single_quotes {
                    closing_single_quote.fmt(f)?;
                }
                Ok(())
            }
            Self::Text {
                opening,
                parts,
                closing,
            } => {
                opening.fmt(f)?;
                for line in parts {
                    line.fmt(f)?;
                }
                closing.fmt(f)
            }
            Self::TextNewline(newline) => newline.fmt(f),
            Self::TextPart(literal) => literal.fmt(f),
            Self::TextInterpolation {
                opening_curly_braces,
                expression,
                closing_curly_braces,
            } => {
                for opening_curly_brace in opening_curly_braces {
                    opening_curly_brace.fmt(f)?;
                }
                expression.fmt(f)?;
                for closing_curly_brace in closing_curly_braces {
                    closing_curly_brace.fmt(f)?;
                }
                Ok(())
            }
            Self::BinaryBar { left, bar, right } => {
                write!(f, "{}{}{}", left.kind, bar.kind, right.kind)
            }
            Self::Parenthesized {
                opening_parenthesis,
                inner,
                closing_parenthesis,
            } => write!(
                f,
                "{}{}{}",
                opening_parenthesis.kind, inner.kind, closing_parenthesis.kind,
            ),
            Self::Call {
                receiver,
                arguments,
            } => {
                receiver.fmt(f)?;
                for argument in arguments {
                    argument.fmt(f)?;
                }
                Ok(())
            }
            Self::List {
                opening_parenthesis,
                items,
                closing_parenthesis,
            } => {
                opening_parenthesis.fmt(f)?;
                for field in items {
                    field.fmt(f)?;
                }
                closing_parenthesis.fmt(f)
            }
            Self::ListItem { value, comma } => {
                value.fmt(f)?;
                if let Some(comma) = comma {
                    comma.fmt(f)?;
                }
                Ok(())
            }
            Self::Struct {
                opening_bracket,
                fields,
                closing_bracket,
            } => {
                opening_bracket.fmt(f)?;
                for field in fields {
                    field.fmt(f)?;
                }
                closing_bracket.fmt(f)
            }
            Self::StructField {
                key_and_colon,
                value,
                comma,
            } => {
                if let Some(box (key, colon)) = key_and_colon {
                    key.fmt(f)?;
                    colon.fmt(f)?;
                }
                value.fmt(f)?;
                if let Some(comma) = comma {
                    comma.fmt(f)?;
                }
                Ok(())
            }
            Self::StructAccess { struct_, dot, key } => {
                struct_.fmt(f)?;
                dot.fmt(f)?;
                key.fmt(f)
            }
            Self::Match {
                expression,
                percent,
                cases,
            } => {
                expression.fmt(f)?;
                percent.fmt(f)?;
                for case in cases {
                    case.fmt(f)?;
                }
                Ok(())
            }
            Self::MatchCase {
                pattern,
                condition,
                arrow,
                body,
            } => {
                pattern.fmt(f)?;
                arrow.fmt(f)?;
                if let Some(box (comma, condition)) = condition {
                    comma.fmt(f)?;
                    condition.fmt(f)?;
                }
                for expression in body {
                    expression.fmt(f)?;
                }
                Ok(())
            }
            Self::Function {
                opening_curly_brace,
                parameters_and_arrow,
                body,
                closing_curly_brace,
            } => {
                opening_curly_brace.fmt(f)?;
                if let Some((parameters, arrow)) = parameters_and_arrow {
                    for parameter in parameters {
                        parameter.fmt(f)?;
                    }
                    arrow.fmt(f)?;
                }
                for expression in body {
                    expression.fmt(f)?;
                }
                closing_curly_brace.fmt(f)
            }
            Self::Assignment {
                left,
                assignment_sign,
                body,
            } => {
                left.fmt(f)?;
                assignment_sign.fmt(f)?;
                for expression in body {
                    expression.fmt(f)?;
                }
                Ok(())
            }
            Self::Error {
                unparsable_input, ..
            } => unparsable_input.fmt(f),
        }
    }
}

impl<D> ToRichIr for CstKind<D>
where
    Cst<D>: ToRichIr,
{
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        match self {
            Self::EqualsSign => {
                builder.push_simple("EqualsSign");
            }
            Self::Comma => {
                builder.push_simple("Comma");
            }
            Self::Dot => {
                builder.push_simple("Dot");
            }
            Self::Colon => {
                builder.push_simple("Colon");
            }
            Self::ColonEqualsSign => {
                builder.push_simple("ColonEqualsSign");
            }
            Self::Bar => {
                builder.push_simple("Bar");
            }
            Self::OpeningParenthesis => {
                builder.push_simple("OpeningParenthesis");
            }
            Self::ClosingParenthesis => {
                builder.push_simple("ClosingParenthesis");
            }
            Self::OpeningBracket => {
                builder.push_simple("OpeningBracket");
            }
            Self::ClosingBracket => {
                builder.push_simple("ClosingBracket");
            }
            Self::OpeningCurlyBrace => {
                builder.push_simple("OpeningCurlyBrace");
            }
            Self::ClosingCurlyBrace => {
                builder.push_simple("ClosingCurlyBrace");
            }
            Self::Arrow => {
                builder.push_simple("Arrow");
            }
            Self::SingleQuote => {
                builder.push_simple("SingleQuote");
            }
            Self::DoubleQuote => {
                builder.push_simple("DoubleQuote");
            }
            Self::Percent => {
                builder.push_simple("Percent");
            }
            Self::Octothorpe => {
                builder.push_simple("Octothorpe");
            }
            Self::Whitespace(whitespace) => {
                builder.push_simple(format!("Whitespace \"{whitespace}\""));
            }
            Self::Newline(newline) => {
                builder.push_simple(format!(
                    "Newline \"{}\"",
                    newline.replace('\n', "\\n").replace('\r', "\\r")
                ));
            }
            Self::Comment {
                octothorpe,
                comment,
            } => {
                builder.push_cst_kind("Comment", |builder| {
                    builder.push_cst_property("octothorpe", octothorpe);
                    builder.push_cst_property("comment", format!("\"{comment}\""));
                });
            }
            Self::TrailingWhitespace { child, whitespace } => {
                builder.push_cst_kind("TrailingWhitespace", |builder| {
                    builder.push_cst_property("child", child);

                    builder.push_cst_property_name("whitespace");
                    builder.push_indented_foldable(|builder| {
                        for whitespace in whitespace {
                            builder.push_newline();
                            whitespace.build_rich_ir(builder);
                        }
                    });
                });
            }
            Self::Identifier(identifier) => {
                builder.push_simple(format!("Identifier \"{identifier}\""));
            }
            Self::Symbol(symbol) => {
                builder.push_simple(format!("Symbol \"{symbol}\""));
            }
            Self::Int {
                radix_prefix,
                value,
                string,
            } => {
                let start = builder.push_simple("Int:").start;
                builder.push_indented_foldable(|builder| {
                    builder.push_indented_optional_cst_property(
                        "radix_prefix",
                        radix_prefix.as_ref(),
                        |builder, (radix, prefix)| {
                            builder.push_cst_property("radix", format!("{radix:?}"));
                            builder.push_cst_property("prefix", format!("\"{prefix}\""));
                        },
                    );

                    builder.push_cst_property_name("value");
                    builder.push_simple(" ");
                    builder.push(value.to_string(), TokenType::Int, EnumSet::new());

                    builder.push_cst_property("string", format!("\"{string}\""));
                });
                builder
                    .push_reference(BigInt::from(value.clone()), start..builder.current_offset());
            }
            Self::OpeningText {
                opening_single_quotes,
                opening_double_quote,
            } => {
                builder.push_cst_kind("OpeningText", |builder| {
                    builder.push_cst_property_name("opening_single_quotes");
                    builder.push_indented_foldable(|builder| {
                        for opening_single_quote in opening_single_quotes {
                            builder.push_newline();
                            opening_single_quote.build_rich_ir(builder);
                        }
                    });

                    builder.push_cst_property("opening_double_quote", opening_double_quote);
                });
            }
            Self::ClosingText {
                closing_double_quote,
                closing_single_quotes,
            } => {
                builder.push_cst_kind("ClosingText", |builder| {
                    builder.push_cst_property("closing_double_quote", closing_double_quote);

                    builder.push_cst_property_name("closing_single_quotes");
                    builder.push_indented_foldable(|builder| {
                        for closing_single_quote in closing_single_quotes {
                            builder.push_newline();
                            closing_single_quote.build_rich_ir(builder);
                        }
                    });
                });
            }
            Self::Text {
                opening,
                parts,
                closing,
            } => {
                builder.push_cst_kind("Text", |builder| {
                    builder.push_cst_property("opening", opening);

                    builder.push_cst_property_name("parts");
                    builder.push_indented_foldable(|builder| {
                        for part in parts {
                            builder.push_newline();
                            part.build_rich_ir(builder);
                        }
                    });

                    builder.push_cst_property("closing", closing);
                });
            }
            Self::TextNewline(newline) => {
                builder.push_simple(format!(
                    "TextNewline \"{}\"",
                    newline.replace('\n', "\\n").replace('\r', "\\r")
                ));
            }
            Self::TextPart(literal) => {
                let start = builder.push_simple("TextPart \"").start;
                builder.push(literal, TokenType::Text, EnumSet::new());
                let end = builder.push_simple("\"").end;
                builder.push_reference(literal.to_string(), start..end);
            }
            Self::TextInterpolation {
                opening_curly_braces,
                expression,
                closing_curly_braces,
            } => {
                builder.push_cst_kind("TextInterpolation", |builder| {
                    builder.push_cst_property_name("opening_curly_braces");
                    builder.push_indented_foldable(|builder| {
                        for opening_curly_brace in opening_curly_braces {
                            builder.push_newline();
                            opening_curly_brace.build_rich_ir(builder);
                        }
                    });

                    builder.push_cst_property("expression", expression);

                    builder.push_cst_property_name("closing_curly_braces");
                    builder.push_indented_foldable(|builder| {
                        for closing_curly_brace in closing_curly_braces {
                            builder.push_newline();
                            closing_curly_brace.build_rich_ir(builder);
                        }
                    });
                });
            }
            Self::BinaryBar { left, bar, right } => {
                builder.push_cst_kind("BinaryBar", |builder| {
                    builder.push_cst_property("left", left);

                    builder.push_cst_property("bar", bar);

                    builder.push_cst_property("right", right);
                });
            }
            Self::Parenthesized {
                opening_parenthesis,
                inner,
                closing_parenthesis,
            } => {
                builder.push_cst_kind("Parenthesized", |builder| {
                    builder.push_cst_property("opening_parenthesis", opening_parenthesis);

                    builder.push_cst_property("inner", inner);

                    builder.push_cst_property("closing_parenthesis", closing_parenthesis);
                });
            }
            Self::Call {
                receiver,
                arguments,
            } => {
                builder.push_cst_kind("Call", |builder| {
                    builder.push_cst_property("receiver", receiver);

                    builder.push_cst_property_name("arguments");
                    builder.push_indented_foldable(|builder| {
                        for argument in arguments {
                            builder.push_newline();
                            argument.build_rich_ir(builder);
                        }
                    });
                });
            }
            Self::List {
                opening_parenthesis,
                items,
                closing_parenthesis,
            } => {
                builder.push_cst_kind("List", |builder| {
                    builder.push_cst_property("opening_parenthesis", opening_parenthesis);

                    builder.push_cst_property_name("items");
                    builder.push_indented_foldable(|builder| {
                        for item in items {
                            builder.push_newline();
                            item.build_rich_ir(builder);
                        }
                    });

                    builder.push_cst_property("closing_parenthesis", closing_parenthesis);
                });
            }
            Self::ListItem { value, comma } => {
                builder.push_cst_kind("ListItem", |builder| {
                    builder.push_cst_property("value", value);

                    builder.push_optional_cst_property("comma", comma.as_ref());
                });
            }
            Self::Struct {
                opening_bracket,
                fields,
                closing_bracket,
            } => {
                builder.push_cst_kind("Struct", |builder| {
                    builder.push_cst_property("opening_bracket", opening_bracket);

                    builder.push_cst_property_name("fields");
                    builder.push_indented_foldable(|builder| {
                        for field in fields {
                            builder.push_newline();
                            field.build_rich_ir(builder);
                        }
                    });

                    builder.push_cst_property("closing_bracket", closing_bracket);
                });
            }
            Self::StructField {
                key_and_colon,
                value,
                comma,
            } => {
                builder.push_cst_kind("StructField", |builder| {
                    builder.push_indented_optional_cst_property(
                        "key_and_colon",
                        key_and_colon.as_ref(),
                        |builder, box (key, colon)| {
                            builder.push_cst_property("key", key);
                            builder.push_cst_property("colon", colon);
                        },
                    );

                    builder.push_cst_property("value", value);

                    builder.push_optional_cst_property("comma", comma.as_ref());
                });
            }
            Self::StructAccess { struct_, dot, key } => {
                builder.push_cst_kind("StructAccess", |builder| {
                    builder.push_cst_property("struct", struct_);
                    builder.push_cst_property("dot", dot);
                    builder.push_cst_property("key", key);
                });
            }
            Self::Match {
                expression,
                percent,
                cases,
            } => {
                builder.push_cst_kind("Match", |builder| {
                    builder.push_cst_property("expression", expression);
                    builder.push_cst_property("percent", percent);

                    builder.push_cst_property_name("cases");
                    builder.push_indented_foldable(|builder| {
                        for case in cases {
                            builder.push_newline();
                            case.build_rich_ir(builder);
                        }
                    });
                });
            }
            Self::MatchCase {
                pattern,
                arrow,
                condition,
                body,
            } => {
                builder.push_cst_kind("MatchCase", |builder| {
                    builder.push_cst_property("pattern", pattern);

                    builder.push_indented_optional_cst_property(
                        "condition",
                        condition.as_ref(),
                        |builder, box (comma, condition)| {
                            builder.push_cst_property("comma", comma);
                            builder.push_cst_property("expression", condition);
                        },
                    );

                    builder.push_cst_property("arrow", arrow);

                    builder.push_cst_property_name("body");
                    builder.push_indented_foldable(|builder| {
                        for expression in body {
                            builder.push_newline();
                            expression.build_rich_ir(builder);
                        }
                    });
                });
            }
            Self::Function {
                opening_curly_brace,
                parameters_and_arrow,
                body,
                closing_curly_brace,
            } => {
                builder.push_cst_kind("Function", |builder| {
                    builder.push_cst_property("opening_curly_brace", opening_curly_brace);

                    builder.push_indented_optional_cst_property(
                        "parameters_and_arrow",
                        parameters_and_arrow.as_ref(),
                        |builder, (parameters, arrow)| {
                            builder.push_newline();
                            builder.push_cst_kind("parameters", |builder| {
                                if !parameters.is_empty() {
                                    builder.push_newline();
                                    builder.push_multiline(parameters);
                                }
                            });
                            builder.push_cst_property("arrow", arrow);
                        },
                    );

                    builder.push_cst_property_name("body");
                    builder.push_indented_foldable(|builder| {
                        for expression in body {
                            builder.push_newline();
                            expression.build_rich_ir(builder);
                        }
                    });

                    builder.push_cst_property("closing_curly_brace", closing_curly_brace);
                });
            }
            Self::Assignment {
                left,
                assignment_sign,
                body,
            } => {
                builder.push_cst_kind("Assignment", |builder| {
                    builder.push_cst_property("left", left);
                    builder.push_cst_property("assignment_sign", assignment_sign);

                    builder.push_cst_property_name("body");
                    builder.push_indented_foldable(|builder| {
                        for expression in body {
                            builder.push_newline();
                            expression.build_rich_ir(builder);
                        }
                    });
                });
            }
            Self::Error {
                unparsable_input,
                error,
            } => {
                builder.push_cst_kind("Error", |builder| {
                    builder
                        .push_cst_property("unparsable_input", format!("\"{unparsable_input}\""));
                    builder.push_cst_property("error", format!("{error:?}"));
                });
            }
        }
    }
}
impl RichIrBuilder {
    fn push_cst_kind(&mut self, kind: &str, build_children: impl FnOnce(&mut Self)) {
        self.push_simple(format!("{kind}:"));
        self.push_indented_foldable(build_children);
    }
    fn push_cst_property_name(&mut self, property_name: &str) {
        self.push_newline();
        self.push_simple(format!("{property_name}:"));
    }
    fn push_cst_property(&mut self, property_name: &str, value: impl ToRichIr) {
        self.push_cst_property_name(property_name);
        self.push_simple(" ");
        value.build_rich_ir(self);
    }
    fn push_none(&mut self) {
        self.push_simple("None");
    }

    fn push_optional_cst_property(&mut self, property_name: &str, value: Option<impl ToRichIr>) {
        self.push_cst_property_name(property_name);
        self.push_simple(" ");
        if let Some(value) = value {
            value.build_rich_ir(self);
        } else {
            self.push_none();
        }
    }

    fn push_indented_optional_cst_property<T>(
        &mut self,
        property_name: &str,
        value: Option<T>,
        build_children: impl FnOnce(&mut Self, T),
    ) {
        self.push_cst_property_name(property_name);
        if let Some(v) = value {
            self.push_indented_foldable(|builder| {
                build_children(builder, v);
            });
        } else {
            self.push_simple(" ");
            self.push_none();
        }
    }
}
