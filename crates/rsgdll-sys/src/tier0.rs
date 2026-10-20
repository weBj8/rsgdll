//! tier0 logging and time (spec §5.2), resolved lazily; silently falls back to stderr.

use core::ffi::c_char;
use std::ffi::CString;
use std::sync::OnceLock;

type MsgFn = unsafe extern "C" fn(*const c_char, ...);
type TimeFn = unsafe extern "C" fn() -> f64;

struct Tier0 {
    _lib: libloading::Library,
    msg: MsgFn,
    warning: MsgFn,
    time: Option<TimeFn>,
}

fn tier0() -> Option<&'static Tier0> {
    static T: OnceLock<Option<Tier0>> = OnceLock::new();
    T.get_or_init(|| {
        let lib = crate::loader::find_module("tier0").or_else(|| crate::loader::find_module("libtier0"))?;
        // SAFETY: tier0 exports `Msg`/`Warning` as `void(const char*, ...)` and `Plat_FloatTime` as `double()`.
        unsafe {
            let msg = *lib.get::<MsgFn>(b"Msg\0").ok()?;
            let warning = *lib.get::<MsgFn>(b"Warning\0").ok()?;
            let time = lib.get::<TimeFn>(b"Plat_FloatTime\0").ok().map(|s| *s);
            Some(Tier0 { _lib: lib, msg, warning, time })
        }
    })
    .as_ref()
}

fn emit(pick: fn(&Tier0) -> MsgFn, text: &str) {
    let line = CString::new(text.replace('\0', "\\0")).unwrap_or_default();
    match tier0() {
        // SAFETY: "%s" + one C string, never user-controlled format strings.
        Some(t) => unsafe { pick(t)(c"%s".as_ptr(), line.as_ptr()) },
        None => eprint!("{text}"),
    }
}

/// Engine console message (no newline added).
pub fn msg(text: &str) {
    emit(|t| t.msg, text)
}

/// Engine console warning (no newline added).
pub fn warning(text: &str) {
    emit(|t| t.warning, text)
}

/// `Plat_FloatTime`, or process-relative time without tier0.
pub fn float_time() -> f64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    tier0()
        .and_then(|t| t.time)
        // SAFETY: no arguments, returns double.
        .map(|f| unsafe { f() })
        .unwrap_or_else(|| START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64())
}
