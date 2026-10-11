use super::*;
use std::{
    os::windows::{ffi::OsStrExt, process::CommandExt},
    process::{Child, Command},
    time::Instant,
};
use windows::{
    Win32::{
        Foundation::{
            APPMODEL_ERROR_NO_PACKAGE, CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE,
            WAIT_OBJECT_0,
        },
        Storage::{FileSystem::GetBinaryTypeW, Packaging::Appx::GetCurrentPackageFullName},
        System::{
            Threading::{CREATE_NO_WINDOW, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
            WindowsProgramming::SCS_64BIT_BINARY,
        },
    },
    core::PCWSTR,
};

pub(super) fn is_portable() -> Result<bool> {
    let mut length = 0;
    let result = unsafe { GetCurrentPackageFullName(&mut length, None) };
    if result == APPMODEL_ERROR_NO_PACKAGE {
        Ok(cfg!(target_arch = "x86_64"))
    } else if result == ERROR_INSUFFICIENT_BUFFER {
        Ok(false)
    } else {
        bail!("Cannot identify installation channel: {}", result.0)
    }
}
pub(super) fn validate_binary(path: &Path) -> Result<()> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut kind = 0;
    unsafe { GetBinaryTypeW(PCWSTR(wide.as_ptr()), &mut kind) }
        .context("Invalid Windows executable")?;
    ensure!(
        kind == SCS_64BIT_BINARY,
        "Expected a 64-bit Windows executable"
    );
    Ok(())
}

