use std::error::Error;
use std::fmt;

use rsgdll::prelude::*;

#[rsgdll::module]
fn module(module: &mut ModuleBuilder) {
    module
        .function("hello", hello)
        .function("get_user", get_user)
        .function("status", status)
        .function("initialize", initialize)
        .function("empty", empty);
    #[cfg(test)]
    module.function("", empty);
}

#[rsgdll::function]
fn hello(name: String) -> String {
    format!("Hello {name}")
}

#[rsgdll::function]
fn get_user(id: u64) -> Result<String, UserError> {
    if id == 0 {
        Err(UserError)
    } else {
        Ok(format!("user-{id}"))
    }
}

#[rsgdll::function]
fn status() -> (String, bool) {
    ("ready".to_owned(), true)
}

#[rsgdll::function]
fn initialize() {}

#[rsgdll::function]
fn empty() -> String {
    String::new()
}

#[derive(Debug)]
struct UserError;

impl fmt::Display for UserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("user id must not be zero")
    }
}

impl Error for UserError {}

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
