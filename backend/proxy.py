"""本地代理服务 - 模型名翻译 + 请求转发 + SSE 流式处理"""

import json
import uuid
from datetime import datetime
from typing import Optional

import httpx
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, StreamingResponse

# 标准 Claude 模型名列表（用于匹配和显示）
CLAUDE_MODEL_NAMES = {
    "sonnet": [
        "claude-sonnet-4-6", "claude-sonnet-4-5", "claude-sonnet-4-0",
        "claude-3-sonnet",
    ],
    "opus": [
        "claude-opus-4-7", "claude-opus-4-5", "claude-opus-4-0",
        "claude-3-opus",
    ],
    "haiku": [
        "claude-haiku-3-5", "claude-haiku-4-5", "claude-3-haiku",
    ],
}


class ProxyStats:
    """代理统计"""

    def __init__(self):
        self.total = 0
        self.success = 0
        self.failed = 0
        self.today = 0
        self._date = datetime.now().strftime("%Y-%m-%d")

    def record(self, success: bool):
        self.total += 1
        today_str = datetime.now().strftime("%Y-%m-%d")
        if today_str != self._date:
            self.today = 0
            self._date = today_str
        self.today += 1
        if success:
            self.success += 1
        else:
            self.failed += 1

    def to_dict(self):
        return {
            "total": self.total,
            "success": self.success,
            "failed": self.failed,
            "today": self.today,
        }


class LogBuffer:
    """环形日志缓冲区"""

    def __init__(self, max_size=200):
        self._logs = []
        self._max_size = max_size

    def add(self, level: str, message: str):
        self._logs.append({
            "time": datetime.now().strftime("%H:%M:%S"),
            "level": level,
            "message": message,
        })
        if len(self._logs) > self._max_size:
            self._logs = self._logs[-self._max_size:]

    def get_all(self):
        return list(self._logs)

    def clear(self):
        self._logs = []


# 全局单例
stats = ProxyStats()
log_buffer = LogBuffer()


def map_model(original_model: str, provider: Optional[dict]) -> str:
    """映射模型名：将标准 Claude 模型名映射为提供商的自定义模型名"""
    if not provider or not original_model:
        return original_model

    models_config = provider.get("models", {})
    if not models_config:
        return original_model

    model_lower = original_model.lower()

    # 按优先级匹配：opus → haiku → sonnet
    for tier, keywords in [("opus", CLAUDE_MODEL_NAMES["opus"]),
                            ("haiku", CLAUDE_MODEL_NAMES["haiku"]),
                            ("sonnet", CLAUDE_MODEL_NAMES["sonnet"])]:
        for kw in keywords:
            if kw in model_lower:
                mapped = models_config.get(tier)
                if mapped:
                    return mapped

    # 也直接检查关键词
    if "opus" in model_lower:
        return models_config.get("opus") or models_config.get("default") or original_model
    if "haiku" in model_lower:
        return models_config.get("haiku") or models_config.get("default") or original_model
    if "sonnet" in model_lower:
        return models_config.get("sonnet") or models_config.get("default") or original_model

    # 默认模型
    return models_config.get("default") or original_model


def build_upstream_url(base_url: str, api_format: str) -> str:
    """根据用户填写的 Base URL 生成最终请求地址。

    用户可能填写基础地址，也可能直接粘贴完整 endpoint；这里统一处理，
    避免重复拼接 /v1/messages 或 /chat/completions。
    """
    clean = str(base_url or "").strip().rstrip("/")
    api_format = str(api_format or "anthropic").lower()
    lower = clean.lower()
    if api_format == "openai":
        if lower.endswith("/chat/completions"):
            return clean
        return f"{clean}/chat/completions"
    if lower.endswith("/v1/messages"):
        return clean
    if lower.endswith("/v1"):
        return f"{clean}/messages"
    return f"{clean}/v1/messages"


