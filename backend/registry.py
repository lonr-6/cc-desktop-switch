"""Windows / macOS 注册表 / plist 操作 - 配置 Claude Desktop 3P 模式"""

import base64
import json
import os
import subprocess
import sys
import tempfile
from typing import Optional

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


def provider_inference_models(provider: Optional[dict]) -> list:
    """生成 Claude Desktop gateway 需要的模型列表。

    Claude Desktop 的 1M 上下文不是只看请求里的 model 字段，还会读取
    managed policy 的 inferenceModels。DeepSeek 的 1M 模型需要显式标注
    supports1m，且 name 要和 gateway /v1/models 返回的 ID 完全一致。
    """
    fallback = ["sonnet", "haiku", "opus"]
    if not provider:
        return fallback

    models = provider.get("models") or {}
    if not isinstance(models, dict):
        return fallback
    model_capabilities = provider.get("modelCapabilities") or {}
    if not isinstance(model_capabilities, dict):
        model_capabilities = {}

    ordered = []
    for key in ("default", "sonnet", "opus", "haiku"):
        model_id = str(models.get(key) or "").strip()
        if model_id and model_id not in ordered:
            ordered.append(model_id)

    if not ordered:
        return fallback

    result = []
    for model_id in ordered:
        item = {"name": model_id, "displayName": model_id}
        capabilities = model_capabilities.get(model_id)
        supports_1m = isinstance(capabilities, dict) and capabilities.get("supports1m") is True
        if "[1m]" in model_id.lower() or supports_1m:
            item["supports1m"] = True
        result.append(item)
    return result


def serialize_inference_models(provider: Optional[dict]) -> str:
    """序列化 inferenceModels，供注册表 / plist 写入。"""
    return json.dumps(
        provider_inference_models(provider),
        ensure_ascii=False,
        separators=(",", ":"),
    )


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


def _b64_utf8(value: str) -> str:
    """把字符串编码成 Base64，避免 PowerShell 参数转义问题。"""
    return base64.b64encode(str(value or "").encode("utf-8")).decode("ascii")


def _ps_single_quote(value: str) -> str:
    """PowerShell 单引号字符串转义。"""
    return "'" + str(value).replace("'", "''") + "'"


def _current_user_sid() -> str:
    """读取当前登录用户 SID，确保提权后仍写回原用户配置。"""
    try:
        result = subprocess.run(
            [
                "powershell",
                "-NoProfile",
                "-Command",
                "[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value",
            ],
            capture_output=True,
            text=True,
            timeout=5,
        )
    except Exception:
        return ""
    if result.returncode != 0:
        return ""
    return result.stdout.strip()


def _run_elevated_powershell(script_text: str) -> tuple[bool, str]:
    """通过 UAC 提权运行临时 PowerShell 脚本。"""
    fd, script_path = tempfile.mkstemp(prefix="ccds-desktop-config-", suffix=".ps1")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(script_text)

        command = (
            "$p = Start-Process -FilePath 'powershell.exe' "
            "-ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',"
            f"{_ps_single_quote(script_path)}) "
            "-Verb RunAs -Wait -PassThru; exit $p.ExitCode"
        )
        result = subprocess.run(
            ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", command],
            capture_output=True,
            text=True,
            timeout=180,
        )
        output = "\n".join(part for part in (result.stdout, result.stderr) if part).strip()
        return result.returncode == 0, output
    except subprocess.TimeoutExpired as exc:
        return False, f"管理员写入超时: {exc}"
    except Exception as exc:
        return False, str(exc)
    finally:
        try:
            os.remove(script_path)
        except OSError:
            pass


