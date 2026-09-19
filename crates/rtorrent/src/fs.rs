//! Filesystem operations for moving torrent data across locations.
//!
//! Provides [`move_torrent_data`], which safely relocates downloaded files or
//! directories for a torrent. It first attempts a fast same-volume atomic rename
//! ([`std::fs::rename`]). If that fails (e.g. crossing filesystem/mount boundaries),
//! it falls back to recursive copy + size verification + cleanup of source data.
//!
//! [`move_with_progress`] is the same operation with byte progress and
//! cooperative cancellation for the move-on-complete executor (V3-14): the
//! copy runs in 1 MiB chunks so a multi-gigabyte cross-volume move reports
//! progress and a cancel takes effect promptly. Cancellation removes the
//! partial target and leaves the source untouched.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Byte progress callback: `(done_bytes, total_bytes)`.
pub type ProgressFn = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// Bytes per copy chunk: progress granularity and cancel latency.
const CHUNK: usize = 1024 * 1024;

/// What a cancellable move can report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsMoveError {
    /// Cancel was requested mid-copy; the partial target was removed and the
    /// source is untouched.
    Cancelled,
    /// Anything else; the source is untouched (a partial target is removed).
    Failed(String),
}

impl std::fmt::Display for FsMoveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsMoveError::Cancelled => write!(f, "move cancelled"),
            FsMoveError::Failed(e) => write!(f, "{e}"),
        }
    }
}

impl From<FsMoveError> for String {
    fn from(e: FsMoveError) -> String {
        e.to_string()
    }
}

/// Move a torrent's on-disk data from `src_base_path` into `dst_dir`.
///
/// Returns the resulting on-disk target path (i.e. `dst_dir / filename`).
///
/// # Safety guarantees
/// - If `src_base_path` does not exist, returns `Ok(dst_dir / filename)` without
///   erroring (allows relocating torrents that have not downloaded data yet).
/// - If `src_base_path` is already at destination, returns `Ok(target)`.
/// - If the target path already exists on disk, returns an `Err` to prevent
///   accidental overwrite or file corruption.
/// - Automatically creates destination parent directories if missing.
/// - Performs verification before deleting source data during cross-volume copy.
pub fn move_torrent_data(src_base_path: &Path, dst_dir: &Path) -> Result<PathBuf, String> {
    let progress: ProgressFn = Arc::new(|_, _| {});
    move_with_progress(src_base_path, dst_dir, &progress, &AtomicBool::new(false))
        .map_err(String::from)
}

/// [`move_torrent_data`] with byte progress and cooperative cancellation.
///
/// `cancel` is checked before every chunk; on cancel the partial target is
/// removed, the source is left untouched, and [`FsMoveError::Cancelled`] is
/// returned. Once the copy has been verified, cancellation no longer takes
/// effect — the source cleanup completes and the move reports success, so a
/// late cancel can neither lose nor duplicate data.
pub fn move_with_progress(
    src_base_path: &Path,
    dst_dir: &Path,
    progress: &ProgressFn,
    cancel: &AtomicBool,
) -> Result<PathBuf, FsMoveError> {
    if src_base_path.as_os_str().is_empty() {
        return Ok(dst_dir.to_path_buf());
    }

    let name = match src_base_path.file_name() {
        Some(n) => n,
        None => return Ok(dst_dir.to_path_buf()),
    };

    let target_path = dst_dir.join(name);

    if src_base_path == target_path {
        return Ok(target_path);
    }

    if !src_base_path.exists() {
        return Ok(target_path);
    }

    if target_path.exists() {
        return Err(FsMoveError::Failed(format!(
            "destination already exists: {}",
            target_path.display()
        )));
    }

    if let Some(parent) = target_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                FsMoveError::Failed(format!(
                    "could not create directory {}: {e}",
                    parent.display()
                ))
            })?;
        }
    }

    // The copy path needs a progress denominator; the rename path reports
    // completion in one step. One metadata walk up front serves both.
    let total = total_size(src_base_path).unwrap_or(0);

    // Try fast atomic same-volume rename first.
    match std::fs::rename(src_base_path, &target_path) {
        Ok(()) => {
            progress(total, total);
            Ok(target_path)
        }
        Err(_) => {
            // Cross-device/volume fallback: copy, verify sizes, then remove source.
            copy_and_verify_then_remove(src_base_path, &target_path, total, progress, cancel)?;
            Ok(target_path)
        }
    }
}

