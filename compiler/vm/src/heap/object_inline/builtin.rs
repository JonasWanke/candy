use super::InlineObjectTrait;
use crate::{
    heap::{object_heap::HeapObject, Heap, InlineObject},
    utils::{impl_debug_display_via_debugdisplay, DebugDisplay},
};
use candy_frontend::builtin_functions::{self, BuiltinFunction};
use derive_more::Deref;
use rustc_hash::FxHashMap;
use std::{
    fmt::{self, Formatter},
    hash::{Hash, Hasher},
    num::NonZeroU64,
};

#[derive(Clone, Copy, Deref)]
pub struct InlineBuiltin<'h>(InlineObject<'h>);

impl<'h> InlineBuiltin<'h> {
    const INDEX_SHIFT: usize = 2;

    pub fn new_unchecked(object: InlineObject<'h>) -> Self {
        Self(object)
    }

    fn index(self) -> usize {
        (self.raw_word().get() >> Self::INDEX_SHIFT) as usize
    }
    pub fn get(self) -> BuiltinFunction {
        builtin_functions::VALUES[self.index()]
    }
}

impl DebugDisplay for InlineBuiltin<'_> {
    fn fmt(&self, f: &mut Formatter, _is_debug: bool) -> fmt::Result {
        write!(f, "builtin{:?}", self.get())
    }
}
impl_debug_display_via_debugdisplay!(InlineBuiltin<'_>);

impl Eq for InlineBuiltin<'_> {}
impl PartialEq for InlineBuiltin<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.index() == other.index()
    }
}
impl Hash for InlineBuiltin<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index().hash(state)
    }
}

impl From<BuiltinFunction> for InlineObject<'_> {
    fn from(builtin_function: BuiltinFunction) -> Self {
        *InlineBuiltin::from(builtin_function)
    }
}
impl From<BuiltinFunction> for InlineBuiltin<'_> {
    fn from(builtin_function: BuiltinFunction) -> Self {
        let index = builtin_function as usize;
        debug_assert_eq!(
            (index << Self::INDEX_SHIFT) >> Self::INDEX_SHIFT,
            index,
            "Builtin function index is too large.",
        );
        let header_word = InlineObject::KIND_BUILTIN | ((index as u64) << Self::INDEX_SHIFT);
        let header_word = unsafe { NonZeroU64::new_unchecked(header_word) };
        Self(InlineObject::new(header_word))
    }
}

impl<'h> InlineObjectTrait<'h> for InlineBuiltin<'h> {
    type Clone<'t> = InlineBuiltin<'t>;

    fn clone_to_heap_with_mapping<'t>(
        self,
        _heap: &mut Heap<'t>,
        _address_map: &mut FxHashMap<HeapObject<'h>, HeapObject<'t>>,
    ) -> Self::Clone<'t> {
        InlineBuiltin(InlineObject::new(self.raw_word()))
    }
}