pub struct Launched {
    process: Child,
    directory: PathBuf,
    committed: bool,
}
impl Launched {
    pub fn commit(mut self) -> Result<()> {
        ensure!(
            self.process.try_wait()?.is_none(),
            "Update helper exited before installation"
        );
        fs::write(self.directory.join("commit"), b"update")?;
        self.committed = true;
        Ok(())
    }
}
impl Drop for Launched {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.process.kill();
            let _ = self.process.wait();
            drop(Prepared {
                directory: self.directory.clone(),
                keep: false,
            });
        }
    }
}
pub fn launch(mut prepared: Prepared) -> Result<Launched> {
    let process = Command::new(prepared.directory.join("updater.exe"))
        .arg("--apply-update")
        .arg(prepared.directory.join("plan.json"))
        .current_dir(&prepared.directory)
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()?;
    // Once spawned, the guard stops our own helper before cleaning its files
    // on every failure path, including a failed try_wait/readiness check.
    prepared.keep = true;
    let mut launched = Launched {
        process,
        directory: prepared.directory.clone(),
        committed: false,
    };
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        if let Some(code) = launched.process.try_wait()? {
            bail!("Update helper failed to start: {code}");
        }
        if prepared.directory.join("ready").exists() {
            return Ok(launched);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    bail!("Update helper startup timed out")
}

#[derive(Serialize, Deserialize)]
pub struct Notice {
    pub success: bool,
    pub message: String,
}
fn notice_path() -> PathBuf {
    crate::config::Config::directory().join("update-result.json")
}
fn write_notice(success: bool, message: String) -> Result<()> {
    fs::create_dir_all(crate::config::Config::directory())?;
    fs::write(
        notice_path(),
        serde_json::to_vec(&Notice { success, message })?,
    )?;
    Ok(())
}
pub fn startup_notice() -> Result<Option<Notice>> {
    let path = notice_path();
    match fs::read(&path) {
        Ok(bytes) => {
            let notice = serde_json::from_slice(&bytes)?;
            fs::remove_file(path)?;
            Ok(Some(notice))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn validate_location(stage: &Path, target: &Path) -> Result<()> {
    ensure!(
        stage.is_absolute() && target.is_absolute(),
        "Update paths must be absolute"
    );
    let canonical_stage = stage.canonicalize()?;
    let install_directory = target
        .parent()
        .context("Missing installation directory")?
        .canonicalize()?;
    ensure!(
        canonical_stage.parent() == Some(install_directory.as_path()),
        "Update staging directory must be inside the installation directory"
    );
    ensure!(
        canonical_stage
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(".shitu-update-")),
        "Unexpected update directory"
    );
    ensure!(
        target
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe")),
        "Expected an executable target"
    );
    Ok(())
}
fn read_plan(stage: &Path) -> Result<Plan> {
    let bytes = bounded_bytes(File::open(stage.join("plan.json"))?, 64 * 1024)?;
    let plan: Plan = serde_json::from_slice(&bytes)?;
    validate_location(stage, &plan.target)?;
    Ok(plan)
}
pub fn signal_started() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--update-started")) {
        return Ok(());
    }
    let stage = PathBuf::from(args.next().context("Missing update startup directory")?);
    let plan = read_plan(&stage)?;
    ensure!(
        std::env::current_exe()?.canonicalize()? == plan.target.canonicalize()?,
        "Unexpected restarted executable"
    );
    ensure!(
        file_hash(&plan.target)? == plan.new_hash,
        "Restarted executable checksum mismatch"
    );
    fs::write(stage.join("started"), b"ready")?;
    Ok(())
}

struct Process(HANDLE);
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

// No custom UI or forced termination of the parent. The app quits only after
// it receives helper readiness; a live parent or a missing commit blocks writes.
fn apply(plan_path: &Path) -> Result<()> {
    let stage = plan_path.parent().context("Missing staging directory")?;
    ensure!(
        plan_path.file_name() == Some(std::ffi::OsStr::new("plan.json")),
        "Unexpected update plan"
    );
    let plan = read_plan(stage)?;
    ensure!(
        std::env::current_exe()?.canonicalize()? == stage.join("updater.exe").canonicalize()?,
        "Helper is outside staging directory"
    );
    ensure!(
        file_hash(&plan.target)? == plan.old_hash,
        "Installed program changed during download"
    );
    ensure!(
        file_hash(&stage.join("new.exe"))? == plan.new_hash,
        "Staged program checksum mismatch"
    );
    ensure!(
        Version::parse(&plan.version)? > Version::parse(env!("CARGO_PKG_VERSION"))?,
        "Downgrades are not allowed"
    );
    let parent = Process(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, plan.parent_pid) }?);
    fs::write(stage.join("ready"), b"ready")?;
    ensure!(
        unsafe { WaitForSingleObject(parent.0, 60_000) } == WAIT_OBJECT_0,
        "Application did not exit; no files replaced"
    );
    ensure!(
        stage.join("commit").is_file(),
        "Update was not committed; no files replaced"
    );
    let result = install_and_restart(stage, &plan);
    if let Err(error) = result {
        let message = format!("{error:#}");
        let _ = write_notice(false, message.clone());
        let _ = fs::write(stage.join("error.txt"), &message);
        // Relaunch only the verified restored original. If restoration itself
        // failed, preserve old.exe and error.txt for explicit recovery.
        if file_hash(&plan.target).is_ok_and(|hash| hash == plan.old_hash) {
            Command::new(&plan.target)
                .current_dir(plan.target.parent().unwrap())
                .spawn()
                .context("Old version restored, but restart failed")?;
        }
        bail!(message);
    }
    Ok(())
}
fn replace_files(
    target: &Path,
    candidate: &Path,
    backup: &Path,
    old_hash: &str,
    new_hash: &str,
) -> Result<()> {
    ensure!(
        file_hash(target)? == old_hash,
        "Installed program changed before replacement"
    );
    ensure!(
        file_hash(candidate)? == new_hash,
        "Staged program changed before replacement"
    );
    ensure!(!backup.try_exists()?, "Backup already exists");
    fs::rename(target, backup).context("Cannot back up the running installation")?;
    if let Err(error) = fs::rename(candidate, target) {
        fs::rename(backup, target).with_context(|| {
            format!("Replacement failed ({error}); restoring the original also failed")
        })?;
        bail!("Replacement failed; original restored: {error}");
    }
    Ok(())
}
fn rollback(target: &Path, candidate: &Path, backup: &Path, new_hash: &str) -> Result<()> {
    ensure!(
        file_hash(target)? == new_hash,
        "Installed file changed; refusing to overwrite it during rollback"
    );
    fs::rename(target, candidate).context("Cannot remove failed update")?;
    fs::rename(backup, target)
        .context("Cannot restore old.exe; backup retained in update directory")?;
    Ok(())
}
fn install_and_restart(stage: &Path, plan: &Plan) -> Result<()> {
    let candidate = stage.join("new.exe");
    let backup = stage.join("old.exe");
    replace_files(
        &plan.target,
        &candidate,
        &backup,
        &plan.old_hash,
        &plan.new_hash,
    )?;
    let result = (|| -> Result<()> {
        ensure!(
            file_hash(&plan.target)? == plan.new_hash,
            "Installed executable checksum mismatch"
        );
        write_notice(true, plan.version.clone())?;
        let mut child = Command::new(&plan.target)
            .arg("--update-started")
            .arg(stage)
            .current_dir(plan.target.parent().unwrap())
            .spawn()
            .context("Starting updated version")?;
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(30) {
            if let Some(code) = child.try_wait()? {
                bail!("Updated application exited during startup: {code}");
            }
            if stage.join("started").is_file() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        child
            .kill()
            .context("New version startup timed out; cannot stop it for rollback")?;
        child.wait()?;
        bail!("New version did not acknowledge startup");
    })();
    if let Err(error) = result {
        rollback(&plan.target, &candidate, &backup, &plan.new_hash)
            .with_context(|| format!("Update failed ({error:#}); rollback failed"))?;
        bail!("Update failed; original restored: {error:#}");
    }
    Ok(())
}

pub fn helper_exit_code() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--apply-update")) {
        return None;
    }
    let result = args
        .next()
        .map(PathBuf::from)
        .context("Missing update plan")
        .and_then(|path| apply(&path));
    if let Err(error) = result {
        // Post-exit errors are already persisted before the original is
        // restarted. Do not recreate a notice the restarted app just consumed.
        // Pre-exit failures are returned to the live app by launch().
        let message = format!("{error:#}");
        crate::logging::initialize(crate::config::Config::log_directory(), "update.log");
        crate::logging::error(message);
        Some(1)
    } else {
        Some(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_and_rollback_preserve_exact_bytes() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "shitu-replace-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&root)?;
        let target = root.join("ShiTu.exe");
        let new = root.join("new.exe");
        let backup = root.join("old.exe");
        fs::write(&target, b"original")?;
        fs::write(&new, b"updated")?;
        let old_hash = file_hash(&target)?;
        let new_hash = file_hash(&new)?;
        assert!(replace_files(&target, &new, &backup, &old_hash, "bad checksum").is_err());
        assert_eq!(fs::read(&target)?, b"original");
        replace_files(&target, &new, &backup, &old_hash, &new_hash)?;
        assert_eq!(fs::read(&target)?, b"updated");
        assert_eq!(fs::read(&backup)?, b"original");
        rollback(&target, &new, &backup, &new_hash)?;
        assert_eq!(fs::read(&target)?, b"original");
        assert_eq!(fs::read(&new)?, b"updated");
        fs::remove_file(target)?;
        fs::remove_file(new)?;
        fs::remove_dir(root)?;
        Ok(())
    }
    #[test]
    fn locked_candidate_restores_original_after_rename_failure() -> Result<()> {
        use std::os::windows::fs::OpenOptionsExt;
        let root = std::env::temp_dir().join(format!(
            "shitu-locked-update-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&root)?;
        let target = root.join("ShiTu.exe");
        let new = root.join("new.exe");
        let backup = root.join("old.exe");
        fs::write(&target, b"original")?;
        fs::write(&new, b"updated")?;
        let locked = fs::OpenOptions::new().read(true).share_mode(1).open(&new)?;
        assert!(
            replace_files(
                &target,
                &new,
                &backup,
                &file_hash(&target)?,
                &file_hash(&new)?
            )
            .is_err()
        );
        assert_eq!(fs::read(&target)?, b"original");
        assert!(!backup.exists());
        drop(locked);
        fs::remove_file(target)?;
        fs::remove_file(new)?;
        fs::remove_dir(root)?;
        Ok(())
    }
}
