//! Lifecycle management for the `llama-server` child processes.
//!
//! ## The bug this exists to fix
//!
//! Both sidecars were spawned as `let (mut rx, _child) = ...`, binding the
//! handle to `_child` and dropping it immediately. `CommandChild` has **no
//! `Drop` implementation** — verified by reading tauri-plugin-shell 2.3.6,
//! which exposes `kill()` but never calls it for you. Dropping the handle
//! therefore orphans the process.
//!
//! Two consequences, one visible and one much worse:
//!
//! * **Visible:** the next `cargo build` fails. Windows locks running
//!   executables, so `tauri-build` cannot overwrite `target/debug/
//!   llama-server.exe` and panics at `fs::remove_file(&dest).unwrap()` with
//!   `PermissionDenied`.
//! * **Worse:** every launch of Orion leaks a ~2.4 GB process that survives
//!   the app closing. Run it three times on an 8 GB laptop and the machine is
//!   out of memory, with no window open to explain why.
//!
//! The build failure is what got noticed. The memory leak is the real defect.
//!
//! ## Approach
//!
//! Handles go in a registry owned by the app. On `RunEvent::Exit` every child
//! is killed. This is deliberately a flat list rather than per-service state:
//! shutdown must not need to know which sidecars happen to exist, so adding a
//! third one later cannot silently reintroduce the leak.

use std::sync::Mutex;

use tauri_plugin_shell::process::CommandChild;

/// Tie child processes to this process at the OS level, on Windows.
///
/// `shutdown()` only runs on a graceful exit. When Orion aborts — and it has,
/// repeatedly, from refcount violations in the Tauri layer — no Rust code
/// runs at all and every sidecar is orphaned. A user counted eight stray
/// `llama-server.exe` processes from four crashed runs, roughly 10 GB of
/// leaked memory that survived until they killed them by hand.
///
/// A Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` fixes this properly:
/// Windows terminates every process in the job when the last handle closes,
/// which happens when this process dies for *any* reason, including a hard
/// abort or being killed from Task Manager. Cleanup that depends on our own
/// code running is cleanup that fails exactly when it is most needed.
#[cfg(windows)]
mod job {
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    struct Job(HANDLE);
    // The handle is owned for the life of the process and only ever read.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    static JOB: OnceLock<Option<Job>> = OnceLock::new();

    fn job_handle() -> Option<HANDLE> {
        JOB.get_or_init(|| unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                tracing::warn!("could not create job object; sidecars may leak on a crash");
                return None;
            }

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            let ok = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                tracing::warn!("could not configure job object; sidecars may leak on a crash");
                CloseHandle(handle);
                return None;
            }

            tracing::debug!("job object created; sidecars will die with this process");
            Some(Job(handle))
        })
        .as_ref()
        .map(|j| j.0)
    }

    /// Add a spawned child to the job.
    pub fn adopt(pid: u32) {
        let Some(job) = job_handle() else { return };
        unsafe {
            let proc = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if proc.is_null() {
                tracing::warn!(pid, "could not open sidecar to add it to the job object");
                return;
            }
            if AssignProcessToJobObject(job, proc) == 0 {
                tracing::warn!(pid, "could not assign sidecar to the job object");
            }
            CloseHandle(proc);
        }
    }
}

#[cfg(not(windows))]
mod job {
    /// No-op elsewhere.
    ///
    /// On Linux and macOS the graceful path is the only one implemented so
    /// far. The equivalents are prctl(PR_SET_PDEATHSIG) and a process group
    /// killed on exit; worth adding before release, but Windows is where the
    /// crashes and the leaked processes actually happened.
    pub fn adopt(_pid: u32) {}
}

/// Owns every spawned sidecar so they can all be killed on exit.
#[derive(Default)]
pub struct SidecarRegistry {
    /// A plain `std::sync::Mutex`, not the tokio one, because shutdown runs
    /// on the main thread outside any async context and must not need a
    /// runtime to do its job.
    children: Mutex<Vec<(String, CommandChild)>>,
}

