use crate::{
    id::CountableId,
    rich_ir::{RichIrBuilder, ToRichIr, TokenType},
};
use enumset::EnumSet;
use std::fmt::{self, Debug, Formatter};

#[derive(Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct Id(usize);
impl Id {
    pub fn to_short_debug_string(&self) -> String {
        format!("${}", self.0)
    }
}

impl CountableId for Id {
    fn from_usize(id: usize) -> Self {
        Self(id)
    }
    fn to_usize(&self) -> usize {
        self.0
    }
}

impl ToString for Id {
    fn to_string(&self) -> String {
        self.to_short_debug_string()
    }
}
impl Debug for Id {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_short_debug_string())
    }
}
impl ToRichIr for Id {
    fn build_rich_ir(&self, builder: &mut RichIrBuilder) {
        let range = builder.push(
            self.to_short_debug_string(),
            TokenType::Variable,
            EnumSet::empty(),
        );
        builder.push_reference(*self, range);
    }
}
