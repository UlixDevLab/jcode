use super::*;

#[cfg(unix)]
enum ZombieProbe {
    AlreadyReaped,
    StillRunning,
    TestReapedZombie,
    OtherError(std::io::Error),
}

#[cfg(unix)]
fn probe_zombie(pid: u32) -> ZombieProbe {
    let mut status = 0;
    let rc = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
    if rc == pid as i32 {
        ZombieProbe::TestReapedZombie
    } else if rc == 0 {
        ZombieProbe::StillRunning
    } else {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ECHILD) {
            ZombieProbe::AlreadyReaped
        } else {
            ZombieProbe::OtherError(error)
        }
    }
}

#[cfg(unix)]
fn assert_reaped_without_test_wait(pid: u32, settle: std::time::Duration) {
    std::thread::sleep(settle);
    match probe_zombie(pid) {
        ZombieProbe::AlreadyReaped => {}
        ZombieProbe::StillRunning => panic!("child pid={pid} did not exit before inspection"),
        ZombieProbe::TestReapedZombie => {
            panic!("child pid={pid} became a zombie; the test reaped it before jcode did")
        }
        ZombieProbe::OtherError(error) => {
            panic!("unexpected waitpid error for pid={pid}: {error}")
        }
    }
}

#[test]
fn desired_nofile_soft_limit_only_raises_when_possible() {
    assert_eq!(desired_nofile_soft_limit(1024, 524_288, 8192), Some(8192));
    assert_eq!(desired_nofile_soft_limit(8192, 524_288, 8192), None);
    assert_eq!(desired_nofile_soft_limit(1024, 4096, 8192), Some(4096));
}

