//! Main-thread-bound checked Source engine interfaces.

#![deny(unsafe_op_in_unsafe_fn)]

mod loader;

use std::error::Error;
use std::fmt;
use std::marker::PhantomData;
use std::ptr::NonNull;

use loader::{EngineLibrary, InterfaceError, LibraryError};
use rsgdll_engine_sys::{RawEngineServer, RawEngineServerVTable, VENGINE_SERVER_VERSION};
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
        }
    }
}

impl Error for EngineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            EngineErrorKind::Library(error) => Some(error),
            EngineErrorKind::Interface(error) => Some(error),
            EngineErrorKind::InvalidVTable => None,
        }
    }
}
