import copy
import asyncio
import json
import os
import shutil
import sqlite3
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

from fastapi.testclient import TestClient

from backend.main import (
    _desktop_health,
    _test_provider_connection,
    create_admin_app,
    desktop_config_target_for_provider,
    get_admin_token,
    register_app_activation_handler,
)
from main import (
    DesktopTrayController,
    acquire_single_instance_lock,
    admin_public_url,
    admin_ui_url,
    release_single_instance_lock,
    request_existing_instance_activate,
    run_browser_mode,
)
from backend import config as cfg
from backend import ccswitch_import
from backend import provider_tools
from backend import registry
from backend import update as updater
from backend.model_alias import provider_model_ids as raw_provider_model_ids
from backend.proxy import (
    _anthropic_to_openai_body,
    _normalize_anthropic_response,
    _normalize_anthropic_sse_event,
    _openai_to_anthropic,
    _openai_chunk_to_anthropic,
    apply_anthropic_request_options,
    build_upstream_url,
    create_proxy_app,
    forward_request,
    forward_request_stream,
    gateway_models_response,
    log_buffer,
    map_model,
)


def admin_headers(extra=None):
    headers = {
        "x-ccds-request": "1",
        "x-ccds-admin-token": get_admin_token(),
    }
    if extra:
        headers.update(extra)
    return headers


class ProviderConfigTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.old_config_dir = cfg.CONFIG_DIR
        self.old_config_file = cfg.CONFIG_FILE
        self.old_backup_dir = cfg.BACKUP_DIR
        cfg.CONFIG_DIR = self.temp_dir.name
        cfg.CONFIG_FILE = os.path.join(self.temp_dir.name, "config.json")
        cfg.BACKUP_DIR = os.path.join(self.temp_dir.name, "backups")
        cfg.save_config(copy.deepcopy(cfg.DEFAULT_CONFIG))

    def tearDown(self):
        cfg.CONFIG_DIR = self.old_config_dir
        cfg.CONFIG_FILE = self.old_config_file
        cfg.BACKUP_DIR = self.old_backup_dir
        self.temp_dir.cleanup()

    def test_update_provider_keeps_saved_key_and_extra_headers_when_blank(self):
        provider = cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "apiKey": "secret-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "deepseek-v4-pro", "haiku": "deepseek-v4-flash"},
            "extraHeaders": {"x-api-key": "{apiKey}"},
        })

        updated = cfg.update_provider(provider["id"], {
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic/v1/messages",
            "apiKey": "",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "deepseek-v4-pro"},
            "extraHeaders": {},
        })

        self.assertEqual(updated["apiKey"], "secret-key")
        self.assertEqual(updated["extraHeaders"], {"x-api-key": "{apiKey}"})
        self.assertEqual(updated["models"]["haiku"], "deepseek-v4-flash")

    def test_update_provider_replaces_key_when_new_key_is_provided(self):
        provider = cfg.add_provider({
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.ai/v1",
            "apiKey": "old-key",
            "authScheme": "bearer",
            "apiFormat": "openai",
        })

        updated = cfg.update_provider(provider["id"], {
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.ai/v1",
            "apiKey": "new-key",
            "authScheme": "bearer",
            "apiFormat": "openai",
        })

        self.assertEqual(updated["apiKey"], "new-key")

    def test_backup_export_and_import_config(self):
        provider = cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "apiKey": "secret-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
        })
        exported = cfg.export_config()

        self.assertEqual(exported["config"]["providers"][0]["apiKey"], "secret-key")

        imported = copy.deepcopy(exported)
        imported["config"]["providers"][0]["name"] = "Imported DeepSeek"
        result = cfg.import_config(imported)

        self.assertTrue(os.path.exists(os.path.join(cfg.BACKUP_DIR, result["backup"]["name"])))
        self.assertEqual(cfg.get_provider(provider["id"])["name"], "Imported DeepSeek")
        self.assertEqual(len(cfg.list_backups()), 1)

    def test_backups_created_in_same_second_do_not_overwrite(self):
        first = cfg.create_backup("manual")
        second = cfg.create_backup("manual")

        self.assertNotEqual(first["name"], second["name"])
        self.assertEqual(len(cfg.list_backups()), 2)

    def test_import_config_sanitizes_provider_ids(self):
        imported = {
            "providers": [
                {"id": "bad\"><script>", "name": "A"},
                {"id": "bad\"><script>", "name": "B"},
            ]
        }

        result = cfg.import_config(imported)
        ids = [provider["id"] for provider in result["config"]["providers"]]

        self.assertEqual(len(ids), 2)
        self.assertEqual(len(set(ids)), 2)
        self.assertTrue(all("<" not in provider_id and '"' not in provider_id for provider_id in ids))

    def test_add_provider_avoids_duplicate_ids(self):
        first = cfg.add_provider({"id": "same", "name": "A"})
        second = cfg.add_provider({"id": "same", "name": "B"})

        self.assertEqual(first["id"], "same")
        self.assertNotEqual(second["id"], "same")
        self.assertEqual(len({p["id"] for p in cfg.get_providers()}), 2)

    def test_builtin_presets_include_expected_provider_urls(self):
        presets = {preset["id"]: preset for preset in cfg.get_presets()}

        expected_urls = {
            "deepseek": "https://api.deepseek.com/anthropic",
            "kimi": "https://api.moonshot.cn/anthropic",
            "kimi-code": "https://api.kimi.com/coding",
            "zhipu": "https://open.bigmodel.cn/api/anthropic",
            "bailian": "https://dashscope.aliyuncs.com/apps/anthropic",
        }

        for preset_id, base_url in expected_urls.items():
            self.assertIn(preset_id, presets)
            self.assertEqual(presets[preset_id]["baseUrl"], base_url)
            self.assertEqual(presets[preset_id]["apiFormat"], "anthropic")
            self.assertTrue(presets[preset_id]["models"]["default"])

        self.assertEqual(presets["kimi"]["models"]["default"], "kimi-k2.6")
        self.assertEqual(presets["kimi-code"]["models"]["default"], "kimi-for-coding")
        self.assertEqual(presets["zhipu"]["models"]["haiku"], "glm-4.7")
        self.assertNotIn("qiniu", presets)
        self.assertNotIn("siliconflow", presets)
        self.assertEqual(presets["bailian"]["modelCapabilities"], {})
        qwen_1m = presets["bailian"]["modelOptions"]["qwen_1m"]
        self.assertIn("开启千问 1M 上下文", qwen_1m["label"])
        self.assertTrue(qwen_1m["modelCapabilities"]["qwen3.6-plus"]["supports1m"])
        self.assertTrue(qwen_1m["modelCapabilities"]["qwen3.6-flash"]["supports1m"])

        deepseek_1m = presets["deepseek"]["modelOptions"]["deepseek_1m"]
        self.assertEqual(deepseek_1m["models"]["sonnet"], "deepseek-v4-pro[1m]")
        self.assertEqual(deepseek_1m["models"]["opus"], "deepseek-v4-pro[1m]")
        self.assertEqual(deepseek_1m["models"]["default"], "deepseek-v4-pro[1m]")
        self.assertTrue(deepseek_1m["modelCapabilities"]["deepseek-v4-pro[1m]"]["supports1m"])
        self.assertTrue(deepseek_1m["modelCapabilities"]["deepseek-v4-flash"]["supports1m"])
        deepseek_max = presets["deepseek"]["requestOptionPresets"]["deepseek_max_effort"]
        self.assertEqual(deepseek_max["requestOptions"]["anthropic"]["output_config"]["effort"], "max")
        self.assertEqual(deepseek_max["requestOptions"]["anthropic"]["thinking"]["type"], "enabled")
        self.assertIn("Low：更快更省", deepseek_max["description"])
        self.assertIn("未勾选则使用 Claude 当前默认配置", deepseek_max["description"])

    def test_registry_inference_models_mark_deepseek_1m(self):
        provider = {
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {
                "sonnet": "deepseek-v4-pro[1m]",
                "haiku": "deepseek-v4-flash",
                "opus": "deepseek-v4-pro[1m]",
                "default": "deepseek-v4-pro[1m]",
            },
            "modelCapabilities": {
                "deepseek-v4-pro[1m]": {"supports1m": True},
                "deepseek-v4-flash": {"supports1m": True},
            },
        }

        models = registry.provider_inference_models(provider)
        serialized = registry.serialize_inference_models(provider)
        by_name = {item["name"]: item for item in models}

        self.assertEqual(set(by_name), {
            "claude-opus-4-7",
            "claude-sonnet-4-6",
            "claude-haiku-4-5",
        })
        self.assertIn("claude-sonnet-4-6", by_name)
        self.assertIn("claude-haiku-4-5", by_name)
        self.assertNotIn("deepseek-v4-pro[1m]", by_name)
        self.assertNotIn("deepseek-v4-flash", by_name)
        self.assertNotIn("claude-opus-4-6", by_name)
        self.assertNotIn("claude-3-opus", by_name)
        self.assertNotIn("claude-sonnet-4-5", by_name)
        self.assertTrue(by_name["claude-opus-4-7"]["supports1m"])
        self.assertTrue(by_name["claude-sonnet-4-6"]["supports1m"])
        self.assertTrue(by_name["claude-haiku-4-5"]["supports1m"])
        self.assertIn('"supports1m":true', serialized)

    def test_registry_inference_models_mark_capability_based_1m_models(self):
        provider = {
            "models": {
                "sonnet": "qwen3.6-plus",
                "haiku": "qwen3.6-flash",
                "opus": "qwen3.6-max-preview",
                "default": "qwen3.6-plus",
            },
            "modelCapabilities": {
                "qwen3.6-plus": {"supports1m": True},
                "qwen3.6-flash": {"supports1m": True},
            },
        }

        models = registry.provider_inference_models(provider)
        by_name = {item["name"]: item for item in models}

        self.assertTrue(by_name["claude-sonnet-4-6"]["supports1m"])
        self.assertTrue(by_name["claude-haiku-4-5"]["supports1m"])
        self.assertNotIn("supports1m", by_name["claude-opus-4-7"])
        self.assertNotIn("claude-opus-4-6", by_name)
        self.assertNotIn("claude-3-opus", by_name)
        self.assertNotIn("claude-sonnet-4-5", by_name)
        self.assertNotIn("qwen3.6-plus", by_name)
        self.assertNotIn("qwen3.6-flash", by_name)

    def test_desktop_config_target_uses_local_proxy_for_anthropic_provider(self):
        provider = {
            "name": "Custom Anthropic",
            "baseUrl": "https://api.example.com/anthropic/",
            "apiKey": "provider-key",
            "authScheme": "x-api-key",
            "apiFormat": "anthropic",
            "extraHeaders": {"x-api-key": "{apiKey}"},
        }

        target = desktop_config_target_for_provider(provider, {"proxyPort": 18080})

        self.assertEqual(target["mode"], "local_proxy")
        self.assertTrue(target["requiresProxy"])
        self.assertEqual(target["baseUrl"], "http://127.0.0.1:18080")
        self.assertTrue(target["apiKey"].startswith("ccds_"))
        self.assertNotEqual(target["apiKey"], "provider-key")
        self.assertEqual(target["authScheme"], "bearer")
        self.assertEqual(target["gatewayHeaders"], "")
        self.assertEqual(target["provider"], provider)

    def test_all_provider_inference_models_use_provider_aliases_and_keep_1m(self):
        providers = [
            {
                "id": "deepseek",
                "name": "DeepSeek",
                "models": {
                    "sonnet": "deepseek-v4-pro[1m]",
                    "haiku": "deepseek-v4-flash",
                    "default": "deepseek-v4-pro[1m]",
                },
                "modelCapabilities": {
                    "deepseek-v4-pro[1m]": {"supports1m": True},
                },
            },
            {
                "id": "kimi",
                "name": "Kimi",
                "models": {
                    "sonnet": "kimi-k2.6",
                    "default": "kimi-k2.6",
                },
            },
        ]

        models = registry.all_provider_inference_models(providers)

        self.assertIn(
            {
                "name": "deepseek/claude-sonnet-4-6",
                "displayName": "DeepSeek / claude-sonnet-4-6",
                "supports1m": True,
            },
            models,
        )
        self.assertIn({"name": "kimi/claude-sonnet-4-6", "displayName": "Kimi / claude-sonnet-4-6"}, models)
        self.assertNotIn({"name": "deepseek-v4-pro[1m]", "displayName": "deepseek-v4-pro[1m]"}, models)
        self.assertFalse(any(item.get("name") == "deepseek/claude-opus-4-6" for item in models))

    def test_windows_registry_apply_falls_back_to_elevated_helper_when_key_is_not_writable(self):
        if not hasattr(registry, "_win_apply_config"):
            self.skipTest("Windows registry helper is unavailable")

        with patch("backend.registry._win_get_key", return_value=None):
            with patch("backend.registry._win_apply_config_elevated", return_value={"success": True}) as elevated:
                result = registry._win_apply_config(
                    "http://127.0.0.1:18080",
                    "gateway-key",
                    '[{"name":"deepseek-v4-pro[1m]","supports1m":true}]',
                )

        self.assertTrue(result["success"])
        elevated.assert_called_once()

    def test_macos_apply_config_reports_defaults_write_failure(self):
        def fake_mac_run(args):
            if args[:4] == ["defaults", "write", registry.MAC_BUNDLE, "inferenceGatewayBaseUrl"]:
                return False, "permission denied"
            return True, ""

        with patch("backend.registry._mac_run", side_effect=fake_mac_run):
            result = registry._mac_apply_plist_config(
                "http://127.0.0.1:18080",
                gateway_api_key="secret-value",
                inference_models='[{"name":"sonnet","displayName":"sonnet"}]',
            )

        self.assertFalse(result["success"])
        self.assertIn("inferenceGatewayBaseUrl", result["message"])
        self.assertNotIn("secret-value", result["message"])

    def test_macos_apply_config_reports_readback_mismatch(self):
        def fake_mac_run(args):
            if args[:3] == ["defaults", "read", registry.MAC_BUNDLE]:
                key = args[3]
                if key == "inferenceGatewayBaseUrl":
                    return True, "http://127.0.0.1:9999"
                if key == registry.CCDS_MARKER:
                    return True, "true"
                value, _ = registry.DESKTOP_CONFIG[key]
                if key == "inferenceGatewayBaseUrl":
                    value = "http://127.0.0.1:18080"
                if key == "inferenceGatewayApiKey":
                    value = "secret-value"
                if key == "inferenceModels":
                    value = '[{"name":"sonnet","displayName":"sonnet"}]'
                return True, str(value)
            return True, ""

        with patch("backend.registry._mac_run", side_effect=fake_mac_run):
            result = registry._mac_apply_plist_config(
                "http://127.0.0.1:18080",
                gateway_api_key="secret-value",
                inference_models='[{"name":"sonnet","displayName":"sonnet"}]',
            )

        self.assertFalse(result["success"])
        self.assertIn("inferenceGatewayBaseUrl", result["message"])
        self.assertIn("readback mismatch", result["message"])
        self.assertNotIn("secret-value", result["message"])

    def test_macos_apply_config_writes_json_and_preserves_preferences(self):
        json_path = os.path.join(self.temp_dir.name, "Claude-3p", "claude_desktop_config.json")
        os.makedirs(os.path.dirname(json_path), exist_ok=True)
        with open(json_path, "w", encoding="utf-8") as handle:
            json.dump({
                "deploymentMode": "1p",
                "enterpriseConfig": {
                    "inferenceProvider": "gateway",
                    "inferenceGatewayBaseUrl": "https://old.example",
                },
                "preferences": {"sidebarMode": "task"},
            }, handle)

        with patch.object(registry, "MAC_3P_CONFIG", json_path):
            with patch("backend.registry._mac_apply_plist_config", return_value={"success": True}) as plist_apply:
                result = registry._mac_apply_config(
                    "http://127.0.0.1:18080",
                    gateway_api_key="secret-value",
                    inference_models='[{"name":"model-a","displayName":"Model A"},{"name":"model-b","supports1m":true}]',
                    auth_scheme="x-api-key",
                    gateway_headers='["x-api-key: secret-value"]',
                )

        self.assertTrue(result["success"])
        plist_apply.assert_called_once()
        with open(json_path, encoding="utf-8") as handle:
            saved = json.load(handle)
        self.assertEqual(saved["deploymentMode"], "3p")
        self.assertEqual(saved["preferences"], {"sidebarMode": "task"})
        self.assertEqual(saved["enterpriseConfig"]["inferenceProvider"], "gateway")
        self.assertEqual(saved["enterpriseConfig"]["inferenceGatewayBaseUrl"], "http://127.0.0.1:18080")
        self.assertEqual(saved["enterpriseConfig"]["inferenceGatewayApiKey"], "secret-value")
        self.assertEqual(saved["enterpriseConfig"]["inferenceGatewayAuthScheme"], "x-api-key")
        self.assertEqual(saved["enterpriseConfig"]["inferenceGatewayHeaders"], ["x-api-key: secret-value"])
        self.assertEqual(saved["enterpriseConfig"]["inferenceModels"], ["model-a", "model-b"])
        self.assertIs(saved["enterpriseConfig"]["isClaudeCodeForDesktopEnabled"], True)

    def test_macos_apply_config_writes_active_config_library_entry(self):
        json_path = os.path.join(self.temp_dir.name, "Claude-3p", "claude_desktop_config.json")
        library_dir = os.path.join(os.path.dirname(json_path), "configLibrary")
        entry_id = "1b050dc2-874f-4096-a303-566f42c64bcb"
        entry_path = os.path.join(library_dir, f"{entry_id}.json")
        os.makedirs(library_dir, exist_ok=True)
        with open(os.path.join(library_dir, "_meta.json"), "w", encoding="utf-8") as handle:
            json.dump({
                "appliedId": entry_id,
                "entries": [{"id": entry_id, "name": "Default"}],
            }, handle)
        with open(entry_path, "w", encoding="utf-8") as handle:
            json.dump({
                "inferenceProvider": "gateway",
                "inferenceGatewayBaseUrl": "https://old.example",
                "note": "keep me",
            }, handle)

        with patch.object(registry, "MAC_3P_CONFIG", json_path):
            with patch("backend.registry._mac_apply_plist_config", return_value={"success": True}):
                result = registry._mac_apply_config(
                    "http://127.0.0.1:18080",
                    gateway_api_key="secret-value",
                    inference_models='[{"name":"model-a","displayName":"Model A"},{"name":"model-b","supports1m":true}]',
                    auth_scheme="x-api-key",
                    gateway_headers='["x-api-key: secret-value"]',
                )

        self.assertTrue(result["success"])
        with open(entry_path, encoding="utf-8") as handle:
            saved = json.load(handle)
        self.assertEqual(saved["note"], "keep me")
        self.assertEqual(saved["inferenceProvider"], "gateway")
        self.assertEqual(saved["inferenceGatewayBaseUrl"], "http://127.0.0.1:18080")
        self.assertEqual(saved["inferenceGatewayApiKey"], "secret-value")
        self.assertEqual(saved["inferenceGatewayAuthScheme"], "x-api-key")
        self.assertEqual(saved["inferenceGatewayHeaders"], ["x-api-key: secret-value"])
        self.assertEqual(saved["inferenceModels"], ["model-a", "model-b"])
        self.assertIs(saved["isClaudeCodeForDesktopEnabled"], True)

    def test_macos_status_prefers_json_runtime_values_and_keeps_plist_models(self):
        json_path = os.path.join(self.temp_dir.name, "Claude-3p", "claude_desktop_config.json")
        os.makedirs(os.path.dirname(json_path), exist_ok=True)
        with open(json_path, "w", encoding="utf-8") as handle:
            json.dump({
                "deploymentMode": "3p",
                "enterpriseConfig": {
                    "inferenceProvider": "gateway",
                    "inferenceGatewayBaseUrl": "https://stale.example",
                    "inferenceGatewayApiKey": "secret-value",
                    "inferenceModels": ["model-a"],
                },
            }, handle)

        plist_models = '[{"name":"model-a","displayName":"model-a","supports1m":true}]'
        with patch.object(registry, "MAC_3P_CONFIG", json_path):
            with patch("backend.registry._mac_get_plist_config_status", return_value={
                "configured": True,
                "keys": {
                    "inferenceProvider": "gateway",
                    "inferenceGatewayBaseUrl": "http://127.0.0.1:18080",
                    "inferenceGatewayApiKey": "******",
                    "inferenceModels": plist_models,
                },
                "message": "",
            }):
                status = registry._mac_get_config_status()

        self.assertTrue(status["configured"])
        self.assertEqual(status["keys"]["inferenceGatewayBaseUrl"], "https://stale.example")
        self.assertEqual(status["keys"]["inferenceGatewayApiKey"], "******")
        self.assertEqual(status["keys"]["inferenceModels"], plist_models)
        self.assertTrue(status["sources"]["plist"])
        self.assertTrue(status["sources"]["json"])
        self.assertFalse(status["sources"]["configLibrary"])

    def test_macos_status_prefers_config_library_over_root_json(self):
        json_path = os.path.join(self.temp_dir.name, "Claude-3p", "claude_desktop_config.json")
        library_dir = os.path.join(os.path.dirname(json_path), "configLibrary")
        entry_id = "1b050dc2-874f-4096-a303-566f42c64bcb"
        os.makedirs(library_dir, exist_ok=True)
        with open(json_path, "w", encoding="utf-8") as handle:
            json.dump({
                "deploymentMode": "3p",
                "enterpriseConfig": {
                    "inferenceProvider": "gateway",
                    "inferenceGatewayBaseUrl": "https://root.example",
                    "inferenceGatewayApiKey": "root-secret",
                    "inferenceModels": ["root-model"],
                },
            }, handle)
        with open(os.path.join(library_dir, "_meta.json"), "w", encoding="utf-8") as handle:
            json.dump({
                "appliedId": entry_id,
                "entries": [{"id": entry_id, "name": "Default"}],
            }, handle)
        with open(os.path.join(library_dir, f"{entry_id}.json"), "w", encoding="utf-8") as handle:
            json.dump({
                "inferenceProvider": "gateway",
                "inferenceGatewayBaseUrl": "https://library.example",
                "inferenceGatewayApiKey": "library-secret",
                "inferenceModels": ["library-model"],
            }, handle)

        with patch.object(registry, "MAC_3P_CONFIG", json_path):
            with patch("backend.registry._mac_get_plist_config_status", return_value={
                "configured": True,
                "keys": {
                    "inferenceProvider": "gateway",
                    "inferenceGatewayBaseUrl": "http://127.0.0.1:18080",
                    "inferenceGatewayApiKey": "******",
                    "inferenceModels": '[{"name":"plist-model","supports1m":true}]',
                },
                "message": "",
            }):
                status = registry._mac_get_config_status()

        self.assertTrue(status["configured"])
        self.assertEqual(status["keys"]["inferenceGatewayBaseUrl"], "https://library.example")
        self.assertEqual(status["keys"]["inferenceGatewayApiKey"], "******")
        self.assertEqual(status["keys"]["inferenceModels"], '["library-model"]')
        self.assertTrue(status["sources"]["plist"])
        self.assertTrue(status["sources"]["json"])
        self.assertTrue(status["sources"]["configLibrary"])

    def test_macos_clear_config_clears_json_without_touching_preferences(self):
        json_path = os.path.join(self.temp_dir.name, "Claude-3p", "claude_desktop_config.json")
        os.makedirs(os.path.dirname(json_path), exist_ok=True)
        with open(json_path, "w", encoding="utf-8") as handle:
            json.dump({
                "deploymentMode": "3p",
                "enterpriseConfig": {
                    "inferenceProvider": "gateway",
                    "inferenceGatewayBaseUrl": "http://127.0.0.1:18080",
                    "inferenceGatewayApiKey": "secret-value",
                },
                "preferences": {"sidebarMode": "task"},
            }, handle)

        with patch.object(registry, "MAC_3P_CONFIG", json_path):
            with patch("backend.registry._mac_clear_plist_config", return_value={"success": True}) as plist_clear:
                result = registry._mac_clear_config()

        self.assertTrue(result["success"])
        plist_clear.assert_called_once()
        with open(json_path, encoding="utf-8") as handle:
            saved = json.load(handle)
        self.assertEqual(saved["deploymentMode"], "clear")
        self.assertNotIn("enterpriseConfig", saved)
        self.assertEqual(saved["preferences"], {"sidebarMode": "task"})

    def test_macos_clear_config_clears_active_config_library_entry(self):
        json_path = os.path.join(self.temp_dir.name, "Claude-3p", "claude_desktop_config.json")
        library_dir = os.path.join(os.path.dirname(json_path), "configLibrary")
        entry_id = "1b050dc2-874f-4096-a303-566f42c64bcb"
        entry_path = os.path.join(library_dir, f"{entry_id}.json")
        os.makedirs(library_dir, exist_ok=True)
        with open(json_path, "w", encoding="utf-8") as handle:
            json.dump({
                "deploymentMode": "3p",
                "enterpriseConfig": {"inferenceProvider": "gateway"},
            }, handle)
        with open(os.path.join(library_dir, "_meta.json"), "w", encoding="utf-8") as handle:
            json.dump({
                "appliedId": entry_id,
                "entries": [{"id": entry_id, "name": "Default"}],
            }, handle)
        with open(entry_path, "w", encoding="utf-8") as handle:
            json.dump({
                "inferenceProvider": "gateway",
                "inferenceGatewayBaseUrl": "http://127.0.0.1:18080",
                "inferenceGatewayApiKey": "secret-value",
                "inferenceGatewayAuthScheme": "bearer",
                "inferenceGatewayHeaders": ["x-api-key: secret-value"],
                "inferenceModels": ["model-a"],
                "note": "keep me",
            }, handle)

        with patch.object(registry, "MAC_3P_CONFIG", json_path):
            with patch("backend.registry._mac_clear_plist_config", return_value={"success": True}):
                result = registry._mac_clear_config()

        self.assertTrue(result["success"])
        with open(entry_path, encoding="utf-8") as handle:
            saved = json.load(handle)
        self.assertEqual(saved, {"note": "keep me"})

    def test_macos_apply_config_entrypoint_forwards_auth_scheme_and_headers(self):
        with patch("backend.registry._os_name", return_value="mac"):
            with patch("backend.registry.serialize_inference_models", return_value='["model-a"]'):
                with patch("backend.registry._mac_apply_config", return_value={"success": True}) as mac_apply:
                    result = registry.apply_config(
                        "https://api.example.com/anthropic",
                        gateway_api_key="provider-key",
                        auth_scheme="x-api-key",
                        gateway_headers='["x-api-key: provider-key"]',
                    )

        self.assertTrue(result["success"])
        mac_apply.assert_called_once_with(
            "https://api.example.com/anthropic",
            "provider-key",
            '["model-a"]',
            "x-api-key",
            '["x-api-key: provider-key"]',
        )

    def test_elevated_registry_script_does_not_contain_plain_gateway_key(self):
        captured = {}

        def fake_run(script):
            captured["script"] = script
            return True, ""

        with patch("backend.registry._current_user_sid", return_value="S-1-5-21-test"):
            with patch("backend.registry._run_elevated_powershell", fake_run):
                result = registry._win_apply_config_elevated(
                    "http://127.0.0.1:18080",
                    "plain-gateway-key",
                    '[{"name":"deepseek-v4-pro[1m]","supports1m":true}]',
                    gateway_headers='["x-api-key: plain-gateway-key"]',
                )

        self.assertTrue(result["success"])
        self.assertNotIn("plain-gateway-key", captured["script"])
        self.assertIn("FromBase64String", captured["script"])
        self.assertNotIn(r"HKCU:\SOFTWARE\Policies\Claude", captured["script"])

    def test_elevated_registry_script_targets_current_user_hive(self):
        captured = {}

        def fake_run(script):
            captured["script"] = script
            return True, ""

        with patch("backend.registry._run_elevated_powershell", fake_run):
            with patch("backend.registry._current_user_sid", return_value="S-1-5-21-1001"):
                result = registry._win_apply_config_elevated(
                    "http://127.0.0.1:18080",
                    "gateway-key",
                    '[{"name":"deepseek-v4-pro[1m]","supports1m":true}]',
                )

        self.assertTrue(result["success"])
        expected_path = registry._b64_utf8(
            r"Registry::HKEY_USERS\S-1-5-21-1001\SOFTWARE\Policies\Claude"
        )
        self.assertIn(expected_path, captured["script"])

    def test_managed_policy_names_include_code_desktop_flag(self):
        names = [
            "inferenceProvider",
            "inferenceGatewayBaseUrl",
            "inferenceGatewayApiKey",
            "inferenceGatewayAuthScheme",
            "inferenceGatewayHeaders",
            "inferenceModels",
            "isClaudeCodeForDesktopEnabled",
            "ccds_managed",
            "unrelatedPreference",
        ]

        self.assertEqual(
            registry._managed_policy_names(names),
            [
                "inferenceProvider",
                "inferenceGatewayBaseUrl",
                "inferenceGatewayApiKey",
                "inferenceGatewayAuthScheme",
                "inferenceGatewayHeaders",
                "inferenceModels",
                "isClaudeCodeForDesktopEnabled",
                "ccds_managed",
            ],
        )

    def test_update_provider_preserves_or_clears_request_options_explicitly(self):
        provider = cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "requestOptions": {
                "anthropic": {
                    "thinking": {"type": "enabled"},
                    "output_config": {"effort": "max"},
                }
            },
        })

        preserved = cfg.update_provider(provider["id"], {"name": "DeepSeek"})
        self.assertEqual(
            preserved["requestOptions"]["anthropic"]["output_config"]["effort"],
            "max",
        )

        cleared = cfg.update_provider(provider["id"], {"requestOptions": {}})
        self.assertEqual(cleared["requestOptions"], {})

    def test_all_builtin_presets_expose_only_explicit_provider_models(self):
        """内置预设只把显式映射的 Claude route 暴露给 Claude Desktop。"""
        for preset in cfg.get_presets():
            with self.subTest(provider=preset["id"]):
                if preset["id"] == "third-party":
                    continue
                models = preset["models"]
                upstream_ids = raw_provider_model_ids(preset)

                desktop_models = registry.provider_inference_models(preset)
                desktop_ids = [
                    item["name"] if isinstance(item, dict) else item
                    for item in desktop_models
                ]
                gateway_ids = [item["id"] for item in gateway_models_response(preset)["data"]]

                self.assertEqual(gateway_ids, desktop_ids)
                for model_id in upstream_ids:
                    self.assertEqual(map_model(model_id, preset), model_id)
                    self.assertNotIn(model_id, desktop_ids)
                    self.assertNotIn(model_id, gateway_ids)
                if not desktop_ids:
                    self.assertEqual(map_model("claude-sonnet-4-6", preset), "claude-sonnet-4-6")
                    continue
                for route_id in desktop_ids:
                    self.assertIn(map_model(route_id, preset), upstream_ids)
                if "claude-sonnet-4-6" in desktop_ids:
                    self.assertEqual(map_model("claude-sonnet-4-6", preset), models["sonnet"])
                if "claude-haiku-4-5" in desktop_ids:
                    self.assertEqual(map_model("claude-haiku-4-5", preset), models["haiku"])
                if "claude-opus-4-7" in desktop_ids:
                    self.assertEqual(map_model("claude-opus-4-7", preset), models["opus"])

        deepseek_1m = cfg.get_presets()[0]["modelOptions"]["deepseek_1m"]
        deepseek_1m_provider = {
            **cfg.get_presets()[0],
            "models": deepseek_1m["models"],
        }
        desktop_models = registry.provider_inference_models(deepseek_1m_provider)
        self.assertTrue(any(
            item["name"] == "claude-sonnet-4-6" and item.get("supports1m") is True
            for item in desktop_models
            if isinstance(item, dict)
        ))
        self.assertFalse(any(
            item["name"] == "deepseek-v4-pro[1m]"
            for item in desktop_models
            if isinstance(item, dict)
        ))
        self.assertEqual(
            map_model("claude-sonnet-4-6", deepseek_1m_provider),
            "deepseek-v4-pro[1m]",
        )
        self.assertEqual(
            map_model("claude-haiku-4-5", deepseek_1m_provider),
            "deepseek-v4-flash",
        )

    def test_settings_fall_back_to_default_update_url(self):
        config = copy.deepcopy(cfg.DEFAULT_CONFIG)
        config["settings"]["updateUrl"] = ""
        cfg.save_config(config)

        settings = cfg.get_settings()
        updated = cfg.update_settings({"updateUrl": ""})

        self.assertEqual(settings["updateUrl"], cfg.DEFAULT_UPDATE_URL)
        self.assertEqual(updated["updateUrl"], cfg.DEFAULT_UPDATE_URL)

    def test_update_version_compare_does_not_flag_same_version(self):
        self.assertFalse(updater.is_newer_version("1.0.4", "1.0.4"))
        self.assertFalse(updater.is_newer_version("v1.0.4", "1.0.4"))
        self.assertTrue(updater.is_newer_version("1.0.10", "1.0.9"))

    def test_fetch_latest_json_accepts_utf8_bom(self):
        class FakeResponse:
            content = b'\xef\xbb\xbf{"version":"1.0.9","platforms":{"windows-x64":{"assets":[]}}}'

            def raise_for_status(self):
                return None

            def json(self):
                raise ValueError("BOM")

        class FakeClient:
            def __init__(self, *args, **kwargs):
                pass

            async def __aenter__(self):
                return self

            async def __aexit__(self, exc_type, exc, tb):
                return False

            async def get(self, *args, **kwargs):
                return FakeResponse()

        with patch("backend.update.httpx.AsyncClient", FakeClient):
            data = asyncio.run(updater.fetch_latest_json("https://example.com/latest.json"))

        self.assertEqual(data["version"], "1.0.9")

    def test_update_installer_asset_prefers_setup_exe(self):
        asset = updater.pick_windows_installer([
            {"name": "CC-Desktop-Switch-v1.0.5-Windows-Portable.zip"},
            {"name": "CC-Desktop-Switch-v1.0.5-Windows-x64.exe"},
            {"name": "CC-Desktop-Switch-v1.0.5-Windows-Setup.exe"},
        ])

        self.assertEqual(asset["name"], "CC-Desktop-Switch-v1.0.5-Windows-Setup.exe")

    def test_update_platform_helpers_support_macos_assets(self):
        self.assertEqual(updater.current_platform("darwin", "arm64"), "macos-arm64")
        self.assertEqual(updater.current_platform("win32", "AMD64"), "windows-x64")

        asset = updater.pick_platform_installer([
            {"name": "CC-Desktop-Switch-v1.0.10-macOS-arm64.dmg"},
            {"name": "CC-Desktop-Switch-v1.0.10-macOS-arm64.pkg"},
        ], "macos-arm64")

        self.assertEqual(asset["name"], "CC-Desktop-Switch-v1.0.10-macOS-arm64.pkg")
        self.assertEqual(updater.install_command("/tmp/app.pkg", "macos-arm64"), ["open", "/tmp/app.pkg"])
        queued = updater.install_after_quit_command("/tmp/app.pkg", "macos-arm64", 4321)
        self.assertEqual(queued[0], "/bin/sh")
        self.assertIn('kill -0 "$pid"', queued[2])
        self.assertEqual(queued[-2:], ["4321", "/tmp/app.pkg"])

    def test_reorder_providers_persists_order_and_sort_index(self):
        first = cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
        })
        second = cfg.add_provider({
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.cn/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
        })

        self.assertTrue(cfg.reorder_providers([second["id"], first["id"]]))
        providers = cfg.get_providers()

        self.assertEqual([provider["id"] for provider in providers], [second["id"], first["id"]])
        self.assertEqual([provider["sortIndex"] for provider in providers], [0, 1])

    def test_desktop_health_detects_stale_gateway_and_missing_1m(self):
        provider = {
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {
                "sonnet": "deepseek-v4-pro[1m]",
                "haiku": "deepseek-v4-flash",
                "opus": "deepseek-v4-pro[1m]",
                "default": "deepseek-v4-pro[1m]",
            },
            "modelCapabilities": {
                "deepseek-v4-pro[1m]": {"supports1m": True},
                "deepseek-v4-flash": {"supports1m": True},
            },
        }
        old_status = {
            "configured": False,
            "keys": {
                "inferenceGatewayBaseUrl": "https://api.deepseek.com/anthropic",
                "inferenceModels": '["sonnet","haiku","opus"]',
            },
        }

        health = _desktop_health(old_status, 18080, provider)
        codes = {issue["code"] for issue in health["issues"]}

        self.assertTrue(health["needsApply"])
        self.assertIn("gateway_base_url_mismatch", codes)
        self.assertIn("one_million_not_written", codes)

        raw_status = {
            "configured": True,
            "keys": {
                "inferenceGatewayBaseUrl": "http://127.0.0.1:18080",
                "inferenceModels": '[{"name":"deepseek-v4-pro[1m]","supports1m":true},{"name":"deepseek-v4-flash","supports1m":true}]',
            },
        }
        raw_health = _desktop_health(raw_status, 18080, provider)
        raw_codes = {issue["code"] for issue in raw_health["issues"]}

        self.assertTrue(raw_health["needsApply"])
        self.assertIn("invalid_inference_model_names", raw_codes)

        stale_status = {
            "configured": True,
            "keys": {
                "inferenceGatewayBaseUrl": "http://127.0.0.1:18080",
                "inferenceModels": '[{"name":"claude-opus-4-7","supports1m":true},{"name":"claude-opus-4-6","supports1m":true},{"name":"claude-sonnet-4-6","supports1m":true},{"name":"claude-sonnet-4-5","supports1m":true},{"name":"claude-haiku-4-5","supports1m":true}]',
            },
        }
        stale_health = _desktop_health(stale_status, 18080, provider)
        stale_codes = {issue["code"] for issue in stale_health["issues"]}

        self.assertTrue(stale_health["needsApply"])
        self.assertIn("stale_inference_model_routes", stale_codes)

        current_status = {
            "configured": True,
            "keys": {
                "inferenceGatewayBaseUrl": "http://127.0.0.1:18080",
                "inferenceModels": registry.serialize_inference_models(provider),
            },
        }

        current_health = _desktop_health(current_status, 18080, provider)

        self.assertFalse(current_health["needsApply"])
        self.assertTrue(current_health["oneMillionReady"])

    def test_desktop_health_detects_capability_based_1m_models(self):
        provider = {
            "baseUrl": "https://dashscope.aliyuncs.com/apps/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {
                "sonnet": "qwen3.6-plus",
                "haiku": "qwen3.6-flash",
                "opus": "qwen3.6-max-preview",
                "default": "qwen3.6-plus",
            },
            "modelCapabilities": {
                "qwen3.6-plus": {"supports1m": True},
                "qwen3.6-flash": {"supports1m": True},
            },
        }

        missing = _desktop_health({
            "configured": True,
            "keys": {
                "inferenceGatewayBaseUrl": "http://127.0.0.1:18080",
                "inferenceModels": '[{"name":"claude-sonnet-4-6"},{"name":"claude-haiku-4-5"}]',
            },
        }, 18080, provider)

        ready = _desktop_health({
            "configured": True,
            "keys": {
                "inferenceGatewayBaseUrl": "http://127.0.0.1:18080",
                "inferenceModels": registry.serialize_inference_models(provider),
            },
        }, 18080, provider)

        self.assertTrue(missing["needsApply"])
        self.assertFalse(missing["oneMillionReady"])
        self.assertFalse(ready["needsApply"])
        self.assertTrue(ready["oneMillionReady"])


class CcSwitchImportTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.ccswitch_dir = Path(self.temp_dir.name) / ".cc-switch"
        self.ccswitch_dir.mkdir()
        self.db_path = self.ccswitch_dir / "cc-switch.db"
        self.old_config_dir = cfg.CONFIG_DIR
        self.old_config_file = cfg.CONFIG_FILE
        self.old_backup_dir = cfg.BACKUP_DIR
        cfg.CONFIG_DIR = os.path.join(self.temp_dir.name, "ccds")
        cfg.CONFIG_FILE = os.path.join(cfg.CONFIG_DIR, "config.json")
        cfg.BACKUP_DIR = os.path.join(cfg.CONFIG_DIR, "backups")
        cfg.save_config(copy.deepcopy(cfg.DEFAULT_CONFIG))
        self._init_ccswitch_db()

    def tearDown(self):
        cfg.CONFIG_DIR = self.old_config_dir
        cfg.CONFIG_FILE = self.old_config_file
        cfg.BACKUP_DIR = self.old_backup_dir
        self.temp_dir.cleanup()

    def _init_ccswitch_db(self):
        conn = sqlite3.connect(self.db_path)
        try:
            conn.execute(
                """
                CREATE TABLE providers (
                    id TEXT PRIMARY KEY,
                    app_type TEXT,
                    name TEXT,
                    settings_config TEXT,
                    meta TEXT,
                    is_current INTEGER,
                    sort_index INTEGER,
                    created_at INTEGER
                )
                """
            )
            conn.commit()
        finally:
            conn.close()

    def _insert_ccswitch_provider(self, provider_id, name, env, meta=None, app_type="claude", current=False):
        conn = sqlite3.connect(self.db_path)
        try:
            conn.execute(
                """
                INSERT INTO providers
                    (id, app_type, name, settings_config, meta, is_current, sort_index, created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    provider_id,
                    app_type,
                    name,
                    json.dumps({"env": env}),
                    json.dumps(meta or {}),
                    1 if current else 0,
                    0,
                    1,
                ),
            )
            conn.commit()
        finally:
            conn.close()

    def test_preview_reads_anthropic_provider_without_exposing_secret(self):
        self._insert_ccswitch_provider(
            "deepseek",
            "DeepSeek",
            {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "sk-test-secret",
                "ANTHROPIC_MODEL": "deepseek-v4-pro",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-flash",
            },
            {"apiFormat": "anthropic"},
            current=True,
        )

        providers = ccswitch_import.read_providers(root=self.ccswitch_dir)

        self.assertEqual(len(providers), 1)
        provider = providers[0]
        self.assertTrue(provider["supported"])
        self.assertTrue(provider["hasApiKey"])
        self.assertEqual(provider["apiKeyPreview"], "sk-t...cret")
        self.assertNotIn("apiKey", provider)
        self.assertEqual(provider["baseUrl"], "https://api.deepseek.com/anthropic")
        self.assertEqual(provider["models"]["default"], "deepseek-v4-pro")
        self.assertEqual(provider["models"]["haiku"], "deepseek-v4-flash")

    def test_preview_skips_openai_formats_and_local_proxy_urls(self):
        self._insert_ccswitch_provider(
            "openai",
            "OpenAI Like",
            {
                "ANTHROPIC_BASE_URL": "https://api.example.com/v1",
                "ANTHROPIC_AUTH_TOKEN": "sk-test-secret",
                "ANTHROPIC_MODEL": "example-model",
            },
            {"apiFormat": "openai_chat"},
        )
        self._insert_ccswitch_provider(
            "local",
            "Local Proxy",
            {
                "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
                "ANTHROPIC_AUTH_TOKEN": "sk-test-secret",
                "ANTHROPIC_MODEL": "local-model",
            },
            {"apiFormat": "anthropic"},
        )

        providers = {provider["id"]: provider for provider in ccswitch_import.read_providers(root=self.ccswitch_dir)}

        self.assertFalse(providers["openai"]["supported"])
        self.assertIn("OpenAI Chat", providers["openai"]["reason"])
        self.assertFalse(providers["local"]["supported"])
        self.assertIn("本机代理地址", providers["local"]["reason"])

    def test_import_adds_supported_only_and_creates_backup(self):
        self._insert_ccswitch_provider(
            "deepseek",
            "DeepSeek",
            {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "sk-test-secret",
                "ANTHROPIC_MODEL": "deepseek-v4-pro",
            },
            {"apiFormat": "anthropic"},
        )
        self._insert_ccswitch_provider(
            "openai",
            "OpenAI Like",
            {
                "ANTHROPIC_BASE_URL": "https://api.example.com/v1",
                "ANTHROPIC_AUTH_TOKEN": "sk-openai-secret",
                "ANTHROPIC_MODEL": "example-model",
            },
            {"apiFormat": "openai_responses"},
        )

        result = ccswitch_import.import_providers(root=self.ccswitch_dir)
        providers = cfg.get_providers()

        self.assertEqual(len(result["imported"]), 1)
        self.assertEqual(len(providers), 1)
        self.assertEqual(providers[0]["name"], "DeepSeek")
        self.assertEqual(providers[0]["apiFormat"], "anthropic")
        self.assertEqual(providers[0]["extraHeaders"], {"x-api-key": "{apiKey}"})
        self.assertEqual(providers[0]["source"], {"type": "cc-switch", "id": "deepseek"})
        self.assertIsNotNone(result["backup"])
        self.assertTrue(os.path.exists(os.path.join(cfg.BACKUP_DIR, result["backup"]["name"])))

    def test_import_renames_same_vendor_without_overwriting_existing_provider(self):
        cfg.add_provider({
            "id": "manual-deepseek",
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "apiKey": "manual-secret",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
        })
        self._insert_ccswitch_provider(
            "deepseek",
            "DeepSeek",
            {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "sk-test-secret",
                "ANTHROPIC_MODEL": "deepseek-v4-pro",
            },
            {"apiFormat": "anthropic"},
        )

        first = ccswitch_import.import_providers(root=self.ccswitch_dir)
        second = ccswitch_import.import_providers(root=self.ccswitch_dir)
        providers = cfg.get_providers()
        names = [provider["name"] for provider in providers]

        self.assertEqual(first["imported"][0]["name"], "DeepSeek CC Switch 导入")
        self.assertEqual(second["imported"], [])
        self.assertEqual(second["skipped"][0]["reason"], "已导入过这个 CC-Switch 配置")
        self.assertEqual(len(providers), 2)
        self.assertIn("DeepSeek", names)
        self.assertIn("DeepSeek CC Switch 导入", names)
        self.assertEqual(cfg.get_provider("manual-deepseek")["apiKey"], "manual-secret")

    def test_admin_routes_preview_masks_secret_and_import_requires_local_header(self):
        self._insert_ccswitch_provider(
            "zhipu",
            "智谱 GLM",
            {
                "ANTHROPIC_BASE_URL": "https://open.bigmodel.cn/api/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "sk-zhipu-secret",
                "ANTHROPIC_MODEL": "glm-4.7",
            },
            {"apiFormat": "anthropic"},
        )
        client = TestClient(create_admin_app())
        headers = admin_headers()

        with patch("backend.main.ccswitch_import.CCSWITCH_DIR", self.ccswitch_dir):
            status_response = client.get("/api/ccswitch/status", headers=headers)
            preview_response = client.get("/api/ccswitch/providers", headers=headers)
            blocked_import = client.post("/api/ccswitch/import", json={"ids": ["zhipu"]})
            import_response = client.post(
                "/api/ccswitch/import",
                headers=headers,
                json={"ids": ["zhipu"]},
            )

        self.assertEqual(status_response.status_code, 200)
        self.assertTrue(status_response.json()["found"])
        self.assertEqual(preview_response.status_code, 200)
        preview = preview_response.json()["providers"][0]
        self.assertNotIn("apiKey", preview)
        self.assertEqual(preview["apiKeyPreview"], "sk-z...cret")
        self.assertEqual(blocked_import.status_code, 403)
        self.assertEqual(import_response.status_code, 200)
        self.assertEqual(import_response.json()["imported"][0]["name"], "智谱 GLM")


class ProviderToolsTests(unittest.TestCase):
    def test_model_endpoint_candidates_handle_common_url_shapes(self):
        openai = provider_tools.model_endpoint_candidates({
            "baseUrl": "https://api.example.com/v1/chat/completions",
            "apiFormat": "openai",
        })
        anthropic = provider_tools.model_endpoint_candidates({
            "baseUrl": "https://api.deepseek.com/anthropic",
            "apiFormat": "anthropic",
        })

        self.assertIn("https://api.example.com/v1/models", openai)
        self.assertIn("https://api.deepseek.com/anthropic/v1/models", anthropic)
        self.assertIn("https://api.deepseek.com/models", anthropic)

        qiniu = provider_tools.model_endpoint_candidates({
            "baseUrl": "https://api.qnaigc.com",
            "apiFormat": "anthropic",
        })
        bailian = provider_tools.model_endpoint_candidates({
            "baseUrl": "https://dashscope.aliyuncs.com/apps/anthropic",
            "apiFormat": "anthropic",
        })
        self.assertIn("https://api.qnaigc.com/v1/models", qiniu)
        self.assertIn("https://dashscope.aliyuncs.com/apps/anthropic/v1/models", bailian)

    def test_extract_model_ids_and_suggest_mappings(self):
        payload = {
            "data": [
                {"id": "text-embedding-v1"},
                {"id": "deepseek-v4-pro"},
                {"id": "deepseek-v4-flash"},
            ]
        }

        models = provider_tools.extract_model_ids(payload)
        suggested = provider_tools.suggest_model_mappings(models)

        self.assertEqual(models, ["text-embedding-v1", "deepseek-v4-pro", "deepseek-v4-flash"])
        self.assertEqual(suggested["sonnet"], "deepseek-v4-pro")
        self.assertEqual(suggested["haiku"], "deepseek-v4-flash")
        self.assertEqual(suggested["default"], "deepseek-v4-pro")

    def test_normalize_openrouter_usage(self):
        items = provider_tools.normalize_balance_payload("openrouter", {
            "data": {"total_credits": 12.5, "total_usage": 2.0}
        })

        self.assertEqual(items[0]["remaining"], 10.5)
        self.assertEqual(items[0]["used"], 2.0)


class AdminApiTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.old_config_dir = cfg.CONFIG_DIR
        self.old_config_file = cfg.CONFIG_FILE
        self.old_backup_dir = cfg.BACKUP_DIR
        cfg.CONFIG_DIR = self.temp_dir.name
        cfg.CONFIG_FILE = os.path.join(self.temp_dir.name, "config.json")
        cfg.BACKUP_DIR = os.path.join(self.temp_dir.name, "backups")
        cfg.save_config(copy.deepcopy(cfg.DEFAULT_CONFIG))
        self.client = TestClient(create_admin_app())

    def tearDown(self):
        register_app_activation_handler(None)
        cfg.CONFIG_DIR = self.old_config_dir
        cfg.CONFIG_FILE = self.old_config_file
        cfg.BACKUP_DIR = self.old_backup_dir
        self.temp_dir.cleanup()

    def test_activate_app_requires_local_header_and_invokes_handler(self):
        calls = []

        def handler():
            calls.append("activated")
            return True

        register_app_activation_handler(handler)

        blocked = self.client.post("/api/app/activate")
        allowed = self.client.post(
            "/api/app/activate",
            headers={"x-ccds-request": "1"},
        )

        self.assertEqual(blocked.status_code, 403)
        self.assertEqual(allowed.status_code, 200)
        self.assertEqual(allowed.json(), {"success": True, "handled": True})
        self.assertEqual(calls, ["activated"])

    def test_ready_is_public_and_status_requires_admin_token(self):
        ready = self.client.get("/api/ready")
        blocked = self.client.get("/api/status")
        wrong_token = self.client.get(
            "/api/status",
            headers={"x-ccds-admin-token": "wrong-token"},
        )
        allowed = self.client.get("/api/status", headers=admin_headers())

        self.assertEqual(ready.status_code, 200)
        self.assertEqual(ready.json(), {"success": True, "ready": True})
        self.assertEqual(blocked.status_code, 403)
        self.assertEqual(wrong_token.status_code, 403)
        self.assertEqual(allowed.status_code, 200)
        self.assertIn("providerCount", allowed.json())

    def test_config_export_requires_admin_token_and_returns_masked_provider_list(self):
        cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "apiKey": "secret-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "extraHeaders": {"x-api-key": "{apiKey}"},
        })

        blocked = self.client.get("/api/config/export")
        static_only = self.client.get("/api/config/export", headers={"x-ccds-request": "1"})
        allowed = self.client.get("/api/config/export", headers=admin_headers())
        providers = self.client.get("/api/providers", headers=admin_headers())

        self.assertEqual(blocked.status_code, 403)
        self.assertEqual(static_only.status_code, 403)
        self.assertEqual(allowed.status_code, 200)
        self.assertEqual(allowed.json()["config"]["providers"][0]["apiKey"], "secret-key")
        public_provider = providers.json()["providers"][0]
        self.assertNotIn("apiKey", public_provider)
        self.assertNotIn("extraHeaders", public_provider)
        self.assertTrue(public_provider["hasApiKey"])

    def test_provider_secret_requires_admin_token(self):
        provider = cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "apiKey": "secret-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
        })

        blocked = self.client.get(f"/api/providers/{provider['id']}/secret")
        allowed = self.client.get(
            f"/api/providers/{provider['id']}/secret",
            headers=admin_headers(),
        )

        self.assertEqual(blocked.status_code, 403)
        self.assertEqual(allowed.status_code, 200)
        self.assertEqual(allowed.json()["apiKey"], "secret-key")

    def test_diagnostics_export_requires_token_and_redacts_secrets(self):
        provider = cfg.add_provider({
            "name": "Private Gateway",
            "baseUrl": (
                "https://user:pass@new.aibado.cn/v1/chat/completions?"
                "token=url-secret-token&access_token=access-secret-token&"
                "refresh_token=refresh-secret-token&client_secret=client-secret-token"
            ),
            "apiKey": "sk-live-secret123456",
            "authScheme": "bearer",
            "apiFormat": "openai_chat",
            "models": {"sonnet": "provider-model", "default": "provider-model"},
            "extraHeaders": {
                "Authorization": "Bearer sk-header-secret123456",
                "x-api-key": "sk-extra-secret123456",
            },
        })
        config = cfg.load_config()
        config["activeProvider"] = provider["id"]
        config["gatewayApiKey"] = "ccds_gateway_secret123456"
        cfg.save_config(config)
        log_buffer.clear()
        log_buffer.add(
            "ERROR",
            (
                "Authorization: Bearer sk-log-secret123456 "
                "url=https://example.test?token=query-secret-token&"
                "access_token=log-access-secret&refresh_token=log-refresh-secret&client_secret=log-client-secret"
            ),
        )

        blocked = self.client.post("/api/diagnostics/export")
        allowed = self.client.post("/api/diagnostics/export", headers=admin_headers())
        check = self.client.post("/api/diagnostics/check", headers=admin_headers())

        self.assertEqual(blocked.status_code, 403)
        self.assertEqual(allowed.status_code, 200)
        payload = allowed.json()["diagnostics"]
        serialized = json.dumps(payload, ensure_ascii=False)

        self.assertEqual(payload["format"], "ccds.diagnostics.v1")
        self.assertEqual(payload["providers"][0]["baseUrl"]["host"], "new.aibado.cn")
        self.assertNotIn("sk-live-secret123456", serialized)
        self.assertNotIn("sk-header-secret123456", serialized)
        self.assertNotIn("sk-extra-secret123456", serialized)
        self.assertNotIn("ccds_gateway_secret123456", serialized)
        self.assertNotIn("user:pass", serialized)
        self.assertNotIn("url-secret-token", serialized)
        self.assertNotIn("access-secret-token", serialized)
        self.assertNotIn("refresh-secret-token", serialized)
        self.assertNotIn("client-secret-token", serialized)
        self.assertNotIn("query-secret-token", serialized)
        self.assertNotIn("log-access-secret", serialized)
        self.assertNotIn("log-refresh-secret", serialized)
        self.assertNotIn("log-client-secret", serialized)
        self.assertIn("******", serialized)
        self.assertEqual(check.status_code, 200)
        self.assertIn("checks", check.json())

    def test_autofill_models_route_updates_provider_mapping(self):
        provider = cfg.add_provider({
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.cn/v1",
            "authScheme": "bearer",
            "apiFormat": "openai",
        })

        async def fake_fetch(_provider):
            return {
                "success": True,
                "endpoint": "https://api.moonshot.cn/v1/models",
                "models": ["kimi-k2.6"],
                "suggested": {
                    "sonnet": "kimi-k2.6",
                    "haiku": "kimi-k2.6",
                    "opus": "kimi-k2.6",
                    "default": "kimi-k2.6",
                },
            }

        with patch("backend.main.provider_tools.fetch_provider_models", fake_fetch):
            response = self.client.post(
                f"/api/providers/{provider['id']}/models/autofill",
                headers=admin_headers(),
            )

        self.assertEqual(response.status_code, 200)
        self.assertEqual(cfg.get_provider(provider["id"])["models"]["default"], "kimi-k2.6")

    def test_fetch_models_from_unsaved_provider_payload(self):
        async def fake_fetch(provider):
            self.assertEqual(provider["baseUrl"], "https://api.example.com/v1")
            return {
                "success": True,
                "endpoint": "https://api.example.com/v1/models",
                "models": ["example-pro"],
                "suggested": {
                    "sonnet": "example-pro",
                    "haiku": "example-pro",
                    "opus": "example-pro",
                    "default": "example-pro",
                },
            }

        with patch("backend.main.provider_tools.fetch_provider_models", fake_fetch):
            response = self.client.post(
                "/api/providers/models/available",
                headers=admin_headers(),
                json={
                    "name": "Example",
                    "baseUrl": "https://api.example.com/v1",
                    "apiKey": "sk-test",
                    "authScheme": "bearer",
                    "apiFormat": "openai",
                },
            )

        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json()["suggested"]["default"], "example-pro")

    def test_provider_connection_marks_auth_failure_as_not_ok(self):
        class FakeResponse:
            def __init__(self, status_code=401):
                self.status_code = status_code

        class FakeClient:
            def __init__(self, *args, **kwargs):
                pass

            async def __aenter__(self):
                return self

            async def __aexit__(self, exc_type, exc, tb):
                return False

            async def head(self, *args, **kwargs):
                return FakeResponse(401)

            async def get(self, *args, **kwargs):
                return FakeResponse(401)

        with patch("backend.main.httpx.AsyncClient", FakeClient):
            result = asyncio.run(_test_provider_connection({
                "name": "Kimi",
                "baseUrl": "https://api.moonshot.ai/anthropic",
                "apiKey": "bad-key",
                "authScheme": "bearer",
                "apiFormat": "anthropic",
            }))

        self.assertTrue(result["success"])
        self.assertFalse(result["ok"])
        self.assertEqual(result["statusCode"], 401)
        self.assertIn("Kimi 认证失败", result["message"])
        self.assertIn("https://api.moonshot.cn/anthropic", result["message"])
        self.assertIn("https://api.kimi.com/coding", result["message"])

    def test_provider_connection_probes_post_when_head_and_get_are_not_supported(self):
        calls = []

        class FakeResponse:
            def __init__(self, status_code):
                self.status_code = status_code

        class FakeClient:
            def __init__(self, *args, **kwargs):
                pass

            async def __aenter__(self):
                return self

            async def __aexit__(self, exc_type, exc, tb):
                return False

            async def head(self, *args, **kwargs):
                calls.append(("head", args, kwargs))
                return FakeResponse(404)

            async def get(self, *args, **kwargs):
                calls.append(("get", args, kwargs))
                return FakeResponse(404)

            async def post(self, *args, **kwargs):
                calls.append(("post", args, kwargs))
                return FakeResponse(401)

        with patch("backend.main.httpx.AsyncClient", FakeClient):
            result = asyncio.run(_test_provider_connection({
                "name": "Kimi",
                "baseUrl": "https://api.moonshot.ai/anthropic",
                "apiKey": "bad-key",
                "authScheme": "bearer",
                "apiFormat": "anthropic",
                "models": {"default": "kimi-k2.6"},
            }))

        self.assertEqual([call[0] for call in calls], ["head", "get", "post"])
        self.assertEqual(calls[-1][2]["json"]["model"], "kimi-k2.6")
        self.assertFalse(result["ok"])
        self.assertEqual(result["statusCode"], 401)
        self.assertIn("Kimi 认证失败", result["message"])

    def test_usage_route_returns_normalized_provider_tools_result(self):
        provider = cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "apiKey": "secret-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
        })

        async def fake_usage(_provider):
            return {
                "success": True,
                "supported": True,
                "ok": True,
                "items": [{"label": "CNY", "remaining": 10.0, "unit": "CNY"}],
            }

        with patch("backend.main.provider_tools.query_provider_usage", fake_usage):
            response = self.client.post(
                f"/api/providers/{provider['id']}/usage",
                headers=admin_headers(),
            )

        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json()["items"][0]["remaining"], 10.0)

    def test_reorder_providers_route_saves_drag_order(self):
        first = cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
        })
        second = cfg.add_provider({
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.cn/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
        })

        response = self.client.put(
            "/api/providers/reorder",
            headers=admin_headers(),
            json={"providerIds": [second["id"], first["id"]]},
        )

        self.assertEqual(response.status_code, 200)
        self.assertEqual(cfg.get_providers()[0]["id"], second["id"])

    def test_update_check_uses_default_url_when_settings_are_blank(self):
        config = cfg.load_config()
        config["settings"]["updateUrl"] = ""
        cfg.save_config(config)
        observed = {}

        async def fake_check_update(url, current_version, platform="windows-x64"):
            observed["url"] = url
            observed["current_version"] = current_version
            observed["platform"] = platform
            return {
                "success": True,
                "updateAvailable": False,
                "currentVersion": current_version,
                "latestVersion": current_version,
                "platform": platform,
                "assets": [],
                "updateProtocol": 1,
            }

        with patch("backend.main.updater.current_platform", return_value="windows-x64"):
            with patch("backend.main.updater.check_update", fake_check_update):
                response = self.client.get("/api/update/check", headers=admin_headers())

        self.assertEqual(response.status_code, 200)
        self.assertEqual(observed["url"], cfg.DEFAULT_UPDATE_URL)
        self.assertEqual(observed["platform"], "windows-x64")

    def test_update_check_uses_current_platform(self):
        observed = {}

        async def fake_check_update(url, current_version, platform="windows-x64"):
            observed["platform"] = platform
            return {
                "success": True,
                "updateAvailable": False,
                "currentVersion": current_version,
                "latestVersion": current_version,
                "platform": platform,
                "assets": [],
            }

        with patch("backend.main.updater.current_platform", return_value="macos-arm64"):
            with patch("backend.main.updater.check_update", fake_check_update):
                response = self.client.get("/api/update/check", headers=admin_headers())

        self.assertEqual(response.status_code, 200)
        self.assertEqual(observed["platform"], "macos-arm64")

    def test_update_check_accepts_explicit_platform(self):
        observed = {}

        async def fake_check_update(url, current_version, platform="windows-x64"):
            observed["platform"] = platform
            return {
                "success": True,
                "updateAvailable": False,
                "currentVersion": current_version,
                "latestVersion": current_version,
                "platform": platform,
                "assets": [],
            }

        with patch("backend.main.updater.check_update", fake_check_update):
            response = self.client.get("/api/update/check?platform=macos-arm64", headers=admin_headers())

        self.assertEqual(response.status_code, 200)
        self.assertEqual(observed["platform"], "macos-arm64")

    def test_set_default_provider_syncs_desktop_models_to_local_proxy_policy(self):
        first = cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
        })
        second = cfg.add_provider({
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.cn/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "kimi-k2.6", "default": "kimi-k2.6"},
        })
        self.assertEqual(cfg.load_config()["activeProvider"], first["id"])

        with patch("backend.main.registry.apply_config", return_value={"success": True}) as apply_config:
            with patch("backend.main._start_proxy_server", return_value=True) as start_proxy:
                response = self.client.put(
                    f"/api/providers/{second['id']}/default",
                    headers=admin_headers(),
                )

        self.assertEqual(response.status_code, 200)
        data = response.json()
        self.assertTrue(data["desktopSync"]["attempted"])
        self.assertTrue(data["desktopSync"]["success"])
        self.assertTrue(data["desktopSync"]["proxyStarted"])
        start_proxy.assert_called_once_with(18080)
        self.assertEqual(apply_config.call_args.args[0], "http://127.0.0.1:18080")
        self.assertTrue(apply_config.call_args.kwargs["gateway_api_key"].startswith("ccds_"))
        self.assertEqual(apply_config.call_args.kwargs["auth_scheme"], "bearer")
        self.assertEqual(apply_config.call_args.kwargs["gateway_headers"], "")
        self.assertFalse(apply_config.call_args.kwargs["expose_all"])
        self.assertIsNone(apply_config.call_args.kwargs["providers"])
        self.assertEqual(apply_config.call_args.kwargs["provider"]["models"]["sonnet"], "kimi-k2.6")
        self.assertEqual(cfg.load_config()["activeProvider"], second["id"])

    def test_set_default_provider_keeps_single_provider_when_expose_all_is_enabled(self):
        first = cfg.add_provider({
            "id": "deepseek",
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "deepseek-v4-pro[1m]", "default": "deepseek-v4-pro[1m]"},
        })
        second = cfg.add_provider({
            "id": "kimi",
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.cn/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "kimi-k2.6", "default": "kimi-k2.6"},
        })
        cfg.update_settings({"exposeAllProviderModels": True})

        with patch("backend.main.registry.apply_config", return_value={"success": True}) as apply_config:
            with patch("backend.main._start_proxy_server", return_value=True):
                response = self.client.put(
                    f"/api/providers/{second['id']}/default",
                    headers=admin_headers(),
                )

        self.assertEqual(response.status_code, 200)
        self.assertTrue(response.json()["desktopSync"]["attempted"])
        self.assertFalse(apply_config.call_args.kwargs["expose_all"])
        self.assertIsNone(apply_config.call_args.kwargs["providers"])
        self.assertEqual(apply_config.call_args.kwargs["provider"]["id"], second["id"])

    def test_configure_desktop_fails_when_proxy_start_fails(self):
        cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "deepseek-v4-pro", "default": "deepseek-v4-pro"},
        })

        with patch("backend.main.registry.apply_config", return_value={"success": True}) as apply_config:
            with patch("backend.main._start_proxy_server", return_value=False) as start_proxy:
                response = self.client.post(
                    "/api/desktop/configure",
                    headers=admin_headers(),
                )

        self.assertEqual(response.status_code, 200)
        data = response.json()
        self.assertFalse(data["success"])
        self.assertTrue(data["configSuccess"])
        self.assertTrue(data["requiresProxy"])
        self.assertFalse(data["proxyStarted"])
        self.assertEqual(data["mode"], "local_proxy")
        self.assertIn("本机转发服务启动失败", data["message"])
        self.assertEqual(apply_config.call_args.args[0], "http://127.0.0.1:18080")
        start_proxy.assert_called_once_with(18080)

    def test_configure_desktop_fails_without_explicit_claude_model_routes(self):
        cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"default": "deepseek-v4-pro"},
        })

        with patch("backend.main.registry.apply_config") as apply_config:
            response = self.client.post(
                "/api/desktop/configure",
                headers=admin_headers(),
            )

        self.assertEqual(response.status_code, 200)
        data = response.json()
        self.assertFalse(data["success"])
        self.assertFalse(data["configSuccess"])
        self.assertTrue(data["requiresProxy"])
        self.assertEqual(data["mode"], "local_proxy")
        self.assertIn("至少映射一个 Claude 模型槽位", data["message"])
        apply_config.assert_not_called()

    def test_provider_compatibility_report_marks_openai_as_experimental(self):
        cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
        })
        cfg.add_provider({
            "name": "Custom OpenAI",
            "baseUrl": "https://api.example.com/v1",
            "authScheme": "bearer",
            "apiFormat": "openai_chat",
        })

        response = self.client.get("/api/providers/compatibility", headers=admin_headers())

        self.assertEqual(response.status_code, 200)
        by_name = {item["name"]: item for item in response.json()["providers"]}
        self.assertEqual(by_name["DeepSeek"]["level"], "stable")
        self.assertEqual(by_name["Custom OpenAI"]["level"], "experimental")
        self.assertFalse(by_name["Custom OpenAI"]["checks"]["streamingTools"])

    def test_update_install_does_not_launch_when_current_version_is_latest(self):
        async def fake_download_update(url, current_version, platform="windows-x64", target_dir=None):
            return {
                "success": True,
                "updateAvailable": False,
                "currentVersion": current_version,
                "latestVersion": current_version,
                "platform": platform,
                "assets": [],
                "downloaded": False,
                "message": "当前已是最新版本",
            }

        with patch("backend.main.updater.current_platform", return_value="windows-x64"):
            with patch("backend.main.updater.download_update", fake_download_update):
                with patch("backend.main._popen_hidden") as popen:
                    response = self.client.post(
                        "/api/update/install",
                        headers=admin_headers(),
                        json={},
                    )

        self.assertEqual(response.status_code, 200)
        self.assertFalse(response.json()["updateAvailable"])
        popen.assert_not_called()

    def test_update_install_launches_downloaded_installer(self):
        async def fake_download_update(url, current_version, platform="windows-x64", target_dir=None):
            return {
                "success": True,
                "updateAvailable": True,
                "currentVersion": current_version,
                "latestVersion": "1.0.11",
                "platform": platform,
                "assets": [],
                "downloaded": True,
                "installerPath": r"C:\Temp\CC-Desktop-Switch-v1.0.11-Windows-Setup.exe",
            }

        with patch("backend.main.updater.current_platform", return_value="windows-x64"):
            with patch("backend.main.updater.download_update", fake_download_update):
                with patch("backend.main._popen_hidden") as popen:
                    response = self.client.post(
                        "/api/update/install",
                        headers=admin_headers(),
                        json={},
                    )

        self.assertEqual(response.status_code, 200)
        self.assertTrue(response.json()["installerStarted"])
        popen.assert_called_once_with([r"C:\Temp\CC-Desktop-Switch-v1.0.11-Windows-Setup.exe"], detached=True)

    def test_update_install_opens_downloaded_macos_package(self):
        async def fake_download_update(url, current_version, platform="windows-x64", target_dir=None):
            return {
                "success": True,
                "updateAvailable": True,
                "currentVersion": current_version,
                "latestVersion": "1.0.11",
                "platform": platform,
                "assets": [],
                "downloaded": True,
                "installerPath": "/tmp/CC-Desktop-Switch-v1.0.11-macOS-arm64.pkg",
            }

        with patch("backend.main.updater.current_platform", return_value="macos-arm64"):
            with patch("backend.main.updater.download_update", fake_download_update):
                with patch("backend.main._popen_hidden") as popen:
                    response = self.client.post(
                        "/api/update/install",
                        headers=admin_headers(),
                        json={},
                    )

        self.assertEqual(response.status_code, 200)
        self.assertTrue(response.json()["installerStarted"])
        self.assertEqual(response.json()["platform"], "macos-arm64")
        self.assertFalse(response.json()["quitRequested"])
        popen.assert_called_once_with(["open", "/tmp/CC-Desktop-Switch-v1.0.11-macOS-arm64.pkg"], detached=True)

    def test_update_install_waits_for_macos_quit_before_opening_package(self):
        async def fake_download_update(url, current_version, platform="windows-x64", target_dir=None):
            return {
                "success": True,
                "updateAvailable": True,
                "currentVersion": current_version,
                "latestVersion": "1.0.11",
                "platform": platform,
                "assets": [],
                "downloaded": True,
                "installerPath": "/tmp/CC-Desktop-Switch-v1.0.11-macOS-arm64.pkg",
            }

        with patch("backend.main.updater.current_platform", return_value="macos-arm64"):
            with patch("backend.main.updater.download_update", fake_download_update):
                with patch("backend.main._get_update_quit_handler", return_value=lambda: None):
                    with patch("backend.main._schedule_update_quit_for_install", return_value=True):
                        with patch("backend.main.os.getpid", return_value=4321):
                            with patch("backend.main._popen_hidden") as popen:
                                response = self.client.post(
                                    "/api/update/install",
                                    headers=admin_headers(),
                                    json={},
                                )

        self.assertEqual(response.status_code, 200)
        self.assertTrue(response.json()["installerStarted"])
        self.assertTrue(response.json()["quitRequested"])
        self.assertEqual(response.json()["platform"], "macos-arm64")
        popen.assert_called_once_with(
            updater.install_after_quit_command(
                "/tmp/CC-Desktop-Switch-v1.0.11-macOS-arm64.pkg",
                "macos-arm64",
                4321,
            ),
            detached=True,
        )


class ReleaseManifestTests(unittest.TestCase):
    VERSION = "1.0.20"

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.root = Path(__file__).resolve().parents[1]
        self.staging = Path(self.temp_dir.name) / "staging"
        self.key_dir = Path(self.temp_dir.name) / "keys"
        self.staging.mkdir()

    def tearDown(self):
        self.temp_dir.cleanup()

    def _powershell(self):
        executable = shutil.which("powershell") or shutil.which("pwsh")
        if not executable:
            self.skipTest("PowerShell is not available")
        return executable

    def _touch_asset(self, name):
        path = self.staging / name
        path.write_bytes(f"asset:{name}".encode("utf-8"))
        return path

    def _write_windows_assets(self):
        for name in (
            f"CC-Desktop-Switch-v{self.VERSION}-Windows-x64.exe",
            f"CC-Desktop-Switch-v{self.VERSION}-Windows-Portable.zip",
            f"CC-Desktop-Switch-v{self.VERSION}-Windows-Setup.exe",
        ):
            self._touch_asset(name)

    def _write_macos_assets(self):
        for name in (
            f"CC-Desktop-Switch-v{self.VERSION}-macOS-arm64.pkg",
            f"CC-Desktop-Switch-v{self.VERSION}-macOS-arm64.dmg",
        ):
            self._touch_asset(name)

    def _run_manifest(self):
        script = self.root / "scripts" / "New-ReleaseManifest.ps1"
        return subprocess.run(
            [
                self._powershell(),
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                str(script),
                "-Version",
                self.VERSION,
                "-StagingDir",
                str(self.staging),
                "-Repository",
                "lonr-6/cc-desktop-switch",
                "-KeyDir",
                str(self.key_dir),
            ],
            cwd=self.root,
            text=True,
            capture_output=True,
            timeout=60,
        )

    def test_release_manifest_fails_when_macos_asset_is_missing(self):
        self._write_windows_assets()

        result = self._run_manifest()

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            f"Required release asset missing: CC-Desktop-Switch-v{self.VERSION}-macOS-arm64.pkg",
            result.stdout + result.stderr,
        )

    def test_release_manifest_generates_signed_platform_complete_latest_json(self):
        self._write_windows_assets()
        self._write_macos_assets()

        result = self._run_manifest()

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        latest = json.loads((self.staging / "latest.json").read_text(encoding="utf-8"))
        self.assertEqual(latest["version"], self.VERSION)
        self.assertIn("windows-x64", latest["platforms"])
        self.assertIn("macos-arm64", latest["platforms"])
        self.assertTrue((self.staging / "latest.json.sha256").exists())
        self.assertTrue((self.staging / "latest.json.sig").exists())
        self.assertTrue((self.staging / "CC-Desktop-Switch-release-public.pem").exists())
        for asset in (
            f"CC-Desktop-Switch-v{self.VERSION}-Windows-x64.exe",
            f"CC-Desktop-Switch-v{self.VERSION}-Windows-Portable.zip",
            f"CC-Desktop-Switch-v{self.VERSION}-Windows-Setup.exe",
            f"CC-Desktop-Switch-v{self.VERSION}-macOS-arm64.pkg",
            f"CC-Desktop-Switch-v{self.VERSION}-macOS-arm64.dmg",
        ):
            self.assertTrue((self.staging / f"{asset}.sha256").exists(), asset)
            self.assertTrue((self.staging / f"{asset}.sig").exists(), asset)

    def test_release_workflow_pins_builds_to_tag_and_clears_draft_assets(self):
        workflow = (self.root / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        new_release = (self.root / "scripts" / "New-Release.ps1").read_text(encoding="utf-8")
        installer = (self.root / "installer.nsi").read_text(encoding="utf-8")

        self.assertIn('tag_sha="$(git rev-list -n 1 "$tag")"', workflow)
        self.assertGreaterEqual(workflow.count("ref: ${{ needs.prepare.outputs.tag }}"), 3)
        self.assertIn("release delete-asset", workflow)
        self.assertIn('"/DPRODUCT_VERSION=$Version"', new_release)
        self.assertIn("!ifndef PRODUCT_VERSION", installer)


class SingleInstanceStartupTests(unittest.TestCase):
    class FakeKernel32:
        def __init__(self, handle, last_error):
            self.handle = handle
            self.last_error = last_error
            self.closed = []

        def CreateMutexW(self, security_attributes, initial_owner, name):
            self.requested_name = name
            return self.handle

        def GetLastError(self):
            return self.last_error

        def CloseHandle(self, handle):
            self.closed.append(handle)
            return True

    class FakeWinDll:
        def __init__(self, kernel32):
            self.kernel32 = kernel32

    class FakeActivationResponse:
        status = 200

        def __init__(self, body):
            self.body = body

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb):
            return False

        def read(self):
            return self.body

    def test_windows_single_instance_lock_keeps_first_mutex_handle(self):
        kernel32 = self.FakeKernel32(handle=123, last_error=0)

        with patch("main.sys.platform", "win32"):
            with patch("main.ctypes.windll", self.FakeWinDll(kernel32)):
                with patch("main._single_instance_mutex", None):
                    self.assertTrue(acquire_single_instance_lock())
                    self.assertEqual(kernel32.requested_name, "Local\\CCDesktopSwitch.SingleInstance")
                    self.assertEqual(kernel32.closed, [])
                    release_single_instance_lock()

        self.assertEqual(kernel32.closed, [123])

    def test_windows_single_instance_lock_rejects_existing_instance(self):
        kernel32 = self.FakeKernel32(handle=456, last_error=183)

        with patch("main.sys.platform", "win32"):
            with patch("main.ctypes.windll", self.FakeWinDll(kernel32)):
                with patch("main._single_instance_mutex", None):
                    self.assertFalse(acquire_single_instance_lock())

        self.assertEqual(kernel32.closed, [456])

    def test_request_existing_instance_activate_posts_local_header(self):
        calls = []

        def fake_urlopen(request, timeout):
            calls.append((request, timeout))
            return self.FakeActivationResponse(b'{"success": true, "handled": true}')

        with patch("main.urlopen", fake_urlopen):
            result = request_existing_instance_activate(18081, timeout=0)

        self.assertTrue(result)
        self.assertEqual(len(calls), 1)
        request, timeout = calls[0]
        self.assertEqual(request.full_url, "http://127.0.0.1:18081/api/app/activate")
        self.assertEqual(request.get_method(), "POST")
        self.assertEqual(request.get_header("X-ccds-request"), "1")
        self.assertGreater(timeout, 0)

    def test_request_existing_instance_activate_requires_window_handler(self):
        def fake_urlopen(request, timeout):
            return self.FakeActivationResponse(b'{"success": true, "handled": false}')

        with patch("main.urlopen", fake_urlopen):
            result = request_existing_instance_activate(18081, timeout=0)

        self.assertFalse(result)

    def test_browser_mode_does_not_print_admin_token(self):
        messages = []

        def fake_safe_print(message):
            messages.append(str(message))

        with patch("main.safe_print", fake_safe_print):
            with patch("main.uvicorn.run") as uvicorn_run:
                run_browser_mode(create_admin_app(), 18081, open_ui=False)

        printed = "\n".join(messages)
        uvicorn_run.assert_called_once()
        self.assertIn(admin_public_url(18081), printed)
        self.assertNotIn(admin_ui_url(18081), printed)
        self.assertNotIn(get_admin_token(), printed)


class ProxyConversionTests(unittest.TestCase):
    def test_build_upstream_url_accepts_base_url_or_full_endpoint(self):
        self.assertEqual(
            build_upstream_url("https://api.deepseek.com/anthropic", "anthropic"),
            "https://api.deepseek.com/anthropic/v1/messages",
        )
        self.assertEqual(
            build_upstream_url("https://api.deepseek.com/anthropic/v1/messages", "anthropic"),
            "https://api.deepseek.com/anthropic/v1/messages",
        )
        self.assertEqual(
            build_upstream_url("https://api.anthropic-compatible.test/v1", "anthropic"),
            "https://api.anthropic-compatible.test/v1/messages",
        )
        self.assertEqual(
            build_upstream_url("https://api.moonshot.ai/v1", "openai"),
            "https://api.moonshot.ai/v1/chat/completions",
        )
        self.assertEqual(
            build_upstream_url("https://api.moonshot.ai/v1/chat/completions", "openai"),
            "https://api.moonshot.ai/v1/chat/completions",
        )

    def test_anthropic_to_openai_body_flattens_text_blocks_without_mutating_input(self):
        body = {
            "model": "kimi-k2.6",
            "system": [{"type": "text", "text": "Be brief."}],
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Hello"},
                        {"type": "text", "text": "World"},
                    ],
                }
            ],
            "max_tokens": 32,
        }

        converted = _anthropic_to_openai_body(body, stream=False)

        self.assertEqual(converted["messages"][0], {"role": "system", "content": "Be brief."})
        self.assertEqual(converted["messages"][1], {"role": "user", "content": "Hello\nWorld"})
        self.assertIsInstance(body["messages"][0]["content"], list)

    def test_anthropic_to_openai_body_converts_tools_and_tool_results(self):
        body = {
            "model": "custom-model",
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "I will call a tool."},
                        {"type": "tool_use", "id": "toolu_1", "name": "read_file", "input": {"path": "README.md"}},
                    ],
                },
                {
                    "role": "user",
                    "content": [
                        {"type": "tool_result", "tool_use_id": "toolu_1", "content": "ok"},
                    ],
                },
            ],
            "tools": [
                {
                    "name": "read_file",
                    "description": "Read a file",
                    "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}},
                }
            ],
            "tool_choice": {"type": "any"},
        }

        converted = _anthropic_to_openai_body(body, stream=False)

        self.assertEqual(converted["tools"][0]["function"]["name"], "read_file")
        self.assertEqual(converted["tool_choice"], "required")
        self.assertEqual(converted["messages"][0]["tool_calls"][0]["id"], "toolu_1")
        self.assertEqual(
            json.loads(converted["messages"][0]["tool_calls"][0]["function"]["arguments"]),
            {"path": "README.md"},
        )
        self.assertEqual(converted["messages"][1], {"role": "tool", "tool_call_id": "toolu_1", "content": "ok"})

    def test_openai_response_converts_tool_calls_and_usage_to_anthropic(self):
        response = _openai_to_anthropic({
            "id": "chatcmpl_1",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "search", "arguments": "{\"q\":\"hello\"}"},
                    }],
                },
                "finish_reason": "tool_calls",
            }],
            "usage": {"prompt_tokens": 11, "completion_tokens": 3},
        }, "custom-model")

        self.assertEqual(response["stop_reason"], "tool_use")
        self.assertEqual(response["usage"], {"input_tokens": 11, "output_tokens": 3})
        self.assertEqual(response["content"][0]["type"], "tool_use")
        self.assertEqual(response["content"][0]["input"], {"q": "hello"})

    def test_openai_streaming_tool_calls_return_clear_experimental_error(self):
        event = _openai_chunk_to_anthropic({
            "choices": [{
                "delta": {"tool_calls": [{"id": "call_1"}]},
                "finish_reason": None,
            }]
        }, "custom-model")

        self.assertEqual(event["type"], "error")
        self.assertEqual(event["error"]["type"], "unsupported_streaming_tool_call")

    def test_map_model_preserves_exact_gateway_model_ids(self):
        provider = {
            "models": {
                "sonnet": "deepseek-v4-pro[1m]",
                "haiku": "deepseek-v4-flash",
                "opus": "deepseek-v4-pro[1m]",
                "default": "deepseek-v4-pro[1m]",
            }
        }

        self.assertEqual(map_model("deepseek-v4-pro[1m]", provider), "deepseek-v4-pro[1m]")
        self.assertEqual(map_model("claude-sonnet-4-6", provider), "deepseek-v4-pro[1m]")
        self.assertEqual(map_model("claude-haiku-4-5", provider), "deepseek-v4-flash")
        self.assertEqual(map_model("claude-sonnet-4-5", provider), "claude-sonnet-4-5")

    def test_gateway_models_response_exposes_safe_route_model_ids(self):
        provider = {
            "models": {
                "sonnet": "deepseek-v4-pro",
                "haiku": "deepseek-v4-flash",
                "opus": "deepseek-v4-pro",
                "default": "deepseek-v4-pro",
            }
        }

        response = gateway_models_response(provider)
        ids = [item["id"] for item in response["data"]]

        self.assertIn("claude-sonnet-4-6", ids)
        self.assertIn("claude-haiku-4-5", ids)
        self.assertNotIn("claude-opus-4-6", ids)
        self.assertNotIn("claude-3-opus", ids)
        self.assertNotIn("claude-sonnet-4-5", ids)
        self.assertNotIn("deepseek-v4-pro", ids)
        self.assertNotIn("deepseek-v4-flash", ids)

    def test_deepseek_request_options_force_max_effort_and_keep_thinking(self):
        provider = {
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "requestOptions": {
                "anthropic": {
                    "thinking": {"type": "enabled"},
                    "output_config": {"effort": "max"},
                }
            },
        }
        body = {
            "model": "deepseek-v4-pro",
            "thinking": {"type": "enabled"},
            "output_config": {"effort": "high"},
        }

        result = apply_anthropic_request_options(body, provider)

        self.assertEqual(result["thinking"], {"type": "enabled"})
        self.assertEqual(result["output_config"]["effort"], "max")

    def test_deepseek_without_max_preserves_current_effort(self):
        provider = {
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
        }
        body = {
            "model": "deepseek-v4-pro",
            "thinking": {"type": "enabled"},
            "output_config": {"effort": "low"},
        }

        result = apply_anthropic_request_options(body, provider)

        self.assertEqual(result["thinking"], {"type": "enabled"})
        self.assertEqual(result["output_config"]["effort"], "low")

    def test_non_deepseek_request_options_keep_legacy_thinking_strip(self):
        provider = {
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.cn/anthropic",
        }
        body = {
            "model": "kimi-k2.6",
            "thinking": {"type": "enabled"},
            "output_config": {"effort": "high"},
        }

        result = apply_anthropic_request_options(body, provider)

        self.assertNotIn("thinking", result)
        self.assertEqual(result["output_config"]["effort"], "high")

    def test_anthropic_response_normalization_adds_usage_fields(self):
        response = _normalize_anthropic_response({
            "id": "msg_test",
            "type": "message",
            "role": "assistant",
            "content": "hello",
        }, "kimi-k2.6")

        self.assertEqual(response["content"], [{"type": "text", "text": "hello"}])
        self.assertEqual(response["usage"]["input_tokens"], 0)
        self.assertEqual(response["usage"]["output_tokens"], 0)

    def test_anthropic_stream_normalization_adds_message_start_usage(self):
        event = _normalize_anthropic_sse_event({
            "type": "message_start",
            "message": {
                "id": "msg_test",
                "type": "message",
                "role": "assistant",
                "content": [],
            },
        }, "kimi-k2.6")

        self.assertEqual(event["message"]["usage"]["input_tokens"], 0)
        self.assertEqual(event["message"]["usage"]["output_tokens"], 0)

    def test_openai_stream_message_start_includes_usage_fields(self):
        event = _openai_chunk_to_anthropic({
            "choices": [{"delta": {"role": "assistant"}, "finish_reason": None}]
        }, "kimi-k2.6")

        self.assertEqual(event["message"]["usage"]["input_tokens"], 0)
        self.assertEqual(event["message"]["usage"]["output_tokens"], 0)

    def test_openai_non_json_success_response_returns_structured_diagnostic_error(self):
        class FakeElapsed:
            def total_seconds(self):
                return 0.01

        class FakeResponse:
            status_code = 200
            is_success = True
            headers = {"content-type": "text/html; charset=utf-8"}
            text = (
                "<html>login required token=secret-token "
                "access_token=access-secret-token refresh_token=refresh-secret-token "
                "client_secret=client-secret-token Authorization: Bearer sk-live-secret123456</html>"
            )
            elapsed = FakeElapsed()

            def json(self):
                raise json.JSONDecodeError("Expecting value", self.text, 0)

        class FakeClient:
            def __init__(self, *args, **kwargs):
                pass

            async def __aenter__(self):
                return self

            async def __aexit__(self, exc_type, exc, tb):
                return False

            async def post(self, *args, **kwargs):
                return FakeResponse()

        provider = {
            "name": "Private Relay",
            "baseUrl": "https://new.aibado.cn/v1/chat/completions",
            "apiKey": "sk-provider-secret123456",
            "authScheme": "bearer",
            "apiFormat": "openai_chat",
        }
        body = {
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 8,
        }

        with patch("backend.proxy.httpx.AsyncClient", FakeClient):
            result = asyncio.run(forward_request(body, provider, "req-test"))

        error = result["error"]
        serialized = json.dumps(error, ensure_ascii=False)
        self.assertEqual(error["type"], "invalid_upstream_response")
        self.assertEqual(error["status"], 200)
        self.assertEqual(error["contentType"], "text/html; charset=utf-8")
        self.assertEqual(error["upstreamHost"], "new.aibado.cn")
        self.assertEqual(error["apiFormat"], "openai_chat")
        self.assertIn("bodyPreview", error)
        self.assertNotIn("sk-live-secret123456", serialized)
        self.assertNotIn("secret-token", serialized)
        self.assertNotIn("access-secret-token", serialized)
        self.assertNotIn("refresh-secret-token", serialized)
        self.assertNotIn("client-secret-token", serialized)
        self.assertIn("******", serialized)

    def test_openai_non_success_error_response_is_redacted(self):
        class FakeElapsed:
            def total_seconds(self):
                return 0.01

        class FakeResponse:
            status_code = 401
            is_success = False
            headers = {"content-type": "text/html"}
            text = (
                "<html>bad auth access_token=access-secret-token&refresh_token=refresh-secret-token "
                "client_secret=client-secret-token Authorization: Bearer sk-error-secret123456</html>"
            )
            elapsed = FakeElapsed()

        class FakeClient:
            def __init__(self, *args, **kwargs):
                pass

            async def __aenter__(self):
                return self

            async def __aexit__(self, exc_type, exc, tb):
                return False

            async def post(self, *args, **kwargs):
                return FakeResponse()

        provider = {
            "name": "Private Relay",
            "baseUrl": "https://new.aibado.cn/v1/chat/completions",
            "apiKey": "sk-provider-secret123456",
            "authScheme": "bearer",
            "apiFormat": "openai_chat",
        }
        body = {
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 8,
        }

        with patch("backend.proxy.httpx.AsyncClient", FakeClient):
            result = asyncio.run(forward_request(body, provider, "req-401"))

        error = result["error"]
        serialized = json.dumps(error, ensure_ascii=False)
        self.assertEqual(error["type"], "upstream_error")
        self.assertEqual(error["status"], 401)
        self.assertNotIn("sk-error-secret123456", serialized)
        self.assertNotIn("access-secret-token", serialized)
        self.assertNotIn("refresh-secret-token", serialized)
        self.assertNotIn("client-secret-token", serialized)
        self.assertIn("******", serialized)

    def test_openai_streaming_non_sse_success_response_returns_sse_error_event(self):
        class FakeStreamResponse:
            status_code = 200
            is_success = True
            headers = {"content-type": "text/html"}

            async def aread(self):
                return b"<html>bad relay token=stream-secret-token Authorization: Bearer sk-stream-secret123456</html>"

            async def aiter_lines(self):
                yield "should not be used"

        class FakeStreamContext:
            async def __aenter__(self):
                return FakeStreamResponse()

            async def __aexit__(self, exc_type, exc, tb):
                return False

        class FakeClient:
            def __init__(self, *args, **kwargs):
                pass

            async def __aenter__(self):
                return self

            async def __aexit__(self, exc_type, exc, tb):
                return False

            def stream(self, *args, **kwargs):
                return FakeStreamContext()

        provider = {
            "name": "Private Relay",
            "baseUrl": "https://new.aibado.cn/v1/chat/completions",
            "apiKey": "sk-provider-secret123456",
            "authScheme": "bearer",
            "apiFormat": "openai_chat",
        }
        body = {
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 8,
            "stream": True,
        }

        async def collect():
            chunks = []
            async for chunk in forward_request_stream(body, provider, "req-stream"):
                chunks.append(chunk)
            return "".join(chunks)

        with patch("backend.proxy.httpx.AsyncClient", FakeClient):
            text = asyncio.run(collect())

        self.assertIn("event: error", text)
        self.assertIn("invalid_upstream_response", text)
        self.assertIn("text/html", text)
        self.assertNotIn("sk-stream-secret123456", text)
        self.assertNotIn("stream-secret-token", text)

    def test_openai_streaming_text_plain_success_response_returns_sse_error_event(self):
        class FakeStreamResponse:
            status_code = 200
            is_success = True
            headers = {"content-type": "text/plain; charset=utf-8"}

            async def aread(self):
                return b"login required access_token=stream-access-secret client_secret=stream-client-secret"

            async def aiter_lines(self):
                yield "login required access_token=stream-access-secret"

        class FakeStreamContext:
            async def __aenter__(self):
                return FakeStreamResponse()

            async def __aexit__(self, exc_type, exc, tb):
                return False

        class FakeClient:
            def __init__(self, *args, **kwargs):
                pass

            async def __aenter__(self):
                return self

            async def __aexit__(self, exc_type, exc, tb):
                return False

            def stream(self, *args, **kwargs):
                return FakeStreamContext()

        provider = {
            "name": "Private Relay",
            "baseUrl": "https://new.aibado.cn/v1/chat/completions",
            "apiKey": "sk-provider-secret123456",
            "authScheme": "bearer",
            "apiFormat": "openai_chat",
        }
        body = {
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 8,
            "stream": True,
        }

        async def collect():
            chunks = []
            async for chunk in forward_request_stream(body, provider, "req-text"):
                chunks.append(chunk)
            return "".join(chunks)

        with patch("backend.proxy.httpx.AsyncClient", FakeClient):
            text = asyncio.run(collect())

        self.assertIn("event: error", text)
        self.assertIn("invalid_upstream_response", text)
        self.assertIn("text/plain; charset=utf-8", text)
        self.assertNotIn("stream-access-secret", text)
        self.assertNotIn("stream-client-secret", text)

    def test_openai_streaming_ndjson_without_valid_chunks_returns_error_event(self):
        class FakeStreamResponse:
            status_code = 200
            is_success = True
            headers = {"content-type": "application/x-ndjson"}

            async def aread(self):
                return b""

            async def aiter_lines(self):
                yield "plain relay failure access_token=ndjson-secret"

        class FakeStreamContext:
            async def __aenter__(self):
                return FakeStreamResponse()

            async def __aexit__(self, exc_type, exc, tb):
                return False

        class FakeClient:
            def __init__(self, *args, **kwargs):
                pass

            async def __aenter__(self):
                return self

            async def __aexit__(self, exc_type, exc, tb):
                return False

            def stream(self, *args, **kwargs):
                return FakeStreamContext()

        provider = {
            "name": "Private Relay",
            "baseUrl": "https://new.aibado.cn/v1/chat/completions",
            "apiKey": "sk-provider-secret123456",
            "authScheme": "bearer",
            "apiFormat": "openai_chat",
        }
        body = {
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 8,
            "stream": True,
        }

        async def collect():
            chunks = []
            async for chunk in forward_request_stream(body, provider, "req-ndjson"):
                chunks.append(chunk)
            return "".join(chunks)

        with patch("backend.proxy.httpx.AsyncClient", FakeClient):
            text = asyncio.run(collect())

        self.assertIn("event: error", text)
        self.assertIn("invalid_upstream_response", text)
        self.assertNotIn("ndjson-secret", text)


class ProxyAppTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.old_config_dir = cfg.CONFIG_DIR
        self.old_config_file = cfg.CONFIG_FILE
        self.old_backup_dir = cfg.BACKUP_DIR
        cfg.CONFIG_DIR = self.temp_dir.name
        cfg.CONFIG_FILE = os.path.join(self.temp_dir.name, "config.json")
        cfg.BACKUP_DIR = os.path.join(self.temp_dir.name, "backups")
        cfg.save_config(copy.deepcopy(cfg.DEFAULT_CONFIG))
        self.client = TestClient(create_proxy_app())

    def tearDown(self):
        cfg.CONFIG_DIR = self.old_config_dir
        cfg.CONFIG_FILE = self.old_config_file
        cfg.BACKUP_DIR = self.old_backup_dir
        self.temp_dir.cleanup()

    def test_models_endpoint_requires_gateway_key_and_returns_active_models(self):
        cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "apiKey": "secret-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {
                "sonnet": "deepseek-v4-pro[1m]",
                "haiku": "deepseek-v4-flash",
                "opus": "deepseek-v4-pro[1m]",
                "default": "deepseek-v4-pro[1m]",
            },
        })
        cfg.save_config({**cfg.load_config(), "gatewayApiKey": "local-gateway-key"})

        blocked = self.client.get("/v1/models")
        allowed = self.client.get("/v1/models", headers={"authorization": "Bearer local-gateway-key"})

        self.assertEqual(blocked.status_code, 401)
        self.assertEqual(allowed.status_code, 200)
        ids = [item["id"] for item in allowed.json()["data"]]
        self.assertIn("claude-sonnet-4-6", ids)
        self.assertNotIn("deepseek-v4-pro[1m]", ids)

    def test_models_endpoint_keeps_active_models_when_all_models_setting_is_hidden(self):
        cfg.add_provider({
            "id": "deepseek",
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "apiKey": "deepseek-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {
                "sonnet": "deepseek-v4-pro[1m]",
                "haiku": "deepseek-v4-flash",
                "default": "deepseek-v4-pro[1m]",
            },
            "modelCapabilities": {"deepseek-v4-pro[1m]": {"supports1m": True}},
        })
        cfg.add_provider({
            "id": "kimi",
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.cn/anthropic",
            "apiKey": "kimi-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "kimi-k2.6", "default": "kimi-k2.6"},
        })
        config = cfg.load_config()
        config["gatewayApiKey"] = "local-gateway-key"
        cfg.save_config(config)
        cfg.update_settings({"exposeAllProviderModels": True})

        response = self.client.get("/v1/models", headers={"authorization": "Bearer local-gateway-key"})

        self.assertEqual(response.status_code, 200)
        by_id = {item["id"]: item for item in response.json()["data"]}
        self.assertIn("claude-sonnet-4-6", by_id)
        self.assertNotIn("deepseek-v4-pro[1m]", by_id)
        self.assertNotIn("kimi/kimi-k2.6", by_id)

    def test_messages_endpoint_ignores_alias_routing_when_all_models_setting_is_hidden(self):
        cfg.add_provider({
            "id": "deepseek",
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "apiKey": "deepseek-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "deepseek-v4-pro", "default": "deepseek-v4-pro"},
        })
        cfg.add_provider({
            "id": "kimi",
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.cn/anthropic",
            "apiKey": "kimi-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "kimi-k2.6", "default": "kimi-k2.6"},
        })
        config = cfg.load_config()
        config["gatewayApiKey"] = "local-gateway-key"
        cfg.save_config(config)
        cfg.update_settings({"exposeAllProviderModels": True})
        observed = {}

        async def fake_forward_request(body, provider, _request_id):
            observed["model"] = body["model"]
            observed["provider"] = provider["id"]
            return {
                "id": "msg_test",
                "type": "message",
                "role": "assistant",
                "model": body["model"],
                "content": [{"type": "text", "text": "ok"}],
                "usage": {"input_tokens": 1, "output_tokens": 1},
            }

        with patch("backend.proxy.forward_request", fake_forward_request):
            response = self.client.post(
                "/v1/messages",
                headers={"authorization": "Bearer local-gateway-key"},
                json={
                    "model": "kimi/kimi-k2.6",
                    "messages": [{"role": "user", "content": "hello"}],
                    "max_tokens": 8,
                },
            )

        self.assertEqual(response.status_code, 200)
        self.assertEqual(observed, {"model": "deepseek-v4-pro", "provider": "deepseek"})

    def test_messages_endpoint_rejects_when_gateway_key_has_not_been_created(self):
        cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "apiKey": "secret-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "deepseek-v4-pro", "default": "deepseek-v4-pro"},
        })

        response = self.client.post("/v1/messages", json={
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 8,
        })

        self.assertEqual(response.status_code, 401)

    def test_messages_endpoint_rejects_unmapped_claude_route(self):
        cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "apiKey": "secret-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "deepseek-v4-pro", "default": "deepseek-v4-pro"},
        })
        cfg.save_config({**cfg.load_config(), "gatewayApiKey": "local-gateway-key"})

        async def fake_forward_request(_body, _provider, _request_id):
            return {"unexpected": True}

        with patch("backend.proxy.forward_request", fake_forward_request):
            response = self.client.post(
                "/v1/messages",
                headers={"authorization": "Bearer local-gateway-key"},
                json={
                    "model": "claude-sonnet-4-5",
                    "messages": [{"role": "user", "content": "hello"}],
                    "max_tokens": 8,
                },
            )

        self.assertEqual(response.status_code, 400)
        self.assertEqual(response.json()["error"]["type"], "invalid_request_error")
        self.assertIn("未在 CC Desktop Switch 中映射", response.json()["error"]["message"])

    def test_messages_endpoint_returns_upstream_error_status(self):
        cfg.add_provider({
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.ai/anthropic",
            "apiKey": "bad-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "kimi-k2.6", "default": "kimi-k2.6"},
        })
        cfg.save_config({**cfg.load_config(), "gatewayApiKey": "local-gateway-key"})

        async def fake_forward_request(_body, _provider, _request_id):
            return {
                "error": {
                    "type": "upstream_error",
                    "status": 401,
                    "message": "Invalid Authentication",
                }
            }

        with patch("backend.proxy.forward_request", fake_forward_request):
            response = self.client.post(
                "/v1/messages",
                headers={"authorization": "Bearer local-gateway-key"},
                json={
                    "model": "claude-sonnet-4-6",
                    "messages": [{"role": "user", "content": "hello"}],
                    "max_tokens": 8,
                },
            )

        self.assertEqual(response.status_code, 401)
        self.assertEqual(response.json()["error"]["status"], 401)

    def test_streaming_upstream_error_uses_sse_error_event(self):
        cfg.add_provider({
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.ai/anthropic",
            "apiKey": "bad-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "kimi-k2.6", "default": "kimi-k2.6"},
        })
        cfg.save_config({**cfg.load_config(), "gatewayApiKey": "local-gateway-key"})

        async def fake_forward_request_stream(_body, _provider, _request_id):
            yield (
                'event: error\n'
                'data: {"type":"error","error":{"type":"upstream_error","status":401}}\n\n'
            )

        with patch("backend.proxy.forward_request_stream", fake_forward_request_stream):
            response = self.client.post(
                "/v1/messages",
                headers={"authorization": "Bearer local-gateway-key"},
                json={
                    "model": "claude-sonnet-4-6",
                    "messages": [{"role": "user", "content": "hello"}],
                    "max_tokens": 8,
                    "stream": True,
                },
            )

        self.assertEqual(response.status_code, 200)
        self.assertIn("event: error", response.text)
        self.assertIn('"status":401', response.text)


class FakeTrayWindow:
    def __init__(self):
        self.hidden = 0
        self.shown = 0
        self.restored = 0
        self.destroyed = 0

    def hide(self):
        self.hidden += 1

    def show(self):
        self.shown += 1

    def restore(self):
        self.restored += 1

    def destroy(self):
        self.destroyed += 1


class FakeTrayIcon:
    def __init__(self):
        self.notifications = []
        self.stopped = 0
        self.updated = 0
        self.menu = None

    def notify(self, message, title):
        self.notifications.append((title, message))

    def stop(self):
        self.stopped += 1

    def update_menu(self):
        self.updated += 1


class FakePystray:
    class Menu:
        SEPARATOR = object()

        def __init__(self, *items):
            self.items = items

    class MenuItem:
        def __init__(self, text, action, **kwargs):
            self.text = text
            self.action = action
            self.kwargs = kwargs


class DesktopTrayControllerTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.old_config_dir = cfg.CONFIG_DIR
        self.old_config_file = cfg.CONFIG_FILE
        self.old_backup_dir = cfg.BACKUP_DIR
        cfg.CONFIG_DIR = self.temp_dir.name
        cfg.CONFIG_FILE = os.path.join(self.temp_dir.name, "config.json")
        cfg.BACKUP_DIR = os.path.join(self.temp_dir.name, "backups")
        cfg.save_config(copy.deepcopy(cfg.DEFAULT_CONFIG))

    def tearDown(self):
        cfg.CONFIG_DIR = self.old_config_dir
        cfg.CONFIG_FILE = self.old_config_file
        cfg.BACKUP_DIR = self.old_backup_dir
        self.temp_dir.cleanup()

    def test_tray_is_disabled_on_macos_to_keep_appkit_on_main_thread(self):
        window = FakeTrayWindow()
        tray = DesktopTrayController(window, "missing-icon.png")

        with patch("main.sys.platform", "darwin"):
            self.assertFalse(tray.start())

        self.assertIsNone(tray.icon)
        self.assertIsNone(tray.thread)

    def test_close_hides_window_and_cancels_close(self):
        window = FakeTrayWindow()
        tray = DesktopTrayController(window, "missing-icon.png")
        tray.icon = FakeTrayIcon()

        result = tray.handle_window_closing()

        self.assertIs(result, False)
        self.assertEqual(window.hidden, 1)
        self.assertTrue(tray.window_hidden)
        self.assertEqual(len(tray.icon.notifications), 1)

    def test_macos_close_hides_window_without_tray_icon(self):
        window = FakeTrayWindow()
        tray = DesktopTrayController(window, "missing-icon.png")

        with patch("main._macos_should_quit_from_close_event", return_value=False):
            result = tray.handle_window_closing()

        self.assertIs(result, False)
        self.assertEqual(window.hidden, 1)
        self.assertTrue(tray.window_hidden)

    def test_macos_quit_event_allows_window_close(self):
        window = FakeTrayWindow()
        tray = DesktopTrayController(window, "missing-icon.png")

        with patch("main._macos_should_quit_from_close_event", return_value=True):
            result = tray.handle_window_closing()

        self.assertIsNone(result)
        self.assertEqual(window.hidden, 0)

    def test_quit_allows_window_close(self):
        window = FakeTrayWindow()
        tray = DesktopTrayController(window, "missing-icon.png")

        tray.quit_app()
        result = tray.handle_window_closing()

        self.assertIsNone(result)
        self.assertTrue(tray.exit_requested)
        self.assertEqual(window.destroyed, 1)

    def test_show_window_restores_hidden_window(self):
        window = FakeTrayWindow()
        tray = DesktopTrayController(window, "missing-icon.png")

        tray.show_window()

        self.assertEqual(window.shown, 1)
        self.assertEqual(window.restored, 1)
        self.assertFalse(tray.window_hidden)

    def test_switch_provider_updates_active_provider_and_refreshes_menu(self):
        first = cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
        })
        second = cfg.add_provider({
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.cn/v1",
            "authScheme": "bearer",
            "apiFormat": "openai",
        })
        window = FakeTrayWindow()
        tray = DesktopTrayController(window, "missing-icon.png")
        tray.pystray = FakePystray
        tray.icon = FakeTrayIcon()

        self.assertEqual(cfg.load_config()["activeProvider"], first["id"])
        with patch("main.registry.apply_config", return_value={"success": True}):
            with patch("main._start_proxy_server", return_value=True):
                with patch.object(tray, "show_desktop_restart_dialog") as restart_dialog:
                    self.assertTrue(tray.switch_provider(second["id"]))

        self.assertEqual(cfg.load_config()["activeProvider"], second["id"])
        self.assertEqual(tray.icon.updated, 1)
        self.assertIn("Kimi", tray.icon.notifications[0][1])
        restart_dialog.assert_not_called()

    def test_switch_provider_syncs_local_proxy_desktop_policy(self):
        first = cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
        })
        second = cfg.add_provider({
            "name": "Kimi",
            "baseUrl": "https://api.moonshot.cn/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {"sonnet": "kimi-k2.6", "default": "kimi-k2.6"},
        })
        window = FakeTrayWindow()
        tray = DesktopTrayController(window, "missing-icon.png")
        tray.pystray = FakePystray
        tray.icon = FakeTrayIcon()
        observed = {}

        with patch("main.registry.apply_config", return_value={"success": True}) as apply_config:
            with patch("main._start_proxy_server") as start_proxy:
                with patch.object(tray, "show_desktop_restart_dialog") as restart_dialog:
                    self.assertTrue(tray.switch_provider(second["id"]))
            observed["provider"] = apply_config.call_args.kwargs["provider"]
            observed["base_url"] = apply_config.call_args.args[0]
            observed["providers"] = apply_config.call_args.kwargs["providers"]
            observed["expose_all"] = apply_config.call_args.kwargs["expose_all"]

        self.assertEqual(cfg.load_config()["activeProvider"], second["id"])
        self.assertEqual(observed["provider"]["models"]["sonnet"], "kimi-k2.6")
        self.assertEqual(observed["base_url"], "http://127.0.0.1:18080")
        self.assertIsNone(observed["providers"])
        self.assertFalse(observed["expose_all"])
        self.assertIn("桌面版配置已同步", tray.icon.notifications[0][1])
        start_proxy.assert_called_once_with(18080)
        restart_dialog.assert_not_called()

    def test_switch_provider_does_not_show_restart_dialog_when_provider_is_unchanged(self):
        provider = cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
        })
        window = FakeTrayWindow()
        tray = DesktopTrayController(window, "missing-icon.png")
        tray.pystray = FakePystray
        tray.icon = FakeTrayIcon()

        with patch.object(tray, "show_desktop_restart_dialog") as restart_dialog:
            self.assertTrue(tray.switch_provider(provider["id"]))

        restart_dialog.assert_not_called()

    def test_tray_restart_dialog_uses_windows_message_box(self):
        window = FakeTrayWindow()
        tray = DesktopTrayController(window, "missing-icon.png")

        with patch("main.show_message_box_async") as message_box:
            tray.show_desktop_restart_dialog({"name": "Kimi"}, desktop_synced=True)

        message_box.assert_called_once()
        self.assertEqual(message_box.call_args.args[0], "需要重启 Claude 桌面版")
        self.assertIn("Kimi", message_box.call_args.args[1])
        self.assertIn("重新打开 Claude 桌面版", message_box.call_args.args[1])

    def test_installer_reuses_previous_install_dir_and_closes_running_app(self):
        installer = Path(__file__).resolve().parents[1] / "installer.nsi"
        text = installer.read_text(encoding="utf-8")

        self.assertIn('InstallDirRegKey HKLM "${PRODUCT_UNINST_KEY}" "InstallLocation"', text)
        self.assertIn('ReadRegStr $R1 HKLM "${PRODUCT_UNINST_KEY}" "InstallLocation"', text)
        self.assertIn('taskkill /IM "CC-Desktop-Switch.exe" /T /F', text)
        self.assertIn('WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "InstallLocation" "$INSTDIR"', text)


class StaticFrontendTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(__file__).resolve().parents[1]

    def test_model_mapping_is_integrated_into_provider_form(self):
        html = (self.root / "frontend" / "index.html").read_text(encoding="utf-8")
        app_js = (self.root / "frontend" / "js" / "app.js").read_text(encoding="utf-8")

        self.assertNotIn('href="#models"', html)
        self.assertNotIn('data-page="models"', html)
        self.assertNotIn('data-nav="desktop"', html)
        self.assertNotIn('href="#desktop" class="btn btn-primary action-button"', html)
        self.assertNotIn('id="formatOpenai"', html)
        self.assertNotIn('name="apiFormat"', html)
        self.assertIn('id="providerMappingStack"', html)
        self.assertIn('id="providerPresetOptions"', html)
        self.assertIn('data-action="apply-provider-desktop"', html)
        self.assertIn('fetchProviderModelsPayload', app_js)
        self.assertIn('presetCache', app_js)
        self.assertIn('data-preset-model-option', app_js)

    def test_desktop_copy_uses_plain_desktop_language(self):
        html = (self.root / "frontend" / "index.html").read_text(encoding="utf-8")
        i18n = (self.root / "frontend" / "js" / "i18n.js").read_text(encoding="utf-8")

        self.assertIn("Claude 桌面版", html)
        self.assertIn("原理很简单", i18n)
        self.assertNotIn("Claude Desktop 3P 模式", html)

    def test_provider_add_presets_and_guide_copy_are_user_facing(self):
        html = (self.root / "frontend" / "index.html").read_text(encoding="utf-8")
        css = (self.root / "frontend" / "css" / "style.css").read_text(encoding="utf-8")
        i18n = (self.root / "frontend" / "js" / "i18n.js").read_text(encoding="utf-8")

        self.assertIn("provider-add-layout", html)
        self.assertIn("一键应用到 Claude 桌面版", html)
        self.assertIn("providersAdd.presetsHint", html)
        self.assertNotIn(".preset-panel {\n  display: none;", css)
        self.assertEqual(html.count('class="timeline-card"'), 3)
        self.assertNotIn("本地代理", html + i18n)
        self.assertNotIn("本机代理", (html + i18n).replace("CC-Switch 本机代理地址", ""))
        self.assertNotIn("确认本地端口可用", html + i18n)

    def test_dashboard_presets_selection_update_and_desktop_health_ui_exist(self):
        html = (self.root / "frontend" / "index.html").read_text(encoding="utf-8")
        css = (self.root / "frontend" / "css" / "style.css").read_text(encoding="utf-8")
        app_js = (self.root / "frontend" / "js" / "app.js").read_text(encoding="utf-8")
        api_js = (self.root / "frontend" / "js" / "api.js").read_text(encoding="utf-8")
        i18n = (self.root / "frontend" / "js" / "i18n.js").read_text(encoding="utf-8")

        self.assertIn('id="dashboardUpdateBadge"', html)
        self.assertIn('id="dashboardDesktopWarning"', html)
        self.assertIn('id="desktopPageWarning"', html)
        self.assertIn("provider-preset-grid", app_js + css)
        self.assertIn("includePresets: true", app_js)
        self.assertIn("updatePresetSelection", app_js)
        self.assertIn("aria-pressed", app_js)
        self.assertIn("formModelCapabilities", app_js)
        self.assertIn("formRequestOptions", app_js)
        self.assertIn("requestOptionPresets", app_js + api_js)
        self.assertIn("modelsMatch(option.models", app_js)
        self.assertIn("capabilitiesMatch", app_js)
        self.assertIn("reorderProviders", api_js)
        self.assertIn("desktopHealth", app_js + api_js)
        self.assertIn("modelCapabilities", app_js + api_js)
        self.assertIn("requestOptions", app_js + api_js)
        self.assertIn("white-space: pre-line", css)
        self.assertIn('data-action="install-update"', html)
        self.assertIn('id="settingsInstallUpdate"', html)
        self.assertIn('id="restartReminderModal"', html)
        self.assertIn('id="restartReminderAck"', html)
        self.assertIn("header-actions-left", html)
        self.assertIn("dashboard-clear-button", html)
        self.assertIn('data-i18n="dashboard.clearDesktopConfig"', html)
        self.assertIn('data-action="clear-desktop"', html)
        self.assertIn("installUpdate(updateUrl)", api_js)
        self.assertIn("assets/providers/aliyun.ico", api_js)
        self.assertTrue((self.root / "frontend" / "assets" / "providers" / "aliyun.ico").exists())
        self.assertIn("restartReminderStorageKey", app_js)
        self.assertIn("showRestartReminder", app_js)
        self.assertIn("toast.defaultUpdatedDesktop", app_js + i18n)
        self.assertIn("restartReminder.dontShow", i18n)
        self.assertIn("confirm.installUpdate", app_js + i18n)
        self.assertIn("setUpdateInstallPhase", app_js)
        self.assertIn("settings.downloadingUpdate", app_js + i18n)
        self.assertIn("settings.installingUpdate", app_js + i18n)
        self.assertIn("toast.updateDownloading", app_js + i18n)
        self.assertIn(".update-badge:disabled", css)
        self.assertIn("不会删除本工具里保存的提供商和 API Key", i18n)
        self.assertIn('class="panel model-menu-mode-panel" hidden aria-hidden="true"', html)
        self.assertIn('class="settings-row" hidden aria-hidden="true"', html)
        self.assertIn('data-action="toggle-model-menu-mode"', html)
        self.assertIn("renderModelMenuModeState", app_js)
        self.assertIn("providers.showAllModels", i18n)

    def test_diagnostics_ui_and_api_hooks_exist(self):
        html = (self.root / "frontend" / "index.html").read_text(encoding="utf-8")
        app_js = (self.root / "frontend" / "js" / "app.js").read_text(encoding="utf-8")
        api_js = (self.root / "frontend" / "js" / "api.js").read_text(encoding="utf-8")

        self.assertIn('data-action="run-diagnostics"', html)
        self.assertIn('data-action="export-diagnostics"', html)
        self.assertIn('data-action="copy-diagnostics"', html)
        self.assertIn("exportDiagnostics", api_js)
        self.assertIn("checkDiagnostics", api_js)
        self.assertIn("renderDiagnosticsResult", app_js)

    def test_current_guides_do_not_use_old_desktop_gateway_wording(self):
        forbidden = [
            "默认直连",
            "关闭本工具也能继续使用",
            "实验转发模式",
            "Claude 桌面版 -> 你的 API 提供商",
        ]
        guide_paths = [
            self.root / "README.md",
            self.root / "README.zh-CN.md",
            self.root / "README.ja.md",
            self.root / "docs" / "QUICK_START.md",
            self.root / "docs" / "USAGE.md",
        ]

        for path in guide_paths:
            text = path.read_text(encoding="utf-8")
            for phrase in forbidden:
                self.assertNotIn(phrase, text, str(path))

    def test_third_party_compatibility_ui_is_folded_and_marked_experimental(self):
        html = (self.root / "frontend" / "index.html").read_text(encoding="utf-8")
        css = (self.root / "frontend" / "css" / "style.css").read_text(encoding="utf-8")
        app_js = (self.root / "frontend" / "js" / "app.js").read_text(encoding="utf-8")
        api_js = (self.root / "frontend" / "js" / "api.js").read_text(encoding="utf-8")
        i18n = (self.root / "frontend" / "js" / "i18n.js").read_text(encoding="utf-8")

        self.assertIn('class="advanced-provider-options" open', html)
        self.assertIn('compat-chevron', html + css)
        self.assertIn("border: 2px solid #ef4444", css)
        self.assertIn('data-api-format="anthropic"', html)
        self.assertIn('data-api-format="openai_chat"', html)
        self.assertIn('data-action="check-provider-compatibility"', html)
        self.assertIn('id="providerCompatibilityList"', html)
        self.assertIn("format-choice-button.active", css)
        self.assertIn("compatibility-item.experimental", css)
        self.assertIn("getProviderCompatibility", api_js)
        self.assertIn("renderProviderCompatibilityList", app_js)
        self.assertIn("OpenAI Chat 属于实验适配", html + i18n)
        self.assertIn("toast.openaiFormatExperimental", app_js + i18n)

    def test_ccswitch_import_ui_is_isolated_and_marks_openai_as_skipped(self):
        html = (self.root / "frontend" / "index.html").read_text(encoding="utf-8")
        css = (self.root / "frontend" / "css" / "style.css").read_text(encoding="utf-8")
        app_js = (self.root / "frontend" / "js" / "app.js").read_text(encoding="utf-8")
        api_js = (self.root / "frontend" / "js" / "api.js").read_text(encoding="utf-8")
        i18n = (self.root / "frontend" / "js" / "i18n.js").read_text(encoding="utf-8")

        self.assertIn('data-action="detect-ccswitch"', html)
        self.assertIn('data-action="import-ccswitch"', html)
        self.assertIn('data-action="open-ccswitch-import"', html)
        self.assertIn('id="ccSwitchImportSection"', html)
        self.assertIn('id="ccSwitchImportList"', html)
        self.assertIn("ccswitch-import-item", css)
        self.assertIn("focusCcSwitchImportSection", app_js)
        self.assertIn("getCcSwitchProviders", api_js)
        self.assertIn("importCcSwitchProviders", app_js + api_js)
        self.assertIn("refreshCcSwitchImportStatus", app_js)
        self.assertIn("只导入 Anthropic 兼容配置", html + i18n)
        self.assertIn("OpenAI 格式会显示为暂不导入", html + i18n)
        self.assertIn("confirm.ccswitchImport", app_js + i18n)
        self.assertIn("不会覆盖现有提供商", i18n)
        self.assertIn("不会修改 CC-Switch", i18n)
        self.assertIn("转发状态", html + i18n)
        self.assertNotIn("代理控制台", html + i18n)


if __name__ == "__main__":
    unittest.main()
