//! Feature-gated W13 offline measurement supervisor and parent/child fixture.
//!
//! Run only with `--features w13-test-driver`. The outer process owns an OS
//! process-group deadline that does not depend on the Tokio runtime exercised by
//! the parent. The parent owns a private home/runtime and the server child, uses
//! no default provider bootstrap, and calls production reload code rather than
//! copying it.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime};

const SUPERVISOR_DEADLINE: Duration = Duration::from_secs(45);
const WATCHDOG_PARENT_DEADLINE: Duration = Duration::from_secs(115);
const WATCHDOG_SUPERVISOR_DEADLINE: Duration = Duration::from_secs(130);
const SUPERVISOR_POLL: Duration = Duration::from_millis(25);
const SUPERVISOR_TERM_GRACE: Duration = Duration::from_secs(5);
const SUPERVISOR_KILL_GRACE: Duration = Duration::from_secs(5);

fn arg_value(args: &[String], name: &str) -> Result<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("missing {name}"))
}

#[derive(Debug)]
struct ExpectedReexecIdentity {
    source_binary: PathBuf,
    sha256: String,
    selection: &'static str,
}

fn expected_reexec_identity(mode: &str) -> Result<ExpectedReexecIdentity> {
    let (target, selection) = match mode {
        "fallback" | "watchdog" => (
            std::env::current_exe().context("resolve W13 current executable for fallback")?,
            "current_executable_only",
        ),
        // This feature-only helper invokes the existing normal selector. It does
        // not publish or select a candidate; production chooses again when it
        // handles the real Reload request.
        "healthy" => (
            jcode::server::w13_test_support::normal_reload_exec_target()
                .context("resolve W13 normal reload target")?,
            "normal_reload_selector",
        ),
        _ => bail!("W13 identity mode must be `fallback`, `healthy`, or `watchdog`"),
    };
    let (source_binary, sha256) =
        jcode::server::w13_test_support::w13_binary_identity(&target, "expected replacement")?;
    Ok(ExpectedReexecIdentity {
        sha256,
        source_binary,
        selection,
    })
}

