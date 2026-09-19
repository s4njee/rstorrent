//! WSL interop for the Windows build.
//!
//! A port of `src-tauri/src/wsl.rs`, extended for the runtime this shell
//! ships. On macOS the daemon and the app share a filesystem, so a path from
//! rtorrent can be handed straight to Finder. On Windows the daemon lives
//! inside a WSL VM and speaks Linux paths (`/root/downloads/x`), while
//! Explorer, the folder pickers and `std::fs` all speak Windows paths. Every
//! path that crosses that boundary is translated, in one of two ways:
//!
//!   * `/mnt/c/...`  <->  `C:\...`          — drvfs, the same bytes on both sides
//!   * anything else <->  `\\wsl.localhost\<distro>\...`  — the VM's own ext4
//!
//! The drvfs form is preferred when going Linux -> Windows because it avoids the
//! 9p share, which is an order of magnitude slower and can't report free space.
//!
//! **Which distro.** The Windows build bundles its own rtorrent as a WSL root
//! filesystem (`tools/build-rtorrent-wsl.sh`), imported on first start as a
//! dedicated distro named [`DISTRO_NAME`] — see `crate::daemon_wsl`. When that
//! distro is registered, everything here targets it; otherwise it falls back
//! to the user's default distro, which is where a hand-installed daemon (the
//! Tauri era's `tools/wsl-setup-rtorrent.sh`) lives.
//!
//! The translation rules are pure and compile on every platform, so their
//! tests run on any host. Everything that shells out to `wsl.exe` degrades to
//! `None`/`Err` rather than panicking: WSL may not be installed, and the app
//! still has to run (mock mode, or a remote daemon over HTTP), just with the
//! local-filesystem affordances disabled.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The distro the bundled runtime is imported as.
pub const DISTRO_NAME: &str = "rstorrent";

/// Current UNC prefix for the WSL 9p share. `\\wsl$\` is the pre-20H1 spelling;
/// both still resolve, so we accept either on input and emit the modern one.
const UNC_PREFIX: &str = r"\\wsl.localhost\";
const UNC_PREFIX_LEGACY: &str = r"\\wsl$\";

/// Don't flash a console window when shelling out to `wsl.exe` from a GUI app.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The distro the daemon lives in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distro {
    /// Distribution name as WSL knows it, e.g. `rstorrent` or `Ubuntu`.
    pub name: String,
    /// The Linux-side home directory of the distro's default user.
    pub home: String,
}

/// The probed distro. `None` = not probed yet; `Some(None)` = probed, no WSL.
/// A `Mutex` rather than a `OnceLock` because importing the bundled distro
/// changes the answer mid-run (see [`forget_distro`]).
static DISTRO: Mutex<Option<Option<Distro>>> = Mutex::new(None);

/// The daemon's distro, or `None` if WSL isn't installed / has no distro.
///
/// The first call starts the WSL VM if it is not already running, which can
/// take a second or two; later calls are free until [`forget_distro`].
pub fn distro() -> Option<Distro> {
    let mut cached = DISTRO.lock().ok()?;
    if cached.is_none() {
        *cached = Some(probe());
    }
    cached.clone().flatten()
}

/// Drop the cached distro so the next [`distro`] probes again — after the
/// bundled distro is imported, it takes over from the default one.
pub fn forget_distro() {
    if let Ok(mut cached) = DISTRO.lock() {
        *cached = None;
    }
}

