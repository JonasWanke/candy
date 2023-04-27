use super::{
    object_heap::{
        closure::HeapClosure, hir_id::HeapHirId, int::HeapInt, list::HeapList, struct_::HeapStruct,
        tag::HeapTag, text::HeapText, HeapData, HeapObject,
    },
    object_inline::{
        builtin::InlineBuiltin,
        int::InlineInt,
        port::{InlineReceivePort, InlineSendPort},
        InlineData, InlineObject,
    },
    Heap,
};
use crate::{
    channel::ChannelId,
    lir::{Instruction, Lir},
    mir_to_lir::MirToLir,
    utils::{impl_debug_display_via_debugdisplay, DebugDisplay},
};
use candy_frontend::{builtin_functions::BuiltinFunction, hir::Id, module::Module, TracingConfig};
use derive_more::{Deref, From};
use num_bigint::BigInt;
use rustc_hash::FxHashMap;
use std::{
    borrow::Cow,
    cmp::Ordering,
    fmt::{self, Formatter},
    hash::Hash,
    ops::{Shl, Shr},
};

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum Data {
    Int(Int),
    Tag(Tag),
    Text(Text),
    List(List),
    Struct(Struct),
    HirId(HirId),
    Closure(Closure),
    Builtin(Builtin),
    SendPort(SendPort),
    ReceivePort(ReceivePort),
}
impl Data {
    pub fn closure(&self) -> Option<&Closure> {
        if let Data::Closure(closure) = self {
            Some(closure)
        } else {
            None
        }
    }
}

impl From<InlineObject> for Data {
    fn from(object: InlineObject) -> Self {
        match object.into() {
            InlineData::Pointer(pointer) => pointer.get().into(),
            InlineData::Int(int) => Data::Int(Int::Inline(int)),
            InlineData::SendPort(send_port) => Data::SendPort(SendPort(send_port)),
            InlineData::ReceivePort(receive_port) => Data::ReceivePort(ReceivePort(receive_port)),
            InlineData::Builtin(builtin) => Data::Builtin(Builtin(builtin)),
        }
    }
}
impl From<HeapObject> for Data {
    fn from(object: HeapObject) -> Self {
        match object.into() {
            HeapData::Int(int) => Data::Int(Int::Heap(int)),
            HeapData::List(list) => Data::List(List(list)),
            HeapData::Struct(struct_) => Data::Struct(Struct(struct_)),
            HeapData::Tag(tag) => Data::Tag(Tag(tag)),
            HeapData::Text(text) => Data::Text(Text(text)),
            HeapData::Closure(closure) => Data::Closure(Closure(closure)),
            HeapData::HirId(hir_id) => Data::HirId(HirId(hir_id)),
        }
    }
}

impl DebugDisplay for Data {
    fn fmt(&self, f: &mut Formatter, is_debug: bool) -> fmt::Result {
        match self {
            Data::Int(int) => DebugDisplay::fmt(int, f, is_debug),
            Data::Tag(tag) => DebugDisplay::fmt(tag, f, is_debug),
            Data::Text(text) => DebugDisplay::fmt(text, f, is_debug),
            Data::List(list) => DebugDisplay::fmt(list, f, is_debug),
            Data::Struct(struct_) => DebugDisplay::fmt(struct_, f, is_debug),
            Data::HirId(hir_id) => DebugDisplay::fmt(hir_id, f, is_debug),
            Data::Closure(closure) => DebugDisplay::fmt(closure, f, is_debug),
            Data::Builtin(builtin) => DebugDisplay::fmt(builtin, f, is_debug),
            Data::SendPort(send_port) => DebugDisplay::fmt(send_port, f, is_debug),
            Data::ReceivePort(receive_port) => DebugDisplay::fmt(receive_port, f, is_debug),
        }
    }
}
impl_debug_display_via_debugdisplay!(Data);

// Int

#[derive(Clone, Copy, Eq, From, Hash, PartialEq)]
pub enum Int {
    Inline(InlineInt),
    Heap(HeapInt),
}

impl Int {
    pub fn create<T>(heap: &mut Heap, value: T) -> Self
    where
        T: Copy + TryInto<i64> + Into<BigInt>,
    {
        value
            .try_into()
            .map_err(|_| ())
            .and_then(InlineInt::try_from)
            .map(|it| it.into())
            .unwrap_or_else(|_| HeapInt::create(heap, value.into()).into())
    }
    pub fn create_from_bigint(heap: &mut Heap, value: BigInt) -> Self {
        i64::try_from(&value)
            .map_err(|_| ())
            .and_then(InlineInt::try_from)
            .map(|it| it.into())
            .unwrap_or_else(|_| HeapInt::create(heap, value).into())
    }

