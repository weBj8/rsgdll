//! Main-thread-bound checked Source engine interfaces.

#![deny(unsafe_op_in_unsafe_fn)]

mod loader;

use std::cell::Cell;
use std::error::Error;
use std::ffi::{CStr, CString};
use std::fmt;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};

use loader::{EngineLibrary, InterfaceError, LibraryError, process_symbol};
pub use rsgdll_engine_sys::LoggingSeverity;
use rsgdll_engine_sys::{
    LOGGING_DO_NOT_ECHO, REGISTER_LOGGING_LISTENER_SYMBOL, RawEngineServer, RawEngineServerVTable,
    RawLoggingContext, RawLoggingListener, RawLoggingListenerVTable, RegisterLoggingListenerFn,
    TIER0_LIBRARIES, VENGINE_SERVER_VERSION,
};
use rsgdll_runtime::MainThread;

/// Main-thread-bound access to the loaded Source engine.
pub struct Engine<'main> {
    library: EngineLibrary,
    _main_thread: PhantomData<&'main mut MainThread>,
}
impl<'main> Engine<'main> {
    /// Attaches to the engine already loaded by the current GMod server.
    pub fn attach(_main_thread: &'main mut MainThread) -> Result<Self, EngineError> {
        // SAFETY: `MainThread` proves this runs inside a compatible GMod
        // callback; the loader uses RTLD_NOLOAD on the mapped engine path.
        let library = unsafe { EngineLibrary::open_engine() }.map_err(EngineError::library)?;
        Ok(Self {
            library,
            _main_thread: PhantomData,
        })
    }

    /// Acquires the pinned `IVEngineServer021` interface.
    pub fn server(&self) -> Result<EngineServer<'_>, EngineError> {
        let raw = self
            .library
            .query(VENGINE_SERVER_VERSION)
            .map_err(EngineError::interface)?
            .cast::<RawEngineServer>();
        // SAFETY: the factory returned a non-null object pointer. Reading its
        // first pointer is valid for the pinned `IVEngineServer021` layout.
        let vtable = unsafe { raw.as_ref().vtable };
        if vtable.is_null() {
            return Err(EngineError {
                kind: EngineErrorKind::InvalidVTable,
            });
        }
        Ok(EngineServer {
            raw,
            _engine: PhantomData,
        })
    }
}

/// Borrowed `IVEngineServer021` interface.
pub struct EngineServer<'engine> {
    raw: NonNull<RawEngineServer>,
    _engine: PhantomData<&'engine EngineLibrary>,
}

impl EngineServer<'_> {
    /// Returns whether the current engine process is a dedicated server.
    #[must_use]
    pub fn is_dedicated_server(&self) -> bool {
        // SAFETY: `Engine::server` validates the object and vtable pointers,
        // pins their library lifetime, and requires the GMod main thread.
        let vtable: &RawEngineServerVTable = unsafe { &*self.raw.as_ref().vtable };
        // SAFETY: slot 2 is pinned as `IsDedicatedServer` with explicit `this`.
        unsafe { (vtable.is_dedicated_server)(self.raw.as_ptr()) }
    }

    /// Queues one engine console command for normal frame execution.
    ///
    /// # Errors
    ///
    /// Returns an error when the command contains an interior NUL byte.
    pub fn queue_command(&self, command: &str) -> Result<(), EngineError> {
        let mut command = command.to_owned();
        if !command.ends_with('\n') {
            command.push('\n');
        }
        let command = CString::new(command).map_err(|_| EngineError {
            kind: EngineErrorKind::InvalidCommand,
        })?;
        // SAFETY: the checked interface pins slot 36 as `ServerCommand`; the
        // C string remains live for the synchronous call.
        unsafe {
            let vtable = &*self.raw.as_ref().vtable;
            (vtable.server_command)(self.raw.as_ptr(), command.as_ptr());
        }
        Ok(())
    }

    #[cfg(test)]
    fn from_raw_for_test(raw: &mut RawEngineServer) -> Self {
        Self {
            raw: NonNull::from(raw),
            _engine: PhantomData,
        }
    }
}

/// Owned Source log message copied before leaving the listener callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineLogMessage {
    pub severity: LoggingSeverity,
    pub message: String,
}

type LogCallback = Arc<dyn Fn(EngineLogMessage) + Send + Sync>;

#[repr(C)]
struct Listener {
    raw: RawLoggingListener,
    callback: Mutex<Option<LogCallback>>,
}

