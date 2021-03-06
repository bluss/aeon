//! A parser for Aeon bytecode streams
//!
//! This module provides various functions that can be used for parsing Aeon
//! bytecode files provided as a stream of bytes.
//!
//! To parse a stream of bytes you can use the `parse` function:
//!
//!     let mut bytes = File::open("path/to/file.aeonc").unwrap().bytes();
//!     let result = bytecode_parser::parse(&mut bytes);
//!
//! Alternatively you can also parse a file directly:
//!
//!     let result = bytecode_parser::parse_file("path/to/file.aeonc");

use std::io::prelude::*;
use std::io::Bytes;
use std::fs::File;
use std::mem;
use std::sync::Arc;

use compiled_code::{CompiledCode, RcCompiledCode};
use instruction::{InstructionType, Instruction};

macro_rules! parser_error {
    ($variant: ident) => (
        return Err(ParserError::$variant);
    );
}

macro_rules! try_byte {
    ($expr: expr, $variant: ident) => (
        match $expr {
            Some(result) => {
                match result {
                    Ok(byte) => byte,
                    Err(_)   => parser_error!($variant)
                }
            },
            None => parser_error!($variant)
        }
    );
}

macro_rules! read_string_vector {
    ($byte_type: ident, $bytes: expr) => (
        try!(read_vector::<String, $byte_type>($bytes, read_string));
    );
}

macro_rules! read_u32_vector {
    ($byte_type: ident, $bytes: expr) => (
        try!(read_vector::<u32, $byte_type>($bytes, read_u32));
    );
}

macro_rules! read_i64_vector {
    ($byte_type: ident, $bytes: expr) => (
        try!(read_vector::<i64, $byte_type>($bytes, read_i64));
    );
}

macro_rules! read_f64_vector {
    ($byte_type: ident, $bytes: expr) => (
        try!(read_vector::<f64, $byte_type>($bytes, read_f64));
    );
}

macro_rules! read_instruction_vector {
    ($byte_type: ident, $bytes: expr) => (
        try!(read_vector::<Instruction, $byte_type>($bytes,
                                                    read_instruction));
    );
}

macro_rules! read_code_vector {
    ($byte_type: ident, $bytes: expr) => (
        try!(read_vector::<RcCompiledCode, $byte_type>($bytes,
                                                       read_compiled_code));
    );
}

const SIGNATURE_BYTES: [u8; 4] = [97, 101, 111, 110]; // "aeon"

const VERSION: u8 = 1;

#[derive(Debug)]
pub enum ParserError {
    InvalidFile,
    InvalidSignature,
    InvalidVersion,
    InvalidString,
    InvalidInteger,
    InvalidFloat,
    MissingByte,
}

pub type ParserResult<T> = Result<T, ParserError>;
pub type BytecodeResult = ParserResult<RcCompiledCode>;

/// Parses a file
///
/// # Examples
///
///     let result = bytecode_parser::parse_file("path/to/file.aeonc");
pub fn parse_file(path: &str) -> BytecodeResult {
    match File::open(path) {
        Ok(file) => parse(&mut file.bytes()),
        Err(_) => parser_error!(InvalidFile),
    }
}

/// Parses a stream of bytes
///
/// # Examples
///
///     let mut bytes = File::open("path/to/file.aeonc").unwrap().bytes();
///     let result = bytecode_parser::parse(&mut bytes);
pub fn parse<T: Read>(bytes: &mut Bytes<T>) -> BytecodeResult {
    // Verify the bytecode signature.
    for expected in SIGNATURE_BYTES.iter() {
        let byte = try_byte!(bytes.next(), InvalidSignature);

        if byte != *expected {
            parser_error!(InvalidSignature);
        }
    }

    // Verify the version
    if try_byte!(bytes.next(), InvalidVersion) != VERSION {
        parser_error!(InvalidVersion);
    }

    let code = try!(read_compiled_code(bytes));

    Ok(code)
}

fn read_string<T: Read>(bytes: &mut Bytes<T>) -> ParserResult<String> {
    let size = try!(read_u64(bytes));

    let mut buff: Vec<u8> = Vec::new();

    for _ in 0..size {
        buff.push(try_byte!(bytes.next(), InvalidString));
    }

    match String::from_utf8(buff) {
        Ok(string) => Ok(string),
        Err(_) => parser_error!(InvalidString),
    }
}

fn read_u8<T: Read>(bytes: &mut Bytes<T>) -> ParserResult<u8> {
    let byte = try_byte!(bytes.next(), InvalidInteger);

    let value: u8 = unsafe { mem::transmute([byte]) };

    Ok(u8::from_be(value))
}

