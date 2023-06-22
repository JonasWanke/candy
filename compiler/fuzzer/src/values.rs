use super::input::Input;
use candy_frontend::builtin_functions;
use candy_vm::heap::{Data, Heap, I64BitLength, InlineObject, Int, List, Struct, Tag, Text};
use extension_trait::extension_trait;
use itertools::Itertools;
use num_bigint::RandBigInt;
use rand::{
    prelude::ThreadRng,
    seq::{IteratorRandom, SliceRandom},
    Rng,
};
use rustc_hash::FxHashMap;
use std::{cell::RefCell, rc::Rc};

#[extension_trait]
pub impl InputGeneration for Input {
    fn generate(heap: Rc<RefCell<Heap>>, num_args: usize, symbols: &[Text]) -> Input {
        let mut arguments = vec![];
        for _ in 0..num_args {
            let address = InlineObject::generate(
                &mut heap.borrow_mut(),
                &mut rand::thread_rng(),
                5.0,
                symbols,
            );
            arguments.push(address);
        }
        Input { heap, arguments }
    }
    fn mutate(&mut self, rng: &mut ThreadRng, symbols: &[Text]) {
        let mut heap = self.heap.borrow_mut();
        let argument = self.arguments.choose_mut(rng).unwrap();
        *argument = argument.generate_mutated(&mut heap, rng, symbols);
    }
    fn complexity(&self) -> usize {
        self.arguments
            .iter()
            .map(|argument| argument.complexity())
            .sum()
    }
}

#[extension_trait]
impl InlineObjectGeneration for InlineObject {
    fn generate(
        heap: &mut Heap,
        rng: &mut ThreadRng,
        mut complexity: f32,
        symbols: &[Text],
    ) -> InlineObject {
        match rng.gen_range(1..=5) {
            1 => Int::create_from_bigint(heap, rng.gen_bigint(10)).into(),
            2 => Text::create(heap, "test").into(),
            3 => {
                let value = if rng.gen_bool(0.8) {
                    Some(Self::generate(heap, rng, complexity - 10.0, symbols))
                } else {
                    None
                };
                Tag::create(heap, *symbols.choose(rng).unwrap(), value).into()
            }
            4 => {
                complexity -= 1.0;
                let mut items = vec![];
                while complexity > 10.0 {
                    let item = Self::generate(heap, rng, 10.0, symbols);
                    items.push(item);
                    complexity -= 10.0;
                }
                List::create(heap, &items).into()
            }
            5 => {
                complexity -= 1.0;
                let mut fields = FxHashMap::default();
                while complexity > 20.0 {
                    let key = Self::generate(heap, rng, 10.0, symbols);
                    let value = Self::generate(heap, rng, 10.0, symbols);
                    fields.insert(key, value);
                    complexity -= 20.0;
                }
                Struct::create(heap, &fields).into()
            }
            6 => {
                builtin_functions::VALUES[rng.gen_range(0..builtin_functions::VALUES.len())].into()
            }
            _ => unreachable!(),
        }
    }
    fn generate_mutated(
        self,
        heap: &mut Heap,
        rng: &mut ThreadRng,
        symbols: &[Text],
    ) -> InlineObject {
        if rng.gen_bool(0.1) {
            return Self::generate(heap, rng, 100.0, symbols);
        }

        match self.into() {
            Data::Int(int) => {
                Int::create_from_bigint(heap, int.get().as_ref() + rng.gen_range(-10..10)).into()
            }
            Data::Text(text) => mutate_string(rng, heap, text.get().to_string()).into(),
            Data::Tag(tag) => {
                assert!(!symbols.is_empty());
                if rng.gen_bool(0.5) {
                    Tag::create(heap, *symbols.choose(rng).unwrap(), tag.value()).into()
                } else if let Some(value) = tag.value() {
                    if rng.gen_bool(0.9) {
                        let value = value.generate_mutated(heap, rng, symbols);
                        Tag::create(heap, tag.symbol(), Some(value)).into()
                    } else {
                        tag.without_value(heap).into()
                    }
                } else {
                    let value = Self::generate(heap, rng, 100.0, symbols);
                    Tag::create(heap, tag.symbol(), Some(value)).into()
                }
            }
            Data::List(list) => {
                let len = list.len();
                if rng.gen_bool(0.9) && len > 0 {
                    let index = rng.gen_range(0..len);
                    let new_item = list.get(index).generate_mutated(heap, rng, symbols);
                    list.replace(heap, index, new_item).into()
                } else if rng.gen_bool(0.5) && len > 0 {
                    list.remove(heap, rng.gen_range(0..len)).into()
                } else {
                    let new_item = Self::generate(heap, rng, 100.0, symbols);
                    list.insert(heap, rng.gen_range(0..=len), new_item).into()
                }
            }
            Data::Struct(struct_) => {
                let len = struct_.len();
                if rng.gen_bool(0.9) && len > 0 {
                    let index = rng.gen_range(0..len);
                    let key = struct_.keys()[index];
                    let value = struct_.values()[index].generate_mutated(heap, rng, symbols);
                    struct_.insert(heap, key, value).into()
                // TODO: Support removing value from a struct
                // } else if rng.gen_bool(0.5) && len > 0 {
                //     struct_
                //         .remove(rng.gen_range(0..len));
                } else {
                    let key = Self::generate(heap, rng, 10.0, symbols);
                    let value = Self::generate(heap, rng, 100.0, symbols);
                    struct_.insert(heap, key, value).into()
                }
            }
            Data::Builtin(_) => (*builtin_functions::VALUES.choose(rng).unwrap()).into(),
            Data::HirId(_) | Data::Function(_) | Data::SendPort(_) | Data::ReceivePort(_) => {
                panic!("Couldn't have been created for fuzzing.")
            }
        }
    }

    fn complexity(self) -> usize {
        match self.into() {
            Data::Int(int) => match int {
                Int::Inline(int) => int.get().bit_length() as usize,
                Int::Heap(int) => int.get().bits() as usize,
            },
            Data::Text(text) => text.byte_len() + 1,
            Data::Tag(tag) => {
                tag.symbol().get().len() + tag.value().map(|it| it.complexity()).unwrap_or_default()
            }
            Data::List(list) => {
                list.items()
                    .iter()
                    .map(|item| item.complexity())
                    .sum::<usize>()
                    + 1
            }
            Data::Struct(struct_) => {
                struct_
                    .iter()
                    .map(|(_, key, value)| key.complexity() + value.complexity())
                    .sum::<usize>()
                    + 1
            }
            Data::HirId(_)
            | Data::Function(_)
            | Data::Builtin(_)
            | Data::SendPort(_)
            | Data::ReceivePort(_) => 1,
        }
    }
}

fn mutate_string(rng: &mut ThreadRng, heap: &mut Heap, mut string: String) -> Text {
    if rng.gen_bool(0.5) && !string.is_empty() {
        let start = string.floor_char_boundary(rng.gen_range(0..=string.len()));
        let end = string.floor_char_boundary(start + rng.gen_range(0..=(string.len() - start)));
        string.replace_range(start..end, "")
    } else {
        let insertion_point = string.floor_char_boundary(rng.gen_range(0..=string.len()));
        let string_to_insert = (0..rng.gen_range(0..10))
            .map(|_| {
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
                    .chars()
                    .choose(rng)
                    .unwrap()
            })
            .join("");
        string.insert_str(insertion_point, &string_to_insert)
    }
    Text::create(heap, &string)
}
