import copy
import asyncio
import os
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

from fastapi.testclient import TestClient

from backend.main import _desktop_health, _test_provider_connection, create_admin_app
from main import DesktopTrayController
from backend import config as cfg
from backend import provider_tools
from backend import registry
from backend import update as updater
from backend.proxy import (
    _anthropic_to_openai_body,
    _normalize_anthropic_response,
    _normalize_anthropic_sse_event,
    _openai_chunk_to_anthropic,
    apply_anthropic_request_options,
    build_upstream_url,
    create_proxy_app,
    gateway_models_response,
    map_model,
)


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
        deepseek_max = presets["deepseek"]["requestOptionPresets"]["deepseek_max_effort"]
        self.assertEqual(deepseek_max["requestOptions"]["anthropic"]["output_config"]["effort"], "max")
        self.assertEqual(deepseek_max["requestOptions"]["anthropic"]["thinking"]["type"], "enabled")
        self.assertIn("Low：更快更省", deepseek_max["description"])
        self.assertIn("未勾选则使用 Claude 当前默认配置", deepseek_max["description"])

    def test_registry_inference_models_mark_deepseek_1m(self):
        provider = {
            "models": {
                "sonnet": "deepseek-v4-pro[1m]",
                "haiku": "deepseek-v4-flash",
                "opus": "deepseek-v4-pro[1m]",
                "default": "deepseek-v4-pro[1m]",
            }
        }

        models = registry.provider_inference_models(provider)
        serialized = registry.serialize_inference_models(provider)

        self.assertEqual(models[0]["name"], "deepseek-v4-pro[1m]")
        self.assertTrue(models[0]["supports1m"])
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

        self.assertTrue(by_name["qwen3.6-plus"]["supports1m"])
        self.assertTrue(by_name["qwen3.6-flash"]["supports1m"])
        self.assertNotIn("supports1m", by_name["qwen3.6-max-preview"])

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

    def test_all_builtin_presets_expose_and_map_provider_models(self):
        """所有内置预设都应能被 Claude Desktop 读取，并被代理实际使用。"""
        for preset in cfg.get_presets():
            with self.subTest(provider=preset["id"]):
                models = preset["models"]
                expected_ids = []
                for key in ("default", "sonnet", "opus", "haiku"):
                    model_id = models.get(key)
                    if model_id and model_id not in expected_ids:
                        expected_ids.append(model_id)

                desktop_models = registry.provider_inference_models(preset)
                desktop_ids = [
                    item["name"] if isinstance(item, dict) else item
                    for item in desktop_models
                ]
                gateway_ids = [item["id"] for item in gateway_models_response(preset)["data"]]

                self.assertEqual(desktop_ids, expected_ids)
                self.assertEqual(gateway_ids, expected_ids)
                for model_id in expected_ids:
                    self.assertEqual(map_model(model_id, preset), model_id)
                self.assertEqual(map_model("claude-sonnet-4-6", preset), models["sonnet"])
                self.assertEqual(map_model("claude-haiku-3-5", preset), models["haiku"])
                self.assertEqual(map_model("claude-opus-4-7", preset), models["opus"])

        deepseek_1m = cfg.get_presets()[0]["modelOptions"]["deepseek_1m"]
        deepseek_1m_provider = {
            **cfg.get_presets()[0],
            "models": deepseek_1m["models"],
        }
        desktop_models = registry.provider_inference_models(deepseek_1m_provider)
        self.assertTrue(any(
            item["name"] == "deepseek-v4-pro[1m]" and item.get("supports1m") is True
            for item in desktop_models
            if isinstance(item, dict)
        ))
        self.assertEqual(
            map_model("claude-sonnet-4-6", deepseek_1m_provider),
            "deepseek-v4-pro[1m]",
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
            content = b'\xef\xbb\xbf{"version":"1.0.8","platforms":{"windows-x64":{"assets":[]}}}'

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

        self.assertEqual(data["version"], "1.0.8")

    def test_update_installer_asset_prefers_setup_exe(self):
        asset = updater.pick_windows_installer([
            {"name": "CC-Desktop-Switch-v1.0.5-Windows-Portable.zip"},
            {"name": "CC-Desktop-Switch-v1.0.5-Windows-x64.exe"},
            {"name": "CC-Desktop-Switch-v1.0.5-Windows-Setup.exe"},
        ])

        self.assertEqual(asset["name"], "CC-Desktop-Switch-v1.0.5-Windows-Setup.exe")

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
            "models": {
                "sonnet": "deepseek-v4-pro[1m]",
                "haiku": "deepseek-v4-flash",
                "opus": "deepseek-v4-pro[1m]",
                "default": "deepseek-v4-pro[1m]",
            }
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

        current_status = {
            "configured": True,
            "keys": {
                "inferenceGatewayBaseUrl": "http://127.0.0.1:18080",
                "inferenceModels": '[{"name":"deepseek-v4-pro[1m]","supports1m":true},{"name":"deepseek-v4-flash"}]',
            },
        }

        current_health = _desktop_health(current_status, 18080, provider)

        self.assertFalse(current_health["needsApply"])
        self.assertTrue(current_health["oneMillionReady"])

    def test_desktop_health_detects_capability_based_1m_models(self):
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

        missing = _desktop_health({
            "configured": True,
            "keys": {
                "inferenceGatewayBaseUrl": "http://127.0.0.1:18080",
                "inferenceModels": '[{"name":"qwen3.6-plus"},{"name":"qwen3.6-flash"}]',
            },
        }, 18080, provider)

        ready = _desktop_health({
            "configured": True,
            "keys": {
                "inferenceGatewayBaseUrl": "http://127.0.0.1:18080",
                "inferenceModels": '[{"name":"qwen3.6-plus","supports1m":true},{"name":"qwen3.6-flash","supports1m":true}]',
            },
        }, 18080, provider)

        self.assertTrue(missing["needsApply"])
        self.assertFalse(missing["oneMillionReady"])
        self.assertFalse(ready["needsApply"])
        self.assertTrue(ready["oneMillionReady"])


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
        cfg.CONFIG_DIR = self.old_config_dir
        cfg.CONFIG_FILE = self.old_config_file
        cfg.BACKUP_DIR = self.old_backup_dir
        self.temp_dir.cleanup()

    def test_config_export_requires_local_header_and_keeps_provider_list_public(self):
        cfg.add_provider({
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "apiKey": "secret-key",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "extraHeaders": {"x-api-key": "{apiKey}"},
        })

        blocked = self.client.get("/api/config/export")
        allowed = self.client.get("/api/config/export", headers={"x-ccds-request": "1"})
        providers = self.client.get("/api/providers")

        self.assertEqual(blocked.status_code, 403)
        self.assertEqual(allowed.status_code, 200)
        self.assertEqual(allowed.json()["config"]["providers"][0]["apiKey"], "secret-key")
        public_provider = providers.json()["providers"][0]
        self.assertNotIn("apiKey", public_provider)
        self.assertNotIn("extraHeaders", public_provider)
        self.assertTrue(public_provider["hasApiKey"])

    def test_provider_secret_requires_local_header(self):
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
            headers={"x-ccds-request": "1"},
        )

        self.assertEqual(blocked.status_code, 403)
        self.assertEqual(allowed.status_code, 200)
        self.assertEqual(allowed.json()["apiKey"], "secret-key")

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
                headers={"x-ccds-request": "1"},
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
                headers={"x-ccds-request": "1"},
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
                headers={"x-ccds-request": "1"},
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
            headers={"x-ccds-request": "1"},
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

        with patch("backend.main.updater.check_update", fake_check_update):
            response = self.client.get("/api/update/check")

        self.assertEqual(response.status_code, 200)
        self.assertEqual(observed["url"], cfg.DEFAULT_UPDATE_URL)

    def test_set_default_provider_syncs_desktop_models_when_managed(self):
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

        with patch("backend.main.registry.get_config_status", return_value={"configured": True}):
            with patch("backend.main.registry.apply_config", return_value={"success": True}) as apply_config:
                response = self.client.put(
                    f"/api/providers/{second['id']}/default",
                    headers={"x-ccds-request": "1"},
                )

        self.assertEqual(response.status_code, 200)
        data = response.json()
        self.assertTrue(data["desktopSync"]["attempted"])
        self.assertTrue(data["desktopSync"]["success"])
        self.assertEqual(apply_config.call_args.args[0], "http://127.0.0.1:18080")
        self.assertEqual(apply_config.call_args.kwargs["provider"]["models"]["sonnet"], "kimi-k2.6")
        self.assertEqual(cfg.load_config()["activeProvider"], second["id"])

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

        with patch("backend.main.updater.download_update", fake_download_update):
            with patch("backend.main.subprocess.Popen") as popen:
                response = self.client.post(
                    "/api/update/install",
                    headers={"x-ccds-request": "1"},
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
                "latestVersion": "1.0.8",
                "platform": platform,
                "assets": [],
                "downloaded": True,
                "installerPath": r"C:\Temp\CC-Desktop-Switch-v1.0.8-Windows-Setup.exe",
            }

        with patch("backend.main.updater.download_update", fake_download_update):
            with patch("backend.main.subprocess.Popen") as popen:
                response = self.client.post(
                    "/api/update/install",
                    headers={"x-ccds-request": "1"},
                    json={},
                )

        self.assertEqual(response.status_code, 200)
        self.assertTrue(response.json()["installerStarted"])
        popen.assert_called_once_with(
            [r"C:\Temp\CC-Desktop-Switch-v1.0.8-Windows-Setup.exe"],
            close_fds=True,
        )


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

    def test_gateway_models_response_exposes_exact_provider_model_ids(self):
        provider = {
            "models": {
                "sonnet": "deepseek-v4-pro[1m]",
                "haiku": "deepseek-v4-flash",
                "opus": "deepseek-v4-pro[1m]",
                "default": "deepseek-v4-pro[1m]",
            }
        }

        response = gateway_models_response(provider)

        self.assertEqual(response["data"][0]["id"], "deepseek-v4-pro[1m]")
        self.assertEqual(response["data"][1]["id"], "deepseek-v4-flash")

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
        self.assertEqual(allowed.json()["data"][0]["id"], "deepseek-v4-pro[1m]")

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

    def test_close_hides_window_and_cancels_close(self):
        window = FakeTrayWindow()
        tray = DesktopTrayController(window, "missing-icon.png")
        tray.icon = FakeTrayIcon()

        result = tray.handle_window_closing()

        self.assertIs(result, False)
        self.assertEqual(window.hidden, 1)
        self.assertEqual(len(tray.icon.notifications), 1)

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
        with patch.object(tray, "show_desktop_restart_dialog") as restart_dialog:
            self.assertTrue(tray.switch_provider(second["id"]))

        self.assertEqual(cfg.load_config()["activeProvider"], second["id"])
        self.assertEqual(tray.icon.updated, 1)
        self.assertIn("Kimi", tray.icon.notifications[0][1])
        restart_dialog.assert_called_once()
        self.assertEqual(restart_dialog.call_args.args[0]["id"], second["id"])

    def test_switch_provider_syncs_desktop_models_when_managed(self):
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

        with patch("main.registry.get_config_status", return_value={"configured": True}):
            with patch("main.registry.apply_config", return_value={"success": True}) as apply_config:
                with patch.object(tray, "show_desktop_restart_dialog") as restart_dialog:
                    self.assertTrue(tray.switch_provider(second["id"]))
                observed["provider"] = apply_config.call_args.kwargs["provider"]
                observed["base_url"] = apply_config.call_args.args[0]

        self.assertEqual(cfg.load_config()["activeProvider"], second["id"])
        self.assertEqual(observed["provider"]["models"]["sonnet"], "kimi-k2.6")
        self.assertEqual(observed["base_url"], "http://127.0.0.1:18080")
        self.assertIn("桌面版模型已同步", tray.icon.notifications[0][1])
        restart_dialog.assert_called_once()
        self.assertIs(restart_dialog.call_args.args[1], True)

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

        with patch("main.show_message_box", return_value=True) as message_box:
            tray.show_desktop_restart_dialog({"name": "Kimi"}, desktop_synced=True)

        message_box.assert_called_once()
        self.assertEqual(message_box.call_args.args[0], "需要重启 Claude 桌面版")
        self.assertIn("Kimi", message_box.call_args.args[1])
        self.assertIn("重新打开 Claude 桌面版", message_box.call_args.args[1])


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
        self.assertNotIn("本机代理", html + i18n)
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
        self.assertIn("installUpdate(updateUrl)", api_js)
        self.assertIn("assets/providers/aliyun.ico", api_js)
        self.assertTrue((self.root / "frontend" / "assets" / "providers" / "aliyun.ico").exists())
        self.assertIn("restartReminderStorageKey", app_js)
        self.assertIn("showRestartReminder", app_js)
        self.assertIn("toast.defaultUpdatedDesktop", app_js + i18n)
        self.assertIn("restartReminder.dontShow", i18n)
        self.assertIn("confirm.installUpdate", app_js + i18n)


if __name__ == "__main__":
    unittest.main()
