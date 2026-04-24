"""配置管理 - JSON 配置文件读写"""

import json
import os
import secrets
import shutil
from typing import Optional

CONFIG_DIR = os.path.expanduser("~/.cc-desktop-switch")
CONFIG_FILE = os.path.join(CONFIG_DIR, "config.json")

DEFAULT_CONFIG = {
    "version": "1.0.0",
    "activeProvider": None,
    "gatewayApiKey": None,
    "providers": [],
    "settings": {
        "theme": "light",
        "language": "zh",
        "proxyPort": 18080,
        "adminPort": 18081,
        "autoStart": False,
        "updateUrl": "",
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
        "extraHeaders": {"x-api-key": "{apiKey}"},
        "isBuiltin": True,
    },
    {
        "id": "kimi",
        "name": "Kimi (月之暗面)",
        "baseUrl": "https://api.moonshot.cn/v1",
        "authScheme": "bearer",
        "apiFormat": "openai",
        "models": {
            "sonnet": "kimi-k2.6",
            "haiku": "kimi-k2.6",
            "opus": "kimi-k2.6",
            "default": "kimi-k2.6",
        },
        "isBuiltin": True,
    },
    {
        "id": "qiniu",
        "name": "七牛云 AI",
        "baseUrl": "https://api.qnaigc.com/v1",
        "authScheme": "bearer",
        "apiFormat": "openai",
        "models": {
            "sonnet": "qwen3-max-2026-01-23",
            "haiku": "deepseek/deepseek-v3.2-251201",
            "opus": "qwen3-max-2026-01-23",
            "default": "qwen3-max-2026-01-23",
        },
        "isBuiltin": True,
    },
    {
        "id": "zhipu",
        "name": "智谱 GLM",
        "baseUrl": "https://open.bigmodel.cn/api/paas/v4/",
        "authScheme": "bearer",
        "apiFormat": "openai",
        "models": {
            "sonnet": "glm-5.1",
            "haiku": "glm-5-turbo",
            "opus": "glm-5.1",
            "default": "glm-5.1",
        },
        "isBuiltin": True,
    },
]


def ensure_config_dir():
    """确保配置目录存在"""
    os.makedirs(CONFIG_DIR, exist_ok=True)


def load_config() -> dict:
    """加载配置文件"""
    ensure_config_dir()
    if not os.path.exists(CONFIG_FILE):
        return dict(DEFAULT_CONFIG)
    try:
        with open(CONFIG_FILE, "r", encoding="utf-8") as f:
            return json.load(f)
    except (json.JSONDecodeError, IOError):
        return dict(DEFAULT_CONFIG)


def save_config(config: dict):
    """保存配置文件"""
    ensure_config_dir()
    # 原子写入：先写临时文件，再重命名
    tmp_file = CONFIG_FILE + ".tmp"
    with open(tmp_file, "w", encoding="utf-8") as f:
        json.dump(config, f, ensure_ascii=False, indent=2)
    shutil.move(tmp_file, CONFIG_FILE)


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
    provider["id"] = provider.get("id", str(uuid.uuid4())[:8])
    provider.setdefault("models", {
        "sonnet": "",
        "haiku": "",
        "opus": "",
        "default": "",
    })
    provider.setdefault("isBuiltin", False)

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
            # 保留 id 和 isBuiltin
            data["id"] = provider_id
            data.setdefault("isBuiltin", p.get("isBuiltin", False))
            config["providers"][i] = data
            save_config(config)
            return data
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


def get_settings() -> dict:
    """获取设置"""
    config = load_config()
    settings = dict(DEFAULT_CONFIG["settings"])
    settings.update(config.get("settings", {}))
    return settings


def update_settings(settings: dict) -> dict:
    """更新设置"""
    config = load_config()
    current = config.get("settings", {})
    current.update(settings)
    config["settings"] = current
    save_config(config)
    return current


def get_presets() -> list:
    """获取内置预设列表"""
    return BUILTIN_PRESETS