def _content_to_text(content) -> str:
    """把 Anthropic 文本块转换为 OpenAI 兼容接口常见的字符串 content。"""
    if content is None:
        return ""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = []
        for item in content:
            if isinstance(item, str):
                parts.append(item)
            elif isinstance(item, dict):
                if isinstance(item.get("text"), str):
                    parts.append(item["text"])
                elif isinstance(item.get("content"), str):
                    parts.append(item["content"])
                elif isinstance(item.get("content"), list):
                    text = _content_to_text(item["content"])
                    if text:
                        parts.append(text)
        return "\n".join(part for part in parts if part)
    return str(content)


def _anthropic_to_openai_body(body: dict, stream: bool) -> dict:
    """将 Claude Desktop 发来的 Anthropic Messages 请求转换为 OpenAI Chat。"""
    messages = [dict(message) for message in body.get("messages", [])]
    system_msg = body.get("system")
    if not system_msg and messages and messages[0].get("role") == "system":
        system_msg = messages.pop(0).get("content")

    openai_messages = []
    system_text = _content_to_text(system_msg)
    if system_text:
        openai_messages.append({"role": "system", "content": system_text})

    for message in messages:
        role = message.get("role", "user")
        if role not in {"system", "user", "assistant", "tool"}:
            role = "user"
        content = _content_to_text(message.get("content"))
        openai_messages.append({"role": role, "content": content})

    openai_body = {
        "model": body.get("model", ""),
        "messages": openai_messages,
        "max_tokens": body.get("max_tokens", 4096),
        "stream": stream,
    }
    if "temperature" in body and body["temperature"] is not None:
        openai_body["temperature"] = body["temperature"]
    if "top_p" in body and body["top_p"] is not None:
        openai_body["top_p"] = body["top_p"]
    if body.get("stop_sequences"):
        openai_body["stop"] = body["stop_sequences"]
    return openai_body


def get_upstream_headers(provider: dict) -> dict:
    """获取上游请求的认证头"""
    auth_scheme = provider.get("authScheme", "bearer")
    api_key = provider.get("apiKey", "")

    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json",
    }

    if str(provider.get("apiFormat", "anthropic")).lower() == "anthropic":
        headers["anthropic-version"] = "2023-06-01"

    if api_key:
        if auth_scheme == "x-api-key":
            headers["x-api-key"] = api_key
        else:
            headers["Authorization"] = f"Bearer {api_key}"

    # 合并提供商自定义的额外请求头（如 DeepSeek 需要同时发 x-api-key）
    extra = provider.get("extraHeaders", {})
    if isinstance(extra, dict):
        for k, v in extra.items():
            # 支持 {apiKey} 模板变量
            headers[k] = v.replace("{apiKey}", api_key) if isinstance(v, str) else v

    return headers


async def forward_request(
    body: dict,
    provider: dict,
    request_id: str,
) -> dict:
    """转发请求到上游 API（非流式）"""
    api_format = str(provider.get("apiFormat", "anthropic")).lower()

    if api_format == "openai":
        upstream_url = build_upstream_url(provider.get("baseUrl", ""), api_format)
        upstream_body = _anthropic_to_openai_body(body, stream=False)
    else:
        # Anthropic 格式透传
        upstream_url = build_upstream_url(provider.get("baseUrl", ""), api_format)

        # 移除流式标记（我们单独处理流式）
        upstream_body = dict(body)
        upstream_body.pop("stream", None)

        # 移除 thinking 相关字段（某些提供商不支持）
        upstream_body.pop("thinking", None)

    headers = get_upstream_headers(provider)

    log_buffer.add("INFO", f"转发请求 → {upstream_url}")
    log_buffer.add("INFO", f"模型: {body.get('model', '')} → {upstream_body.get('model', '')}")

    try:
        async with httpx.AsyncClient(timeout=120.0) as client:
            resp = await client.post(
                upstream_url,
                json=upstream_body,
                headers=headers,
            )

        stats.record(resp.is_success)
        log_buffer.add(
            "SUCCESS" if resp.is_success else "ERROR",
            f"响应 {resp.status_code} ({round(resp.elapsed.total_seconds(), 2)}s)",
        )

        if not resp.is_success:
            return {
                "error": {
                    "type": "upstream_error",
                    "status": resp.status_code,
                    "message": resp.text[:500] or "上游 API 返回错误",
                }
            }

        try:
            upstream_data = resp.json()
        except json.JSONDecodeError:
            stats.failed += 1
            stats.success = max(0, stats.success - 1)
            log_buffer.add("ERROR", "上游 API 返回了非 JSON 响应")
            return {
                "error": {
                    "type": "invalid_upstream_response",
                    "message": "上游 API 返回了非 JSON 响应",
                }
            }

        if api_format == "openai":
            # OpenAI → Anthropic 格式转换
            return _openai_to_anthropic(upstream_data, body.get("model", ""))
        return upstream_data

    except httpx.TimeoutException:
        stats.record(False)
        log_buffer.add("ERROR", "请求超时")
        return {"error": {"type": "timeout", "message": "上游 API 请求超时"}}
    except Exception as e:
        stats.record(False)
        message = f"{e.__class__.__name__}: {str(e)}".rstrip()
        log_buffer.add("ERROR", f"请求失败: {message}")
        return {"error": {"type": "connection_error", "message": message}}


