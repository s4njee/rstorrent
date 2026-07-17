// Transport security helpers (B9). isLocalhost gates delete-data and
// reveal-in-Finder, so a remote daemon must never be mistaken for a local one.
// These mirror `host_is_local` / `is_insecure_credentialed` in Rust — if you
// change one side, change the other.
import { describe, it, expect } from "vitest";
import {
  isLocalhost,
  httpHostIsLocal,
  isInsecureCredentialed,
} from "./settings";

describe("httpHostIsLocal", () => {
  it("recognises loopback in its various spellings", () => {
    expect(httpHostIsLocal("http://127.0.0.1:8080/RPC2")).toBe(true);
    expect(httpHostIsLocal("http://localhost/RPC2")).toBe(true);
    expect(httpHostIsLocal("http://[::1]:8080/RPC2")).toBe(true);
  });

  it("treats a real host as remote", () => {
    expect(httpHostIsLocal("https://seedbox.example.com/RPC2")).toBe(false);
  });

  it("does not mistake userinfo for the host", () => {
    // The host here is evil.example, not localhost.
    expect(httpHostIsLocal("http://localhost@evil.example/RPC2")).toBe(false);
  });

  it("is false for unparseable input rather than throwing", () => {
    expect(httpHostIsLocal("")).toBe(false);
    expect(httpHostIsLocal("not a url")).toBe(false);
  });
});

describe("isLocalhost", () => {
  it("covers every transport kind", () => {
    expect(isLocalhost({ kind: "unixSocket", path: "/x" })).toBe(true);
    expect(isLocalhost({ kind: "tcp", host: "127.0.0.1", port: 5000 })).toBe(
      true,
    );
    expect(isLocalhost({ kind: "tcp", host: "10.0.0.5", port: 5000 })).toBe(
      false,
    );
    // A remote daemon's files are not on this machine.
    expect(
      isLocalhost({
        kind: "http",
        url: "https://box.example/RPC2",
        username: "a",
      }),
    ).toBe(false);
    expect(
      isLocalhost({
        kind: "http",
        url: "http://127.0.0.1:8099/RPC2",
        username: "",
      }),
    ).toBe(true);
  });
});

describe("isInsecureCredentialed", () => {
  it("flags a password sent over plain http to a remote host", () => {
    expect(
      isInsecureCredentialed({
        kind: "http",
        url: "http://box.example/RPC2",
        username: "alice",
      }),
    ).toBe(true);
  });

  it("is quiet when there is nothing to leak or the channel is safe", () => {
    // https protects it.
    expect(
      isInsecureCredentialed({
        kind: "http",
        url: "https://box.example/RPC2",
        username: "alice",
      }),
    ).toBe(false);
    // No credential at all.
    expect(
      isInsecureCredentialed({
        kind: "http",
        url: "http://box.example/RPC2",
        username: "",
      }),
    ).toBe(false);
    // Never leaves the machine.
    expect(
      isInsecureCredentialed({
        kind: "http",
        url: "http://127.0.0.1:8099/RPC2",
        username: "alice",
      }),
    ).toBe(false);
    // Not an HTTP transport at all.
    expect(isInsecureCredentialed({ kind: "unixSocket", path: "/x" })).toBe(
      false,
    );
    expect(isInsecureCredentialed(undefined)).toBe(false);
  });
});
