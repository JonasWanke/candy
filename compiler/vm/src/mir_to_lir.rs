use crate::{
    fiber::InstructionPointer,
    heap::{Builtin, Function, Heap, HirId, InlineObject, Int, List, Struct, Tag, Text},
    lir::{Instruction, Lir, StackOffset},
};
use candy_frontend::{
    cst::CstDb,
    error::{CompilerError, CompilerErrorPayload},
    hir,
    id::CountableId,
    mir::{Body, Expression, Id, Mir},
    mir_optimize::OptimizeMir,
    module::Module,
    rich_ir::ToRichIr,
    tracing::TracingConfig,
};
use extension_trait::extension_trait;
use itertools::Itertools;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

pub fn compile_lir<Db>(
    db: &Db,
    module: Module,
    tracing: TracingConfig,
) -> (Lir, Arc<FxHashSet<CompilerError>>)
where
    Db: CstDb + OptimizeMir,
{
    let (mir, errors) = db
        .optimized_mir(module.clone(), tracing)
        .unwrap_or_else(|error| {
            let payload = CompilerErrorPayload::Module(error);
            let mir = Mir::build(|body| {
                let reason = body.push_text(payload.to_string());
                let responsible = body.push_hir_id(hir::Id::user());
                body.push_panic(reason, responsible);
            });
            let errors = vec![CompilerError::for_whole_module(module.clone(), payload)]
                .into_iter()
                .collect();
            (Arc::new(mir), Arc::new(errors))
        });

    let mut constant_heap = Heap::default();

    // The body instruction pointer of the module function will be changed from
    // zero to the correct one once the instructions are compiled.
    let module_function = Function::create(&mut constant_heap, &[], 0, 0.into());
    let responsible_module =
        HirId::create(&mut constant_heap, hir::Id::new(module.clone(), vec![]));

    let mut lir = Lir {
        module: module.clone(),
        constant_heap,
        instructions: vec![],
        origins: vec![],
        module_function,
        responsible_module,
    };

    let start = compile_function(
        &mut lir,
        &mut FxHashMap::default(),
        FxHashSet::from_iter([hir::Id::new(module, vec![])]),
        &FxHashSet::default(),
        &[],
        Id::from_usize(0),
        &mir.body,
    );
    module_function.set_body(start);

    (lir, errors)
}

fn compile_function(
    lir: &mut Lir,
    constants: &mut FxHashMap<Id, InlineObject>,
    original_hirs: FxHashSet<hir::Id>,
    captured: &FxHashSet<Id>,
    parameters: &[Id],
    responsible_parameter: Id,
    body: &Body,
) -> InstructionPointer {
    let mut context = LoweringContext {
        lir,
        constants,
        stack: vec![],
        instructions: vec![],
    };
    for captured in captured {
        context.stack.push(*captured);
    }
    for parameter in parameters {
        context.stack.push(*parameter);
    }
    context.stack.push(responsible_parameter);

    for (id, expression) in body.iter() {
        context.compile_expression(id, expression);
    }
    // Expressions may not push things onto the stack, but to the constant heap
    // instead.
    if *context.stack.last().unwrap() != body.return_value() {
        context.emit_reference_to(body.return_value());
    }

    if matches!(
        context.instructions.last().unwrap(),
        Instruction::Call { .. },
    ) {
        let Instruction::Call { num_args } = context.instructions.pop().unwrap() else { unreachable!() };
        context.instructions.push(Instruction::TailCall {
            num_locals_to_pop: context.stack.len() - 1,
            num_args,
        });
    } else {
        let dummy_id = Id::from_usize(0);
        context.emit(
            dummy_id,
            Instruction::PopMultipleBelowTop(context.stack.len() - 1),
        );
        context.emit(dummy_id, Instruction::Return);
    }

    let mut instructions = context.instructions;
    let num_instructions = instructions.len();
    let start = lir.instructions.len().into();
    lir.instructions.append(&mut instructions);
    lir.origins
        .extend((0..num_instructions).map(|_| original_hirs.clone()));
    start
}