async fn wait_for_ready(path: &Path, nonce: &str) -> Result<SystemTime> {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            if std::fs::read_to_string(path).ok().as_deref() == Some(nonce) {
                return std::fs::metadata(path)?
                    .modified()
                    .context("ready marker mtime");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .context("timed out waiting for owned W13 child readiness")?
}

async fn child(args: &[String]) -> Result<()> {
    let root = PathBuf::from(arg_value(args, "--root")?).canonicalize()?;
    let socket = PathBuf::from(arg_value(args, "--socket")?);
    let debug_socket = PathBuf::from(arg_value(args, "--debug-socket")?);
    let ready = PathBuf::from(arg_value(args, "--ready")?);
    let mode = arg_value(args, "--mode")?;
    validate_child_paths(&root, &socket, &debug_socket, &ready)?;
    let lock_request = match mode.as_str() {
        "fallback" | "watchdog" => true,
        "healthy" => false,
        _ => bail!("W13 child mode must be `fallback`, `healthy`, or `watchdog`"),
    };
    jcode::logging::init();
    jcode::server::w13_test_support::run_offline_request_child(
        socket,
        debug_socket,
        ready,
        arg_value(args, "--nonce")?,
        lock_request,
    )
    .await
}

/// Accept only the exact argv used by the production same-build exec, while
/// retaining the fixture's existing private-root identity checks.
async fn serve(args: &[String]) -> Result<()> {
    if args.get(1).map(String::as_str) != Some("serve") {
        bail!("W13 re-exec must be invoked as `serve --socket <owned socket>`");
    }
    let socket = PathBuf::from(arg_value(args, "--socket")?);
    let runtime = PathBuf::from(std::env::var("JCODE_RUNTIME_DIR")?).canonicalize()?;
    let home = PathBuf::from(std::env::var("JCODE_HOME")?).canonicalize()?;
    let root = runtime
        .parent()
        .filter(|root| home.parent() == Some(*root))
        .context("W13 re-exec home/runtime do not share an owned root")?;
    let expected_socket = runtime.join("w13.sock");
    let debug_socket = runtime.join("w13-debug.sock");
    if socket != expected_socket
        || PathBuf::from(std::env::var("JCODE_SOCKET")?) != expected_socket
        || root.join("runtime") != runtime
        || root.join("home") != home
    {
        bail!("W13 re-exec socket or inherited owned root did not match");
    }
    for path in [&socket, &debug_socket] {
        if path.exists() && std::fs::symlink_metadata(path)?.file_type().is_symlink() {
            bail!(
                "W13 re-exec rejects symlinked owned path {}",
                path.display()
            );
        }
    }
    jcode::logging::init();
    jcode::server::w13_test_support::write_w13_reexec_identity_receipt(
        &PathBuf::from(std::env::var("JCODE_W13_REEXEC_RECEIPT")?),
        root,
        &socket,
    )?;
    jcode::server::w13_test_support::run_offline_reexec_server(socket, debug_socket).await
}

fn validate_child_paths(
    root: &Path,
    socket: &Path,
    debug_socket: &Path,
    ready: &Path,
) -> Result<()> {
    let home = root.join("home").canonicalize()?;
    let runtime = root.join("runtime").canonicalize()?;
    if home.parent() != Some(root) || runtime.parent() != Some(root) {
        bail!("W13 child root must contain canonical home and runtime siblings");
    }
    let expected_socket = runtime.join("w13.sock");
    let expected_debug = runtime.join("w13-debug.sock");
    let expected_ready = runtime.join("w13.ready");
    if socket != expected_socket || debug_socket != expected_debug || ready != expected_ready {
        bail!("W13 child paths are not the exact owned runtime siblings");
    }
    for (name, actual, expected) in [
        (
            "JCODE_HOME",
            PathBuf::from(std::env::var("JCODE_HOME")?),
            home,
        ),
        (
            "JCODE_RUNTIME_DIR",
            PathBuf::from(std::env::var("JCODE_RUNTIME_DIR")?),
            runtime,
        ),
        (
            "JCODE_SOCKET",
            PathBuf::from(std::env::var("JCODE_SOCKET")?),
            expected_socket.clone(),
        ),
    ] {
        if name == "JCODE_SOCKET" {
            if actual != expected {
                bail!("W13 child inherited {name} does not match its owned socket");
            }
        } else if actual.canonicalize()? != expected {
            bail!("W13 child inherited {name} does not match its owned root");
        }
    }
    for path in [socket, debug_socket, ready] {
        if path.exists() && std::fs::symlink_metadata(path)?.file_type().is_symlink() {
            bail!(
                "W13 child rejects symlinked owned-path candidate {}",
                path.display()
            );
        }
    }
    Ok(())
}

async fn reap_owned_child(child: &mut tokio::process::Child, expected_pid: u32) -> Result<String> {
    if child.id() != Some(expected_pid) {
        bail!("owned W13 child pid identity changed before cleanup");
    }
    if let Some(status) = child.try_wait()? {
        return Ok(format!("already-exited:{status}"));
    }
    child.start_kill().context("signal owned W13 child")?;
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .context("timed out reaping owned W13 child")??;
    Ok(format!("terminated:{status}"))
}

fn write_supervisor_receipt(path: &Path, root: &Path, child_pid: u32) -> Result<()> {
    std::fs::write(
        path,
        format!(
            "root={}\nparent_pid={}\nchild_pid={child_pid}\n",
            root.display(),
            std::process::id()
        ),
    )
    .with_context(|| format!("write W13 supervisor receipt {}", path.display()))
}

fn verify_supervisor_receipt(path: &Path, root: &Path, parent_pid: u32) -> Result<()> {
    let receipt = std::fs::read_to_string(path)
        .with_context(|| format!("read W13 supervisor receipt {}", path.display()))?;
    let expected_root = format!("root={}", root.display());
    let expected_parent = format!("parent_pid={parent_pid}");
    let child_pid = receipt
        .lines()
        .find_map(|line| line.strip_prefix("child_pid="))
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid != 0);
    if !receipt.lines().any(|line| line == expected_root)
        || !receipt.lines().any(|line| line == expected_parent)
        || child_pid.is_none()
    {
        bail!("W13 supervisor receipt does not identify the exact owned parent/group");
    }
    Ok(())
}

fn arm_watchdog_log_receipt(home: &Path, receipt_path: &Path) -> Result<PathBuf> {
    let receipt = jcode::server::w13_test_support::canonical_w13_regular_file(
        receipt_path,
        "watchdog log receipt",
    )?;
    anyhow::ensure!(
        std::fs::metadata(&receipt)?.len() == 0,
        "W13 watchdog receipt nonempty"
    );
    let log_path = jcode::logging::log_path().context("resolve private W13 watchdog log path")?;
    anyhow::ensure!(
        log_path.starts_with(home),
        "W13 watchdog log outside private home"
    );
    std::fs::create_dir_all(
        log_path
            .parent()
            .context("private W13 watchdog log parent")?,
    )?;
    std::fs::File::create_new(&log_path)
        .with_context(|| format!("create private W13 watchdog log {}", log_path.display()))?;
    std::fs::remove_file(&receipt)?;
    std::fs::hard_link(&log_path, receipt_path)
        .with_context(|| format!("retain W13 watchdog log receipt {}", receipt_path.display()))?;
    let receipt = jcode::server::w13_test_support::canonical_w13_regular_file(
        receipt_path,
        "retained watchdog log receipt",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let source = std::fs::metadata(&log_path)?;
        let retained = std::fs::metadata(&receipt)?;
        if source.dev() != retained.dev() || source.ino() != retained.ino() {
            bail!("W13 watchdog log receipt is not linked to its private source log");
        }
    }
    Ok(receipt)
}