impl SidecarRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take ownership of a spawned child.
    pub fn register(&self, name: impl Into<String>, child: CommandChild) {
        let name = name.into();
        // Hand it to the OS first. If this process dies without running any
        // more of our code, Windows still cleans up.
        job::adopt(child.pid());

        match self.children.lock() {
            Ok(mut guard) => {
                tracing::debug!(sidecar = %name, pid = child.pid(), "sidecar registered");
                guard.push((name, child));
            }
            Err(poisoned) => {
                // A poisoned lock means another thread panicked while holding
                // it. Still register, because failing to do so leaks a
                // process, which is worse than touching poisoned state.
                tracing::warn!("sidecar registry lock was poisoned; registering anyway");
                poisoned.into_inner().push((name, child));
            }
        }
    }

    /// How many children are currently tracked.
    pub fn len(&self) -> usize {
        self.children
            .lock()
            .map(|g| g.len())
            .unwrap_or_else(|p| p.into_inner().len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Terminate a specific named sidecar by name, e.g. "llama-server (chat)".
    ///
    /// Used by the inactivity sleep watchdog to offload the model process from RAM
    /// after 8 minutes of inactivity while leaving other services unharmed.
    pub fn terminate(&self, target_name: &str) -> bool {
        let mut to_kill = None;
        match self.children.lock() {
            Ok(mut guard) => {
                if let Some(pos) = guard.iter().position(|(name, _)| name == target_name) {
                    to_kill = Some(guard.remove(pos));
                }
            }
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                if let Some(pos) = guard.iter().position(|(name, _)| name == target_name) {
                    to_kill = Some(guard.remove(pos));
                }
            }
        }
        if let Some((name, child)) = to_kill {
            let pid = child.pid();
            match child.kill() {
                Ok(()) => {
                    tracing::info!(sidecar = %name, pid, "sidecar terminated");
                    true
                }
                Err(e) => {
                    tracing::warn!(sidecar = %name, pid, error = %e, "could not kill sidecar");
                    false
                }
            }
        } else {
            false
        }
    }

    /// Kill every registered child.
    ///
    /// Safe to call more than once: the list is drained, so a second call is
    /// a no-op. Errors are logged and never propagated — during shutdown
    /// there is nobody left to handle them, and one stubborn process must not
    /// prevent the rest from being cleaned up.
    pub fn shutdown(&self) {
        let children = match self.children.lock() {
            Ok(mut guard) => guard.drain(..).collect::<Vec<_>>(),
            Err(poisoned) => poisoned.into_inner().drain(..).collect::<Vec<_>>(),
        };

        if children.is_empty() {
            return;
        }

        tracing::info!(count = children.len(), "shutting down sidecars");
        for (name, child) in children {
            let pid = child.pid();
            match child.kill() {
                Ok(()) => tracing::info!(sidecar = %name, pid, "sidecar terminated"),
                Err(e) => {
                    // Most likely the process already exited on its own.
                    tracing::warn!(sidecar = %name, pid, error = %e, "could not kill sidecar")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_registry_is_empty() {
        let r = SidecarRegistry::new();
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn shutting_down_an_empty_registry_is_harmless() {
        // Called on every exit, including when no sidecar ever started
        // because the model was missing.
        let r = SidecarRegistry::new();
        r.shutdown();
        r.shutdown();
        assert!(r.is_empty());
    }

    // Spawning a real CommandChild needs a Tauri app handle, so the kill path
    // itself cannot be unit-tested here. What is testable is that the
    // registry never silently discards a handle, which is the property whose
    // absence caused the leak: `let (rx, _child) = ...` dropped it outright.
    #[test]
    fn the_registry_owns_handles_rather_than_dropping_them() {
        let src = include_str!("sidecars.rs");
        assert!(
            src.contains("children: Mutex<Vec<(String, CommandChild)>>"),
            "handles must be stored, not bound to _child and dropped"
        );
    }

    #[test]
    fn no_sidecar_is_spawned_without_being_registered() {
        // Structural guard. Every `.spawn()` of a sidecar must hand its child
        // to the registry; a future third sidecar that forgets would
        // reintroduce the orphaned-process leak silently.
        for (file, src) in [
            ("lib.rs", include_str!("lib.rs")),
            ("documents.rs", include_str!("documents.rs")),
        ] {
            let spawns = src.matches(".spawn()").count();
            if spawns == 0 {
                continue;
            }
            let registrations = src.matches("register(").count();
            assert!(
                registrations >= spawns,
                "{file} spawns {spawns} sidecar(s) but registers {registrations}; \
                 an unregistered child is never killed and leaks on exit"
            );
            assert!(
                !src.contains("let (mut rx, _child)"),
                "{file} still discards a child handle with `_child`"
            );
        }
    }
}