async def forward_request_stream(
    body: dict,
    provider: dict,
    request_id: str,
):
    """转发流式请求到上游 API（SSE）"""
    api_format = str(provider.get("apiFormat", "anthropic")).lower()

    if api_format == "openai":
        upstream_url = build_upstream_url(provider.get("baseUrl", ""), api_format)
        upstream_body = _anthropic_to_openai_body(body, stream=True)
    else:
        upstream_url = build_upstream_url(provider.get("baseUrl", ""), api_format)
        upstream_body = dict(body)
        upstream_body.pop("thinking", None)
        # 确保流式开启
        upstream_body["stream"] = True

    headers = get_upstream_headers(provider)

    log_buffer.add("INFO", f"流式请求 → {upstream_url}")

    try:
        async with httpx.AsyncClient(timeout=300.0) as client:
            async with client.stream(
                "POST",
                upstream_url,
                json=upstream_body,
                headers=headers,
            ) as resp:

                log_buffer.add(
                    "SUCCESS" if resp.is_success else "ERROR",
                    f"流式连接 {resp.status_code}",
                )

                if not resp.is_success:
                    stats.record(False)
                    error_text = (await resp.aread()).decode("utf-8", errors="replace")[:500]
                    error_event = {
                        "type": "error",
                        "error": {
                            "type": "upstream_error",
                            "status": resp.status_code,
                            "message": error_text or "上游 API 返回错误",
                        },
                    }
                    yield f"data: {json.dumps(error_event, ensure_ascii=False)}\n\n"
                    return

                if api_format == "openai":
                    prefix = "data: "
                    async for line in resp.aiter_lines():
                        if not line.strip():
                            continue
                        if line.startswith(prefix):
                            data_str = line[len(prefix):]
                            if data_str.strip() == "[DONE]":
                                yield "event: done\ndata: {}\n\n"
                                continue
                            try:
                                openai_chunk = json.loads(data_str)
                                anthropic_chunk = _openai_chunk_to_anthropic(openai_chunk, body.get("model", ""))
                                yield f"data: {json.dumps(anthropic_chunk)}\n\n"
                            except json.JSONDecodeError:
                                continue
                else:
                    async for line in resp.aiter_lines():
                        yield line + "\n"

                stats.record(True)
                log_buffer.add("SUCCESS", f"流式完成")

    except Exception as e:
        stats.record(False)
        message = f"{e.__class__.__name__}: {str(e)}".rstrip()
        log_buffer.add("ERROR", f"流式请求失败: {message}")
        error_event = {
            "type": "error",
            "error": {"message": message},
        }
        yield f"data: {json.dumps(error_event)}\n\n"