fn watchdog_remaining(started: Instant) -> Result<Duration> {
    let remaining = WATCHDOG_PARENT_DEADLINE.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        bail!(
            "W13 watchdog parent exceeded its {}ms deadline",
            WATCHDOG_PARENT_DEADLINE.as_millis()
        );
    }
    Ok(remaining)
}

async fn run_watchdog_recovery(socket: &Path, started: Instant) -> Result<()> {
    let mut client = tokio::time::timeout(
        watchdog_remaining(started)?,
        jcode::server::Client::connect_with_path(socket.to_path_buf()),
    )
    .await
    .context("W13 watchdog client connection exceeded the parent deadline")??;
    let subscribe_id = tokio::time::timeout(watchdog_remaining(started)?, client.subscribe())
        .await
        .context("W13 watchdog Subscribe send exceeded the parent deadline")??;
    println!(
        "W13_WATCHDOG_SUBSCRIBE_SENT request_id={subscribe_id} socket={}",
        socket.display()
    );

    match tokio::time::timeout(watchdog_remaining(started)?, client.read_event()).await {
        Ok(Err(error)) => {
            println!("W13_WATCHDOG_SUBSCRIBE_TERMINATED request_id={subscribe_id} error={error:#}")
        }
        Ok(Ok(event)) => {
            bail!("W13 watchdog Subscribe unexpectedly completed before recovery: {event:?}")
        }
        Err(_) => bail!("W13 watchdog pending Subscribe exceeded the parent deadline"),
    }

    let remaining = watchdog_remaining(started)?;
    let handoff = tokio::time::timeout(
        remaining,
        jcode::server::await_reload_handoff(socket, remaining),
    )
    .await
    .context("W13 watchdog reload handoff exceeded the parent deadline")?;
    if handoff != jcode::server::ReloadWaitStatus::Ready {
        bail!("W13 watchdog recovery did not reach Ready: {handoff:?}");
    }
    println!("W13_WATCHDOG_RECOVERY_READY socket={}", socket.display());
    Ok(())
}

fn supervisor_deadline(mode: &str) -> Duration {
    if mode == "watchdog" {
        WATCHDOG_SUPERVISOR_DEADLINE
    } else {
        SUPERVISOR_DEADLINE
    }
}

