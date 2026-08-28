use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Write;

const SHARED_STRUCTS: [&str; 6] = [
    "AbiLayout",
    "ModuleRegistration",
    "DispatchResult",
    "ReturnSlot",
    "ReturnBuffer",
    "LuaOperation",
];

pub fn generate(source: &str) -> Result<String, Box<dyn Error>> {
    verify_parser_contract()?;
    let constants = rust_constants(source)?;
    generated_header(source, &constants)
}

fn verify_parser_contract() -> Result<(), Box<dyn Error>> {
    const MISSING_REPR: &str = "pub struct Probe {\n    pub value: usize,\n}\n";
    const PRIVATE_FIELD: &str = "#[repr(C)]\npub struct Probe {\n    value: usize,\n}\n";
    const VALID: &str =
        "#[repr(C)]\n#[derive(Clone, Copy)]\npub struct Probe {\n    pub value: usize,\n}\n";
    if rust_struct(MISSING_REPR, "Probe").is_ok() {
        return Err("ABI parser accepted a struct without #[repr(C)]".into());
    }
    if rust_struct(PRIVATE_FIELD, "Probe").is_ok() {
        return Err("ABI parser silently skipped a private field".into());
    }
    if rust_struct(VALID, "Probe")? != [("value".to_owned(), "usize".to_owned())] {
        return Err("ABI parser changed a valid #[repr(C)] struct".into());
    }
    Ok(())
}

fn rust_constants(source: &str) -> Result<BTreeMap<String, (String, String)>, Box<dyn Error>> {
    let mut constants = BTreeMap::new();
    for line in source.lines().map(str::trim) {
        let Some(declaration) = line.strip_prefix("pub const ") else {
            continue;
        };
        if declaration.starts_with("fn ") {
            continue;
        }
        let Some((name, declaration)) = declaration.split_once(':') else {
            return Err(format!("invalid public constant declaration: {line}").into());
        };
        let Some((rust_type, value)) = declaration.split_once('=') else {
            return Err(format!("invalid public constant value: {line}").into());
        };
        constants.insert(
            name.trim().to_owned(),
            (
                rust_type.trim().to_owned(),
                value.trim().trim_end_matches(';').to_owned(),
            ),
        );
    }
    Ok(constants)
}

fn generated_header(
    source: &str,
    constants: &BTreeMap<String, (String, String)>,
) -> Result<String, Box<dyn Error>> {
    let mut output =
        String::from("#pragma once\n#include <cstddef>\n#include <cstdint>\n\nstruct LuaState;\n");
    let mut fields = BTreeMap::new();
    for name in SHARED_STRUCTS {
        let parsed = rust_struct(source, name)?;
        write_cpp_struct(&mut output, name, &parsed, constants)?;
        fields.insert(name, parsed);
    }

    output.push_str(
        "\n#if defined(_WIN32) && !defined(_WIN64)\n\
         #define RSGDLL_LUA_CALL __thiscall\n\
         #else\n\
         #define RSGDLL_LUA_CALL\n\
         #endif\n\
         \nusing Dispatcher = DispatchResult (*)(LuaState *, char *, std::uint32_t, ReturnBuffer *);\n\
         using DebugDispatcher = void (*)(LuaState *, void *);\n\
         using LuaHook = void (*)(LuaState *, void *);\n\
         using ThrowError = void (RSGDLL_LUA_CALL *)(void *, const char *);\n\
         using CreateTable = void (RSGDLL_LUA_CALL *)(void *);\n\
         using RawSet = void (RSGDLL_LUA_CALL *)(void *, std::int32_t);\n\
         using PushNil = void (RSGDLL_LUA_CALL *)(void *);\n\
         using PushString = void (RSGDLL_LUA_CALL *)(void *, const char *, std::uint32_t);\n\
         using PushNumber = void (RSGDLL_LUA_CALL *)(void *, double);\n\
         using PushBool = void (RSGDLL_LUA_CALL *)(void *, bool);\n\
         using PushClosure = void (RSGDLL_LUA_CALL *)(void *, std::int32_t (*)(LuaState *), std::int32_t);\n\
         using PushSpecial = void (RSGDLL_LUA_CALL *)(void *, std::int32_t);\n\
         using Pop = void (RSGDLL_LUA_CALL *)(void *, std::int32_t);\n\
         using SetState = void (RSGDLL_LUA_CALL *)(void *, LuaState *);\n\
using ModuleInitializer = std::uint8_t (*)(ModuleRegistration *, std::uint32_t,\n\
    std::uint32_t *, const std::uint8_t **, std::uint32_t *, const AbiLayout **,\n\
    char *, std::uint32_t);\n\
         using Push = void (RSGDLL_LUA_CALL *)(void *, std::int32_t);\n\
         using Top = std::int32_t (RSGDLL_LUA_CALL *)(void *);\n\
         using Remove = void (RSGDLL_LUA_CALL *)(void *, std::int32_t);\n\
         using PCall = std::int32_t (RSGDLL_LUA_CALL *)(void *, std::int32_t, std::int32_t, std::int32_t);\n\
         using SetMetaTable = void (RSGDLL_LUA_CALL *)(void *, std::int32_t);\n\
         using NewUserdata = void *(RSGDLL_LUA_CALL *)(void *, std::uint32_t);\n\
         using Next = std::int32_t (RSGDLL_LUA_CALL *)(void *, std::int32_t);\n\
         using ReferenceCreate = std::int32_t (RSGDLL_LUA_CALL *)(void *);\n\
         using ReferenceFree = void (RSGDLL_LUA_CALL *)(void *, std::int32_t);\n\
         using ReferencePush = void (RSGDLL_LUA_CALL *)(void *, std::int32_t);\n\
         using CreateMetaTable = std::int32_t (RSGDLL_LUA_CALL *)(void *, const char *);\n\
         using PushMetaTable = bool (RSGDLL_LUA_CALL *)(void *, std::int32_t);\n\
         using SetUserType = void (RSGDLL_LUA_CALL *)(void *, std::int32_t, void *);\n\
         using GetType = std::int32_t (RSGDLL_LUA_CALL *)(void *, std::int32_t);\n",
    );

    write_cpp_enum(&mut output, constants, "RETURN_")?;
    write_cpp_enum(&mut output, constants, "STATUS_")?;
    write_cpp_enum(&mut output, constants, "OP_")?;

    let abi_fields = fields
        .get("AbiLayout")
        .ok_or("AbiLayout was not generated")?;
    for (name, _) in abi_fields {
        if name.ends_with("_slot") {
            writeln!(output, "#define {name} (current_abi_layout()->{name})")?;
        }
    }

    let error_capacity = constant_value(constants, "ERROR_BUFFER_CAPACITY")?;
    writeln!(
        output,
        "\nstatic constexpr std::uint32_t error_capacity = {error_capacity};"
    )?;
    Ok(output)
}

