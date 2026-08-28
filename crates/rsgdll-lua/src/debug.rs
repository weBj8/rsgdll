use std::cell::Cell;
use std::ffi::{CStr, c_int};
use std::marker::PhantomData;
use std::ops::{BitOr, BitOrAssign};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::rc::Rc;

use rsgdll_platform::__private::{
    LUA_HOOK_CALL, LUA_HOOK_COUNT, LUA_HOOK_LINE, LUA_HOOK_RETURN, LUA_HOOK_TAIL_RETURN,
    LUA_MASK_CALL, LUA_MASK_COUNT, LUA_MASK_LINE, LUA_MASK_RETURN, LuaHook, RawLuaDebug,
    RawLuaState,
};

use crate::{FromLua, IntoLua, Lua, LuaBytes, LuaError, LuaResult, LuaType, StackFrame};

/// Lua debug hook event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DebugEvent {
    Call,
    Return,
    Line,
    Count,
    TailReturn,
    Unknown(c_int),
}

impl DebugEvent {
    #[must_use]
    pub const fn from_raw(event: c_int) -> Self {
        match event {
            LUA_HOOK_CALL => Self::Call,
            LUA_HOOK_RETURN => Self::Return,
            LUA_HOOK_LINE => Self::Line,
            LUA_HOOK_COUNT => Self::Count,
            LUA_HOOK_TAIL_RETURN => Self::TailReturn,
            event => Self::Unknown(event),
        }
    }
}

/// Bit mask selecting Lua debug hook events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugMask(c_int);

impl DebugMask {
    pub const NONE: Self = Self(0);
    pub const CALLS: Self = Self(LUA_MASK_CALL);
    pub const RETURNS: Self = Self(LUA_MASK_RETURN);
    pub const LINES: Self = Self(LUA_MASK_LINE);
    pub const COUNTS: Self = Self(LUA_MASK_COUNT);

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    const fn raw(self) -> c_int {
        self.0
    }
}

impl BitOr for DebugMask {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for DebugMask {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Callback invoked by Lua while one debug event and its VM frames are valid.
pub type DebugHook = for<'event> fn(DebugContext<'event>);

#[derive(Clone, Copy)]
struct ActiveHook {
    state: NonNull<RawLuaState>,
    callback: DebugHook,
    generation: u64,
}

thread_local! {
    static ACTIVE_HOOK: Cell<Option<ActiveHook>> = const { Cell::new(None) };
    static NEXT_GENERATION: Cell<u64> = const { Cell::new(1) };
}

/// Explicit ownership token for one installed Lua debug hook.
///
/// Call [`Self::restore`] with the originating Lua state before discarding
/// this token. Dropping it cannot safely touch a foreign VM whose lifetime is
/// managed by Garry's Mod.
#[must_use = "restore the previous Lua hook before discarding this guard"]
pub struct DebugHookGuard {
    state: NonNull<RawLuaState>,
    previous_hook: LuaHook,
    previous_mask: c_int,
    previous_count: c_int,
    generation: u64,
    active: bool,
    _main_thread: PhantomData<Rc<()>>,
}

impl DebugHookGuard {
    /// Restores the hook, mask, and count captured during installation.
    pub fn restore(&mut self, lua: &mut Lua<'_>) -> LuaResult<()> {
        if !self.active {
            return Ok(());
        }
        if lua.state() != self.state {
            return Err(LuaError::DebugHookWrongState);
        }
        // SAFETY: state equality proves this is the live originating callback
        // state; captured hook metadata came from that same Lua VM.
        let installed = unsafe {
            rsgdll_bridge::debug_set_hook(
                self.state.as_ptr(),
                self.previous_hook,
                self.previous_mask,
                self.previous_count,
            )
        };
        if installed == 0 {
            return Err(LuaError::DebugHookInstallFailed);
        }
        ACTIVE_HOOK.with(|active| {
            if active
                .get()
                .is_some_and(|hook| hook.state == self.state && hook.generation == self.generation)
            {
                active.set(None);
            }
        });
        self.active = false;
        Ok(())
    }

    /// Restores this hook through an ordinary module callback frame.
    pub fn restore_with_frame(&mut self, frame: &mut StackFrame<'_, '_>) -> LuaResult<()> {
        self.restore(frame.lua_mut())
    }

    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }
}

