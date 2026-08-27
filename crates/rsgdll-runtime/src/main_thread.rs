use std::marker::PhantomData;
use std::rc::Rc;

/// Explicit capability proving code runs inside a GMod main-thread callback.
#[derive(Debug)]
pub struct MainThread {
    _main_thread: PhantomData<Rc<()>>,
}

impl MainThread {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            _main_thread: PhantomData,
        }
    }
}
