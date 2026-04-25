"""Provider 辅助能力：模型列表、模型映射建议、余额/用量查询。"""

from __future__ import annotations

from typing import Any
from urllib.parse import urlsplit, urlunsplit

import httpx

from backend.proxy import build_upstream_url, get_upstream_headers


MODEL_EXCLUDE_KEYWORDS = (
    "embedding",
    "rerank",
    "moderation",
    "whisper",
    "tts",
    "image",
    "vision",
    "audio",
)


def _clean_base_url(url: str) -> str:
    return str(url or "").strip().rstrip("/")


def _replace_path_suffix(url: str, suffixes: tuple[str, ...], replacement: str) -> str:
    parts = urlsplit(url)
    path = parts.path.rstrip("/")
    lower = path.lower()
    for suffix in suffixes:
        if lower.endswith(suffix):
            path = path[: -len(suffix)]
            break
    return urlunsplit((parts.scheme, parts.netloc, f"{path.rstrip('/')}/{replacement.lstrip('/')}", "", ""))


def model_endpoint_candidates(provider: dict) -> list[str]:
    """生成可能的模型列表 endpoint。

    不同国产 API 对 OpenAI/Anthropic 兼容层的 URL 约定不同，所以这里按常见
    端点做少量候选，逐个尝试。
    """
    base_url = _clean_base_url(provider.get("baseUrl", ""))
    if not base_url:
        return []

    api_format = str(provider.get("apiFormat", "anthropic")).lower()
    upstream = build_upstream_url(base_url, api_format)
    candidates: list[str] = []

    if api_format == "openai":
        candidates.append(_replace_path_suffix(upstream, ("/chat/completions", "/completions"), "/models"))
        candidates.append(f"{base_url}/models")
    else:
        candidates.append(_replace_path_suffix(upstream, ("/v1/messages", "/messages"), "/v1/models"))
        if base_url.lower().endswith("/v1"):
            candidates.append(f"{base_url}/models")
        candidates.append(f"{base_url}/models")
        parts = urlsplit(base_url)
        stripped_path = parts.path.rstrip("/")
        if stripped_path.lower().endswith("/anthropic"):
            root_path = stripped_path[: -len("/anthropic")]
            root_url = urlunsplit((parts.scheme, parts.netloc, root_path.rstrip("/"), "", "")).rstrip("/")
            candidates.append(f"{root_url}/models")
            candidates.append(f"{root_url}/v1/models")

    seen = set()
    unique = []
    for item in candidates:
        if item and item not in seen:
            unique.append(item)
            seen.add(item)
    return unique


def _model_id_from_item(item: Any) -> str | None:
    if isinstance(item, str):
        return item
    if not isinstance(item, dict):
        return None
    for key in ("id", "name", "model", "model_id"):
        value = item.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return None


def extract_model_ids(payload: Any) -> list[str]:
    """从 OpenAI/Anthropic/通用模型列表响应中提取模型 ID。"""
    candidates: list[Any] = []
    if isinstance(payload, list):
        candidates = payload
    elif isinstance(payload, dict):
        for key in ("data", "models", "items", "result"):
            value = payload.get(key)
            if isinstance(value, list):
                candidates = value
                break
        if not candidates and isinstance(payload.get("data"), dict):
            data = payload["data"]
            for key in ("models", "items"):
                value = data.get(key)
                if isinstance(value, list):
                    candidates = value
                    break

    model_ids = []
    seen = set()
    for item in candidates:
        model_id = _model_id_from_item(item)
        if not model_id:
            continue
        if model_id in seen:
            continue
        model_ids.append(model_id)
        seen.add(model_id)
    return model_ids


def _usable_model_ids(model_ids: list[str]) -> list[str]:
    usable = []
    for model_id in model_ids:
        lower = model_id.lower()
        if any(keyword in lower for keyword in MODEL_EXCLUDE_KEYWORDS):
            continue
        usable.append(model_id)
    return usable or model_ids


def _pick_model(model_ids: list[str], keywords: tuple[str, ...], fallback_index: int = 0) -> str:
    for keyword in keywords:
        for model_id in model_ids:
            if keyword in model_id.lower():
                return model_id
    if not model_ids:
        return ""
    return model_ids[min(fallback_index, len(model_ids) - 1)]


def suggest_model_mappings(model_ids: list[str]) -> dict:
    """根据模型名称给 Claude Sonnet/Haiku/Opus 自动推荐映射。"""
    usable = _usable_model_ids(model_ids)
    sonnet = _pick_model(
        usable,
        ("sonnet", "claude", "k2", "glm-5.1", "qwen3-max", "max", "pro", "chat"),
    )
    haiku = _pick_model(
        usable,
        ("haiku", "flash", "lite", "mini", "turbo", "fast", "v3", "chat"),
        fallback_index=0,
    )
    opus = _pick_model(
        usable,
        ("opus", "reasoner", "thinking", "r1", "max", "pro", "plus"),
        fallback_index=0,
    )
    default = sonnet or opus or haiku or (usable[0] if usable else "")
    return {
        "sonnet": sonnet or default,
        "haiku": haiku or default,
        "opus": opus or default,
        "default": default,
    }


