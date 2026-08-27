use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

pub(super) fn drop_caught_panic(payload: Box<dyn Any + Send>) {
    let mut payload = payload;
    for _ in 0..2 {
        match catch_unwind(AssertUnwindSafe(|| drop(payload))) {
            Ok(()) => return,
            Err(next) => payload = next,
        }
    }
    std::mem::forget(payload);
}

#[cfg(test)]
mod tests {
    use std::panic::panic_any;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    struct PanickingPayload(Arc<AtomicBool>);

    impl Drop for PanickingPayload {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
            panic!("panic payload destructor");
        }
    }

    #[test]
    fn caught_panic_payload_is_dropped_without_resuming_unwind() {
        // Given: a caught panic payload whose destructor also panics.
        let dropped = Arc::new(AtomicBool::new(false));
        let payload = super::catch_unwind(super::AssertUnwindSafe({
            let dropped = Arc::clone(&dropped);
            move || panic_any(PanickingPayload(dropped))
        }))
        .expect_err("panic payload");

        // When: the FFI boundary disposes the caught payload.
        super::drop_caught_panic(payload);

        // Then: the payload destructor ran and its panic stayed contained.
        assert!(dropped.load(Ordering::SeqCst));
    }
}
