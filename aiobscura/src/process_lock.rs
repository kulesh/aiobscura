//! Process-level locks for coordinating aiobscura and aiobscura-sync.
//!
//! Lock strategy:
//! - `aiobscura-ui.lock` indicates an active TUI process.
//! - `aiobscura-sync.lock` indicates an active ingest owner.
//! - Locks are advisory OS file locks (flock), held for process lifetime.

use anyhow::{Context, Result};
use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{self, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

const UI_LOCK_FILE: &str = "aiobscura-ui.lock";
const SYNC_LOCK_FILE: &str = "aiobscura-sync.lock";

/// How the TUI should run after lock negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum UiRunMode {
    /// TUI owns ingest responsibility and may parse/write.
    OwnsIngest,
    /// Another process owns ingest; TUI must stay read-only.
    ReadOnly,
}

/// Guards held by aiobscura (TUI) process.
#[allow(dead_code)]
pub struct UiProcessGuards {
    /// Held for full process lifetime to mark UI process active.
    _ui_lock: ProcessLock,
    /// Held only when UI is ingest owner.
    _sync_lock: Option<ProcessLock>,
    /// Startup mode derived from lock negotiation.
    pub mode: UiRunMode,
}

/// Guard held by aiobscura-sync process.
#[allow(dead_code)]
pub struct SyncProcessGuard {
    /// Held for full process lifetime to mark sync process active.
    _sync_lock: ProcessLock,
}

/// Acquire locks for aiobscura.
///
/// Behavior:
/// - Fails if another aiobscura instance is running.
/// - Enters read-only mode if aiobscura-sync already owns ingest.
/// - Otherwise owns ingest and keeps sync lock for process lifetime.
#[allow(dead_code)]
pub fn acquire_ui_guards(db_path: &Path) -> Result<UiProcessGuards> {
    let ui_lock = acquire_lock(UI_LOCK_FILE, db_path).with_context(|| {
        "failed to start aiobscura: another aiobscura instance appears to be running"
    })?;

    match try_acquire_lock(SYNC_LOCK_FILE, db_path)? {
        Some(sync_lock) => Ok(UiProcessGuards {
            _ui_lock: ui_lock,
            _sync_lock: Some(sync_lock),
            mode: UiRunMode::OwnsIngest,
        }),
        None => Ok(UiProcessGuards {
            _ui_lock: ui_lock,
            _sync_lock: None,
            mode: UiRunMode::ReadOnly,
        }),
    }
}

/// Acquire lock for aiobscura-sync.
///
/// Behavior:
/// - Fails if aiobscura is already running.
/// - Fails if another aiobscura-sync instance is already running.
#[allow(dead_code)]
pub fn acquire_sync_guard(db_path: &Path) -> Result<SyncProcessGuard> {
    // Probe UI lock first while holding it briefly so startup is serialized
    // against concurrent aiobscura launches.
    let ui_probe = acquire_lock(UI_LOCK_FILE, db_path).with_context(|| {
        "refusing to start aiobscura-sync: aiobscura is already running (not safe to run both)"
    })?;

    let sync_lock = acquire_lock(SYNC_LOCK_FILE, db_path).with_context(|| {
        "failed to start aiobscura-sync: another aiobscura-sync or ingest owner is already running"
    })?;

    drop(ui_probe);
    Ok(SyncProcessGuard {
        _sync_lock: sync_lock,
    })
}

struct ProcessLock {
    file: File,
    path: PathBuf,
}

impl Drop for ProcessLock {
    fn drop(&mut self) {
        let _ = unlock_file(&self.file);
        // Best-effort cleanup of lock file itself (not required for correctness).
        let _ = fs::remove_file(&self.path);
    }
}

fn acquire_lock(filename: &str, db_path: &Path) -> Result<ProcessLock> {
    match try_acquire_lock(filename, db_path)? {
        Some(lock) => Ok(lock),
        None => anyhow::bail!("lock is already held: {}", filename),
    }
}

fn try_acquire_lock(filename: &str, db_path: &Path) -> Result<Option<ProcessLock>> {
    let dir = lock_dir()?;
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create runtime lock directory: {}", dir.display()))?;

    let path = dir.join(scoped_lock_filename(filename, db_path));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("failed to open lock file: {}", path.display()))?;

    match lock_file_nonblocking(&file) {
        Ok(()) => {
            // Write basic owner info for debugging.
            let _ = file.set_len(0);
            let _ = file.seek(SeekFrom::Start(0));
            let _ = writeln!(file, "pid={}", std::process::id());
            let _ = file.flush();

            Ok(Some(ProcessLock { file, path }))
        }
        Err(e) if is_lock_busy(&e) => Ok(None),
        Err(e) => Err(e).with_context(|| format!("failed to lock file: {}", path.display())),
    }
}

fn lock_dir() -> Result<PathBuf> {
    let mut dir = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => std::env::temp_dir(),
    };
    dir.push("aiobscura");
    Ok(dir)
}

fn scoped_lock_filename(base_filename: &str, db_path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    db_path.to_string_lossy().hash(&mut hasher);
    let digest = hasher.finish();
    format!("{base_filename}.{digest:016x}")
}

fn is_lock_busy(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::WouldBlock)
        || matches!(error.raw_os_error(), Some(11) | Some(35))
}

#[cfg(unix)]
fn lock_file_nonblocking(file: &File) -> io::Result<()> {
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    let fd = file.as_raw_fd();
    // SAFETY: flock is called with a valid file descriptor and constant flags.
    let rc = unsafe { flock(fd, LOCK_EX | LOCK_NB) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn unlock_file(file: &File) -> io::Result<()> {
    const LOCK_UN: i32 = 8;
    let fd = file.as_raw_fd();
    // SAFETY: flock is called with a valid file descriptor and constant flags.
    let rc = unsafe { flock(fd, LOCK_UN) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

#[cfg(not(unix))]
compile_error!("aiobscura process locks currently require Unix (macOS/Linux)");
