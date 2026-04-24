"""Windows / macOS 注册表 / plist 操作 - 配置 Claude Desktop 3P 模式"""

import subprocess
import sys

REGISTRY_PATH = r"SOFTWARE\Policies\Claude"
CCDS_MARKER = "ccds_managed"

# 预期的配置项（名称 → 默认值, 值类型）
DESKTOP_CONFIG = {
    "inferenceProvider": ("gateway", str),
    "inferenceGatewayApiKey": ("", str),
    "inferenceGatewayAuthScheme": ("bearer", str),
    "inferenceModels": ('["sonnet","haiku","opus"]', str),
    "inferenceGatewayBaseUrl": ("http://127.0.0.1:18080", str),
    "isClaudeCodeForDesktopEnabled": (1, int),
}

# ── 辅助函数 ──

def _safe_config_value(name: str, value) -> str:
    """返回可展示的配置值，避免把密钥暴露给前端。"""
    lowered = name.lower()
    if any(token in lowered for token in ("key", "token", "secret", "authorization")):
        return "******" if value else ""
    return str(value)

def _os_name() -> str:
    """返回 'win', 'mac', 'linux'"""
    if sys.platform == "win32":
        return "win"
    if sys.platform == "darwin":
        return "mac"
    return "linux"


def _not_supported() -> dict:
    """非 Windows 且非 macOS 时的提示"""
    return {"success": False, "message": "Claude Desktop 没有 Linux GUI 版本，无需配置"}


# ── Windows ──

def _win_get_key(read_only=False):
    import winreg
    try:
        if read_only:
            return winreg.OpenKey(winreg.HKEY_CURRENT_USER, REGISTRY_PATH, 0, winreg.KEY_READ)
        else:
            return winreg.CreateKey(winreg.HKEY_CURRENT_USER, REGISTRY_PATH)
    except (PermissionError, FileNotFoundError, OSError):
        return None


def _win_get_config_status() -> dict:
    import winreg
    key = _win_get_key(read_only=True)
    if key is None:
        return {"configured": False, "keys": {}, "message": "注册表键不存在"}
    result = {"configured": False, "keys": {}, "message": ""}
    try:
        i = 0
        while True:
            name, value, _ = winreg.EnumValue(key, i)
            result["keys"][name] = _safe_config_value(name, value)
            i += 1
    except OSError:
        pass
    finally:
        winreg.CloseKey(key)
    result["configured"] = (
        result["keys"].get("inferenceProvider") == "gateway"
        and result["keys"].get(CCDS_MARKER) == "true"
    )
    return result


def _win_apply_config(base_url: str, gateway_api_key: str = "") -> dict:
    import winreg
    key = _win_get_key(read_only=False)
    if key is None:
        return {"success": False, "message": "无法打开注册表，请以管理员身份运行"}
    try:
        values = {
            "inferenceProvider": ("gateway", winreg.REG_SZ),
            "inferenceGatewayBaseUrl": (base_url, winreg.REG_SZ),
            "inferenceGatewayApiKey": (gateway_api_key, winreg.REG_SZ),
            "inferenceGatewayAuthScheme": ("bearer", winreg.REG_SZ),
            "inferenceModels": ('["sonnet","haiku","opus"]', winreg.REG_SZ),
            "isClaudeCodeForDesktopEnabled": (1, winreg.REG_DWORD),
            CCDS_MARKER: ("true", winreg.REG_SZ),
        }
        for name, (value, type_) in values.items():
            winreg.SetValueEx(key, name, 0, type_, value)
        return {"success": True, "message": "Desktop 3P 配置已应用"}
    except PermissionError:
        return {"success": False, "message": "权限不足，请以管理员身份运行"}
    except Exception as e:
        return {"success": False, "message": f"配置失败: {str(e)}"}
    finally:
        winreg.CloseKey(key)


