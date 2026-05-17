# Feedback Worker

Codex App Transfer 用户反馈收集 Worker —— 跑在 Cloudflare,接收应用提交的文本/截图/日志,落 R2 + 通过 Resend 发邮件通知。

## 架构

```
[App] → POST multipart/form-data → [Worker] → R2 (durable storage)
                                            → Resend → 邮件
```

## 资源(已创建)

| 资源 | 名称 / ID |
|---|---|
| R2 Bucket | `codex-app-transfer-feedback` |
| KV Namespace | `FEEDBACK_RATE_LIMIT` (`e372cb1b0bf747dd9744c4a4cce4f42c`) |
| Worker | `codex-app-transfer-feedback`(部署后生效) |

## 部署步骤

前置:已经 `wrangler login`(OAuth)。

```bash
cd feedback-worker

# 1. 填 .env(只需要填 RESEND_API_KEY)
$EDITOR .env

# 2. 把 .env 的值推到 Worker secret
chmod +x setup-secrets.sh
./setup-secrets.sh

# 3. 部署 Worker
env -u CLOUDFLARE_API_TOKEN wrangler deploy
```

部署完会输出 Worker URL,形如 `https://codex-app-transfer-feedback.<your-subdomain>.workers.dev`。当前 v2 主线的 `/api/feedback` 仍是 Rust 端暂存占位,Worker URL 接入会在后续 v2.0.x 中恢复;如果维护 v1.x 历史分支,请在对应 tag / branch 的 `backend/main.py` 中配置 `FEEDBACK_WORKER_URL`。

## 自测(curl)

```bash
WORKER_URL="https://codex-app-transfer-feedback.<your-subdomain>.workers.dev"

# 健康检查
curl "$WORKER_URL"

# 测试反馈
curl -X POST "$WORKER_URL" \
  -F 'title=测试反馈' \
  -F 'body=Hello from curl' \
  -F 'meta={"app_version":"1.0.1","os":"macOS"}'
# 期望返回 {"ok":true,"id":"fb-xxxxxxxx","email_sent":true}
```

成功的话:
1. NOTIFY_EMAIL_TO 收到一封 `[反馈] 测试反馈 · fb-xxx` 邮件
2. Cloudflare Dashboard → R2 → `codex-app-transfer-feedback` 桶里有 `feedback/<日期>/fb-xxx/meta.json`

## 速率/大小限制

- 单 IP 每天 10 条
- 单次提交总 ≤ 10MB,单文件 ≤ 5MB
- 邮件附件总 < 8MB(超过的只在 R2 存,不附邮件)

可在 `wrangler.toml` 的 `[vars]` 区块调。

## 隐私

- IP 不存原值,只存 `SHA-256(IP + IP_HASH_SALT)` 前 16 位
- IP_HASH_SALT 一旦生成不要改 —— 改了等于所有历史 hash 失效

## 排错

- 部署后 GET 应返回 `{"ok":true,"service":"..."}`
- 邮件没收到看 Resend dashboard → Logs;NOTIFY_EMAIL_FROM 用 `onboarding@resend.dev` 时只能发到 Resend 账号注册邮箱
- R2 没数据但邮件来了:`env -u CLOUDFLARE_API_TOKEN wrangler tail` 看实时日志
