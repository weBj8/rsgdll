//! Serde bridge (spec §6.4, feature `serde`): own Serializer/Deserializer over the stack.

use core::ffi::c_int;
use std::fmt::Display;

use rsgdll_sys::lua::*;
use serde::de::{self, DeserializeOwned, IntoDeserializer, Visitor};
use serde::ser::{self, Serialize};

use super::prim::{from_int, string_bytes};
use super::table::{expect_table, fetch_index, len, pairs};
use super::{FromLua, IntoLua, Stage, mismatch};
use crate::error::ConvError;
use crate::lua::Lua;
use crate::protected::Op;

/// Wrapper converting any `Serialize + DeserializeOwned` type.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LuaSerde<T>(pub T);

#[derive(Debug)]
pub struct SerdeError(pub ConvError);

impl Display for SerdeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for SerdeError {}
impl ser::Error for SerdeError {
    fn custom<T: Display>(m: T) -> Self {
        SerdeError(ConvError::msg(m.to_string()))
    }
}
impl de::Error for SerdeError {
    fn custom<T: Display>(m: T) -> Self {
        SerdeError(ConvError::msg(m.to_string()))
    }
}

type R<T> = Result<T, SerdeError>;

impl<T: Serialize> IntoLua for LuaSerde<T> {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        self.0.serialize(Ser(s)).map_err(|e| e.0)
    }
}

impl<T: DeserializeOwned> FromLua for LuaSerde<T> {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        T::deserialize(De {
            lua,
            idx: lua.abs(idx),
        })
        .map(LuaSerde)
        .map_err(|e| e.0)
    }
}

pub struct Ser<'a, 'l>(pub &'a mut Stage<'l>);

/// Builds a table: sequence (`next` index) or map/struct (key pushed before value).
pub struct Table<'a, 'l> {
    s: &'a mut Stage<'l>,
    next: c_int,
    /// Enum variant wrapper `{ Variant = <table> }` to close on `end`.
    wrapped: bool,
}

impl<'a, 'l> Table<'a, 'l> {
    fn item<T: ?Sized + Serialize>(&mut self, v: &T) -> R<()> {
        v.serialize(Ser(self.s))?;
        self.next += 1;
        self.s.raw_seti(self.next);
        Ok(())
    }
    fn field<T: ?Sized + Serialize>(&mut self, k: &'static str, v: &T) -> R<()> {
        self.s.op(Op::PushStr(k.as_ptr(), k.len()), 1);
        v.serialize(Ser(self.s))?;
        self.s.raw_set();
        Ok(())
    }
    fn close(self) -> R<()> {
        if self.wrapped {
            self.s.raw_set();
        }
        Ok(())
    }
}

fn wrap_variant(s: &mut Stage<'_>, variant: &'static str) {
    s.table(0, 1)
        .op(Op::PushStr(variant.as_ptr(), variant.len()), 1);
}

impl<'a, 'l> ser::Serializer for Ser<'a, 'l> {
    type Ok = ();
    type Error = SerdeError;
    type SerializeSeq = Table<'a, 'l>;
    type SerializeTuple = Table<'a, 'l>;
    type SerializeTupleStruct = Table<'a, 'l>;
    type SerializeTupleVariant = Table<'a, 'l>;
    type SerializeMap = Table<'a, 'l>;
    type SerializeStruct = Table<'a, 'l>;
    type SerializeStructVariant = Table<'a, 'l>;