fn read_u16<T: Read>(bytes: &mut Bytes<T>) -> ParserResult<u16> {
    let mut buff: [u8; 2] = [0, 0];

    for index in 0..2 {
        buff[index] = try_byte!(bytes.next(), InvalidInteger);
    }

    let value: u16 = unsafe { mem::transmute(buff) };

    Ok(u16::from_be(value))
}

fn read_i32<T: Read>(bytes: &mut Bytes<T>) -> ParserResult<i32> {
    let mut buff: [u8; 4] = [0, 0, 0, 0];

    for index in 0..4 {
        buff[index] = try_byte!(bytes.next(), InvalidInteger);
    }

    let value: i32 = unsafe { mem::transmute(buff) };

    Ok(i32::from_be(value))
}

fn read_u32<T: Read>(bytes: &mut Bytes<T>) -> ParserResult<u32> {
    Ok(try!(read_i32(bytes)) as u32)
}

fn read_i64<T: Read>(bytes: &mut Bytes<T>) -> ParserResult<i64> {
    let mut buff: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];

    for index in 0..8 {
        buff[index] = try_byte!(bytes.next(), InvalidInteger);
    }

    let value: i64 = unsafe { mem::transmute(buff) };

    Ok(i64::from_be(value))
}

fn read_u64<T: Read>(bytes: &mut Bytes<T>) -> ParserResult<u64> {
    Ok(try!(read_i64(bytes)) as u64)
}

fn read_f64<T: Read>(bytes: &mut Bytes<T>) -> ParserResult<f64> {
    let mut buff: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];

    for index in 0..8 {
        buff[index] = try_byte!(bytes.next(), InvalidFloat);
    }

    let int: u64 = u64::from_be(unsafe { mem::transmute(buff) });
    let float: f64 = unsafe { mem::transmute(int) };

    Ok(float)
}

fn read_vector<V, T: Read>(bytes: &mut Bytes<T>,
                           reader: fn(&mut Bytes<T>) -> ParserResult<V>)
                           -> ParserResult<Vec<V>> {
    let amount = try!(read_u64(bytes));

    let mut buff: Vec<V> = Vec::new();

    for _ in 0..amount {
        buff.push(try!(reader(bytes)));
    }

    Ok(buff)
}

fn read_instruction<T: Read>(bytes: &mut Bytes<T>) -> ParserResult<Instruction> {
    let ins_type: InstructionType =
        unsafe { mem::transmute(try!(read_u16(bytes))) };

    let args = read_u32_vector!(T, bytes);
    let line = try!(read_u32(bytes));
    let column = try!(read_u32(bytes));
    let ins = Instruction::new(ins_type, args, line, column);

    Ok(ins)
}

fn read_compiled_code<T: Read>(bytes: &mut Bytes<T>)
                               -> ParserResult<RcCompiledCode> {
    let name = try!(read_string(bytes));
    let file = try!(read_string(bytes));
    let line = try!(read_u32(bytes));
    let args = try!(read_u32(bytes));
    let req_args = try!(read_u32(bytes));
    let rest_arg = try!(read_u8(bytes)) == 1;

    let locals = read_string_vector!(T, bytes);
    let instructions = read_instruction_vector!(T, bytes);
    let int_literals = read_i64_vector!(T, bytes);
    let float_literals = read_f64_vector!(T, bytes);
    let str_literals = read_string_vector!(T, bytes);
    let code_objects = read_code_vector!(T, bytes);

    let code_obj = CompiledCode {
        name: name,
        file: file,
        line: line,
        arguments: args,
        required_arguments: req_args,
        rest_argument: rest_arg,
        locals: locals,
        instructions: instructions,
        integer_literals: int_literals,
        float_literals: float_literals,
        string_literals: str_literals,
        code_objects: code_objects,
    };

    Ok(Arc::new(code_obj))
}

#[cfg(test)]
mod tests {
    use instruction::InstructionType;
    use std::io::prelude::*;
    use std::mem;

    macro_rules! unwrap {
        ($expr: expr) => ({
            match $expr {
                Ok(value)  => value,
                Err(error) => panic!("Failed to parse input: {:?}", error)
            }
        });
    }

    macro_rules! read {
        ($name: ident, $buffer: expr) => (
            super::$name(&mut $buffer.bytes())
        );
    }

    macro_rules! pack_u8 {
        ($num: expr, $buffer: expr) => ({
            let num   = u8::to_be($num);
            let bytes = [num];

            $buffer.extend_from_slice(&bytes);
        });
    }

    macro_rules! pack_u16 {
        ($num: expr, $buffer: expr) => ({
            let num = u16::to_be($num);
            let bytes: [u8; 2] = unsafe { mem::transmute(num) };

            $buffer.extend_from_slice(&bytes);
        });
    }

