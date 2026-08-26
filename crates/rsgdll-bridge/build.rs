fn main() {
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file("src/firewall.cpp")
        .compile("rsgdll_bridge");
    println!("cargo::rerun-if-changed=src/firewall.cpp");
}
