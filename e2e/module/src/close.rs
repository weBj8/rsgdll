const MARKER: &str = "rsgdll-e2e-close-v1";

pub(crate) fn run() {
    let Some(path) = std::env::var_os("RSGDLL_E2E_CLOSE_MARKER") else {
        return;
    };
    if let Err(error) = std::fs::write(path, MARKER) {
        eprintln!("[rsgdll-e2e] failed to write close hook marker: {error}");
    }
}
