use std::error::Error;
use std::fmt;

use rsgdll_bridge::{
    RETURN_BOOL, RETURN_BYTE_CAPACITY, RETURN_NIL, RETURN_NUMBER, RETURN_SLOT_CAPACITY,
    RETURN_STRING, ReturnBuffer, ReturnSlot,
};

const MAX_EXACT_LUA_INTEGER: u64 = 1_u64 << 53;

/// Failure while staging Rust values for Lua.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnError {
    TooManyValues,
    StringDataTooLong,
    IntegerNotExactlyRepresentable,
    MixedReturnModes,
}

impl fmt::Display for ReturnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyValues => formatter.write_str("more than 16 Lua return values"),
            Self::StringDataTooLong => {
                formatter.write_str("Lua return strings exceed the 4096-byte staging buffer")
            }
            Self::IntegerNotExactlyRepresentable => {
                formatter.write_str("integer is not exactly representable by a Lua number")
            }
            Self::MixedReturnModes => {
                formatter.write_str("cannot mix staged and on-stack Lua return values")
            }
        }
    }
}

impl Error for ReturnError {}

/// Converts one owned Rust value into non-throwing Lua return staging data.
pub trait IntoLuaReturn {
    fn into_lua_return(self, writer: &mut ReturnWriter<'_>) -> Result<(), ReturnError>;
}

/// Writes values into C++-owned storage without calling Lua.
pub struct ReturnWriter<'buffer> {
    buffer: &'buffer mut ReturnBuffer,
    count: usize,
    bytes_used: usize,
    stack_count: Option<usize>,
}

impl<'buffer> ReturnWriter<'buffer> {
    pub(crate) fn new(buffer: &'buffer mut ReturnBuffer) -> Self {
        Self {
            buffer,
            count: 0,
            bytes_used: 0,
            stack_count: None,
        }
    }

    pub fn push<T: IntoLuaReturn>(&mut self, value: T) -> Result<(), ReturnError> {
        value.into_lua_return(self)
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    pub(crate) fn stack_count(&self) -> Option<usize> {
        self.stack_count
    }

    fn push_slot(&mut self, slot: ReturnSlot) -> Result<(), ReturnError> {
        if self.stack_count.is_some() {
            return Err(ReturnError::MixedReturnModes);
        }
        let Some(output) = self.buffer.slots.get_mut(self.count) else {
            return Err(ReturnError::TooManyValues);
        };
        *output = slot;
        self.count += 1;
        Ok(())
    }

    fn push_string(&mut self, value: &[u8]) -> Result<(), ReturnError> {
        let end = self
            .bytes_used
            .checked_add(value.len())
            .filter(|end| *end <= RETURN_BYTE_CAPACITY)
            .ok_or(ReturnError::StringDataTooLong)?;
        let offset = u32::try_from(self.bytes_used).map_err(|_| ReturnError::StringDataTooLong)?;
        let length = u32::try_from(value.len()).map_err(|_| ReturnError::StringDataTooLong)?;
        let slot = ReturnSlot {
            tag: RETURN_STRING,
            offset,
            length,
            reserved: 0,
            number: 0.0,
        };
        self.push_slot(slot)?;
        self.buffer.bytes[self.bytes_used..end].copy_from_slice(value);
        self.bytes_used = end;
        Ok(())
    }

    fn use_stack(&mut self, count: usize) -> Result<(), ReturnError> {
        if self.count != 0 || self.stack_count.is_some() {
            return Err(ReturnError::MixedReturnModes);
        }
        self.stack_count = Some(count);
        Ok(())
    }
}

/// Declares that a generated callback left its return values on the Lua stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LuaStackValues(usize);

impl LuaStackValues {
    #[must_use]
    pub const fn new(count: usize) -> Self {
        Self(count)
    }
}

impl IntoLuaReturn for LuaStackValues {
    fn into_lua_return(self, writer: &mut ReturnWriter<'_>) -> Result<(), ReturnError> {
        writer.use_stack(self.0)
    }
}

impl IntoLuaReturn for () {
    fn into_lua_return(self, _: &mut ReturnWriter<'_>) -> Result<(), ReturnError> {
        Ok(())
    }
}

impl IntoLuaReturn for bool {
    fn into_lua_return(self, writer: &mut ReturnWriter<'_>) -> Result<(), ReturnError> {
        writer.push_slot(ReturnSlot {
            tag: RETURN_BOOL,
            offset: 0,
            length: 0,
            reserved: 0,
            number: f64::from(self),
        })
    }
}

impl IntoLuaReturn for f64 {
    fn into_lua_return(self, writer: &mut ReturnWriter<'_>) -> Result<(), ReturnError> {
        writer.push_slot(ReturnSlot {
            tag: RETURN_NUMBER,
            offset: 0,
            length: 0,
            reserved: 0,
            number: self,
        })
    }
}

macro_rules! impl_integer_returns {
    ($($integer:ty),+ $(,)?) => {
        $(
            impl IntoLuaReturn for $integer {
                fn into_lua_return(
                    self,
                    writer: &mut ReturnWriter<'_>,
                ) -> Result<(), ReturnError> {
                    let value = self as f64;
                    if value.abs() > MAX_EXACT_LUA_INTEGER as f64 || value as Self != self {
                        return Err(ReturnError::IntegerNotExactlyRepresentable);
                    }
                    value.into_lua_return(writer)
                }
            }
        )+
    };
}

impl_integer_returns!(u8, u16, u32, u64, i8, i16, i32, i64);

impl IntoLuaReturn for String {
    fn into_lua_return(self, writer: &mut ReturnWriter<'_>) -> Result<(), ReturnError> {
        writer.push_string(self.as_bytes())
    }
}

impl IntoLuaReturn for &str {
    fn into_lua_return(self, writer: &mut ReturnWriter<'_>) -> Result<(), ReturnError> {
        writer.push_string(self.as_bytes())
    }
}

impl IntoLuaReturn for rsgdll_lua::LuaBytes {
    fn into_lua_return(self, writer: &mut ReturnWriter<'_>) -> Result<(), ReturnError> {
        writer.push_string(self.as_bytes())
    }
}

impl IntoLuaReturn for Option<()> {
    fn into_lua_return(self, writer: &mut ReturnWriter<'_>) -> Result<(), ReturnError> {
        if self.is_none() {
            writer.push_slot(ReturnSlot {
                tag: RETURN_NIL,
                offset: 0,
                length: 0,
                reserved: 0,
                number: 0.0,
            })?;
        }
        Ok(())
    }
}

const _: () = assert!(RETURN_SLOT_CAPACITY == 16);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_return_preserves_every_byte() {
        // Given: a fresh bridge return buffer.
        let mut buffer = ReturnBuffer {
            slots: [ReturnSlot {
                tag: 0,
                offset: 0,
                length: 0,
                reserved: 0,
                number: 0.0,
            }; RETURN_SLOT_CAPACITY],
            bytes: [0; RETURN_BYTE_CAPACITY],
        };
        let mut writer = ReturnWriter::new(&mut buffer);

        // When: arbitrary bytes are staged as one Lua string.
        writer
            .push(rsgdll_lua::LuaBytes::from(vec![0, 0xff, b'A', 0]))
            .expect("binary return");

        // Then: tag, length, and copied bytes remain exact.
        assert_eq!(buffer.slots[0].tag, RETURN_STRING);
        assert_eq!(buffer.slots[0].length, 4);
        assert_eq!(&buffer.bytes[..4], &[0, 0xff, b'A', 0]);
    }
}
