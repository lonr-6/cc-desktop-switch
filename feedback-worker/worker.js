// Codex App Transfer 用户反馈 Worker
// 接收 multipart/form-data,落 R2 + 通过 Resend 发邮件通知。

export default {
  async fetch(request, env, ctx) {
    // 健康检查
    if (request.method === "GET") {
      return jsonResponse({ ok: true, service: "codex-app-transfer-feedback" });
    }

    if (request.method !== "POST") {
      return jsonResponse({ error: "method_not_allowed" }, 405);
    }

    // 1. IP 速率限制
    const ip = request.headers.get("CF-Connecting-IP") || "unknown";
    const today = new Date().toISOString().slice(0, 10);
    const ipKey = `ip:${ip}:${today}`;
    const ipCount = parseInt((await env.RATE_LIMIT.get(ipKey)) || "0", 10);
    const dailyLimit = parseInt(env.DAILY_IP_LIMIT || "10", 10);
    if (ipCount >= dailyLimit) {
      return jsonResponse({ error: "rate_limited", retry_after_hours: 24 }, 429);
    }

    // 2. 解析 multipart
    const contentType = request.headers.get("Content-Type") || "";
    if (!contentType.startsWith("multipart/form-data")) {
      return jsonResponse({ error: "bad_content_type", expected: "multipart/form-data" }, 400);
    }

    let form;
    try {
      form = await request.formData();
    } catch (e) {
      return jsonResponse({ error: "form_parse_failed", message: String(e) }, 400);
    }

    const title = (form.get("title") || "").toString().slice(0, 200);
    const contactEmail = (form.get("contact_email") || "").toString().trim().slice(0, 200);
    const body = (form.get("body") || "").toString().slice(0, 50000);
    let meta = {};
    try {
      meta = JSON.parse((form.get("meta") || "{}").toString());
    } catch {}

    if (!body.trim()) {
      return jsonResponse({ error: "empty_body" }, 400);
    }

    // 3. 生成 ID + 路径
    const id = `fb-${crypto.randomUUID().replace(/-/g, "").slice(0, 8)}`;
    const prefix = `feedback/${today}/${id}`;
    const ipHashHex = await sha256Hex(ip + (env.IP_HASH_SALT || "salt"));
    const ipHash = ipHashHex.slice(0, 16);

    // 4. 写 meta.json 到 R2
    const metaContent = {
      id,
      title,
      contact_email: contactEmail,
      body,
      submitted_at: new Date().toISOString(),
      ip_hash: ipHash,
      country: request.headers.get("CF-IPCountry") || "??",
      ...meta, // app_version, os, arch, active_provider_name 等(应用端传)
    };
    await env.R2_BUCKET.put(`${prefix}/meta.json`, JSON.stringify(metaContent, null, 2), {
      httpMetadata: { contentType: "application/json; charset=utf-8" },
    });

    // 5. 写附件,顺便收集邮件附件(<5MB 以内的小附件直接邮件附,大的只留 R2)
    const maxTotal = parseInt(env.MAX_TOTAL_BYTES || "10485760", 10);
    const maxFile = parseInt(env.MAX_FILE_BYTES || "5242880", 10);
    let totalSize = 0;
    const emailAttachments = [];
    const fileSummary = [];

    for (const [key, value] of form.entries()) {
      if (typeof value === "string") continue;
      if (!value || typeof value.size !== "number") continue;
      if (value.size > maxFile) {
        fileSummary.push(`${value.name} (跳过:${formatSize(value.size)} 超过单文件上限)`);
        continue;
      }
      if (totalSize + value.size > maxTotal) {
        fileSummary.push(`${value.name} (跳过:总大小超 ${formatSize(maxTotal)})`);
        continue;
      }

      // 决定子目录
      let subdir;
      if (key.startsWith("screenshot")) subdir = "screenshots";
      else if (key.startsWith("log")) subdir = "logs";
      else subdir = "attachments";

      const safeName = (value.name || `${key}.bin`).replace(/[^a-zA-Z0-9._-]/g, "_");
      const path = `${prefix}/${subdir}/${safeName}`;

      const bytes = new Uint8Array(await value.arrayBuffer());
      await env.R2_BUCKET.put(path, bytes, {
        httpMetadata: { contentType: value.type || "application/octet-stream" },
      });
      totalSize += value.size;
      fileSummary.push(`${value.name} (${formatSize(value.size)}, ${subdir})`);

      // 邮件附件:总大小 < 8MB 时才附,避免 Resend 单封邮件被拒
      if (emailAttachments.reduce((s, a) => s + a.bytes, 0) + value.size < 8 * 1024 * 1024) {
        emailAttachments.push({
          filename: safeName,
          content: arrayBufferToBase64(bytes),
          bytes: value.size,
        });
      }
    }

    // 6. 更新 IP 计数(KV 24h 过期)
    await env.RATE_LIMIT.put(ipKey, String(ipCount + 1), { expirationTtl: 86400 });

    // 7. 通过 Resend 发邮件(失败不阻塞反馈本身)
    const emailResult = await sendNotificationEmail(env, {
      id, title, body, meta: metaContent, fileSummary, emailAttachments,
    });

    return jsonResponse({
      ok: true,
      id,
      email_sent: emailResult.ok,
      email_error: emailResult.error,
    });
  },
};


