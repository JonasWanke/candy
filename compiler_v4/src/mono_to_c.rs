use crate::{
    hir::BuiltinFunction,
    memory_layout::TypeLayoutKind,
    mono::{
        Body, BodyOrBuiltin, BuiltinType, Expression, ExpressionKind, Function, Id, Lambda, Mono,
        Parameter, TypeDeclaration,
    },
};
use itertools::Itertools;
use rustc_hash::FxHashSet;

pub fn mono_to_c(mono: &Mono) -> String {
    let mut context = Context::new(mono);
    context.lower_mono();
    context.c
}

#[derive(Debug)]
struct Context<'h> {
    mono: &'h Mono,
    c: String,
}
impl<'h> Context<'h> {
    #[must_use]
    const fn new(mono: &'h Mono) -> Self {
        Self {
            mono,
            c: String::new(),
        }
    }

    fn lower_mono(&mut self) {
        self.push("#include <errno.h>\n");
        self.push("#include <stdint.h>\n");
        self.push("#include <stdio.h>\n");
        self.push("#include <stdlib.h>\n");
        self.push("#include <string.h>\n\n");

        self.push("/// Type Forward Declarations\n\n");
        self.lower_type_forward_declarations();
        self.push("\n");

        self.push("/// Type Declarations\n\n");
        self.lower_type_declarations();
        self.push("\n");

        self.push("/// Assignment Declarations\n\n");
        self.lower_assignment_declarations();
        self.push("\n");

        self.push("/// Function Declarations\n\n");
        self.lower_function_declarations();
        self.push("\n");

        self.push("/// Assignment Definitions\n\n");
        self.lower_assignment_definitions();
        self.push("\n");

        self.push("/// Function Definitions\n\n");
        self.lower_function_definitions();

        self.push("int main() {\n");
        for name in &self.mono.assignment_initialization_order {
            self.push(format!("{name}$init();\n"));
        }
        self.push(format!("return {}().value;\n}}\n", self.mono.main_function,));
    }

    fn lower_type_forward_declarations(&mut self) {
        for name in self.mono.type_declarations.keys() {
            self.push(format!("typedef struct {name} {name};\n"));
        }
    }
    fn lower_type_declarations(&mut self) {
        for name in &self.mono.type_declaration_order {
            let declaration = &self.mono.type_declarations[name];
            self.push(format!("struct {name} {{\n"));
            match declaration {
                TypeDeclaration::Builtin(builtin_type) => {
                    match builtin_type {
                        BuiltinType::Int => {
                            self.push("int64_t value;\n");
                        }
                        BuiltinType::List(item_type_) => {
                            self.push("uint64_t length;\n");
                            self.push(format!("{item_type_}* values;\n"));
                        }
                        BuiltinType::Text => {
                            self.push("char* value;\n");
                        }
                    }
                    self.push("};\n");
                }
                TypeDeclaration::Struct { fields } => {
                    for (name, type_) in &**fields {
                        self.push(format!("{type_} {name}; "));
                    }
                    self.push("};\n");
                }
                TypeDeclaration::Enum { variants } => {
                    if !variants.is_empty() {
                        self.push("enum {");
                        for variant in &**variants {
                            self.push(format!("{name}_{},", variant.name));
                        }
                        self.push("} variant;\n");
                    }

                    self.push("union {");
                    let boxed_variants = self.get_boxed_variants(name).clone();
                    for variant in &**variants {
                        if let Some(value_type) = &variant.value_type {
                            self.push(format!(
                                "{value_type}{} {};",
                                if boxed_variants.contains(&*variant.name) {
                                    "*"
                                } else {
                                    ""
                                },
                                variant.name,
                            ));
                        }
                    }
                    self.push("} value;\n};\n");
                }
                TypeDeclaration::Function {
                    parameter_types,
                    return_type,
                } => {
                    self.push("void* closure;\n");
                    self.push(format!("{return_type} (*function)(void*"));
                    for parameter_type in &**parameter_types {
                        self.push(format!(", {parameter_type}"));
                    }
                    self.push(");\n");
                    self.push("};\n");
                }
            }
        }
    }
    fn get_boxed_variants(&self, enum_type: &str) -> &FxHashSet<Box<str>> {
        let TypeLayoutKind::Enum { boxed_variants, .. } = &self.mono.memory_layouts[enum_type].kind
        else {
            panic!("Not an enum type: `{enum_type}`");
        };
        boxed_variants
    }

