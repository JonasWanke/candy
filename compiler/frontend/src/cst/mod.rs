use self::tree_with_ids::TreeWithIds;
pub use self::{
    error::CstError, id::Id, is_multiline::IsMultiline, kind::CstKind,
    unwrap_whitespace_and_comment::UnwrapWhitespaceAndComment,
};
use crate::{module::Module, position::Offset, rcst_to_cst::RcstToCst};
use derive_more::Deref;
use std::{
    fmt::{self, Display, Formatter},
    ops::Range,
};

mod error;
mod id;
mod is_multiline;
mod kind;
mod tree_with_ids;
mod unwrap_whitespace_and_comment;

#[derive(Clone, Debug, Deref, Eq, Hash, PartialEq)]
pub struct Cst<D = CstData> {
    pub data: D,
    #[deref]
    pub kind: CstKind<D>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CstData {
    pub id: Id,
    pub span: Range<Offset>,
}

impl Cst {
    /// Returns a span that makes sense to display in the editor.
    ///
    /// For example, if a call contains errors, we want to only underline the
    /// name of the called function itself, not everything including arguments.
    pub fn display_span(&self) -> Range<Offset> {
        match &self.kind {
            CstKind::TrailingWhitespace { child, .. } => child.display_span(),
            CstKind::Call { receiver, .. } => receiver.display_span(),
            CstKind::Assignment { left, .. } => left.display_span(),
            _ => self.data.span.clone(),
        }
    }
}
impl Display for Cst {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        self.kind.fmt(f)
    }
}

#[salsa::query_group(CstDbStorage)]
pub trait CstDb: RcstToCst {
    fn find_cst(&self, module: Module, id: Id) -> Cst;
    fn find_cst_by_offset(&self, module: Module, offset: Offset) -> Cst;
}

fn find_cst(db: &dyn CstDb, module: Module, id: Id) -> Cst {
    db.cst(module).unwrap().find(id).unwrap().clone()
}
fn find_cst_by_offset(db: &dyn CstDb, module: Module, offset: Offset) -> Cst {
    db.cst(module)
        .unwrap()
        .find_by_offset(offset)
        .unwrap()
        .clone()
}
