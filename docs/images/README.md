# README screenshots

Four images, referenced from the top-level [README](../../README.md).

| File | Screen |
|---|---|
| `main-window.png` | Main view, a torrent selected (`?screen=main`) |
| `pieces-bar.png` | General tab with the pieces bar (`?screen=pieces`) |
| `smart-filters.png` | A saved smart filter + a multi-select (`?screen=smart`) |
| `preferences-connection.png` | Preferences → Connection (`?screen=prefs`) |

## Regenerating them

These are captured from the **browser demo** (`src/demo/`) — the real UI running
against mocked IPC + the ten fixtures, so nothing real ends up in a committed
image. No daemon, no desktop build.

```sh
# 1. serve the demo (any free port; Tauri's 1420 may be taken)
npx vite --port 5199 --host 127.0.0.1

# 2. shoot each state with headless Chrome
for s in main pieces smart prefs; do
  chrome --headless=new --hide-scrollbars --window-size=1360,900 \
    --virtual-time-budget=6000 --screenshot="$s.png" \
    "http://127.0.0.1:5199/demo.html?screen=$s"
done
# then rename main→main-window, pieces→pieces-bar, smart→smart-filters,
# prefs→preferences-connection.
```

The demo's fixtures live in [`src/demo/fixtures.ts`](../../src/demo/fixtures.ts);
the Connection pane uses a `https://seedbox.example.com/RPC2` stand-in so no real
endpoint leaks. These are committed to git, so a replacement is permanent — the
old image stays in history.
