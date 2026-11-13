#!/usr/bin/env python3
"""Exercise a copied standalone Host: factory copies, atomic reset, history and restart."""
import http.cookiejar
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import urllib.request


def run():
    source = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="devdesk-standalone-templates-") as directory:
        root = Path(directory)
        binary = root / "devdesk-host"
        shutil.copy2(source, binary)
        environment = dict(os.environ)
        environment.pop("DEVDESK_ACCESS_TOKEN", None)

        def start():
            child = subprocess.Popen(
                [str(binary), "--data", str(root / "dataset"), "--port", "0"],
                cwd=root, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            )
            child.stdout.readline()
            url = child.stdout.readline().strip()
            assert url.startswith("http://127.0.0.1:"), child.stderr.read()
            client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

            def post(path, data):
                return json.load(client.open(urllib.request.Request(
                    url + path, json.dumps(data).encode(), {"Content-Type": "application/json"}
                ), timeout=30))

            post("/session", {})

            def rpc(method, params=None):
                response = post("/rpc", {"jsonrpc": "2.0", "id": 1, "method": method, "params": params or {}})
                assert "error" not in response, response
                return response["result"]
            return child, rpc

        child, rpc = start()
        try:
            defaults = rpc("query.configuration_defaults")
            delivery_count = rpc("query.configurations", {"kind": "delivery_type", "limit": 100})["total"]
            rpc("settings.update", {"global_concurrency": 3, "default_harness": "dsh"})
            custom = rpc("delivery_type.create", {"command_id": "custom-delivery", "payload": {"spec_refs":["builtin-spec-stage-prd:1"],
                "name": "Standalone custom delivery", "data": {"format":"markdown","body":"database copy"}
            }})
            preview = rpc("query.template_reset_preview")
            command = {"command_id": "reset-standalone", "payload": {
                "expected_catalog_generation": preview["generation"],
                "catalog_fingerprint": preview["catalog_fingerprint"], "bundle_hash": preview["bundle_hash"],
            }}
            result = rpc("configuration.templates.reset", command)
            assert result["generation"] == 2
            assert rpc("configuration.templates.reset", command) == result
            current = rpc("query.configurations", {"kind": "delivery_type", "limit": 100})
            assert current["total"] == delivery_count
            assert not any(record["id"] == custom["id"] for record in current["records"])
            history = rpc("query.configurations", {"kind": "delivery_type", "include_archived": True, "include_history": True, "limit": 100})
            assert custom in history["records"]
            restored = rpc("query.configuration_defaults")
            assert restored["model_ref"] == defaults["model_ref"]
            state = rpc("query.workspace")
            assert state["global_concurrency"] == 3 and state["default_harness"] == "dsh"
        finally:
            child.terminate()
            child.wait(timeout=15)
        child, rpc = start()
        try:
            assert rpc("query.template_catalog")["generation"] == 2
            assert rpc("query.configuration_defaults")["workflow_ref"] == restored["workflow_ref"]
            assert rpc("query.configurations", {"kind": "delivery_type"})["total"] == delivery_count
            preview = rpc("query.template_reset_preview")
            result = rpc("configuration.templates.reset", {"command_id": "repeat-standalone", "payload": {
                "expected_catalog_generation": preview["generation"],
                "catalog_fingerprint": preview["catalog_fingerprint"], "bundle_hash": preview["bundle_hash"],
            }})
            assert result["changed"] is False and result["generation"] == 2
        finally:
            child.terminate()
            child.wait(timeout=15)
    print("PASS standalone templates: database copies, reset, immutable history, replay, preferences, restart, no-op")


if __name__ == "__main__":
    run()
