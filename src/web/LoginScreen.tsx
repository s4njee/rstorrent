/**
 * Login screen (WE5-S3). A centered card on the page background — logo mark +
 * wordmark, a password field, a primary cyan button, and an error line. Built
 * from the design tokens (the handoff has no login screen; flag for a design
 * pass).
 */

import { useState, type FormEvent } from "react";
import { webLogin } from "../ipc/web";

export function LoginScreen({ onSuccess }: { onSuccess: () => void }) {
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await webLogin(password);
      onSuccess();
    } catch (err) {
      setError(err instanceof Error ? err.message : "login failed");
      setPassword("");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={S.page}>
      <form style={S.card} onSubmit={(e) => void submit(e)}>
        <div style={S.brand}>
          <span style={S.logo}>r</span>
          <span style={S.wordmark}>
            rtorrent
            <span style={{ color: "var(--text-dim)", fontWeight: 400 }}>
              {" "}
              / web
            </span>
          </span>
        </div>
        <input
          style={S.input}
          type="password"
          placeholder="password"
          autoFocus
          value={password}
          onChange={(e) => setPassword(e.currentTarget.value)}
        />
        <button style={S.button} type="submit" disabled={busy}>
          {busy ? "signing in…" : "Sign in"}
        </button>
        {error && <div style={S.error}>{error}</div>}
      </form>
    </div>
  );
}

const S = {
  page: {
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    height: "100vh",
    background: "var(--bg-field)",
    fontFamily: "var(--font-mono)",
  } as const,
  card: {
    display: "flex",
    flexDirection: "column",
    gap: 12,
    width: 320,
    padding: 24,
    background: "var(--bg-panel)",
    border: "1px solid var(--border-mid)",
    borderRadius: 8,
  } as const,
  brand: {
    display: "flex",
    alignItems: "center",
    gap: 9,
    justifyContent: "center",
    marginBottom: 8,
  } as const,
  logo: {
    width: 22,
    height: 22,
    borderRadius: 5,
    background: "var(--bg-selected)",
    border: "1px solid var(--accent-cyan)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    color: "var(--accent-cyan-bright)",
    fontWeight: 700,
    fontSize: 12,
  } as const,
  wordmark: {
    fontWeight: 700,
    color: "var(--text-primary)",
    fontSize: 13,
  } as const,
  input: {
    padding: "8px 11px",
    border: "1px solid var(--border-strong)",
    borderRadius: 5,
    background: "var(--bg-field)",
    color: "var(--text-body)",
    fontSize: 12,
    fontFamily: "inherit",
  } as const,
  button: {
    padding: "8px 12px",
    border: "1px solid var(--accent-cyan)",
    background: "var(--bg-selected)",
    color: "var(--accent-cyan-bright)",
    borderRadius: 5,
    fontSize: 12,
    fontWeight: 600,
    fontFamily: "inherit",
    cursor: "pointer",
  } as const,
  error: {
    color: "var(--accent-red)",
    fontSize: 11,
    textAlign: "center",
  } as const,
};