impl StackFrame<'_, '_> {
    /// Installs a Lua debug hook from an ordinary module callback.
    pub fn install_debug_hook(
        &mut self,
        mask: DebugMask,
        count: u32,
        callback: DebugHook,
    ) -> LuaResult<DebugHookGuard> {
        self.lua_mut().install_debug_hook(mask, count, callback)
    }
}

impl Lua<'_> {
    /// Installs one callback-scoped Lua debug hook and captures its predecessor.
    pub fn install_debug_hook(
        &mut self,
        mask: DebugMask,
        count: u32,
        callback: DebugHook,
    ) -> LuaResult<DebugHookGuard> {
        let count = c_int::try_from(count).map_err(|_| LuaError::CountOverflow)?;
        let generation = ACTIVE_HOOK.with(|active| {
            if active.get().is_some() {
                return Err(LuaError::DebugHookAlreadyInstalled);
            }
            NEXT_GENERATION.with(|next| {
                let generation = next.get();
                next.set(generation.wrapping_add(1).max(1));
                Ok(generation)
            })
        })?;
        // SAFETY: dispatcher catches panics and borrows pointers only for one
        // C++-scoped hook invocation.
        unsafe { rsgdll_bridge::set_debug_dispatcher(dispatch_debug_hook) };
        let state = self.state();
        // SAFETY: this checked Lua handle owns main-thread access to the state.
        let previous_hook = unsafe { rsgdll_bridge::debug_get_hook(state.as_ptr()) };
        // SAFETY: this checked Lua handle owns main-thread access to the state.
        let previous_mask = unsafe { rsgdll_bridge::debug_get_hook_mask(state.as_ptr()) };
        // SAFETY: this checked Lua handle owns main-thread access to the state.
        let previous_count = unsafe { rsgdll_bridge::debug_get_hook_count(state.as_ptr()) };
        // SAFETY: bridge hook has the pinned Lua hook ABI and remains loaded
        // for the module's process lifetime.
        let installed = unsafe {
            rsgdll_bridge::debug_set_hook(
                state.as_ptr(),
                rsgdll_bridge::debug_hook(),
                mask.raw(),
                count,
            )
        };
        if installed == 0 {
            return Err(LuaError::DebugHookInstallFailed);
        }
        ACTIVE_HOOK.with(|active| {
            active.set(Some(ActiveHook {
                state,
                callback,
                generation,
            }));
        });
        Ok(DebugHookGuard {
            state,
            previous_hook,
            previous_mask,
            previous_count,
            generation,
            active: true,
            _main_thread: PhantomData,
        })
    }
}

unsafe extern "C" fn dispatch_debug_hook(state: *mut RawLuaState, record: *mut RawLuaDebug) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(state_pointer) = NonNull::new(state) else {
            return;
        };
        let Some(record) = NonNull::new(record) else {
            return;
        };
        let callback = ACTIVE_HOOK.with(|active| {
            active
                .get()
                .filter(|hook| hook.state == state_pointer)
                .map(|hook| hook.callback)
        });
        let Some(callback) = callback else {
            return;
        };
        // SAFETY: C++ invokes this only for one live main-thread Lua hook call.
        let Ok(lua) = (unsafe { Lua::from_raw(state) }) else {
            return;
        };
        // SAFETY: Lua initializes `event` and private call information before
        // invoking a hook. Other record fields remain unread until getinfo.
        let event = unsafe { record.as_ptr().cast::<c_int>().read() };
        callback(DebugContext {
            lua,
            record,
            event: DebugEvent::from_raw(event),
        });
    }));
}

/// Checked access valid only during one Lua debug hook callback.
pub struct DebugContext<'event> {
    lua: Lua<'event>,
    record: NonNull<RawLuaDebug>,
    event: DebugEvent,
}

impl<'event> DebugContext<'event> {
    #[must_use]
    pub const fn event(&self) -> DebugEvent {
        self.event
    }

