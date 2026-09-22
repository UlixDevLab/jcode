use std::ops::{Deref, DerefMut};
use std::process::Child;

/// A detached child that remains source-compatible with `std::process::Child`
/// while ensuring a dropped, still-running process is eventually reaped.
pub struct DetachedChild {
    child: Option<Child>,
}

impl DetachedChild {
    pub(super) fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }
}

impl Deref for DetachedChild {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        self.child.as_ref().expect("detached child already moved")
    }
}

impl DerefMut for DetachedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.child.as_mut().expect("detached child already moved")
    }
}

impl Drop for DetachedChild {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        match child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => reaper::submit(child),
            Err(error) => crate::logging::warn(&format!(
                "Failed to inspect detached child {} before drop: {error}",
                child.id()
            )),
        }
    }
}

pub(super) fn ensure_reaper_started() -> std::io::Result<()> {
    reaper::ensure_started()
}

#[cfg(unix)]
mod reaper {
    use std::io;
    use std::process::Child;
    use std::sync::Mutex;
    use std::sync::mpsc::{self, Sender};
    use std::thread;
    use std::time::Duration;

    static SENDER: Mutex<Option<Sender<Child>>> = Mutex::new(None);

    pub(super) fn ensure_started() -> io::Result<()> {
        sender().map(|_| ())
    }

    pub(super) fn submit(child: Child) {
        let pid = child.id();
        let sender = match sender() {
            Ok(sender) => sender,
            Err(error) => {
                crate::logging::error(&format!(
                    "Detached-child reaper unavailable for pid {pid}: {error}"
                ));
                return;
            }
        };
        if sender.send(child).is_err() {
            crate::logging::error(&format!(
                "Detached-child reaper stopped before accepting pid {pid}"
            ));
        }
    }

    fn sender() -> io::Result<Sender<Child>> {
        let mut slot = SENDER
            .lock()
            .map_err(|_| io::Error::other("detached-child reaper lock poisoned"))?;
        if let Some(sender) = slot.as_ref() {
            return Ok(sender.clone());
        }

        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("jcode-child-reaper".to_string())
            .spawn(move || {
                let mut children: Vec<Child> = Vec::new();
                loop {
                    let timeout = if children.is_empty() {
                        Duration::from_secs(60)
                    } else {
                        Duration::from_millis(20)
                    };
                    match receiver.recv_timeout(timeout) {
                        Ok(child) => children.push(child),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                    children.extend(receiver.try_iter());
                    children.retain_mut(|child| match child.try_wait() {
                        Ok(Some(_)) => false,
                        Ok(None) => true,
                        Err(error) => {
                            crate::logging::warn(&format!(
                                "Failed to reap detached child {}: {error}",
                                child.id()
                            ));
                            false
                        }
                    });
                }
            })?;
        *slot = Some(sender.clone());
        Ok(sender)
    }
}

#[cfg(not(unix))]
mod reaper {
    use std::process::Child;

    pub(super) fn ensure_started() -> std::io::Result<()> {
        Ok(())
    }

    pub(super) fn submit(_child: Child) {}
}
