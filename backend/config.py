"""配置管理 - JSON 配置文件读写"""

import json
import os
import secrets
import shutil
import copy
from datetime import datetime
from typing import Optional

CONFIG_DIR = os.path.expanduser("~/.cc-desktop-switch")
CONFIG_FILE = os.path.join(CONFIG_DIR, "config.json")
BACKUP_DIR = os.path.join(CONFIG_DIR, "backups")
DEFAULT_UPDATE_URL = "https://github.com/lonr-6/cc-desktop-switch/releases/latest/download/latest.json"

DEFAULT_CONFIG = {
    "version": "1.0.6",
    "activeProvider": None,
    "gatewayApiKey": None,
    "providers": [],
    "settings": {
        "theme": "light",
        "language": "zh",
        "proxyPort": 18080,
        "adminPort": 18081,
        "autoStart": False,
        "updateUrl": DEFAULT_UPDATE_URL,
    },
}

BUILTIN_PRESETS = [
    {
        "id": "deepseek",
        "name": "DeepSeek",
        "baseUrl": "https://api.deepseek.com/anthropic",
        "authScheme": "bearer",
        "apiFormat": "anthropic",
        "models": {
            "sonnet": "deepseek-v4-pro",
            "haiku": "deepseek-v4-flash",
            "opus": "deepseek-v4-pro",
            "default": "deepseek-v4-pro",
        },
        "modelOptions": {
            "deepseek_1m": {
                "label": "解锁 1M 上下文",
                "description": "用于 Claude Code/长上下文场景。开启后 Sonnet、Opus 和默认模型使用 deepseek-v4-pro[1m]。",
                "models": {
                    "sonnet": "deepseek-v4-pro[1m]",
                    "haiku": "deepseek-v4-flash",
                    "opus": "deepseek-v4-pro[1m]",
                    "default": "deepseek-v4-pro[1m]",
                },
                "modelCapabilities": {
                    "deepseek-v4-pro[1m]": {"supports1m": True},
                },
            }
        },
        "requestOptions": {},
        "requestOptionPresets": {
            "deepseek_max_effort": {
                "label": "DeepSeek Max 思维",
                "description": "Low：更快更省，适合简单任务。\nMedium：速度和效果平衡，适合日常使用。\nHigh：更认真思考，适合复杂代码和排错。\n勾选后：本工具会按 DeepSeek Max 转发；未勾选则使用 Claude 当前默认配置。",
                "requestOptions": {
                    "anthropic": {
                        "thinking": {"type": "enabled"},
                        "output_config": {"effort": "max"},
                    }
                },
            }
        },
        "extraHeaders": {"x-api-key": "{apiKey}"},
        "isBuiltin": True,
    },
    {
        "id": "kimi",
        "name": "Kimi (月之暗面)",
        "baseUrl": "https://api.moonshot.ai/anthropic",
        "authScheme": "bearer",
        "apiFormat": "anthropic",
        "models": {
            "sonnet": "kimi-k2.6",
            "haiku": "kimi-k2.6",
            "opus": "kimi-k2.6",
            "default": "kimi-k2.6",
        },
        "isBuiltin": True,
    },
    {
        "id": "zhipu",
        "name": "智谱 GLM",
        "baseUrl": "https://open.bigmodel.cn/api/anthropic",
        "authScheme": "x-api-key",
        "apiFormat": "anthropic",
        "models": {
            "sonnet": "glm-5.1",
            "haiku": "glm-4.7",
            "opus": "glm-5.1",
            "default": "glm-5.1",
        },
        "isBuiltin": True,
    },
    {
        "id": "bailian",
        "name": "阿里云百炼",
        "baseUrl": "https://dashscope.aliyuncs.com/apps/anthropic",
        "authScheme": "x-api-key",
        "apiFormat": "anthropic",
        "models": {
            "sonnet": "qwen3.6-plus",
            "haiku": "qwen3.6-flash",
            "opus": "qwen3.6-max-preview",
            "default": "qwen3.6-plus",
        },
        "modelOptions": {
            "qwen_1m": {
                "label": "开启千问 1M 上下文",
                "description": "阿里云文档确认 qwen3.6-plus / qwen3.6-flash 支持 1M。勾选后会把 1M 能力写入 Claude 桌面版；不勾选则按普通上下文显示。",
                "modelCapabilities": {
                    "qwen3.6-plus": {"supports1m": True},
                    "qwen3.6-flash": {"supports1m": True},
                },
            }
        },
        "modelCapabilities": {},
        "requestOptions": {},
        "isBuiltin": True,
    },
]


