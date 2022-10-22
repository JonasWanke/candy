// mod constant_folding;
// mod inlining;
mod constant_folding;
mod follow_references;
mod inlining;
mod tree_shaking;
mod utils;

use super::hir::Body;
use tracing::{debug, info, warn};

impl Body {
    pub fn optimize(&mut self) {
        warn!("HIR: {self}");
        warn!("Following references");
        self.follow_references();
        warn!("HIR: {self}");
        warn!("Tree shaking");
        warn!("Complexity: {}", self.complexity());
        self.tree_shake();
        warn!("HIR: {self}");
        warn!("Folding constants");
        self.fold_constants();
        warn!("HIR: {self}");
            self.inline_functions_containing_use();
    }
}
