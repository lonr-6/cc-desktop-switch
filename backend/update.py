"""自动更新检查和下载协议。"""

from __future__ import annotations

import hashlib
import os
import re
import tempfile
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

import httpx


class UpdateCheckError(Exception):
    """更新检查失败。"""


def _version_parts(version: str) -> list[int]:
    text = (version or "").strip().lstrip("vV")
    parts = re.findall(r"\d+", text)
    return [int(part) for part in parts] or [0]


def is_newer_version(latest: str, current: str) -> bool:
    """比较两个语义版本号，latest 大于 current 时返回 True。"""
    latest_parts = _version_parts(latest)
    current_parts = _version_parts(current)
    width = max(len(latest_parts), len(current_parts))
    latest_parts.extend([0] * (width - len(latest_parts)))
    current_parts.extend([0] * (width - len(current_parts)))
    return latest_parts > current_parts


def _validate_update_url(url: str) -> str:
    parsed = urlparse((url or "").strip())
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        raise UpdateCheckError("更新地址必须是 http 或 https URL")
    return parsed.geturl()


def _safe_asset_name(name: str) -> str:
    """只保留文件名，避免 latest.json 里带路径造成覆盖风险。"""
    filename = Path(str(name or "").strip()).name
    if not filename:
        raise UpdateCheckError("更新资产缺少文件名")
    return filename


def _file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _pick_platform(latest_json: dict[str, Any], platform: str) -> dict[str, Any]:
    platforms = latest_json.get("platforms") or {}
    data = platforms.get(platform)
    if not isinstance(data, dict):
        raise UpdateCheckError(f"latest.json 中没有 {platform} 平台资产")
    return data


def pick_windows_installer(assets: list[dict[str, Any]]) -> dict[str, Any]:
    """优先选择安装包资产，避免把便携版 zip 或单文件 exe 当成安装器。"""
    for asset in assets:
        name = str(asset.get("name") or "").lower()
        if name.endswith("windows-setup.exe"):
            return asset
    raise UpdateCheckError("当前版本没有 Windows 安装包资产")


async def fetch_latest_json(url: str) -> dict[str, Any]:
    safe_url = _validate_update_url(url)
    try:
        async with httpx.AsyncClient(timeout=10.0, follow_redirects=True) as client:
            response = await client.get(safe_url)
            response.raise_for_status()
            data = response.json()
    except httpx.HTTPError as exc:
        raise UpdateCheckError(f"更新地址请求失败: {exc}") from exc
    except ValueError as exc:
        raise UpdateCheckError("更新地址返回的不是有效 JSON") from exc

    if not isinstance(data, dict):
        raise UpdateCheckError("latest.json 格式错误")
    return data


async def check_update(
    url: str,
    current_version: str,
    platform: str = "windows-x64",
) -> dict[str, Any]:
    latest_json = await fetch_latest_json(url)
    latest_version = str(latest_json.get("version") or "")
    if not latest_version:
        raise UpdateCheckError("latest.json 缺少 version 字段")

    platform_data = _pick_platform(latest_json, platform)
    assets = platform_data.get("assets") or []
    if not isinstance(assets, list):
        raise UpdateCheckError("latest.json assets 字段格式错误")

    return {
        "success": True,
        "updateAvailable": is_newer_version(latest_version, current_version),
        "currentVersion": current_version,
        "latestVersion": latest_version,
        "platform": platform,
        "pubDate": latest_json.get("pub_date"),
        "notes": latest_json.get("notes", ""),
        "assets": assets,
        "minimumSupportedVersion": latest_json.get("minimum_supported_version"),
        "updateProtocol": latest_json.get("update_protocol", 1),
    }


async def download_asset(asset: dict[str, Any], target_dir: str | Path | None = None) -> dict[str, Any]:
    """下载资产并按 latest.json 中的 sha256 校验。"""
    url = _validate_update_url(str(asset.get("url") or ""))
    filename = _safe_asset_name(str(asset.get("name") or Path(urlparse(url).path).name))
    if not filename.lower().endswith(".exe"):
        raise UpdateCheckError("只能在应用内下载安装程序 exe")

    updates_dir = Path(target_dir or Path(tempfile.gettempdir()) / "CC-Desktop-Switch" / "updates")
    updates_dir.mkdir(parents=True, exist_ok=True)
    target = updates_dir / filename
    partial = target.with_name(f"{target.name}.download")

    try:
        async with httpx.AsyncClient(timeout=None, follow_redirects=True) as client:
            async with client.stream("GET", url) as response:
                response.raise_for_status()
                with partial.open("wb") as handle:
                    async for chunk in response.aiter_bytes():
                        if chunk:
                            handle.write(chunk)
    except httpx.HTTPError as exc:
        partial.unlink(missing_ok=True)
        raise UpdateCheckError(f"下载安装包失败: {exc}") from exc
    except OSError as exc:
        partial.unlink(missing_ok=True)
        raise UpdateCheckError(f"写入安装包失败: {exc}") from exc

    actual_sha = _file_sha256(partial)
    expected_sha = str(asset.get("sha256") or "").strip().lower()
    if expected_sha and actual_sha.lower() != expected_sha:
        partial.unlink(missing_ok=True)
        raise UpdateCheckError("安装包校验失败，已取消安装")

    os.replace(partial, target)
    return {
        "asset": asset,
        "path": str(target),
        "sha256": actual_sha,
        "size": target.stat().st_size,
    }


async def download_update(
    url: str,
    current_version: str,
    platform: str = "windows-x64",
    target_dir: str | Path | None = None,
) -> dict[str, Any]:
    """检查更新，确认落后时下载 Windows 安装包。"""
    result = await check_update(url, current_version, platform)
    if not result.get("updateAvailable"):
        return {
            **result,
            "downloaded": False,
            "message": "当前已是最新版本",
        }

    installer_asset = pick_windows_installer(result.get("assets") or [])
    downloaded = await download_asset(installer_asset, target_dir=target_dir)
    return {
        **result,
        "downloaded": True,
        "installerAsset": installer_asset,
        "installerPath": downloaded["path"],
        "installerSha256": downloaded["sha256"],
        "installerSize": downloaded["size"],
    }
