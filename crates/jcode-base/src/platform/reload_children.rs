//! Reload-only cleanup for direct child processes before Unix `exec`.
//!
//! Normal runtime reaping remains handle-based. This module is deliberately
//! used only at the reload boundary, where `exec` would otherwise discard all
//! `Child` handles while preserving the process tree and its zombies.

use std::collections::HashSet;
use std::time::{Duration, Instant};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReloadChildReapOutcome {
    pub discovered: usize,
    pub skipped_detached: usize,
    pub signalled: usize,
    pub reaped: usize,
    pub remaining: usize,
}

/// Terminate and reap non-detached direct children before replacing the
/// current Unix process image. Children created by `spawn_detached` are session
/// leaders (`setsid`) and are preserved by construction.
pub fn reap_reload_children(deadline: Duration) -> ReloadChildReapOutcome {
    #[cfg(unix)]
    {
        reap_reload_children_unix(deadline)
    }
    #[cfg(not(unix))]
    {
        let _ = deadline;
        ReloadChildReapOutcome::default()
    }
}

#[cfg(unix)]
fn reap_reload_children_unix(deadline: Duration) -> ReloadChildReapOutcome {
    let mut outcome = ReloadChildReapOutcome::default();
    let mut pending = HashSet::new();

    for pid in direct_children() {
        outcome.discovered += 1;
        let session = unsafe { libc::getsid(pid) };
        if session == pid {
            outcome.skipped_detached += 1;
            continue;
        }
        pending.insert(pid);
    }

    for &pid in &pending {
        if unsafe { libc::kill(pid, libc::SIGTERM) } == 0 {
            outcome.signalled += 1;
        }
    }

    let started = Instant::now();
    let gentle_deadline = deadline.min(Duration::from_millis(150));
    while !pending.is_empty() && started.elapsed() < gentle_deadline {
        reap_ready(&mut pending, &mut outcome);
        if !pending.is_empty() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    for &pid in &pending {
        let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    while !pending.is_empty() && started.elapsed() < deadline {
        reap_ready(&mut pending, &mut outcome);
        if !pending.is_empty() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    outcome.remaining = pending.len();
    outcome
}

#[cfg(unix)]
fn reap_ready(pending: &mut HashSet<libc::pid_t>, outcome: &mut ReloadChildReapOutcome) {
    pending.retain(|&pid| {
        let mut status = 0;
        let rc = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if rc == pid {
            outcome.reaped += 1;
            return false;
        }
        if rc == -1 {
            let error = std::io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(code) if code == libc::ECHILD) {
                return false;
            }
        }
        true
    });
}

#[cfg(target_os = "linux")]
fn direct_children() -> Vec<libc::pid_t> {
    let pid = std::process::id();
    let mut children = HashSet::new();
    if let Ok(threads) = std::fs::read_dir(format!("/proc/{pid}/task")) {
        for thread in threads.flatten() {
            let tid = thread.file_name();
            let Some(tid) = tid.to_str() else { continue };
            let Ok(list) = std::fs::read_to_string(format!("/proc/{pid}/task/{tid}/children"))
            else {
                continue;
            };
            children.extend(
                list.split_whitespace()
                    .filter_map(|value| value.parse::<libc::pid_t>().ok()),
            );
        }
    }
    children.into_iter().collect()
}

#[cfg(target_os = "macos")]
fn direct_children() -> Vec<libc::pid_t> {
    unsafe extern "C" {
        fn proc_listpids(kind: u32, kind_info: u32, buffer: *mut libc::c_void, size: i32) -> i32;
    }

    // libproc.h: list processes whose parent PID equals `kind_info`.
    const PROC_PPID_ONLY: u32 = 6;
    const MAX_CHILDREN: usize = 4096;
    let mut pids = vec![0 as libc::pid_t; MAX_CHILDREN];
    let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as i32;
    let written = unsafe {
        proc_listpids(
            PROC_PPID_ONLY,
            std::process::id(),
            pids.as_mut_ptr().cast(),
            bytes,
        )
    };
    if written <= 0 {
        return Vec::new();
    }
    pids.truncate(written as usize / std::mem::size_of::<libc::pid_t>());
    pids.retain(|pid| *pid > 0);
    pids
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn direct_children() -> Vec<libc::pid_t> {
    Vec::new()
}

#[cfg(test)]
#[path = "reload_children_tests.rs"]
mod tests;
