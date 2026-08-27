mod abi_header;

use std::error::Error;
use std::fs;
use std::path::Path;

const CPP_PURE_LINE_BUDGET: usize = 600;

fn main() -> Result<(), Box<dyn Error>> {
    let rust_source = fs::read_to_string("src/lib.rs")?;
    let header = abi_header::generate(&rust_source)?;
    let output = std::env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?;
    fs::write(Path::new(&output).join("firewall_abi.h"), header)?;

    enforce_cpp_budget("src/firewall.cpp")?;

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .flag_if_supported("-fno-exceptions")
        .flag_if_supported("/EHs-c-")
        .include(output)
        .file("src/firewall.cpp");
    if std::env::var_os("CARGO_FEATURE_TEST_SUPPORT").is_some() {
        build.define("RSGDLL_TEST_SUPPORT", None);
    }
    build.compile("rsgdll_bridge");

    println!("cargo::rerun-if-changed=abi_header.rs");
    println!("cargo::rerun-if-changed=src/firewall.cpp");
    println!("cargo::rerun-if-changed=src/lib.rs");
    println!("cargo::rerun-if-env-changed=CARGO_FEATURE_TEST_SUPPORT");
    Ok(())
}

fn enforce_cpp_budget(path: &str) -> Result<(), Box<dyn Error>> {
    let source = fs::read_to_string(path)?;
    let pure_lines = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .count();
    if pure_lines > CPP_PURE_LINE_BUDGET {
        return Err(format!(
            "{path} has {pure_lines} pure lines; firewall budget is {CPP_PURE_LINE_BUDGET}"
        )
        .into());
    }
    Ok(())
}
