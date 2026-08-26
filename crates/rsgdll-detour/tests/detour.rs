use std::ffi::c_void;
use std::ptr::NonNull;

use rsgdll_detour::{Detour, PATCH_LEN};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(Detour: Send, Sync);

extern "C" fn replacement() {}

#[test]
fn installed_absolute_jump_restores_original_bytes_on_drop() {
    let mut target = [0xCC_u8; PATCH_LEN];
    let original = target;
    let replacement = NonNull::new(replacement as *const () as *mut c_void)
        .expect("function pointer is non-null");
    {
        // SAFETY: this owned test buffer is readable, writable, exclusively
        // accessed, and remains allocated for the detour's lifetime.
        let _detour = unsafe {
            Detour::install(
                NonNull::new(target.as_mut_ptr()).expect("array pointer is non-null"),
                replacement,
            )
        };
        assert_eq!(&target[..6], &[0xFF, 0x25, 0, 0, 0, 0]);
        assert_eq!(&target[6..], &(replacement.as_ptr() as usize).to_le_bytes());
    }
    assert_eq!(target, original);
}
