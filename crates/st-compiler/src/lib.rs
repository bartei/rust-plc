//! AST-to-IR lowering for IEC 61131-3 Structured Text.
//!
//! Compiles typed AST nodes into bytecode instructions, handling expression
//! evaluation, control flow, and function block calls.

mod compile;

pub use compile::{compile, compile_with_native_fbs, CompileError};
