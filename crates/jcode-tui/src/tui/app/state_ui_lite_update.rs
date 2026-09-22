// Included by state_ui_maintenance.rs. The existing upstream /update stays intact.
fn verified_lite_updater(
    executable: &std::path::Path,
    root: &std::path::Path,
    home: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let canonical = |p: &std::path::Path| p.canonicalize().map_err(|e| e.to_string());
    let root = canonical(root)?;
    let home = canonical(home)?;
    let executable = canonical(executable)?;
    if home != root.join("home") {
        return Err("Lite HOME does not match this installation.".into());
    }
    let package = executable
        .parent()
        .and_then(|bin| bin.parent())
        .ok_or("Missing Lite package directory")?;
    if package.parent() != Some(root.as_path())
        || executable.file_name().and_then(|v| v.to_str()) != Some("jcode")
        || !package.join("release.json").is_file()
        || !package.join("lite-manifest.json").is_file()
    {
        return Err("This command only updates the running Jcode Lite package.".into());
    }
    let updater = canonical(&package.join("update.command"))?;
    if updater.parent() != Some(package) || !updater.is_file() {
        return Err("The Lite updater is missing or points outside its package.".into());
    }
    Ok(updater)
}

impl App {
    pub(super) fn request_lite_update(&mut self, session_id: String) {
        if self.is_processing || self.background_client_action.is_some() {
            self.push_display_message(DisplayMessage::system(
                "Finish or stop the current task before /update_lite. Your session is unchanged."
                    .to_string(),
            ));
            return;
        }
        let updater = (|| {
            let root = std::env::var_os("JCODE_LITE_ROOT")
                .ok_or("/update_lite is available only through the Jcode Lite launcher.")?;
            let home = std::env::var_os("JCODE_HOME").ok_or("Lite HOME is missing.")?;
            let executable = std::env::current_exe().map_err(|e| e.to_string())?;
            verified_lite_updater(&executable, root.as_ref(), home.as_ref())
        })();
        match updater {
            Ok(path) => {
                // Existing hot_reload execs this exact updater after restoring the
                // terminal, passing --resume SESSION --no-update. The updater then
                // verifies/install/restarts Lite, not the shared upstream channel.
                self.save_input_for_reload(&session_id);
                crate::env::set_var("JCODE_MIGRATE_BINARY", path);
                crate::env::set_var("JCODE_LITE_UPDATE_REQUEST", "1");
                self.reload_requested = Some(session_id);
                self.should_quit = true;
            }
            Err(message) => self.push_display_message(DisplayMessage::error(message)),
        }
    }
}

#[cfg(test)]
mod lite_update_tests {
    use super::verified_lite_updater;
    use std::fs;

    #[test]
    fn lite_update_accepts_only_same_installation_package() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let package = root.join("0.2.0-beta.5");
        fs::create_dir_all(package.join("bin")).unwrap();
        fs::create_dir(root.join("home")).unwrap();
        for file in [
            "bin/jcode",
            "release.json",
            "lite-manifest.json",
            "update.command",
        ] {
            fs::write(package.join(file), "fixture").unwrap();
        }
        let result = verified_lite_updater(&package.join("bin/jcode"), root, &root.join("home"));
        assert_eq!(
            result.unwrap(),
            package.join("update.command").canonicalize().unwrap()
        );
        assert!(verified_lite_updater(&package.join("bin/jcode"), root, root).is_err());
        fs::remove_file(package.join("lite-manifest.json")).unwrap();
        assert!(
            verified_lite_updater(&package.join("bin/jcode"), root, &root.join("home")).is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn lite_update_refuses_external_updater_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let package = root.join("0.2.0-beta.5");
        fs::create_dir_all(package.join("bin")).unwrap();
        fs::create_dir(root.join("home")).unwrap();
        for file in ["bin/jcode", "release.json", "lite-manifest.json"] {
            fs::write(package.join(file), "fixture").unwrap();
        }
        fs::write(root.join("external"), "fixture").unwrap();
        std::os::unix::fs::symlink(root.join("external"), package.join("update.command")).unwrap();
        assert!(
            verified_lite_updater(&package.join("bin/jcode"), root, &root.join("home")).is_err()
        );
    }
}