#[cfg(unix)]
#[test]
fn spawn_detached_creates_new_session() {
    // Capture the parent test runner's session id before spawning so we can
    // later assert the detached child is in a different session. getsid(0)
    // returns the SID of the calling process and is POSIX-standard, so it
    // works on Linux and macOS (where `ps -o sid=` is unsupported).
    let parent_sid = unsafe { libc::getsid(0) };
    assert!(
        parent_sid >= 0,
        "getsid(0) failed in parent: {}",
        std::io::Error::last_os_error()
    );

    // The child sleeps briefly so the parent can reliably observe its session
    // id while it is still alive. `setsid` runs inside `spawn_detached`'s
    // `pre_exec`, so by the time `spawn` returns the child has already been
    // made a session leader; the brief poll below absorbs the (very narrow)
    // scheduling window between fork, pre_exec, and the parent's first
    // observation, and also defends against the child reaping itself before
    // we manage to query its SID.
    let mut cmd = std::process::Command::new("perl");
    cmd.args(["-e", "select undef, undef, undef, 0.5"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let mut child = super::spawn_detached(&mut cmd).expect("spawn detached child");
    let child_pid = child.id() as i32;

    let mut child_sid = -1i32;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        child_sid = unsafe { libc::getsid(child_pid) };
        if child_sid >= 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let status = child.wait().expect("wait for child");
    assert!(status.success(), "child should exit successfully");

    assert!(
        child_sid >= 0,
        "getsid failed for detached child pid={child_pid}: {}",
        std::io::Error::last_os_error()
    );

    assert_eq!(
        child_sid as u32, child_pid as u32,
        "detached child should lead its own session (SID == PID)"
    );
    assert_ne!(
        child_sid, parent_sid,
        "detached child should not share parent session"
    );
}

#[cfg(windows)]
#[test]
fn is_process_running_reports_exited_children_as_stopped() {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/C", "ping -n 3 127.0.0.1 >NUL"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("spawn child");
    let pid = child.id();
    assert!(
        super::is_process_running(pid),
        "child should initially be running"
    );

    let status = child.wait().expect("wait for child");
    assert!(status.success(), "child should exit successfully");
    std::thread::sleep(Duration::from_millis(100));

    assert!(
        !super::is_process_running(pid),
        "exited child should not be reported as running"
    );
}

#[cfg(windows)]
#[test]
fn signal_detached_process_group_terminates_descendant_tree() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let temp = tempfile::tempdir().expect("temp dir");
    let ready_path = temp.path().join("child-ready.txt");
    let survived_path = temp.path().join("child-survived.txt");
    let child_script_path = temp.path().join("child.cmd");
    let parent_script_path = temp.path().join("parent.cmd");
    let child_script = concat!(
        "@echo off\r\n",
        "echo ready>\"%~dp0child-ready.txt\"\r\n",
        "ping -n 6 127.0.0.1 >NUL\r\n",
        "echo survived>\"%~dp0child-survived.txt\"\r\n"
    );
    let parent_script = concat!(
        "@echo off\r\n",
        "start \"\" /B cmd.exe /D /C \"\"%~dp0child.cmd\"\"\r\n",
        "ping -n 30 127.0.0.1 >NUL\r\n"
    );
    std::fs::write(&child_script_path, child_script).expect("write child command script");
    std::fs::write(&parent_script_path, parent_script).expect("write parent command script");
    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/D", "/C"])
        .arg(&parent_script_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut parent = super::spawn_detached(&mut cmd).expect("spawn detached process tree");
    let parent_pid = parent.id();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready_path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(ready_path.exists(), "descendant should report ready");
    assert!(super::is_process_running(parent_pid));

    super::signal_detached_process_group(parent_pid, 0).expect("terminate process tree");
    let deadline = Instant::now() + Duration::from_secs(10);
    while super::is_process_running(parent_pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = parent.wait();

    assert!(!super::is_process_running(parent_pid), "parent should stop");
    std::thread::sleep(Duration::from_secs(6));
    assert!(
        !survived_path.exists(),
        "descendant should not survive termination of the detached process tree"
    );
}

#[cfg(windows)]
#[test]
fn spawn_replacement_process_returns_without_waiting_for_child_exit() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/C", "ping -n 4 127.0.0.1 >NUL"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let start = Instant::now();
    let mut child = super::spawn_replacement_process(&mut cmd)
        .expect("spawn replacement process should succeed");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(1),
        "replacement spawn should not block, took {:?}",
        elapsed
    );
    assert!(
        child.try_wait().expect("poll child status").is_none(),
        "replacement child should still be running immediately after spawn"
    );

    child.kill().ok();
    let _ = child.wait();
}

#[cfg(unix)]
#[test]
fn detached_drop_hands_zombie_to_shared_reaper() {
    let mut cmd = std::process::Command::new("perl");
    cmd.args(["-e", "select undef, undef, undef, 0.05"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let child = super::spawn_detached(&mut cmd).expect("spawn detached child");
    let pid = child.id();
    drop(child);

    assert_reaped_without_test_wait(pid, std::time::Duration::from_millis(250));
}

#[cfg(unix)]
#[test]
fn detached_reaper_does_not_block_short_children_behind_long_child() {
    let mut long_cmd = std::process::Command::new("perl");
    long_cmd
        .args(["-e", "select undef, undef, undef, 2"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let long_child = super::spawn_detached(&mut long_cmd).expect("spawn long child");
    let long_pid = long_child.id();
    drop(long_child);

    let mut short_pids = Vec::new();
    for _ in 0..8 {
        let mut cmd = std::process::Command::new("perl");
        cmd.args(["-e", "select undef, undef, undef, 0.05"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let child = super::spawn_detached(&mut cmd).expect("spawn short child");
        short_pids.push(child.id());
        drop(child);
    }

    std::thread::sleep(std::time::Duration::from_millis(300));
    for pid in short_pids {
        assert_reaped_without_test_wait(pid, std::time::Duration::ZERO);
    }
    assert!(
        super::is_process_running(long_pid),
        "long child must still be alive while short children are reaped"
    );
    super::signal_detached_process_group(long_pid, libc::SIGTERM).ok();
    assert_reaped_without_test_wait(long_pid, std::time::Duration::from_millis(250));
}

#[cfg(unix)]
#[test]
fn spawn_reaped_stays_in_parent_session_and_reaps_on_drop() {
    let parent_session = unsafe { libc::getsid(0) };
    assert!(parent_session > 0, "parent session id must be available");

    let mut command = std::process::Command::new("/bin/sh");
    command.arg("-c").arg("sleep 0.05");
    let child = super::spawn_reaped(&mut command).expect("spawn reaped child");
    let pid = child.id();
    assert_eq!(
        unsafe { libc::getsid(pid as libc::pid_t) },
        parent_session,
        "observer child must remain visible to reload-boundary cleanup"
    );
    drop(child);

    assert_reaped_without_test_wait(pid, std::time::Duration::from_millis(250));
}
