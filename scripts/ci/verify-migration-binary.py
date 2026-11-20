#!/usr/bin/env python3
"""Verify the linked Chimera desktop migration entry using disposable roots.

Usage from the repository root:
    python3 scripts/ci/verify-migration-binary.py backend/target/debug/chimera

This never starts the Tauri GUI, touches the current user's config, or restores
into an existing directory. No dependencies beyond the Python standard library.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
LEGACY = ROOT / "ref/backend/nyanpasu-migration/src/fixtures/v1_6_1/nyanpasu-config.yaml"
CHILD_FLAG = "--chimera-internal-migrate"


def child(binary: Path, config: Path, data: Path, result: Path):
    return subprocess.run(
        [str(binary), CHILD_FLAG, str(config), str(data), str(result)],
        cwd=ROOT,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=60,
        check=False,
    )


def outcome(binary: Path, config: Path, data: Path, result: Path):
    process = child(binary, config, data, result)
    assert result.is_file(), (
        f"child did not write {result}: exit={process.returncode}, "
        f"stderr={process.stderr[:2000]!r}"
    )
    report = json.loads(result.read_bytes())
    assert report["protocol_version"] == 1, report
    return process.returncode, report["outcome"]


def paths(root: Path):
    config, data = root / "config", root / "data"
    config.mkdir(parents=True)
    data.mkdir()
    return config, data


def verify_snapshot(snapshot: Path):
    manifest = json.loads((snapshot / "manifest.json").read_bytes())
    assert manifest["kind"] == "migration"
    assert manifest["format_version"] == 1
    assert "config" in manifest["directories"]
    for entry in manifest["files"]:
        name = entry["path"]
        assert name.startswith(("config/", "data/")) and ".." not in name
        data = (snapshot / name).read_bytes()
        assert entry["size"] == len(data), name
        assert hashlib.sha256(data).hexdigest() == entry["sha256"], name
    return len(manifest["files"])


def verify(binary: Path):
    with tempfile.TemporaryDirectory(prefix="chimera-real-migration-child-") as workspace:
        root = Path(workspace)

        # New installation: the migration state is stable across restarts,
        # and the child never creates a pointless backup.
        config, data = paths(root / "fresh")
        code, first = outcome(binary, config, data, root / "fresh-first.json")
        assert code == 0 and first == {"status": "success", "backup_name": None}, first
        before = (config / "migration-state.yaml").read_bytes()
        code, second = outcome(binary, config, data, root / "fresh-second.json")
        assert code == 0 and second == first
        assert (config / "migration-state.yaml").read_bytes() == before
        assert not (data / "backups").exists() or not list((data / "backups").iterdir())
        print("PASS fresh install and idempotent second startup")

        # Actual pinned upstream v1.6.1 YAML, SHA-256 snapshot, and typed files.
        config, data = paths(root / "upstream-v1.6.1")
        legacy = LEGACY.read_bytes()
        (config / "chimera-config.yaml").write_bytes(legacy)
        code, report = outcome(binary, config, data, root / "legacy-result.json")
        assert code == 0 and report["status"] == "success" and report["backup_name"], report
        snapshot = data / "backups" / report["backup_name"]
        count = verify_snapshot(snapshot)
        assert (snapshot / "config/chimera-config.yaml").read_bytes() == legacy
        for name in ("application.yaml", "session-state.yaml", "clash-config.yaml", "migration-state.yaml"):
            assert (config / name).is_file(), name
        print(f"PASS real v1.6.1 migration with {count} SHA-256-verified backup files")

        # Run failure: exact legacy bytes and verified snapshot retained.
        config, data = paths(root / "invalid-legacy")
        original = b"enable_system_proxy: [unterminated"
        (config / "chimera-config.yaml").write_bytes(original)
        code, report = outcome(binary, config, data, root / "broken-result.json")
        assert code != 0 and report["status"] == "failure", report
        assert report["report"]["failure"]["stage"] == "migration_run", report
        assert report["report"]["backups_dir"] == str(data / "backups"), report
        verify_snapshot(data / "backups" / report["backup_name"])
        assert (config / "chimera-config.yaml").read_bytes() == original
        assert not (config / "application.yaml").exists()
        print("PASS malformed input failure and verified original backup")

        # Preparation failure: a future schema must never be overwritten.
        config, data = paths(root / "future-schema")
        original = b"store_schema_version: 99\napp:\n  last_succeeded: 9.9.9\n"
        (config / "migration-state.yaml").write_bytes(original)
        code, report = outcome(binary, config, data, root / "future-result.json")
        assert code != 0 and report["status"] == "failure", report
        failure = report["report"]["failure"]
        assert failure["stage"] == "migration_prepare" and failure["kind"] == "newer_version", failure
        assert report["backup_name"] is None and report["report"]["backups_dir"] is None
        assert (config / "migration-state.yaml").read_bytes() == original
        assert not (config / "application.yaml").exists()
        print("PASS newer migration schema refusal without changes")

        # Occupied result destinations must be refused *before* migration runs.
        config, data = paths(root / "occupied-result")
        legacy = b"hotkeys: [clash_mode_rule,Control+Q]\n"
        (config / "chimera-config.yaml").write_bytes(legacy)
        result = root / "preexisting-result.json"
        result.write_bytes(b"existing user bytes")
        assert child(binary, config, data, result).returncode != 0
        assert result.read_bytes() == b"existing user bytes"
        assert not (config / "migration-state.yaml").exists()
        assert (config / "chimera-config.yaml").read_bytes() == legacy
        missing_result = root / "missing-result-parent" / "result.json"
        assert child(binary, config, data, missing_result).returncode != 0
        assert not missing_result.parent.exists()
        assert not (config / "migration-state.yaml").exists()
        print("PASS occupied or missing-parent result refusal before migration")

        # Unix links: the existing link itself and its outside target survive.
        if os.name == "posix":
            external = root / "external-user-owned-file"
            external.write_bytes(b"outside data")
            result = root / "linked-result.json"
            result.symlink_to(external)
            assert child(binary, config, data, result).returncode != 0
            assert result.is_symlink() and result.readlink() == external
            assert external.read_bytes() == b"outside data"
            assert (config / "chimera-config.yaml").read_bytes() == legacy
            assert not (config / "migration-state.yaml").exists()
            print("PASS occupied symlink result refusal without touching external data")
        else:
            print("SKIP symlink node check (Unix-only fixture)")

    print("All linked desktop migration binary scenarios passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", help="linked desktop executable, e.g. backend/target/debug/chimera")
    args = parser.parse_args()
    binary = Path(args.binary).resolve()
    if not binary.is_file():
        parser.error(f"desktop binary not found: {binary}")
    if not LEGACY.is_file():
        parser.error(f"pinned upstream fixture not found: {LEGACY}")
    verify(binary)


if __name__ == "__main__":
    main()
