#!/usr/bin/env python3
"""Build and publish one immutable, source-bound Host Monitoring archive."""

from __future__ import annotations

import hashlib
import os
import platform
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import NoReturn


APPLICATION = "host-monitoring-server"
VERSION = "0.9.11"
TARGET = "x86_64-unknown-linux-gnu"
TAG = f"v{VERSION}"
RELEASE_README = Path("docs/server-release-readme.md")


def fail(message: str) -> NoReturn:
    raise SystemExit(f"package server release: {message}")


def require_release_host(
    system: str | None = None,
    machine: str | None = None,
    gnu_libc: str | None = None,
) -> None:
    system = platform.system() if system is None else system
    machine = platform.machine() if machine is None else machine
    if gnu_libc is None:
        try:
            gnu_libc = os.confstr("CS_GNU_LIBC_VERSION")
        except (AttributeError, OSError, ValueError):
            gnu_libc = None
    if (
        system != "Linux"
        or machine != "x86_64"
        or gnu_libc is None
        or not gnu_libc.startswith("glibc ")
    ):
        fail(
            f"official Server releases require an x86_64 GNU/Linux build host; "
            f"detected system={system!r}, machine={machine!r}, libc={gnu_libc!r}"
        )


def server_build_command() -> list[str]:
    return [
        "cargo",
        "build",
        "--locked",
        "--release",
        "-p",
        APPLICATION,
        "--target",
        TARGET,
    ]


def built_server_path(cargo_target: Path) -> Path:
    return cargo_target / TARGET / "release" / APPLICATION


def run(
    arguments: list[str],
    *,
    cwd: Path,
    env: dict[str, str] | None = None,
    capture: bool = False,
) -> subprocess.CompletedProcess[bytes]:
    result = subprocess.run(
        arguments,
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
        check=False,
    )
    if result.returncode != 0:
        if capture:
            sys.stderr.buffer.write(result.stdout)
            sys.stderr.buffer.write(result.stderr)
        fail(f"command failed ({result.returncode}): {' '.join(arguments)}")
    return result


def git_output(source: Path, *arguments: str) -> str:
    result = run(["git", *arguments], cwd=source, capture=True)
    if result.stderr:
        sys.stderr.buffer.write(result.stderr)
    try:
        return result.stdout.decode("utf-8").strip()
    except UnicodeDecodeError as error:
        fail(f"Git output is not UTF-8: {error}")


def require_clean_source(source: Path) -> None:
    status = git_output(source, "status", "--porcelain=v1", "--untracked-files=all")
    if status:
        fail("source tree must be completely clean before and after the build")


def chmod_tree_for_cleanup(root: Path) -> None:
    if not root.exists():
        return
    for directory, child_directories, files in os.walk(root, topdown=False):
        for name in files:
            try:
                os.chmod(Path(directory) / name, 0o600, follow_symlinks=False)
            except FileNotFoundError:
                pass
        for name in child_directories:
            try:
                os.chmod(Path(directory) / name, 0o700, follow_symlinks=False)
            except FileNotFoundError:
                pass
    os.chmod(root, 0o700, follow_symlinks=False)


def copy_exclusive(source: Path, destination: Path) -> None:
    descriptor = os.open(
        destination,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
        0o444,
    )
    try:
        os.fchmod(descriptor, 0o444)
        with source.open("rb") as input_file, os.fdopen(
            descriptor, "wb", closefd=True
        ) as output_file:
            shutil.copyfileobj(input_file, output_file, length=1024 * 1024)
            output_file.flush()
            os.fsync(output_file.fileno())
    except BaseException:
        destination.unlink(missing_ok=True)
        raise