    fn serialize_bool(self, v: bool) -> R<()> {
        self.0.bool(v);
        Ok(())
    }
    fn serialize_i8(self, v: i8) -> R<()> {
        self.serialize_i64(v.into())
    }
    fn serialize_i16(self, v: i16) -> R<()> {
        self.serialize_i64(v.into())
    }
    fn serialize_i32(self, v: i32) -> R<()> {
        self.serialize_i64(v.into())
    }
    fn serialize_i64(self, v: i64) -> R<()> {
        from_int(v.into())
            .map(|n| {
                self.0.number(n);
            })
            .map_err(SerdeError)
    }
    fn serialize_u8(self, v: u8) -> R<()> {
        self.serialize_i64(v.into())
    }
    fn serialize_u16(self, v: u16) -> R<()> {
        self.serialize_i64(v.into())
    }
    fn serialize_u32(self, v: u32) -> R<()> {
        self.serialize_i64(v.into())
    }
    fn serialize_u64(self, v: u64) -> R<()> {
        from_int(v.into())
            .map(|n| {
                self.0.number(n);
            })
            .map_err(SerdeError)
    }
    fn serialize_f32(self, v: f32) -> R<()> {
        self.0.number(v.into());
        Ok(())
    }
    fn serialize_f64(self, v: f64) -> R<()> {
        self.0.number(v);
        Ok(())
    }
    fn serialize_char(self, v: char) -> R<()> {
        self.serialize_str(v.encode_utf8(&mut [0; 4]))
    }
    fn serialize_str(self, v: &str) -> R<()> {
        self.0.bytes(v.as_bytes());
        Ok(())
    }
    fn serialize_bytes(self, v: &[u8]) -> R<()> {
        self.0.bytes(v);
        Ok(())
    }
    fn serialize_none(self) -> R<()> {
        self.0.nil();
        Ok(())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, v: &T) -> R<()> {
        v.serialize(self)
    }
    fn serialize_unit(self) -> R<()> {
        self.0.nil();
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> R<()> {
        self.serialize_unit()
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, v: &'static str) -> R<()> {
        self.serialize_str(v)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &'static str, v: &T) -> R<()> {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        v: &T,
    ) -> R<()> {
        wrap_variant(self.0, variant);
        v.serialize(Ser(self.0))?;
        self.0.raw_set();
        Ok(())
    }
    fn serialize_seq(self, n: Option<usize>) -> R<Table<'a, 'l>> {
        self.0.table(n.unwrap_or(0), 0);
        Ok(Table {
            s: self.0,
            next: 0,
            wrapped: false,
        })
    }
    fn serialize_tuple(self, n: usize) -> R<Table<'a, 'l>> {
        self.serialize_seq(Some(n))
    }
    fn serialize_tuple_struct(self, _: &'static str, n: usize) -> R<Table<'a, 'l>> {
        self.serialize_seq(Some(n))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        n: usize,
    ) -> R<Table<'a, 'l>> {
        wrap_variant(self.0, variant);
        self.0.table(n, 0);
        Ok(Table {
            s: self.0,
            next: 0,
            wrapped: true,
        })
    }
    fn serialize_map(self, n: Option<usize>) -> R<Table<'a, 'l>> {
        self.0.table(0, n.unwrap_or(0));
        Ok(Table {
            s: self.0,
            next: 0,
            wrapped: false,
        })
    }
    fn serialize_struct(self, _: &'static str, n: usize) -> R<Table<'a, 'l>> {
        self.serialize_map(Some(n))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        n: usize,
    ) -> R<Table<'a, 'l>> {
        wrap_variant(self.0, variant);
        self.0.table(0, n);
        Ok(Table {
            s: self.0,
            next: 0,
            wrapped: true,
        })
    }
}

macro_rules! seq_impl {
    ($($tr:ident :: $m:ident),*) => {$(
        impl ser::$tr for Table<'_, '_> {
            type Ok = ();
            type Error = SerdeError;
            fn $m<T: ?Sized + Serialize>(&mut self, v: &T) -> R<()> { self.item(v) }
            fn end(self) -> R<()> { self.close() }
        }
    )*};
}
seq_impl!(
    SerializeSeq::serialize_element,
    SerializeTuple::serialize_element,
    SerializeTupleStruct::serialize_field,
    SerializeTupleVariant::serialize_field
);

impl ser::SerializeMap for Table<'_, '_> {
    type Ok = ();
    type Error = SerdeError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, k: &T) -> R<()> {
        k.serialize(Ser(self.s))
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, v: &T) -> R<()> {
        v.serialize(Ser(self.s))?;
        self.s.raw_set();
        Ok(())
    }
    fn end(self) -> R<()> {
        self.close()
    }
}