    fn lower_assignment_declarations(&mut self) {
        for (name, assignment) in &self.mono.assignments {
            self.lower_lambda_declarations_in(name, &assignment.body);
            self.push(format!("{} {name};\n", &assignment.type_));
        }
    }
    fn lower_assignment_definitions(&mut self) {
        for (name, assignment) in &self.mono.assignments {
            self.lower_lambda_definitions_in(name, &[], &assignment.body);

            self.push(format!("void {name}$init() {{\n"));
            self.lower_body_expressions(name, &assignment.body);
            self.push(format!(
                "{name} = {};\n}}\n\n",
                assignment.body.return_value_id(),
            ));
        }
    }

    fn lower_function_declarations(&mut self) {
        for (name, function) in &self.mono.functions {
            if let BodyOrBuiltin::Body(body) = &function.body {
                self.lower_lambda_declarations_in(name, body);
            }

            self.lower_function_signature(name, function);
            self.push(";\n");
        }
    }
    fn lower_lambda_declarations_in(&mut self, declaration_name: &str, body: &'h Body) {
        Self::visit_lambdas_inside_body(body, &mut |id, lambda| {
            self.push(format!(
                "typedef struct {declaration_name}$lambda{id}_closure {declaration_name}$lambda{id}_closure;\n",
            ));

            self.lower_lambda_signature(declaration_name, id, lambda);
            self.push(";\n");
        });
    }
    fn lower_function_definitions(&mut self) {
        for (name, function) in &self.mono.functions {
            if let BodyOrBuiltin::Body(body) = &function.body {
                self.lower_lambda_definitions_in(name, &function.parameters, body);
            }

            self.lower_function_signature(name, function);
            self.push(" {\n");
            self.lower_body_or_builtin(name, function);
            self.push("}\n\n");
        }
    }
    fn lower_lambda_definitions_in(
        &mut self,
        declaration_name: &str,
        declaration_parameters: &[Parameter],
        body: &'h Body,
    ) {
        Self::visit_lambdas_inside_body(body, &mut |id, lambda| {
            let closure = lambda.closure_with_types(declaration_parameters, body);

            self.push(format!("struct {declaration_name}$lambda{id}_closure {{"));
            for (id, type_) in &closure {
                self.push(format!("{type_} {id}; "));
            }
            self.push("};\n");

            self.lower_lambda_signature(declaration_name, id, lambda);
            self.push(" {\n");
            self.push(format!(
                "{declaration_name}$lambda{id}_closure* closure = raw_closure;\n"
            ));
            for (id, type_) in &closure {
                self.push(format!("{type_} {id} = closure->{id};\n"));
            }
            self.lower_body(declaration_name, &lambda.body);
            self.push("}\n");
        });
    }
    fn lower_function_signature(&mut self, name: &str, function: &Function) {
        self.push(format!("{} {name}(", &function.return_type));
        for (index, parameter) in function.parameters.iter().enumerate() {
            if index > 0 {
                self.push(", ");
            }
            self.push(format!("{} {}", &parameter.type_, parameter.id));
        }
        self.push(")");
    }
    fn lower_lambda_signature(&mut self, declaration_name: &str, id: Id, lambda: &Lambda) {
        self.push(format!(
            "{} {declaration_name}$lambda{id}_function(void* raw_closure",
            &lambda.body.return_type()
        ));
        for parameter in &lambda.parameters {
            self.push(format!(", {} {}", &parameter.type_, parameter.id));
        }
        self.push(")");
    }