async def fetch_provider_models(provider: dict) -> dict:
    """调用 provider 模型列表接口并返回模型 ID 与推荐映射。"""
    endpoints = model_endpoint_candidates(provider)
    if not endpoints:
        return {"success": False, "message": "API 地址无效", "models": [], "suggested": {}}

    headers = get_upstream_headers(provider)
    headers.pop("Content-Type", None)
    errors = []
    timeout = httpx.Timeout(12.0, connect=6.0)
    async with httpx.AsyncClient(timeout=timeout, follow_redirects=True) as client:
        for endpoint in endpoints:
            try:
                response = await client.get(endpoint, headers=headers)
            except httpx.RequestError as exc:
                errors.append(f"{endpoint}: {exc.__class__.__name__}")
                continue
            if not response.is_success:
                errors.append(f"{endpoint}: HTTP {response.status_code}")
                continue
            try:
                payload = response.json()
            except ValueError:
                errors.append(f"{endpoint}: 非 JSON 响应")
                continue
            model_ids = extract_model_ids(payload)
            if model_ids:
                return {
                    "success": True,
                    "endpoint": endpoint,
                    "models": model_ids,
                    "suggested": suggest_model_mappings(model_ids),
                }
            errors.append(f"{endpoint}: 未发现模型列表")

    return {
        "success": False,
        "message": "无法自动获取模型列表",
        "models": [],
        "suggested": {},
        "errors": errors[-5:],
    }


def _provider_kind(provider: dict) -> str:
    probe = f"{provider.get('name', '')} {provider.get('baseUrl', '')}".lower()
    if "deepseek" in probe:
        return "deepseek"
    if "siliconflow" in probe:
        return "siliconflow"
    if "openrouter" in probe:
        return "openrouter"
    if "novita" in probe:
        return "novita"
    if "stepfun" in probe or "step" in probe:
        return "stepfun"
    return "unknown"


def balance_endpoint(provider: dict) -> tuple[str, str] | None:
    kind = _provider_kind(provider)
    base = _clean_base_url(provider.get("baseUrl", "")).lower()
    if kind == "deepseek":
        return kind, "https://api.deepseek.com/user/balance"
    if kind == "siliconflow":
        host = "https://api.siliconflow.cn"
        if ".com" in base:
            host = "https://api.siliconflow.com"
        return kind, f"{host}/v1/user/info"
    if kind == "openrouter":
        return kind, "https://openrouter.ai/api/v1/credits"
    if kind == "novita":
        return kind, "https://api.novita.ai/v3/user/balance"
    if kind == "stepfun":
        return kind, "https://api.stepfun.com/v1/accounts"
    return None


def _float_or_none(value: Any) -> float | None:
    try:
        if value is None or value == "":
            return None
        return float(value)
    except (TypeError, ValueError):
        return None


def _money_item(label: str, remaining=None, total=None, used=None, unit: str = "") -> dict:
    return {
        "label": label,
        "remaining": _float_or_none(remaining),
        "total": _float_or_none(total),
        "used": _float_or_none(used),
        "unit": unit,
    }


def normalize_balance_payload(kind: str, payload: dict) -> list[dict]:
    """把不同厂商的余额/用量响应整理为统一结构。"""
    if kind == "deepseek":
        items = []
        for item in payload.get("balance_infos", []) or []:
            if not isinstance(item, dict):
                continue
            currency = item.get("currency", "CNY")
            items.append(_money_item(
                label=str(currency),
                remaining=item.get("total_balance"),
                total=item.get("granted_balance"),
                used=item.get("topped_up_balance"),
                unit=str(currency),
            ))
        return items

    if kind == "openrouter":
        data = payload.get("data", payload)
        total = _float_or_none(data.get("total_credits"))
        used = _float_or_none(data.get("total_usage"))
        remaining = total - used if total is not None and used is not None else None
        return [_money_item("credits", remaining=remaining, total=total, used=used, unit="USD")]

    data = payload.get("data", payload)
    if isinstance(data, dict):
        for remaining_key in ("balance", "remaining", "available_balance", "availableBalance", "credit"):
            if remaining_key in data:
                return [_money_item(
                    "balance",
                    remaining=data.get(remaining_key),
                    total=data.get("total") or data.get("totalBalance") or data.get("total_credits"),
                    used=data.get("used") or data.get("usage") or data.get("usedBalance"),
                    unit=str(data.get("currency") or data.get("unit") or ""),
                )]
    return []


async def query_provider_usage(provider: dict) -> dict:
    """查询 provider 余额/用量。当前仅支持已知公开余额接口的厂商。"""
    if not provider.get("apiKey"):
        return {"success": False, "message": "请先保存 API Key"}

    endpoint_info = balance_endpoint(provider)
    if not endpoint_info:
        return {
            "success": True,
            "supported": False,
            "items": [],
            "message": "这个提供商暂未适配余额/用量接口",
        }

    kind, endpoint = endpoint_info
    headers = get_upstream_headers(provider)
    headers.pop("Content-Type", None)
    timeout = httpx.Timeout(12.0, connect=6.0)
    try:
        async with httpx.AsyncClient(timeout=timeout, follow_redirects=True) as client:
            response = await client.get(endpoint, headers=headers)
    except httpx.RequestError as exc:
        return {
            "success": True,
            "supported": True,
            "ok": False,
            "message": f"查询失败：{exc.__class__.__name__}",
            "items": [],
        }

    if not response.is_success:
        return {
            "success": True,
            "supported": True,
            "ok": False,
            "statusCode": response.status_code,
            "message": f"余额接口返回 HTTP {response.status_code}",
            "items": [],
        }

    try:
        payload = response.json()
    except ValueError:
        return {
            "success": True,
            "supported": True,
            "ok": False,
            "message": "余额接口返回了非 JSON 响应",
            "items": [],
        }

    items = normalize_balance_payload(kind, payload)
    return {
        "success": True,
        "supported": True,
        "ok": bool(items),
        "endpoint": endpoint,
        "items": items,
        "message": "查询完成" if items else "余额接口响应中未识别到余额字段",
    }