macro_rules! struct_impl {
    ($($tr:ident),*) => {$(
        impl ser::$tr for Table<'_, '_> {
            type Ok = ();
            type Error = SerdeError;
            fn serialize_field<T: ?Sized + Serialize>(&mut self, k: &'static str, v: &T) -> R<()> { self.field(k, v) }
            fn end(self) -> R<()> { self.close() }
        }
    )*};
}
struct_impl!(SerializeStruct, SerializeStructVariant);

/// Deserializer over one stack slot.
pub struct De<'l> {
    lua: &'l Lua,
    idx: c_int,
}

impl<'l> De<'l> {
    fn err(&self, expected: &str) -> SerdeError {
        SerdeError(mismatch(self.lua, self.idx, expected))
    }
    fn is_seq(&self) -> R<bool> {
        let n = len(self.lua, self.idx).map_err(SerdeError)?;
        Ok(n > 0)
    }
}

/// Collects table entries into owned `(key, value)` registry-free stack copies is not possible;
/// instead we visit while iterating, materialising each value through a nested `De`.
fn visit_seq<'de, V: Visitor<'de>>(lua: &Lua, t: c_int, v: V) -> R<V::Value> {
    let n = len(lua, t).map_err(SerdeError)?;
    let mut i = 0usize;
    v.visit_seq(SeqAccess {
        lua,
        t,
        i: &mut i,
        n,
    })
}

struct SeqAccess<'a, 'l> {
    lua: &'l Lua,
    t: c_int,
    i: &'a mut usize,
    n: usize,
}

impl<'de> de::SeqAccess<'de> for SeqAccess<'_, '_> {
    type Error = SerdeError;
    fn next_element_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> R<Option<T::Value>> {
        match *self.i < self.n {
            false => Ok(None),
            true => {
                *self.i += 1;
                let g = self.lua.guard();
                let idx = fetch_index(self.lua, self.t, *self.i).map_err(SerdeError)?;
                let r = seed
                    .deserialize(De { lua: self.lua, idx })
                    .map(Some)
                    .map_err(|e| SerdeError(e.0.at(self.i.to_string())));
                drop(g);
                r
            }
        }
    }
}

/// Map access buffers entries as (key string/number, value-index) pairs by re-reading with
/// `pairs`; values are deserialised inside the iteration callback.
fn visit_map<'de, V: Visitor<'de>>(lua: &Lua, t: c_int, v: V) -> R<V::Value> {
    // Keys must be collected first because serde drives the iteration.
    let mut keys: Vec<String> = Vec::new();
    pairs(lua, t, |k, _| {
        let key = string_bytes(lua, k).map(|b| String::from_utf8_lossy(b).into_owned())?;
        keys.push(key);
        Ok(())
    })
    .map_err(SerdeError)?;
    v.visit_map(MapAccess {
        lua,
        t,
        keys: keys.into_iter(),
        cur: None,
    })
}

struct MapAccess<'l, I> {
    lua: &'l Lua,
    t: c_int,
    keys: I,
    cur: Option<String>,
}

impl<'de, I: Iterator<Item = String>> de::MapAccess<'de> for MapAccess<'_, I> {
    type Error = SerdeError;
    fn next_key_seed<K: de::DeserializeSeed<'de>>(&mut self, seed: K) -> R<Option<K::Value>> {
        match self.keys.next() {
            None => Ok(None),
            Some(k) => {
                let out = seed.deserialize(k.clone().into_deserializer()).map(Some);
                self.cur = Some(k);
                out
            }
        }
    }
    fn next_value_seed<V: de::DeserializeSeed<'de>>(&mut self, seed: V) -> R<V::Value> {
        let k = self.cur.take().unwrap_or_default();
        let g = self.lua.guard();
        let idx = super::table::fetch_field(self.lua, self.t, &k).map_err(SerdeError)?;
        let r = seed
            .deserialize(De { lua: self.lua, idx })
            .map_err(|e| SerdeError(e.0.at(k)));
        drop(g);
        r
    }
}

