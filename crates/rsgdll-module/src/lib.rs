//! Module lifecycle, callback dispatch, and registration.

#![deny(unsafe_op_in_unsafe_fn)]

mod dispatcher;
mod report;

pub use dispatcher::{
    BoxError, Callback, CallbackId, RegistrationError, install_dispatcher, register_callback,
    trampoline,
};
pub use report::{ErrorReport, PanicReport};