def _win_clear_config() -> dict:
    import winreg
    # 读取所有键名
    key = _win_get_key(read_only=True)
    if key is None:
        return {"success": True, "message": "注册表键不存在，无需清除"}
    names = []
    try:
        i = 0
        while True:
            name, _, _ = winreg.EnumValue(key, i)
            names.append(name)
            i += 1
    except OSError:
        pass
    finally:
        winreg.CloseKey(key)

    managed = [n for n in names if n.startswith("inference") or n == CCDS_MARKER]
    if not managed:
        return {"success": True, "message": "没有需要清除的配置"}

    key = _win_get_key(read_only=False)
    if key is None:
        return {"success": False, "message": "无法打开注册表"}
    try:
        for name in managed:
            winreg.DeleteValue(key, name)
        return {"success": True, "message": f"已清除 {len(managed)} 项配置"}
    except Exception as e:
        return {"success": False, "message": f"清除失败: {str(e)}"}
    finally:
        winreg.CloseKey(key)


# ── macOS ──

MAC_BUNDLE = "com.anthropic.claudefordesktop"
MAC_PLIST = f"~/Library/Preferences/{MAC_BUNDLE}.plist"


def _mac_run(args: list) -> tuple:
    """运行 defaults 命令，返回 (ok, output)"""
    try:
        r = subprocess.run(args, capture_output=True, text=True, timeout=5)
        return r.returncode == 0, r.stdout.strip()
    except (FileNotFoundError, subprocess.TimeoutExpired) as e:
        return False, str(e)


def _mac_get_config_status() -> dict:
    keys = {}
    for name in DESKTOP_CONFIG:
        ok, out = _mac_run(["defaults", "read", MAC_BUNDLE, name])
        if ok:
            keys[name] = _safe_config_value(name, out)
    # 检查标记
    ok, marker = _mac_run(["defaults", "read", MAC_BUNDLE, CCDS_MARKER])
    marked = ok and marker == "true"
    configured = keys.get("inferenceProvider") == "gateway" and marked
    return {"configured": configured, "keys": keys, "message": ""}


def _mac_apply_config(base_url: str, gateway_api_key: str = "") -> dict:
    try:
        for name in DESKTOP_CONFIG:
            val, typ = DESKTOP_CONFIG[name]
            if name == "inferenceGatewayBaseUrl":
                val = base_url
            if name == "inferenceGatewayApiKey":
                val = gateway_api_key
            # 根据 Python 类型选择 defaults 的 -type 参数
            if typ == int:
                _mac_run(["defaults", "write", MAC_BUNDLE, name, "-int", str(val)])
            else:
                _mac_run(["defaults", "write", MAC_BUNDLE, name, "-string", str(val)])
        _mac_run(["defaults", "write", MAC_BUNDLE, CCDS_MARKER, "-string", "true"])
        return {"success": True, "message": "macOS Desktop 3P 配置已应用"}
    except Exception as e:
        return {"success": False, "message": f"macOS 配置失败: {str(e)}"}


def _mac_clear_config() -> dict:
    managed = list(DESKTOP_CONFIG.keys()) + [CCDS_MARKER]
    count = 0
    for name in managed:
        ok, _ = _mac_run(["defaults", "delete", MAC_BUNDLE, name])
        if ok:
            count += 1
    if count:
        return {"success": True, "message": f"已清除 {count} 项配置"}
    return {"success": True, "message": "没有需要清除的配置"}


# ── 统一入口 ──

def is_configured() -> bool:
    """检查 Desktop 是否已通过我们的工具配置"""
    status = get_config_status()
    return status.get("configured", False)


def get_config_status() -> dict:
    """获取当前 Desktop 配置状态"""
    os_name = _os_name()
    if os_name == "win":
        return _win_get_config_status()
    elif os_name == "mac":
        return _mac_get_config_status()
    return {"configured": False, "keys": {}, "message": "仅 Windows / macOS 需要配置"}


def apply_config(base_url: str = "http://127.0.0.1:18080", gateway_api_key: str = "") -> dict:
    """应用 Desktop 3P 配置"""
    os_name = _os_name()
    if os_name == "win":
        return _win_apply_config(base_url, gateway_api_key)
    elif os_name == "mac":
        return _mac_apply_config(base_url, gateway_api_key)
    return _not_supported()


def clear_config() -> dict:
    """清除 Desktop 3P 配置"""
    os_name = _os_name()
    if os_name == "win":
        return _win_clear_config()
    elif os_name == "mac":
        return _mac_clear_config()
    return _not_supported()
