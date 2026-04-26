"""FastAPI 应用 - 管理 API + 静态文件服务"""

import json
import subprocess
import threading
import time
from urllib.parse import urlparse

import httpx
import uvicorn
from pathlib import Path
from typing import Optional

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse
from fastapi.staticfiles import StaticFiles

from backend import config as cfg
from backend import provider_tools
from backend import registry
from backend import update as updater
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


def _desktop_health(desktop_status: dict, proxy_port: int, provider: Optional[dict]) -> dict:
    """判断 Claude Desktop 配置是否仍指向本工具当前 provider。"""
    keys = desktop_status.get("keys") or {}
    expected_base_url = f"http://127.0.0.1:{proxy_port}"
    actual_base_url = str(keys.get("inferenceGatewayBaseUrl") or "").rstrip("/")
    issues = []

    if actual_base_url and actual_base_url != expected_base_url:
        issues.append({
            "code": "gateway_base_url_mismatch",
            "message": "Claude 桌面版仍指向旧地址，请重新一键应用到 Claude 桌面版。",
        })

    if desktop_status.get("configured") is False and keys:
        issues.append({
            "code": "not_managed_by_ccds",
            "message": "当前桌面版配置不是由本工具最新版本写入。",
        })

    inference_models = _parse_inference_models(str(keys.get("inferenceModels") or ""))
    provider_models = provider.get("models", {}) if provider else {}
    model_capabilities = provider.get("modelCapabilities", {}) if provider else {}
    if not isinstance(model_capabilities, dict):
        model_capabilities = {}
    one_million_models = [
        str(model_id)
        for model_id in provider_models.values()
        if (
            isinstance(model_id, str)
            and (
                "[1m]" in model_id.lower()
                or (
                    isinstance(model_capabilities.get(model_id), dict)
                    and model_capabilities[model_id].get("supports1m") is True
                )
            )
        )
    ]
    one_million_ready = True
    if one_million_models:
        one_million_ready = False
        for item in inference_models:
            if (
                isinstance(item, dict)
                and item.get("name") in one_million_models
                and item.get("supports1m") is True
            ):
                one_million_ready = True
                break
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
        "issues": issues,
    }


def _sync_desktop_for_active_provider() -> dict:
    """默认 provider 切换后，同步本工具管理的 Claude 桌面版模型列表。"""
    provider = cfg.get_active_provider()
    if not provider:
        return {"attempted": False, "success": False, "message": "没有默认提供商"}

    status = registry.get_config_status()
    if not status.get("configured"):
        return {"attempted": False, "success": True, "message": "Claude 桌面版尚未由本工具配置"}

    proxy_port = cfg.get_settings().get("proxyPort", 18080)
    result = registry.apply_config(
        f"http://127.0.0.1:{proxy_port}",
        gateway_api_key=cfg.get_or_create_gateway_api_key(),
        provider=provider,
    )
    return {
        "attempted": True,
        **result,
    }


def _provider_test_model(provider: dict) -> str:
    models = provider.get("models") or {}
    if isinstance(models, dict):
        for key in ("default", "sonnet", "haiku", "opus"):
            model = models.get(key)
            if model:
                return model
    return "claude-sonnet-4-6"


def _provider_test_body(provider: dict, api_format: str) -> dict:
    model = _provider_test_model(provider)
    if api_format == "openai":
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


async def _test_provider_connection(provider: dict) -> dict:
    """测试 provider 是否能真实访问上游接口。"""
    api_format = str(provider.get("apiFormat", "anthropic")).lower()
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