static LISTENER_VTABLE: RawLoggingListenerVTable = RawLoggingListenerVTable { log: listener_log };
static LISTENER: OnceLock<&'static Listener> = OnceLock::new();

// SAFETY: the raw vtable pointer is immutable process-lifetime data and
// callback access is synchronized by the mutex.
unsafe impl Send for Listener {}
// SAFETY: Source may invoke logging from any thread; all mutable state is
// protected by the mutex.
unsafe impl Sync for Listener {}

thread_local! {
    static IN_LOG_CALLBACK: Cell<bool> = const { Cell::new(false) };
}

/// Registered official Source logging listener.
pub struct LoggingListenerGuard {
    listener: &'static Listener,
    _main_thread: PhantomData<Rc<()>>,
}

impl LoggingListenerGuard {
    /// Registers an owned listener with Source's global logging system.
    ///
    /// # Errors
    ///
    /// Returns an error if the supported tier0 symbols are unavailable.
    pub fn register(
        _main_thread: &mut MainThread,
        callback: impl Fn(EngineLogMessage) + Send + Sync + 'static,
    ) -> Result<Self, EngineError> {
        // SAFETY: names and C function signatures are pinned to the selected
        // GMod tier0 exports documented in `engine-abi-reference.md`.
        let register = unsafe {
            std::mem::transmute::<*mut std::ffi::c_void, RegisterLoggingListenerFn>(
                process_symbol(TIER0_LIBRARIES, REGISTER_LOGGING_LISTENER_SYMBOL)
                    .map_err(EngineError::library)?
                    .as_ptr(),
            )
        };
        let listener = if let Some(listener) = LISTENER.get() {
            *listener
        } else {
            let listener = Box::leak(Box::new(Listener {
                raw: RawLoggingListener {
                    vtable: &LISTENER_VTABLE,
                },
                callback: Mutex::new(None),
            }));
            // SAFETY: the leaked object is pinned for the process lifetime and
            // its first field implements the pinned listener layout.
            unsafe { register(&mut listener.raw) };
            let _ = LISTENER.set(&*listener);
            &*listener
        };
        let mut active = listener
            .callback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.is_some() {
            return Err(EngineError {
                kind: EngineErrorKind::LoggingListenerActive,
            });
        }
        *active = Some(Arc::new(callback));
        drop(active);
        Ok(Self {
            listener,
            _main_thread: PhantomData,
        })
    }
}

impl Drop for LoggingListenerGuard {
    fn drop(&mut self) {
        *self
            .listener
            .callback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}

#[cfg(all(target_os = "windows", target_arch = "x86"))]
unsafe extern "thiscall" fn listener_log(
    listener: *mut RawLoggingListener,
    context: *const RawLoggingContext,
    message: *const std::ffi::c_char,
) {
    listener_log_impl(listener, context, message);
}

#[cfg(not(all(target_os = "windows", target_arch = "x86")))]
unsafe extern "C" fn listener_log(
    listener: *mut RawLoggingListener,
    context: *const RawLoggingContext,
    message: *const std::ffi::c_char,
) {
    listener_log_impl(listener, context, message);
}

fn listener_log_impl(
    listener: *mut RawLoggingListener,
    context: *const RawLoggingContext,
    message: *const std::ffi::c_char,
) {
    let Some(listener) = NonNull::new(listener.cast::<Listener>()) else {
        return;
    };
    let Some(context) = NonNull::new(context.cast_mut()) else {
        return;
    };
    let Some(message) = NonNull::new(message.cast_mut()) else {
        return;
    };
    // SAFETY: Source invokes this method with the registered object and live
    // callback arguments for the duration of the call.
    let context = unsafe { context.as_ref() };
    if context.flags & LOGGING_DO_NOT_ECHO != 0 {
        return;
    }
    let severity = match context.severity {
        1 => LoggingSeverity::Warning,
        2 => LoggingSeverity::Assert,
        3 => LoggingSeverity::Error,
        _ => LoggingSeverity::Message,
    };
    // SAFETY: Source logging messages are NUL-terminated for this callback.
    let message = unsafe { CStr::from_ptr(message.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: listener points to the process-lifetime object registered above.
    let callback = unsafe { listener.as_ref() }
        .callback
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(callback) = callback {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            dispatch_log(&callback, EngineLogMessage { severity, message });
        }));
    }
}