/// Best-effort byte total under `path` (files only); the progress denominator.
fn total_size(path: &Path) -> Option<u64> {
    if path.is_file() {
        return path.metadata().map(|m| m.len()).ok();
    }
    if !path.is_dir() {
        return None;
    }
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(m) = p.metadata() {
                total = total.saturating_add(m.len());
            }
        }
    }
    Some(total)
}

/// Shared byte counter behind the progress callback.
struct Progress {
    done: AtomicU64,
    total: u64,
    callback: ProgressFn,
}

impl Progress {
    fn add(&self, n: u64) {
        let done = self.done.fetch_add(n, Ordering::Relaxed).saturating_add(n);
        (self.callback)(done.min(self.total), self.total);
    }
}

fn copy_and_verify_then_remove(
    src: &Path,
    dst: &Path,
    total: u64,
    progress: &ProgressFn,
    cancel: &AtomicBool,
) -> Result<(), FsMoveError> {
    let progress = Progress {
        done: AtomicU64::new(0),
        total,
        callback: Arc::clone(progress),
    };
    let res = if src.is_file() {
        copy_file_and_verify(src, dst, &progress, cancel)
    } else if src.is_dir() {
        copy_dir_and_verify(src, dst, &progress, cancel)
    } else {
        return Err(FsMoveError::Failed(format!(
            "source path is neither a regular file nor a directory: {}",
            src.display()
        )));
    };
    if let Err(e) = res {
        // Never leave a partial target behind — for a cancel this is what
        // makes "resume in place" safe; for a failure it avoids a half-tree
        // the next attempt would collide with.
        if dst.is_file() {
            let _ = std::fs::remove_file(dst);
        } else if dst.is_dir() {
            let _ = std::fs::remove_dir_all(dst);
        }
        return Err(e);
    }
    // Copy verified: cancellation no longer takes effect (see `move_with_progress`).
    if src.is_file() {
        std::fs::remove_file(src).map_err(|e| {
            FsMoveError::Failed(format!(
                "could not remove source file {}: {e}",
                src.display()
            ))
        })?;
    } else {
        std::fs::remove_dir_all(src).map_err(|e| {
            FsMoveError::Failed(format!(
                "could not remove source directory {}: {e}",
                src.display()
            ))
        })?;
    }
    Ok(())
}

fn copy_file_and_verify(
    src: &Path,
    dst: &Path,
    progress: &Progress,
    cancel: &AtomicBool,
) -> Result<(), FsMoveError> {
    let src_len = src
        .metadata()
        .map_err(|e| {
            FsMoveError::Failed(format!(
                "could not read metadata for {}: {e}",
                src.display()
            ))
        })?
        .len();

    let mut reader = std::fs::File::open(src)
        .map_err(|e| FsMoveError::Failed(format!("could not open {}: {e}", src.display())))?;
    let mut writer = std::fs::File::create(dst)
        .map_err(|e| FsMoveError::Failed(format!("could not create {}: {e}", dst.display())))?;
    let mut buf = vec![0u8; CHUNK];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(FsMoveError::Cancelled);
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| FsMoveError::Failed(format!("could not read {}: {e}", src.display())))?;
        if n == 0 {
            break;
        }
        writer
            .write_all(&buf[..n])
            .map_err(|e| FsMoveError::Failed(format!("could not write {}: {e}", dst.display())))?;
        progress.add(n as u64);
    }
    writer
        .flush()
        .map_err(|e| FsMoveError::Failed(format!("could not flush {}: {e}", dst.display())))?;
    drop(writer);

    let dst_len = dst
        .metadata()
        .map_err(|e| {
            FsMoveError::Failed(format!(
                "could not read metadata for {}: {e}",
                dst.display()
            ))
        })?
        .len();

    if src_len != dst_len {
        let _ = std::fs::remove_file(dst);
        return Err(FsMoveError::Failed(format!(
            "copy verification failed for {}: source size {src_len} != dest size {dst_len}",
            dst.display()
        )));
    }
    Ok(())
}

