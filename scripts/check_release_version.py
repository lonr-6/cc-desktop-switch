#!/usr/bin/env python3
"""Validate that release inputs match the package versions baked into bundles."""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CARGO_TOML = ROOT / "src-tauri" / "Cargo.toml"
TAURI_CONF = ROOT / "src-tauri" / "tauri.conf.json"


def github_error(message: str) -> None:
    print(f"::error::{message}", file=sys.stderr)


def read_cargo_version() -> str:
    with CARGO_TOML.open("rb") as handle:
        data = tomllib.load(handle)
    return str(data["package"]["version"])


def read_tauri_version() -> str:
    with TAURI_CONF.open("r", encoding="utf-8") as handle:
        data = json.load(handle)
    return str(data["version"])


def normalize_expected_version(version: str) -> str:
    version = version.strip()
    if not version:
        raise ValueError("expected version is empty")
    if version.startswith("v"):
        raise ValueError(
            f"expected version must not include a leading 'v': {version!r}"
        )
    return version


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Fail release builds when workflow/tag version differs from "
            "src-tauri/Cargo.toml or src-tauri/tauri.conf.json."
        )
    )
    parser.add_argument("expected_version", help="Release version without leading v")
    args = parser.parse_args()

    try:
        expected = normalize_expected_version(args.expected_version)
        cargo_version = read_cargo_version()
        tauri_version = read_tauri_version()
    except Exception as exc:
        github_error(f"release version check failed to read version sources: {exc}")
        return 1

    mismatches = []
    if cargo_version != expected:
        mismatches.append(f"{CARGO_TOML.relative_to(ROOT)} has {cargo_version}")
    if tauri_version != expected:
        mismatches.append(f"{TAURI_CONF.relative_to(ROOT)} has {tauri_version}")

    if mismatches:
        github_error(
            "Release version mismatch: workflow resolved "
            f"{expected}, but " + "; ".join(mismatches)
        )
        github_error(
            "Update both version sources before publishing so installer metadata, "
            "bundle metadata, asset names, and latest.json describe the same release."
        )
        return 1

    print(
        "Release version check passed: "
        f"{expected} matches src-tauri/Cargo.toml and src-tauri/tauri.conf.json."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
