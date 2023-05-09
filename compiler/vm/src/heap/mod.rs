pub use self::{
    object::{Builtin, Data, Function, HirId, Int, List, ReceivePort, SendPort, Struct, Tag, Text},
    object_heap::{HeapData, HeapObject, HeapObjectTrait},
    object_inline::{
        int::I64BitLength, InlineData, InlineObject, InlineObjectSliceCloneToHeap,
        InlineObjectTrait,
    },
    pointer::Pointer,
};
use crate::channel::ChannelId;
use derive_more::{DebugCustom, Deref, Pointer};
use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet};
use std::{
    alloc::{self, Allocator, Layout},
    fmt::{self, Debug, Formatter},
    hash::{Hash, Hasher},
    mem,
};

mod object;
mod object_heap;
mod object_inline;
mod pointer;

#[derive(Default)]
pub struct Heap {
    objects: FxHashSet<ObjectInHeap>,
    channel_refcounts: FxHashMap<ChannelId, usize>,
}

impl Heap {
    pub fn allocate(&mut self, header_word: u64, content_size: usize) -> HeapObject {
        let layout = Layout::from_size_align(
            2 * HeapObject::WORD_SIZE + content_size,
            HeapObject::WORD_SIZE,
        )
        .unwrap();

        // TODO: Handle allocation failure by stopping the fiber.
        let pointer = alloc::Global
            .allocate(layout)
            .expect("Not enough memory.")
            .cast();
        unsafe { *pointer.as_ptr() = header_word };
        let object = HeapObject::new(pointer);
        object.set_reference_count(1);
        self.objects.insert(ObjectInHeap(object));
        object
    }
    /// Don't call this method directly, call [drop] or [free] instead!
    pub(super) fn deallocate(&mut self, object: HeapData) {
        object.deallocate_external_stuff();
        let layout = Layout::from_size_align(
            2 * HeapObject::WORD_SIZE + object.content_size(),
            HeapObject::WORD_SIZE,
        )
        .unwrap();
        self.objects.remove(&ObjectInHeap(*object));
        unsafe { alloc::Global.deallocate(object.address().cast(), layout) };
    }

    pub(self) fn notify_port_created(&mut self, channel_id: ChannelId) {
        *self.channel_refcounts.entry(channel_id).or_default() += 1;
    }
    pub(self) fn dup_channel_by(&mut self, channel_id: ChannelId, amount: usize) {
        *self.channel_refcounts.entry(channel_id).or_insert_with(|| {
            panic!("Called `dup_channel_by`, but {channel_id:?} doesn't exist.")
        }) += amount;
    }
    pub(self) fn drop_channel(&mut self, channel_id: ChannelId) {
        let channel_refcount = self
            .channel_refcounts
            .entry(channel_id)
            .or_insert_with(|| panic!("Called `drop_channel`, but {channel_id:?} doesn't exist."));
        *channel_refcount -= 1;
        if *channel_refcount == 0 {
            self.channel_refcounts.remove(&channel_id).unwrap();
        }
    }

    pub fn adopt(&mut self, mut other: Heap) {
        self.objects.extend(mem::take(&mut other.objects));
        for (channel_id, refcount) in mem::take(&mut other.channel_refcounts) {
            *self.channel_refcounts.entry(channel_id).or_default() += refcount;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = HeapObject> + '_ {
        self.objects.iter().map(|it| **it)
    }

    pub fn known_channels(&self) -> impl IntoIterator<Item = ChannelId> + '_ {
        self.channel_refcounts.keys().copied()
    }

    // We do not confuse this with the `std::Clone::clone` method.
    #[allow(clippy::should_implement_trait)]
    pub fn clone(&self) -> (Heap, FxHashMap<HeapObject, HeapObject>) {
        let mut cloned = Heap {
            objects: FxHashSet::default(),
            channel_refcounts: self.channel_refcounts.clone(),
        };

        let mut mapping = FxHashMap::default();
        for object in &self.objects {
            object.clone_to_heap_with_mapping(&mut cloned, &mut mapping);
        }

        (cloned, mapping)
    }

    pub(super) fn reset_reference_counts(&mut self) {
        for value in self.channel_refcounts.values_mut() {
            *value = 0;
        }

        let to_deallocate = self
            .objects
            .iter()
            .filter(|it| it.reference_count() == 0)
            .map(|&it| *it)
            .collect_vec();
        for object in to_deallocate {
            self.deallocate(object.into());
        }
    }
    pub(super) fn drop_all_unreferenced(&mut self) {
        self.channel_refcounts
            .retain(|_, &mut refcount| refcount > 0);
        for object in &self.objects {
            object.set_reference_count(0);
        }
    }

    pub fn clear(&mut self) {
        for object in mem::take(&mut self.objects).iter() {
            self.deallocate(HeapData::from(object.0));
        }
        self.channel_refcounts.clear();
    }
}

impl Debug for Heap {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        writeln!(f, "{{\n  channel_refcounts: {:?}", self.channel_refcounts)?;

        for &object in &self.objects {
            let reference_count = object.reference_count();
            writeln!(
                f,
                "  {object:p} ({reference_count} {}): {object:?}",
                if reference_count == 1 { "ref" } else { "refs" },
            )?;
        }
        write!(f, "}}")
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        self.clear();
    }
}

/// For tracking objects allocated in the heap, we don't want deep equality, but
/// only care about the addresses.
#[derive(Clone, Copy, DebugCustom, Deref, Pointer)]
struct ObjectInHeap(HeapObject);

impl Eq for ObjectInHeap {}
impl PartialEq for ObjectInHeap {
    fn eq(&self, other: &Self) -> bool {
        self.0.address() == other.0.address()
    }
}

impl Hash for ObjectInHeap {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.address().hash(state)
    }
}
