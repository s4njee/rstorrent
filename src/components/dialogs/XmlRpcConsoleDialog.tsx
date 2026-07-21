/**
 * Raw XML-RPC console (D15) — a hidden power tool reached only by ⌘/Ctrl+Shift+X.
 *
 * Send any method the daemon exposes and see the decoded result pretty-printed.
 * `system.listMethods` (fetched on open) drives the method-name autocomplete.
 *
 * The backend is the security boundary: it refuses `execute.*` / `method.insert`
 * outright, and refuses anything that looks state-changing unless "Allow
 * mutations" is armed for the session. This dialog just surfaces that policy —
 * it never decides what's safe, so a rejected call comes back as an error here.
 */

import { useEffect, useState } from "react";
import { useUi } from "../../store/ui";
import { xmlrpcCall } from "../../ipc/commands";
import { ModalBase, Button } from "./ModalBase";
import forms from "./forms.module.css";
import styles from "./XmlRpcConsoleDialog.module.css";

type Output =
  | { kind: "idle" }
  | { kind: "ok"; value: unknown; elapsedMs: number }
  | { kind: "error"; message: string };

export function XmlRpcConsoleDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const allowMutations = useUi((s) => s.allowXmlrpcMutations);
  const setArm = useUi((s) => s.setAllowXmlrpcMutations);

  const [method, setMethod] = useState("system.listMethods");
  const [args, setArgs] = useState("");
  const [methods, setMethods] = useState<string[]>([]);
  const [output, setOutput] = useState<Output>({ kind: "idle" });
  const [running, setRunning] = useState(false);

  // Populate autocomplete from the live method table. Read-only, so it needs no
  // arming and works against the mock too; failures just leave it empty.
  useEffect(() => {
    let live = true;
    void xmlrpcCall("system.listMethods", "[]", false)
      .then((r) => {
        if (live && Array.isArray(r.value)) {
          setMethods(r.value.filter((m): m is string => typeof m === "string"));
        }
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  const run = async () => {
    const m = method.trim();
    if (!m || running) return;
    setRunning(true);
    try {
      const r = await xmlrpcCall(m, args, allowMutations);
      setOutput({ kind: "ok", value: r.value, elapsedMs: r.elapsedMs });
    } catch (err) {
      setOutput({ kind: "error", message: String(err) });
    } finally {
      setRunning(false);
    }
  };

  return (
    <ModalBase
      title="XML-RPC Console"
      width={560}
      onCancel={closeDialog}
      onPrimary={run}
      footer={
        <>
          <Button variant="secondary" onClick={closeDialog}>
            Close
          </Button>
          <Button
            variant="primary"
            onClick={run}
            disabled={running || method.trim().length === 0}
          >
            {running ? "Running…" : "Run"}
          </Button>
        </>
      }
    >
      <div className={styles.wrap}>
        <div>
          <div className={styles.label}>Method</div>
          <div className={forms.field}>
            <input
              className={`${forms.input} ${styles.mono}`}
              list="xmlrpc-methods"
              value={method}
              spellCheck={false}
              autoComplete="off"
              placeholder="system.listMethods"
              onChange={(e) => setMethod(e.target.value)}
            />
            <datalist id="xmlrpc-methods">
              {methods.map((m) => (
                <option key={m} value={m} />
              ))}
            </datalist>
          </div>
        </div>

        <div>
          <div className={styles.label}>Arguments</div>
          <textarea
            className={`${forms.textarea} ${styles.mono}`}
            value={args}
            spellCheck={false}
            placeholder='[]  — a JSON array, e.g. ["<info-hash>", ""]'
            onChange={(e) => setArgs(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                void run();
              }
            }}
          />
          <div className={styles.hint}>
            Each element is one argument: strings, numbers, booleans, or nested
            arrays. Leave empty for none. <code>⌘/Ctrl+Enter</code> runs.
          </div>
        </div>

        <div>
          <label className={`${styles.arm} ${allowMutations ? styles.on : ""}`}>
            <input
              type="checkbox"
              className={forms.hidden}
              checked={allowMutations}
              onChange={(e) => setArm(e.target.checked)}
            />
            <span
              className={`${forms.box} ${allowMutations ? forms.checked : ""}`}
            >
              {allowMutations ? "✓" : ""}
            </span>
            Allow mutations (this session)
          </label>
          <div className={styles.hint}>
            State-changing methods are refused until armed.{" "}
            <code>execute.*</code> and <code>method.insert</code> stay blocked
            always.
          </div>
        </div>

        <div className={styles.status}>
          {running && <span>running…</span>}
          {!running && output.kind === "ok" && (
            <>
              <span className={styles.ok}>ok</span>
              <span>{output.elapsedMs} ms</span>
            </>
          )}
          {!running && output.kind === "error" && <span>error</span>}
        </div>

        {output.kind === "error" ? (
          <pre className={`${styles.result} ${styles.error}`}>
            {output.message}
          </pre>
        ) : output.kind === "ok" ? (
          <pre className={styles.result}>
            {JSON.stringify(output.value, null, 2)}
          </pre>
        ) : (
          <pre className={`${styles.result} ${styles.empty}`}>
            Run a method to see its result.
          </pre>
        )}
      </div>
    </ModalBase>
  );
}
