"""FastAPI 应用 - 管理 API + 静态文件服务"""

import asyncio
import json
import os
import platform as platform_module
import re
import secrets
import socket
import subprocess
import sys
import threading
import time
from urllib.parse import urlparse, urlunparse

import httpx
import uvicorn
from pathlib import Path
from typing import Callable, Optional

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse
from fastapi.staticfiles import StaticFiles

from backend import config as cfg
from backend import ccswitch_import
from backend import provider_tools
from backend import registry
from backend import update as updater
from backend.api_adapters import normalize_api_format
from backend.model_alias import desktop_route_ids, provider_model_ids
from backend.proxy import (
    build_upstream_url,
    create_proxy_app,
    get_upstream_headers,
    stats as proxy_stats,
    log_buffer as proxy_logs,
)

# ── 路径设置 ──
# 前端目录: 项目根目录下的 frontend/
FRONTEND_DIR = Path(__file__).resolve().parent.parent / "frontend"
_update_quit_handler: Optional[Callable[[], None]] = None
_app_activation_handler: Optional[Callable[[], bool]] = None
_admin_token = secrets.token_urlsafe(32)
ADMIN_TOKEN_HEADER = "x-ccds-admin-token"
STATIC_REQUEST_HEADER = "x-ccds-request"
DIAGNOSTICS_FORMAT = "ccds.diagnostics.v1"
REDACTED = "******"
_SENSITIVE_FIELD_RE = re.compile(
    r"(api[-_]?key|gatewayapikey|token|secret|password|authorization|cookie|headers)",
    re.IGNORECASE,
)


def get_admin_token() -> str:
    """返回当前进程的本机管理 API token。"""
    return _admin_token


def verify_admin_token(value: str) -> bool:
    """校验本机管理 API token。"""
    return bool(value) and secrets.compare_digest(str(value), _admin_token)


def _popen_hidden(command: list[str], *, detached: bool = False):
    """启动外部程序时避免 Windows 弹出黑色终端窗口。"""
    kwargs = {"close_fds": True}
    if detached:
        kwargs["stdin"] = subprocess.DEVNULL
        kwargs["stdout"] = subprocess.DEVNULL
        kwargs["stderr"] = subprocess.DEVNULL
    if sys.platform == "win32":
        startupinfo = subprocess.STARTUPINFO()
        startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        kwargs["startupinfo"] = startupinfo
        creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
        if detached:
            creationflags |= getattr(subprocess, "DETACHED_PROCESS", 0)
            creationflags |= getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
        kwargs["creationflags"] = creationflags
    elif detached:
        kwargs["start_new_session"] = True
    return subprocess.Popen(command, **kwargs)


def register_update_quit_handler(handler: Optional[Callable[[], None]]):
    """注册更新安装前用于优雅退出主应用的回调。"""
    global _update_quit_handler
    _update_quit_handler = handler


def register_app_activation_handler(handler: Optional[Callable[[], bool]]):
    """注册第二次启动时唤起已有窗口的回调。"""
    global _app_activation_handler
    _app_activation_handler = handler


def _get_update_quit_handler() -> Optional[Callable[[], None]]:
    handler = _update_quit_handler
    return handler if callable(handler) else None


def _activate_existing_app() -> bool:
    handler = _app_activation_handler
    if not callable(handler):
        return False
    try:
        return bool(handler())
    except Exception:
        return False


def _schedule_update_quit_for_install(handler: Callable[[], None], delay: float = 0.8) -> bool:
    """延迟一点退出主应用，确保前端先收到安装响应。"""
    if not callable(handler):
        return False

    def _invoke_handler():
        try:
            handler()
        except Exception:
            return

    timer = threading.Timer(delay, _invoke_handler)
    timer.daemon = True
    timer.start()
    return True


def _launch_update_installer(installer_path: str, platform: str) -> bool:
    """启动安装器；macOS 上优先等待当前应用退出后再打开安装包。"""
    quit_handler = _get_update_quit_handler() if platform.startswith("macos-") else None
    if platform.startswith("macos-") and quit_handler:
        command = updater.install_after_quit_command(installer_path, platform, os.getpid())
        _popen_hidden(command, detached=True)
        return _schedule_update_quit_for_install(quit_handler)

    command = updater.install_command(installer_path, platform)
    _popen_hidden(command, detached=True)
    return False


def _public_provider(provider: Optional[dict]) -> Optional[dict]:
    """返回给前端展示的 provider，避免泄露 API Key。"""
    if not provider:
        return None
    public = dict(provider)
    if "apiKey" in public:
        public["hasApiKey"] = bool(public.get("apiKey"))
        public.pop("apiKey", None)
    public.pop("extraHeaders", None)
    return public


def _redact_text(value: str, limit: int = 500) -> str:
    """脱敏文本，供诊断包和支持信息使用。"""
    text = str(value or "")
    text = re.sub(r"(?i)(bearer\s+)[a-z0-9._~+/=-]{12,}", r"\1******", text)
    text = re.sub(
        r"(?i)(\b(?:authorization|x-api-key|api[-_]?key|access[-_]?token|refresh[-_]?token|id[-_]?token|client[-_]?secret|token|secret|password|key)\b\s*[:=]\s*[\"']?)[^\"'\s,&<>]+",
        r"\1******",
        text,
    )
    text = re.sub(
        r"(?i)([?&][^=&#\s]*(?:api[-_]?key|access[-_]?token|refresh[-_]?token|id[-_]?token|client[-_]?secret|token|secret|password|key)[^=&#\s]*=)[^&#\s]+",
        r"\1******",
        text,
    )
    text = re.sub(
        r"(?i)(\b[\w.-]*(?:api[-_]?key|access[-_]?token|refresh[-_]?token|id[-_]?token|client[-_]?secret|token|secret|password|key)[\w.-]*\s*=\s*)[^<>\s,&]+",
        r"\1******",
        text,
    )
    text = re.sub(r"(?i)\b(sk-[a-z0-9_-]{8,}|ccds_[a-z0-9_-]{8,})\b", REDACTED, text)
    text = re.sub(r"(https?://)([^/@\s:]+):([^/@\s]+)@", r"\1******:******@", text)
    return text[:limit]


