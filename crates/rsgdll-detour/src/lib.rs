//! Explicitly unsafe function-entry detouring primitives.
//!
//! This crate deliberately does not change page protection or build an
//! instruction-relocating trampoline. Callers must establish those invariants.

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
compile_error!("rsgdll-detour currently supports only Linux x86_64");

use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::{self, NonNull};
use std::rc::Rc;

/// Length of `jmp qword ptr [rip]` followed by one absolute x86_64 address.
pub const PATCH_LEN: usize = 14;

/// Installed x86_64 absolute function-entry redirect.
///
/// Dropping this value restores the original bytes.
#[must_use = "dropping the detour immediately restores the target"]
pub struct Detour {
    target: NonNull<u8>,
    original: [u8; PATCH_LEN],
    _main_thread_only: PhantomData<Rc<()>>,
}

impl Detour {
    /// Replaces `PATCH_LEN` bytes at `target` with an absolute jump.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that:
    ///
    /// - `target` is readable and writable for `PATCH_LEN` bytes;
    /// - no thread executes or mutates those bytes during this call;
    /// - the target allocation remains valid, executable, and writable until
    ///   restoration completes;
    /// - replacing exactly `PATCH_LEN` entry bytes is valid for the target.
    /// - `replacement` has the target's exact ABI, remains executable until
    ///   restoration completes, and never unwinds across that ABI;
    /// - before this value is dropped, every thread is again prevented from
    ///   executing or mutating the target until restoration completes;
    /// - unwinding cannot drop this value while target execution is allowed.
    ///
    /// Target execution may resume after this call and must stop again before
    /// drop. This primitive neither synchronizes threads nor changes page
    /// protection, and it does not provide a trampoline for calling the
    /// replaced function.
    pub unsafe fn install(target: NonNull<u8>, replacement: NonNull<c_void>) -> Self {
        let mut original = [0; PATCH_LEN];
        // SAFETY: the caller guarantees `target` is readable for PATCH_LEN
        // bytes and does not overlap this stack-owned destination.
        unsafe { ptr::copy_nonoverlapping(target.as_ptr(), original.as_mut_ptr(), PATCH_LEN) };

        let mut patch = [0_u8; PATCH_LEN];
        patch[..6].copy_from_slice(&[0xFF, 0x25, 0, 0, 0, 0]);
        patch[6..].copy_from_slice(&(replacement.as_ptr() as usize).to_le_bytes());
        // SAFETY: the caller guarantees exclusive writable access to the
        // target bytes for the detour lifetime.
        unsafe { ptr::copy_nonoverlapping(patch.as_ptr(), target.as_ptr(), PATCH_LEN) };

        Self {
            target,
            original,
            _main_thread_only: PhantomData,
        }
    }
}

impl Drop for Detour {
    fn drop(&mut self) {
        // SAFETY: `install` requires the caller to restore exclusive access
        // before drop and retain it until these original bytes are restored.
        unsafe {
            ptr::copy_nonoverlapping(self.original.as_ptr(), self.target.as_ptr(), PATCH_LEN);
        }
    }
}
