"""配置管理 - JSON 配置文件读写"""

import copy
import json
import os
import secrets
import shutil
from datetime import datetime
from typing import Optional

from backend.model_alias import model_mappings_with_legacy_aliases, normalize_model_mappings
from backend.preset_loader import load_builtin_presets


def _resolve_config_paths() -> tuple[str, str, str]:
    override = os.environ.get("CCDS_CONFIG_DIR", "").strip()
    config_dir = os.path.abspath(os.path.expanduser(override or "~/.cc-desktop-switch"))
    return (
        config_dir,
        os.path.join(config_dir, "config.json"),
        os.path.join(config_dir, "backups"),
    )


CONFIG_DIR, CONFIG_FILE, BACKUP_DIR = _resolve_config_paths()
DEFAULT_UPDATE_URL = "https://github.com/lonr-6/cc-desktop-switch/releases/latest/download/latest.json"

DEFAULT_CONFIG = {
    "version": "1.0.25",
    "activeProvider": None,
    "gatewayApiKey": None,
    "providers": [],
    "settings": {
        "theme": "default",
        "language": "zh",
        "proxyPort": 18080,
        "adminPort": 18081,
        "autoStart": False,
        "exposeAllProviderModels": False,
        "updateUrl": DEFAULT_UPDATE_URL,
        "upstreamProxy": "",
        "upstreamProxyEnabled": False,
        "enableBillingHeaderRectifier": True,
    },
}


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
        return _config_with_legacy_model_aliases(copy.deepcopy(DEFAULT_CONFIG))
    try:
        with open(CONFIG_FILE, "r", encoding="utf-8-sig") as f:
            raw = json.load(f)
    except (json.JSONDecodeError, IOError):
        return _config_with_legacy_model_aliases(copy.deepcopy(DEFAULT_CONFIG))
    return _config_with_legacy_model_aliases(normalize_config(raw))


def save_config(config: dict):
    """保存配置文件"""
    ensure_config_dir()
    normalized = normalize_config(config)
    # 原子写入：先写临时文件，再重命名
    tmp_file = CONFIG_FILE + ".tmp"
    with open(tmp_file, "w", encoding="utf-8") as f:
        json.dump(normalized, f, ensure_ascii=False, indent=2)
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
    normalized["models"] = normalize_model_mappings(normalized.get("models"))
    return normalized


def _provider_with_legacy_model_aliases(provider: dict) -> dict:
    compat = copy.deepcopy(provider)
    compat["models"] = model_mappings_with_legacy_aliases(compat.get("models"))
    return compat


def _config_with_legacy_model_aliases(config: dict) -> dict:
    compat = copy.deepcopy(config)
    providers = compat.get("providers", [])
    if isinstance(providers, list):
        compat["providers"] = [
            _provider_with_legacy_model_aliases(provider)
            if isinstance(provider, dict) else provider
            for provider in providers
        ]
    return compat


def _preset_with_legacy_model_aliases(preset: dict) -> dict:
    compat = _provider_with_legacy_model_aliases(_normalize_provider(copy.deepcopy(preset)))
    model_options = compat.get("modelOptions")
    if isinstance(model_options, dict):
        for option in model_options.values():
            if isinstance(option, dict) and isinstance(option.get("models"), dict):
                option["models"] = model_mappings_with_legacy_aliases(option.get("models"))
    return compat


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
    return _provider_with_legacy_model_aliases(provider)


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
                updated["models"] = normalize_model_mappings(merged_models)

            config["providers"][i] = updated
            save_config(config)
            return _provider_with_legacy_model_aliases(updated)
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
            p["models"] = normalize_model_mappings(models)
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
    # 统一模型菜单暂不开放，避免不同厂商能力混在一起导致 1M / 思维深度失效。
    settings["exposeAllProviderModels"] = False
    if not settings.get("updateUrl"):
        settings["updateUrl"] = DEFAULT_UPDATE_URL
    return settings


def update_settings(settings: dict) -> dict:
    """更新设置"""
    config = load_config()
    current = dict(DEFAULT_CONFIG["settings"])
    current.update(config.get("settings", {}))
    current.update(settings)
    current["exposeAllProviderModels"] = False
    if not current.get("updateUrl"):
        current["updateUrl"] = DEFAULT_UPDATE_URL
    config["settings"] = current
    save_config(config)
    return current


def get_presets() -> list:
    """获取内置预设列表"""
    return [_preset_with_legacy_model_aliases(preset) for preset in load_builtin_presets()]