def _is_sensitive_field(key: str) -> bool:
    return bool(_SENSITIVE_FIELD_RE.search(str(key or "")))


def _redact_value(value, field_name: str = ""):
    """递归脱敏诊断输出，避免配置 key 或日志文本泄露。"""
    if _is_sensitive_field(field_name):
        if isinstance(value, dict):
            return {str(key): REDACTED if item else "" for key, item in value.items()}
        if isinstance(value, list):
            return [REDACTED if item else "" for item in value]
        return REDACTED if value else ""
    if isinstance(value, dict):
        return {str(key): _redact_value(item, str(key)) for key, item in value.items()}
    if isinstance(value, list):
        return [_redact_value(item, field_name) for item in value]
    if isinstance(value, str):
        return _redact_text(value, 2000)
    return value


def _safe_url_parts(url: str) -> dict:
    """只保留不含凭据和查询参数的 URL 诊断信息。"""
    try:
        parsed = urlparse(str(url or ""))
    except Exception:
        return {"scheme": "", "host": "", "path": ""}
    return {
        "scheme": parsed.scheme,
        "host": parsed.hostname or "",
        "port": parsed.port,
        "path": parsed.path or "",
        "base": urlunparse((parsed.scheme, parsed.netloc.split("@")[-1], parsed.path, "", "", "")),
    }


def _diagnostics_provider(provider: dict) -> dict:
    models = provider.get("models") if isinstance(provider.get("models"), dict) else {}
    return {
        "id": provider.get("id"),
        "name": provider.get("name"),
        "apiFormat": provider.get("apiFormat") or "anthropic",
        "authScheme": provider.get("authScheme") or "bearer",
        "hasApiKey": bool(provider.get("apiKey")),
        "baseUrl": _safe_url_parts(provider.get("baseUrl", "")),
        "mappedSlots": sorted([key for key, value in models.items() if value]),
    }


def _diagnostics_recent_logs(limit: int = 50) -> list[dict]:
    logs = proxy_logs.get_all()[-limit:]
    return [
        {
            "time": item.get("time"),
            "level": item.get("level"),
            "message": _redact_text(item.get("message", ""), 800),
        }
        for item in logs
    ]


def _diagnostics_payload() -> dict:
    settings = cfg.get_settings()
    providers = cfg.get_providers()
    active = cfg.get_active_provider()
    desktop_status = registry.get_config_status()
    proxy_port = settings.get("proxyPort", 18080)
    desktop_health = _desktop_health(desktop_status, proxy_port, active, providers, False)
    return {
        "format": DIAGNOSTICS_FORMAT,
        "exportedAt": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "app": {
            "version": cfg.DEFAULT_CONFIG.get("version", "1.0.0"),
            "platform": sys.platform,
            "platformDetail": platform_module.platform(),
            "adminPort": settings.get("adminPort", 18081),
            "proxyPort": proxy_port,
        },
        "desktop": {
            "configured": bool(desktop_status.get("configured")),
            "health": _redact_value(desktop_health),
            "keys": _redact_value(desktop_status.get("keys") or {}),
        },
        "providers": [_diagnostics_provider(provider) for provider in providers],
        "activeProviderId": active.get("id") if active else None,
        "settings": {
            "autoStart": bool(settings.get("autoStart")),
            "upstreamProxy": {
                "enabled": bool(settings.get("upstreamProxyEnabled")),
                "value": "******" if settings.get("upstreamProxy") else "",
            },
        },
        "proxy": {
            "running": bool(_proxy_running),
            "port": _proxy_port or proxy_port,
            "stats": proxy_stats.to_dict(),
            "recentLogs": _diagnostics_recent_logs(),
        },
        "redactions": [
            "apiKey",
            "gatewayApiKey",
            "authorization",
            "x-api-key",
            "url.userinfo",
            "token-like-query",
        ],
    }


def _diagnostics_checks(payload: dict) -> list[dict]:
    checks = []
    active_id = payload.get("activeProviderId")
    providers = payload.get("providers") or []
    active = next((provider for provider in providers if provider.get("id") == active_id), None)
    checks.append({
        "code": "active_provider",
        "ok": bool(active),
        "message": "已选择默认 provider" if active else "尚未选择默认 provider",
    })
    checks.append({
        "code": "active_provider_api_key",
        "ok": bool(active and active.get("hasApiKey")),
        "message": "默认 provider 已保存 API Key" if active and active.get("hasApiKey") else "默认 provider 缺少 API Key",
    })
    checks.append({
        "code": "desktop_config",
        "ok": bool(payload.get("desktop", {}).get("configured")),
        "message": "Claude Desktop 配置由本工具管理" if payload.get("desktop", {}).get("configured") else "Claude Desktop 尚未配置或不是由本工具管理",
    })
    health = payload.get("desktop", {}).get("health") or {}
    checks.append({
        "code": "desktop_health",
        "ok": not bool(health.get("needsApply")),
        "message": "桌面版配置与当前 provider 一致" if not health.get("needsApply") else "桌面版配置需要重新应用",
        "issues": health.get("issues") or [],
    })
    checks.append({
        "code": "local_gateway",
        "ok": bool(payload.get("proxy", {}).get("running")),
        "message": "本机 gateway 正在运行" if payload.get("proxy", {}).get("running") else "本机 gateway 未运行",
    })
    return checks


def _provider_not_found():
    return JSONResponse(
        status_code=404,
        content={"success": False, "message": "提供商不存在"},
    )


def _parse_inference_models(raw_value: str) -> list:
    """解析 Desktop managed policy 中的 inferenceModels。"""
    try:
        parsed = json.loads(raw_value or "[]")
    except (TypeError, ValueError):
        return []
    return parsed if isinstance(parsed, list) else []