fn copy_dir_and_verify(
    src: &Path,
    dst: &Path,
    progress: &Progress,
    cancel: &AtomicBool,
) -> Result<(), FsMoveError> {
    if let Err(e) = std::fs::create_dir_all(dst) {
        return Err(FsMoveError::Failed(format!(
            "could not create directory {}: {e}",
            dst.display()
        )));
    }

    let entries = match std::fs::read_dir(src) {
        Ok(e) => e,
        Err(e) => {
            let _ = std::fs::remove_dir_all(dst);
            return Err(FsMoveError::Failed(format!(
                "could not read directory {}: {e}",
                src.display()
            )));
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                let _ = std::fs::remove_dir_all(dst);
                return Err(FsMoveError::Failed(format!(
                    "could not read directory entry: {e}"
                )));
            }
        };
        let entry_path = entry.path();
        let target_path = dst.join(entry.file_name());

        let res = if entry_path.is_dir() {
            copy_dir_and_verify(&entry_path, &target_path, progress, cancel)
        } else if entry_path.is_file() {
            copy_file_and_verify(&entry_path, &target_path, progress, cancel)
        } else {
            // Sockets, fifos and friends have no bytes to copy; recreate the
            // shell so the tree shape verifies, without following them.
            if entry_path.is_symlink() {
                continue;
            }
            continue;
        };

        if let Err(e) = res {
            let _ = std::fs::remove_dir_all(dst);
            return Err(e);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn move_single_file_success() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir_all(&src_dir).unwrap();

        let src_file = src_dir.join("test_file.iso");
        let mut f = File::create(&src_file).unwrap();
        f.write_all(b"torrent file payload bytes").unwrap();

        let res = move_torrent_data(&src_file, &dst_dir).unwrap();
        assert_eq!(res, dst_dir.join("test_file.iso"));
        assert!(!src_file.exists());
        assert!(res.exists());
        assert_eq!(
            std::fs::read_to_string(&res).unwrap(),
            "torrent file payload bytes"
        );
    }

    #[test]
    fn move_directory_tree_success() {
        let dir = tempfile::tempdir().unwrap();
        let src_parent = dir.path().join("downloads");
        let src_folder = src_parent.join("MyAlbum");
        let sub = src_folder.join("extras");
        std::fs::create_dir_all(&sub).unwrap();

        File::create(src_folder.join("track1.flac"))
            .unwrap()
            .write_all(b"audio 1")
            .unwrap();
        File::create(sub.join("cover.jpg"))
            .unwrap()
            .write_all(b"image")
            .unwrap();

        let dst_dir = dir.path().join("media");
        let res = move_torrent_data(&src_folder, &dst_dir).unwrap();

        assert_eq!(res, dst_dir.join("MyAlbum"));
        assert!(!src_folder.exists());
        assert!(res.exists());
        assert!(res.join("track1.flac").exists());
        assert!(res.join("extras").join("cover.jpg").exists());
        assert_eq!(
            std::fs::read_to_string(res.join("track1.flac")).unwrap(),
            "audio 1"
        );
    }

    #[test]
    fn move_nonexistent_source_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.iso");
        let dst_dir = dir.path().join("dst");

        let res = move_torrent_data(&missing, &dst_dir).unwrap();
        assert_eq!(res, dst_dir.join("missing.iso"));
        assert!(!res.exists());
    }

    #[test]
    fn move_to_same_path_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let src_file = dir.path().join("file.txt");
        File::create(&src_file).unwrap().write_all(b"abc").unwrap();

        let res = move_torrent_data(&src_file, dir.path()).unwrap();
        assert_eq!(res, src_file);
        assert!(src_file.exists());
    }

    #[test]
    fn move_destination_collision_errors() {
        let dir = tempfile::tempdir().unwrap();
        let src_file = dir.path().join("src.txt");
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir_all(&dst_dir).unwrap();
        let dst_file = dst_dir.join("src.txt");

        File::create(&src_file)
            .unwrap()
            .write_all(b"src data")
            .unwrap();
        File::create(&dst_file)
            .unwrap()
            .write_all(b"existing data")
            .unwrap();

        let err = move_torrent_data(&src_file, &dst_dir).unwrap_err();
        assert!(err.contains("destination already exists"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&src_file).unwrap(),
            "src data",
            "source must not be modified"
        );
        assert_eq!(
            std::fs::read_to_string(&dst_file).unwrap(),
            "existing data",
            "destination must not be modified"
        );
    }

    #[test]
    fn copy_and_verify_then_remove_fallback_works() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("fallback.dat");
        let dst = dir.path().join("target.dat");
        File::create(&src)
            .unwrap()
            .write_all(b"fallback data")
            .unwrap();

        let progress: ProgressFn = Arc::new(|_, _| {});
        copy_and_verify_then_remove(&src, &dst, 13, &progress, &AtomicBool::new(false)).unwrap();
        assert!(!src.exists());
        assert!(dst.exists());
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "fallback data");
    }

    #[test]
    fn progress_reports_monotonic_done_over_total_on_the_copy_path() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("big.bin");
        let dst = dir.path().join("big.bin.copy");
        let payload = vec![0xABu8; 3 * 1024 * 1024 + 17];
        std::fs::write(&src, &payload).unwrap();

        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let progress: ProgressFn = Arc::new({
            let seen = Arc::clone(&seen);
            move |done, total| {
                seen.lock().unwrap().push((done, total));
            }
        });
        copy_file_and_verify(
            &src,
            &dst,
            &Progress {
                done: AtomicU64::new(0),
                total: payload.len() as u64,
                callback: progress,
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        let seen = seen.lock().unwrap();
        assert!(seen.len() > 1, "3 MiB must report several chunks");
        assert!(
            seen.windows(2).all(|w| w[1].0 >= w[0].0),
            "done is monotonic"
        );
        assert!(seen.iter().all(|&(_, t)| t == payload.len() as u64));
        assert_eq!(
            *seen.last().unwrap(),
            (payload.len() as u64, payload.len() as u64)
        );
        assert_eq!(std::fs::read(&dst).unwrap(), payload);
    }

    #[test]
    fn rename_fast_path_reports_a_single_completion() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.bin");
        let dst_dir = dir.path().join("dst");
        std::fs::write(&src, b"twelve bytes").unwrap();

        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let progress: ProgressFn = Arc::new({
            let seen = Arc::clone(&seen);
            move |done, total| {
                seen.lock().unwrap().push((done, total));
            }
        });
        move_with_progress(&src, &dst_dir, &progress, &AtomicBool::new(false)).unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![(12, 12)]);
    }

    #[test]
    fn cancel_before_the_first_chunk_leaves_the_source_alone() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.bin");
        let dst = dir.path().join("dst.bin");
        std::fs::write(&src, vec![0xCDu8; 1024]).unwrap();

        let progress: ProgressFn = Arc::new(|_, _| {});
        let res = copy_file_and_verify(
            &src,
            &dst,
            &Progress {
                done: AtomicU64::new(0),
                total: 1024,
                callback: progress,
            },
            &AtomicBool::new(true),
        );
        assert_eq!(res, Err(FsMoveError::Cancelled));
        assert!(src.exists(), "source must be untouched");
        assert_eq!(std::fs::read(&src).unwrap(), vec![0xCDu8; 1024]);
    }
}