struct LoweringContext<'c> {
    lir: &'c mut Lir,
    constants: &'c mut FxHashMap<Id, InlineObject>,
    stack: Vec<Id>,
    instructions: Vec<Instruction>,
}
impl<'c> LoweringContext<'c> {
    fn compile_expression(&mut self, id: Id, expression: &Expression) {
        match expression {
            Expression::Int(int) => {
                let int = Int::create_from_bigint(&mut self.lir.constant_heap, int.clone());
                self.constants.insert(id, int.into());
            }
            Expression::Text(text) => {
                let text = Text::create(&mut self.lir.constant_heap, text);
                self.constants.insert(id, text.into());
            }
            Expression::Reference(referenced) => {
                if let Some(&constant) = self.constants.get(referenced) {
                    self.constants.insert(id, constant);
                } else {
                    let offset = self.stack.find_id(*referenced);
                    self.emit(id, Instruction::PushFromStack(offset));
                }
            }
            Expression::Tag { symbol, value } => {
                let symbol = Text::create(&mut self.lir.constant_heap, symbol);

                match value {
                    Some(value) => {
                        if let Some(value) = self.constants.get(value) {
                            let tag = Tag::create(&mut self.lir.constant_heap, symbol, *value);
                            self.constants.insert(id, tag.into());
                        } else {
                            self.emit_reference_to(*value);
                            self.emit(id, Instruction::CreateTag { symbol });
                        }
                    }
                    None => {
                        let tag = Tag::create(&mut self.lir.constant_heap, symbol, None);
                        self.constants.insert(id, tag.into());
                    }
                }
            }
            Expression::Builtin(builtin) => {
                let builtin = Builtin::create(*builtin);
                self.constants.insert(id, builtin.into());
            }
            Expression::List(items) => {
                if let Some(items) = items
                    .iter()
                    .map(|item| self.constants.get(item).copied())
                    .collect::<Option<Vec<_>>>()
                {
                    let list = List::create(&mut self.lir.constant_heap, &items);
                    self.constants.insert(id, list.into());
                } else {
                    for item in items {
                        self.emit_reference_to(*item);
                    }
                    self.emit(
                        id,
                        Instruction::CreateList {
                            num_items: items.len(),
                        },
                    );
                }
            }
            Expression::Struct(fields) => {
                if let Some(fields) = fields
                    .iter()
                    .flat_map(|(key, value)| [key, value].into_iter())
                    .map(|item| self.constants.get(item).copied())
                    .collect::<Option<Vec<_>>>()
                {
                    let fields = fields.into_iter().tuples().collect();
                    let struct_ = Struct::create(&mut self.lir.constant_heap, &fields);
                    self.constants.insert(id, struct_.into());
                } else {
                    for (key, value) in fields {
                        self.emit_reference_to(*key);
                        self.emit_reference_to(*value);
                    }
                    self.emit(
                        id,
                        Instruction::CreateStruct {
                            num_fields: fields.len(),
                        },
                    );
                }
            }
            Expression::HirId(hir_id) => {
                let hir_id = HirId::create(&mut self.lir.constant_heap, hir_id.clone());
                self.constants.insert(id, hir_id.into());
            }
            Expression::Function {
                original_hirs,
                parameters,
                responsible_parameter,
                body,
            } => {
                let captured = expression
                    .captured_ids()
                    .into_iter()
                    .filter(|captured| !self.constants.contains_key(captured))
                    .collect();

                let instructions = compile_function(
                    self.lir,
                    self.constants,
                    original_hirs.clone(),
                    &captured,
                    parameters,
                    *responsible_parameter,
                    body,
                );

                if captured.is_empty() {
                    let list = Function::create(
                        &mut self.lir.constant_heap,
                        &[],
                        parameters.len(),
                        instructions,
                    );
                    self.constants.insert(id, list.into());
                } else {
                    for captured in &captured {
                        self.emit_reference_to(*captured);
                    }
                    self.emit(
                        id,
                        Instruction::CreateFunction {
                            captured: captured
                                .iter()
                                .map(|id| self.stack.find_id(*id))
                                .collect_vec(),
                            num_args: parameters.len(),
                            body: instructions,
                        },
                    );
                }
            }
            Expression::Parameter => {
                panic!("The MIR should not contain any parameter expressions.")
            }
            Expression::Call {
                function,
                arguments,
                responsible,
            } => {
                self.emit_reference_to(*function);
                for argument in arguments {
                    self.emit_reference_to(*argument);
                }
                self.emit_reference_to(*responsible);
                self.emit(
                    id,
                    Instruction::Call {
                        num_args: arguments.len(),
                    },
                );
            }
            Expression::UseModule { .. } => {
                panic!("MIR still contains use. This should have been optimized out.");
            }
            Expression::Panic {
                reason,
                responsible,
            } => {
                self.emit_reference_to(*reason);
                self.emit_reference_to(*responsible);
                self.emit(id, Instruction::Panic);
            }
            Expression::Multiple(_) => {
                panic!("The MIR shouldn't contain multiple expressions anymore.");
            }
            Expression::TraceCallStarts {
                hir_call,
                function,
                arguments,
                responsible,
            } => {
                self.emit_reference_to(*hir_call);
                self.emit_reference_to(*function);
                for argument in arguments {
                    self.emit_reference_to(*argument);
                }
                self.emit_reference_to(*responsible);
                self.emit(
                    id,
                    Instruction::TraceCallStarts {
                        num_args: arguments.len(),
                    },
                );
            }
            Expression::TraceCallEnds { return_value } => {
                self.emit_reference_to(*return_value);
                self.emit(id, Instruction::TraceCallEnds);
            }
            Expression::TraceExpressionEvaluated {
                hir_expression,
                value,
            } => {
                self.emit_reference_to(*hir_expression);
                self.emit_reference_to(*value);
                self.emit(id, Instruction::TraceExpressionEvaluated);
            }
            Expression::TraceFoundFuzzableFunction {
                hir_definition,
                function,
            } => {
                self.emit_reference_to(*hir_definition);
                self.emit_reference_to(*function);
                self.emit(id, Instruction::TraceFoundFuzzableFunction);
            }
        }
    }

    fn emit_reference_to(&mut self, id: Id) {
        if let Some(constant) = self.constants.get(&id) {
            self.emit(id, Instruction::PushConstant(*constant));
        } else {
            let offset = self.stack.find_id(id);
            self.emit(id, Instruction::PushFromStack(offset));
        }
    }
    fn emit(&mut self, id: Id, instruction: Instruction) {
        instruction.apply_to_stack(&mut self.stack, id);
        self.instructions.push(instruction);
    }
}

#[extension_trait]
impl StackExt for Vec<Id> {
    fn pop_multiple(&mut self, n: usize) {
        for _ in 0..n {
            self.pop();
        }
    }
    fn find_id(&self, id: Id) -> StackOffset {
        self.iter()
            .rev()
            .position(|it| *it == id)
            .unwrap_or_else(|| {
                panic!(
                    "Id {} not found in stack: {}",
                    id.to_rich_ir(),
                    self.iter().map(|it| it.to_rich_ir()).join(" "),
                )
            })
    }
}
