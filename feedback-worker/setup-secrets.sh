#!/usr/bin/env bash
# 把 .env 里的 RESEND_API_KEY / NOTIFY_EMAIL_TO / NOTIFY_EMAIL_FROM 推到
# Cloudflare Worker 的 secret(运行时的环境变量,不进代码不进 toml)。
#
# 用法:
#   cd feedback-worker
#   编辑 .env 填 RESEND_API_KEY
#   ./setup-secrets.sh

set -euo pipefail
cd "$(dirname "$0")"

if [[ ! -f .env ]]; then
    echo "✗ .env 不存在,先 cp .env.example .env 然后编辑" >&2
    exit 1
fi

# 读取 .env(忽略注释空行)
set -a
source .env
set +a

if [[ -z "${RESEND_API_KEY:-}" || "${RESEND_API_KEY}" == "re_xxxxxxxxxxxxxxxxxxxx" ]]; then
    echo "✗ .env 里的 RESEND_API_KEY 未填,先去 https://resend.com 创建" >&2
    exit 1
fi
if [[ -z "${NOTIFY_EMAIL_TO:-}" ]]; then
    echo "✗ .env 里的 NOTIFY_EMAIL_TO 未填" >&2
    exit 1
fi
NOTIFY_EMAIL_FROM="${NOTIFY_EMAIL_FROM:-onboarding@resend.dev}"

# 屏蔽 shell env 里的 CLOUDFLARE_API_TOKEN(用 OAuth 登录的 token,而不是受限的 env var)
WRANGLER="env -u CLOUDFLARE_API_TOKEN wrangler"

echo "==> 推送 RESEND_API_KEY"
echo -n "$RESEND_API_KEY" | $WRANGLER secret put RESEND_API_KEY

echo "==> 推送 NOTIFY_EMAIL_TO"
echo -n "$NOTIFY_EMAIL_TO" | $WRANGLER secret put NOTIFY_EMAIL_TO

echo "==> 推送 NOTIFY_EMAIL_FROM"
echo -n "$NOTIFY_EMAIL_FROM" | $WRANGLER secret put NOTIFY_EMAIL_FROM

echo
echo "✓ 三个 secret 已推送到 Worker"
echo "下一步:env -u CLOUDFLARE_API_TOKEN wrangler deploy"
