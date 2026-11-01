// What Luau compiles a script into, as text: the bytecode listing, the
// compiler's remarks, or the native code generator's IR and assembly. Mirrors
// what Luau's own `luau-compile` prints for `--text`, `--remarks`,
// `--codegenir`, `--codegen` and `--codegenverbose`.

#include "Luau/BytecodeBuilder.h"
#include "Luau/CodeGen.h"
#include "Luau/Compiler.h"
#include "Luau/ParseResult.h"

#include "lua.h"
#include "lualib.h"

#include <cstdlib>
#include <cstring>
#include <memory>
#include <stdexcept>
#include <string>

extern "C" {

struct RbxBytecodeOptions
{
    int optimization; // 0-2
    int debug;        // 0-2
    int typeInfo;     // 0-1
    int roblox;       // Vector3 as the vector type, Roblox userdata names
    int mode;         // 0 bytecode, 1 remarks, 2 IR, 3 assembly, 4 IR + assembly
    int target;       // AssemblyOptions::Target
    int cold;         // generate native code for functions not worth it too
};

}

namespace
{

const char* const robloxUserdata[] = {
    "BrickColor",
    "CFrame",
    "Color3",
    "ColorSequence",
    "Content",
    "Font",
    "Instance",
    "NumberRange",
    "NumberSequence",
    "Ray",
    "Rect",
    "Region3",
    "TweenInfo",
    "UDim",
    "UDim2",
    "Vector2",
    "Vector2int16",
    "Vector3int16",
    nullptr,
};

void annotate(void* context, std::string& text, int fid, int instpos)
{
    static_cast<Luau::BytecodeBuilder*>(context)->annotateInstruction(text, fid, instpos);
}

std::string where(const Luau::Location& location)
{
    return std::to_string(location.begin.line + 1) + ":" + std::to_string(location.begin.column + 1) + ": ";
}

std::string run(const std::string& source, const RbxBytecodeOptions& o)
{
    Luau::CompileOptions copts;
    copts.optimizationLevel = o.optimization;
    copts.debugLevel = o.debug;
    copts.typeInfoLevel = o.typeInfo;
    if (o.roblox)
    {
        copts.vectorLib = "Vector3";
        copts.vectorCtor = "new";
        copts.vectorType = "Vector3";
        copts.userdataTypes = robloxUserdata;
    }

    Luau::BytecodeBuilder bcb;
    switch (o.mode)
    {
    case 0:
        bcb.setDumpFlags(
            Luau::BytecodeBuilder::Dump_Code | Luau::BytecodeBuilder::Dump_Source | Luau::BytecodeBuilder::Dump_Locals |
            Luau::BytecodeBuilder::Dump_Remarks | Luau::BytecodeBuilder::Dump_Types
        );
        break;
    case 1:
        bcb.setDumpFlags(Luau::BytecodeBuilder::Dump_Source | Luau::BytecodeBuilder::Dump_Remarks);
        break;
    default:
        bcb.setDumpFlags(
            Luau::BytecodeBuilder::Dump_Code | Luau::BytecodeBuilder::Dump_Source | Luau::BytecodeBuilder::Dump_Locals |
            Luau::BytecodeBuilder::Dump_Remarks
        );
        break;
    }
    bcb.setDumpSource(source);

    Luau::compileOrThrow(bcb, source, copts);

    if (o.mode == 0)
        return bcb.dumpEverything();
    if (o.mode == 1)
        return bcb.dumpSourceRemarks();

    Luau::CodeGen::AssemblyOptions aopts;
    aopts.target = Luau::CodeGen::AssemblyOptions::Target(o.target);
    aopts.includeIr = o.mode == 2 || o.mode == 4;
    aopts.includeAssembly = o.mode == 3 || o.mode == 4;
    aopts.annotator = annotate;
    aopts.annotatorContext = &bcb;
    if (o.cold)
        aopts.compilationOptions.flags |= Luau::CodeGen::CodeGen_ColdFunctions;
    if (o.roblox)
        aopts.compilationOptions.userdataTypes = robloxUserdata;

    std::unique_ptr<lua_State, void (*)(lua_State*)> state(luaL_newstate(), lua_close);
    lua_State* L = state.get();
    const std::string& bytecode = bcb.getBytecode();
    if (luau_load(L, "=script", bytecode.data(), bytecode.size(), 0) != 0)
        throw std::runtime_error(lua_tostring(L, -1));
    return Luau::CodeGen::getAssembly(L, -1, aopts);
}

char* copy(const std::string& text)
{
    char* out = static_cast<char*>(std::malloc(text.size() + 1));
    if (out)
        std::memcpy(out, text.c_str(), text.size() + 1);
    return out;
}

} // namespace

extern "C" {

// Returns a malloc'd string, the listing when *ok is 1, the error otherwise.
// Free it with rbx_bytecode_free.
char* rbx_bytecode_dump(const char* source, size_t len, const RbxBytecodeOptions* options, int* ok)
{
    *ok = 0;
    try
    {
        std::string text = run(std::string(source, len), *options);
        *ok = 1;
        return copy(text);
    }
    catch (const Luau::ParseErrors& e)
    {
        std::string text;
        for (const Luau::ParseError& error : e.getErrors())
            text += where(error.getLocation()) + error.getMessage() + "\n";
        return copy(text);
    }
    catch (const Luau::CompileError& e)
    {
        return copy(where(e.getLocation()) + e.what() + "\n");
    }
    catch (const std::exception& e)
    {
        return copy(std::string(e.what()) + "\n");
    }
    catch (...)
    {
        return copy("the compiler failed\n");
    }
}

void rbx_bytecode_free(char* text)
{
    std::free(text);
}

}
