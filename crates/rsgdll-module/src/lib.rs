//! Module lifecycle, callback dispatch, and registration.

#![deny(unsafe_op_in_unsafe_fn)]

mod builder;
mod dispatcher;
mod report;
mod returns;

pub use builder::{
    Function, ModuleBuilder, RawRegistration, initialize_module, install_userdata_gc,
};
pub use dispatcher::{
    BoxError, Callback, CallbackId, RegistrationError, install_dispatcher, register_callback,
    trampoline,
};
pub use report::{ErrorReport, PanicReport};
pub use returns::{IntoLuaReturn, LuaStackValues, ReturnError, ReturnWriter};