/// Ask WSL who it is. One round trip for both fields.
///
/// `wsl.exe -l` emits UTF-16, but the output of `-e` is whatever the Linux
/// process wrote, so asking the shell to echo the values keeps this UTF-8.
fn probe() -> Option<Distro> {
    let mut command = wsl_command();
    if has_own_distro() {
        command.args(["-d", DISTRO_NAME]);
    }
    let out = command
        .args([
            "-e",
            "sh",
            "-c",
            "printf '%s\\n%s\\n' \"$WSL_DISTRO_NAME\" \"$HOME\"",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let name = lines.next()?.trim().to_string();
    let home = lines.next()?.trim().to_string();
    if name.is_empty() || home.is_empty() {
        return None;
    }
    Some(Distro { name, home })
}

/// A bare `wsl.exe` invocation with no console window attached.
fn wsl_command() -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new("wsl.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// `wsl.exe` aimed at the daemon's distro (`-d <name>`), ready for `-e ...`.
pub fn distro_command() -> Command {
    let mut cmd = wsl_command();
    if let Some(distro) = distro() {
        cmd.args(["-d", &distro.name]);
    }
    cmd
}

/// Whether WSL itself is installed and working (a distro need not exist).
///
/// `wsl.exe` ships as a stub on every Windows 10/11, so its presence proves
/// nothing; `--status` exits non-zero until the WSL feature is installed.
pub fn available() -> bool {
    wsl_command()
        .arg("--status")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The registered distro names, from `wsl.exe -l -q`.
pub fn registered_distros() -> Vec<String> {
    let Ok(out) = wsl_command().args(["-l", "-q"]).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    parse_distro_list(&decode_wsl_output(&out.stdout))
}

/// Whether the bundled runtime's distro has been imported.
pub fn has_own_distro() -> bool {
    registered_distros()
        .iter()
        .any(|name| name.eq_ignore_ascii_case(DISTRO_NAME))
}

/// Import a root filesystem tarball as the dedicated distro.
///
/// `install_dir` holds the distro's virtual disk (`ext4.vhdx`); deleting the
/// distro (`wsl --unregister rstorrent`) deletes that disk and everything in
/// the VM — which is why downloads default to a Windows folder.
///
/// # Errors
///
/// Returns `wsl.exe`'s own message when the import fails.
pub fn import(rootfs: &Path, install_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(install_dir)
        .map_err(|error| format!("could not create {}: {error}", install_dir.display()))?;
    let out = wsl_command()
        .arg("--import")
        .arg(DISTRO_NAME)
        .arg(install_dir)
        .arg(rootfs)
        .args(["--version", "2"])
        .output()
        .map_err(|error| format!("could not run wsl.exe: {error}"))?;
    forget_distro();
    if out.status.success() {
        Ok(())
    } else {
        let mut message = decode_wsl_output(&out.stdout);
        message.push_str(&decode_wsl_output(&out.stderr));
        Err(format!(
            "could not import the rtorrent runtime into WSL: {}",
            message.trim()
        ))
    }
}

/// `wsl.exe`'s own messages (`-l`, `--import`, errors) are UTF-16LE, while
/// anything a Linux process prints is UTF-8. Tell them apart by the NULs that
/// UTF-16 puts in every ASCII character.
#[must_use]
pub fn decode_wsl_output(bytes: &[u8]) -> String {
    let looks_utf16 = bytes.len() >= 2 && bytes.iter().skip(1).step_by(2).any(|b| *b == 0);
    if looks_utf16 {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
            .trim_start_matches('\u{feff}')
            .to_owned()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// One distro name per line, blank lines and stray NULs/BOMs dropped.
fn parse_distro_list(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim_matches(|c: char| c.is_whitespace() || c == '\0' || c == '\u{feff}'))
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Translate a Linux path from the daemon into a Windows path.
///
/// Returns `None` for relative paths and, for VM-local paths, when the distro
/// can't be probed (there is no UNC prefix to build without a distro name).
pub fn to_windows(linux: &str) -> Option<PathBuf> {
    if !linux.starts_with('/') {
        return None;
    }
    if let Some(win) = drvfs_to_windows(linux) {
        return Some(win);
    }
    Some(vm_to_unc(linux, &distro()?.name))
}

/// A VM-local path as the 9p share Explorer can open.
fn vm_to_unc(linux: &str, distro: &str) -> PathBuf {
    // Trim the leading `/` so it doesn't double up against the prefix.
    let rest = linux.trim_start_matches('/').replace('/', "\\");
    PathBuf::from(format!("{UNC_PREFIX}{distro}\\{rest}"))
}

/// `/mnt/c/users/you` -> `C:\users\you`. `None` if this isn't a drvfs mount.
#[must_use]
pub fn drvfs_to_windows(linux: &str) -> Option<PathBuf> {
    let rest = linux.strip_prefix("/mnt/")?;
    let mut chars = rest.chars();
    let letter = chars.next()?;
    if !letter.is_ascii_alphabetic() {
        return None;
    }
    // Must be exactly one letter, then a separator or end of string.
    let tail = match chars.next() {
        None => "",
        Some('/') => &rest[2..],
        Some(_) => return None,
    };
    let drive = letter.to_ascii_uppercase();
    Some(PathBuf::from(format!(
        "{drive}:\\{}",
        tail.replace('/', "\\")
    )))
}

/// Translate a Windows path (from a folder picker or a dropped file) into the
/// Linux path the daemon should be given.
///
/// Returns `None` for relative paths and for UNC paths that aren't a WSL share
/// — a network drive is visible to Windows but not to the VM, so there is no
/// honest translation and the caller must surface that rather than guess.
#[must_use]
pub fn to_wsl(win: &Path) -> Option<String> {
    to_wsl_str(win.to_str()?)
}

fn to_wsl_str(s: &str) -> Option<String> {
    // `\\wsl.localhost\Ubuntu\home\you` -> `/home/you`
    for prefix in [UNC_PREFIX, UNC_PREFIX_LEGACY] {
        if let Some(rest) = strip_prefix_ci(s, prefix) {
            // Drop the distro component; the rest is already VM-absolute.
            let after_distro = match rest.split_once(['\\', '/']) {
                Some((_distro, tail)) => tail,
                // `\\wsl.localhost\Ubuntu` on its own is the VM root.
                None => return Some("/".to_string()),
            };
            return Some(format!("/{}", after_distro.replace('\\', "/")));
        }
    }
    if s.starts_with(r"\\") {
        return None; // some other UNC share; not reachable from the VM
    }
    // `C:\users\you` -> `/mnt/c/users/you`
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        let tail = s[2..].trim_start_matches(['\\', '/']).replace('\\', "/");
        return Some(if tail.is_empty() {
            format!("/mnt/{drive}")
        } else {
            format!("/mnt/{drive}/{tail}")
        });
    }
    None
}

/// Case-insensitive `strip_prefix`, for UNC prefixes users may type any way.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// How long a free-space reading stays good for. The status bar polls at 1 Hz
/// and each miss costs a process spawn, so the number is deliberately stale.
const FREE_SPACE_TTL: Duration = Duration::from_secs(30);

type FreeSpaceCache = Option<(String, Instant, Option<i64>)>;
static FREE_SPACE_CACHE: Mutex<FreeSpaceCache> = Mutex::new(None);

/// Bytes available on the filesystem holding `linux_path`, asked of WSL itself.
///
/// Going through `df` inside the VM rather than `GetDiskFreeSpaceEx` on the UNC
/// path is deliberate: the 9p share reports the *host* volume's free space, not
/// the ext4 filesystem's, and the two diverge once the VHD has grown.
///
/// TTL-cached; call it off the UI thread, since a miss spawns a process.
pub fn free_space(linux_path: &str) -> Option<i64> {
    {
        let cache = FREE_SPACE_CACHE.lock().ok()?;
        if let Some((path, at, value)) = cache.as_ref() {
            if path == linux_path && at.elapsed() < FREE_SPACE_TTL {
                return *value;
            }
        }
    }
    let fresh = df_avail(linux_path);
    if let Ok(mut cache) = FREE_SPACE_CACHE.lock() {
        *cache = Some((linux_path.to_string(), Instant::now(), fresh));
    }
    fresh
}

fn df_avail(linux_path: &str) -> Option<i64> {
    // POSIX `df -Pk` rather than GNU's `-B1 --output=avail`: the bundled
    // distro is Alpine, whose busybox `df` has neither flag.
    let out = distro_command()
        .args(["-e", "df", "-Pk", linux_path])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_df_available(&String::from_utf8_lossy(&out.stdout))
}

/// The `Available` column of `df -Pk` output, in bytes.
fn parse_df_available(text: &str) -> Option<i64> {
    // Header, then `fs 1024-blocks used available capacity mount`.
    let kib: i64 = text
        .lines()
        .nth(1)?
        .split_whitespace()
        .nth(3)?
        .parse()
        .ok()?;
    kib.checked_mul(1024)
}

/// Move a VM-local path into the distro's freedesktop trash.
///
/// The Windows Recycle Bin doesn't cover the 9p share, so "delete data" would
/// otherwise have to become an `rm` — which the app never does. Writing a
/// `.trashinfo` alongside keeps the file restorable from a Linux file manager.
///
/// # Errors
///
/// Returns the shell's message when the move fails.
pub fn trash(linux_path: &str) -> Result<(), String> {
    // `Path=` should strictly be URL-encoded per the freedesktop spec; the
    // common file managers accept a plain path, and encoding it in shell would
    // cost more than it buys.
    const SCRIPT: &str = r#"
set -e
p="$1"
[ -e "$p" ] || { echo "no such path: $p" >&2; exit 1; }
t="${XDG_DATA_HOME:-$HOME/.local/share}/Trash"
mkdir -p "$t/files" "$t/info"
b=$(basename "$p")
n="$b"; i=1
while [ -e "$t/files/$n" ]; do n="$b.$i"; i=$((i + 1)); done
printf '[Trash Info]\nPath=%s\nDeletionDate=%s\n' \
  "$(realpath "$p")" "$(date +%Y-%m-%dT%H:%M:%S)" > "$t/info/$n.trashinfo"
mv -- "$p" "$t/files/$n"
"#;
    run_script(SCRIPT, &[linux_path]).map(|_| ())
}

/// Run a `sh -c` script in the daemon's distro with positional arguments
/// (`$1`, `$2`, … — never interpolated, so paths can't be reinterpreted).
///
/// # Errors
///
/// Returns stderr (or the spawn error) when the script fails.
pub fn run_script(script: &str, args: &[&str]) -> Result<String, String> {
    let out = distro_command()
        .args(["-e", "sh", "-c", script, "_"])
        .args(args)
        .output()
        .map_err(|e| format!("could not run wsl.exe: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(decode_wsl_output(&out.stderr).trim().to_string())
    }
}

/// Read a file under the WSL user's home (`$HOME/<rel>`).
///
/// Returns `Some(contents)` when WSL is reachable (an empty string if the file
/// simply doesn't exist yet), and `None` only when WSL itself can't be run — so
/// the caller can tell "no such file" apart from "no WSL".
pub fn read_home_file(rel: &str) -> Option<String> {
    // `$1` carries the relative path so it can't be reinterpreted by the
    // shell; a missing file yields empty output, not an error.
    run_script(r#"cat "$HOME/$1" 2>/dev/null || true"#, &[rel]).ok()
}

/// Write `content` to `$HOME/<rel>` inside the WSL VM, replacing the file.
///
/// # Errors
///
/// Returns the pipe or shell error when the write fails.
pub fn write_home_file(rel: &str, content: &str) -> Result<(), String> {
    let mut child = distro_command()
        .args([
            "-e",
            "sh",
            "-c",
            r#"mkdir -p "$(dirname "$HOME/$1")" && cat > "$HOME/$1""#,
            "_",
            rel,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run wsl.exe: {e}"))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or("could not open a pipe to wsl.exe")?;
        stdin
            .write_all(content.as_bytes())
            .map_err(|e| format!("could not write to wsl.exe: {e}"))?;
        // stdin drops here, sending EOF so `cat` finishes.
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("wsl.exe did not finish: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(decode_wsl_output(&out.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These exercise the pure translation rules; nothing here starts WSL.

    #[test]
    fn drvfs_paths_map_to_drive_letters() {
        assert_eq!(
            drvfs_to_windows("/mnt/c/users/you"),
            Some(PathBuf::from(r"C:\users\you"))
        );
        assert_eq!(drvfs_to_windows("/mnt/d"), Some(PathBuf::from(r"D:\")));
        // Not a single-letter mount: `/mnt/data` is an ordinary VM directory.
        assert_eq!(drvfs_to_windows("/mnt/data/x"), None);
        assert_eq!(drvfs_to_windows("/home/you"), None);
    }

    #[test]
    fn vm_paths_map_to_the_distro_share() {
        assert_eq!(
            vm_to_unc("/root/downloads/x", DISTRO_NAME),
            PathBuf::from(r"\\wsl.localhost\rstorrent\root\downloads\x")
        );
    }

    #[test]
    fn windows_paths_map_to_drvfs() {
        assert_eq!(
            to_wsl_str(r"C:\Users\you\x.torrent"),
            Some("/mnt/c/Users/you/x.torrent".into())
        );
        assert_eq!(to_wsl_str(r"D:\"), Some("/mnt/d".into()));
        assert_eq!(to_wsl_str(r"E:"), Some("/mnt/e".into()));
    }

    #[test]
    fn unc_wsl_shares_map_back_to_vm_paths() {
        assert_eq!(
            to_wsl_str(r"\\wsl.localhost\Ubuntu\home\you\dl"),
            Some("/home/you/dl".into())
        );
        // The legacy `\\wsl$\` spelling and odd casing both still resolve.
        assert_eq!(to_wsl_str(r"\\wsl$\Ubuntu\srv"), Some("/srv".into()));
        assert_eq!(
            to_wsl_str(r"\\WSL.LOCALHOST\Ubuntu\srv"),
            Some("/srv".into())
        );
        assert_eq!(to_wsl_str(r"\\wsl.localhost\Ubuntu"), Some("/".into()));
    }

    #[test]
    fn unmappable_paths_are_refused_rather_than_guessed() {
        // A real network share is not visible inside the VM.
        assert_eq!(to_wsl_str(r"\\fileserver\share\x"), None);
        assert_eq!(to_wsl_str(r"relative\path"), None);
        // A relative Linux path has no Windows equivalent either.
        assert_eq!(to_windows("downloads/x"), None);
    }

    #[test]
    fn drvfs_round_trips() {
        let win = r"C:\Users\you\Downloads";
        let linux = to_wsl_str(win).unwrap();
        assert_eq!(linux, "/mnt/c/Users/you/Downloads");
        assert_eq!(drvfs_to_windows(&linux), Some(PathBuf::from(win)));
    }

    #[test]
    fn wsl_list_output_is_decoded_from_utf16() {
        // `wsl -l -q` as it arrives: UTF-16LE with a BOM and CRLFs.
        let text = "\u{feff}Ubuntu\r\nrstorrent\r\n\r\n";
        let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let decoded = decode_wsl_output(&bytes);
        assert_eq!(parse_distro_list(&decoded), vec!["Ubuntu", "rstorrent"]);
        // A Linux process's UTF-8 passes through untouched.
        assert_eq!(decode_wsl_output(b"hello\n"), "hello\n");
    }

    #[test]
    fn busybox_df_output_yields_bytes_available() {
        let out = "Filesystem     1024-blocks    Used Available Capacity Mounted on\n\
                   /dev/sdc        1055762868 9416724 992642672       1% /\n";
        assert_eq!(parse_df_available(out), Some(992_642_672 * 1024));
        assert_eq!(parse_df_available("garbage"), None);
    }
}