def ensure_config_dir():
    """确保配置目录存在"""
    os.makedirs(CONFIG_DIR, exist_ok=True)


def ensure_backup_dir():
    """确保配置备份目录存在"""
    ensure_config_dir()
    os.makedirs(BACKUP_DIR, exist_ok=True)


def load_config() -> dict:
    """加载配置文件"""
    ensure_config_dir()
    if not os.path.exists(CONFIG_FILE):
        return copy.deepcopy(DEFAULT_CONFIG)
    try:
        with open(CONFIG_FILE, "r", encoding="utf-8") as f:
            return json.load(f)
    except (json.JSONDecodeError, IOError):
        return copy.deepcopy(DEFAULT_CONFIG)


def save_config(config: dict):
    """保存配置文件"""
    ensure_config_dir()
    # 原子写入：先写临时文件，再重命名
    tmp_file = CONFIG_FILE + ".tmp"
    with open(tmp_file, "w", encoding="utf-8") as f:
        json.dump(config, f, ensure_ascii=False, indent=2)
    shutil.move(tmp_file, CONFIG_FILE)


def _normalize_provider(provider: dict) -> dict:
    """补齐 provider 必要字段，导入旧配置时保持兼容。"""
    normalized = dict(provider)
    provider_id = str(normalized.get("id") or "")
    safe_id = "".join(ch for ch in provider_id if ch.isalnum() or ch in {"-", "_"})[:64]
    normalized["id"] = safe_id or secrets.token_hex(4)
    normalized.setdefault("name", "Unnamed Provider")
    normalized.setdefault("baseUrl", "")
    normalized.setdefault("authScheme", "bearer")
    normalized.setdefault("apiFormat", "anthropic")
    normalized.setdefault("apiKey", "")
    normalized.setdefault("extraHeaders", {})
    normalized.setdefault("modelCapabilities", {})
    normalized.setdefault("requestOptions", {})
    normalized.setdefault("isBuiltin", False)
    normalized.setdefault("sortIndex", 0)
    normalized.setdefault("models", {
        "sonnet": "",
        "haiku": "",
        "opus": "",
        "default": "",
    })
    return normalized


def normalize_config(config: dict) -> dict:
    """把外部导入的配置整理成当前版本可读取的结构。"""
    if not isinstance(config, dict):
        raise ValueError("配置文件必须是 JSON 对象")

    source = config.get("config") if isinstance(config.get("config"), dict) else config
    normalized = copy.deepcopy(DEFAULT_CONFIG)
    normalized.update({k: v for k, v in source.items() if k in normalized})
    normalized["version"] = source.get("version", DEFAULT_CONFIG["version"])

    settings = dict(DEFAULT_CONFIG["settings"])
    imported_settings = source.get("settings", {})
    if isinstance(imported_settings, dict):
        settings.update(imported_settings)
    normalized["settings"] = settings

    providers = source.get("providers", [])
    if not isinstance(providers, list):
        raise ValueError("providers 必须是数组")
    normalized_providers = []
    seen_ids = set()
    for provider in providers:
        if not isinstance(provider, dict):
            continue
        normalized_provider = _normalize_provider(provider)
        if normalized_provider["id"] in seen_ids:
            normalized_provider["id"] = f"{normalized_provider['id']}-{secrets.token_hex(2)}"
        seen_ids.add(normalized_provider["id"])
        normalized_providers.append(normalized_provider)
    normalized["providers"] = normalized_providers

    provider_ids = {p["id"] for p in normalized["providers"]}
    active_provider = source.get("activeProvider")
    if active_provider in provider_ids:
        normalized["activeProvider"] = active_provider
    else:
        normalized["activeProvider"] = normalized["providers"][0]["id"] if normalized["providers"] else None

    if source.get("gatewayApiKey"):
        normalized["gatewayApiKey"] = source["gatewayApiKey"]

    return normalized


