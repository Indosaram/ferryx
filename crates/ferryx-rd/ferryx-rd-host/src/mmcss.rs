//! Thread-affine, single-attempt ownership for multimedia scheduling registration.

pub(crate) struct Registration<H> {
    handle: Option<H>,
    revert: fn(H),
    _thread_affinity: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl<H> Registration<H> {
    pub(crate) fn acquire(register: impl FnOnce() -> Option<H>, revert: fn(H)) -> Option<Self> {
        register().map(|handle| Self {
            handle: Some(handle),
            revert,
            _thread_affinity: std::marker::PhantomData,
        })
    }
}

impl<H> Drop for Registration<H> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            (self.revert)(handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Registration;
    use std::cell::Cell;
    use std::rc::Rc;

    fn revert(count: Rc<Cell<u32>>) {
        count.set(count.get() + 1);
    }

    #[test]
    fn success_reverts_exactly_once_on_scope_exit() {
        let calls = Cell::new(0);
        let releases = Rc::new(Cell::new(0));
        {
            let _registration = Registration::acquire(
                || {
                    calls.set(calls.get() + 1);
                    Some(Rc::clone(&releases))
                },
                revert,
            )
            .unwrap();
            assert_eq!(calls.get(), 1);
            assert_eq!(releases.get(), 0);
        }
        assert_eq!(releases.get(), 1);
    }

    #[test]
    fn failure_is_single_attempt_without_revert() {
        let calls = Cell::new(0);
        let registration = Registration::<Rc<Cell<u32>>>::acquire(
            || {
                calls.set(calls.get() + 1);
                None
            },
            |_| panic!("failed registration must not revert"),
        );
        assert!(registration.is_none());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn unwinding_reverts_registration() {
        let releases = Rc::new(Cell::new(0));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _registration = Registration::acquire(|| Some(Rc::clone(&releases)), revert);
            panic!("worker failed");
        }));
        assert!(result.is_err());
        assert_eq!(releases.get(), 1);
    }
}
