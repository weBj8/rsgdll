use std::any::TypeId;
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::ffi::CString;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

use rsgdll_platform::__private::{RawLuaBase, RawLuaState, RawUserData};

use crate::{LuaError, LuaResult, LuaType, StackFrame};

static USERDATA_NAMES: OnceLock<Mutex<HashMap<String, TypeId>>> = OnceLock::new();

/// One named userdata type registered in a specific Lua state.
pub struct UserDataType<'lua, T> {
    raw: NonNull<RawLuaBase>,
    lua_type: u8,
    _value: PhantomData<fn() -> T>,
    _state: PhantomData<&'lua mut RawLuaState>,
    _main_thread: PhantomData<Rc<()>>,
}

impl<'lua, T: 'static> UserDataType<'lua, T> {
    pub(crate) unsafe fn register(frame: &mut StackFrame<'_, 'lua>, name: &str) -> LuaResult<Self> {
        let name_c = CString::new(name).map_err(|_| LuaError::StringContainsNul)?;
        let names = USERDATA_NAMES.get_or_init(|| Mutex::new(HashMap::new()));
        let mut names = names
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match names.get(name) {
            Some(type_id) if *type_id != TypeId::of::<T>() => {
                return Err(LuaError::UserDataTypeNameConflict(name.to_owned()));
            }
            Some(_) => {}
            None => {
                names.insert(name.to_owned(), TypeId::of::<T>());
            }
        }
        drop(names);

        // SAFETY: caller guarantees metatable lookup/creation cannot longjmp;
        // CString remains live for this call.
        let raw_type =
            unsafe { RawLuaBase::create_meta_table(frame.raw().as_ptr(), name_c.as_ptr()) };
        let lua_type = u8::try_from(raw_type).map_err(|_| LuaError::UserDataTypeOutOfRange)?;
        // SAFETY: caller accepts stack growth and raw assignment obligations.
        unsafe {
            frame.push("__index")?;
            frame.push_value(-2);
            frame.raw_set(-3)?;
        }
        frame.pop(1)?;
        Ok(Self {
            raw: frame.raw(),
            lua_type,
            _value: PhantomData,
            _state: PhantomData,
            _main_thread: PhantomData,
        })
    }

    /// Pushes one Rust value as typed full userdata.
    ///
    /// # Safety
    ///
    /// Caller must ensure userdata allocation, metatable push, and stack
    /// growth cannot raise a Lua error or longjmp.
    pub unsafe fn push(&self, frame: &mut StackFrame<'_, 'lua>, value: T) -> LuaResult<()> {
        self.ensure_state(frame)?;
        let cell = Box::new(RefCell::new(value));
        let data = Box::into_raw(cell);
        let size = u32::try_from(std::mem::size_of::<RawUserData>())
            .map_err(|_| LuaError::CountOverflow)?;
        // SAFETY: caller guarantees allocation cannot longjmp.
        let header = unsafe { RawLuaBase::new_userdata(self.raw.as_ptr(), size) };
        let Some(mut header) = NonNull::new(header.cast::<RawUserData>()) else {
            // SAFETY: ownership did not transfer because no userdata exists.
            unsafe { drop(Box::from_raw(data)) };
            return Err(LuaError::NullUserData);
        };
        // SAFETY: upstream allocated an initialized writable RawUserData header.
        unsafe {
            header.as_mut().data = data.cast();
            header.as_mut().lua_type = self.lua_type;
        }
        // SAFETY: caller guarantees metatable push cannot longjmp.
        let pushed =
            unsafe { RawLuaBase::push_meta_table(self.raw.as_ptr(), i32::from(self.lua_type)) };
        if !pushed {
            // SAFETY: header belongs to stack-top userdata and points to `data`.
            unsafe { header.as_mut().data = std::ptr::null_mut() };
            // SAFETY: ownership remained local because metatable setup failed.
            unsafe { drop(Box::from_raw(data)) };
            frame.pop(1)?;
            return Err(LuaError::MissingMetaTable);
        }
        // SAFETY: userdata is directly below its metatable; caller guarantees
        // metatable assignment cannot longjmp.
        unsafe { RawLuaBase::set_meta_table(self.raw.as_ptr(), -2) };
        Ok(())
    }

    pub fn borrow<'frame>(
        &self,
        frame: &'frame StackFrame<'_, 'lua>,
        index: i32,
    ) -> LuaResult<Ref<'frame, T>> {
        let cell = self.cell(frame, index)?;
        cell.try_borrow()
            .map_err(|_| LuaError::UserDataBorrowConflict)
    }

    pub fn borrow_mut<'frame>(
        &self,
        frame: &'frame StackFrame<'_, 'lua>,
        index: i32,
    ) -> LuaResult<RefMut<'frame, T>> {
        let cell = self.cell(frame, index)?;
        cell.try_borrow_mut()
            .map_err(|_| LuaError::UserDataBorrowConflict)
    }

    /// Pushes this type's registered metatable.
    ///
    /// # Safety
    ///
    /// Caller must ensure stack growth cannot raise a Lua error or longjmp.
    pub unsafe fn push_metatable(&self, frame: &mut StackFrame<'_, 'lua>) -> LuaResult<()> {
        self.ensure_state(frame)?;
        // SAFETY: caller accepts stack growth's no-longjmp obligation.
        let pushed =
            unsafe { RawLuaBase::push_meta_table(self.raw.as_ptr(), i32::from(self.lua_type)) };
        if pushed {
            Ok(())
        } else {
            Err(LuaError::MissingMetaTable)
        }
    }

    /// Finalizes this userdata value and releases its Rust allocation.
    pub fn finalize(&self, frame: &mut StackFrame<'_, 'lua>, index: i32) -> LuaResult<()> {
        self.ensure_state(frame)?;
        // SAFETY: this type token binds `T` to the expected foreign type ID.
        unsafe { Self::finalize_registered(frame, index, self.lua_type) }
    }

    #[doc(hidden)]
    #[must_use]
    pub const fn type_id(&self) -> u8 {
        self.lua_type
    }

    /// Finalizes userdata after a generated `__gc` callback validates its
    /// captured type ID.
    ///
    /// # Safety
    ///
    /// `lua_type` must have been captured from a [`UserDataType<T>`] installed
    /// with this exact `T`.
    #[doc(hidden)]
    pub unsafe fn finalize_registered(
        frame: &mut StackFrame<'_, 'lua>,
        index: i32,
        lua_type: u8,
    ) -> LuaResult<()> {
        let actual = frame.value_type(index);
        if actual != LuaType::USER_DATA && actual.0 != i32::from(lua_type) {
            return Err(LuaError::TypeMismatch {
                expected: LuaType::USER_DATA,
                actual,
            });
        }
        // SAFETY: exact userdata type was checked before retrieving its header.
        let header = unsafe { RawLuaBase::get_userdata(frame.raw().as_ptr(), index) };
        let mut header =
            NonNull::new(header.cast::<RawUserData>()).ok_or(LuaError::NullUserData)?;
        // SAFETY: upstream returned a readable RawUserData header.
        if unsafe { header.as_ref().lua_type } != lua_type {
            return Err(LuaError::UserDataTypeMismatch);
        }
        // SAFETY: upstream returned a readable RawUserData header.
        let data = NonNull::new(unsafe { header.as_ref().data }.cast::<RefCell<T>>())
            .ok_or(LuaError::FinalizedUserData)?;
        // Invalidate before destructor execution, including a panicking Drop.
        // SAFETY: header is live writable userdata storage.
        unsafe { header.as_mut().data = std::ptr::null_mut() };
        // SAFETY: exact T/type registration proves this pointer came from one
        // Box::into_raw in `push`, and invalidation above prevents double free.
        unsafe { drop(Box::from_raw(data.as_ptr())) };
        Ok(())
    }

    pub(crate) fn ensure_state(&self, frame: &StackFrame<'_, 'lua>) -> LuaResult<()> {
        if self.raw == frame.raw() {
            Ok(())
        } else {
            Err(LuaError::WrongState)
        }
    }

    fn cell<'frame>(
        &self,
        frame: &'frame StackFrame<'_, 'lua>,
        index: i32,
    ) -> LuaResult<&'frame RefCell<T>> {
        self.ensure_state(frame)?;
        let actual = frame.value_type(index);
        if actual != LuaType::USER_DATA && actual.0 != i32::from(self.lua_type) {
            return Err(LuaError::TypeMismatch {
                expected: LuaType::USER_DATA,
                actual,
            });
        }
        // SAFETY: exact userdata type was checked before retrieving its header.
        let header = unsafe { RawLuaBase::get_userdata(self.raw.as_ptr(), index) };
        let header = NonNull::new(header.cast::<RawUserData>()).ok_or(LuaError::NullUserData)?;
        // SAFETY: upstream returned a readable RawUserData header.
        let header = unsafe { header.as_ref() };
        if header.lua_type != self.lua_type {
            return Err(LuaError::UserDataTypeMismatch);
        }
        let data =
            NonNull::new(header.data.cast::<RefCell<T>>()).ok_or(LuaError::FinalizedUserData)?;
        // SAFETY: matching Rust type registration and lua_type prove this data
        // pointer came from `Box<RefCell<T>>` in `push`; frame lifetime keeps
        // userdata live while the returned borrow exists.
        Ok(unsafe { data.as_ref() })
    }
}
