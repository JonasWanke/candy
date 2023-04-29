use super::{utils::heap_object_impls, HeapObjectTrait};
use crate::{
    fiber::InstructionPointer,
    heap::{object_heap::HeapObject, Heap, InlineObject},
    utils::{impl_debug_display_via_debugdisplay, DebugDisplay},
};
use derive_more::Deref;
use itertools::Itertools;
use rustc_hash::FxHashMap;
use std::{
    fmt::{self, Formatter},
    hash::{Hash, Hasher},
    ptr::{self, NonNull},
    slice,
};

#[derive(Clone, Copy, Deref)]
pub struct HeapClosure(HeapObject);

impl HeapClosure {
    const CAPTURED_LEN_SHIFT: usize = 32;
    const ARGUMENT_COUNT_SHIFT: usize = 3;

    pub fn new_unchecked(object: HeapObject) -> Self {
        Self(object)
    }
    pub fn create(
        heap: &mut Heap,
        captured: &[InlineObject],
        argument_count: usize,
        body: InstructionPointer,
    ) -> Self {
        let captured_len = captured.len();
        assert_eq!(
            (captured_len << Self::CAPTURED_LEN_SHIFT) >> Self::CAPTURED_LEN_SHIFT,
            captured_len,
            "Closure captures too many things.",
        );

        let argument_count_shift_for_max_size =
            Self::CAPTURED_LEN_SHIFT + Self::ARGUMENT_COUNT_SHIFT;
        assert_eq!(
            (argument_count << argument_count_shift_for_max_size)
                >> argument_count_shift_for_max_size,
            argument_count,
            "Closure accepts too many arguments.",
        );

        let closure = Self(heap.allocate(
            HeapObject::KIND_CLOSURE
                | ((captured_len as u64) << Self::CAPTURED_LEN_SHIFT)
                | ((argument_count as u64) << Self::ARGUMENT_COUNT_SHIFT),
            (1 + captured_len) * HeapObject::WORD_SIZE,
        ));
        unsafe {
            *closure.body_pointer().as_mut() = *body as u64;
            ptr::copy_nonoverlapping(
                captured.as_ptr(),
                closure.captured_pointer().as_ptr(),
                captured_len,
            );
        }

        closure
    }

    pub fn captured_len(self) -> usize {
        (self.header_word() >> Self::CAPTURED_LEN_SHIFT) as usize
    }
    fn captured_pointer(self) -> NonNull<InlineObject> {
        self.content_word_pointer(1).cast()
    }
    pub fn captured<'a>(self) -> &'a [InlineObject] {
        unsafe { slice::from_raw_parts(self.captured_pointer().as_ptr(), self.captured_len()) }
    }

    pub fn argument_count(self) -> usize {
        ((self.header_word() & 0xFFFF_FFFF) >> Self::ARGUMENT_COUNT_SHIFT) as usize
    }

    fn body_pointer(self) -> NonNull<u64> {
        self.content_word_pointer(0)
    }
    pub fn body(self) -> InstructionPointer {
        unsafe { *self.body_pointer().as_ref() as usize }.into()
    }
}

impl DebugDisplay for HeapClosure {
    fn fmt(&self, f: &mut Formatter, is_debug: bool) -> fmt::Result {
        let argument_count = self.argument_count();
        let captured = self.captured();
        if is_debug {
            write!(
                f,
                "{{ {} {} (capturing {}) → {:?} }}",
                argument_count,
                if argument_count == 1 {
                    "argument"
                } else {
                    "arguments"
                },
                if captured.is_empty() {
                    "nothing".to_string()
                } else {
                    captured
                        .iter()
                        .map(|it| DebugDisplay::to_string(it, false))
                        .join(", ")
                },
                self.body(),
            )
        } else {
            write!(f, "{{…}}")
        }
    }
}
impl_debug_display_via_debugdisplay!(HeapClosure);

impl Eq for HeapClosure {}
impl PartialEq for HeapClosure {
    fn eq(&self, other: &Self) -> bool {
        // TODO: Compare the underlying HIR ID once we have it here (plus captured stuff)
        self.captured() == other.captured()
            && self.argument_count() == other.argument_count()
            && self.body() == other.body()
    }
}
impl Hash for HeapClosure {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.captured().hash(state);
        self.argument_count().hash(state);
        self.body().hash(state);
    }
}

heap_object_impls!(HeapClosure);

impl HeapObjectTrait for HeapClosure {
    fn content_size(self) -> usize {
        (1 + self.captured_len()) * HeapObject::WORD_SIZE
    }

    fn clone_content_to_heap_with_mapping(
        self,
        heap: &mut Heap,
        clone: HeapObject,
        address_map: &mut FxHashMap<HeapObject, HeapObject>,
    ) {
        let clone = Self(clone);
        unsafe { *clone.body_pointer().as_mut() = *self.body() as u64 };
        for (index, &captured) in self.captured().iter().enumerate() {
            clone.unsafe_set_content_word(
                1 + index,
                captured
                    .clone_to_heap_with_mapping(heap, address_map)
                    .raw_word()
                    .get(),
            );
        }
    }

    fn drop_children(self, heap: &mut Heap) {
        for captured in self.captured() {
            captured.drop(heap);
        }
    }
}
