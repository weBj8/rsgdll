use framework::prelude::{MainThread, ModuleBuilder};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct Config {
    enabled: bool,
}

#[framework::function]
fn echo(_main_thread: &mut MainThread, input: String) -> String {
    input
}

#[framework::function(serde)]
fn serde_round_trip(input: Config) -> Config {
    input
}

#[framework::module]
fn register(module: &mut ModuleBuilder) {
    module
        .function("echo", echo)
        .function("serde_round_trip", serde_round_trip);
}

#[test]
fn generated_descriptor_is_available() {
    let _descriptor = echo;
    let _serde_descriptor = serde_round_trip;
}