// ============== helpers ==============

function jsonResponse(obj, status = 200) {
  return new Response(JSON.stringify(obj, null, 2), {
    status,
    headers: { "Content-Type": "application/json; charset=utf-8" },
  });
}

async function sha256Hex(s) {
  const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(s));
  return Array.from(new Uint8Array(buf))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function formatSize(n) {
  if (n < 1024) return `${n}B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)}KB`;
  return `${(n / 1024 / 1024).toFixed(2)}MB`;
}

function arrayBufferToBase64(bytes) {
  let binary = "";
  for (let i = 0; i < bytes.byteLength; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

async function sendNotificationEmail(env, { id, title, body, meta, fileSummary, emailAttachments }) {
  if (!env.RESEND_API_KEY) {
    return { ok: false, error: "RESEND_API_KEY not configured" };
  }
  if (!env.NOTIFY_EMAIL_TO) {
    return { ok: false, error: "NOTIFY_EMAIL_TO not configured" };
  }

  const from = env.NOTIFY_EMAIL_FROM || "Codex App Transfer Feedback <onboarding@resend.dev>";
  const subject = `[反馈] ${title || "(无标题)"} · ${id}`;

  const lines = [
    `📬 收到一条新反馈`,
    ``,
    `**ID**: ${id}`,
    `**提交时间**: ${meta.submitted_at}`,
    `**应用版本**: ${meta.app_version || "unknown"}`,
    `**系统**: ${meta.os || "?"} ${meta.arch || ""}`,
    `**Active Provider**: ${meta.active_provider_name || "(无)"}`,
    `**国家**: ${meta.country || "??"}`,
    `**IP Hash**: ${meta.ip_hash}`,
    `**联系邮箱**: ${meta.contact_email || "(未填写)"}`,
    ``,
    `## 标题`,
    title || "(无)",
    ``,
    `## 描述`,
    body,
    ``,
    `## 附件 (${fileSummary.length})`,
    fileSummary.length ? fileSummary.map((f) => `- ${f}`).join("\n") : "(无)",
    ``,
    `## 数据位置`,
    `R2: codex-app-transfer-feedback/feedback/${meta.submitted_at.slice(0, 10)}/${id}/`,
    `(Cloudflare Dashboard → R2 → codex-app-transfer-feedback 桶查看)`,
  ];
  const text = lines.join("\n");

  const payload = {
    from,
    to: [env.NOTIFY_EMAIL_TO],
    subject,
    text,
  };
  if (emailAttachments.length) {
    payload.attachments = emailAttachments.map(({ filename, content }) => ({ filename, content }));
  }

  try {
    const resp = await fetch("https://api.resend.com/emails", {
      method: "POST",
      headers: {
        "Authorization": `Bearer ${env.RESEND_API_KEY}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    });
    if (!resp.ok) {
      const errText = await resp.text();
      return { ok: false, error: `resend ${resp.status}: ${errText.slice(0, 300)}` };
    }
    return { ok: true };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
}