fn rust_struct(source: &str, name: &str) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let marker = format!("pub struct {name} {{");
    let (head, tail) = source
        .split_once(&marker)
        .ok_or_else(|| format!("missing shared Rust struct {name}"))?;
    let has_repr_c = head
        .lines()
        .rev()
        .map(str::trim)
        .take_while(|line| line.starts_with("#["))
        .any(|line| line == "#[repr(C)]");
    if !has_repr_c {
        return Err(format!("shared Rust struct {name} must have #[repr(C)]").into());
    }
    let mut fields = Vec::new();
    for line in tail.lines() {
        let line = line.trim();
        if line == "}" {
            return Ok(fields);
        }
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let Some(field) = line.strip_prefix("pub ") else {
            return Err(format!(
                "unsupported non-public field in shared Rust struct {name}: {line}"
            )
            .into());
        };
        let Some((field_name, rust_type)) = field.trim_end_matches(',').split_once(':') else {
            return Err(format!("invalid field in {name}: {line}").into());
        };
        fields.push((field_name.trim().to_owned(), rust_type.trim().to_owned()));
    }
    Err(format!("unterminated shared Rust struct {name}").into())
}

fn write_cpp_struct(
    output: &mut String,
    name: &str,
    fields: &[(String, String)],
    constants: &BTreeMap<String, (String, String)>,
) -> Result<(), Box<dyn Error>> {
    writeln!(output, "\nstruct {name} {{")?;
    for (field, rust_type) in fields {
        if let Some(array) = rust_type
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            let (element, capacity) = array
                .split_once(';')
                .ok_or_else(|| format!("invalid array field {name}.{field}"))?;
            writeln!(
                output,
                "    {} {field}[{}];",
                cpp_type(element.trim())?,
                constant_value(constants, capacity.trim())?
            )?;
        } else {
            writeln!(output, "    {} {field};", cpp_type(rust_type)?)?;
        }
    }
    output.push_str("};\n");
    Ok(())
}

fn cpp_type(rust_type: &str) -> Result<&'static str, Box<dyn Error>> {
    match rust_type {
        "usize" => Ok("std::size_t"),
        "u8" => Ok("std::uint8_t"),
        "u32" => Ok("std::uint32_t"),
        "i32" => Ok("std::int32_t"),
        "i64" => Ok("std::int64_t"),
        "f64" => Ok("double"),
        "ReturnSlot" => Ok("ReturnSlot"),
        "*const u8" => Ok("const std::uint8_t *"),
        "*const c_void" => Ok("const void *"),
        "*mut c_void" => Ok("void *"),
        _ => Err(format!("unsupported shared ABI type {rust_type}").into()),
    }
}

fn write_cpp_enum(
    output: &mut String,
    constants: &BTreeMap<String, (String, String)>,
    prefix: &str,
) -> Result<(), Box<dyn Error>> {
    let signed = prefix == "STATUS_";
    let rust_repr = if signed { "i32" } else { "u32" };
    let cpp_repr = if signed {
        "std::int32_t"
    } else {
        "std::uint32_t"
    };
    writeln!(output, "\nenum : {cpp_repr} {{")?;
    for (name, (rust_type, value)) in constants {
        if name.starts_with(prefix) && rust_type == rust_repr {
            writeln!(output, "    {} = {value},", name.to_ascii_lowercase())?;
        }
    }
    output.push_str("};\n");
    Ok(())
}

fn constant_value<'a>(
    constants: &'a BTreeMap<String, (String, String)>,
    name: &str,
) -> Result<&'a str, Box<dyn Error>> {
    constants
        .get(name)
        .map(|(_, value)| value.as_str())
        .ok_or_else(|| format!("missing shared Rust constant {name}").into())
}
