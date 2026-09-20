//! Dodo's frontend and optional native LLVM backend.
//!
//! Disable default features to use the parser, checker, package loader, formatter,
//! and editor analysis without LLVM. The `llvm` feature enables native codegen
//! and the LSP server, which uses LLVM to validate compilation targets.
pub mod ast;
pub mod cli;
#[cfg(feature = "llvm")]
pub mod codegen;
pub mod consteval;
pub mod diagnostic;
pub mod editor;
pub mod format;
pub mod json;
mod json_derive;
pub mod lexer;
#[cfg(feature = "llvm")]
pub mod lsp;
pub mod package;
pub mod parser;
pub mod project;
pub mod sema;
pub mod toml;

#[cfg(any(feature = "llvm", test))]
mod file_uri;
mod prepare;
