#!/usr/bin/env python3
"""Dev-only HTTP -> SCGI bridge, standing in for an nginx `scgi_pass` front end.

Lets you exercise rstorrent's remote HTTP(S) transport (B9) against a local
rtorrent without owning a seedbox: it accepts XML-RPC over HTTP with Basic auth
and forwards the body to rtorrent's SCGI socket.

    python3 tools/scgi-http-bridge.py [--socket PATH] [--port N] [--user U] [--password P]

Then point Preferences -> Connection at http://127.0.0.1:8099/RPC2.

NOT for production: no TLS, single-threaded, credentials passed on the command
line. It exists so the transport can be tested end-to-end.
"""

import argparse
import base64
import http.server
import re
import socket


def scgi_call(sock_path: str, body: bytes) -> bytes:
    """Frame an XML-RPC body as an SCGI netstring request and read the reply."""
    headers = b"CONTENT_LENGTH\0" + str(len(body)).encode() + b"\0SCGI\0001\0"
    frame = str(len(headers)).encode() + b":" + headers + b"," + body

    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(sock_path)
    try:
        s.sendall(frame)
        chunks = []
        while True:
            chunk = s.recv(65536)
            if not chunk:
                break
            chunks.append(chunk)
    finally:
        s.close()

    raw = b"".join(chunks)
    # Strip the CGI headers rtorrent prefixes to the XML body.
    for sep in (b"\r\n\r\n", b"\n\n"):
        i = raw.find(sep)
        if i != -1:
            return raw[i + len(sep):]
    return raw


def make_handler(args):
    expected = "Basic " + base64.b64encode(
        f"{args.user}:{args.password}".encode()
    ).decode()

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass  # quiet; we log the RPC method instead

        def do_POST(self):
            if args.user and self.headers.get("Authorization") != expected:
                self.send_response(401)
                self.send_header("WWW-Authenticate", 'Basic realm="rtorrent"')
                self.end_headers()
                self.wfile.write(b"unauthorized")
                return
            if self.path != args.path:
                self.send_response(404)
                self.end_headers()
                self.wfile.write(b"not found")
                return

            body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
            method = re.search(rb"<methodName>([^<]+)</methodName>", body)
            print("call", method.group(1).decode() if method else "?", flush=True)

            out = scgi_call(args.socket, body)
            self.send_response(200)
            self.send_header("Content-Type", "text/xml")
            self.send_header("Content-Length", str(len(out)))
            self.end_headers()
            self.wfile.write(out)

    return Handler


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--socket", default="~/.rtorrent/rpc.socket")
    p.add_argument("--port", type=int, default=8099)
    p.add_argument("--path", default="/RPC2")
    p.add_argument("--user", default="alice", help='empty string disables auth')
    p.add_argument("--password", default="hunter2")
    args = p.parse_args()

    import os
    args.socket = os.path.expanduser(args.socket)

    print(f"bridge: http://127.0.0.1:{args.port}{args.path} -> {args.socket}")
    print(f"auth: {'basic as ' + args.user if args.user else 'disabled'}")
    http.server.HTTPServer(("127.0.0.1", args.port), make_handler(args)).serve_forever()


if __name__ == "__main__":
    main()
