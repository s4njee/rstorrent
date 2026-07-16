# Building & releasing rstorrent

## Build a distributable

```sh
npm install
npm run tauri build
```

This runs `vite build` (frontend → `dist/`) then compiles the Rust binary in
release mode and bundles it. Outputs land in
`src-tauri/target/release/bundle/`:

- `macos/rstorrent.app` — the application bundle
- `dmg/rstorrent_<version>_aarch64.dmg` — the drag-to-Applications disk image

The release profile (`src-tauri/Cargo.toml`) enables LTO + `strip` + `opt-level=s`
for a smaller, faster binary, so the build takes a few minutes.

## Icon

The app icon is generated from `docs`/scratch SVG art via `tauri icon <png>`
into `src-tauri/icons/` (`icon.icns` for macOS). Regenerate with:

```sh
npm run tauri icon path/to/icon-1024.png
# then remove the android/ ios/ variants (macOS-only app)
```

## Code signing & notarization

The dev/CI builds are **ad-hoc signed** (unsigned for distribution) — they run
locally but Gatekeeper will warn on another machine. To ship a signed,
notarized build you need an Apple Developer ID certificate. With one installed:

1. Set the signing identity for the bundler:
   ```sh
   export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
   ```
2. Provide notarization credentials (an app-specific password or an API key):
   ```sh
   export APPLE_ID="you@example.com"
   export APPLE_PASSWORD="app-specific-password"
   export APPLE_TEAM_ID="TEAMID"
   ```
3. `npm run tauri build` — Tauri signs the `.app`, submits it to Apple for
   notarization, and staples the ticket to the `.dmg`.

See the Tauri macOS distribution guide for the current variable names and the
hardened-runtime entitlements if you add capabilities that require them.

None of this is required for local development or the mock/live testing flows.

## Release checklist

1. Bump `version` in `package.json` **and** `src-tauri/tauri.conf.json` (+ `Cargo.toml`).
2. `npm run lint && npm run typecheck && npm test` and, in `src-tauri/`,
   `cargo test && cargo clippy --all-targets -- -D warnings`.
3. `npm run tauri build`; run `docs/qa-checklist.md` against the built `.app` on
   a clean macOS account (mock **and** live).
4. Tag the release and attach the `.dmg`.