def _inference_model_names(items: list) -> list[str]:
    """提取 Desktop inferenceModels 里的模型名称。"""
    names = []
    for item in items:
        if isinstance(item, dict):
            value = item.get("name")
        else:
            value = item
        name = str(value or "").strip()
        if name:
            names.append(name)
    return names


def _raw_desktop_model_names(names: list[str], provider: Optional[dict], providers: Optional[list[dict]]) -> list[str]:
    """识别旧版本写入的真实上游模型名。"""
    suspicious_tokens = (
        "deepseek",
        "kimi",
        "moonshot",
        "glm",
        "qwen",
        "dashscope",
        "aliyun",
        "siliconflow",
        "mimo",
    )
    upstream_ids = set(provider_model_ids(provider))
    for item in providers or []:
        upstream_ids.update(provider_model_ids(item))

    raw_names = []
    for name in names:
        lowered = name.lower()
        if name in upstream_ids or any(token in lowered for token in suspicious_tokens):
            raw_names.append(name)
    return raw_names


def _stale_desktop_route_names(names: list[str], target_models: list) -> list[str]:
    """识别旧配置里残留的未映射 Claude-safe route。"""
    allowed = {
        str(item.get("name"))
        for item in target_models
        if isinstance(item, dict) and item.get("name")
    }
    known_routes = set(desktop_route_ids())
    return [
        name
        for name in names
        if name in known_routes and name not in allowed
    ]


def _no_desktop_model_routes_response(target: dict, proxy_port: int, *, attempted: bool = False) -> dict:
    """生成没有显式模型映射时的失败响应。"""
    response = {
        "success": False,
        "message": "请至少映射一个 Claude 模型槽位后再应用到 Claude 桌面版。",
        "mode": target["mode"],
        "requiresProxy": target["requiresProxy"],
        "proxyStarted": False,
        "proxyPort": proxy_port,
        "configSuccess": False,
    }
    if attempted:
        response["attempted"] = True
    return response


def desktop_config_target_for_provider(provider: Optional[dict], settings: Optional[dict] = None) -> dict:
    """生成 Claude Desktop 写入目标。

    v1.0.18 起默认写入本机转发地址。这样所有第三方 provider 都走同一条
    代理链，模型映射、额外请求头和协议转换逻辑统一由后台处理。
    """
    settings = settings or cfg.get_settings()
    proxy_port = settings.get("proxyPort", 18080)
    return {
        "baseUrl": f"http://127.0.0.1:{proxy_port}",
        "apiKey": cfg.get_or_create_gateway_api_key(),
        "authScheme": "bearer",
        "gatewayHeaders": "",
        "provider": provider,
        "providers": None,
        "exposeAll": False,
        "requiresProxy": True,
        "mode": "local_proxy",
    }


def _desktop_health(
    desktop_status: dict,
    proxy_port: int,
    provider: Optional[dict],
    providers: Optional[list[dict]] = None,
    expose_all: bool = False,
) -> dict:
    """判断 Claude Desktop 配置是否仍指向本工具当前 provider。"""
    keys = desktop_status.get("keys") or {}
    settings = dict(cfg.get_settings())
    settings["proxyPort"] = proxy_port
    target = desktop_config_target_for_provider(provider, settings)
    expected_base_url = str(target.get("baseUrl") or "").rstrip("/")
    actual_base_url = str(keys.get("inferenceGatewayBaseUrl") or "").rstrip("/")
    issues = []

    if actual_base_url and actual_base_url != expected_base_url:
        issues.append({
            "code": "gateway_base_url_mismatch",
            "message": "Claude 桌面版仍指向旧地址，请重新一键应用到 Claude 桌面版。",
        })

    if desktop_status.get("configured") is False:
        if keys:
            issues.append({
                "code": "not_managed_by_ccds",
                "message": "当前桌面版配置不是由本工具最新版本写入。",
            })
        else:
            issues.append({
                "code": "desktop_not_configured",
                "message": "桌面版尚未配置，请添加提供商并一键应用到 Claude 桌面版。",
            })

    inference_models = _parse_inference_models(str(keys.get("inferenceModels") or ""))
    inference_model_names = _inference_model_names(inference_models)
    raw_model_names = _raw_desktop_model_names(inference_model_names, provider, providers)
    if raw_model_names:
        issues.append({
            "code": "invalid_inference_model_names",
            "message": "Claude 桌面版配置里仍有第三方真实模型名，请重新一键应用到 Claude 桌面版。",
            "models": raw_model_names,
        })
    target_models = registry.provider_inference_models(provider)
    stale_route_names = _stale_desktop_route_names(inference_model_names, target_models)
    if stale_route_names:
        issues.append({
            "code": "stale_inference_model_routes",
            "message": "Claude 桌面版配置里仍有未映射模型入口，请重新一键应用并重启 Claude 桌面版。",
            "models": stale_route_names,
        })
    one_million_models = [
        str(item.get("name"))
        for item in target_models
        if isinstance(item, dict) and item.get("supports1m") is True and item.get("name")
    ]
    one_million_ready = True
    if one_million_models:
        written_one_million = {
            str(item.get("name"))
            for item in inference_models
            if (
                isinstance(item, dict)
                and item.get("name")
                and item.get("supports1m") is True
            )
        }
        one_million_ready = all(model in written_one_million for model in one_million_models)
        if not one_million_ready:
            issues.append({
                "code": "one_million_not_written",
                "message": "1M 上下文模型尚未写入桌面版配置，请重新一键应用并重启 Claude 桌面版。",
            })

    return {
        "needsApply": bool(issues),
        "oneMillionReady": one_million_ready,
        "expectedBaseUrl": expected_base_url,
        "actualBaseUrl": actual_base_url,
        "mode": target.get("mode"),
        "requiresProxy": bool(target.get("requiresProxy")),
        "issues": issues,
    }