    pub fn get(&self) -> Cow<BigInt> {
        match self {
            Int::Inline(int) => Cow::Owned(int.get().into()),
            Int::Heap(int) => Cow::Borrowed(int.get()),
        }
    }
    pub fn try_get<T>(&self) -> Option<T>
    where
        T: TryFrom<i64> + for<'a> TryFrom<&'a BigInt>,
    {
        match self {
            Int::Inline(int) => int.try_get(),
            Int::Heap(int) => int.get().try_into().ok(),
        }
    }

    operator_fn!(add);
    operator_fn!(subtract);
    operator_fn!(multiply);
    operator_fn!(int_divide_truncating);
    operator_fn!(remainder);
    pub fn modulo(&self, heap: &mut Heap, rhs: &Int) -> Self {
        match (self, rhs) {
            (Int::Inline(lhs), Int::Inline(rhs)) => lhs.modulo(heap, *rhs),
            (Int::Heap(on_heap), Int::Inline(inline))
            | (Int::Inline(inline), Int::Heap(on_heap)) => {
                on_heap.modulo(heap, &inline.get().into())
            }
            (Int::Heap(lhs), Int::Heap(rhs)) => lhs.modulo(heap, rhs.get()),
        }
    }

    pub fn compare_to(&self, heap: &mut Heap, rhs: &Int) -> Tag {
        match (self, rhs) {
            (Int::Inline(lhs), Int::Inline(rhs)) => lhs.compare_to(heap, *rhs),
            (Int::Heap(on_heap), Int::Inline(inline))
            | (Int::Inline(inline), Int::Heap(on_heap)) => {
                on_heap.compare_to(heap, &inline.get().into())
            }
            (Int::Heap(lhs), Int::Heap(rhs)) => lhs.compare_to(heap, rhs.get()),
        }
    }

    shift_fn!(shift_left, shl);
    shift_fn!(shift_right, shr);

    pub fn bit_length(&self, heap: &mut Heap) -> Self {
        match self {
            Int::Inline(int) => int.bit_length().into(),
            Int::Heap(int) => int.bit_length(heap),
        }
    }

    bitwise_fn!(bitwise_and);
    bitwise_fn!(bitwise_or);
    bitwise_fn!(bitwise_xor);
}

macro_rules! bitwise_fn {
    ($name:ident) => {
        pub fn $name(&self, heap: &mut Heap, rhs: &Int) -> Self {
            match (self, rhs) {
                (Int::Inline(lhs), Int::Inline(rhs)) => lhs.$name(*rhs).into(),
                (Int::Heap(on_heap), Int::Inline(inline))
                | (Int::Inline(inline), Int::Heap(on_heap)) => {
                    on_heap.$name(heap, &inline.get().into())
                }
                (Int::Heap(lhs), Int::Heap(rhs)) => lhs.$name(heap, rhs.get()),
            }
        }
    };
}
macro_rules! operator_fn {
    ($name:ident) => {
        pub fn $name(&self, heap: &mut Heap, rhs: &Int) -> Self {
            match (self, rhs) {
                (Int::Inline(lhs), Int::Inline(rhs)) => lhs.$name(heap, *rhs),
                (Int::Heap(on_heap), Int::Inline(inline))
                | (Int::Inline(inline), Int::Heap(on_heap)) => on_heap.$name(heap, inline.get()),
                (Int::Heap(lhs), Int::Heap(rhs)) => lhs.$name(heap, rhs.get()),
            }
        }
    };
}
macro_rules! shift_fn {
    ($name:ident, $function:ident) => {
        pub fn $name(&self, heap: &mut Heap, rhs: &Int) -> Self {
            match (self, rhs) {
                (Int::Inline(lhs), Int::Inline(rhs)) => lhs.$name(heap, *rhs),
                // TODO: Support shifting by larger numbers
                (Int::Inline(lhs), rhs) => Int::create_from_bigint(
                    heap,
                    BigInt::from(lhs.get()).$function(rhs.try_get::<i128>().unwrap()),
                ),
                (Int::Heap(lhs), rhs) => lhs.$name(heap, rhs.try_get::<i128>().unwrap()),
            }
        }
    };
}
use {bitwise_fn, operator_fn, shift_fn};

