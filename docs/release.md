# Building & releasing rstorrent

The desktop app is the native GPUI shell (`crates/gpui`, binary
`rstorrent-gpui`). Each platform has one packaging command, and CI runs the
same commands on a version tag ([`.github/workflows/release.yml`](../.github/workflows/release.yml)).

## macOS

```sh
npm ci
tools/bundle-gpui-macos.sh
```

That one script:
1. stages the bundled rtorrent (`tools/build-rtorrent-macos.sh`, from source into
   `binaries/rtorrent-macos/` the first time, cached after);
2. builds the web console the binary embeds;
3. builds the release binary and assembles `dist-gpui/rstorrent-gpui.app`, with
   the runtime in `Contents/Resources/binaries/rtorrent/`;
4. ad-hoc signs it and verifies it.

`RTORRENT_FORCE_REBUILD=1` rebuilds the runtime from scratch. Zip the app for
distribution with:

```sh
ditto -c -k --sequesterRsrc --keepParent dist-gpui/rstorrent-gpui.app rstorrent-macos.zip
```

The release profile (workspace `Cargo.toml`) enables LTO, `strip` and
`opt-level=s`, so a release build takes a few minutes.

## Windows

The runtime is a Linux rtorrent in a WSL root filesystem, built on Linux x86-64
as root (inside WSL on the Windows machine works, as does another host over SSH):

```sh
sudo tools/build-rtorrent-wsl.sh                              # on the Linux/WSL host
BUILD_HOST=root@<linux-host> tools/build-rtorrent-wsl.sh      # or from elsewhere
```

Then, on Windows:

```powershell
npm ci
.\tools\bundle-gpui-windows.ps1
```

This produces `dist-gpui\windows\rstorrent-<version>-windows-x64.zip`: a
portable folder with `rstorrent-gpui.exe` and
`runtime\rstorrent-rootfs.tar.gz`. See [windows.md](../windows.md) for how the
runtime is imported and started, and for what is still being verified.

## Icons

The app owns its icons in `crates/gpui/assets/icons/`:
- `icon.icns` goes into the `.app` and is embedded for direct launches (`app_icon.rs`).
- `icon.ico` is embedded in the exe by `build.rs`, as resource ID 1, which gpui
  loads as the window icon.

Regenerate both from a 1024 px PNG with any icns/ico tool (e.g. `iconutil`
for `.icns`, ImageMagick for `.ico`).

## Code signing & notarization

Builds are **ad-hoc signed** on macOS and **unsigned** on Windows. They run
locally, but Gatekeeper and SmartScreen warn on another machine.

**macOS, with a Developer ID certificate:**
1. Sign inside-out with your identity instead of `-`: every dylib and the
   `rtorrent` binary under `Contents/Resources/binaries/`, then
   `Contents/MacOS/rstorrent-gpui`, then the bundle, all with
   `--options runtime --timestamp`.
2. Loading the bundled dylibs under the hardened runtime may need the
   `com.apple.security.cs.disable-library-validation` entitlement.
3. Submit with `xcrun notarytool submit … --wait`, then run
   `xcrun stapler staple` on the `.app`.

`tools/bundle-gpui-macos.sh` has the signing loop to adapt.

**Windows:** sign `rstorrent-gpui.exe` with `signtool`, once a certificate
exists. There's no installer yet (see windows.md §5).

None of this is needed for local development or testing.

## Release checklist

1. Bump `version` in `crates/gpui/Cargo.toml`, and in `package.json` if the web
   console changed.
2. `npm run lint && npm run typecheck && npm test`, then
   `cargo fmt --check --all && cargo clippy --workspace --all-targets && cargo test --workspace`.
3. Build both platforms (above) and run [qa-checklist.md](qa-checklist.md)
   against each on a clean account, both mock (`--demo`) and live.
4. Push a `v<version>` tag. The Release workflow builds the WSL runtime, the
   macOS zip and the Windows zip, and attaches both zips to the GitHub Release.
