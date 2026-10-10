#!/usr/bin/env python3
"""Offline unit tests for xsos server release tooling."""

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
    def test_read_only_release_and_current_links_are_cleaned_without_following_links(self):
        with tempfile.TemporaryDirectory() as temporary:
            parent = Path(temporary)
            outside = parent / "outside"
            outside.mkdir()
            protected = outside / "protected"
            protected.write_text("preserve")
            protected.chmod(0o444)
            outside.chmod(0o555)
            stage = parent / "stage"
            stage.mkdir()
            physical = stage / "release"
            physical.mkdir()
            resource = physical / "asset"
            resource.write_text("asset")
            resource.chmod(0o444)
            physical.chmod(0o555)
            (stage / "current").symlink_to(physical)
            (stage / "outside-link").symlink_to(outside)
            (stage / "file-link").symlink_to(protected)
            (stage / "missing-link").symlink_to(parent / "missing")
            PACKAGE.chmod_tree_for_cleanup(stage)
            self.assertEqual(stat.S_IMODE(resource.stat().st_mode), 0o600)
            self.assertEqual(stat.S_IMODE(physical.stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(outside.stat().st_mode), 0o555)
            self.assertEqual(stat.S_IMODE(protected.stat().st_mode), 0o444)
            self.assertTrue((stage / "current").is_symlink())
            outside.chmod(0o700)

    def test_manifest_writer_accepts_current_schema_and_rejects_old_schema(self) -> None:
        script = SCRIPT.with_name("write-server-release-manifest.py")
        spec = importlib.util.spec_from_file_location("manifest_writer", script)
        assert spec is not None and spec.loader is not None
        writer = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(writer)
        identity = json.loads((SCRIPT.parent.parent / "release.json").read_text())
        identity["source_revision"] = "a" * 40
        identity["web_assets_sha256"] = "b" * 64
        for revision, accepted in [(1, True), (0, False)]:
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
                response.getheader.return_value = "xsos"
                response.status = status
                response.read.return_value = body
                with patch.object(PACKAGE.urllib.request, "urlopen", return_value=response) as request:
                    self.assertEqual(PACKAGE.server_is_ready(18104), expected)
                    request.assert_called_once_with("http://127.0.0.1:18104/readyz", timeout=1)

    def test_readiness_rejects_another_service_on_the_port(self) -> None:
        response = MagicMock()
        response.__enter__.return_value = response
        response.status = 200
        response.read.return_value = b'{"ready":true}'
        for identity in [None, "other-service"]:
            response.getheader.return_value = identity
            with patch.object(PACKAGE.urllib.request, "urlopen", return_value=response):
                self.assertFalse(PACKAGE.server_is_ready(18104))

    def test_readiness_retries_connection_errors_and_timeouts(self) -> None:
        for error in [PACKAGE.urllib.error.URLError("not started"), TimeoutError()]:
            with self.subTest(error=error):
                with patch.object(PACKAGE.urllib.request, "urlopen", side_effect=error):
                    self.assertFalse(PACKAGE.server_is_ready(18104))

    def test_release_readme_is_package_local_and_chinese(self) -> None:
        repository = SCRIPT.parent.parent
        readme = repository / PACKAGE.RELEASE_README
        text = readme.read_text(encoding="utf-8")
        for required in (
            "## 安装与初始化", "## HTTPS 入口", "## 运行检查",
            "## 安装布局", "## 遇到问题", "sha256sum --check --strict",
            "verify-release --root", "groupadd --system xsos",
            "EnvironmentFile=/etc/isarmg/xsos.env", "bin/xsos init",
            "systemctl enable --now xsos.service", "/readyz", "sudoedit",
        ):
            self.assertIn(required, text)
        for destination in re.findall(r"\]\(([^)]+)\)", text):
            self.assertTrue(
                destination.startswith(("https://", "#")),
                f"packaged README link must work without the source tree: {destination}",
            )
        self.assertIn("xsos 1.0.0 发行包部署手册", text)
        self.assertIn("bin/xsos", text)
        self.assertIn("systemd/xsos.service", text)
        self.assertIn("RELEASE-MANIFEST.json", text)
        identity_rows = {
            cells[0]: cells[1]
            for line in text.splitlines()
            if line.startswith("|")
            and len(cells := [cell.strip().strip("`") for cell in line.strip("|").split("|")]) == 2
        }
        self.assertEqual(identity_rows.get("schema_revision"), "1")
        self.assertIn(
            "XSOC_AUTHORIZATION_KEY=REPLACE_WITH_BASE64_ENCODED_32_RANDOM_BYTES",
            text,
        )
        self.assertNotIn("XSOS_SESSION_IDLE_TTL_SECONDS", text)
        self.assertNotIn("XSOS_SESSION_ABSOLUTE_TTL_SECONDS", text)
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

    def test_server_build_declares_supported_target_in_shared_pipeline(self) -> None:
        config = json.loads((SCRIPT.parent.parent / "xcss-web-build.json").read_text())
        self.assertEqual(config["rust"]["package"], "xsos")
        self.assertEqual(config["rust"]["binary"], "xsos")
        self.assertEqual(config["rust"]["source_revision_env"], "XSOS_SOURCE_REVISION")
        self.assertEqual(PACKAGE.built_server_path(Path("/tmp/release-target")), Path("/tmp/release-target/x86_64-unknown-linux-gnu/release/xsos"))

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