def _sync_desktop_for_active_provider() -> dict:
    """默认 provider 切换后，同步本工具管理的 Claude 桌面版模型列表。"""
    provider = cfg.get_active_provider()
    if not provider:
        return {"attempted": False, "success": False, "message": "没有默认提供商"}

    settings = cfg.get_settings()
    target = desktop_config_target_for_provider(provider, settings)
    proxy_port = int(settings.get("proxyPort", 18080))
    if not registry.provider_inference_models(provider):
        return _no_desktop_model_routes_response(target, proxy_port, attempted=True)
    result = registry.apply_config(
        target["baseUrl"],
        gateway_api_key=target["apiKey"],
        provider=target["provider"],
        providers=target["providers"],
        expose_all=target["exposeAll"],
        auth_scheme=target["authScheme"],
        gateway_headers=target["gatewayHeaders"],
    )
    response = {
        "attempted": True,
        "mode": target["mode"],
        "requiresProxy": target["requiresProxy"],
        "configSuccess": bool(result.get("success")),
        "proxyStarted": False,
        "proxyPort": proxy_port,
        **result,
    }
    if result.get("success") and target["requiresProxy"]:
        proxy_started = _start_proxy_server(proxy_port)
        response["proxyStarted"] = bool(proxy_started)
        if not proxy_started:
            response["success"] = False
            response["message"] = (
                "桌面版配置已写入，但本机转发服务启动失败。"
                "请检查转发端口是否被占用，或在设置中更换转发端口后重试。"
            )
    elif result.get("success"):
        response["proxyStarted"] = bool(_proxy_running)
    return response


def _provider_test_model(provider: dict) -> str:
    for model in provider_model_ids(provider):
        if model:
            return model
    return "claude-sonnet-4-6"


def _provider_test_body(provider: dict, api_format: str) -> dict:
    model = _provider_test_model(provider)
    if api_format == "openai_chat":
        return {
            "model": model,
            "messages": [{"role": "user", "content": "ping"}],
            "max_tokens": 8,
            "stream": False,
        }
    return {
        "model": model,
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 8,
    }


def _is_kimi_provider(provider: dict) -> bool:
    """粗略识别 Kimi provider，用于给出更具体的排错提示。"""
    probe = f"{provider.get('name', '')} {provider.get('baseUrl', '')}".lower()
    return "kimi" in probe or "moonshot" in probe


async def _test_provider_connection(provider: dict) -> dict:
    """测试 provider 是否能真实访问上游接口。"""
    api_format = normalize_api_format(provider.get("apiFormat", "anthropic"))
    base_url = build_upstream_url(provider.get("baseUrl", ""), api_format)
    parsed = urlparse(base_url)
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        return {
            "success": False,
            "message": "API 地址无效",
        }

    headers = get_upstream_headers(provider)
    headers.pop("Content-Type", None)
    started = time.perf_counter()

    try:
        timeout = httpx.Timeout(8.0, connect=5.0)
        async with httpx.AsyncClient(timeout=timeout, follow_redirects=False) as client:
            response = await client.head(base_url, headers=headers)
            if response.status_code in {404, 405}:
                response = await client.get(base_url, headers=headers)
            if response.status_code in {404, 405} and provider.get("apiKey"):
                response = await client.post(
                    base_url,
                    headers=get_upstream_headers(provider),
                    json=_provider_test_body(provider, api_format),
                )
    except httpx.RequestError as exc:
        latency_ms = round((time.perf_counter() - started) * 1000)
        return {
            "success": True,
            "ok": False,
            "latencyMs": latency_ms,
            "message": f"连接失败：{exc.__class__.__name__}",
        }

    latency_ms = round((time.perf_counter() - started) * 1000)
    status_code = response.status_code
    reachable = status_code < 500
    if 200 <= status_code < 300:
        message = f"连接正常，{latency_ms} ms"
    elif status_code in {401, 403}:
        reachable = False
        if _is_kimi_provider(provider):
            message = (
                f"Kimi 认证失败，HTTP {status_code}。Kimi Platform Key 请使用 "
                f"https://api.moonshot.cn/anthropic；Kimi Code 会员 Key 请使用 "
                f"https://api.kimi.com/coding，{latency_ms} ms"
            )
        else:
            message = f"认证失败，HTTP {status_code}，请检查 API Key 和 API 地址是否匹配，{latency_ms} ms"
    elif status_code in {404, 405}:
        reachable = False
        message = f"接口不可用，HTTP {status_code}，请检查 API 地址是否填到了兼容 Claude 的接口，{latency_ms} ms"
    else:
        message = f"地址可达，HTTP {status_code}，{latency_ms} ms"

    return {
        "success": True,
        "ok": reachable,
        "latencyMs": latency_ms,
        "statusCode": status_code,
        "message": message,
    }


def _provider_compatibility(provider: dict) -> dict:
    """返回 provider 第三方接口兼容性摘要，不发起网络请求。"""
    api_format = normalize_api_format(provider.get("apiFormat", "anthropic"))
    if api_format == "anthropic":
        return {
            "id": provider.get("id"),
            "name": provider.get("name"),
            "apiFormat": api_format,
            "level": "stable",
            "message": "Anthropic 兼容接口，适合 Claude 桌面版主流程。",
            "checks": {
                "models": True,
                "text": True,
                "stream": True,
                "tools": True,
                "streamingTools": True,
            },
        }
    if api_format == "openai_chat":
        return {
            "id": provider.get("id"),
            "name": provider.get("name"),
            "apiFormat": api_format,
            "level": "experimental",
            "message": "OpenAI Chat 实验适配：文本和非流式工具调用可测试，流式工具调用暂不作为稳定能力。",
            "checks": {
                "models": True,
                "text": True,
                "stream": True,
                "tools": True,
                "streamingTools": False,
            },
        }
    return {
        "id": provider.get("id"),
        "name": provider.get("name"),
        "apiFormat": api_format,
        "level": "unsupported",
        "message": f"{api_format} 暂未适配。",
        "checks": {
            "models": False,
            "text": False,
            "stream": False,
            "tools": False,
            "streamingTools": False,
        },
    }


