use std::any::TypeId;
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::ffi::CString;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

use rsgdll_platform::__private::{RawLuaBase, RawLuaState, RawUserData};

use crate::{Lua, LuaError, LuaResult, LuaType, StackFrame, protected};

mod allocation;
mod panic_payload;

use allocation::{
    UserDataAllocation, UserDataPointer, allocation, drop_allocation, remove_allocation,
    remove_allocation_erased, userdata_allocations,
};
use panic_payload::drop_caught_panic;

static USERDATA_NAMES: OnceLock<Mutex<HashMap<String, TypeId>>> = OnceLock::new();

/// One named userdata type registered in a specific Lua state.
///
/// Lua may finalize unreachable values during any protected Lua operation.
/// `T::drop` must not wait on locks held across such an operation.
pub struct UserDataType<'lua, T> {
    state: NonNull<RawLuaState>,
    raw: NonNull<RawLuaBase>,
    lua_type: u8,
    _value: PhantomData<fn() -> T>,
    _state: PhantomData<&'lua mut RawLuaState>,
    _main_thread: PhantomData<Rc<()>>,
}

impl<'lua, T: 'static> UserDataType<'lua, T> {
    pub(crate) fn register(frame: &mut StackFrame<'_, 'lua>, name: &str) -> LuaResult<Self> {
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

        let raw_type =
            protected::create_meta_table(frame.context(), name_c.as_bytes_with_nul().as_ptr())?;
        let lua_type = u8::try_from(raw_type).map_err(|_| LuaError::UserDataTypeOutOfRange)?;
        frame.push("__index")?;
        frame.push_value(-2)?;
        frame.raw_set(-3)?;
        frame.push("__gc")?;
        frame.push(f64::from(lua_type))?;
        frame.push_c_closure(userdata_gc, 1)?;
        frame.raw_set(-3)?;
        frame.pop(1)?;
        Ok(Self {
            state: frame.state(),
            raw: frame.raw(),
            lua_type,
            _value: PhantomData,
            _state: PhantomData,
            _main_thread: PhantomData,
        })
    }

    /// Pushes one Rust value as typed full userdata.
    pub fn push(&self, frame: &mut StackFrame<'_, 'lua>, value: T) -> LuaResult<()> {
        self.ensure_state(frame)?;
        let cell = Box::new(RefCell::new(value));
        let size = u32::try_from(std::mem::size_of::<RawUserData>())
            .map_err(|_| LuaError::CountOverflow)?;
        let header = protected::new_userdata(protected::Context::new(self.state, self.raw), size)?;
        let Some(mut header) = NonNull::new(header.cast::<RawUserData>()) else {
            return Err(LuaError::NullUserData);
        };
        let data = NonNull::from(cell.as_ref());
        let context = protected::Context::new(self.state, self.raw);
        protected::set_user_type(context, -1, data.as_ptr().cast())?;
        let allocation = UserDataAllocation {
            data: UserDataPointer(data.cast()),
            lua_base: self.raw.as_ptr().addr(),
            lua_type: self.lua_type,
            rust_type: TypeId::of::<T>(),
            dropper: drop_allocation::<T>,
        };
        let key = header.as_ptr().addr();
        let inserted = {
            use std::collections::hash_map::Entry;

            match userdata_allocations().entry(key) {
                Entry::Vacant(entry) => {
                    entry.insert(allocation);
                    true
                }
                Entry::Occupied(_) => false,
            }
        };
        if !inserted {
            let _ = protected::set_user_type(context, -1, std::ptr::null_mut());
            frame.pop(1)?;
            return Err(LuaError::UserDataTypeMismatch);
        }
        // SAFETY: [UB category 8] `NewUserdata` returned a live writable
        // `RawUserData` header with the pinned ABI layout.
        unsafe { header.as_mut().lua_type = self.lua_type };
        let setup = (|| {
            let pushed = protected::push_meta_table(context, i32::from(self.lua_type))?;
            if !pushed {
                return Err(LuaError::MissingMetaTable);
            }
            protected::set_meta_table(context, -2)
        })();
        if let Err(error) = setup {
            userdata_allocations().remove(&key);
            let _ = protected::set_user_type(context, -1, std::ptr::null_mut());
            frame.pop(1)?;
            return Err(error);
        }
        let _ = Box::into_raw(cell);
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
    pub fn push_metatable(&self, frame: &mut StackFrame<'_, 'lua>) -> LuaResult<()> {
        self.ensure_state(frame)?;
        let pushed = protected::push_meta_table(
            protected::Context::new(self.state, self.raw),
            i32::from(self.lua_type),
        )?;
        if pushed {
            Ok(())
        } else {
            Err(LuaError::MissingMetaTable)
        }
    }

    /// Finalizes this userdata value and releases its Rust allocation.
    pub fn finalize(&self, frame: &mut StackFrame<'_, 'lua>, index: i32) -> LuaResult<()> {
        self.ensure_state(frame)?;
        Self::finalize_registered(frame, index, self.lua_type)
    }

    #[doc(hidden)]
    #[must_use]
    pub(crate) const fn type_id(&self) -> u8 {
        self.lua_type
    }

    /// Finalizes userdata after a generated `__gc` callback validates its
    /// captured type ID.
    ///
    #[doc(hidden)]
    pub(crate) fn finalize_registered(
        frame: &mut StackFrame<'_, 'lua>,
        index: i32,
        lua_type: u8,
    ) -> LuaResult<()> {
        let actual = frame.value_type(index);
        if actual == LuaType::USER_DATA {
            return Err(LuaError::UserDataTypeMismatch);
        }
        if actual.0 != i32::from(lua_type) {
            return Err(LuaError::TypeMismatch {
                expected: LuaType::USER_DATA,
                actual,
            });
        }
        // SAFETY: exact userdata type was checked before retrieving its header.
        let header = unsafe { RawLuaBase::get_userdata(frame.raw().as_ptr(), index) };
        let header = NonNull::new(header.cast::<RawUserData>()).ok_or(LuaError::NullUserData)?;
        // SAFETY: upstream returned a readable RawUserData header.
        if unsafe { header.as_ref().data }.is_null() {
            return Err(LuaError::FinalizedUserData);
        }
        let allocation = remove_allocation::<T>(frame.raw(), header, lua_type)?;
        // SAFETY: [Category 12 — invalid free] registry removal proves unique
        // ownership, and the retained pointer came from this Box allocation.
        unsafe { (allocation.dropper)(allocation.data) };
        Ok(())
    }

    pub(crate) fn ensure_state(&self, frame: &StackFrame<'_, 'lua>) -> LuaResult<()> {
        if self.raw == frame.raw() && self.state == frame.state() {
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
        if actual == LuaType::USER_DATA {
            return Err(LuaError::UserDataTypeMismatch);
        }
        if actual.0 != i32::from(self.lua_type) {
            return Err(LuaError::TypeMismatch {
                expected: LuaType::USER_DATA,
                actual,
            });
        }
        // SAFETY: exact userdata type was checked before retrieving its header.
        let header = unsafe { RawLuaBase::get_userdata(self.raw.as_ptr(), index) };
        let header = NonNull::new(header.cast::<RawUserData>()).ok_or(LuaError::NullUserData)?;
        // SAFETY: upstream returned a readable RawUserData header.
        if unsafe { header.as_ref().data }.is_null() {
            return Err(LuaError::FinalizedUserData);
        }
        let allocation = allocation::<T>(self.raw, header, self.lua_type)?;
        let data = allocation.data.0.cast::<RefCell<T>>();
        // SAFETY: [Category 11 — provenance] registry ownership retains the
        // original allocation pointer; frame lifetime keeps userdata live.
        Ok(unsafe { data.as_ref() })
    }
}

unsafe extern "C" fn userdata_gc(state: *mut RawLuaState) -> i32 {
    let finalize = || -> LuaResult<()> {
        let state = NonNull::new(state).ok_or(LuaError::NullState)?;
        // SAFETY: Lua invokes this callback with its live state. This read does
        // not create a second safe Lua capability.
        let raw = NonNull::new(unsafe { RawLuaState::lua_base(state.as_ptr()) })
            .ok_or(LuaError::NullLuaBase)?;
        let upvalue = Lua::upvalue_index(1)?;
        // SAFETY: GetType accepts every stack index and cannot raise Lua errors.
        if LuaType(unsafe { RawLuaBase::get_type(raw.as_ptr(), upvalue) }) != LuaType::NUMBER {
            return Err(LuaError::UserDataTypeMismatch);
        }
        // SAFETY: exact number type was checked above; GetNumber is nonthrowing.
        let captured_type = unsafe { RawLuaBase::get_number(raw.as_ptr(), upvalue) };
        if !captured_type.is_finite()
            || captured_type.fract() != 0.0
            || !(0.0..=f64::from(u8::MAX)).contains(&captured_type)
        {
            return Err(LuaError::UserDataTypeOutOfRange);
        }
        let lua_type = captured_type as u8;
        // SAFETY: GetType accepts every stack index and cannot raise Lua errors.
        let actual = unsafe { RawLuaBase::get_type(raw.as_ptr(), 1) };
        if actual != i32::from(lua_type) {
            return Err(LuaError::TypeMismatch {
                expected: LuaType(lua_type.into()),
                actual: LuaType(actual),
            });
        }
        // SAFETY: exact registered userdata type was checked before retrieving
        // its framework-owned header.
        let header = unsafe { RawLuaBase::get_userdata(raw.as_ptr(), 1) };
        let header = NonNull::new(header.cast::<RawUserData>()).ok_or(LuaError::NullUserData)?;
        let allocation = remove_allocation_erased(raw, header, lua_type)?;
        // SAFETY: registry removal proved unique ownership and retained the
        // matching monomorphized destructor.
        unsafe { (allocation.dropper)(allocation.data) };
        Ok(())
    };
    if let Err(payload) = catch_unwind(AssertUnwindSafe(finalize)) {
        drop_caught_panic(payload);
    }
    0
}
