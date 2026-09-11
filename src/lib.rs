//! Dodo's parser, semantic analysis, and native LLVM backend.
pub mod ast;
pub mod codegen;
pub mod consteval;
pub mod diagnostic;
pub mod editor;
pub mod format;
pub mod lexer;
pub mod package;
pub mod parser;
pub mod sema;

mod prepare;
