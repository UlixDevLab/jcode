use super::*;
use std::process::{Command, Stdio};

#[cfg(unix)]
#[test]
fn reload_barrier_reaps_normal_direct_child() {
    let mut child = Command::new("sh")
        .args(["-c", "sleep 60"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn direct child");
    let pid = child.id();

    let outcome = reap_reload_children(Duration::from_secs(2));

    assert!(outcome.discovered >= 1, "{outcome:?}");
    assert!(outcome.reaped >= 1, "{outcome:?}");
    assert_eq!(outcome.remaining, 0, "{outcome:?}");
    assert!(!crate::platform::is_process_running(pid));
    let _ = child.try_wait();
}

#[cfg(unix)]
#[test]
fn reload_barrier_preserves_setsid_detached_child() {
    let mut command = Command::new("sh");
    command
        .args(["-c", "sleep 60"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = crate::platform::spawn_detached(&mut command).expect("spawn detached child");
    let pid = child.id();

    let outcome = reap_reload_children(Duration::from_secs(2));

    assert!(outcome.skipped_detached >= 1, "{outcome:?}");
    assert!(crate::platform::is_process_running(pid));
    child.kill().expect("kill detached fixture");
    child.wait().expect("reap detached fixture");
}
