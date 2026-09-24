//! SILICON — a scratch-built 8-bit computer.
//!
//! Public library surface: instruction set (`isa`), virtual machine (`cpu`),
//! assembler, disassembler and screen renderer (`shadow`). The CLI lives in
//! `main.rs` and uses nothing but these modules.
//!
//! Zero external dependencies: everything is built with the Rust standard
//! library only.

pub mod assembler;
pub mod cpu;
pub mod disasm;
pub mod isa;
pub mod shadow;

pub use assembler::{assemble, assemble_str, AssembleError, Program};
pub use cpu::{Cpu, KEY_PORT, ScreenPlane, TICK_PORT, VC_CTRL, VC_CONSOLE};
pub use isa::{Instr, Mode};