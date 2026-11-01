//! What Luau turns a script into, as text: the bytecode listing, the
//! compiler's remarks, and the native code generator's IR and assembly for
//! any of its targets.
//!
//! It's the compiler's own output, through a small C++ shim over the Luau
//! sources mlua builds (`src/shim.cpp`), with the dump flags Luau's own
//! `luau-compile` uses for the same views.

use std::ffi::{c_char, c_int};

// Nothing here names mlua-sys, but the shim needs the Luau libraries it
// links, and rustc drops a dependency nobody names.
extern crate mlua_sys;

/// Which text to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum View {
    /// The bytecode listing with the source interleaved (`--text`).
    Bytecode,
    /// The source with the compiler's remarks, like what got inlined
    /// (`--remarks`).
    Remarks,
    /// The native code generator's IR (`--codegenir`).
    Ir,
    /// The native code, annotated with the bytecode it came from
    /// (`--codegen`).
    Assembly,
    /// IR and native code together (`--codegenverbose`).
    IrAndAssembly,
}

impl View {
    pub const ALL: [View; 5] = [
        View::Bytecode,
        View::Remarks,
        View::Ir,
        View::Assembly,
        View::IrAndAssembly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            View::Bytecode => "Bytecode",
            View::Remarks => "Remarks",
            View::Ir => "IR",
            View::Assembly => "Assembly",
            View::IrAndAssembly => "IR + Assembly",
        }
    }

    /// Whether this goes through the native code generator, so the target
    /// matters.
    pub fn is_native(self) -> bool {
        matches!(self, View::Ir | View::Assembly | View::IrAndAssembly)
    }

    fn code(self) -> c_int {
        match self {
            View::Bytecode => 0,
            View::Remarks => 1,
            View::Ir => 2,
            View::Assembly => 3,
            View::IrAndAssembly => 4,
        }
    }
}

/// The CPU the native code is generated for. Nothing runs, so any target
/// works on any machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    /// Whatever this machine is.
    Host,
    /// ARM64 with every optional feature (Apple silicon, recent phones).
    A64,
    /// ARM64 without the optional features.
    A64NoFeatures,
    /// x64 with the Windows calling convention.
    X64Windows,
    /// x64 with the System V calling convention (Linux, macOS).
    X64SystemV,
}

impl Target {
    pub const ALL: [Target; 5] = [
        Target::Host,
        Target::X64Windows,
        Target::X64SystemV,
        Target::A64,
        Target::A64NoFeatures,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Target::Host => "This machine",
            Target::A64 => "ARM64",
            Target::A64NoFeatures => "ARM64 (no extensions)",
            Target::X64Windows => "x64 Windows",
            Target::X64SystemV => "x64 Linux/macOS",
        }
    }

    // `Luau::CodeGen::AssemblyOptions::Target`.
    fn code(self) -> c_int {
        match self {
            Target::Host => 0,
            Target::A64 => 1,
            Target::A64NoFeatures => 2,
            Target::X64Windows => 3,
            Target::X64SystemV => 4,
        }
    }
}

/// How the script is compiled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Options {
    /// 0 none, 1 the default, 2 adds inlining and loop unrolling (what
    /// Roblox ships live games with).
    pub optimization: u8,
    /// 0 none, 1 line info and function names, 2 local and upvalue names too.
    pub debug: u8,
    /// Type info for every function rather than only `--!native` ones.
    pub type_info: bool,
    /// Compile like Roblox does: `Vector3.new` builds native vectors, and
    /// Roblox's datatypes are known userdata to the native code generator.
    pub roblox: bool,
    /// Native code for every function, even the ones the generator thinks
    /// aren't worth it.
    pub cold_functions: bool,
    pub target: Target,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            optimization: 2,
            debug: 1,
            type_info: true,
            roblox: true,
            cold_functions: true,
            target: Target::Host,
        }
    }
}

#[repr(C)]
struct RawOptions {
    optimization: c_int,
    debug: c_int,
    type_info: c_int,
    roblox: c_int,
    mode: c_int,
    target: c_int,
    cold: c_int,
}

unsafe extern "C" {
    fn rbx_bytecode_dump(
        source: *const c_char,
        len: usize,
        options: *const RawOptions,
        ok: *mut c_int,
    ) -> *mut c_char;
    fn rbx_bytecode_free(text: *mut c_char);
}

/// Compiles `source` and prints it as `view`. The error is the compiler's
/// own message, each line starting with `line:column: `.
pub fn dump(source: &str, view: View, options: &Options) -> Result<String, String> {
    let raw = RawOptions {
        optimization: options.optimization.min(2).into(),
        debug: options.debug.min(2).into(),
        type_info: options.type_info.into(),
        roblox: options.roblox.into(),
        mode: view.code(),
        target: options.target.code(),
        cold: options.cold_functions.into(),
    };
    let mut ok: c_int = 0;
    // SAFETY: the shim reads `len` bytes of `source` and the options, and
    // hands back a malloc'd C string that we free once copied.
    let text = unsafe {
        let ptr = rbx_bytecode_dump(source.as_ptr().cast(), source.len(), &raw, &mut ok);
        if ptr.is_null() {
            return Err("out of memory".into());
        }
        let text = std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned();
        rbx_bytecode_free(ptr);
        text
    };
    match ok {
        0 => Err(text.trim_end().to_owned()),
        _ => Ok(text),
    }
}

#[cfg(test)]
mod tests;