def _win_apply_config_elevated(base_url: str, gateway_api_key: str = "", inference_models: str = "") -> dict:
    """权限不足时通过 UAC 写入当前用户的 Claude Desktop policy。"""
    sid = _current_user_sid()
    target_path = f"Registry::HKEY_USERS\\{sid}\\{REGISTRY_PATH}" if sid else r"HKCU:\SOFTWARE\Policies\Claude"
    script = f"""
$ErrorActionPreference = 'Stop'
function DecodeUtf8([string]$Value) {{
    [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($Value))
}}
$path = DecodeUtf8 '{_b64_utf8(target_path)}'
if (-not (Test-Path -LiteralPath $path)) {{
    New-Item -Path $path -Force | Out-Null
}}
$baseUrl = DecodeUtf8 '{_b64_utf8(base_url)}'
$gatewayApiKey = DecodeUtf8 '{_b64_utf8(gateway_api_key)}'
$inferenceModels = DecodeUtf8 '{_b64_utf8(inference_models or DESKTOP_CONFIG["inferenceModels"][0])}'
New-ItemProperty -LiteralPath $path -Name 'inferenceProvider' -Value 'gateway' -PropertyType String -Force | Out-Null
New-ItemProperty -LiteralPath $path -Name 'inferenceGatewayBaseUrl' -Value $baseUrl -PropertyType String -Force | Out-Null
New-ItemProperty -LiteralPath $path -Name 'inferenceGatewayApiKey' -Value $gatewayApiKey -PropertyType String -Force | Out-Null
New-ItemProperty -LiteralPath $path -Name 'inferenceGatewayAuthScheme' -Value 'bearer' -PropertyType String -Force | Out-Null
New-ItemProperty -LiteralPath $path -Name 'inferenceModels' -Value $inferenceModels -PropertyType String -Force | Out-Null
New-ItemProperty -LiteralPath $path -Name 'isClaudeCodeForDesktopEnabled' -Value 1 -PropertyType DWord -Force | Out-Null
New-ItemProperty -LiteralPath $path -Name '{CCDS_MARKER}' -Value 'true' -PropertyType String -Force | Out-Null
"""
    ok, output = _run_elevated_powershell(script)
    if ok:
        return {"success": True, "message": "已通过管理员权限写入 Claude 桌面版配置"}
    detail = output or "用户取消了管理员授权，或系统拒绝提权"
    return {"success": False, "message": f"需要管理员权限写入 Claude 桌面版配置：{detail}"}


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


def _win_apply_config(base_url: str, gateway_api_key: str = "", inference_models: str = "") -> dict:
    import winreg
    key = _win_get_key(read_only=False)
    if key is None:
        return _win_apply_config_elevated(base_url, gateway_api_key, inference_models)
    try:
        inference_models = inference_models or DESKTOP_CONFIG["inferenceModels"][0]
        values = {
            "inferenceProvider": ("gateway", winreg.REG_SZ),
            "inferenceGatewayBaseUrl": (base_url, winreg.REG_SZ),
            "inferenceGatewayApiKey": (gateway_api_key, winreg.REG_SZ),
            "inferenceGatewayAuthScheme": ("bearer", winreg.REG_SZ),
            "inferenceModels": (inference_models, winreg.REG_SZ),
            "isClaudeCodeForDesktopEnabled": (1, winreg.REG_DWORD),
            CCDS_MARKER: ("true", winreg.REG_SZ),
        }
        for name, (value, type_) in values.items():
            winreg.SetValueEx(key, name, 0, type_, value)
        return {"success": True, "message": "Desktop 3P 配置已应用"}
    except PermissionError:
        return _win_apply_config_elevated(base_url, gateway_api_key, inference_models)
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


def _mac_apply_config(base_url: str, gateway_api_key: str = "", inference_models: str = "") -> dict:
    try:
        inference_models = inference_models or DESKTOP_CONFIG["inferenceModels"][0]
        for name in DESKTOP_CONFIG:
            val, typ = DESKTOP_CONFIG[name]
            if name == "inferenceGatewayBaseUrl":
                val = base_url
            if name == "inferenceGatewayApiKey":
                val = gateway_api_key
            if name == "inferenceModels":
                val = inference_models
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


def apply_config(
    base_url: str = "http://127.0.0.1:18080",
    gateway_api_key: str = "",
    provider: Optional[dict] = None,
) -> dict:
    """应用 Desktop 3P 配置"""
    inference_models = serialize_inference_models(provider)
    os_name = _os_name()
    if os_name == "win":
        return _win_apply_config(base_url, gateway_api_key, inference_models)
    elif os_name == "mac":
        return _mac_apply_config(base_url, gateway_api_key, inference_models)
    return _not_supported()


def clear_config() -> dict:
    """清除 Desktop 3P 配置"""
    os_name = _os_name()
    if os_name == "win":
        return _win_clear_config()
    elif os_name == "mac":
        return _mac_clear_config()
    return _not_supported()
