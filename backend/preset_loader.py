"""内置 provider 预设文件加载与 schema 校验。"""

from __future__ import annotations

import json
import logging
import re
from pathlib import Path
from typing import Any


logger = logging.getLogger(__name__)
PRESETS_DIR = Path(__file__).resolve().parent / "presets"
PRESET_SCHEMA_FILE = PRESETS_DIR / "schema.json"


def _read_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def _value_type_name(value: Any) -> str:
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, dict):
        return "object"
    if isinstance(value, list):
        return "array"
    if isinstance(value, str):
        return "string"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, float):
        return "number"
    if value is None:
        return "null"
    return type(value).__name__


def _matches_type(value: Any, expected_type: str) -> bool:
    checks = {
        "array": lambda item: isinstance(item, list),
        "boolean": lambda item: isinstance(item, bool),
        "integer": lambda item: isinstance(item, int) and not isinstance(item, bool),
        "number": lambda item: isinstance(item, (int, float)) and not isinstance(item, bool),
        "object": lambda item: isinstance(item, dict),
        "string": lambda item: isinstance(item, str),
    }
    checker = checks.get(str(expected_type or "").strip().lower())
    return checker(value) if checker else True


def _schema_path(path: str, key: str) -> str:
    return f"{path}.{key}" if path != "$" else f"$.{key}"


def _validate_schema(value: Any, schema: dict[str, Any], path: str = "$") -> list[str]:
    errors: list[str] = []

    expected_type = schema.get("type")
    if isinstance(expected_type, str):
        expected_types = [expected_type]
    elif isinstance(expected_type, list):
        expected_types = [item for item in expected_type if isinstance(item, str)]
    else:
        expected_types = []

    if expected_types and not any(_matches_type(value, item) for item in expected_types):
        expected_text = " or ".join(expected_types)
        return [f"{path}: expected {expected_text}, got {_value_type_name(value)}"]

    if "enum" in schema and value not in schema["enum"]:
        errors.append(f"{path}: expected one of {schema['enum']}, got {value!r}")

    if isinstance(value, str):
        min_length = schema.get("minLength")
        if isinstance(min_length, int) and len(value) < min_length:
            errors.append(f"{path}: expected length >= {min_length}, got {len(value)}")
        pattern = schema.get("pattern")
        if isinstance(pattern, str) and not re.fullmatch(pattern, value):
            errors.append(f"{path}: does not match pattern {pattern!r}")

    if isinstance(value, dict):
        required = schema.get("required")
        if isinstance(required, list):
            for key in required:
                if isinstance(key, str) and key not in value:
                    errors.append(f"{_schema_path(path, key)}: missing required field")

        properties = schema.get("properties") if isinstance(schema.get("properties"), dict) else {}
        additional = schema.get("additionalProperties", True)
        for key, item in value.items():
            child_path = _schema_path(path, str(key))
            if key in properties:
                errors.extend(_validate_schema(item, properties[key], child_path))
                continue
            if additional is True or additional is None:
                continue
            if additional is False:
                errors.append(f"{child_path}: unexpected field")
                continue
            if isinstance(additional, dict):
                errors.extend(_validate_schema(item, additional, child_path))

    if isinstance(value, list):
        item_schema = schema.get("items")
        if isinstance(item_schema, dict):
            for index, item in enumerate(value):
                errors.extend(_validate_schema(item, item_schema, f"{path}[{index}]"))

    return errors


def _load_preset_schema() -> dict[str, Any]:
    try:
        payload = _read_json(PRESET_SCHEMA_FILE)
    except (OSError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"内置 provider schema 加载失败: {PRESET_SCHEMA_FILE} ({exc})") from exc
    if not isinstance(payload, dict):
        raise RuntimeError(f"内置 provider schema 必须是 JSON 对象: {PRESET_SCHEMA_FILE}")
    return payload


def load_builtin_presets() -> list[dict[str, Any]]:
    """从 backend/presets 加载内置 provider 预设。"""
    if not PRESETS_DIR.exists():
        raise RuntimeError(f"内置 provider 预设目录不存在: {PRESETS_DIR}")

    schema = _load_preset_schema()
    preset_files = sorted(
        path for path in PRESETS_DIR.glob("*.json")
        if path.name != PRESET_SCHEMA_FILE.name
    )
    if not preset_files:
        raise RuntimeError(f"未找到任何内置 provider 预设数据文件: {PRESETS_DIR}")

    presets: list[dict[str, Any]] = []
    seen_ids: set[str] = set()
    for path in preset_files:
        try:
            payload = _read_json(path)
        except (OSError, json.JSONDecodeError) as exc:
            logger.warning("跳过内置 provider 预设 %s：读取失败：%s", path.name, exc)
            continue

        if not isinstance(payload, dict):
            logger.warning("跳过内置 provider 预设 %s：顶层必须是 JSON 对象", path.name)
            continue

        errors = _validate_schema(payload, schema)
        if errors:
            logger.warning("跳过内置 provider 预设 %s：%s", path.name, "; ".join(errors[:5]))
            continue

        preset_id = str(payload.get("id") or "").strip()
        if preset_id in seen_ids:
            logger.warning("跳过内置 provider 预设 %s：重复的 id %s", path.name, preset_id)
            continue
        seen_ids.add(preset_id)
        presets.append(payload)

    if not presets:
        raise RuntimeError(f"未加载到任何有效的内置 provider 预设: {PRESETS_DIR}")

    return presets