    /// Borrows the frame that triggered this hook event.
    pub fn current_frame<'frame>(&'frame mut self) -> DebugFrame<'frame, 'event> {
        let mut record = RawLuaDebug::empty();
        record.event = match self.event {
            DebugEvent::Call => LUA_HOOK_CALL,
            DebugEvent::Return => LUA_HOOK_RETURN,
            DebugEvent::Line => LUA_HOOK_LINE,
            DebugEvent::Count => LUA_HOOK_COUNT,
            DebugEvent::TailReturn => LUA_HOOK_TAIL_RETURN,
            DebugEvent::Unknown(event) => event,
        };
        // SAFETY: hook records always initialize private call information.
        record.private_call_info =
            unsafe { std::ptr::addr_of!((*self.record.as_ptr()).private_call_info).read() };
        DebugFrame {
            lua: &mut self.lua,
            record,
        }
    }

    /// Returns one stack frame, where level zero is the current Lua function.
    pub fn frame<'frame>(
        &'frame mut self,
        level: c_int,
    ) -> LuaResult<Option<DebugFrame<'frame, 'event>>> {
        if level < 0 {
            return Err(LuaError::InvalidDebugPosition);
        }
        let mut record = RawLuaDebug::empty();
        // SAFETY: pointers belong to this callback's live main-thread state.
        let found = unsafe {
            rsgdll_bridge::debug_get_stack(self.lua.state().as_ptr(), level, &mut record)
        };
        Ok((found != 0).then_some(DebugFrame {
            lua: &mut self.lua,
            record,
        }))
    }
}

/// Callback-scoped checked Lua stack frame.
pub struct DebugFrame<'frame, 'lua> {
    lua: &'frame mut Lua<'lua>,
    record: RawLuaDebug,
}

impl<'lua> DebugFrame<'_, 'lua> {
    /// Loads owned source and function metadata for this frame.
    pub fn info(&mut self) -> LuaResult<DebugFrameInfo> {
        // SAFETY: record belongs to this live state; `nSlu` initializes every
        // field copied below and does not request a stack result.
        let available = unsafe {
            rsgdll_bridge::debug_get_info(
                self.lua.state().as_ptr(),
                c"nSlu".as_ptr(),
                &mut self.record,
            )
        };
        if available == 0 {
            return Err(LuaError::DebugInfoUnavailable);
        }
        Ok(DebugFrameInfo {
            name: copy_c_string(self.record.name),
            name_kind: copy_c_string(self.record.name_what),
            function_kind: copy_c_string(self.record.what),
            source: copy_c_string(self.record.source),
            current_line: self.record.current_line,
            upvalue_count: self.record.upvalue_count,
            line_defined: self.record.line_defined,
            last_line_defined: self.record.last_line_defined,
            short_source: copy_short_source(&self.record.short_source),
        })
    }

    /// Pushes and borrows one local until the returned guard is dropped.
    pub fn local(&mut self, index: c_int) -> LuaResult<Option<DebugLocal<'_, 'lua>>> {
        if index <= 0 {
            return Err(LuaError::InvalidDebugPosition);
        }
        let record = &self.record;
        let frame = StackFrame::new(self.lua);
        // SAFETY: frame record and state are live; reserved callback capacity
        // prevents this non-allocating value copy from overflowing the stack.
        let name = unsafe { rsgdll_bridge::debug_get_local(frame.state().as_ptr(), record, index) };
        let Some(name) = copy_c_string(name) else {
            return Ok(None);
        };
        Ok(Some(DebugLocal { name, frame }))
    }

    /// Replaces one local with a checked Rust value.
    pub fn set_local<T: IntoLua>(&mut self, index: c_int, value: T) -> LuaResult<Option<LuaBytes>> {
        if index <= 0 {
            return Err(LuaError::InvalidDebugPosition);
        }
        let record = &self.record;
        let mut frame = StackFrame::new(self.lua);
        frame.push(value)?;
        // SAFETY: value was pushed in this frame and Lua consumes it.
        let name = unsafe { rsgdll_bridge::debug_set_local(frame.state().as_ptr(), record, index) };
        frame.finish()?;
        Ok(copy_c_string(name))
    }

    /// Pushes and borrows one function upvalue until its guard is dropped.
    pub fn upvalue(&mut self, index: c_int) -> LuaResult<Option<DebugUpvalue<'_, 'lua>>> {
        if index <= 0 {
            return Err(LuaError::InvalidDebugPosition);
        }
        let record = &mut self.record;
        let frame = StackFrame::new(self.lua);
        // SAFETY: `f` requests this frame's function on the checked stack.
        let available =
            unsafe { rsgdll_bridge::debug_get_info(frame.state().as_ptr(), c"f".as_ptr(), record) };
        if available == 0 {
            return Err(LuaError::DebugInfoUnavailable);
        }
        // SAFETY: getinfo just pushed the current function at stack index -1.
        let name = unsafe { rsgdll_bridge::debug_get_upvalue(frame.state().as_ptr(), -1, index) };
        let Some(name) = copy_c_string(name) else {
            return Ok(None);
        };
        Ok(Some(DebugUpvalue { name, frame }))
    }

    /// Replaces one function upvalue with a checked Rust value.
    pub fn set_upvalue<T: IntoLua>(
        &mut self,
        index: c_int,
        value: T,
    ) -> LuaResult<Option<LuaBytes>> {
        if index <= 0 {
            return Err(LuaError::InvalidDebugPosition);
        }
        let record = &mut self.record;
        let mut frame = StackFrame::new(self.lua);
        // SAFETY: `f` requests this frame's function on the checked stack.
        let available =
            unsafe { rsgdll_bridge::debug_get_info(frame.state().as_ptr(), c"f".as_ptr(), record) };
        if available == 0 {
            return Err(LuaError::DebugInfoUnavailable);
        }
        frame.push(value)?;
        // SAFETY: function is at -2 and the checked value at -1 is consumed.
        let name = unsafe { rsgdll_bridge::debug_set_upvalue(frame.state().as_ptr(), -2, index) };
        frame.finish()?;
        Ok(copy_c_string(name))
    }
}