impl DebugDisplay for Int {
    fn fmt(&self, f: &mut Formatter, is_debug: bool) -> fmt::Result {
        match self {
            Int::Inline(int) => DebugDisplay::fmt(int, f, is_debug),
            Int::Heap(int) => DebugDisplay::fmt(int, f, is_debug),
        }
    }
}
impl_debug_display_via_debugdisplay!(Int);

impl From<Int> for InlineObject {
    fn from(int: Int) -> Self {
        match int {
            Int::Inline(int) => *int,
            Int::Heap(int) => (*int).into(),
        }
    }
}
impl_try_froms!(Int, "Expected an int.");
impl_try_from_heap_object!(Int, "Expected an int.");

// Tag

#[derive(Clone, Copy, Deref, Eq, From, Hash, PartialEq)]
pub struct Tag(HeapTag);

impl Tag {
    pub fn create(heap: &mut Heap, symbol: Text, value: impl Into<Option<InlineObject>>) -> Self {
        HeapTag::create(heap, symbol, value).into()
    }
    pub fn create_from_str(
        heap: &mut Heap,
        symbol: &str,
        value: impl Into<Option<InlineObject>>,
    ) -> Self {
        let symbol = Text::create(heap, symbol);
        Self::create(heap, symbol, value)
    }
    pub fn create_nothing(heap: &mut Heap) -> Self {
        Self::create_from_str(heap, "Nothing", None)
    }
    pub fn create_bool(heap: &mut Heap, value: bool) -> Self {
        Self::create_from_str(heap, if value { "True" } else { "False" }, None)
    }
    pub fn create_ordering(heap: &mut Heap, value: Ordering) -> Self {
        let value = match value {
            Ordering::Less => "Less",
            Ordering::Equal => "Equal",
            Ordering::Greater => "Greater",
        };
        Self::create_from_str(heap, value, None)
    }
}

impls_via_0!(Tag);
impl_try_froms!(Tag, "Expected a tag.");
impl_try_from_heap_object!(Tag, "Expected a tag.");

impl TryFrom<InlineObject> for bool {
    type Error = &'static str;

    fn try_from(value: InlineObject) -> Result<Self, Self::Error> {
        (Data::from(value)).try_into()
    }
}
impl TryFrom<Data> for bool {
    type Error = &'static str;

    fn try_from(value: Data) -> Result<Self, Self::Error> {
        let tag: Tag = value.try_into()?;
        if tag.value().is_some() {
            return Err("Expected a tag without a value.");
        }

        match tag.symbol().get() {
            "True" => Ok(true),
            "False" => Ok(false),
            _ => Err("Expected `True` or `False`."),
        }
    }
}

// Text

#[derive(Clone, Copy, Deref, Eq, From, Hash, PartialEq)]
pub struct Text(HeapText);

impl Text {
    pub fn create(heap: &mut Heap, value: &str) -> Self {
        HeapText::create(heap, value).into()
    }
}

impls_via_0!(Text);
impl_try_froms!(Text, "Expected a text.");
impl_try_from_heap_object!(Text, "Expected a text.");

// List

#[derive(Clone, Copy, Deref, Eq, From, Hash, PartialEq)]
pub struct List(HeapList);

impl List {
    pub fn create(heap: &mut Heap, items: &[InlineObject]) -> Self {
        HeapList::create(heap, items).into()
    }
}

impls_via_0!(List);
impl_try_froms!(List, "Expected a list.");
impl_try_from_heap_object!(List, "Expected a list.");

// Struct

#[derive(Clone, Copy, Deref, Eq, From, Hash, PartialEq)]
pub struct Struct(HeapStruct);

impl Struct {
    pub fn create(heap: &mut Heap, fields: &FxHashMap<InlineObject, InlineObject>) -> Self {
        HeapStruct::create(heap, fields).into()
    }
    pub fn create_with_symbol_keys(
        heap: &mut Heap,
        fields: impl IntoIterator<Item = (&str, InlineObject)>,
    ) -> Self {
        let fields = fields
            .into_iter()
            .map(|(key, value)| ((Tag::create_from_str(heap, key, None)).into(), value))
            .collect();
        Self::create(heap, &fields)
    }
    pub fn create_result(heap: &mut Heap, value: Result<InlineObject, InlineObject>) -> Self {
        let (type_, value) = match value {
            Ok(it) => ("Ok", it),
            Err(it) => ("Error", it),
        };
        // PERF: Avoid allocating an intermediate map.
        let fields = [
            ("Type", Tag::create_from_str(heap, type_, None).into()),
            ("Value", value),
        ];
        Self::create_with_symbol_keys(heap, fields)
    }
}