async def _detect_local_proxy() -> Optional[str]:
    """尝试自动检测本地代理端口。先检查环境变量，再扫描常见端口。"""
    # 1. 优先读取环境变量
    for env in ("HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"):
        val = os.environ.get(env, os.environ.get(env.lower(), "")).strip()
        if val:
            return val

    # 2. 扫描常见本地代理端口
    common_ports = [
        7890,   # Clash / ClashX / Clash Verge (HTTP)
        7897,   # Clash Verge Rev
        7891,   # Clash (SOCKS，部分版本同时开 HTTP)
        6152,   # Surge (HTTP)
        6153,   # Surge (SOCKS)
        1080,   # Shadowsocks / SSR / v2rayN (SOCKS)
        10808,  # v2rayN (SOCKS)
        10809,  # v2rayN (HTTP)
        1082,   # Shadowrocket
        8118,   # Privoxy
        8888,   # Fiddler / Charles
        8889,   # Surge Mac
    ]

    for port in common_ports:
        try:
            _, writer = await asyncio.wait_for(
                asyncio.open_connection("127.0.0.1", port),
                timeout=1.0,
            )
            writer.close()
            await writer.wait_closed()
            return f"http://127.0.0.1:{port}"
        except (OSError, asyncio.TimeoutError):
            continue
    return None


