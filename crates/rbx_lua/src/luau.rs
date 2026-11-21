//! The Luau we embed, kept at the version live Roblox ships, with Roblox's
//! own switches: the fast flags Roblox has on, and the Studio betas that
//! turn on more.
//!
//! Bumping: see `vendor/README.md` (`luau0-src`). The guard test below
//! fails when the linked Luau is older than [`LUAU_VERSION`].

use std::sync::Once;

/// The Luau release live Roblox (client and Studio 0.748) runs: every fast
/// flag Luau 0.748 added is in Studio 0.748's binary and every flag 0.748
/// removed is gone from it. Raise it when Roblox moves on.
pub const LUAU_VERSION: &str = "0.748";

/// Luau's own fast flags (VM, compiler, code generator) that Roblox has on,
/// from Studio's live settings (`clientsettingscdn.roblox.com`,
/// `PCStudioApp`) on 2026-11-21. A flag Roblox leaves at its default (off)
/// is not listed. The allocator's memory-cage flags (`LuauBufferCage`,
/// `LuauBufferSweep`, `LuauCageSplitData`) are left out on purpose: they
/// change how pages are laid out, so flipping them while any VM in the
/// process is alive would corrupt it.
pub(crate) const ROBLOX_FFLAGS: &[&str] = &[
    "DebugLuauReportReturnTypeVariadicWithTypeSuffix",
    "LuauAllowGlobalDeclarationToBeCalledClass",
    "LuauCodegenA64ReloadLargeConstOffset",
    "LuauCodegenCmpAnySideEffecting",
    "LuauCodegenExitSyncExpectedTag",
    "LuauCodegenFastcallAnalysisLimit",
    "LuauCodegenForwardSplitStore",
    "LuauCodegenIntelMacUnwind",
    "LuauCodegenMarkUsed",
    "LuauCodegenMetatableCache",
    "LuauCodegenMultiLaneDot",
    "LuauCodegenNopPadding",
    "LuauCodegenRuntimeFeedback",
    "LuauCodegenUserdataDirect",
    "LuauCompileCapturedCallTarget",
    "LuauCompileLoopUnrollNegativeZero",
    "LuauCompileRequireNoInline",
    "LuauCompileTableFoldFix",
    "LuauCoroutineFinally",
    "LuauGcHeapShrinkFix",
    "LuauInlinerNamecallSelfFromMove",
    "LuauIsYieldableSimplify",
    "LuauLoadExplicitGcDisable",
    "LuauOptimizeStringGsub",
    "LuauOptimizeStringSplit",
    "LuauPcallErrorContinuationReentry",
    "LuauPromoteProto",
    "LuauSaltedInterningTable",
    "LuauSandboxFreezesVectorMetatable",
    "LuauSingleTypeOptionalPackReturnsAttributeParens",
    "LuauSkipDebugLocked",
];

/// What Studio's "If Local/Const Bindings" beta turns on: `if local`,
/// `if const` (statement and expression) and `while local`/`while const`.
pub const IF_LOCAL_FFLAGS: &[&str] = &[
    "LuauExperimentalIfLocalSyntax",
    "LuauExperimentalWhileLocalSyntax",
];

static ROBLOX: Once = Once::new();

/// A Luau VM with Roblox's fast flags set. Every VM that runs place code
/// starts here, so the flags are in place before anything compiles.
/// `_VERSION` is Roblox's bare `"Luau"`, not mlua's `"Luau 0.748"`, and
/// `loadstring` is Roblox's (see [`loadstring`]).
pub(crate) fn new_vm() -> mlua::Lua {
    ROBLOX.call_once(|| {
        for flag in ROBLOX_FFLAGS {
            // A flag of a part this binary doesn't link (the code generator
            // outside `rbx_bytecode`) isn't registered, and doesn't matter.
            let _ = mlua::Lua::set_fflag(flag, true);
        }
    });
    let lua = mlua::Lua::new();
    let globals = lua.globals();
    globals
        .raw_set("_VERSION", "Luau")
        .and_then(|()| globals.raw_set("loadstring", lua.create_function(loadstring)?))
        .expect("a fresh VM's globals are writable");
    lua
}

/// Roblox's `loadstring(source, chunkname?)`: code that doesn't compile
/// returns `nil` and the message rather than raising, and the chunk is
/// named after its source (`[string "return 1i"]:1: Malformed number`),
/// as Studio's Command Bar shows.
fn loadstring(
    lua: &mlua::Lua,
    (source, name): (mlua::LuaString, Option<String>),
) -> mlua::Result<(Option<mlua::Function>, Option<String>)> {
    let name = name.unwrap_or_else(|| source.to_string_lossy());
    match lua
        .load(source.as_bytes().to_vec())
        .set_name(name)
        .into_function()
    {
        Ok(function) => Ok((Some(function), None)),
        Err(mlua::Error::SyntaxError { message, .. }) => Ok((None, Some(message))),
        Err(error) => Err(error),
    }
}

/// Turns Studio's "If Local/Const Bindings" beta on or off for every VM in
/// the process. Call it before any script compiles.
pub fn set_if_local(on: bool) {
    for flag in IF_LOCAL_FFLAGS {
        mlua::Lua::set_fflag(flag, on).expect("Luau knows the if-local flags");
    }
}

/// The Luau release this build links (`"0.748"`).
pub fn linked_version() -> &'static str {
    mlua::ffi::luau_version().expect("luau0-src reports its Luau version")
}

#[cfg(test)]
mod tests;
