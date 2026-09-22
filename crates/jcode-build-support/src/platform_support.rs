use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
pub fn running_executable_paths() -> std::io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        if let Ok(path) = std::fs::read_link(entry.path().join("exe")) {
            paths.push(path);
        }
    }
    Ok(paths)
}

#[cfg(target_os = "macos")]
pub fn running_executable_paths() -> std::io::Result<Vec<PathBuf>> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    // SAFETY: the first call asks libproc only for the PID count. The second
    // call receives a correctly-sized pid_t buffer and byte length.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut pids = vec![0 as libc::pid_t; count as usize + 64];
    let bytes = pids
        .len()
        .checked_mul(std::mem::size_of::<libc::pid_t>())
        .and_then(|bytes| i32::try_from(bytes).ok())
        .ok_or_else(|| std::io::Error::other("process list buffer is too large"))?;
    // SAFETY: `pids` is writable for `bytes` bytes and remains alive for the call.
    let listed = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    if listed < 0 {
        return Err(std::io::Error::last_os_error());
    }
    pids.truncate(listed as usize);

    let mut paths = Vec::new();
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    for pid in pids.into_iter().filter(|pid| *pid > 0) {
        buffer.fill(0);
        // SAFETY: `buffer` is valid and writable for the length passed to libproc.
        let length = unsafe {
            libc::proc_pidpath(
                pid,
                buffer.as_mut_ptr().cast(),
                buffer.len().try_into().unwrap_or(u32::MAX),
            )
        };
        if length <= 0 {
            continue;
        }
        let length = usize::try_from(length)
            .unwrap_or_default()
            .min(buffer.len());
        let bytes = &buffer[..length];
        let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
        if !bytes.is_empty() {
            paths.push(PathBuf::from(OsStr::from_bytes(bytes)));
        }
    }
    Ok(paths)
}

#[cfg(windows)]
pub fn running_executable_paths() -> std::io::Result<Vec<PathBuf>> {
    // Windows refuses to remove a directory containing a loaded executable.
    // The pruning caller treats that sharing violation as a retained version.
    Ok(Vec::new())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn running_executable_paths() -> std::io::Result<Vec<PathBuf>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "running executable discovery is unsupported on this platform",
    ))
}

/// Set file permissions to owner read/write/execute (0o755).
/// No-op on Windows (executability is determined by file extension).
pub fn set_permissions_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(path, perms)
    }
    #[cfg(windows)]
    {
        let _ = path;
        Ok(())
    }
}

/// Atomically swap a symlink by creating a temp symlink and renaming.
///
/// On Unix: creates temp symlink, then renames over target (atomic).
/// On Windows: stages the source, renames the target aside, then moves the
/// staged file into place. This avoids the lock on a running executable but is
/// not fully atomic.
pub fn atomic_symlink_swap(src: &Path, dst: &Path, temp: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(temp);
        std::os::unix::fs::symlink(src, temp)?;
        std::fs::rename(temp, dst)?;
    }
    #[cfg(windows)]
    {
        // Windows keeps a loaded executable open, so removing or copying over
        // the PATH launcher fails with ERROR_SHARING_VIOLATION. It does allow
        // the directory entry to be renamed while the process keeps running
        // from its existing handle. Stage the new file, rename the old entry
        // aside, then put the staged file at the stable path. This is the same
        // rename-aside strategy used by the PowerShell installer.
        let _ = std::fs::remove_file(temp);
        std::fs::copy(src, temp)?;

        let operation_id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        );
        let old = dst.parent().unwrap_or_else(|| Path::new(".")).join(format!(
            ".jcode-launcher-old-{operation_id}{}",
            dst.extension()
                .map(|extension| format!(".{}", extension.to_string_lossy()))
                .unwrap_or_default()
        ));
        let mut moved_old = false;
        if dst.exists() {
            match std::fs::rename(dst, &old) {
                Ok(()) => moved_old = true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    let _ = std::fs::remove_file(temp);
                    return Err(error);
                }
            }
        }

        if let Err(error) = std::fs::rename(temp, dst) {
            if moved_old {
                let _ = std::fs::rename(&old, dst);
            }
            let _ = std::fs::remove_file(temp);
            return Err(error);
        }

        // An old loaded executable cannot be deleted until its process exits.
        // Best-effort cleanup is safe; later installs can remove leftovers.
        if moved_old {
            let _ = std::fs::remove_file(old);
        }
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::atomic_symlink_swap;

    #[test]
    fn windows_swap_replaces_existing_launcher_via_staged_file() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let src = dir.path().join("source.exe");
        let dst = dir.path().join("jcode.exe");
        let temp = dir.path().join(".jcode-current");
        std::fs::write(&src, b"new binary").expect("source");
        std::fs::write(&dst, b"old binary").expect("destination");

        atomic_symlink_swap(&src, &dst, &temp).expect("swap succeeds");

        assert_eq!(std::fs::read(&dst).expect("new launcher"), b"new binary");
        assert!(!temp.exists());
        let stale_old_launchers: Vec<_> = std::fs::read_dir(dir.path())
            .expect("list swap directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".jcode-launcher-old-")
            })
            .collect();
        assert!(
            stale_old_launchers.is_empty(),
            "swap left stale launcher backups: {stale_old_launchers:?}"
        );
    }
}