/// Owned metadata copied from one callback-scoped Lua frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugFrameInfo {
    pub name: Option<LuaBytes>,
    pub name_kind: Option<LuaBytes>,
    pub function_kind: Option<LuaBytes>,
    pub source: Option<LuaBytes>,
    pub current_line: c_int,
    pub upvalue_count: c_int,
    pub line_defined: c_int,
    pub last_line_defined: c_int,
    pub short_source: LuaBytes,
}

/// One local whose value remains at stack index `-1` for this guard's scope.
pub struct DebugLocal<'value, 'lua> {
    name: LuaBytes,
    frame: StackFrame<'value, 'lua>,
}

impl<'value, 'lua> DebugLocal<'value, 'lua> {
    #[must_use]
    pub fn name(&self) -> &LuaBytes {
        &self.name
    }

    #[must_use]
    pub fn value_type(&self) -> LuaType {
        self.frame.value_type(-1)
    }

    pub fn get<T: FromLua>(&self) -> LuaResult<T> {
        self.frame.get(-1)
    }

    pub fn frame(&mut self) -> &mut StackFrame<'value, 'lua> {
        &mut self.frame
    }
}

/// One upvalue whose value remains at stack index `-1` for this guard's scope.
pub struct DebugUpvalue<'value, 'lua> {
    name: LuaBytes,
    frame: StackFrame<'value, 'lua>,
}

impl<'value, 'lua> DebugUpvalue<'value, 'lua> {
    #[must_use]
    pub fn name(&self) -> &LuaBytes {
        &self.name
    }

    #[must_use]
    pub fn value_type(&self) -> LuaType {
        self.frame.value_type(-1)
    }

    pub fn get<T: FromLua>(&self) -> LuaResult<T> {
        self.frame.get(-1)
    }

    pub fn frame(&mut self) -> &mut StackFrame<'value, 'lua> {
        &mut self.frame
    }
}

fn copy_c_string(value: *const std::ffi::c_char) -> Option<LuaBytes> {
    if value.is_null() {
        return None;
    }
    // SAFETY: pinned debug API returns a live NUL-terminated name/source.
    Some(LuaBytes::from(
        unsafe { CStr::from_ptr(value) }.to_bytes().to_vec(),
    ))
}

fn copy_short_source(
    value: &[std::ffi::c_char; rsgdll_platform::__private::LUA_DEBUG_SHORT_SOURCE_CAPACITY],
) -> LuaBytes {
    let length = value
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(value.len());
    LuaBytes::from(
        value[..length]
            .iter()
            .map(|byte| *byte as u8)
            .collect::<Vec<_>>(),
    )
}
