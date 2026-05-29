use std::sync::{Mutex, MutexGuard, OnceLock};

#[doc(hidden)]
pub fn env_guard() -> MutexGuard<'static, ()> {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}