async fn parent(args: &[String]) -> Result<()> {
    let mode = arg_value(args, "--mode")?;
    if !matches!(mode.as_str(), "fallback" | "healthy" | "watchdog") {
        bail!("W13 parent mode must be `fallback`, `healthy`, or `watchdog`");
    }
    let root = PathBuf::from(arg_value(args, "--root")?)
        .canonicalize()
        .context("canonicalize supervisor-owned W13 temp root")?;
    let receipt = PathBuf::from(arg_value(args, "--receipt")?);
    let reexec_receipt = jcode::server::w13_test_support::canonical_w13_regular_file(
        &PathBuf::from(arg_value(args, "--reexec-receipt")?),
        "reexec receipt",
    )?;
    let runtime = root.join("runtime");
    let home = root.join("home");
    std::fs::create_dir_all(&runtime)?;
    std::fs::create_dir_all(&home)?;
    jcode::env::set_var("JCODE_HOME", &home);
    jcode::env::set_var("JCODE_RUNTIME_DIR", &runtime);
    let socket = runtime.join("w13.sock");
    let debug_socket = runtime.join("w13-debug.sock");
    jcode::server::set_socket_path(
        socket
            .to_str()
            .context("owned W13 socket path is not UTF-8")?,
    );
    let resolved_target = jcode::server::socket_path();
    if resolved_target != socket
        || resolved_target
            .file_name()
            .is_some_and(|name| name == "jcode.sock")
    {
        bail!(
            "refusing W13 measurement: CLI target {} is not the owned private socket {}",
            resolved_target.display(),
            socket.display()
        );
    }
    if mode == "watchdog" {
        let watchdog_log_receipt = arm_watchdog_log_receipt(
            &home,
            &PathBuf::from(arg_value(args, "--watchdog-log-receipt")?),
        )?;
        println!(
            "W13_WATCHDOG_LOG_RECEIPT={}",
            watchdog_log_receipt.display()
        );
    }
    println!("W13_PARENT_ROOT={}", root.display());
    println!("W13_DRIVER_TARGET={}", resolved_target.display());
    let ready = runtime.join("w13.ready");
    let nonce = format!(
        "w13-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let executable = std::env::current_exe().context("resolve W13 driver executable")?;
    let mut child = tokio::process::Command::new(&executable)
        .arg("--child")
        .arg("--mode")
        .arg(&mode)
        .arg("--root")
        .arg(&root)
        .arg("--socket")
        .arg(&socket)
        .arg("--debug-socket")
        .arg(&debug_socket)
        .arg("--ready")
        .arg(&ready)
        .arg("--nonce")
        .arg(&nonce)
        .env("JCODE_HOME", &home)
        .env("JCODE_RUNTIME_DIR", &runtime)
        .env("JCODE_SOCKET", &socket)
        .env("JCODE_W13_REEXEC_RECEIPT", &reexec_receipt)
        .spawn()
        .context("spawn owned W13 child")?;
    let child_pid = child.id().context("owned W13 child missing pid")?;
    write_supervisor_receipt(&receipt, &root, child_pid)?;

    let measurement_started = Instant::now();
    let result = async {
        let _first_ready = wait_for_ready(&ready, &nonce).await?;
        let expected = expected_reexec_identity(&mode)?;
        match mode.as_str() {
            "fallback" => jcode::cli::commands::run_server_reload_command(false, true).await?,
            "healthy" => jcode::cli::commands::run_server_reload_command(true, true).await?,
            "watchdog" => run_watchdog_recovery(&socket, measurement_started).await?,
            _ => unreachable!("parent mode was validated before spawning the child"),
        }
        jcode::server::w13_test_support::validate_w13_reexec_identity_receipt(
            &reexec_receipt,
            &root,
            &socket,
            child_pid,
            &expected.source_binary,
            &expected.sha256,
        )
        .with_context(|| {
            format!(
                "W13 observed reexec identity did not match {} expected target",
                expected.selection
            )
        })?;
        println!(
            "W13_REEXEC_IDENTITY_VALIDATED selection={} replacement_pid={} source_binary={} source_sha256={}",
            expected.selection,
            child_pid,
            expected.source_binary.display(),
            expected.sha256
        );
        Ok(())
    }
    .await;

    // An exec retains this PID. Success or expected error is valid only after
    // the exact spawned child has been reaped; retain both outcomes.
    let cleanup = reap_owned_child(&mut child, child_pid).await;
    if let Ok(outcome) = &cleanup {
        println!("W13_CHILD_REAPED pid={child_pid} outcome={outcome}");
    }
    println!(
        "W13_PARENT_PATHS_BEFORE_SUPERVISOR_DROP root_exists={} socket_exists={} debug_exists={} ready_exists={}",
        root.exists(),
        socket.exists(),
        debug_socket.exists(),
        ready.exists()
    );
    match (result, cleanup) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(measurement), Ok(_)) => Err(measurement),
        (Ok(()), Err(cleanup)) => Err(cleanup),
        (Err(measurement), Err(cleanup)) => {
            Err(measurement.context(format!("owned W13 child cleanup also failed: {cleanup:#}")))
        }
    }
}

