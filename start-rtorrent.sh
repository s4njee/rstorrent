#!/usr/bin/env bash
set -e

# Increase file descriptor limit for rtorrent (minimum 512 required)
ulimit -n 4096 2>/dev/null || true

SESSION_DIR="$HOME/.rtorrent/session"
LOCK_FILE="$SESSION_DIR/rtorrent.lock"
SOCKET_FILE="$HOME/.rtorrent/rpc.socket"
TMUX_SESSION="rtorrent"

mkdir -p "$SESSION_DIR"

# Check if rtorrent is already running in tmux or as process
if tmux has-session -t "$TMUX_SESSION" 2>/dev/null; then
    echo "[info] rtorrent tmux session '$TMUX_SESSION' is already running."
    echo "[info] Attach to it anytime with: tmux attach -t $TMUX_SESSION"
    exit 0
fi

if pgrep -x rtorrent >/dev/null 2>&1; then
    echo "[info] rtorrent process is already running."
    exit 0
fi

# Clean up stale lock and socket files if rtorrent isn't running
if [ -f "$LOCK_FILE" ]; then
    echo "[info] Removing stale lock file: $LOCK_FILE"
    rm -f "$LOCK_FILE"
fi

if [ -e "$SOCKET_FILE" ]; then
    echo "[info] Removing stale socket file: $SOCKET_FILE"
    rm -f "$SOCKET_FILE"
fi

# Find rtorrent binary
RTORRENT_BIN=$(which rtorrent 2>/dev/null || echo "/opt/homebrew/bin/rtorrent")

if [ ! -x "$RTORRENT_BIN" ]; then
    echo "[error] rtorrent executable not found." >&2
    exit 1
fi

echo "[info] Starting rtorrent in detached tmux session '$TMUX_SESSION'..."
tmux new-session -d -s "$TMUX_SESSION" "ulimit -n 4096; TERM=xterm-256color '$RTORRENT_BIN'"

# Wait up to 3 seconds for socket to be created
for i in {1..15}; do
    if [ -S "$SOCKET_FILE" ]; then
        break
    fi
    sleep 0.2
done

if [ -S "$SOCKET_FILE" ]; then
    echo "[success] rtorrent started successfully!"
    echo "[success] RPC socket active: $SOCKET_FILE"
    echo "[info] Attach to UI: tmux attach -t $TMUX_SESSION"
else
    echo "[warning] rtorrent launched, but RPC socket $SOCKET_FILE was not detected yet."
    echo "[info] Check tmux status: tmux attach -t $TMUX_SESSION"
fi
