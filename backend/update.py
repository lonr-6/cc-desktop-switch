"""自动更新检查协议。

当前阶段只做安全的检查动作：读取 latest.json、比较版本、返回可用资产。
不会自动下载、安装或替换当前程序。
"""

from __future__ import annotations

import re
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


def _pick_platform(latest_json: dict[str, Any], platform: str) -> dict[str, Any]:
    platforms = latest_json.get("platforms") or {}
    data = platforms.get(platform)
    if not isinstance(data, dict):
        raise UpdateCheckError(f"latest.json 中没有 {platform} 平台资产")
    return data


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
