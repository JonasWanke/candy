//! Optimizations are a necessity for Candy code to run reasonably fast. For
//! example, without optimizations, if two modules import a third module using
//! `use "..foo"`, then the `foo` module is instantiated twice completely
//! separately. Because this module can in turn depend on other modules, this
//! approach would lead to exponential code blowup.
//!
//! When optimizing code in general, there are two main objectives:
//!
//! - Making the code fast.
//! - Making the code small.
//!
//! Some optimizations benefit both of those objectives. For example, removing
//! unused code from the program makes it smaller, but also means there's less
//! code to be executed. Other optimizations further one objective, but harm the
//! other. For example, inlining functions (basically copying their code to
//! where they're used), can make the code bigger, but faster because there are
//! less function calls to be performed.
//!
//! Depending on the use case, the tradeoff between both objectives changes. To
//! put you in the right mindset, here are just two use cases:
//!
//! - Programming for a microcontroller with 1 MB of ROM available for the
//!   program. In this case, you want your code to be as fast as possible while
//!   still fitting in 1 MB. Interestingly, the importance of code size is a
//!   step function: There's no benefit in only using 0.5 MB, but 1.1 MB makes
//!   the program completely unusable.
//!
//! - Programming for a WASM module to be downloaded. In this case, you might
//!   have some concrete measurements on how performance and download size
//!   affect user retention.
//!
//! It should be noted that we can't judge performance statically. Although some
//! optimizations such as inlining typically improve performance, there are rare
//! cases where they don't. For example, inlining a function that's used in
//! multiple places means the CPU's branch predictor can't benefit from the
//! knowledge gained by previous function executions. Inlining might also make
//! your program bigger, causing more cache misses. Thankfully, Candy is not yet
//! optimized enough for us to care about such details.
//!
//! This module contains several optimizations. All of them operate on the MIR.
//! Some are called "obvious". Those are optimizations that typically improve
//! both performance and code size. Whenever they can be applied, they should be
//! applied.

mod common_subtree_elimination;
mod complexity;
mod constant_folding;
mod constant_lifting;
mod inlining;
mod module_folding;
mod multiple_flattening;
mod reference_following;
mod tree_shaking;
mod utils;

use super::mir::Mir;
use crate::{database::Database, module::Module};
use tracing::debug;

impl Mir {
    pub fn optimize(&mut self, db: &Database) {
        debug!("MIR: {self:?}");
        debug!("Complexity: {}", self.complexity());
        self.optimize_obvious(db, &[]);
        debug!("Done optimizing.");
        debug!("MIR: {self:?}");
        debug!("Complexity: {}", self.complexity());
    }

    /// Performs optimizations that improve both performance and code size.
    pub fn optimize_obvious(&mut self, db: &Database, import_chain: &[Module]) {
        self.optimize_obvious_self_contained();
        self.fold_modules(db, import_chain);
        self.optimize_obvious_self_contained();
    }

    /// Performs optimizations that improve both performance and code size and
    /// that work without looking at other modules.
    pub fn optimize_obvious_self_contained(&mut self) {
        loop {
            let before = self.clone();

            // debug!("Following references");
            self.checked_optimization(|mir| mir.follow_references());
            // debug!("Tree shake");
            self.checked_optimization(|mir| mir.tree_shake());
            // debug!("Fold constants");
            self.checked_optimization(|mir| mir.fold_constants());
            // debug!("Inline functions containing use");
            self.checked_optimization(|mir| mir.inline_functions_containing_use());
            // debug!("Lift constants");
            self.checked_optimization(|mir| mir.lift_constants());
            // debug!("Eliminate common subtrees");
            // self.checked_optimization(|mir| mir.eliminate_common_subtrees());
            // debug!("Flatten multiple");
            self.checked_optimization(|mir| mir.flatten_multiples());

            if *self == before {
                return;
            }
        }
    }

    fn checked_optimization(&mut self, optimization: fn(&mut Mir) -> ()) {
        optimization(self);
        self.validate();
    }
}
