"""
A stand-in for GitHub's releases API, serving a real portable archive.

`tests-host` runs the real binary, and the one thing it cannot reach is GitHub: the
app bundles Mozilla's roots rather than the system store, so it will not go through a
TLS-inspecting proxy, and a container without direct egress can never answer an update
check. That left the update path — check, download, verify, stage — as the only major
flow with no end-to-end coverage, and it is the one with two reports against it.

So this serves the two things the updater asks for, and the archive it serves is a
real zip with the shape `stage_zip` requires: everything under a single top-level
folder, with `aurora-app` inside it. The digest is computed from the bytes actually
served, so a verification failure here means the verification is wrong rather than the
fixture.
"""
import hashlib
import http.server
import io
import json
import os
import threading
import zipfile

REPO = "noahklimczuk/iptv-player"


# What `aurora_ingest::selfupdate::APP_EXE` is on this platform. `stage_zip` uses the
# name to decide the archive really is Aurora, so an archive without it is refused.
APP_EXE = "aurora-app.exe" if os.name == "nt" else "aurora-app"


def portable_zip(version):
    """An archive shaped the way CI builds one: one top-level folder, app inside."""
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        root = "Aurora TV/"
        # Random bytes so the digest differs per run, which is the point of checking it.
        z.writestr(root + APP_EXE, b"#!/bin/true\n" + os.urandom(4096))
        z.writestr(root + "THIRD-PARTY-NOTICES.md", "# Notices\n")
        z.writestr(root + "README.txt", f"Aurora TV {version}\n")
    return buf.getvalue()


class FakeGitHub:
    """Serves one release. `url` is what `AURORA_UPDATE_API` should be set to."""

    def __init__(self, version="99.0.0", port=8100):
        self.version = version
        self.asset = portable_zip(version)
        self.digest = hashlib.sha256(self.asset).hexdigest()
        self.port = port
        # What the app actually asked for, so a scenario can tell "never requested"
        # apart from "requested and failed".
        self.hits = []
        self._server = None
        self._thread = None

    @property
    def url(self):
        return f"http://127.0.0.1:{self.port}"

    def _release_json(self):
        name = f"Aurora-TV-{self.version}-portable.zip"
        return {
            "tag_name": f"v{self.version}",
            "body": "A release that exists only for tests-host.",
            "html_url": f"{self.url}/releases/tag/v{self.version}",
            "published_at": "2026-09-26T00:00:00Z",
            "assets": [{
                "name": name,
                "browser_download_url": f"{self.url}/download/{name}",
                "size": len(self.asset),
                "digest": f"sha256:{self.digest}",
            }],
        }

    def start(self):
        outer = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *a):
                pass

            def do_GET(self):
                outer.hits.append(self.path)
                if self.path == f"/repos/{REPO}/releases/latest":
                    body = json.dumps(outer._release_json()).encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                elif self.path.startswith("/download/"):
                    self.send_response(200)
                    self.send_header("Content-Type", "application/zip")
                    self.send_header("Content-Length", str(len(outer.asset)))
                    self.end_headers()
                    self.wfile.write(outer.asset)
                else:
                    self.send_error(404)

        self._server = http.server.ThreadingHTTPServer(("127.0.0.1", self.port), Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()
        return self

    def stop(self):
        if self._server:
            self._server.shutdown()
            self._server.server_close()