impls_via_0!(Struct);
impl_try_froms!(Struct, "Expected a struct.");
impl_try_from_heap_object!(Struct, "Expected a struct.");

// Closure

#[derive(Clone, Copy, Deref, Eq, From, Hash, PartialEq)]
pub struct Closure(HeapClosure);

impl Closure {
    pub fn create(
        heap: &mut Heap,
        captured: &[InlineObject],
        argument_count: usize,
        instructions: Vec<Instruction>,
    ) -> Self {
        HeapClosure::create(heap, captured, argument_count, instructions).into()
    }
    pub fn create_from_module_lir(heap: &mut Heap, lir: Lir) -> Self {
        Self::create(heap, &[], 0, lir.instructions)
    }
    pub fn create_from_module(
        heap: &mut Heap,
        db: &impl MirToLir,
        module: Module,
        tracing: TracingConfig,
    ) -> Option<Self> {
        let lir = db.lir(module, tracing)?;
        Some(Self::create_from_module_lir(heap, lir.as_ref().to_owned()))
    }
}

impls_via_0!(Closure);
impl_try_froms!(Closure, "Expected a closure.");
impl_try_from_heap_object!(Closure, "Expected a closure.");

// HIR ID

#[derive(Clone, Copy, Deref, Eq, From, Hash, PartialEq)]

pub struct HirId(HeapHirId);

impl HirId {
    pub fn create(heap: &mut Heap, id: Id) -> HirId {
        HeapHirId::create(heap, id).into()
    }
}

impls_via_0!(HirId);
impl_try_froms!(HirId, "Expected a HIR ID.");
impl_try_from_heap_object!(HirId, "Expected a HIR ID.");

// Builtin

#[derive(Clone, Copy, Deref, Eq, From, Hash, PartialEq)]
pub struct Builtin(InlineBuiltin);

impl Builtin {
    pub fn create(builtin: BuiltinFunction) -> Self {
        InlineBuiltin::from(builtin).into()
    }
}

impls_via_0!(Builtin);

// Send Port

#[derive(Clone, Copy, Deref, Eq, Hash, PartialEq)]
pub struct SendPort(InlineSendPort);

impl SendPort {
    pub fn create(heap: &mut Heap, channel_id: ChannelId) -> InlineObject {
        InlineSendPort::create(heap, channel_id)
    }
}

impls_via_0!(SendPort);
impl_try_froms!(SendPort, "Expected a send port.");

// Receive Port

#[derive(Clone, Copy, Deref, Eq, Hash, PartialEq)]
pub struct ReceivePort(InlineReceivePort);

impl ReceivePort {
    pub fn create(heap: &mut Heap, channel_id: ChannelId) -> InlineObject {
        InlineReceivePort::create(heap, channel_id)
    }
}

impls_via_0!(ReceivePort);
impl_try_froms!(ReceivePort, "Expected a receive port.");

// Utils

macro_rules! impls_via_0 {
    ($type:ty) => {
        impl DebugDisplay for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter, is_debug: bool) -> std::fmt::Result {
                DebugDisplay::fmt(&self.0, f, is_debug)
            }
        }
        impl_debug_display_via_debugdisplay!($type);

        impl From<$type> for InlineObject {
            fn from(value: $type) -> Self {
                (**value).into()
            }
        }
    };
}
pub(self) use impls_via_0;

// FIXME: merge with sections above
macro_rules! impl_try_froms {
    ($type:tt, $error_message:expr$(,)?) => {
        impl TryFrom<InlineObject> for $type {
            type Error = &'static str;

            fn try_from(value: InlineObject) -> Result<Self, Self::Error> {
                Data::from(value).try_into()
            }
        }
        impl TryFrom<Data> for $type {
            type Error = &'static str;

            fn try_from(value: Data) -> Result<Self, Self::Error> {
                match value {
                    Data::$type(it) => Ok(it),
                    _ => Err($error_message),
                }
            }
        }
        impl<'a> TryFrom<&'a Data> for &'a $type {
            type Error = &'static str;

            fn try_from(value: &'a Data) -> Result<Self, Self::Error> {
                match &value {
                    Data::$type(it) => Ok(it),
                    _ => Err($error_message),
                }
            }
        }
    };
}
macro_rules! impl_try_from_heap_object {
    ($type:tt, $error_message:expr$(,)?) => {
        impl TryFrom<HeapObject> for $type {
            type Error = &'static str;

            fn try_from(value: HeapObject) -> Result<Self, Self::Error> {
                Data::from(value).try_into()
            }
        }
    };
}
use {impl_try_from_heap_object, impl_try_froms};