    macro_rules! pack_u32 {
        ($num: expr, $buffer: expr) => ({
            let num = u32::to_be($num);
            let bytes: [u8; 4] = unsafe { mem::transmute(num) };

            $buffer.extend_from_slice(&bytes);
        });
    }

    macro_rules! pack_u64 {
        ($num: expr, $buffer: expr) => ({
            let num = u64::to_be($num);
            let bytes: [u8; 8] = unsafe { mem::transmute(num) };

            $buffer.extend_from_slice(&bytes);
        });
    }

    macro_rules! pack_f64 {
        ($num: expr, $buffer: expr) => ({
            let int: u64 = unsafe { mem::transmute($num) };

            pack_u64!(int, $buffer);
        });
    }

    macro_rules! pack_string {
        ($string: expr, $buffer: expr) => ({
            pack_u64!($string.len() as u64, $buffer);

            $buffer.extend_from_slice(&$string.as_bytes());
        });
    }

    #[test]
    fn test_parse_empty() {
        let buffer = Vec::new();
        let output = super::parse(&mut buffer.bytes());

        assert!(output.is_err());
    }

    #[test]
    fn test_parse_invalid_signature() {
        let mut buffer = Vec::new();

        pack_string!("cats", buffer);

        let output = super::parse(&mut buffer.bytes());

        assert!(output.is_err());
    }

    #[test]
    fn test_parse_invalid_version() {
        let mut buffer = Vec::new();

        buffer.push(97);
        buffer.push(101);
        buffer.push(111);
        buffer.push(110);

        buffer.push(super::VERSION + 1);

        let output = super::parse(&mut buffer.bytes());

        assert!(output.is_err());
    }

    #[test]
    fn test_parse() {
        let mut buffer = Vec::new();

        buffer.push(97);
        buffer.push(101);
        buffer.push(111);
        buffer.push(110);

        buffer.push(super::VERSION);

        pack_string!("main", buffer);
        pack_string!("test.aeon", buffer);
        pack_u32!(4, buffer); // line
        pack_u32!(0, buffer); // arguments
        pack_u32!(0, buffer); // required arguments
        pack_u8!(0, buffer); // rest argument
        pack_u64!(0, buffer); // locals
        pack_u64!(0, buffer); // instructions
        pack_u64!(0, buffer); // integer literals
        pack_u64!(0, buffer); // float literals
        pack_u64!(0, buffer); // string literals
        pack_u64!(0, buffer); // code objects

        let object = unwrap!(super::parse(&mut buffer.bytes()));

        assert_eq!(object.name, "main".to_string());
        assert_eq!(object.file, "test.aeon".to_string());
        assert_eq!(object.line, 4);
    }

    #[test]
    fn test_read_string() {
        let mut buffer = Vec::new();

        pack_string!("aeon", buffer);

        let output = unwrap!(read!(read_string, buffer));

        assert_eq!(output, "aeon".to_string());
    }

    #[test]
    fn test_read_string_longer_than_size() {
        let mut buffer = Vec::new();

        pack_u64!(2, buffer);

        buffer.extend_from_slice(&"aeon".as_bytes());

        let output = unwrap!(read!(read_string, buffer));

        assert_eq!(output, "ae".to_string());
    }

    #[test]
    fn test_read_string_invalid_utf8() {
        let mut buffer = Vec::new();
        let bytes: [u8; 4] = [0, 159, 146, 150];

        pack_u64!(4, buffer);

        buffer.extend_from_slice(&bytes);

        let output = read!(read_string, buffer);

        assert!(output.is_err());
    }

    #[test]
    fn test_read_string_empty() {
        let output = read!(read_string, []);

        assert!(output.is_err());
    }

    #[test]
    fn test_read_u8() {
        let mut buffer = Vec::new();

        pack_u8!(2, buffer);

        let output = unwrap!(read!(read_u8, buffer));

        assert_eq!(output, 2);
    }

    #[test]
    fn test_read_u8_empty() {
        let output = read!(read_u8, []);

        assert!(output.is_err());
    }

    #[test]
    fn test_read_u16() {
        let mut buffer = Vec::new();

        pack_u16!(2, buffer);

        let output = unwrap!(read!(read_u16, buffer));

        assert_eq!(output, 2);
    }

    #[test]
    fn test_read_u16_empty() {
        let output = read!(read_u16, []);

        assert!(output.is_err());
    }

    #[test]
    fn test_read_i32() {
        let mut buffer = Vec::new();

        pack_u32!(2, buffer);

        let output = unwrap!(read!(read_i32, buffer));

        assert_eq!(output, 2);
    }

    #[test]
    fn test_read_i32_empty() {
        let output = read!(read_i32, []);

        assert!(output.is_err());
    }

