use super::{
    cst::{Cst, CstKind},
    rcst::Rcst,
    string_to_rcst::{ModuleError, StringToRcst},
};
use crate::{
    cst::{CstData, Id},
    id::IdGenerator,
    module::Module,
    position::Offset,
};
use extension_trait::extension_trait;
use std::sync::Arc;

#[salsa::query_group(RcstToCstStorage)]
pub trait RcstToCst: StringToRcst {
    fn cst(&self, module: Module) -> Result<Arc<Vec<Cst>>, ModuleError>;
}

pub type CstResult = Result<Arc<Vec<Cst>>, ModuleError>;

fn cst(db: &dyn RcstToCst, module: Module) -> Result<Arc<Vec<Cst>>, ModuleError> {
    let rcsts = db.rcst(module)?;
    Ok(Arc::new(rcsts.to_csts()))
}

#[derive(Default)]
struct State {
    offset: Offset,
    id_generator: IdGenerator<Id>,
}

impl Rcst {
    fn to_cst(&self, state: &mut State) -> Cst {
        let id = state.id_generator.generate();
        let start_offset = state.offset;
        let kind = self.to_cst_kind(state);
        let end_offset = state.offset;
        Cst {
            data: CstData {
                id,
                span: start_offset..end_offset,
            },
            kind,
        }
    }
    fn to_cst_kind(&self, state: &mut State) -> CstKind<CstData> {
        match &self.kind {
            CstKind::EqualsSign => {
                *state.offset += 1;
                CstKind::EqualsSign
            }
            CstKind::Comma => {
                *state.offset += 1;
                CstKind::Comma
            }
            CstKind::Dot => {
                *state.offset += 1;
                CstKind::Dot
            }
            CstKind::Colon => {
                *state.offset += 1;
                CstKind::Colon
            }
            CstKind::ColonEqualsSign => {
                *state.offset += 2;
                CstKind::ColonEqualsSign
            }
            CstKind::Bar => {
                *state.offset += 1;
                CstKind::Bar
            }
            CstKind::OpeningParenthesis => {
                *state.offset += 1;
                CstKind::OpeningParenthesis
            }
            CstKind::ClosingParenthesis => {
                *state.offset += 1;
                CstKind::ClosingParenthesis
            }
            CstKind::OpeningBracket => {
                *state.offset += 1;
                CstKind::OpeningBracket
            }
            CstKind::ClosingBracket => {
                *state.offset += 1;
                CstKind::ClosingBracket
            }
            CstKind::OpeningCurlyBrace => {
                *state.offset += 1;
                CstKind::OpeningCurlyBrace
            }
            CstKind::ClosingCurlyBrace => {
                *state.offset += 1;
                CstKind::ClosingCurlyBrace
            }
            CstKind::Arrow => {
                *state.offset += 2;
                CstKind::Arrow
            }
            CstKind::SingleQuote => {
                *state.offset += 1;
                CstKind::SingleQuote
            }
            CstKind::DoubleQuote => {
                *state.offset += 1;
                CstKind::DoubleQuote
            }
            CstKind::Percent => {
                *state.offset += 1;
                CstKind::Percent
            }
            CstKind::Octothorpe => {
                *state.offset += 1;
                CstKind::Octothorpe
            }
            CstKind::Whitespace(whitespace) => {
                *state.offset += whitespace.len();
                CstKind::Whitespace(whitespace.clone())
            }
            CstKind::Newline(newline) => {
                *state.offset += newline.len();
                CstKind::Newline(newline.clone())
            }
            CstKind::Comment {
                octothorpe,
                comment,
            } => {
                let octothorpe = octothorpe.to_cst(state);
                *state.offset += comment.len();
                CstKind::Comment {
                    octothorpe: Box::new(octothorpe),
                    comment: comment.clone(),
                }
            }
            CstKind::TrailingWhitespace { child, whitespace } => CstKind::TrailingWhitespace {
                child: Box::new(child.to_cst(state)),
                whitespace: whitespace.to_csts_helper(state),
            },
            CstKind::Identifier(identifier) => {
                *state.offset += identifier.len();
                CstKind::Identifier(identifier.clone())
            }
            CstKind::Symbol(symbol) => {
                *state.offset += symbol.len();
                CstKind::Symbol(symbol.clone())
            }
            CstKind::Int {
                radix_prefix,
                value,
                string,
            } => {
                *state.offset += radix_prefix
                    .as_ref()
                    .map(|(_, radix_string)| radix_string.len())
                    .unwrap_or_default();
                *state.offset += string.len();
                CstKind::Int {
                    radix_prefix: radix_prefix.clone(),
                    value: value.clone(),
                    string: string.clone(),
                }
            }
            CstKind::OpeningText {
                opening_single_quotes,
                opening_double_quote,
            } => CstKind::OpeningText {
                opening_single_quotes: opening_single_quotes.to_csts_helper(state),
                opening_double_quote: Box::new(opening_double_quote.to_cst(state)),
            },
            CstKind::ClosingText {
                closing_double_quote,
                closing_single_quotes,
            } => CstKind::ClosingText {
                closing_double_quote: Box::new(closing_double_quote.to_cst(state)),
                closing_single_quotes: closing_single_quotes.to_csts_helper(state),
            },
            CstKind::Text {
                opening,
                parts,
                closing,
            } => CstKind::Text {
                opening: Box::new(opening.to_cst(state)),
                parts: parts.to_csts_helper(state),
                closing: Box::new(closing.to_cst(state)),
            },
            CstKind::TextNewline(newline) => {
                *state.offset += newline.len();
                CstKind::TextNewline(newline.clone())
            }
            CstKind::TextPart(text) => {
                *state.offset += text.len();
                CstKind::TextPart(text.clone())
            }
            CstKind::TextInterpolation {
                opening_curly_braces,
                expression,
                closing_curly_braces,
            } => CstKind::TextInterpolation {
                opening_curly_braces: opening_curly_braces.to_csts_helper(state),
                expression: Box::new(expression.to_cst(state)),
                closing_curly_braces: closing_curly_braces.to_csts_helper(state),
            },
            CstKind::Call {
                receiver,
                arguments,
            } => CstKind::Call {
                receiver: Box::new(receiver.to_cst(state)),
                arguments: arguments.to_csts_helper(state),
            },
            CstKind::List {
                opening_parenthesis,
                items,
                closing_parenthesis,
            } => CstKind::List {
                opening_parenthesis: Box::new(opening_parenthesis.to_cst(state)),
                items: items.to_csts_helper(state),
                closing_parenthesis: Box::new(closing_parenthesis.to_cst(state)),
            },
            CstKind::ListItem { value, comma } => CstKind::ListItem {
                value: Box::new(value.to_cst(state)),
                comma: comma.as_ref().map(|comma| Box::new(comma.to_cst(state))),
            },
            CstKind::Struct {
                opening_bracket,
                fields,
                closing_bracket,
            } => CstKind::Struct {
                opening_bracket: Box::new(opening_bracket.to_cst(state)),
                fields: fields.to_csts_helper(state),
                closing_bracket: Box::new(closing_bracket.to_cst(state)),
            },
            CstKind::StructField {
                key_and_colon,
                value,
                comma,
            } => CstKind::StructField {
                key_and_colon: key_and_colon
                    .as_deref()
                    .map(|(key, colon)| Box::new((key.to_cst(state), colon.to_cst(state)))),
                value: Box::new(value.to_cst(state)),
                comma: comma.as_ref().map(|comma| Box::new(comma.to_cst(state))),
            },
            CstKind::StructAccess { struct_, dot, key } => CstKind::StructAccess {
                struct_: Box::new(struct_.to_cst(state)),
                dot: Box::new(dot.to_cst(state)),
                key: Box::new(key.to_cst(state)),
            },
            CstKind::Match {
                expression,
                percent,
                cases,
            } => CstKind::Match {
                expression: Box::new(expression.to_cst(state)),
                percent: Box::new(percent.to_cst(state)),
                cases: cases.to_csts_helper(state),
            },
            CstKind::MatchCase {
                pattern,
                condition,
                arrow,
                body,
            } => CstKind::MatchCase {
                pattern: Box::new(pattern.to_cst(state)),
                condition: condition.as_ref().map(|v| Box::new(v.to_cst(state))),
                arrow: Box::new(arrow.to_cst(state)),
                body: body.to_csts_helper(state),
            },
            CstKind::BinaryBar { left, bar, right } => CstKind::BinaryBar {
                left: Box::new(left.to_cst(state)),
                bar: Box::new(bar.to_cst(state)),
                right: Box::new(right.to_cst(state)),
            },
            CstKind::Parenthesized {
                opening_parenthesis,
                inner,
                closing_parenthesis,
            } => CstKind::Parenthesized {
                opening_parenthesis: Box::new(opening_parenthesis.to_cst(state)),
                inner: Box::new(inner.to_cst(state)),
                closing_parenthesis: Box::new(closing_parenthesis.to_cst(state)),
            },
            CstKind::Function {
                opening_curly_brace,
                parameters_and_arrow,
                body,
                closing_curly_brace,
            } => CstKind::Function {
                opening_curly_brace: Box::new(opening_curly_brace.to_cst(state)),
                parameters_and_arrow: parameters_and_arrow.as_ref().map(|(parameters, arrow)| {
                    (
                        parameters.to_csts_helper(state),
                        Box::new(arrow.to_cst(state)),
                    )
                }),
                body: body.to_csts_helper(state),
                closing_curly_brace: Box::new(closing_curly_brace.to_cst(state)),
            },
            CstKind::Assignment {
                left,
                assignment_sign,
                body,
            } => CstKind::Assignment {
                left: Box::new(left.to_cst(state)),
                assignment_sign: Box::new(assignment_sign.to_cst(state)),
                body: body.to_csts_helper(state),
            },
            CstKind::Error {
                unparsable_input,
                error,
            } => {
                *state.offset += unparsable_input.len();
                CstKind::Error {
                    unparsable_input: unparsable_input.clone(),
                    error: *error,
                }
            }
        }
    }
}

#[extension_trait]
pub impl RcstsToCstsExt for Vec<Rcst> {
    fn to_csts(&self) -> Vec<Cst> {
        let mut state = State::default();
        self.to_csts_helper(&mut state)
    }
}
#[extension_trait]
impl RcstsToCstsHelperExt for Vec<Rcst> {
    fn to_csts_helper(&self, state: &mut State) -> Vec<Cst> {
        let mut csts = vec![];
        for rcst in self {
            csts.push(rcst.to_cst(state));
        }
        csts
    }
}

#[extension_trait]
impl ConvertToCst for (Rcst, Rcst) {
    fn to_cst(&self, state: &mut State) -> (Cst, Cst) {
        (self.0.to_cst(state), self.1.to_cst(state))
    }
}
