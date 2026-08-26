use std::fmt;

use crate::LuaType;

/// Failure produced by checked Rust-side Lua access.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LuaError {
    /// A raw callback state pointer was null.
    NullState,
    /// A callback state did not contain an `ILuaBase` pointer.
    NullLuaBase,
    /// A stack value did not have the required type.
    TypeMismatch { expected: LuaType, actual: LuaType },
    /// A Lua string was not valid UTF-8.
    InvalidUtf8,
    /// `GetString` returned null after an exact string type check.
    NullStringPointer,
    /// A string cannot be represented by the pinned ABI length type.
    StringTooLong,
    /// A frame operation would remove caller-owned stack values.
    StackUnderflow { baseline: i32, requested_top: i32 },
    /// Callback return count disagreed with values left on the stack.
    ReturnCountMismatch { expected: i32, actual: i32 },
    /// A count cannot be represented by the pinned ABI.
    CountOverflow,
    /// Closure upvalues are one-based.
    InvalidUpvaluePosition,
}

/// Result type used by checked Lua operations.
pub type LuaResult<T> = Result<T, LuaError>;

impl fmt::Display for LuaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullState => formatter.write_str("Lua state pointer is null"),
            Self::NullLuaBase => formatter.write_str("Lua state has no ILuaBase pointer"),
            Self::TypeMismatch { expected, actual } => write!(
                formatter,
                "expected {}, got {}",
                type_name(*expected),
                type_name(*actual)
            ),
            Self::InvalidUtf8 => formatter.write_str("Lua string is not valid UTF-8"),
            Self::NullStringPointer => {
                formatter.write_str("Lua returned a null pointer for a string value")
            }
            Self::StringTooLong => formatter.write_str("Lua string exceeds the ABI length limit"),
            Self::StackUnderflow {
                baseline,
                requested_top,
            } => write!(
                formatter,
                "stack frame starts at {baseline}, cannot restore requested top {requested_top}"
            ),
            Self::ReturnCountMismatch { expected, actual } => write!(
                formatter,
                "callback declared {expected} return values but left {actual} on the stack"
            ),
            Self::CountOverflow => formatter.write_str("value count exceeds the ABI integer limit"),
            Self::InvalidUpvaluePosition => {
                formatter.write_str("closure upvalue positions start at one")
            }
        }
    }
}

impl std::error::Error for LuaError {}

fn type_name(value_type: LuaType) -> &'static str {
    match value_type {
        LuaType::NONE => "none",
        LuaType::NIL => "nil",
        LuaType::BOOL => "bool",
        LuaType::LIGHT_USER_DATA => "lightuserdata",
        LuaType::NUMBER => "number",
        LuaType::STRING => "string",
        LuaType::TABLE => "table",
        LuaType::FUNCTION => "function",
        LuaType::USER_DATA => "userdata",
        LuaType::THREAD => "thread",
        LuaType::ENTITY => "entity",
        _ => "unknown",
    }
}