    fn visit_lambdas_inside_body(body: &'h Body, visitor: &mut impl FnMut(Id, &'h Lambda)) {
        for (id, _, expression) in &body.expressions {
            match &expression.kind {
                ExpressionKind::Int(_)
                | ExpressionKind::Text(_)
                | ExpressionKind::CreateStruct { .. }
                | ExpressionKind::StructAccess { .. }
                | ExpressionKind::CreateEnum { .. }
                | ExpressionKind::GlobalAssignmentReference(_)
                | ExpressionKind::LocalReference(_)
                | ExpressionKind::CallFunction { .. }
                | ExpressionKind::CallLambda { .. } => {}
                ExpressionKind::Switch { cases, .. } => {
                    for case in &**cases {
                        Self::visit_lambdas_inside_body(&case.body, visitor);
                    }
                }
                ExpressionKind::Lambda(lambda) => {
                    Self::visit_lambdas_inside_body(&lambda.body, visitor);
                    visitor(*id, lambda);
                }
            }
        }
    }

    fn lower_body_or_builtin(&mut self, declaration_name: &str, function: &Function) {
        match &function.body {
            BodyOrBuiltin::Builtin {
                builtin_function,
                substitutions,
            } => {
                self.push("// builtin function\n");
                match builtin_function {
                    BuiltinFunction::IntAdd => self.push(format!(
                        "\
                        Int result = {{.value = {a}.value + {b}.value}};
                        return result;",
                        a = function.parameters[0].id,
                        b = function.parameters[1].id,
                    )),
                    BuiltinFunction::IntBitwiseAnd => self.push(format!(
                        "\
                        Int result = {{.value = {a}.value & {b}.value}};
                        return result;",
                        a = function.parameters[0].id,
                        b = function.parameters[1].id,
                    )),
                    BuiltinFunction::IntBitwiseOr => self.push(format!(
                        "\
                        Int result = {{.value = {a}.value | {b}.value}};
                        return result;",
                        a = function.parameters[0].id,
                        b = function.parameters[1].id,
                    )),
                    BuiltinFunction::IntBitwiseXor => self.push(format!(
                        "\
                        Int result = {{.value = {a}.value ^ {b}.value}};
                        return result;",
                        a = function.parameters[0].id,
                        b = function.parameters[1].id,
                    )),
                    BuiltinFunction::IntCompareTo => self.push(format!(
                        "\
                        Ordering result = {{.variant = {a}.value < {b}.value    ? Ordering_less
                                                      : {a}.value == {b}.value ? Ordering_equal
                                                                               : Ordering_greater}};
                        return result;",
                        a = function.parameters[0].id,
                        b = function.parameters[1].id,
                    )),
                    BuiltinFunction::IntDivideTruncating => self.push(format!(
                        "\
                        Int result = {{.value = {dividend}.value / {divisor}.value}};
                        return result;",
                        dividend = function.parameters[0].id,
                        divisor = function.parameters[1].id,
                    )),
                    BuiltinFunction::IntMultiply => self.push(format!(
                        "\
                        Int result = {{.value = {factorA}.value * {factorB}.value}};
                        return result;",
                        factorA = function.parameters[0].id,
                        factorB = function.parameters[1].id,
                    )),
                    BuiltinFunction::IntParse => self.push(format!(
                        "\
                        {return_type} result;
                        char *end_pointer;
                        errno = 0;
                        uint64_t value = strtol({text}.value, &end_pointer, 10);
                        if (errno == ERANGE) {{
                            result = {{
                                .variant = {return_type}_error,
                                .value.error = {{.value = \"Value is out of range.\"}},
                            }};
                        }} else if (end_pointer == {text}.value) {{
                            result = {{
                                .variant = {return_type}_error,
                                .value.error = {{.value = \"Text is empty.\"}},
                            }};
                        }} else if (*end_pointer != '\\0') {{
                            char* message_format = \"Non-numeric character \\\"%c\\\" at index %ld.\";
                            int length = snprintf(NULL, 0, message_format, *end_pointer, end_pointer - {text}.value);
                            char *message = malloc(length + 1);
                            snprintf(message, length + 1, message_format, *end_pointer, end_pointer - {text}.value);
                            result = {{
                                .variant = {return_type}_error,
                                .value.error = {{.value = message}},
                            }};
                        }} else {{
                            result = {{
                                .variant = {return_type}_ok,
                                .value.ok = {{.value = value}},
                            }};
                        }}
                        return result;",
                        text = function.parameters[0].id,
                        return_type = function.return_type,
                    )),
                    BuiltinFunction::IntRemainder => self.push(format!(
                        "\
                        Int result = {{.value = {dividend}.value % {divisor}.value}};
                        return result;",
                        dividend = function.parameters[0].id,
                        divisor = function.parameters[1].id,
                    )),
                    BuiltinFunction::IntShiftLeft => self.push(format!(
                        "\
                        Int result = {{.value = {value}.value << {amount}.value}};
                        return result;",
                        value = function.parameters[0].id,
                        amount = function.parameters[1].id,
                    )),
                    BuiltinFunction::IntShiftRight => self.push(format!(
                        "\
                        Int result = {{.value = {value}.value >> {amount}.value}};
                        return result;",
                        value = function.parameters[0].id,
                        amount = function.parameters[1].id,
                    )),
                    BuiltinFunction::IntSubtract => self.push(format!(
                        "\
                        Int result = {{.value = {minuend}.value - {subtrahend}.value}};
                        return result;",
                        minuend = function.parameters[0].id,
                        subtrahend = function.parameters[1].id,
                    )),
                    BuiltinFunction::IntToText => self.push(format!(
                        "\
                        int length = snprintf(NULL, 0, \"%ld\", {int}.value);
                        Text result = {{.value = malloc(length + 1)}};
                        snprintf(result.value, length + 1, \"%ld\", {int}.value);
                        return result;",
                        int = function.parameters[0].id,
                    )),
                    BuiltinFunction::ListFilled => self.push(format!(
                        "\
                        if ({length}.value < 0) {{
                            char* message_format = \"List length must not be negative; was %ld.\";
                            int length = snprintf(NULL, 0, message_format, {length}.value);
                            Text message = {{.value = malloc(length + 1)}};
                            snprintf(message.value, length + 1, message_format, {length}.value);
                            builtinPanic$$Text(message);
                        }}

                        {list_type} result = {{
                            length = {length}.value,
                            values = malloc({length}.value * sizeof({item_type})),
                        }};
                        for (uint64_t i = 0; i < {length}.value; i++) {{
                            result.values[i] = {item};
                        }}
                        return result;",
                        item_type = substitutions["T"],
                        list_type = function.return_type,
                        length = function.parameters[0].id,
                        item = function.parameters[1].id,
                    )),
                    BuiltinFunction::ListGenerate => self.push(format!(
                        "\
                        if ({length}.value < 0) {{
                            char* message_format = \"List length must not be negative; was %ld.\";
                            int length = snprintf(NULL, 0, message_format, {length}.value);
                            Text message = {{.value = malloc(length + 1)}};
                            snprintf(message.value, length + 1, message_format, {length}.value);
                            builtinPanic$$Text(message);
                        }}

                        {list_type} result = {{
                            .length = {length}.value,
                            .values = malloc({length}.value * sizeof({item_type}*)),
                        }};
                        for (uint64_t i = 0; i < {length}.value; i++) {{
                            Int index = {{.value = i}};
                            result.values[i] = {item_getter}.function({item_getter}.closure, index);
                        }}
                        return result;",
                        item_type = substitutions["T"],
                        list_type = function.return_type,
                        length = function.parameters[0].id,
                        item_getter = function.parameters[1].id,
                    )),
                    BuiltinFunction::ListGet => self.push(format!(
                        "\
                        {return_type} result = malloc(sizeof({return_type}));
                        if (0 <= {index}.value && {index}.value < {list}.length) {{
                            result = {{
                                .variant = {return_type}_some,
                                .value.some = {list}.values[{index}.value],
                            }};
                        }} else {{
                            result = {{.variant = {return_type}_none}};
                        }}
                        return result;",
                        return_type = function.return_type,
                        list = function.parameters[0].id,
                        index = function.parameters[1].id,
                    )),
                    BuiltinFunction::ListInsert => self.push(format!(
                        "\
                        if (0 > {index}.value || {index}.value > {list}.length) {{
                            char* message_format = \"Index out of bounds: Tried inserting at index %ld in list of length %ld.\";
                            int length = snprintf(NULL, 0, message_format, {index}.value, {list}.length);
                            Text message = {{.value = malloc(length + 1)}};
                            snprintf(message.value, length + 1, message_format, {index}.value, {list}.length);
                            builtinPanic$$Text(message);
                        }}

                        uint64_t length = {list}.length + 1;
                        {list_type} result = {{
                            .length = length,
                            .values = malloc(length * sizeof({item_type})),
                        }};
                        memcpy(result.values, {list}.values, {index}.value * sizeof({item_type}*));
                        result.values[{index}.value] = {item};
                        memcpy(result.values + {index}.value + 1, {list}.values + {index}.value, ({list}.length - {index}.value) * sizeof({item_type}));
                        return result;",
                        item_type = substitutions["T"],
                        list_type = function.return_type,
                        list = function.parameters[0].id,
                        index = function.parameters[1].id,
                        item = function.parameters[2].id,
                    )),
                    BuiltinFunction::ListLength => self.push(format!(
                        "\
                        Int result = {{.value = {list}.length}};
                        return result;",
                        list = function.parameters[0].id,
                    )),
                    BuiltinFunction::ListOf0 => self.push(format!(
                        "\
                        {list_type} result = {{
                            .length = 0,
                            .values = NULL,
                        }};
                        return result;",
                        list_type = function.return_type,
                    )),
                    BuiltinFunction::ListOf1 => self.push(format!(
                        "\
                        {list_type} result = {{
                            .length = 1,
                            .values = malloc(sizeof({item_type})),
                        }};
                        result.values[0] = {item0};
                        return result;",
                        item_type = substitutions["T"],
                        list_type = function.return_type,
                        item0 = function.parameters[0].id,
                    )),
                    BuiltinFunction::ListOf2 => self.push(format!(
                        "\
                        {list_type} result = {{
                            .length = 2,
                            .values = malloc(2 * sizeof({item_type})),
                        }};
                        result.values[0] = {item0};
                        result.values[1] = {item1};
                        return result;",
                        item_type = substitutions["T"],
                        list_type = function.return_type,
                        item0 = function.parameters[0].id,
                        item1 = function.parameters[1].id,
                    )),
                    BuiltinFunction::ListOf3 => self.push(format!(
                        "\
                        {list_type} result = {{
                            .length = 3,
                            .values = malloc(3 * sizeof({item_type})),
                        }};
                        result.values[0] = {item0};
                        result.values[1] = {item1};
                        result.values[2] = {item2};
                        return result;",
                        item_type = substitutions["T"],
                        list_type = function.return_type,
                        item0 = function.parameters[0].id,
                        item1 = function.parameters[1].id,
                        item2 = function.parameters[2].id,
                    )),
                    BuiltinFunction::ListOf4 => self.push(format!(
                        "\
                        {list_type} result = {{
                            .length = 4,
                            .values = malloc(4 * sizeof({item_type})),
                        }};
                        result.values[0] = {item0};
                        result.values[1] = {item1};
                        result.values[2] = {item2};
                        result.values[3] = {item3};
                        return result;",
                        item_type = substitutions["T"],
                        list_type = function.return_type,
                        item0 = function.parameters[0].id,
                        item1 = function.parameters[1].id,
                        item2 = function.parameters[2].id,
                        item3 = function.parameters[3].id,
                    )),
                    BuiltinFunction::ListOf5 => self.push(format!(
                        "\
                        {list_type} result = {{
                            .length = 5,
                            .values = malloc(5 * sizeof({item_type})),
                        }};
                        result.values[0] = {item0};
                        result.values[1] = {item1};
                        result.values[2] = {item2};
                        result.values[3] = {item3};
                        result.values[4] = {item4};
                        return result;",
                        item_type = substitutions["T"],
                        list_type = function.return_type,
                        item0 = function.parameters[0].id,
                        item1 = function.parameters[1].id,
                        item2 = function.parameters[2].id,
                        item3 = function.parameters[3].id,
                        item4 = function.parameters[4].id,
                    )),
                    BuiltinFunction::ListRemoveAt => self.push(format!(
                        "\
                        if (0 > {index}.value || {index}.value >= {list}.length) {{
                            char* message_format = \"Index out of bounds: Tried removing item at index %ld from list of length %ld.\";
                            int length = snprintf(NULL, 0, message_format, {index}.value, {list}.length);
                            Text message = {{.value = malloc(length + 1)}};
                            snprintf(message.value, length + 1, message_format, {index}.value, {list}.length);
                            builtinPanic$$Text(message);
                        }}

                        uint64_t length = {list}.length - 1;
                        {list_type} result = {{
                            .length = length,
                            .values = malloc(length * sizeof({item_type})),
                        }};
                        memcpy(result.values, {list}.values, {index}.value * sizeof({item_type}*));
                        memcpy(result.values + {index}.value, {list}.values + {index}.value + 1, ({list}.length - {index}.value - 1) * sizeof({item_type}*));
                        return result;",
                        item_type = substitutions["T"],
                        list_type = function.return_type,
                        list = function.parameters[0].id,
                        index = function.parameters[1].id,
                    )),
                    BuiltinFunction::ListReplace => self.push(format!(
                        "\
                        if (0 > {index}.value || {index}.value >= {list}.length) {{
                            char* message_format = \"Index out of bounds: Tried replacing index %ld in list of length %ld.\";
                            int length = snprintf(NULL, 0, message_format, {index}.value, {list}.length);
                            Text message = {{.value = malloc(length + 1)}};
                            snprintf(message.value, length + 1, message_format, {index}.value, {list}.length);
                            builtinPanic$$Text(message);
                        }}

                        {list_type} result = {{
                            .length = {list}.length,
                            .values = malloc({list}.length * sizeof({item_type})),
                        }};
                        memcpy(result.values, {list}.values, {list}.length * sizeof({item_type}*));
                        result.values[{index}.value] = {new_item};
                        return result;",
                        item_type = substitutions["T"],
                        list_type = function.return_type,
                        list = function.parameters[0].id,
                        index = function.parameters[1].id,
                        new_item = function.parameters[2].id,
                    )),
                    BuiltinFunction::Panic => {
                        self.push(format!(
                            "\
                            fputs({}.value, stderr);
                            exit(1);",
                            function.parameters[0].id,
                        ));
                    }
                    BuiltinFunction::Print => {
                        self.push(format!(
                            "\
                            puts({}.value);
                            Nothing _1 = {{}};
                            return _1;",
                            function.parameters[0].id,
                        ));
                    }
                    BuiltinFunction::TextCompareTo => self.push(format!(
                        "\
                        int raw_result = strcmp({a}.value, {b}.value);
                        Ordering result = {{.variant = raw_result < 0    ? Ordering_less
                                                    : raw_result == 0 ? Ordering_equal
                                                                      : Ordering_greater}};
                        return result;",
                        a = function.parameters[0].id,
                        b = function.parameters[1].id,
                    )),
                    BuiltinFunction::TextConcat => self.push(format!(
                        "\
                        size_t lengthA = strlen({a}.value);\n\
                        size_t lengthB = strlen({b}.value);\n\
                        Text result = {{.value = malloc(lengthA + lengthB + 1)}};\n\
                        memcpy(result.value, {a}.value, lengthA);\n\
                        memcpy(result.value + lengthA, {b}.value, lengthB + 1);\n\
                        return result;",
                        a = function.parameters[0].id,
                        b = function.parameters[1].id,
                    )),
                    BuiltinFunction::TextGetRange => self.push(format!(
                        "\
                        size_t text_length = strlen({text}.value);
                        if (0 > {start_inclusive}.value || {start_inclusive}.value > text_length
                            || 0 > {end_exclusive}.value || {end_exclusive}.value > text_length) {{
                            char* message_format = \"Index out of bounds: Tried getting range %ld..%ld from text that is only %ld long.\";
                            int length = snprintf(NULL, 0, message_format, {start_inclusive}.value, {end_exclusive}.value, text_length);
                            Text message = {{.value = malloc(length + 1)}};
                            snprintf(message.value, length + 1, message_format, {start_inclusive}.value, {end_exclusive}.value, text_length);
                            builtinPanic$$Text(message);
                        }} else if ({start_inclusive}.value > {end_exclusive}.value) {{
                            char* message_format = \"Invalid range %ld..%ld: `start_inclusive` must be less than or equal to `end_exclusive`.\";
                            int length = snprintf(NULL, 0, message_format, {start_inclusive}.value, {end_exclusive}.value);
                            Text message = {{.value = malloc(length + 1)}};
                            snprintf(message.value, length + 1, message_format, {start_inclusive}.value, {end_exclusive}.value);
                            builtinPanic$$Text(message);
                        }}

                        size_t length = {end_exclusive}.value - {start_inclusive}.value;\n\
                        Text result = {{.value = malloc(length + 1)}};\n\
                        memcpy(result.value, {text}.value + {start_inclusive}.value, length);\n\
                        return result;",
                        text = function.parameters[0].id,
                        start_inclusive = function.parameters[1].id,
                        end_exclusive = function.parameters[2].id,
                    )),
                    BuiltinFunction::TextIndexOf => self.push(format!(
                        "\
                        {return_type} result;
                        char* result_raw = strstr({a}.value, {b}.value);
                        if (result_raw == NULL) {{
                            result = {{.variant = {return_type}_none}};
                        }} else {{
                            result = {{
                                .variant = {return_type}_some,
                                .value.some = {{.value = result_raw - {a}.value}},
                            }};
                        }}
                        return result;",
                        a = function.parameters[0].id,
                        b = function.parameters[1].id,
                        return_type = function.return_type,
                    )),
                    BuiltinFunction::TextLength => self.push(format!(
                        "\
                        Int result = {{.value = strlen({text}.value)}};
                        return result;",
                        text = function.parameters[0].id,
                    )),
                }
            }
            BodyOrBuiltin::Body(body) => self.lower_body(declaration_name, body),
        }
    }
    fn lower_body(&mut self, declaration_name: &str, body: &Body) {
        self.lower_body_expressions(declaration_name, body);
        self.push(format!("return {};", body.return_value_id()));
    }
    fn lower_body_expressions(&mut self, declaration_name: &str, body: &Body) {
        for (id, name, expression) in &body.expressions {
            if let Some(name) = name {
                self.push(format!("// {name}\n"));
            }

            self.lower_expression(declaration_name, *id, expression);
            if &*expression.type_ == "Never" {
                self.push("// Returns `Never`\n");
            }
            self.push("\n\n");
        }
    }
    fn lower_expression(&mut self, declaration_name: &str, id: Id, expression: &Expression) {
        match &expression.kind {
            ExpressionKind::Int(int) => {
                self.push(format!("{} {id} = {{.value = {int} }};", &expression.type_));
            }
            ExpressionKind::Text(text) => {
                self.push(format!(
                    "{} {id} = {{.value = \"{}\"}};",
                    &expression.type_,
                    text.escape_default(),
                ));
            }
            ExpressionKind::CreateStruct { struct_, fields } => {
                let TypeDeclaration::Struct {
                    fields: type_fields,
                } = &self.mono.type_declarations[struct_]
                else {
                    unreachable!();
                };

                self.push(format!("{} {id} = {{", &expression.type_));
                for ((name, _), value) in type_fields.iter().zip_eq(fields.iter()) {
                    self.push(format!("\n.{name} = {value},"));
                }
                self.push("};");
            }
            ExpressionKind::StructAccess { struct_, field } => {
                self.push(format!("{} {id} = {struct_}.{field};", &expression.type_));
            }
            ExpressionKind::CreateEnum {
                enum_,
                variant,
                value,
            } => {
                if self.get_boxed_variants(enum_).contains(variant) {
                    let TypeDeclaration::Enum { variants } = &self.mono.type_declarations[enum_]
                    else {
                        unreachable!()
                    };
                    let value_type = variants
                        .iter()
                        .find(|it| &it.name == variant)
                        .as_ref()
                        .unwrap()
                        .value_type
                        .as_ref()
                        .unwrap();
                    self.push(format!(
                        "\
                        {value_type}* {id}_value_boxed = malloc(sizeof({value_type}*));
                        *{id}_value_boxed = {value};
                        {type_} {id} = {{
                            .variant = {enum_}_{variant},
                            .value.{variant} = {id}_value_boxed,
                        }};",
                        value = value.unwrap(),
                        type_ = &expression.type_,
                    ));
                } else {
                    self.push(format!(
                        "{} {id} = {{.variant = {enum_}_{variant}",
                        &expression.type_,
                    ));
                    if let Some(value) = value {
                        self.push(format!("\n.value.{variant} = {value};"));
                    }
                    self.push("};");
                }
            }
            ExpressionKind::GlobalAssignmentReference(assignment) => {
                self.push(format!("{} {id} = {assignment};", &expression.type_));
            }
            ExpressionKind::LocalReference(referenced_id) => {
                self.push(format!("{} {id} = {referenced_id};", &expression.type_));
            }
            ExpressionKind::CallFunction {
                function,
                arguments,
            } => {
                self.push(format!("{} {id} = {function}(", &expression.type_));
                for (index, argument) in arguments.iter().enumerate() {
                    if index > 0 {
                        self.push(", ");
                    }
                    self.push(format!("{argument}"));
                }
                self.push(");");
            }
            ExpressionKind::CallLambda { lambda, arguments } => {
                self.push(format!(
                    "{} {id} = {lambda}.function({lambda}.closure",
                    &expression.type_
                ));
                for argument in &**arguments {
                    self.push(format!(", {argument}"));
                }
                self.push(");");
            }
            ExpressionKind::Switch {
                value,
                enum_,
                cases,
            } => {
                self.push(format!("{} {id};\n", &expression.type_));

                self.push(format!("switch ({value}.variant) {{"));
                for case in &**cases {
                    self.push(format!("case {enum_}_{}:\n", case.variant));
                    if let Some((value_id, value_type)) = &case.value {
                        self.push(format!(
                            "{value_type} {value_id} = {}{value}.value.{};\n",
                            if self.get_boxed_variants(enum_).contains(&case.variant) {
                                "*"
                            } else {
                                ""
                            },
                            case.variant,
                        ));
                    }

                    self.lower_body_expressions(declaration_name, &case.body);
                    if case.body.return_type() == "Never" {
                        self.push("// Returns `Never`\n");
                    } else {
                        self.push(format!("{id} = {};\n", case.body.return_value_id()));
                    }

                    self.push("break;");
                }
                self.push("}");
            }
            ExpressionKind::Lambda(lambda) => {
                self.push(format!("{declaration_name}$lambda{id}_closure* {id}_closure = malloc(sizeof({declaration_name}$lambda{id}_closure));\n",));
                for referenced_id in lambda.closure().iter().sorted() {
                    self.push(format!(
                        "{id}_closure->{referenced_id} = {referenced_id};\n"
                    ));
                }
                self.push(format!(
                    "\
                    {type_} {id} = {{
                        .closure = {id}_closure,
                        .function = {declaration_name}$lambda{id}_function,
                    }};",
                    type_ = &expression.type_,
                ));
            }
        }
    }

    fn push(&mut self, s: impl AsRef<str>) {
        self.c.push_str(s.as_ref());
    }
}