def expect_rejection(
    arguments: list[str],
    marker: bytes,
    *,
    cwd: Path,
    env: dict[str, str] | None = None,
) -> None:
    result = subprocess.run(
        arguments,
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode == 0 or marker not in result.stderr:
        sys.stderr.buffer.write(result.stdout)
        sys.stderr.buffer.write(result.stderr)
        fail(f"negative release check was not rejected: {' '.join(arguments)}")


def verify_bound_command_boundary(root: Path, temporary: Path) -> None:
    binary = root / "bin/host-monitoring-server"
    expect_rejection(
        [os.fspath(binary), "serve"],
        b"source-bound release binaries must use serve-release",
        cwd=Path("/"),
    )

    other_web = temporary / "wrong-static-web"
    shutil.copytree(root / "web", other_web)
    database = temporary / "wrong-static.db"
    environment = os.environ.copy()
    environment.update(
        {
            "HOST_MONITORING_DATABASE_URL": f"sqlite://{database}",
            "HOST_MONITORING_CLIENT_AUTHORIZATION_KEY": "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=",
            "HOST_MONITORING_STATIC_DIR": os.fspath(other_web),
            "HOST_MONITORING_BIND": "127.0.0.1:0",
            "HOST_MONITORING_DEVELOPMENT": "true",
        }
    )
    expect_rejection(
        [
            os.fspath(binary),
            "serve-release",
            "--root",
            os.fspath(root),
        ],
        b"must equal the verified release web directory",
        cwd=Path("/"),
        env=environment,
    )
    if database.exists():
        fail("wrong static directory rejection opened the application database")
    outside = temporary / "host-monitoring-server-outside-tree"
    shutil.copyfile(binary, outside)
    outside.chmod(0o555)
    expect_rejection(
        [os.fspath(outside), "verify-release", "--root", os.fspath(root)],
        b"must be verified by its own Host Monitoring server binary",
        cwd=Path("/"),
    )
    expect_rejection(
        [
            os.fspath(binary),
            "verify-release",
            "--root",
            os.fspath(root / ".." / VERSION),
        ],
        b"normalized absolute path",
        cwd=Path("/"),
    )


def server_is_ready(port: int) -> bool:
    try:
        with urllib.request.urlopen(
            f"http://127.0.0.1:{port}/readyz", timeout=1
        ) as response:
            return response.status == 200 and response.read(128) == b'{"ready":true}'
    except (urllib.error.URLError, TimeoutError):
        return False


def relocated_smoke(extracted: Path, temporary: Path) -> None:
    state = temporary / "smoke-state/db"
    state.mkdir(parents=True)
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    environment = os.environ.copy()
    environment.update(
        {
            "HOST_MONITORING_DATABASE_URL": f"sqlite://{state / 'app.db'}",
            "HOST_MONITORING_CLIENT_AUTHORIZATION_KEY": "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=",
            "HOST_MONITORING_STATIC_DIR": os.fspath(extracted / "web"),
            "HOST_MONITORING_BIND": f"127.0.0.1:{port}",
            "HOST_MONITORING_DEVELOPMENT": "true",
            "HOST_MONITORING_BOOTSTRAP_ADMIN_USERNAME": "release-smoke",
            "HOST_MONITORING_BOOTSTRAP_ADMIN_PASSWORD": "release-smoke-password",
        }
    )
    log_path = temporary / "relocated-smoke.log"
    with log_path.open("wb") as log:
        process = subprocess.Popen(
            [
                os.fspath(extracted / "bin/host-monitoring-server"),
                "serve-release",
                "--root",
                os.fspath(extracted),
            ],
            cwd=Path("/"),
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        try:
            ready = False
            for _ in range(180):
                if process.poll() is not None:
                    break
                ready = server_is_ready(port)
                if ready:
                    break
                time.sleep(0.1)
            if not ready:
                log.flush()
                sys.stderr.buffer.write(log_path.read_bytes())
                fail("relocated release did not become live")
            with urllib.request.urlopen(
                f"http://127.0.0.1:{port}/", timeout=2
            ) as response:
                index = response.read()
            assets = re.findall(rb"/assets/[A-Za-z0-9._-]+\.js", index)
            if not assets:
                fail("relocated release index has no compiled JavaScript asset")
            with urllib.request.urlopen(
                f"http://127.0.0.1:{port}{assets[0].decode('ascii')}", timeout=2
            ) as response:
                if response.status != 200 or not response.read(1):
                    fail("relocated release asset could not be served")
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)

    asset_path = extracted / "web" / assets[0].decode("ascii").removeprefix("/")
    asset_path.chmod(0o644)
    with asset_path.open("ab") as asset:
        asset.write(b"\ntampered\n")
    asset_path.chmod(0o444)
    rejection = subprocess.run(
        [
            os.fspath(extracted / "bin/host-monitoring-server"),
            "verify-release",
            "--root",
            os.fspath(extracted),
        ],
        cwd=Path("/"),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if rejection.returncode == 0 or not any(
        marker in rejection.stderr for marker in [b"digest mismatch", b"size mismatch"]
    ):
        fail("release verifier did not reject a tampered compiled asset")


def main() -> None:
    if len(sys.argv) != 2:
        fail("usage: package-server-release.py /absolute/output-directory")
    require_release_host()
    source = Path(__file__).resolve(strict=True).parent.parent
    output_directory = Path(sys.argv[1])
    if not output_directory.is_absolute():
        fail("output directory must be absolute")
    if output_directory.resolve(strict=True) != output_directory or not output_directory.is_dir():
        fail("output directory must be an existing real directory")
    if source == output_directory or source in output_directory.parents:
        fail("output directory must be outside the source tree")

    for command in ["cargo", "git", "npm", "python3", "tar"]:
        if shutil.which(command) is None:
            fail(f"required command is missing: {command}")
    require_clean_source(source)
    revision = git_output(source, "rev-parse", "--verify", "HEAD")
    if len(revision) != 40 or any(character not in "0123456789abcdef" for character in revision):
        fail("HEAD is not a full lowercase 40-hex Git commit")
    if git_output(source, "cat-file", "-t", f"refs/tags/{TAG}") != "tag":
        fail(f"official packaging requires the annotated tag {TAG}")
    if git_output(source, "rev-parse", f"refs/tags/{TAG}^{{commit}}") != revision:
        fail(f"annotated tag {TAG} does not identify HEAD")
    source_epoch_text = git_output(source, "show", "-s", "--format=%ct", revision)
    if not source_epoch_text.isdigit():
        fail("source commit timestamp is invalid")

    archive_name = f"{APPLICATION}-{VERSION}-{TARGET}.tar.gz"
    archive_output = output_directory / archive_name
    checksum_output = output_directory / f"{archive_name}.sha256"
    if archive_output.exists() or archive_output.is_symlink():
        fail(f"refusing to replace existing archive: {archive_output}")
    if checksum_output.exists() or checksum_output.is_symlink():
        fail(f"refusing to replace existing checksum: {checksum_output}")

    temporary = Path(tempfile.mkdtemp(prefix="host-monitoring-server-release-"))
    published_archive = False
    published_checksum = False
    try:
        releases = temporary / "releases"
        root = releases / VERSION
        web_stage = temporary / "web"
        for directory in [root / "bin", root / "systemd", web_stage]:
            directory.mkdir(parents=True, exist_ok=False)

        run(["npm", "ci"], cwd=source / "clients/web")
        run(
            ["npm", "run", "build", "--", "--outDir", os.fspath(web_stage), "--emptyOutDir"],
            cwd=source / "clients/web",
        )
        if sorted(path.name for path in web_stage.iterdir()) != ["assets", "index.html"]:
            fail("Web build is not the exact current assets/index.html layout")

        cargo_target = Path(
            os.environ.get("HOST_MONITORING_RELEASE_CARGO_TARGET_DIR", source / "target")
        )
        if not cargo_target.is_absolute():
            fail("HOST_MONITORING_RELEASE_CARGO_TARGET_DIR must be absolute")
        build_environment = os.environ.copy()
        build_environment.update(
            {
                "CARGO_TARGET_DIR": os.fspath(cargo_target),
                "CARGO_INCREMENTAL": "0",
                "HOST_MONITORING_SOURCE_REVISION": revision,
            }
        )
        run(server_build_command(), cwd=source, env=build_environment)
        built_binary = built_server_path(cargo_target)
        if not built_binary.is_file() or built_binary.is_symlink():
            fail("Cargo did not produce the expected server release binary")

        shutil.copyfile(built_binary, root / "bin/host-monitoring-server")
        shutil.copyfile(
            source / "deploy/host-monitoring-server.service",
            root / "systemd/host-monitoring-server.service",
        )
        shutil.copyfile(source / RELEASE_README, root / "README.md")
        shutil.copytree(web_stage, root / "web", copy_function=shutil.copyfile)

        executable_paths = [root / "bin/host-monitoring-server"]
        for path in executable_paths:
            path.chmod(0o555)
        for path in root.rglob("*"):
            if path.is_symlink():
                fail(f"release staging contains a symbolic link: {path}")
            if path.is_dir():
                path.chmod(0o555)
            elif path not in executable_paths:
                path.chmod(0o444)

        run(
            [
                "python3",
                os.fspath(source / "scripts/write-server-release-manifest.py"),
                os.fspath(root),
            ],
            cwd=source,
        )
        root.chmod(0o555)
        run(
            [
                os.fspath(root / "bin/host-monitoring-server"),
                "verify-release",
                "--root",
                os.fspath(root),
            ],
            cwd=Path("/"),
        )
        verify_bound_command_boundary(root, temporary)
        require_clean_source(source)

        archive = temporary / archive_name
        archive_members = temporary / "archive-members"
        files: list[Path] = []
        directories = [Path(VERSION)]
        for path in root.rglob("*"):
            member = Path(VERSION) / path.relative_to(root)
            if path.is_dir():
                directories.append(member)
            else:
                files.append(member)
        files.sort(key=lambda path: path.as_posix())
        directories.sort(key=lambda path: (-len(path.parts), path.as_posix()))
        archive_members.write_bytes(
            b"\0".join(os.fspath(path).encode("ascii") for path in [*files, *directories])
            + b"\0"
        )
        run(
            [
                "tar",
                "--create",
                "--gzip",
                "--file",
                os.fspath(archive),
                "--directory",
                os.fspath(releases),
                "--sort=name",
                f"--mtime=@{source_epoch_text}",
                "--owner=0",
                "--group=0",
                "--numeric-owner",
                "--format=posix",
                "--pax-option=delete=atime,delete=ctime",
                "--no-recursion",
                "--null",
                "--files-from",
                os.fspath(archive_members),
            ],
            cwd=source,
        )

        extracted_releases = temporary / "extracted/releases"
        extracted_releases.mkdir(parents=True)
        run(
            [
                "tar",
                "--extract",
                "--gzip",
                "--file",
                os.fspath(archive),
                "--directory",
                os.fspath(extracted_releases),
                "--no-same-owner",
                "--same-permissions",
                "--delay-directory-restore",
            ],
            cwd=source,
        )
        extracted = extracted_releases / VERSION
        if sorted(path.name for path in extracted_releases.iterdir()) != [VERSION]:
            fail("release archive has an unexpected top-level layout")
        run(
            [
                os.fspath(extracted / "bin/host-monitoring-server"),
                "verify-release",
                "--root",
                os.fspath(extracted),
            ],
            cwd=Path("/"),
        )
        relocated_smoke(extracted, temporary)

        require_clean_source(source)
        if git_output(source, "rev-parse", "--verify", "HEAD") != revision:
            fail("source HEAD changed during release construction")
        if git_output(source, "rev-parse", f"refs/tags/{TAG}^{{commit}}") != revision:
            fail(f"annotated tag {TAG} changed during release construction")

        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        checksum = temporary / f"{archive_name}.sha256"
        checksum.write_text(f"{digest}  {archive_name}\n", encoding="ascii")
        checksum.chmod(0o444)
        copy_exclusive(archive, archive_output)
        published_archive = True
        try:
            if hashlib.sha256(archive_output.read_bytes()).hexdigest() != digest:
                fail("published archive digest changed during exclusive copy")
            copy_exclusive(checksum, checksum_output)
            published_checksum = True
            if checksum_output.read_bytes() != checksum.read_bytes():
                fail("published checksum changed during exclusive copy")
            directory_descriptor = os.open(
                output_directory, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0)
            )
            try:
                os.fsync(directory_descriptor)
            finally:
                os.close(directory_descriptor)
        except BaseException:
            if published_checksum:
                checksum_output.unlink(missing_ok=True)
                published_checksum = False
            archive_output.unlink(missing_ok=True)
            published_archive = False
            try:
                directory_descriptor = os.open(
                    output_directory, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0)
                )
                try:
                    os.fsync(directory_descriptor)
                finally:
                    os.close(directory_descriptor)
            except OSError:
                pass
            raise
        print(archive_output)
        print(checksum_output)
    finally:
        if published_archive and not published_checksum:
            archive_output.unlink(missing_ok=True)
        chmod_tree_for_cleanup(temporary)
        shutil.rmtree(temporary)


if __name__ == "__main__":
    main()
