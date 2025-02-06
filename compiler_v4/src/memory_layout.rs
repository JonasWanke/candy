use crate::{
    mono::{BuiltinType, TypeDeclaration},
    utils::HashSetExtension,
};
use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet};
use std::{cmp::Reverse, collections::hash_map::Entry};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeLayout {
    pub layout: Layout,
    pub kind: TypeLayoutKind,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeLayoutKind {
    Builtin,
    Struct {
        field_offsets: FxHashMap<Box<str>, usize>,
    },
    Enum {
        tag_offset: usize,
        boxed_variants: FxHashSet<Box<str>>,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Layout {
    size: usize,
    alignment: Alignment,
}
impl Layout {
    pub const POINTER: Self = Self::new(8, Alignment::_8);

    #[must_use]
    pub const fn new(size: usize, alignment: Alignment) -> Self {
        Self { size, alignment }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum Alignment {
    #[default]
    _1,
    _2,
    _4,
    _8,
}
impl Alignment {
    #[must_use]
    const fn get(self) -> usize {
        match self {
            Self::_1 => 1,
            Self::_2 => 2,
            Self::_4 => 4,
            Self::_8 => 8,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct AggregateLayout {
    layout: Layout,
    part_offsets: Box<[usize]>,
}

pub fn lay_out_memory(
    type_declarations: &FxHashMap<Box<str>, TypeDeclaration>,
) -> (FxHashMap<Box<str>, TypeLayout>, Box<[Box<str>]>) {
    let mut context = Context::new(type_declarations);
    for type_ in type_declarations.keys() {
        context.lay_out(type_);
    }
    let memory_layouts = context
        .memory_layouts
        .into_iter()
        .map(|(type_, layout)| (type_, layout.unwrap()))
        .collect();
    (
        memory_layouts,
        context.sorted_declarations.into_boxed_slice(),
    )
}
struct Context<'m> {
    type_declarations: &'m FxHashMap<Box<str>, TypeDeclaration>,
    memory_layouts: FxHashMap<Box<str>, Option<TypeLayout>>,
    sorted_declarations: Vec<Box<str>>,
}
impl<'m> Context<'m> {
    fn new(type_declarations: &'m FxHashMap<Box<str>, TypeDeclaration>) -> Self {
        Self {
            type_declarations,
            memory_layouts: FxHashMap::default(),
            sorted_declarations: Vec::new(),
        }
    }

    fn lay_out(&mut self, type_: &str) -> Option<Layout> {
        match self.memory_layouts.entry(type_.into()) {
            Entry::Occupied(entry) => {
                return entry.get().as_ref().map(|it| it.layout);
            }
            Entry::Vacant(entry) => {
                entry.insert(None);
            }
        }

        let declaration = &self.type_declarations[type_];
        let type_layout = match declaration {
            TypeDeclaration::Builtin(builtin_type) => {
                let layout = match builtin_type {
                    BuiltinType::Int => Layout::new(8, Alignment::_8),
                    BuiltinType::List(_) => Layout::new(16, Alignment::_8),
                    BuiltinType::Text => Layout::new(8, Alignment::_8),
                };
                TypeLayout {
                    layout,
                    kind: TypeLayoutKind::Builtin,
                }
            }
            TypeDeclaration::Struct { fields } => {
                let Some(field_layouts) = fields
                    .iter()
                    .map(|(name, type_)| try { (name, self.lay_out(type_)?) })
                    .collect::<Option<Vec<_>>>()
                else {
                    assert!(self.memory_layouts.remove(type_).unwrap().is_none());
                    return None;
                };
                let parts = field_layouts
                    .iter()
                    .map(|(_, layout)| *layout)
                    .collect::<Vec<_>>();
                let aggregate_layout = Self::lay_out_aggregate(&parts);
                TypeLayout {
                    layout: aggregate_layout.layout,
                    kind: TypeLayoutKind::Struct {
                        field_offsets: field_layouts
                            .iter()
                            .zip_eq(aggregate_layout.part_offsets.iter())
                            .map(|((name, _), offset)| ((*name).clone(), *offset))
                            .collect(),
                    },
                }
            }
            TypeDeclaration::Enum { variants } => {
                if variants.len() > 256 {
                    todo!("support enums with more than 256 variants")
                }

                let mut size = 0;
                let mut alignment = Alignment::default();
                let mut boxed_variants = FxHashSet::default();
                for variant in variants {
                    if let Some(value_type) = variant.value_type.as_ref() {
                        let layout = self.lay_out(value_type).unwrap_or_else(|| {
                            boxed_variants.force_insert(variant.name.clone());
                            Layout::POINTER
                        });
                        size = size.max(layout.size);
                        alignment = alignment.max(layout.alignment);
                    }
                }
                let tag_offset = size;
                size += 1;
                TypeLayout {
                    layout: Layout { size, alignment },
                    kind: TypeLayoutKind::Enum {
                        tag_offset,
                        boxed_variants,
                    },
                }
            }
            TypeDeclaration::Function { .. } => TypeLayout {
                layout: Layout::new(16, Alignment::_8),
                kind: TypeLayoutKind::Builtin,
            },
        };
        self.sorted_declarations.push(type_.into());
        let layout = type_layout.layout;
        assert!(self
            .memory_layouts
            .insert(type_.into(), Some(type_layout))
            .unwrap()
            .is_none());
        Some(layout)
    }

    fn lay_out_aggregate(parts: &[Layout]) -> AggregateLayout {
        if parts.is_empty() {
            return AggregateLayout::default();
        }

        let parts = parts
            .iter()
            .enumerate()
            .sorted_by_key(|(index, layout)| {
                (Reverse(layout.alignment), Reverse(layout.size), *index)
            })
            .collect_vec();
        let alignment = parts.first().unwrap().1.alignment;

        let mut part_offsets = Box::<[usize]>::new_uninit_slice(parts.len());
        let mut offset = 0usize;
        for (index, layout) in parts {
            offset = offset.next_multiple_of(layout.alignment.get());
            part_offsets[index].write(offset);
            offset += layout.size;
        }

        let part_offsets = unsafe { part_offsets.assume_init() };
        AggregateLayout {
            layout: Layout {
                size: offset,
                alignment,
            },
            part_offsets,
        }
    }
}