def create_backup(reason: str = "manual") -> dict:
    """备份当前配置文件，返回备份文件元数据。"""
    ensure_backup_dir()
    if not os.path.exists(CONFIG_FILE):
        save_config(load_config())

    safe_reason = "".join(ch for ch in str(reason or "manual").lower() if ch.isalnum() or ch in {"-", "_"})[:32]
    timestamp = datetime.now().strftime("%Y%m%d-%H%M%S-%f")
    filename = f"config-{timestamp}-{safe_reason or 'manual'}-{secrets.token_hex(2)}.json"
    target = os.path.join(BACKUP_DIR, filename)
    shutil.copy2(CONFIG_FILE, target)
    stat = os.stat(target)
    return {
        "name": filename,
        "size": stat.st_size,
        "createdAt": datetime.fromtimestamp(stat.st_mtime).isoformat(timespec="seconds"),
    }


def list_backups() -> list:
    """列出配置备份。"""
    ensure_backup_dir()
    backups = []
    for name in os.listdir(BACKUP_DIR):
        if not name.endswith(".json"):
            continue
        path = os.path.join(BACKUP_DIR, name)
        if not os.path.isfile(path):
            continue
        stat = os.stat(path)
        backups.append({
            "name": name,
            "size": stat.st_size,
            "createdAt": datetime.fromtimestamp(stat.st_mtime).isoformat(timespec="seconds"),
        })
    return sorted(backups, key=lambda item: item["createdAt"], reverse=True)


def export_config() -> dict:
    """导出完整配置。包含 API Key，仅供用户本机保存。"""
    return {
        "format": "cc-desktop-switch.config",
        "exportedAt": datetime.now().isoformat(timespec="seconds"),
        "config": load_config(),
    }


def import_config(data: dict) -> dict:
    """导入配置。导入前自动备份当前配置。"""
    backup = create_backup("before-import")
    normalized = normalize_config(data)
    save_config(normalized)
    return {"config": normalized, "backup": backup}


def get_or_create_gateway_api_key() -> str:
    """获取本地 gateway 认证密钥，没有则生成一个。

    这个密钥写入 Claude Desktop 的 managed policy，用于满足 gateway 模式的
    必填凭据要求。它不是上游提供商 API Key。
    """
    config = load_config()
    key = config.get("gatewayApiKey")
    if not key:
        key = "ccds_" + secrets.token_urlsafe(32)
        config["gatewayApiKey"] = key
        save_config(config)
    return key


def get_gateway_api_key() -> Optional[str]:
    """读取本地 gateway 认证密钥，不存在时不自动创建。"""
    return load_config().get("gatewayApiKey")


def get_providers() -> list:
    """获取所有提供商列表"""
    config = load_config()
    return config.get("providers", [])


def get_provider(provider_id: str) -> Optional[dict]:
    """按 ID 获取提供商"""
    for provider in get_providers():
        if provider.get("id") == provider_id:
            return provider
    return None


def get_active_provider() -> Optional[dict]:
    """获取当前激活的提供商"""
    config = load_config()
    active_id = config.get("activeProvider")
    if not active_id:
        providers = config.get("providers", [])
        return providers[0] if providers else None
    for p in config.get("providers", []):
        if p["id"] == active_id:
            return p
    return None


def add_provider(provider: dict) -> dict:
    """添加提供商"""
    config = load_config()
    providers = config.get("providers", [])

    # 生成唯一 ID
    import uuid
    provider = _normalize_provider(provider)
    existing_ids = {p.get("id") for p in providers}
    candidate_id = provider.get("id") or str(uuid.uuid4())[:8]
    while candidate_id in existing_ids:
        candidate_id = f"{provider.get('id') or 'provider'}-{secrets.token_hex(2)}"
    provider["id"] = candidate_id
    provider["sortIndex"] = len(providers)

    providers.append(provider)
    config["providers"] = providers

    # 如果是第一个提供商，自动设为默认
    if len(providers) == 1:
        config["activeProvider"] = provider["id"]

    save_config(config)
    return provider


