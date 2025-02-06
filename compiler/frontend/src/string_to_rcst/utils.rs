use crate::{
    cst::CstKind,
    rcst::Rcst,
    rich_ir::{RichIrBuilder, ToRichIr},
};

pub static MEANINGFUL_PUNCTUATION: &str = r#"=,.:|()[]{}->'"%#"#;
pub static SUPPORTED_WHITESPACE: &str = " \r\n\t";

impl CstKind<()> {
    #[must_use]
    pub fn wrap_in_whitespace(self, whitespace: Vec<Rcst>) -> Rcst {
        Rcst::from(self).wrap_in_whitespace(whitespace)
    }
}
impl Rcst {
    #[must_use]
    pub fn wrap_in_whitespace(mut self, mut whitespace: Vec<Self>) -> Self {
        if whitespace.is_empty() {
            return self;
        }

        if let CstKind::TrailingWhitespace {
            whitespace: self_whitespace,
            ..
        } = &mut self.kind
        {
            self_whitespace.append(&mut whitespace);
            self
        } else {
            CstKind::TrailingWhitespace {
                child: Box::new(self),
                whitespace,
            }
            .into()
        }
    }
}

pub fn whitespace_indentation_score(whitespace: &str) -> usize {
    whitespace
        .chars()
        .map(|c| match c {
            '\t' => 2,
            c if c.is_whitespace() => 1,
            _ => panic!("whitespace_indentation_score called with something non-whitespace"),
        })
        .sum()
}

pub fn parse_multiple<F>(
    mut input: &str,
    parse_single: F,
    count: Option<(usize, bool)>,
) -> Option<(&str, Vec<Rcst>)>
where
    F: Fn(&str) -> Option<(&str, Rcst)>,
{
    let mut rcsts = vec![];
    while let Some((input_after_single, rcst)) = parse_single(input)
        && count.map_or(true, |(count, exact)| exact || rcsts.len() < count)
    {
        input = input_after_single;
        rcsts.push(rcst);
    }
    match count {
        Some((count, _)) if count != rcsts.len() => None,
        _ => Some((input, rcsts)),
    }
}

impl<T: ToRichIr> ToRichIr for (&str, T) {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push_simple(format!("Remaining input: \"{}\"\nParsed: ", self.0));
        self.1.build_rich_ir(builder);
    }
}
impl ToRichIr for Vec<Rcst> {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        builder.push_multiline(self);
    }
}
impl<T: ToRichIr> ToRichIr for Option<(&str, T)> {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        if let Some(value) = self {
            value.build_rich_ir(builder);
        } else {
            builder.push_simple("Nothing was parsed");
        }
    }
}

#[cfg(test)]
macro_rules!  assert_rich_ir_snapshot {
    ($value:expr, @$string:literal) => {
        insta::_assert_snapshot_base!(
            transform=|it| $crate::rich_ir::ToRichIr::to_rich_ir(it, false).text,
            $value,
            @$string
        );
    };
}
#[cfg(test)]
pub(super) use assert_rich_ir_snapshot;