def _openai_to_anthropic(openai_resp: dict, model: str) -> dict:
    """将 OpenAI 响应格式转换为 Anthropic 格式"""
    choice = openai_resp.get("choices", [{}])[0]
    message = choice.get("message", {})
    content = message.get("content", "")

    # 提取 usage
    usage = openai_resp.get("usage", {})

    return {
        "id": openai_resp.get("id", f"msg_{uuid.uuid4().hex[:12]}"),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{"type": "text", "text": content}],
        "usage": {
            "input_tokens": usage.get("prompt_tokens", 0),
            "output_tokens": usage.get("completion_tokens", 0),
        },
    }


def _openai_chunk_to_anthropic(chunk: dict, model: str) -> dict:
    """将 OpenAI 流式块转换为 Anthropic SSE 格式"""
    choices = chunk.get("choices", [])
    if not choices:
        return {"type": "message_stop"}

    delta = choices[0].get("delta", {})
    finish_reason = choices[0].get("finish_reason")

    content = delta.get("content", "")
    if not content:
        if finish_reason:
            return {"type": "message_stop"}
        # 可能有 role 块但没有内容
        if delta.get("role"):
            return {
                "type": "message_start",
                "message": {
                    "id": f"msg_{uuid.uuid4().hex[:12]}",
                    "type": "message",
                    "role": "assistant",
                    "model": model,
                    "content": [],
                },
            }
        return {"type": "ping"}

    return {
        "type": "content_block_delta",
        "index": 0,
        "delta": {
            "type": "text_delta",
            "text": content,
        },
    }


# ========== FastAPI 应用 ==========

from backend.config import get_active_provider, get_gateway_api_key


def create_proxy_app() -> FastAPI:
    """创建代理 FastAPI 应用"""
    app = FastAPI(title="CC Desktop Switch Proxy", version="1.0.2")

    @app.get("/health")
    @app.get("/status")
    async def health():
        return {"status": "ok", "stats": stats.to_dict()}

    @app.api_route("/v1/messages", methods=["POST", "OPTIONS"])
    @app.api_route("/claude/v1/messages", methods=["POST", "OPTIONS"])
    async def handle_messages(request: Request):
        if request.method == "OPTIONS":
            return JSONResponse(
                content={},
                headers={
                    "Access-Control-Allow-Origin": "*",
                    "Access-Control-Allow-Methods": "POST, OPTIONS",
                    "Access-Control-Allow-Headers": "*",
                },
            )

        request_id = request.headers.get("x-request-id", uuid.uuid4().hex[:12])
        body = await request.json()

        gateway_api_key = get_gateway_api_key()
        if gateway_api_key:
            auth_header = request.headers.get("authorization", "")
            bearer_token = auth_header.removeprefix("Bearer ").strip()
            x_api_key = request.headers.get("x-api-key", "").strip()
            if gateway_api_key not in {bearer_token, x_api_key}:
                log_buffer.add("ERROR", "本地 gateway 认证失败")
                return JSONResponse(
                    status_code=401,
                    content={"error": {"message": "Invalid gateway API key"}},
                )

        # 获取当前激活的提供商
        provider = get_active_provider()
        if not provider or not provider.get("apiKey"):
            log_buffer.add("ERROR", "没有配置有效的提供商")
            return JSONResponse(
                status_code=400,
                content={"error": {"message": "No active provider configured"}},
            )

        # 模型名翻译
        original_model = body.get("model", "")
        mapped_model = map_model(original_model, provider)
        body["model"] = mapped_model

        log_buffer.add("INFO", f"请求: POST /v1/messages")
        log_buffer.add("INFO", f"模型映射: {original_model} → {mapped_model}")

        # 判断是否流式
        is_stream = body.get("stream", False)

        if is_stream:
            return StreamingResponse(
                forward_request_stream(body, provider, request_id),
                media_type="text/event-stream",
                headers={
                    "Cache-Control": "no-cache",
                    "Connection": "keep-alive",
                    "Access-Control-Allow-Origin": "*",
                },
            )
        else:
            result = await forward_request(body, provider, request_id)
            return JSONResponse(content=result)

    return app