    #[test]
    fn test_read_u32() {
        let mut buffer = Vec::new();

        pack_u32!(2, buffer);

        let output = unwrap!(read!(read_u32, buffer));

        assert_eq!(output, 2);
    }

    #[test]
    fn test_read_i64() {
        let mut buffer = Vec::new();

        pack_u64!(2, buffer);

        let output = unwrap!(read!(read_i64, buffer));

        assert_eq!(output, 2);
    }

    #[test]
    fn test_read_i64_empty() {
        let output = read!(read_i64, []);

        assert!(output.is_err());
    }

    #[test]
    fn test_read_u64() {
        let mut buffer = Vec::new();

        pack_u64!(2, buffer);

        let output = unwrap!(read!(read_u64, buffer));

        assert_eq!(output, 2);
    }

    #[test]
    fn test_read_f64() {
        let mut buffer = Vec::new();

        pack_f64!(2.123456, buffer);

        let output = unwrap!(read!(read_f64, buffer));

        assert!((2.123456 - output).abs() < 0.00001);
    }

    #[test]
    fn test_read_f64_empty() {
        let output = read!(read_f64, []);

        assert!(output.is_err());
    }

    #[test]
    fn test_read_vector() {
        let mut buffer = Vec::new();

        pack_u64!(2, buffer);
        pack_string!("hello", buffer);
        pack_string!("world", buffer);

        let output = unwrap!(super::read_vector::<String,
                                                  &[u8]>(&mut buffer.bytes(),
                                                         super::read_string));

        assert_eq!(output.len(), 2);
        assert_eq!(output[0], "hello".to_string());
        assert_eq!(output[1], "world".to_string());
    }

    #[test]
    fn test_read_vector_empty() {
        let buffer = Vec::new();
        let output = super::read_vector::<String, &[u8]>(&mut buffer.bytes(),
                                                         super::read_string);

        assert!(output.is_err());
    }

    #[test]
    fn test_read_instruction() {
        let mut buffer = Vec::new();

        pack_u16!(0, buffer); // type
        pack_u64!(1, buffer); // args
        pack_u32!(6, buffer);
        pack_u32!(2, buffer); // line
        pack_u32!(4, buffer); // column

        let ins = unwrap!(super::read_instruction(&mut buffer.bytes()));

        match ins.instruction_type {
            InstructionType::SetInteger => {}
            _ => panic!("expected SetInteger, not {:?}", ins.instruction_type),
        };

        assert_eq!(ins.arguments[0], 6);
        assert_eq!(ins.line, 2);
        assert_eq!(ins.column, 4);
    }

    #[test]
    fn test_read_compiled_code() {
        let mut buffer = Vec::new();

        pack_string!("main", buffer); // name
        pack_string!("test.aeon", buffer); // file
        pack_u32!(4, buffer); // line
        pack_u32!(3, buffer); // arguments
        pack_u32!(2, buffer); // required args
        pack_u8!(1, buffer); // rest argument
        pack_u64!(0, buffer); // locals

        pack_u64!(1, buffer); // instructions
        pack_u16!(0, buffer); // type
        pack_u64!(1, buffer); // args
        pack_u32!(6, buffer);
        pack_u32!(2, buffer); // line
        pack_u32!(4, buffer); // column

        pack_u64!(1, buffer); // integer literals
        pack_u64!(10, buffer);

        pack_u64!(1, buffer); // float literals
        pack_f64!(1.2, buffer);

        pack_u64!(1, buffer); // string literals
        pack_string!("foo", buffer);

        pack_u64!(0, buffer); // code objects

        let object = unwrap!(super::read_compiled_code(&mut buffer.bytes()));

        assert_eq!(object.name, "main".to_string());
        assert_eq!(object.file, "test.aeon".to_string());
        assert_eq!(object.line, 4);
        assert_eq!(object.arguments, 3);
        assert_eq!(object.required_arguments, 2);
        assert_eq!(object.rest_argument, true);

        assert_eq!(object.locals.len(), 0);

        assert_eq!(object.instructions.len(), 1);

        let ref ins = object.instructions[0];

        match ins.instruction_type {
            InstructionType::SetInteger => {}
            _ => panic!("expected SetInteger, not {:?}", ins.instruction_type),
        };

        assert_eq!(ins.arguments[0], 6);
        assert_eq!(ins.line, 2);
        assert_eq!(ins.column, 4);

        assert_eq!(object.integer_literals.len(), 1);
        assert_eq!(object.integer_literals[0], 10);

        assert_eq!(object.float_literals.len(), 1);
        assert!((object.float_literals[0] - 1.2).abs() < 0.001);

        assert_eq!(object.string_literals.len(), 1);
        assert_eq!(object.string_literals[0], "foo".to_string());

        assert_eq!(object.code_objects.len(), 0);
    }
}
