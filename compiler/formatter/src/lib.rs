#![feature(
    anonymous_lifetime_in_impl_trait,
    box_patterns,
    const_trait_impl,
    let_chains
)]
#![allow(clippy::all)]
// #![warn(clippy::nursery, clippy::pedantic, unused_crate_dependencies)]
// #![allow(
//     clippy::cognitive_complexity,
//     clippy::match_same_arms,
//     clippy::module_name_repetitions,
//     clippy::too_many_lines
// )]

use candy_frontend::{cst::Cst, position::Offset};
use existing_whitespace::{TrailingWithIndentationConfig, WhitespacePositionInBody};
use extension_trait::extension_trait;
use format::{format_csts, FormattingInfo};
use itertools::Itertools;
use text_edits::TextEdits;
use width::{Indentation, Width};

mod existing_parentheses;
mod existing_whitespace;
mod format;
mod format_collection;
mod formatted_cst;
mod text_edits;
mod width;

#[extension_trait]
pub impl<C: AsRef<[Cst]>> Formatter for C {
    fn format_to_string(&self) -> String {
        self.format_to_edits().apply()
    }
    fn format_to_edits(&self) -> TextEdits {
        let csts = self.as_ref();
        // TOOD: Is there an elegant way to avoid stringifying the whole CST?
        let source = csts.iter().join("");
        let mut edits = TextEdits::new(source);

        let formatted = format_csts(
            &mut edits,
            Width::default(),
            csts,
            Offset::default(),
            &FormattingInfo::default(),
        );
        if formatted.child_width() == Width::default() && !formatted.whitespace.has_comments() {
            _ = formatted.into_empty_trailing(&mut edits);
        } else {
            let config = TrailingWithIndentationConfig::Body {
                position: if formatted.child_width() == Width::default() {
                    WhitespacePositionInBody::Start
                } else {
                    WhitespacePositionInBody::End
                },
                indentation: Indentation::default(),
            };
            _ = formatted.into_trailing_with_indentation_detailed(&mut edits, &config);
        };

        edits
    }
}
