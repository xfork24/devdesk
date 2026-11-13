//! Scoped application sources make scheduling and recovery reproducible in tests.
use crate::ports::ApplicationEnvironment;
use std::{cell::RefCell, sync::Arc};
thread_local! { static SOURCE: RefCell<Option<Arc<dyn ApplicationEnvironment>>> = const { RefCell::new(None) }; }
pub fn with_application_environment<T>(
    source: Arc<dyn ApplicationEnvironment>,
    work: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<Arc<dyn ApplicationEnvironment>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SOURCE.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let previous = SOURCE.with(|slot| slot.replace(Some(source)));
    let _restore = Restore(previous);
    work()
}
pub fn id() -> String {
    SOURCE
        .with(|slot| slot.borrow().as_ref().map(|s| s.allocate_id()))
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}
pub(crate) fn now() -> u64 {
    SOURCE
        .with(|slot| slot.borrow().as_ref().map(|s| s.unix_millis()))
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Fixed;
    impl ApplicationEnvironment for Fixed {
        fn allocate_id(&self) -> String {
            "fixed-action".into()
        }
        fn unix_millis(&self) -> u64 {
            1234
        }
    }
    #[test]
    fn deterministic_sources_are_scoped_and_restored_after_unwind() {
        let failure = std::panic::catch_unwind(|| {
            with_application_environment(Arc::new(Fixed), || {
                assert_eq!(id(), "fixed-action");
                assert_eq!(now(), 1234);
                panic!("interrupt tick")
            })
        });
        assert!(failure.is_err());
        assert!(uuid::Uuid::parse_str(&id()).is_ok());
        assert!(now() > 1234);
    }
}
