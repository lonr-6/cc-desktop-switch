import copy
import os
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

from fastapi.testclient import TestClient

from backend.main import create_admin_app
from main import DesktopTrayController
from backend import config as cfg
from backend import provider_tools
from backend import registry
from backend.proxy import (
    _anthropic_to_openai_body,
    _normalize_anthropic_response,
    _normalize_anthropic_sse_event,
    _openai_chunk_to_anthropic,
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

    def test_builtin_presets_include_expected_provider_urls(self):
        presets = {preset["id"]: preset for preset in cfg.get_presets()}

        expected_urls = {
            "deepseek": "https://api.deepseek.com/anthropic",
            "kimi": "https://api.moonshot.cn/anthropic",
            "qiniu": "https://api.qnaigc.com",
            "zhipu": "https://open.bigmodel.cn/api/anthropic",
            "siliconflow": "https://api.siliconflow.cn",
            "bailian": "https://dashscope.aliyuncs.com/apps/anthropic",
        }

        for preset_id, base_url in expected_urls.items():
            self.assertIn(preset_id, presets)
            self.assertEqual(presets[preset_id]["baseUrl"], base_url)
            self.assertEqual(presets[preset_id]["apiFormat"], "anthropic")
            self.assertTrue(presets[preset_id]["models"]["default"])

        self.assertEqual(presets["kimi"]["models"]["default"], "kimi-k2.6")
        self.assertEqual(presets["qiniu"]["models"]["default"], "moonshotai/kimi-k2-thinking")
        self.assertEqual(presets["zhipu"]["models"]["haiku"], "glm-4.7")
        self.assertEqual(presets["siliconflow"]["models"]["default"], "Pro/moonshotai/Kimi-K2.5")

        deepseek_1m = presets["deepseek"]["modelOptions"]["deepseek_1m"]
        self.assertEqual(deepseek_1m["models"]["sonnet"], "deepseek-v4-pro[1m]")
        self.assertEqual(deepseek_1m["models"]["opus"], "deepseek-v4-pro[1m]")
        self.assertEqual(deepseek_1m["models"]["default"], "deepseek-v4-pro[1m]")

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
        self.assertTrue(tray.switch_provider(second["id"]))

        self.assertEqual(cfg.load_config()["activeProvider"], second["id"])
        self.assertEqual(tray.icon.updated, 1)
        self.assertIn("Kimi", tray.icon.notifications[0][1])


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


if __name__ == "__main__":
    unittest.main()