fn dispatch_log(callback: &LogCallback, message: EngineLogMessage) {
    IN_LOG_CALLBACK.with(|active| {
        if active.replace(true) {
            return;
        }
        struct Reset<'a>(&'a Cell<bool>);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _reset = Reset(active);
        callback(message);
    });
}

/// Checked engine attachment or interface failure.
#[derive(Debug)]
pub struct EngineError {
    kind: EngineErrorKind,
}

#[derive(Debug)]
enum EngineErrorKind {
    Library(LibraryError),
    Interface(InterfaceError),
    InvalidVTable,
    InvalidCommand,
    LoggingListenerActive,
}

impl EngineError {
    fn library(error: LibraryError) -> Self {
        Self {
            kind: EngineErrorKind::Library(error),
        }
    }

    fn interface(error: InterfaceError) -> Self {
        Self {
            kind: EngineErrorKind::Interface(error),
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            EngineErrorKind::Library(error) => error.fmt(formatter),
            EngineErrorKind::Interface(error) => error.fmt(formatter),
            EngineErrorKind::InvalidVTable => {
                formatter.write_str("Source engine interface has a null vtable")
            }
            EngineErrorKind::InvalidCommand => {
                formatter.write_str("engine command contains a NUL byte")
            }
            EngineErrorKind::LoggingListenerActive => {
                formatter.write_str("Source logging listener is already active")
            }
        }
    }
}

impl Error for EngineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            EngineErrorKind::Library(error) => Some(error),
            EngineErrorKind::Interface(error) => Some(error),
            EngineErrorKind::InvalidVTable => None,
            EngineErrorKind::InvalidCommand => None,
            EngineErrorKind::LoggingListenerActive => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::{CStr, c_char, c_int};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rsgdll_engine_sys::{RawEngineServer, RawEngineServerVTable, UnusedEngineMethod};

    use super::{EngineLogMessage, EngineServer, dispatch_log};

    static COMMAND_CALLS: AtomicUsize = AtomicUsize::new(0);

    macro_rules! engine_methods {
        ($(fn $name:ident($($argument:tt: $argument_type:ty),* $(,)?) $(-> $return_type:ty)? $body:block)+) => {
            $(
                #[cfg(all(target_os = "windows", target_arch = "x86"))]
                unsafe extern "thiscall" fn $name(
                    $($argument: $argument_type),*
                ) $(-> $return_type)? $body

                #[cfg(not(all(target_os = "windows", target_arch = "x86")))]
                unsafe extern "C" fn $name(
                    $($argument: $argument_type),*
                ) $(-> $return_type)? $body
            )+
        };
    }

    engine_methods! {
        fn change_level(
            _this: *mut RawEngineServer,
            _map: *const c_char,
            _landmark: *const c_char,
        ) {}
        fn is_map_valid(_this: *mut RawEngineServer, _name: *const c_char) -> c_int { 1 }
        fn is_dedicated_server(_this: *mut RawEngineServer) -> bool { true }
        fn server_command(_this: *mut RawEngineServer, command: *const c_char) {
            // SAFETY: the checked wrapper passes a live NUL-terminated command.
            assert_eq!(unsafe { CStr::from_ptr(command) }, c"status\n");
            COMMAND_CALLS.fetch_add(1, Ordering::Relaxed);
        }
    }

    unsafe extern "C" fn unused_engine_method() {}

    #[test]
    fn server_command_uses_pinned_engine_slot() {
        let vtable = RawEngineServerVTable {
            change_level,
            is_map_valid,
            is_dedicated_server,
            before_server_command: [unused_engine_method as UnusedEngineMethod; 33],
            server_command,
        };
        let mut raw = RawEngineServer { vtable: &vtable };
        let server = EngineServer::from_raw_for_test(&mut raw);

        server.queue_command("status").unwrap();

        assert_eq!(COMMAND_CALLS.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn recursive_logging_is_suppressed() {
        let calls = Arc::new(AtomicUsize::new(0));
        let recursive_calls = Arc::clone(&calls);
        let callback: super::LogCallback = Arc::new(move |message: EngineLogMessage| {
            recursive_calls.fetch_add(1, Ordering::Relaxed);
            let recursive: super::LogCallback = Arc::new(|_: EngineLogMessage| {
                panic!("recursive callback must be suppressed");
            });
            dispatch_log(&recursive, message);
        });

        dispatch_log(
            &callback,
            EngineLogMessage {
                severity: rsgdll_engine_sys::LoggingSeverity::Message,
                message: "hello".to_owned(),
            },
        );

        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}
