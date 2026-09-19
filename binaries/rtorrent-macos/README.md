# rtorrent macOS runtime

The rtorrent daemon the macOS app ships, built by
`tools/build-rtorrent-macos.sh` from source at a pinned tag (0.15.7) with
XML-RPC. Everything here except this README is build output, ignored by git:
`rtorrent` plus the non-system dylibs it needs, each referenced through
`@executable_path`, so the directory is relocatable.

Two things read it:

- `crates/gpui/build.rs` copies it to `target/<profile>/binaries/rtorrent/`,
  beside a cargo-built binary.
- `tools/bundle-gpui-macos.sh` copies it into
  `rstorrent-gpui.app/Contents/Resources/binaries/rtorrent/`.

The Windows equivalent is `binaries/rtorrent-wsl/`.
