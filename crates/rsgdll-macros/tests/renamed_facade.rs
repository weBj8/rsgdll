use framework::prelude::{MainThread, ModuleBuilder};

#[framework::function]
fn echo(_main_thread: &mut MainThread, input: String) -> String {
    input
}

#[framework::module]
fn register(module: &mut ModuleBuilder) {
    module.function("echo", echo);
}

#[test]
fn generated_descriptor_is_available() {
    let _descriptor = echo;
}