def create_admin_app() -> FastAPI:
    """创建管理后台 FastAPI 应用"""
    app = FastAPI(title="CC Desktop Switch Admin", version="1.0.7")

    @app.middleware("http")
    async def require_app_header_for_writes(request: Request, call_next):
        """阻止普通网页表单跨站触发本地写操作。"""
        sensitive_read = (
            request.url.path == "/api/config/export"
            or (
                request.url.path.startswith("/api/providers/")
                and request.url.path.endswith("/secret")
            )
        )
        if (
            request.url.path.startswith("/api/")
            and (
                request.method not in {"GET", "HEAD", "OPTIONS"}
                or sensitive_read
            )
            and request.headers.get("x-ccds-request") != "1"
        ):
            return JSONResponse(
                status_code=403,
                content={"success": False, "message": "Invalid local request"},
            )
        return await call_next(request)

    # ── 状态 API ──
    @app.get("/api/status")
    async def get_status():
        """获取全局状态"""
        providers = cfg.get_providers()
        active = cfg.get_active_provider()
        desktop_status = registry.get_config_status()
        proxy_port = cfg.get_settings().get("proxyPort", 18080)

        return {
            "desktopConfigured": desktop_status.get("configured", False),
            "proxyRunning": _proxy_running,
            "proxyPort": proxy_port,
            "activeProvider": _public_provider(active),
            "activeProviderId": active["id"] if active else None,
            "providerCount": len(providers),
            "desktopHealth": _desktop_health(desktop_status, proxy_port, active),
        }

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

    # ── Desktop 集成 API ──
    @app.get("/api/desktop/status")
    async def get_desktop_status():
        """获取 Desktop 注册表配置状态"""
        status = registry.get_config_status()
        proxy_port = cfg.get_settings().get("proxyPort", 18080)
        status["health"] = _desktop_health(status, proxy_port, cfg.get_active_provider())
        return status

    @app.post("/api/desktop/configure")
    async def apply_desktop_config(request: Request):
        """应用 Desktop 配置到注册表"""
        data = await request.json() if request.headers.get("content-type") == "application/json" else {}
        proxy_port = data.get("port", cfg.get_settings().get("proxyPort", 18080))
        base_url = f"http://127.0.0.1:{proxy_port}"
        gateway_api_key = cfg.get_or_create_gateway_api_key()
        active_provider = cfg.get_active_provider()
        return registry.apply_config(
            base_url,
            gateway_api_key=gateway_api_key,
            provider=active_provider,
        )

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
            "port": cfg.get_settings().get("proxyPort", 18080),
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
            return {"success": True, "message": "代理已在运行中"}

        port = cfg.get_settings().get("proxyPort", 18080)
        success = _start_proxy_server(port)
        if success:
            return {"success": True, "message": f"代理已启动，端口: {port}"}
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

    @app.get("/api/update/check")
    async def check_update(url: Optional[str] = None, current: Optional[str] = None):
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
        try:
            result = await updater.download_update(
                url=update_url,
                current_version=data.get("current") or cfg.DEFAULT_CONFIG.get("version", "1.0.0"),
            )
            if not result.get("updateAvailable"):
                return result
            installer_path = result.get("installerPath")
            if not installer_path:
                raise updater.UpdateCheckError("下载安装包失败")
            subprocess.Popen([installer_path], close_fds=True)
            return {
                **result,
                "success": True,
                "installerStarted": True,
                "message": "安装包已下载并启动。按安装器提示完成更新；如提示文件占用，请先退出当前应用。",
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


def _start_proxy_server(port: int) -> bool:
    """在新线程中启动代理服务器"""
    global _proxy_running, _proxy_thread, _proxy_server

    if _proxy_running:
        return True

    proxy_app = create_proxy_app()

    config = uvicorn.Config(
        proxy_app,
        host="127.0.0.1",
        port=port,
        log_level="warning",
        access_log=False,
        log_config=None,
    )
    _proxy_server = uvicorn.Server(config)

    def run():
        global _proxy_running
        _proxy_running = True
        _proxy_server.run()
        _proxy_running = False

    _proxy_thread = threading.Thread(target=run, daemon=True)
    _proxy_thread.start()
    return True


def _stop_proxy_server():
    """停止代理服务器"""
    global _proxy_running, _proxy_server
    if _proxy_server:
        _proxy_server.should_exit = True
    _proxy_running = False