impl<'de> de::Deserializer<'de> for De<'_> {
    type Error = SerdeError;

    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        let r = self.lua.read(self.idx);
        match r.tag {
            LUA_TNIL | LUA_TNONE => v.visit_unit(),
            LUA_TBOOLEAN => v.visit_bool(r.num != 0.0),
            LUA_TNUMBER if r.num.fract() == 0.0 && r.num.abs() < 9.007_199_254_740_992e15 => {
                v.visit_i64(r.num as i64)
            }
            LUA_TNUMBER => v.visit_f64(r.num),
            LUA_TSTRING => match self.lua.raw_bytes(self.idx).map(std::str::from_utf8) {
                Some(Ok(s)) => v.visit_str(s),
                Some(Err(_)) => v.visit_bytes(self.lua.raw_bytes(self.idx).unwrap_or_default()),
                None => Err(self.err("string")),
            },
            LUA_TTABLE => match self.is_seq()? {
                true => visit_seq(self.lua, self.idx, v),
                false => visit_map(self.lua, self.idx, v),
            },
            _ => Err(self.err("serializable value")),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        match self.lua.read(self.idx).tag {
            LUA_TNIL | LUA_TNONE => v.visit_none(),
            _ => v.visit_some(self),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        expect_table(self.lua, self.idx).map_err(SerdeError)?;
        visit_seq(self.lua, self.idx, v)
    }

    fn deserialize_map<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        expect_table(self.lua, self.idx).map_err(SerdeError)?;
        visit_map(self.lua, self.idx, v)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        v: V,
    ) -> R<V::Value> {
        self.deserialize_map(v)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _: &'static str, v: V) -> R<V::Value> {
        v.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        v: V,
    ) -> R<V::Value> {
        match self.lua.read(self.idx).tag {
            LUA_TSTRING => {
                let s = String::from_lua(self.lua, self.idx).map_err(SerdeError)?;
                v.visit_enum(s.into_deserializer())
            }
            LUA_TTABLE => {
                let mut entry: Option<String> = None;
                pairs(self.lua, self.idx, |k, _| {
                    entry = Some(String::from_lua(self.lua, k)?);
                    Ok(())
                })
                .map_err(SerdeError)?;
                let variant = entry.ok_or_else(|| self.err("enum variant table"))?;
                v.visit_enum(EnumAccess {
                    lua: self.lua,
                    t: self.idx,
                    variant,
                })
            }
            _ => Err(self.err("enum (string or table)")),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        unit unit_struct tuple tuple_struct identifier ignored_any
    }
}

struct EnumAccess<'l> {
    lua: &'l Lua,
    t: c_int,
    variant: String,
}

impl<'de, 'l> de::EnumAccess<'de> for EnumAccess<'l> {
    type Error = SerdeError;
    type Variant = VariantAccess<'l>;
    fn variant_seed<V: de::DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> R<(V::Value, VariantAccess<'l>)> {
        let v = seed.deserialize(self.variant.clone().into_deserializer())?;
        Ok((
            v,
            VariantAccess {
                lua: self.lua,
                t: self.t,
                variant: self.variant,
            },
        ))
    }
}

struct VariantAccess<'l> {
    lua: &'l Lua,
    t: c_int,
    variant: String,
}

impl VariantAccess<'_> {
    fn with<T>(&self, f: impl FnOnce(De<'_>) -> R<T>) -> R<T> {
        let g = self.lua.guard();
        let idx = super::table::fetch_field(self.lua, self.t, &self.variant).map_err(SerdeError)?;
        let r = f(De { lua: self.lua, idx });
        drop(g);
        r
    }
}

impl<'de> de::VariantAccess<'de> for VariantAccess<'_> {
    type Error = SerdeError;
    fn unit_variant(self) -> R<()> {
        Ok(())
    }
    fn newtype_variant_seed<T: de::DeserializeSeed<'de>>(self, seed: T) -> R<T::Value> {
        self.with(|d| seed.deserialize(d))
    }
    fn tuple_variant<V: Visitor<'de>>(self, _: usize, v: V) -> R<V::Value> {
        self.with(|d| de::Deserializer::deserialize_seq(d, v))
    }
    fn struct_variant<V: Visitor<'de>>(self, _: &'static [&'static str], v: V) -> R<V::Value> {
        self.with(|d| de::Deserializer::deserialize_map(d, v))
    }
}
