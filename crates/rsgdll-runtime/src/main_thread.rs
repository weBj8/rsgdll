use std::marker::PhantomData;
use std::rc::Rc;

/// Explicit capability proving code runs inside a GMod main-thread callback.
#[derive(Debug)]
pub struct MainThread {
    _main_thread: PhantomData<Rc<()>>,
}

impl MainThread {
    /// Creates the capability at the framework callback boundary.
    ///
    /// # Safety
    ///
    /// Caller must be generated framework glue currently executing a Lua
    /// callback on GMod's main thread.
    #[doc(hidden)]
    #[must_use]
    pub const unsafe fn __from_callback() -> Self {
        Self {
            _main_thread: PhantomData,
        }
    }
}