#[cfg(unix)]
fn configure_supervised_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_supervised_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn signal_supervised_process_group(pid: u32, signal: i32) -> Result<()> {
    if unsafe { libc::kill(-(pid as i32), signal) } != 0 {
        bail!(
            "signal W13 supervisor process group {pid}: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn signal_supervised_process_group(_pid: u32, _signal: i32) -> Result<()> {
    bail!("W13 OS-process supervision requires Unix")
}

#[cfg(unix)]
fn supervised_process_group_gone(pid: u32) -> bool {
    (unsafe { libc::kill(-(pid as i32), 0) }) != 0
        && matches!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        )
}

#[cfg(not(unix))]
fn supervised_process_group_gone(_pid: u32) -> bool {
    false
}

fn wait_for_supervised_parent(child: &mut Child, deadline: Instant) -> Result<Option<ExitStatus>> {
    loop {
        if let Some(status) = child.try_wait().context("poll supervised W13 parent")? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(SUPERVISOR_POLL);
    }
}

fn supervisor(mode: &str) -> Result<()> {
    if !matches!(mode, "fallback" | "healthy" | "watchdog") {
        bail!("mode must be `fallback`, `healthy`, or `watchdog`");
    }
    let root_guard = tempfile::tempdir().context("create supervisor-owned W13 temp root")?;
    let root = root_guard
        .path()
        .canonicalize()
        .context("canonicalize supervisor-owned W13 temp root")?;
    let receipt = root.join("w13-supervisor-receipt");
    let executable = std::env::current_exe().context("resolve W13 supervisor executable")?;
    let (stdout_file, stdout_path) = tempfile::NamedTempFile::new()
        .context("create persistent W13 stdout receipt")?
        .keep()
        .context("persist W13 stdout receipt")?;
    let (stderr_file, stderr_path) = tempfile::NamedTempFile::new()
        .context("create persistent W13 stderr receipt")?
        .keep()
        .context("persist W13 stderr receipt")?;
    let (reexec_receipt_file, reexec_receipt_path) = tempfile::NamedTempFile::new()
        .context("create persistent W13 reexec receipt")?
        .keep()
        .context("persist W13 reexec receipt")?;
    drop(reexec_receipt_file);
    let watchdog_log_receipt = if mode == "watchdog" {
        let (file, path) = tempfile::NamedTempFile::new()
            .context("create persistent W13 watchdog log receipt")?
            .keep()
            .context("persist W13 watchdog log receipt")?;
        drop(file);
        Some(path)
    } else {
        None
    };
    let mut command = Command::new(executable);
    command
        .arg("--parent")
        .arg("--mode")
        .arg(mode)
        .arg("--root")
        .arg(&root)
        .arg("--receipt")
        .arg(&receipt)
        .arg("--reexec-receipt")
        .arg(&reexec_receipt_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));
    if let Some(path) = &watchdog_log_receipt {
        command.arg("--watchdog-log-receipt").arg(path);
    }
    configure_supervised_process_group(&mut command);
    let mut child = command.spawn().context("spawn supervised W13 parent")?;
    let pid = child.id();
    println!("W13_SUPERVISOR_STARTED pid={pid} root={}", root.display());
    let deadline = supervisor_deadline(mode);
    let (status, timed_out) = match wait_for_supervised_parent(
        &mut child,
        Instant::now() + deadline,
    )? {
        Some(status) => (status, false),
        None => {
            println!(
                "W13_SUPERVISOR_TIMEOUT pid={pid} deadline_ms={} stdout_receipt={} stderr_receipt={}",
                deadline.as_millis(),
                stdout_path.display(),
                stderr_path.display()
            );
            verify_supervisor_receipt(&receipt, &root, pid)?;
            signal_supervised_process_group(pid, libc::SIGTERM)?;
            if let Some(status) =
                wait_for_supervised_parent(&mut child, Instant::now() + SUPERVISOR_TERM_GRACE)?
            {
                (status, true)
            } else {
                signal_supervised_process_group(pid, libc::SIGKILL)?;
                (
                    wait_for_supervised_parent(&mut child, Instant::now() + SUPERVISOR_KILL_GRACE)?
                        .context("W13 cleanup remained unresolved after TERM and KILL grace")?,
                    true,
                )
            }
        }
    };

    let before_drop = root.exists();
    println!(
        "W13_SUPERVISOR_RECEIPTS stdout={} stderr={} reexec={}",
        stdout_path.display(),
        stderr_path.display(),
        reexec_receipt_path.display()
    );
    if let Some(path) = &watchdog_log_receipt {
        println!("W13_SUPERVISOR_WATCHDOG_LOG_RECEIPT={}", path.display());
    }
    drop(root_guard);
    println!(
        "W13_SUPERVISOR_PATHS_BEFORE_DROP root_exists={before_drop} AFTER_DROP root_exists={}",
        root.exists()
    );
    let group_gone = supervised_process_group_gone(pid);
    println!(
        "W13_SUPERVISOR_GROUP_CLEANUP pid={pid} group_gone={}",
        group_gone
    );
    if timed_out {
        bail!("W13 supervisor timed out after {}ms", deadline.as_millis());
    }
    if !group_gone {
        bail!("W13 supervisor left an owned process group for pid {pid}");
    }
    if !status.success() {
        bail!("W13 supervised parent exited {status}");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--child") {
        return child(&args).await;
    }
    if args.iter().any(|arg| arg == "--parent") {
        return parent(&args).await;
    }
    if args.get(1).map(String::as_str) == Some("serve") {
        return serve(&args).await;
    }
    let mode = args.get(1).map(String::as_str).unwrap_or("fallback");
    supervisor(mode)
}
