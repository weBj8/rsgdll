use std::any::TypeId;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Mutex, MutexGuard, OnceLock};

use rsgdll_platform::__private::{RawLuaBase, RawUserData};

use crate::{LuaError, LuaResult};

static USERDATA_ALLOCATIONS: OnceLock<Mutex<HashMap<usize, UserDataAllocation>>> = OnceLock::new();

#[derive(Clone, Copy)]
pub(super) struct UserDataPointer(pub(super) NonNull<c_void>);

// SAFETY: [Category 9 — Send/Sync] this wrapper only transports and compares
// an opaque pointer inside the allocation registry. Pointee access requires a
// main-thread-bound `UserDataType`, then validates state, Lua tag, and TypeId.
unsafe impl Send for UserDataPointer {}

#[derive(Clone, Copy)]
pub(super) struct UserDataAllocation {
    pub(super) data: UserDataPointer,
    pub(super) lua_base: usize,
    pub(super) lua_type: u8,
    pub(super) rust_type: TypeId,
    pub(super) dropper: unsafe fn(UserDataPointer),
}

pub(super) fn userdata_allocations() -> MutexGuard<'static, HashMap<usize, UserDataAllocation>> {
    match USERDATA_ALLOCATIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        Ok(allocations) => allocations,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(super) fn allocation<T: 'static>(
    raw: NonNull<RawLuaBase>,
    header: NonNull<RawUserData>,
    lua_type: u8,
) -> LuaResult<UserDataAllocation> {
    let allocations = userdata_allocations();
    let allocation = allocations
        .get(&header.as_ptr().addr())
        .copied()
        .ok_or(LuaError::UserDataTypeMismatch)?;
    validate_allocation::<T>(raw, header, lua_type, allocation)?;
    Ok(allocation)
}

pub(super) fn remove_allocation<T: 'static>(
    raw: NonNull<RawLuaBase>,
    mut header: NonNull<RawUserData>,
    lua_type: u8,
) -> LuaResult<UserDataAllocation> {
    let mut allocations = userdata_allocations();
    let allocation = allocations
        .get(&header.as_ptr().addr())
        .copied()
        .ok_or(LuaError::UserDataTypeMismatch)?;
    validate_allocation::<T>(raw, header, lua_type, allocation)?;
    // SAFETY: validation proved this is our writable header.
    unsafe { header.as_mut().data = std::ptr::null_mut() };
    allocations.remove(&header.as_ptr().addr());
    Ok(allocation)
}

pub(super) fn remove_allocation_erased(
    raw: NonNull<RawLuaBase>,
    mut header: NonNull<RawUserData>,
    lua_type: u8,
) -> LuaResult<UserDataAllocation> {
    let mut allocations = userdata_allocations();
    let allocation = allocations
        .get(&header.as_ptr().addr())
        .copied()
        .ok_or(LuaError::UserDataTypeMismatch)?;
    validate_identity(raw, header, lua_type, allocation)?;
    // SAFETY: validation proved this is our writable header.
    unsafe { header.as_mut().data = std::ptr::null_mut() };
    allocations.remove(&header.as_ptr().addr());
    Ok(allocation)
}

fn validate_allocation<T: 'static>(
    raw: NonNull<RawLuaBase>,
    header: NonNull<RawUserData>,
    lua_type: u8,
    allocation: UserDataAllocation,
) -> LuaResult<()> {
    validate_identity(raw, header, lua_type, allocation)?;
    if allocation.rust_type != TypeId::of::<T>() {
        return Err(LuaError::UserDataTypeMismatch);
    }
    Ok(())
}

fn validate_identity(
    raw: NonNull<RawLuaBase>,
    header: NonNull<RawUserData>,
    lua_type: u8,
    allocation: UserDataAllocation,
) -> LuaResult<()> {
    if allocation.lua_base != raw.as_ptr().addr() || allocation.lua_type != lua_type {
        return Err(LuaError::UserDataTypeMismatch);
    }
    // SAFETY: upstream returned a readable RawUserData ABI header.
    let header = unsafe { header.as_ref() };
    if header.lua_type != allocation.lua_type || header.data != allocation.data.0.as_ptr() {
        return Err(LuaError::UserDataTypeMismatch);
    }
    Ok(())
}

pub(super) unsafe fn drop_allocation<T: 'static>(pointer: UserDataPointer) {
    // SAFETY: caller removed the unique allocation record for the pointer
    // originally produced by `Box<RefCell<T>>`.
    unsafe {
        drop(Box::from_raw(
            pointer.0.cast::<std::cell::RefCell<T>>().as_ptr(),
        ))
    };
}

#[cfg(test)]
mod tests {
    use super::{UserDataAllocation, UserDataPointer};
    use std::any::TypeId;
    use std::cell::RefCell;
    use std::ptr::NonNull;

    #[test]
    fn stored_userdata_pointer_retains_provenance() {
        // Given: one live Rust allocation stored as userdata metadata.
        let pointer = NonNull::new(Box::into_raw(Box::new(RefCell::new(41_u64))))
            .expect("Box pointers are non-null");
        let allocation = UserDataAllocation {
            data: UserDataPointer(pointer.cast()),
            lua_base: 0,
            lua_type: 1,
            rust_type: TypeId::of::<u64>(),
            dropper: super::drop_allocation::<u64>,
        };

        // When: metadata restores the original typed pointer.
        let restored = allocation.data.0.cast::<RefCell<u64>>();

        // Then: access and ownership recovery use retained provenance.
        // SAFETY: [Category 11 — provenance] `restored` preserves the exact
        // pointer produced by `Box::into_raw`, and this test owns it uniquely.
        unsafe { restored.as_ref().replace(42) };
        // SAFETY: [Category 11 — provenance] same retained live pointer; no
        // mutable reference exists after the previous statement.
        assert_eq!(*unsafe { restored.as_ref() }.borrow(), 42);
        // SAFETY: [Category 12 — invalid free] this is the sole recovery of
        // the pointer produced by `Box::into_raw`.
        unsafe { drop(Box::from_raw(restored.as_ptr())) };
    }
}
