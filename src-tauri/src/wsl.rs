//! WSL interop for the Windows build.
//!
//! On macOS the daemon and the app share a filesystem, so a path from rtorrent
//! can be handed straight to Finder. On Windows the daemon lives inside the WSL
//! VM and speaks Linux paths (`/home/you/downloads/x`), while Explorer, the
//! folder pickers and `std::fs` all speak Windows paths. Every path that crosses
//! that boundary has to be translated, in one of two ways:
//!
//!   * `/mnt/c/...`  <->  `C:\...`          — drvfs, the same bytes on both sides
//!   * anything else <->  `\\wsl.localhost\<distro>\...`  — the VM's own ext4
//!
//! The drvfs form is preferred when going Linux -> Windows because it avoids the
//! 9p share, which is an order of magnitude slower and can't report free space.
//!
//! Every helper here degrades to `None` rather than erroring: WSL may not be
//! installed, and the app still has to run (mock mode, or a remote daemon over
//! HTTP), just with the local-filesystem affordances disabled.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Current UNC prefix for the WSL 9p share. `\\wsl$\` is the pre-20H1 spelling;
/// both still resolve, so we accept either on input and emit the modern one.
const UNC_PREFIX: &str = r"\\wsl.localhost\";
const UNC_PREFIX_LEGACY: &str = r"\\wsl$\";

/// Don't flash a console window when shelling out to `wsl.exe` from a GUI app.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The default WSL distribution, probed once per process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distro {
    /// Distribution name as WSL knows it, e.g. `Ubuntu`.
    pub name: String,
    /// The Linux-side home directory of the distro's default user.
    pub home: String,
}

static DISTRO: OnceLock<Option<Distro>> = OnceLock::new();

/// The default distro, or `None` if WSL isn't installed / has no distro.
///
/// The first call starts the WSL VM if it is not already running, which can
/// take a second or two; every later call is free.
pub fn distro() -> Option<&'static Distro> {
    DISTRO.get_or_init(probe).as_ref()
}

/// Ask WSL who it is. One round trip for both fields.
///
/// `wsl.exe -l` emits UTF-16, but the output of `-e` is whatever the Linux
/// process wrote, so asking the shell to echo the values keeps this UTF-8.
fn probe() -> Option<Distro> {
    let out = wsl_command()
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

/// A `wsl.exe` invocation with no console window attached.
fn wsl_command() -> Command {
    let mut cmd = Command::new("wsl.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
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
    let name = &distro()?.name;
    // Trim the leading `/` so it doesn't double up against the prefix.
    let rest = linux.trim_start_matches('/').replace('/', "\\");
    Some(PathBuf::from(format!("{UNC_PREFIX}{name}\\{rest}")))
}

/// `/mnt/c/users/you` -> `C:\users\you`. `None` if this isn't a drvfs mount.
fn drvfs_to_windows(linux: &str) -> Option<PathBuf> {
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
pub fn to_wsl(win: &Path) -> Option<String> {
    let s = win.to_str()?;
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

static FREE_SPACE_CACHE: Mutex<Option<(String, Instant, Option<i64>)>> = Mutex::new(None);

/// Bytes available on the filesystem holding `linux_path`, asked of WSL itself.
///
/// Going through `df` inside the VM rather than `GetDiskFreeSpaceEx` on the UNC
/// path is deliberate: the 9p share reports the *host* volume's free space, not
/// the ext4 filesystem's, and the two diverge once the VHD has grown.
///
/// TTL-cached; call it from the blocking pool, since a miss spawns a process.
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
    let out = wsl_command()
        .args(["-e", "df", "-B1", "--output=avail", linux_path])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // Two lines: the `Avail` header, then the number.
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .nth(1)?
        .trim()
        .parse()
        .ok()
}

/// Move a VM-local path into the distro's freedesktop trash.
///
/// The Windows Recycle Bin doesn't cover the 9p share, so "delete data" would
/// otherwise have to become an `rm` — which the app never does. Writing a
/// `.trashinfo` alongside keeps the file restorable from a Linux file manager.
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
    let out = wsl_command()
        .args(["-e", "sh", "-c", SCRIPT, "_", linux_path])
        .output()
        .map_err(|e| format!("could not run wsl.exe: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Read a file under the WSL user's home (`$HOME/<rel>`).
///
/// Returns `Some(contents)` when WSL is reachable (an empty string if the file
/// simply doesn't exist yet), and `None` only when WSL itself can't be run — so
/// the caller can tell "no such file" apart from "no WSL".
pub fn read_home_file(rel: &str) -> Option<String> {
    let out = wsl_command()
        // `$1` carries the relative path so it can't be reinterpreted by the
        // shell; a missing file yields empty output, not an error.
        .args([
            "-e",
            "sh",
            "-c",
            r#"cat "$HOME/$1" 2>/dev/null || true"#,
            "_",
            rel,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Write `content` to `$HOME/<rel>` inside the WSL VM, replacing the file.
pub fn write_home_file(rel: &str, content: &str) -> Result<(), String> {
    let mut child = wsl_command()
        .args(["-e", "sh", "-c", r#"cat > "$HOME/$1""#, "_", rel])
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
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
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
    fn windows_paths_map_to_drvfs() {
        assert_eq!(
            to_wsl(Path::new(r"C:\Users\you\x.torrent")),
            Some("/mnt/c/Users/you/x.torrent".into())
        );
        assert_eq!(to_wsl(Path::new(r"D:\")), Some("/mnt/d".into()));
        assert_eq!(to_wsl(Path::new(r"E:")), Some("/mnt/e".into()));
    }

    #[test]
    fn unc_wsl_shares_map_back_to_vm_paths() {
        assert_eq!(
            to_wsl(Path::new(r"\\wsl.localhost\Ubuntu\home\you\dl")),
            Some("/home/you/dl".into())
        );
        // The legacy `\\wsl$\` spelling and odd casing both still resolve.
        assert_eq!(to_wsl(Path::new(r"\\wsl$\Ubuntu\srv")), Some("/srv".into()));
        assert_eq!(
            to_wsl(Path::new(r"\\WSL.LOCALHOST\Ubuntu\srv")),
            Some("/srv".into())
        );
        assert_eq!(
            to_wsl(Path::new(r"\\wsl.localhost\Ubuntu")),
            Some("/".into())
        );
    }

    #[test]
    fn unmappable_paths_are_refused_rather_than_guessed() {
        // A real network share is not visible inside the VM.
        assert_eq!(to_wsl(Path::new(r"\\fileserver\share\x")), None);
        assert_eq!(to_wsl(Path::new(r"relative\path")), None);
        // A relative Linux path has no Windows equivalent either.
        assert_eq!(to_windows("downloads/x"), None);
    }

    #[test]
    fn drvfs_round_trips() {
        let win = r"C:\Users\you\Downloads";
        let linux = to_wsl(Path::new(win)).unwrap();
        assert_eq!(linux, "/mnt/c/Users/you/Downloads");
        assert_eq!(to_windows(&linux), Some(PathBuf::from(win)));
    }
}
