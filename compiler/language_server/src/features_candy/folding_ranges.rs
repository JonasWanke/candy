use std::ops::Range;

use candy_frontend::{
    cst::{Cst, CstKind, UnwrapWhitespaceAndComment},
    module::{Module, ModuleDb},
    position::{Offset, PositionConversionDb},
    rcst_to_cst::RcstToCst,
};
use lsp_types::{FoldingRange, FoldingRangeKind};

use crate::utils::LspPositionConversion;

pub fn folding_ranges<DB: ModuleDb + PositionConversionDb + RcstToCst>(
    db: &DB,
    module: Module,
) -> Vec<FoldingRange> {
    let mut context = Context::new(db, module.clone());
    let cst = db.cst(module).unwrap();
    context.visit_csts(&cst);
    context.ranges
}

struct Context<'a, DB: ModuleDb + PositionConversionDb + ?Sized> {
    db: &'a DB,
    module: Module,
    ranges: Vec<FoldingRange>,
}
impl<'a, DB> Context<'a, DB>
where
    DB: ModuleDb + PositionConversionDb + ?Sized,
{
    fn new(db: &'a DB, module: Module) -> Self {
        Context {
            db,
            module,
            ranges: vec![],
        }
    }

    fn visit_csts(&mut self, csts: &[Cst]) {
        for cst in csts {
            self.visit_cst(cst);
        }
    }
    fn visit_cst(&mut self, cst: &Cst) {
        match &cst.kind {
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
            | CstKind::Newline(_) => {}
            // TODO: support folding ranges for comments
            CstKind::Comment { .. } => {}
            CstKind::TrailingWhitespace { child, .. } => self.visit_cst(child),
            CstKind::Identifier(_) | CstKind::Symbol(_) | CstKind::Int { .. } => {}
            // TODO: support folding ranges for multiline texts
            CstKind::OpeningText { .. }
            | CstKind::ClosingText { .. }
            | CstKind::Text { .. }
            | CstKind::TextPart(_)
            | CstKind::TextInterpolation { .. } => {}
            CstKind::BinaryBar { left, bar, right } => {
                self.visit_cst(left);
                self.visit_cst(bar);
                self.visit_cst(right);
            }
            CstKind::Parenthesized { inner, .. } => self.visit_cst(inner),
            CstKind::Call {
                receiver,
                arguments,
            } => {
                if !arguments.is_empty() {
                    let receiver = receiver.unwrap_whitespace_and_comment();
                    let last_argument = arguments.last().unwrap().unwrap_whitespace_and_comment();
                    self.push(
                        receiver.span.end..last_argument.span.end,
                        FoldingRangeKind::Region,
                    );
                }

                self.visit_cst(receiver);
                self.visit_csts(arguments);
            }
            // TODO: support folding ranges for lists
            CstKind::List { items, .. } => self.visit_csts(items),
            CstKind::ListItem { value, .. } => self.visit_cst(value),
            // TODO: support folding ranges for structs
            CstKind::Struct { fields, .. } => self.visit_csts(fields),
            CstKind::StructField {
                key_and_colon,
                value,
                ..
            } => {
                if let Some(box (key, _)) = key_and_colon {
                    self.visit_cst(key);
                }
                self.visit_cst(value);
            }
            CstKind::StructAccess { struct_, dot, key } => {
                self.visit_cst(struct_);
                self.visit_cst(dot);
                self.visit_cst(key);
            }
            CstKind::Match {
                expression,
                percent,
                cases,
            } => {
                self.visit_cst(expression);

                let percent = percent.unwrap_whitespace_and_comment();
                let cases_end = cases
                    .unwrap_whitespace_and_comment()
                    .last()
                    .unwrap()
                    .span
                    .end;
                self.push(percent.span.end..cases_end, FoldingRangeKind::Region);

                self.visit_csts(cases);
            }
            CstKind::MatchCase {
                pattern,
                arrow,
                body,
            } => {
                self.visit_cst(pattern);

                let arrow = arrow.unwrap_whitespace_and_comment();
                let body_end = body
                    .unwrap_whitespace_and_comment()
                    .last()
                    .unwrap()
                    .span
                    .end;
                self.push(arrow.span.end..body_end, FoldingRangeKind::Region);

                self.visit_csts(body);
            }
            CstKind::Lambda {
                opening_curly_brace,
                parameters_and_arrow,
                body,
                closing_curly_brace,
            } => {
                let opening_curly_brace = opening_curly_brace.unwrap_whitespace_and_comment();
                assert!(matches!(
                    opening_curly_brace.kind,
                    CstKind::OpeningCurlyBrace { .. }
                ));

                let closing_curly_brace = closing_curly_brace.unwrap_whitespace_and_comment();

                self.push(
                    opening_curly_brace.span.end..closing_curly_brace.span.start,
                    FoldingRangeKind::Region,
                );
                if let Some((parameters, _)) = parameters_and_arrow {
                    self.visit_csts(parameters);
                }
                self.visit_csts(body);
            }
            CstKind::Assignment {
                name_or_pattern,
                assignment_sign,
                body,
            } => {
                if !body.is_empty() {
                    let assignment_sign = assignment_sign.unwrap_whitespace_and_comment();
                    let last_expression = body.last().unwrap().unwrap_whitespace_and_comment();

                    self.push(
                        assignment_sign.span.end..last_expression.span.end,
                        FoldingRangeKind::Region,
                    );
                }

                self.visit_cst(name_or_pattern);
                self.visit_csts(body);
            }
            CstKind::Error { .. } => {}
        }
    }

    fn push(&mut self, range: Range<Offset>, kind: FoldingRangeKind) {
        let range = self.db.range_to_lsp_range(self.module.clone(), range);
        self.ranges.push(FoldingRange {
            start_line: range.start.line,
            start_character: Some(range.start.character),
            end_line: range.end.line,
            end_character: Some(range.end.character),
            kind: Some(kind),
        });
    }
}
