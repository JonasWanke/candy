use crate::{
    builtin_functions::BuiltinFunction,
    compiler::hir::{self, Expression},
    input::InputReference,
};

use super::{
    run::{Discover, DiscoverResult},
    value::{Environment, Value},
};

pub fn run_builtin_function(
    db: &dyn Discover,
    input_reference: InputReference,
    builtin_function: BuiltinFunction,
    arguments: Vec<hir::Id>,
    environment: Environment,
) -> DiscoverResult {
    log::info!(
        "run_builtin_function: {:?} {}",
        builtin_function,
        arguments.len()
    );
    // Handle builtin functions that don't need to resolve the
    match builtin_function {
        BuiltinFunction::Call0 => return call0(db, input_reference, arguments, environment),
        BuiltinFunction::Call1 => return call1(db, input_reference, arguments, environment),
        BuiltinFunction::Call2 => return call2(db, input_reference, arguments, environment),
        BuiltinFunction::Call3 => return call3(db, input_reference, arguments, environment),
        BuiltinFunction::Call4 => return call4(db, input_reference, arguments, environment),
        BuiltinFunction::Call5 => return call5(db, input_reference, arguments, environment),
        BuiltinFunction::IfElse => return if_else(db, input_reference, arguments, environment),
        _ => {}
    }

    let arguments =
        db.run_multiple_with_environment(input_reference.to_owned(), arguments, environment)?;
    let arguments = match arguments {
        Ok(arguments) => arguments,
        Err(error) => return Some(Err(error)),
    };
    match builtin_function {
        BuiltinFunction::Add => Some(Ok(add(arguments))),
        BuiltinFunction::Equals => Some(Ok(equals(arguments))),
        BuiltinFunction::GetArgumentCount => get_argument_count(db, input_reference, arguments),
        BuiltinFunction::Panic => panic(arguments),
        BuiltinFunction::Print => Some(Ok(print(arguments))),
        BuiltinFunction::TypeOf => Some(Ok(type_of(arguments))),
        _ => panic!("Unhandled builtin function: {:?}", builtin_function),
    }
}

macro_rules! destructure {
    ($arguments:expr, $enum:pat, $body:block) => {{
        if let $enum = &$arguments[..] {
            $body
        } else {
            panic!()
        }
    }};
}

macro_rules! generate_call {
    ($function_name:ident $(, $argument_names:ident)*) => {
        fn $function_name(
    db: &dyn Discover,
    input_reference: InputReference,arguments: Vec<hir::Id>, environment: Environment) -> DiscoverResult
        {
            destructure!(arguments, [function, $($argument_names),*], {
                db.run_call(input_reference, function.to_owned(), vec![$($argument_names.clone()),*], environment)
            })
      }
    };
}

fn add(arguments: Vec<Value>) -> Value {
    destructure!(arguments, [Value::Int(a), Value::Int(b)], {
        Value::Int(a + b)
    })
}

generate_call!(call0);
generate_call!(call1, argument0);
generate_call!(call2, argument0, argument1);
generate_call!(call3, argument0, argument1, argument2);
generate_call!(call4, argument0, argument1, argument2, argument3);
generate_call!(call5, argument0, argument1, argument2, argument3, argument4);

fn equals(arguments: Vec<Value>) -> Value {
    destructure!(arguments, [a, b], { (a == b).into() })
}

fn get_argument_count(
    db: &dyn Discover,
    input_reference: InputReference,
    arguments: Vec<Value>,
) -> DiscoverResult {
    destructure!(arguments, [Value::Lambda(function)], {
        // TODO: support parameter counts > 2^64 on 128-bit systems and better
        let expression = match db.find_expression(input_reference, function.id.to_owned())? {
            Expression::Lambda(lambda) => lambda,
            _ => return None,
        };
        Some(Ok((expression.parameters.len() as u64).into()))
    })
}

fn if_else(
    db: &dyn Discover,
    input_reference: InputReference,
    arguments: Vec<hir::Id>,
    environment: Environment,
) -> DiscoverResult {
    if let [condition, then, else_] = &arguments[..] {
        let body_id = match db.run_with_environment(
            input_reference.clone(),
            condition.to_owned(),
            environment.to_owned(),
        )? {
            Ok(value) if value == Value::bool_true() => then,
            Ok(value) if value == Value::bool_false() => else_,
            Ok(_) => return None,
            Err(error) => return Some(Err(error)),
        };

        db.run_call(input_reference, body_id.to_owned(), vec![], environment)
    } else {
        panic!()
    }
}

fn panic(arguments: Vec<Value>) -> DiscoverResult {
    destructure!(arguments, [value], { Some(Err(value.clone())) })
}

fn print(arguments: Vec<Value>) -> Value {
    destructure!(arguments, [value], {
        println!("{:?}", value);
        Value::nothing()
    })
}

fn type_of(arguments: Vec<Value>) -> Value {
    destructure!(arguments, [value], {
        match value {
            Value::Int(_) => Value::Symbol("Int".to_owned()),
            Value::Text(_) => Value::Symbol("Text".to_owned()),
            Value::Symbol(_) => Value::Symbol("Symbol".to_owned()),
            Value::Lambda(_) => Value::Symbol("Function".to_owned()),
        }
    })
}

pub trait DestructureTuple<T> {
    fn tuple2(self, function_name: &str) -> Result<(T, T), Value>;
}
impl<T> DestructureTuple<T> for Vec<T> {
    fn tuple2(self, function_name: &str) -> Result<(T, T), Value> {
        if self.len() != 2 {
            Err(Value::argument_count_mismatch_text(
                function_name,
                self.len(),
                2,
            ))
        } else {
            let mut iter = self.into_iter();
            let first = iter.next().unwrap();
            let second = iter.next().unwrap();
            assert!(matches!(iter.next(), None));
            Ok((first, second))
        }
    }
}
