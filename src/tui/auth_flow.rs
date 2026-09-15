//! Auth login subprocess tracking: register/cancel the in-flight browser
//! login process and the channel message reporting its completion.
#[cfg(not(test))]
use anyhow::bail;
use anyhow::Result;
#[cfg(not(test))]
use std::process::Command;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug)]
struct ActiveAuthProcess {
    generation: u64,
    pid: u32,
}

static ACTIVE_AUTH_PROCESS: OnceLock<Mutex<Option<ActiveAuthProcess>>> = OnceLock::new();

fn with_active_auth_process<R>(f: impl FnOnce(&mut Option<ActiveAuthProcess>) -> R) -> R {
    let state = ACTIVE_AUTH_PROCESS.get_or_init(|| Mutex::new(None));
    let mut guard = state.lock().expect("active auth process lock poisoned");
    f(&mut guard)
}

#[cfg(not(test))]
pub(in crate::tui) fn set_active_auth_process(generation: u64, pid: u32) {
    with_active_auth_process(|state| {
        *state = Some(ActiveAuthProcess { generation, pid });
    });
}

#[cfg(not(test))]
pub(in crate::tui) fn clear_active_auth_process_if_generation(generation: u64) {
    with_active_auth_process(|state| {
        if state
            .as_ref()
            .is_some_and(|active| active.generation == generation)
        {
            *state = None;
        }
    });
}

pub(in crate::tui) fn cancel_active_auth_process(generation: u64) -> Result<bool> {
    let pid = with_active_auth_process(|state| match state.as_ref() {
        Some(active) if active.generation == generation => {
            let pid = active.pid;
            *state = None;
            Some(pid)
        }
        _ => None,
    });
    let Some(pid) = pid else {
        return Ok(false);
    };
    terminate_process_by_pid(pid)?;
    Ok(true)
}

fn terminate_process_by_pid(pid: u32) -> Result<()> {
    #[cfg(not(test))]
    {
        let status = Command::new("kill").arg(pid.to_string()).status()?;
        if !status.success() {
            bail!("failed to terminate auth login process: pid={pid}");
        }
        Ok(())
    }

    #[cfg(test)]
    {
        let _ = pid;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(in crate::tui) enum AuthFlowMessage {
    Completed {
        generation: u64,
        success: bool,
        detail: String,
    },
}
