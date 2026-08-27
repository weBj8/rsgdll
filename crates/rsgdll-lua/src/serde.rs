//! Optional serde conversion through ordinary Lua values.

use std::collections::BTreeMap;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Number, Value};

use crate::{LuaBytes, LuaError, LuaResult, LuaType, StackFrame, convert::MAX_EXACT_LUA_INTEGER};

const MAX_DEPTH: usize = 64;

/// Serializes one serde value and pushes its Lua representation.
pub fn to_lua<T: Serialize>(frame: &mut StackFrame<'_, '_>, value: &T) -> LuaResult<()> {
    let value = serde_json::to_value(value).map_err(serde_error)?;
    push_value(frame, &value, 0)
}

/// Deserializes one Lua value through serde.
pub fn from_lua<T: DeserializeOwned>(frame: &mut StackFrame<'_, '_>, index: i32) -> LuaResult<T> {
    let top = frame.top();
    let value = read_value(frame, index, 0);
    let extra = frame.top().saturating_sub(top);
    let extra = usize::try_from(extra).map_err(|_| LuaError::CountOverflow)?;
    frame.pop(extra)?;
    serde_json::from_value(value?).map_err(serde_error)
}

fn push_value(frame: &mut StackFrame<'_, '_>, value: &Value, depth: usize) -> LuaResult<()> {
    if depth > MAX_DEPTH {
        return Err(LuaError::SerdeDepthExceeded);
    }
    match value {
        Value::Null => frame.push(()),
        Value::Bool(value) => frame.push(*value),
        Value::Number(value) => {
            if let Some(value) = value.as_u64() {
                if value > MAX_EXACT_LUA_INTEGER as u64 {
                    return Err(LuaError::IntegerOutOfRange);
                }
                frame.push(value as f64)
            } else if let Some(value) = value.as_i64() {
                if value.unsigned_abs() > MAX_EXACT_LUA_INTEGER as u64 {
                    return Err(LuaError::IntegerOutOfRange);
                }
                frame.push(value as f64)
            } else {
                let value = value
                    .as_f64()
                    .ok_or_else(|| LuaError::Serde("number is outside Lua range".to_owned()))?;
                frame.push(value)
            }
        }
        Value::String(value) => frame.push(value.as_str()),
        Value::Array(values) => {
            frame.create_table()?;
            for (offset, value) in values.iter().enumerate() {
                let key = u64::try_from(offset)
                    .ok()
                    .and_then(|offset| offset.checked_add(1))
                    .ok_or(LuaError::CountOverflow)?;
                frame.push(key as f64)?;
                push_value(frame, value, depth + 1)?;
                frame.raw_set(-3)?;
            }
            Ok(())
        }
        Value::Object(values) => {
            frame.create_table()?;
            for (key, value) in values {
                frame.push(key.as_str())?;
                push_value(frame, value, depth + 1)?;
                frame.raw_set(-3)?;
            }
            Ok(())
        }
    }
}

fn read_value(frame: &mut StackFrame<'_, '_>, index: i32, depth: usize) -> LuaResult<Value> {
    if depth > MAX_DEPTH {
        return Err(LuaError::SerdeDepthExceeded);
    }
    match frame.value_type(index) {
        LuaType::NIL => Ok(Value::Null),
        LuaType::BOOL => frame.get(index).map(Value::Bool),
        LuaType::NUMBER => {
            let value: f64 = frame.get(index)?;
            let number = if value.fract() == 0.0 && value >= 0.0 {
                value
                    .to_string()
                    .parse::<u64>()
                    .map(Number::from)
                    .map_err(|_| LuaError::Serde("Lua integer is outside u64 range".to_owned()))?
            } else if value.fract() == 0.0 {
                value
                    .to_string()
                    .parse::<i64>()
                    .map(Number::from)
                    .map_err(|_| LuaError::Serde("Lua integer is outside i64 range".to_owned()))?
            } else {
                Number::from_f64(value)
                    .ok_or_else(|| LuaError::Serde("Lua number is not finite".to_owned()))?
            };
            Ok(Value::Number(number))
        }
        LuaType::STRING => {
            let bytes: LuaBytes = frame.get(index)?;
            let string = String::from_utf8(bytes.into_vec())
                .map_err(|_| LuaError::Serde("Lua string is not UTF-8".to_owned()))?;
            Ok(Value::String(string))
        }
        LuaType::TABLE => read_table(frame, index, depth + 1),
        actual => Err(LuaError::UnsupportedSerdeType(actual)),
    }
}

fn read_table(frame: &mut StackFrame<'_, '_>, index: i32, depth: usize) -> LuaResult<Value> {
    let table = frame.absolute_index(index)?;
    let mut object = Map::new();
    let mut sequence = BTreeMap::new();
    let mut has_object_keys = false;
    frame.push(())?;
    while frame.next(table)? {
        // Value is at -1 and key at -2.
        let value = read_value(frame, -1, depth)?;
        match frame.value_type(-2) {
            LuaType::STRING => {
                has_object_keys = true;
                let key: String = frame.get(-2)?;
                object.insert(key, value);
            }
            LuaType::NUMBER => {
                let key: f64 = frame.get(-2)?;
                if !key.is_finite() || key.fract() != 0.0 || key < 1.0 {
                    return Err(LuaError::Serde(
                        "Lua sequence keys must be positive integers".to_owned(),
                    ));
                }
                let key = key as usize;
                sequence.insert(key, value);
            }
            actual => return Err(LuaError::UnsupportedSerdeType(actual)),
        }
        frame.pop(1)?;
    }
    if has_object_keys && !sequence.is_empty() {
        return Err(LuaError::Serde(
            "mixed Lua table keys cannot be represented by serde".to_owned(),
        ));
    }
    if has_object_keys || sequence.is_empty() {
        Ok(Value::Object(object))
    } else {
        let mut values = Vec::with_capacity(sequence.len());
        for (expected, (actual, value)) in (1..).zip(sequence) {
            if actual != expected {
                return Err(LuaError::Serde("Lua sequence contains a gap".to_owned()));
            }
            values.push(value);
        }
        Ok(Value::Array(values))
    }
}

fn serde_error(error: serde_json::Error) -> LuaError {
    LuaError::Serde(error.to_string())
}