def update_provider(provider_id: str, data: dict) -> Optional[dict]:
    """更新提供商"""
    config = load_config()
    for i, p in enumerate(config.get("providers", [])):
        if p["id"] == provider_id:
            updated = dict(p)
            updated.update(data)
            updated["id"] = provider_id
            updated["isBuiltin"] = p.get("isBuiltin", False)

            # 编辑表单中 API Key 留空表示“不修改”，避免误清空已保存密钥。
            if not data.get("apiKey"):
                updated["apiKey"] = p.get("apiKey", "")

            # preset 的额外认证头也要保留，例如 DeepSeek 的 x-api-key。
            if "extraHeaders" not in data or data.get("extraHeaders") in (None, {}):
                updated["extraHeaders"] = p.get("extraHeaders", {})

            if "modelCapabilities" not in data:
                updated["modelCapabilities"] = p.get("modelCapabilities", {})

            if "requestOptions" not in data:
                updated["requestOptions"] = p.get("requestOptions", {})

            if "models" in data and isinstance(data["models"], dict):
                merged_models = dict(p.get("models", {}))
                merged_models.update(data["models"])
                updated["models"] = merged_models

            config["providers"][i] = updated
            save_config(config)
            return updated
    return None


def delete_provider(provider_id: str) -> bool:
    """删除提供商"""
    config = load_config()
    original_len = len(config.get("providers", []))
    config["providers"] = [p for p in config.get("providers", []) if p["id"] != provider_id]

    if len(config["providers"]) == original_len:
        return False

    # 如果删除的是当前激活的，切换到第一个可用的
    if config.get("activeProvider") == provider_id:
        config["activeProvider"] = config["providers"][0]["id"] if config["providers"] else None

    for index, provider in enumerate(config["providers"]):
        provider["sortIndex"] = index

    save_config(config)
    return True


def set_active_provider(provider_id: str) -> bool:
    """设置默认提供商"""
    config = load_config()
    for p in config.get("providers", []):
        if p["id"] == provider_id:
            config["activeProvider"] = provider_id
            save_config(config)
            return True
    return False


def update_models(provider_id: str, models: dict) -> bool:
    """更新模型映射"""
    config = load_config()
    for p in config.get("providers", []):
        if p["id"] == provider_id:
            p["models"] = models
            save_config(config)
            return True
    return False


def reorder_providers(provider_ids: list[str]) -> bool:
    """按照前端拖动后的 ID 顺序保存 providers。"""
    config = load_config()
    providers = config.get("providers", [])
    by_id = {provider.get("id"): provider for provider in providers}
    ordered = []
    seen = set()
    for provider_id in provider_ids:
        provider = by_id.get(provider_id)
        if provider and provider_id not in seen:
            ordered.append(provider)
            seen.add(provider_id)
    ordered.extend(provider for provider in providers if provider.get("id") not in seen)
    if len(ordered) != len(providers):
        return False
    for index, provider in enumerate(ordered):
        provider["sortIndex"] = index
    config["providers"] = ordered
    save_config(config)
    return True


def get_settings() -> dict:
    """获取设置"""
    config = load_config()
    settings = dict(DEFAULT_CONFIG["settings"])
    settings.update(config.get("settings", {}))
    if not settings.get("updateUrl"):
        settings["updateUrl"] = DEFAULT_UPDATE_URL
    return settings


def update_settings(settings: dict) -> dict:
    """更新设置"""
    config = load_config()
    current = dict(DEFAULT_CONFIG["settings"])
    current.update(config.get("settings", {}))
    current.update(settings)
    if not current.get("updateUrl"):
        current["updateUrl"] = DEFAULT_UPDATE_URL
    config["settings"] = current
    save_config(config)
    return current


def get_presets() -> list:
    """获取内置预设列表"""
    return BUILTIN_PRESETS