def create_admin_app() -> FastAPI:
    """创建管理后台 FastAPI 应用"""
    app = FastAPI(title="CC Desktop Switch Admin", version="1.0.20")

    @app.middleware("http")
    async def require_local_admin_auth(request: Request, call_next):
        """保护本机管理 API，避免普通网页读取或触发敏感操作。"""
        path = request.url.path
        if path == "/api/ready":
            return await call_next(request)

        if path == "/api/app/activate":
            if request.headers.get(STATIC_REQUEST_HEADER) != "1":
                return JSONResponse(
                    status_code=403,
                    content={"success": False, "message": "Invalid local request"},
                )
            return await call_next(request)

        if path.startswith("/api/"):
            token = request.headers.get(ADMIN_TOKEN_HEADER, "")
            if not verify_admin_token(token):
                return JSONResponse(
                    status_code=403,
                    content={"success": False, "message": "Invalid admin token"},
                )

        return await call_next(request)

    @app.get("/api/ready")
    async def ready():
        """公开最小探活端点，不返回本机配置。"""
        return {"success": True, "ready": True}

    # ── 状态 API ──
    @app.get("/api/status")
    async def get_status():
        """获取全局状态"""
        providers = cfg.get_providers()
        active = cfg.get_active_provider()
        desktop_status = registry.get_config_status()
        settings = cfg.get_settings()
        proxy_port = settings.get("proxyPort", 18080)
        expose_all = False
        target = desktop_config_target_for_provider(active, settings)

        return {
            "desktopConfigured": desktop_status.get("configured", False),
            "proxyRunning": _proxy_running,
            "proxyPort": proxy_port,
            "desktopMode": target["mode"],
            "desktopRequiresProxy": target["requiresProxy"],
            "activeProvider": _public_provider(active),
            "activeProviderId": active["id"] if active else None,
            "providerCount": len(providers),
            "desktopHealth": _desktop_health(desktop_status, proxy_port, active, providers, expose_all),
            "exposeAllProviderModels": expose_all,
        }

    @app.post("/api/app/activate")
    async def activate_app():
        """第二次启动时由新进程调用，用于把已有窗口带回前台。"""
        handled = _activate_existing_app()
        return {"success": True, "handled": handled}

    # ── 提供商 API ──
    @app.get("/api/providers")
    async def list_providers():
        """获取所有提供商"""
        providers = [_public_provider(p) for p in cfg.get_providers()]
        active_id = cfg.load_config().get("activeProvider")
        return {
            "providers": providers,
            "activeId": active_id,
        }

    @app.put("/api/providers/reorder")
    async def reorder_providers(request: Request):
        """保存拖动后的 provider 顺序。"""
        data = await request.json()
        provider_ids = data.get("providerIds", [])
        if not isinstance(provider_ids, list) or not all(isinstance(item, str) for item in provider_ids):
            return JSONResponse(
                status_code=400,
                content={"success": False, "message": "providerIds 必须是字符串数组"},
            )
        if cfg.reorder_providers(provider_ids):
            return {"success": True, "providers": [_public_provider(p) for p in cfg.get_providers()]}
        return JSONResponse(
            status_code=400,
            content={"success": False, "message": "provider 排序保存失败"},
        )

    @app.get("/api/providers/{provider_id}/secret")
    async def get_provider_secret(provider_id: str):
        """读取已保存的 Provider API Key，仅允许本机前端调用。"""
        provider = cfg.get_provider(provider_id)
        if not provider:
            return _provider_not_found()
        return {"apiKey": provider.get("apiKey", "")}

    @app.post("/api/providers")
    async def create_provider(request: Request):
        """添加提供商"""
        data = await request.json()
        provider = cfg.add_provider(data)
        return {"success": True, "provider": _public_provider(provider)}

    @app.put("/api/providers/{provider_id}")
    async def edit_provider(provider_id: str, request: Request):
        """编辑提供商"""
        data = await request.json()
        result = cfg.update_provider(provider_id, data)
        if result:
            return {"success": True, "provider": _public_provider(result)}
        return JSONResponse(
            status_code=404,
            content={"success": False, "message": "提供商不存在"},
        )

    @app.post("/api/providers/{provider_id}/models/{model}/check")
    async def check_provider_model(provider_id: str, model: str):
        """检测指定模型是否可用（通过最小对话请求）"""
        provider = cfg.get_provider(provider_id)
        if not provider:
            return JSONResponse(
                status_code=404,
                content={"success": False, "message": "提供商不存在"},
            )
        result = await provider_tools.check_model_available(provider, model)
        return {"success": True, **result}

    @app.delete("/api/providers/{provider_id}")
    async def remove_provider(provider_id: str):
        """删除提供商"""
        if cfg.delete_provider(provider_id):
            return {"success": True, "message": "已删除"}
        return JSONResponse(
            status_code=404,
            content={"success": False, "message": "提供商不存在"},
        )

    @app.put("/api/providers/{provider_id}/default")
    async def set_default_provider(provider_id: str):
        """设为默认"""
        if cfg.set_active_provider(provider_id):
            try:
                desktop_sync = _sync_desktop_for_active_provider()
            except Exception as exc:
                desktop_sync = {
                    "attempted": True,
                    "success": False,
                    "message": f"桌面版模型同步失败: {exc}",
                }
            return {
                "success": True,
                "message": "默认提供商已更新",
                "desktopSync": desktop_sync,
            }
        return JSONResponse(
            status_code=404,
            content={"success": False, "message": "提供商不存在"},
        )

    @app.post("/api/providers/test")
    async def test_provider_payload(request: Request):
        """测试表单中尚未保存的 provider 连接。"""
        data = await request.json()
        return await _test_provider_connection(data)

    @app.post("/api/providers/detect-format")
    async def detect_provider_format(request: Request):
        """探测第三方 API 的协议类型。"""
        data = await request.json()
        base_url = str(data.get("baseUrl") or "").strip()
        api_key = str(data.get("apiKey") or "").strip()
        if not base_url:
            return JSONResponse(
                status_code=400,
                content={"success": False, "message": "请填写 Base URL"},
            )
        result = await provider_tools.detect_api_format(base_url, api_key)
        if result.get("success"):
            return result
        return JSONResponse(
            status_code=400,
            content=result,
        )

    @app.post("/api/providers/models/available")
    async def get_available_models_from_payload(request: Request):
        """自动获取表单中尚未保存的 provider 模型列表。"""
        data = await request.json()
        result = await provider_tools.fetch_provider_models(data)
        if result.get("success"):
            return result
        return JSONResponse(status_code=400, content=result)

    @app.post("/api/providers/{provider_id}/test")
    async def test_saved_provider(provider_id: str):
        """测试已保存 provider 的连接延迟。"""
        for provider in cfg.get_providers():
            if provider["id"] == provider_id:
                return await _test_provider_connection(provider)
        return JSONResponse(
            status_code=404,
            content={"success": False, "message": "提供商不存在"},
        )

    @app.post("/api/providers/{provider_id}/usage")
    async def query_provider_usage(provider_id: str):
        """查询提供商余额/用量。"""
        provider = cfg.get_provider(provider_id)
        if not provider:
            return _provider_not_found()
        return await provider_tools.query_provider_usage(provider)

    @app.get("/api/providers/compatibility")
    async def provider_compatibility_report():
        """查看已保存 provider 的第三方接口兼容性摘要。"""
        providers = [_provider_compatibility(provider) for provider in cfg.get_providers()]
        return {
            "success": True,
            "providers": providers,
            "experimentalCount": len([item for item in providers if item["level"] == "experimental"]),
        }

    # ── 模型映射 API ──
    @app.get("/api/providers/{provider_id}/models")
    async def get_models(provider_id: str):
        """获取模型映射"""
        providers = cfg.get_providers()
        for p in providers:
            if p["id"] == provider_id:
                return {"models": p.get("models", {})}
        return JSONResponse(
            status_code=404,
            content={"success": False, "message": "提供商不存在"},
        )

    @app.get("/api/providers/{provider_id}/models/available")
    async def get_available_models(provider_id: str):
        """自动获取 provider 支持的模型列表。"""
        provider = cfg.get_provider(provider_id)
        if not provider:
            return _provider_not_found()
        result = await provider_tools.fetch_provider_models(provider)
        if result.get("success"):
            return result
        return JSONResponse(status_code=400, content=result)

    @app.post("/api/providers/{provider_id}/models/autofill")
    async def autofill_models(provider_id: str):
        """自动获取模型列表并写入推荐模型映射。"""
        provider = cfg.get_provider(provider_id)
        if not provider:
            return _provider_not_found()
        result = await provider_tools.fetch_provider_models(provider)
        if not result.get("success"):
            return JSONResponse(status_code=400, content=result)
        if cfg.update_models(provider_id, result.get("suggested", {})):
            return {
                "success": True,
                "models": result.get("models", []),
                "suggested": result.get("suggested", {}),
                "endpoint": result.get("endpoint"),
                "message": "模型映射已自动填充",
            }
        return _provider_not_found()

    @app.put("/api/providers/{provider_id}/models")
    async def save_models(provider_id: str, request: Request):
        """保存模型映射"""
        data = await request.json()
        if cfg.update_models(provider_id, data.get("models", {})):
            return {"success": True, "message": "模型映射已保存"}
        return JSONResponse(
            status_code=404,
            content={"success": False, "message": "提供商不存在"},
        )

    # ── 配置备份 / 导入导出 API ──
    @app.post("/api/config/backup")
    async def create_config_backup():
        """手动创建配置备份。"""
        return {"success": True, "backup": cfg.create_backup("manual")}

    @app.get("/api/config/backups")
    async def list_config_backups():
        """列出配置备份。"""
        return {"backups": cfg.list_backups()}

    @app.get("/api/config/export")
    async def export_config():
        """导出完整配置。会包含 API Key，仅供用户本机下载保存。"""
        return cfg.export_config()

    @app.post("/api/config/import")
    async def import_config(request: Request):
        """导入完整配置。导入前自动备份当前配置。"""
        try:
            data = await request.json()
            result = cfg.import_config(data)
        except ValueError as exc:
            return JSONResponse(
                status_code=400,
                content={"success": False, "message": str(exc)},
            )
        return {
            "success": True,
            "message": "配置已导入",
            "backup": result["backup"],
        }

    # ── 诊断 API ──
    @app.get("/api/diagnostics/summary")
    async def diagnostics_summary():
        """返回可展示的脱敏诊断摘要。"""
        payload = _diagnostics_payload()
        return {
            "success": True,
            "format": payload["format"],
            "summary": {
                "format": payload["format"],
                "app": payload["app"],
                "desktop": payload["desktop"],
                "activeProviderId": payload["activeProviderId"],
                "proxy": {
                    "running": payload["proxy"]["running"],
                    "port": payload["proxy"]["port"],
                    "stats": payload["proxy"]["stats"],
                },
            },
        }

    @app.post("/api/diagnostics/export")
    async def diagnostics_export():
        """导出可发给维护者的脱敏诊断包。"""
        payload = _diagnostics_payload()
        return {"success": True, "format": payload["format"], "diagnostics": payload}

    @app.post("/api/diagnostics/check")
    async def diagnostics_check():
        """执行本机配置状态检查。"""
        payload = _diagnostics_payload()
        checks = _diagnostics_checks(payload)
        return {
            "success": True,
            "format": payload["format"],
            "ok": all(item.get("ok") for item in checks),
            "checks": checks,
        }

    # ── CC-Switch 导入 API ──
    @app.get("/api/ccswitch/status")
    async def get_ccswitch_status():
        """检测本机 CC-Switch 配置。不会返回 API Key。"""
        return {"success": True, **ccswitch_import.status()}

    @app.get("/api/ccswitch/providers")
    async def get_ccswitch_providers():
        """预览可从 CC-Switch 导入的 Claude provider。API Key 只返回掩码。"""
        try:
            providers = ccswitch_import.read_providers()
        except ccswitch_import.CcSwitchImportError as exc:
            return JSONResponse(
                status_code=400,
                content={"success": False, "message": str(exc)},
            )
        return {
            "success": True,
            "providers": providers,
            "supportedCount": len([item for item in providers if item.get("supported")]),
            "unsupportedCount": len([item for item in providers if not item.get("supported")]),
        }

    @app.post("/api/ccswitch/import")
    async def import_ccswitch_providers(request: Request):
        """把 CC-Switch 的 Anthropic 兼容 provider 导入本工具。"""
        data = await request.json() if request.headers.get("content-type") == "application/json" else {}
        try:
            result = ccswitch_import.import_providers(
                ids=data.get("ids"),
                set_default=bool(data.get("setDefault")),
            )
        except ccswitch_import.CcSwitchImportError as exc:
            return JSONResponse(
                status_code=400,
                content={"success": False, "message": str(exc)},
            )
        return {
            "success": True,
            "message": f"已导入 {len(result['imported'])} 个 CC-Switch 配置",
            **result,
        }

    # ── Desktop 集成 API ──
    @app.get("/api/desktop/status")
    async def get_desktop_status():
        """获取 Desktop 注册表配置状态"""
        status = registry.get_config_status()
        settings = cfg.get_settings()
        proxy_port = settings.get("proxyPort", 18080)
        status["health"] = _desktop_health(
            status,
            proxy_port,
            cfg.get_active_provider(),
            cfg.get_providers(),
            False,
        )
        return status

    @app.post("/api/desktop/configure")
    async def apply_desktop_config(request: Request):
        """应用 Desktop 配置到注册表"""
        data = await request.json() if request.headers.get("content-type") == "application/json" else {}
        active_provider = cfg.get_active_provider()
        settings = cfg.get_settings()
        if data.get("port"):
            settings = dict(settings)
            settings["proxyPort"] = int(data["port"])
        target = desktop_config_target_for_provider(active_provider, settings)
        proxy_port = int(settings.get("proxyPort", 18080))
        if not registry.provider_inference_models(active_provider):
            return _no_desktop_model_routes_response(target, proxy_port)
        result = registry.apply_config(
            target["baseUrl"],
            gateway_api_key=target["apiKey"],
            provider=target["provider"],
            providers=target["providers"],
            expose_all=target["exposeAll"],
            auth_scheme=target["authScheme"],
            gateway_headers=target["gatewayHeaders"],
        )
        response = {
            **result,
            "configSuccess": bool(result.get("success")),
            "mode": target["mode"],
            "requiresProxy": target["requiresProxy"],
            "proxyStarted": False,
            "proxyPort": proxy_port,
        }
        if result.get("success") and target["requiresProxy"]:
            proxy_started = _start_proxy_server(proxy_port)
            response["proxyStarted"] = bool(proxy_started)
            if not proxy_started:
                response["success"] = False
                response["message"] = (
                    "桌面版配置已写入，但本机转发服务启动失败。"
                    "请检查转发端口是否被占用，或在设置中更换转发端口后重试。"
                )
        elif result.get("success"):
            response["proxyStarted"] = bool(_proxy_running)
        return response

    @app.post("/api/desktop/clear")
    async def clear_desktop_config():
        """清除 Desktop 注册表配置"""
        return registry.clear_config()

    # ── 代理 API ──
    @app.get("/api/proxy/status")
    async def get_proxy_status():
        """获取代理状态"""
        return {
            "running": _proxy_running,
            "port": _proxy_port or cfg.get_settings().get("proxyPort", 18080),
            "stats": proxy_stats.to_dict(),
        }

    @app.post("/api/proxy/start")
    async def start_proxy(request: Request):
        """启动代理"""
        global _proxy_running
        data = await request.json() if request.headers.get("content-type") == "application/json" else {}
        requested_port = data.get("port")
        if requested_port:
            cfg.update_settings({"proxyPort": int(requested_port)})

        if _proxy_running:
            return {"success": True, "message": "代理已在运行中", "port": _proxy_port or cfg.get_settings().get("proxyPort", 18080)}

        port = cfg.get_settings().get("proxyPort", 18080)
        success = _start_proxy_server(port)
        if success:
            return {"success": True, "message": f"代理已启动，端口: {port}", "port": port}
        return JSONResponse(
            status_code=500,
            content={"success": False, "message": "代理启动失败"},
        )

    @app.post("/api/proxy/stop")
    async def stop_proxy():
        """停止代理"""
        global _proxy_running
        if not _proxy_running:
            return {"success": True, "message": "代理未在运行"}

        _stop_proxy_server()
        return {"success": True, "message": "代理已停止"}

    @app.get("/api/proxy/logs")
    async def get_proxy_logs():
        """获取代理日志"""
        return {"logs": proxy_logs.get_all()}

    @app.post("/api/proxy/logs/clear")
    async def clear_proxy_logs():
        """清除代理日志"""
        proxy_logs.clear()
        return {"success": True}

    # ── 设置 API ──
    @app.get("/api/settings")
    async def get_settings():
        """获取设置"""
        return cfg.get_settings()

    @app.put("/api/settings")
    async def save_settings(request: Request):
        """保存设置"""
        data = await request.json()
        settings = cfg.update_settings(data)
        return {"success": True, "settings": settings}

    @app.get("/api/proxy/detect")
    async def detect_local_proxy():
        """自动检测本地代理端口"""
        detected = await _detect_local_proxy()
        return {"detected": detected or ""}

    @app.get("/api/update/check")
    async def check_update(url: Optional[str] = None, current: Optional[str] = None, platform: Optional[str] = None):
        """检查最新版本，不自动下载或安装。"""
        settings = cfg.get_settings()
        update_url = url or settings.get("updateUrl") or cfg.DEFAULT_UPDATE_URL
        if not update_url:
            return JSONResponse(
                status_code=400,
                content={"success": False, "message": "请先配置 latest.json 更新地址"},
            )
        try:
            return await updater.check_update(
                url=update_url,
                current_version=current or cfg.DEFAULT_CONFIG.get("version", "1.0.0"),
                platform=platform or updater.current_platform(),
            )
        except updater.UpdateCheckError as exc:
            return JSONResponse(
                status_code=400,
                content={"success": False, "message": str(exc)},
            )

    @app.post("/api/update/install")
    async def download_and_install_update(request: Request):
        """下载最新安装包并启动安装器。"""
        data = await request.json() if request.headers.get("content-type") == "application/json" else {}
        settings = cfg.get_settings()
        update_url = data.get("url") or settings.get("updateUrl") or cfg.DEFAULT_UPDATE_URL
        platform = data.get("platform") or updater.current_platform()
        try:
            result = await updater.download_update(
                url=update_url,
                current_version=data.get("current") or cfg.DEFAULT_CONFIG.get("version", "1.0.0"),
                platform=platform,
            )
            if not result.get("updateAvailable"):
                return result
            installer_path = result.get("installerPath")
            if not installer_path:
                raise updater.UpdateCheckError("下载安装包失败")
            resolved_platform = result.get("platform") or platform
            quit_requested = _launch_update_installer(installer_path, resolved_platform)
            is_macos = resolved_platform.startswith("macos-")
            return {
                **result,
                "success": True,
                "installerStarted": True,
                "quitRequested": quit_requested,
                "message": (
                    (
                        "更新包已下载，应用即将退出并启动安装器。"
                        if quit_requested
                        else "更新包已下载并打开。请先退出当前应用，再按 macOS 提示完成安装。"
                    )
                    if is_macos
                    else "安装包已下载并启动。安装器会沿用旧安装目录，并在安装前关闭正在运行的 CC Desktop Switch。"
                ),
            }
        except updater.UpdateCheckError as exc:
            return JSONResponse(
                status_code=400,
                content={"success": False, "message": str(exc)},
            )
        except OSError as exc:
            return JSONResponse(
                status_code=500,
                content={"success": False, "message": f"启动安装器失败: {exc}"},
            )

    @app.get("/api/update/progress")
    async def get_update_progress():
        """返回当前更新下载进度，供前端轮询。"""
        return updater.get_download_progress()

    # ── 预设 API ──
    @app.get("/api/presets")
    async def get_presets():
        """获取内置预设"""
        return {"presets": cfg.get_presets()}

    # ── 挂载前端静态文件 ──
    # 必须放在 API 路由之后，否则 "/" 挂载会先匹配 /api/* 并返回静态 404。
    if FRONTEND_DIR.exists():
        frontend_static = StaticFiles(directory=str(FRONTEND_DIR), html=True)
        app.mount("/", frontend_static, name="frontend")

    return app


