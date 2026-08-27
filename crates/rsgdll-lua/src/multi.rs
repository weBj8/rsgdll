use crate::{FromLua, IntoLua, LuaResult, StackFrame};

/// Pushes zero or more Rust values as Lua call arguments.
pub trait IntoLuaMulti {
    fn count(&self) -> usize;

    /// Pushes every argument in call order.
    fn push(self, frame: &mut StackFrame<'_, '_>) -> LuaResult<()>;
}

/// Reads a fixed number of protected Lua call results.
pub trait FromLuaMulti: Sized {
    const COUNT: usize;

    fn read(frame: &StackFrame<'_, '_>, first: i32) -> LuaResult<Self>;
}

impl IntoLuaMulti for () {
    fn count(&self) -> usize {
        0
    }

    fn push(self, _: &mut StackFrame<'_, '_>) -> LuaResult<()> {
        Ok(())
    }
}

impl FromLuaMulti for () {
    const COUNT: usize = 0;

    fn read(_: &StackFrame<'_, '_>, _: i32) -> LuaResult<Self> {
        Ok(())
    }
}

macro_rules! multi {
    ($count:expr; $(($type:ident, $value:ident, $offset:expr)),+ $(,)?) => {
        impl<$($type: IntoLua),+> IntoLuaMulti for ($($type,)+) {
            fn count(&self) -> usize {
                $count
            }

            fn push(self, frame: &mut StackFrame<'_, '_>) -> LuaResult<()> {
                let ($($value,)+) = self;
                $(
                    frame.push($value)?;
                )+
                Ok(())
            }
        }

        impl<$($type: FromLua),+> FromLuaMulti for ($($type,)+) {
            const COUNT: usize = $count;

            fn read(frame: &StackFrame<'_, '_>, first: i32) -> LuaResult<Self> {
                Ok(($(
                    frame.get(first + $offset)?,
                )+))
            }
        }
    };
}

multi!(1; (A, a, 0));
multi!(2; (A, a, 0), (B, b, 1));
multi!(3; (A, a, 0), (B, b, 1), (C, c, 2));
multi!(4; (A, a, 0), (B, b, 1), (C, c, 2), (D, d, 3));
multi!(5; (A, a, 0), (B, b, 1), (C, c, 2), (D, d, 3), (E, e, 4));
multi!(
    6;
    (A, a, 0),
    (B, b, 1),
    (C, c, 2),
    (D, d, 3),
    (E, e, 4),
    (F, f, 5)
);
