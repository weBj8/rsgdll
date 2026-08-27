use std::ffi::{c_int, c_void};
use std::ptr::{self, NonNull};

use rsgdll_detour::{Detour, PATCH_LEN};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(Detour: Send, Sync);

type TestFunction = extern "C" fn() -> u32;

extern "C" fn replacement() -> u32 {
    42
}

const PAGE_LEN: usize = 4096;
const PROT_READ: c_int = 1;
const PROT_WRITE: c_int = 2;
const PROT_EXEC: c_int = 4;
const MAP_PRIVATE: c_int = 2;
const MAP_ANONYMOUS: c_int = 0x20;

unsafe extern "C" {
    fn mmap(
        address: *mut c_void,
        length: usize,
        protection: c_int,
        flags: c_int,
        file: c_int,
        offset: isize,
    ) -> *mut c_void;
    fn munmap(address: *mut c_void, length: usize) -> c_int;
}

struct ExecutablePage(NonNull<u8>);

impl ExecutablePage {
    fn new() -> Self {
        // SAFETY: anonymous private mapping requests one fresh page with no
        // backing file. The returned mapping is exclusively owned below.
        let address = unsafe {
            mmap(
                ptr::null_mut(),
                PAGE_LEN,
                PROT_READ | PROT_WRITE | PROT_EXEC,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        assert_ne!(address.addr(), usize::MAX, "mmap failed");
        Self(NonNull::new(address.cast()).expect("mmap returned null"))
    }
}

impl Drop for ExecutablePage {
    fn drop(&mut self) {
        // SAFETY: this is the sole release of the live mapping created in new.
        assert_eq!(unsafe { munmap(self.0.as_ptr().cast(), PAGE_LEN) }, 0);
    }
}

#[test]
#[cfg_attr(miri, ignore = "requires executable mmap and native x86_64 code")]
fn installed_absolute_jump_restores_original_bytes_on_drop() {
    let target = ExecutablePage::new();
    let original = [
        0xB8, 7, 0, 0, 0,    // mov eax, 7
        0xC3, // ret
        0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, // nop padding
    ];
    // SAFETY: [Category 10 — Out-of-bounds access] target owns an exclusive
    // writable page, and both source and destination cover PATCH_LEN bytes.
    unsafe { ptr::copy_nonoverlapping(original.as_ptr(), target.0.as_ptr(), PATCH_LEN) };
    // SAFETY: [Category 8 — FFI boundary UB] the executable mapping begins
    // with a complete System V `extern "C" fn() -> u32` body and remains live.
    let target_function: TestFunction = unsafe { std::mem::transmute(target.0.as_ptr()) };
    assert_eq!(target_function(), 7);
    let replacement = NonNull::new(replacement as *const () as *mut c_void)
        .expect("function pointer is non-null");
    {
        // SAFETY: target is an exclusively accessed executable mapping, no
        // thread executes it, and both functions have the exact TestFunction
        // ABI and return without unwinding.
        let _detour = unsafe { Detour::install(target.0, replacement) };
        assert_eq!(target_function(), 42);
        // SAFETY: mapping remains live and no mutable reference exists.
        let patched = unsafe { std::slice::from_raw_parts(target.0.as_ptr(), PATCH_LEN) };
        assert_eq!(&patched[..6], &[0xFF, 0x25, 0, 0, 0, 0]);
        assert_eq!(
            &patched[6..],
            &(replacement.as_ptr() as usize).to_le_bytes()
        );
    }
    // SAFETY: mapping remains live and restoration completed before this read.
    let restored = unsafe { std::slice::from_raw_parts(target.0.as_ptr(), PATCH_LEN) };
    assert_eq!(restored, original);
    assert_eq!(target_function(), 7);
}