# ── 代理服务器管理 ──
_proxy_running = False
_proxy_thread: Optional[threading.Thread] = None
_proxy_server = None
_proxy_port: Optional[int] = None


def _wait_for_proxy_server_start(server, thread: threading.Thread, port: int, timeout: float = 2.0) -> bool:
    """等待 uvicorn 完成监听，避免端口冲突时误报启动成功。"""
    deadline = time.perf_counter() + timeout
    has_started_flag = hasattr(server, "started")
    while time.perf_counter() < deadline:
        if has_started_flag and getattr(server, "started", False):
            return True
        if not thread.is_alive():
            return False
        if not has_started_flag:
            try:
                with socket.create_connection(("127.0.0.1", int(port)), timeout=0.1):
                    return True
            except OSError:
                pass
        time.sleep(0.05)
    if has_started_flag:
        return bool(getattr(server, "started", False))
    return bool(thread.is_alive())


def _start_proxy_server(port: int) -> bool:
    """在新线程中启动代理服务器"""
    global _proxy_running, _proxy_thread, _proxy_server, _proxy_port

    requested_port = int(port)
    if _proxy_running:
        if _proxy_port == requested_port:
            return True
        _stop_proxy_server()
        if _proxy_thread and _proxy_thread.is_alive():
            _proxy_thread.join(timeout=1.0)

    proxy_app = create_proxy_app()

    config = uvicorn.Config(
        proxy_app,
        host="127.0.0.1",
        port=requested_port,
        log_level="warning",
        access_log=False,
        log_config=None,
    )
    server = uvicorn.Server(config)
    _proxy_server = server

    def run():
        global _proxy_running, _proxy_port
        try:
            server.run()
        finally:
            if _proxy_server is server:
                _proxy_running = False
                _proxy_port = None

    _proxy_thread = threading.Thread(target=run, daemon=True)
    _proxy_thread.start()
    _proxy_running = _wait_for_proxy_server_start(server, _proxy_thread, requested_port)
    if not _proxy_running:
        server.should_exit = True
        _proxy_port = None
    else:
        _proxy_port = requested_port
    return _proxy_running


def _stop_proxy_server():
    """停止代理服务器"""
    global _proxy_running, _proxy_server, _proxy_port
    if _proxy_server:
        _proxy_server.should_exit = True
    _proxy_running = False
    _proxy_port = None
