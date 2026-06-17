use std::sync::{Mutex, MutexGuard, OnceLock};

#[doc(hidden)]
pub fn env_guard() -> MutexGuard<'static, ()> {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    // Tests have no real LAN devices; keep device control on the deterministic
    // cloud path and avoid spinning up the LAN/mDNS background threads.
    std::env::set_var("MIT_DISABLE_LAN_DISCOVERY", "1");
    guard
}
