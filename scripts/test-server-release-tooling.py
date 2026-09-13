#!/usr/bin/env python3
"""Offline unit tests for Host Monitoring server release tooling."""

from __future__ import annotations

import importlib.util
import json
import re
import stat
import tempfile
import unittest
from unittest.mock import MagicMock, patch
from pathlib import Path


SCRIPT = Path(__file__).with_name("package-server-release.py")
SPEC = importlib.util.spec_from_file_location("package_server_release", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("could not load package-server-release.py")
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)


class ReleaseToolingTests(unittest.TestCase):
    def test_manifest_writer_accepts_current_schema_and_rejects_old_schema(self) -> None:
        script = SCRIPT.with_name("write-server-release-manifest.py")
        spec = importlib.util.spec_from_file_location("manifest_writer", script)
        assert spec is not None and spec.loader is not None
        writer = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(writer)
        identity = json.loads((SCRIPT.parent.parent / "host-monitoring-server/release.json").read_text())
        identity["source_revision"] = "a" * 40
        for revision, accepted in [(5, True), (4, False)]:
            identity["schema_revision"] = revision
            result = MagicMock(returncode=0, stderr=b"", stdout=(json.dumps(identity) + "\n").encode())
            with patch.object(writer.subprocess, "run", return_value=result):
                if accepted:
                    self.assertEqual(writer.read_identity(Path("/unused/binary"))[0], identity)
                else:
                    with self.assertRaises(SystemExit):
                        writer.read_identity(Path("/unused/binary"))

    def test_readiness_requires_current_endpoint_and_exact_ready_response(self) -> None:
        for status, body, expected in [
            (200, b'{"ready":true}', True),
            (200, b'{"ready":false}', False),
            (200, b'<html>SPA fallback</html>', False),
            (204, b'', False),
            (503, b'{"ready":true}', False),
        ]:
            with self.subTest(status=status, body=body):
                response = MagicMock()
                response.__enter__.return_value = response
                response.status = status
                response.read.return_value = body
                with patch.object(PACKAGE.urllib.request, "urlopen", return_value=response) as request:
                    self.assertEqual(PACKAGE.server_is_ready(18104), expected)
                    request.assert_called_once_with("http://127.0.0.1:18104/readyz", timeout=1)

    def test_readiness_retries_connection_errors_and_timeouts(self) -> None:
        for error in [PACKAGE.urllib.error.URLError("not started"), TimeoutError()]:
            with self.subTest(error=error):
                with patch.object(PACKAGE.urllib.request, "urlopen", side_effect=error):
                    self.assertFalse(PACKAGE.server_is_ready(18104))

    def test_release_readme_is_package_local_and_chinese(self) -> None:
        repository = SCRIPT.parent.parent
        readme = repository / PACKAGE.RELEASE_README
        text = readme.read_text(encoding="utf-8")
        self.assertGreater(len(text.encode("utf-8")), 10_000)
        self.assertIn("Host Monitoring Server 0.9.11 发行包部署手册", text)
        self.assertIn("bin/host-monitoring-server", text)
        self.assertIn("systemd/host-monitoring-server.service", text)
        self.assertIn("RELEASE-MANIFEST.json", text)
        source_only = re.compile(r"(?:^|[`\s])(scripts|clients|config|deploy)/")
        self.assertIsNone(
            source_only.search(text),
            "packaged README must not reference source-only repository paths",
        )

    def test_release_host_is_exactly_linux_x86_64(self) -> None:
        PACKAGE.require_release_host("Linux", "x86_64", "glibc 2.41")
        for system, machine, libc in [
            ("Linux", "aarch64", "glibc 2.41"),
            ("Linux", "x86_64", "musl 1.2.5"),
            ("Darwin", "x86_64", None),
            ("Windows", "AMD64", None),
        ]:
            with self.subTest(system=system, machine=machine, libc=libc):
                with self.assertRaisesRegex(
                    SystemExit, "require an x86_64 GNU/Linux build host"
                ):
                    PACKAGE.require_release_host(system, machine, libc)

    def test_server_build_uses_the_only_supported_target(self) -> None:
        self.assertEqual(
            PACKAGE.server_build_command(),
            [
                "cargo",
                "build",
                "--locked",
                "--release",
                "-p",
                "host-monitoring-server",
                "--target",
                "x86_64-unknown-linux-gnu",
            ],
        )
        target_directory = Path("/tmp/release-target")
        self.assertEqual(
            PACKAGE.built_server_path(target_directory),
            target_directory
            / "x86_64-unknown-linux-gnu/release/host-monitoring-server",
        )

    def test_copy_exclusive_creates_read_only_content(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            destination = root / "destination"
            source.write_bytes(b"current archive")
            PACKAGE.copy_exclusive(source, destination)
            self.assertEqual(destination.read_bytes(), b"current archive")
            self.assertEqual(stat.S_IMODE(destination.stat().st_mode), 0o444)
            self.assertEqual(destination.stat().st_nlink, 1)

    def test_copy_exclusive_never_overwrites_or_unlinks_existing_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            destination = root / "destination"
            source.write_bytes(b"new archive")
            destination.write_bytes(b"published archive")
            with self.assertRaises(FileExistsError):
                PACKAGE.copy_exclusive(source, destination)
            self.assertEqual(destination.read_bytes(), b"published archive")


if __name__ == "__main__":
    unittest.main()
