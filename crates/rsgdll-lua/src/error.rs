use std::fmt;

use crate::{LuaBytes, LuaType};

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
    /// A Lua number cannot be represented as the requested exact integer.
    IntegerOutOfRange,
    /// A frame operation would remove caller-owned stack values.
    StackUnderflow { baseline: i32, requested_top: i32 },
    /// Callback return count disagreed with values left on the stack.
    ReturnCountMismatch { expected: i32, actual: i32 },
    /// A count cannot be represented by the pinned ABI.
    CountOverflow,
    /// Closure upvalues are one-based.
    InvalidUpvaluePosition,
    /// A state-owned value was used with a different Lua state.
    WrongState,
    /// A protected Lua call raised an error.
    Call { status: i32, message: LuaBytes },
    /// A userdata type name was reused for a different Rust type.
    UserDataTypeNameConflict(String),
    /// A foreign userdata type identifier does not fit its one-byte header.
    UserDataTypeOutOfRange,
    /// Userdata allocation returned null.
    NullUserData,
    /// Registered userdata metatable was unavailable.
    MissingMetaTable,
    /// Userdata belongs to another registered Rust type.
    UserDataTypeMismatch,
    /// Userdata has already been finalized.
    FinalizedUserData,
    /// Userdata is already borrowed incompatibly.
    UserDataBorrowConflict,
    /// A C-compatible name contained an interior NUL byte.
    StringContainsNul,
    /// Serde conversion failed.
    Serde(String),
    /// Serde conversion exceeded the supported nesting limit.
    SerdeDepthExceeded,
    /// A Lua type has no serde representation.
    UnsupportedSerdeType(LuaType),
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
            Self::IntegerOutOfRange => {
                formatter.write_str("Lua number is not an exactly representable integer")
            }
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
            Self::WrongState => formatter.write_str("Lua value belongs to a different state"),
            Self::Call { status, message } => write!(
                formatter,
                "protected Lua call failed with status {status}: {}",
                String::from_utf8_lossy(message.as_bytes())
            ),
            Self::UserDataTypeNameConflict(name) => {
                write!(
                    formatter,
                    "userdata type name {name:?} is already registered"
                )
            }
            Self::UserDataTypeOutOfRange => {
                formatter.write_str("userdata type identifier exceeds one-byte ABI storage")
            }
            Self::NullUserData => formatter.write_str("Lua returned null userdata storage"),
            Self::MissingMetaTable => formatter.write_str("userdata metatable is unavailable"),
            Self::UserDataTypeMismatch => formatter.write_str("userdata has a different Rust type"),
            Self::FinalizedUserData => formatter.write_str("userdata was already finalized"),
            Self::UserDataBorrowConflict => {
                formatter.write_str("userdata is already borrowed incompatibly")
            }
            Self::StringContainsNul => formatter.write_str("name contains an interior NUL byte"),
            Self::Serde(message) => write!(formatter, "serde conversion failed: {message}"),
            Self::SerdeDepthExceeded => {
                formatter.write_str("serde Lua value nesting exceeds 64 levels")
            }
            Self::UnsupportedSerdeType(actual) => {
                write!(
                    formatter,
                    "Lua type {actual:?} cannot be represented by serde"
                )
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
