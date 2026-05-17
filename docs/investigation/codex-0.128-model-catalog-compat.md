# Codex 0.128 Model Catalog 兼容方案

> 状态:PR #12 本地整改推进中  
> 起草日期:2026-05-05  
> 适用范围:Codex CLI 集成、模型路由、1M 上下文能力声明  
> 目标:让 1M provider 在 Codex CLI 0.128+ 下被正确识别,同时保持旧版 Codex 兼容和本应用配置恢复能力。
> 当前分支:`codex/pr-12-codex-128-config-catalog`

## 1. 背景与已验证事实

当前 Rust 主线的 Codex CLI 集成沿用了 v1.0.3 的方案:当 active provider 支持 1M 上下文时,在 `~/.codex/config.toml` 写入:

```toml
model_context_window = 1000000
```

这个方案对旧版 Codex 有兼容价值,但已不满足 Codex CLI 0.128 的模型能力读取路径。当前实测环境如下:

- `/opt/homebrew/bin/codex --version` 输出 `codex-cli 0.128.0`。
- `/opt/homebrew/bin/codex debug models -c model_context_window=1000000` 下,`gpt-5.5` 仍显示 `context_window = 272000`、`max_context_window = 272000`。
- `/opt/homebrew/bin/codex debug models -c 'model_catalog_json="<catalog path>"'` 下,当 catalog 中声明 `gpt-5.5.context_window = 1000000`、`max_context_window = 1000000`、`effective_context_window_percent = 95` 时,Codex 0.128 会实际读取并显示 1M。
- 按 PR #11 中 `model_catalog.rs` 的 JSON 形状构造临时 catalog 后,Codex 0.128 可以解析该 schema。

因此,`model_context_window` 不能再作为 Codex 0.128 的硬验收依据。0.128+ 的有效入口应是 `model_catalog_json`,旧 key 只作为向后兼容保留。

## 2. 当前代码存在的问题

### 2.1 1M 上下文能力只写旧 key

当前 `crates/codex_integration/src/apply.rs` 的 managed TOML key 只有:

```rust
const MANAGED_TOML_KEYS: &[&str] = &["openai_base_url", "model_context_window"];
```

`apply_provider()` 在 `supports_1m = true` 时只写 `model_context_window = 1000000`,未生成 Codex 0.128 所需的 model catalog,也未写 `model_catalog_json`。

实际结果是:用户启用 1M provider 后,代理上游可能已经指向 1M 模型,但 Codex CLI 0.128 的模型目录仍按内置 `gpt-5.5` 等槽位计算上下文窗口,会继续按约 272k 处理。

### 2.2 model catalog 文件缺少明确所有权和生命周期

PR #11 的方向是写:

```toml
model_catalog_json = "~/.codex/codex-app-transfer-models.json"
```

并生成 `codex-app-transfer-models.json`。这个方向成立,但文件边界必须补完整。否则会出现以下风险:

- 切换到非 1M provider 后,TOML key 被删除,但旧 catalog 文件仍残留。
- restore 时只恢复 `config.toml` 中的 key,没有恢复或删除本应用生成的 catalog 文件。
- 如果用户原本自己配置了 `model_catalog_json`,restore 必须回到用户原值,不能无条件删除用户文件。
- 多次 apply 时,本应用生成的 catalog 需要能被稳定覆盖,但不能污染 snapshot 中的用户原始配置。

### 2.3 Codex 侧模型槽位没有映射到 provider 真实模型

当前 `crates/proxy/src/resolver.rs` 只支持 body 中的 `model = "<slug>/<real_model>"` 路由形式。如果 Codex 根据 catalog 发出 `gpt-5.5`、`gpt-5.4`、`gpt-5.4-mini`、`gpt-5.3-codex`、`gpt-5.2` 这类槽位模型,当前 resolver 会走默认 provider,但不会把模型名改写成 provider 的真实模型。

实际风险是:上游 provider 收到 `gpt-5.5` 这种 OpenAI/Codex 侧槽位名,而不是 `deepseek-v4-pro`、`kimi-k2.6` 等真实模型名,从而返回未知模型错误或落到错误模型。

### 2.4 `[1m]` 内部后缀可能泄漏到上游

当前 `src-tauri/src/admin/handlers.rs` 中的 `provider_supports_1m()` 仍兼容识别默认模型里的 `[1m]` 后缀。这个后缀可以作为旧配置的内部能力标记,但不应进入:

- model catalog 的真实模型 slug。
- proxy 转发给上游的 JSON body `model` 字段。

v1.0.3 release note 已经记录过同类问题:上游 DeepSeek 只接受 `deepseek-v4-pro` / `deepseek-v4-flash`,不接受 `deepseek-v4-pro[1m]`。Rust 主线需要把这一清洗能力补回代理转发链路。

### 2.5 reasoning-only final summary 不应混入本次修复

PR #11 还提出在 Chat SSE -> Responses SSE 转换中,当上游只返回 reasoning item、没有最终可见 assistant message 时,由 adapter 注入一段可见 fallback 文本:

```text
上游已结束本轮，但没有返回最终可见文本。Upstream completed this turn without final visible text.
```

这个症状本身可能真实存在,因为当前 converter 的测试明确允许 reasoning-only 完成流,`response.completed.output[]` 只包含 reasoning item。但默认注入 assistant 文本会改变模型输出语义,并可能污染会话历史。因此它不应和 `model_catalog_json` 修复放在同一个实现范围内。

## 3. 解决思路

### 3.1 用双轨配置兼容 Codex 新旧版本

保留:

```toml
model_context_window = 1000000
```

新增:

```toml
model_catalog_json = "<absolute app-owned catalog path>"
```

两者职责不同:

| 配置项 | 作用 | 验收方式 |
|---|---|---|
| `model_context_window` | 旧 Codex 兼容提示 | 不作为 0.128 的生效依据 |
| `model_catalog_json` | Codex 0.128+ 模型目录入口 | `codex debug models` 显示对应槽位为 1M |

### 3.2 把 catalog 视为本应用生成物

catalog 文件应由 Codex App Transfer 明确拥有,并纳入 apply / restore 生命周期。推荐路径:

```text
~/.codex/codex-app-transfer-models.json
```

选择该路径的原因:

- Codex 0.128 已验证可以通过 `model_catalog_json` 读取任意绝对路径。
- 文件名带 `codex-app-transfer`,足以区分用户手写 catalog。
- 与 `~/.codex/config.toml` 同域,便于用户排查。

也可以使用更清晰的目录形式:

```text
~/.codex/codex-app-transfer/model-catalog.json
```

无论选哪一种,实现必须满足同一个边界:只删除本应用生成的 catalog 文件,不要删除用户原本配置的 catalog 文件。

### 3.3 catalog 只声明 Codex 侧槽位,proxy 负责真实模型映射

Codex 侧仍看到熟悉的模型名:

- `gpt-5.5`
- `gpt-5.4`
- `gpt-5.4-mini`
- `gpt-5.3-codex`
- `gpt-5.2`
- active provider 的真实 default model,用于诊断和手动选择

proxy 侧负责把这些槽位模型映射到 provider 配置中的真实模型。这样能保持 Codex UI / TUI 的模型选择体验,同时避免把 `gpt-5.5` 原样发给非 OpenAI 上游。

### 3.4 `[1m]` 只保留为输入兼容标记

`[1m]` 可以继续用于判断旧配置是否支持 1M,但所有输出边界都要清洗:

- catalog 的 slug 使用清洗后的真实模型名。
- display_name 可以保留 provider 名和真实模型名,不包含 `[1m]`。
- proxy 最终 upstream body 中的 `model` 不包含 `[1m]`。

### 3.5 reasoning-only 另立问题处理

本方案不处理 final summary fallback。后续如需处理,应单独形成问题:

- 先确认 Codex UI / CLI 对 reasoning-only completed 的实际用户体验。
- 优先在 UI 或状态层显示“上游没有返回最终可见文本”。
- 不默认在 Responses adapter 中伪造 assistant message。

## 4. 详细实现方案

### 4.1 新增 model catalog 模块

新增文件:

```text
crates/codex_integration/src/model_catalog.rs
```

职责:

- 定义 `CODEX_MODEL_CATALOG_KEY = "model_catalog_json"`。
- 定义 `CatalogModel` 数据结构。
- 提供 `catalog_models_for_provider(provider_name, default_model, supports_1m, mappings)`。
- 提供 `write_catalog(path, models)`。
- 提供 `strip_model_suffix(model)` 或复用 registry/proxy 的公共函数。

catalog entry 至少包含 Codex 0.128 已验证可解析的字段:

```json
{
  "slug": "gpt-5.5",
  "display_name": "Provider / real-model",
  "description": "Routed through Codex App Transfer as Provider / real-model.",
  "default_reasoning_level": "high",
  "supported_reasoning_levels": [
    {"effort": "low", "description": "Fast responses with lighter reasoning"},
    {"effort": "medium", "description": "Balanced speed and reasoning depth"},
    {"effort": "high", "description": "Greater reasoning depth for complex tasks"}
  ],
  "shell_type": "default",
  "visibility": "list",
  "supported_in_api": true,
  "priority": 10,
  "context_window": 1000000,
  "max_context_window": 1000000,
  "effective_context_window_percent": 95,
  "experimental_supported_tools": [],
  "input_modalities": ["text", "image"],
  "supports_search_tool": false
}
```

实现时可以先采用 PR #11 已验证可解析的 schema,再视需要补齐 0.128 bundled catalog 中的更多字段。

### 4.2 扩展 CodexPaths

修改:

```text
crates/codex_integration/src/paths.rs
```

新增字段:

```rust
pub model_catalog_json: PathBuf,
```

路径建议:

```rust
codex_home.join("codex-app-transfer-models.json")
```

如果改用目录形式,则:

```rust
codex_home.join("codex-app-transfer").join("model-catalog.json")
```

测试要求:

- `CodexPaths::from_home_dir()` 生成稳定路径。
- 路径不依赖当前工作目录。

### 4.3 扩展 ApplyConfig 和 ApplyResult

修改:

```text
crates/codex_integration/src/apply.rs
```

`ApplyConfig` 新增:

```rust
pub provider_name: &'a str,
pub default_model: &'a str,
pub model_mappings: Option<&'a serde_json::Value>,
```

如果不想让 codex_integration 直接依赖 UI 原始 JSON,可以改成更干净的结构:

```rust
pub provider_name: &'a str,
pub default_model: &'a str,
pub slot_models: &'a ModelMappings,
```

`ApplyResult` 新增:

```rust
pub model_catalog_json_set: bool,
pub model_catalog_json_path: Option<String>,
```

### 4.4 apply_provider 写入新旧双轨配置

当前逻辑:

```rust
if cfg.supports_1m {
    sync_root_value(&paths.config_toml, "model_context_window", Some("1000000"))?;
} else {
    sync_root_value(&paths.config_toml, "model_context_window", None)?;
}
```

目标逻辑:

```rust
if cfg.supports_1m {
    sync_root_value(&paths.config_toml, "model_context_window", Some("1000000"))?;

    let catalog_literal = toml_string_literal(&paths.model_catalog_json.display().to_string());
    sync_root_value(
        &paths.config_toml,
        CODEX_MODEL_CATALOG_KEY,
        Some(&catalog_literal),
    )?;

    let models = catalog_models_for_provider(
        cfg.provider_name,
        cfg.default_model,
        cfg.slot_models,
        true,
    );
    write_catalog(&paths.model_catalog_json, &models)?;
} else {
    sync_root_value(&paths.config_toml, "model_context_window", None)?;
    sync_root_value(&paths.config_toml, CODEX_MODEL_CATALOG_KEY, None)?;
    remove_app_owned_catalog(paths)?;
}
```

`remove_app_owned_catalog()` 只删除 `paths.model_catalog_json` 指向的本应用固定路径,不要解析并删除用户配置中的任意 `model_catalog_json` 路径。

### 4.5 restore_codex_state 恢复 key 并清理本应用文件

`MANAGED_TOML_KEYS` 扩展为:

```rust
const MANAGED_TOML_KEYS: &[&str] = &[
    "openai_base_url",
    "model_context_window",
    CODEX_MODEL_CATALOG_KEY,
];
```

restore 行为:

- 有 snapshot 时,`model_catalog_json` 按 snapshot 中原始字面量恢复。
- snapshot 原本没有 `model_catalog_json` 时,删除该 key。
- restore 完成后,如果当前 key 不再指向本应用 catalog,删除本应用生成的 catalog 文件。
- 如果 snapshot 原本指向用户自己的 catalog 路径,只恢复 TOML key,不要删除用户 catalog 文件。

无 snapshot 的 fallback 行为:

- 删除 managed TOML keys。
- 删除本应用生成的 catalog 文件。
- 保留其他用户字段。

### 4.6 Tauri admin handler 传入 provider 信息

修改:

```text
src-tauri/src/admin/handlers.rs
```

当前 `desktop_configure()` 只计算 `supports_1m`,然后调用 `ApplyConfig`。需要增加:

- active provider 展示名。
- active provider default 模型。
- active provider 模型映射。

建议提取辅助函数:

```rust
fn provider_display_name(provider: &serde_json::Value) -> String
fn provider_default_model(provider: &serde_json::Value) -> String
fn provider_model_mappings(provider: &serde_json::Value) -> serde_json::Value
```

`provider_supports_1m()` 继续作为能力判断入口,但默认模型传给 catalog 前必须先清洗 `[1m]`。

### 4.7 proxy resolver 增加 Codex 槽位映射

修改:

```text
crates/proxy/src/resolver.rs
```

新增映射逻辑:

```rust
fn map_model_for_provider(provider: &Provider, requested_model: &str) -> Option<String> {
    let mappings = normalize_model_mappings(Some(&serde_json::to_value(&provider.models).ok()?));
    let slot = match requested_model {
        "gpt-5.5" => Some("gpt_5_5"),
        "gpt-5.4" => Some("gpt_5_4"),
        "gpt-5.4-mini" => Some("gpt_5_4_mini"),
        "gpt-5.3-codex" => Some("gpt_5_3_codex"),
        "gpt-5.2" => Some("gpt_5_2"),
        _ => None,
    }?;

    let mapped = mappings.get(slot).map(|s| s.trim()).unwrap_or("");
    if !mapped.is_empty() {
        return Some(strip_model_suffix(mapped));
    }

    let default = mappings.get("default").map(|s| s.trim()).unwrap_or("");
    if !default.is_empty() {
        return Some(strip_model_suffix(default));
    }

    None
}
```

`decide_provider()` 的顺序:

1. 如果 model 是 `<slug>/<real>`,按现有逻辑路由到 slug provider,并把 real 清洗后写入 `rewritten_model`。
2. 否则取 default provider。
3. 如果 requested model 是 Codex 槽位,映射到 provider 真实模型。
4. 如果不是可识别槽位,保持现状。

### 4.8 proxy forward 最终清洗 `[1m]`

修改:

```text
crates/proxy/src/forward.rs
```

在 adapter prepare 之前或模型改写后,保证最终 body 中的 `model` 不包含内部后缀:

```rust
if let Some(cleaned) = strip_model_suffix_in_json_body(&body_bytes) {
    body_bytes = cleaned;
}
```

注意事项:

- 非 JSON body 不处理,保持透传。
- 没有 `model` 字段不处理。
- `model` 不是字符串不处理。
- 清洗逻辑必须和 catalog 生成共用同一个函数或同一套测试,避免两处行为漂移。

### 4.9 不修改 Responses converter fallback

本轮不修改:

```text
crates/adapters/src/responses/converter.rs
```

尤其不要把 `reasoning_only_emits_reasoning_lifecycle_no_message` 改成注入可见 assistant message。当前合理边界是:

- config/model catalog 修复只解决 Codex 模型能力识别与上游模型路由。
- reasoning-only 用户体验另开问题,另做复现和设计。

## 5. 测试计划

### 5.1 codex_integration 单测

新增或更新测试:

- `apply_with_supports_1m_writes_context_window_and_model_catalog_json`
- `apply_with_supports_1m_writes_valid_catalog_file`
- `apply_without_supports_1m_removes_catalog_key_and_app_owned_file`
- `restore_with_snapshot_restores_original_model_catalog_json`
- `restore_with_snapshot_without_original_catalog_removes_key_and_app_owned_file`
- `restore_without_snapshot_removes_managed_catalog_key_and_app_owned_file`
- `does_not_delete_user_owned_catalog_path`
- `catalog_strips_internal_model_suffix`

### 5.2 proxy 单测

新增或更新测试:

- `openai_slot_model_maps_to_provider_specific_slot`
- `openai_slot_model_falls_back_to_provider_default`
- `slash_route_strips_internal_suffix`
- `default_provider_request_strips_internal_suffix`
- `unknown_model_without_mapping_remains_unchanged`
- `invalid_json_body_is_not_rewritten`

### 5.3 Codex 0.128 实机验证

必须使用原路径:

```bash
/opt/homebrew/bin/codex --version
```

期望:

```text
codex-cli 0.128.0
```

验证旧 key 不足:

```bash
/opt/homebrew/bin/codex debug models -c model_context_window=1000000 \
  | jq '.models[] | select(.slug=="gpt-5.5") | {context_window,max_context_window,effective_context_window_percent}'
```

期望仍为内置窗口,说明旧 key 不能作为 0.128 生效依据。

验证 catalog 生效:

```bash
/opt/homebrew/bin/codex debug models \
  -c 'model_catalog_json="/absolute/path/to/codex-app-transfer-models.json"' \
  | jq '.models[] | select(.slug=="gpt-5.5") | {context_window,max_context_window,effective_context_window_percent}'
```

期望:

```json
{
  "context_window": 1000000,
  "max_context_window": 1000000,
  "effective_context_window_percent": 95
}
```

### 5.4 Rust 校验

至少运行:

```bash
cargo fmt --all -- --check
cargo test -p codex-app-transfer-codex-integration
cargo test -p codex-app-transfer-proxy
cargo test -p codex-app-transfer-registry
```

如变更触及 adapter 或 Tauri handler,再运行:

```bash
cargo test -p codex-app-transfer-adapters
cargo check -p codex-app-transfer-tauri
```

## 6. 验收标准

修复完成必须同时满足:

1. active provider 支持 1M 时,`~/.codex/config.toml` 同时包含 `model_context_window = 1000000` 和 `model_catalog_json = "<app-owned catalog path>"`。
2. `/opt/homebrew/bin/codex debug models` 读取该 catalog 后,`gpt-5.5` 等 Codex 槽位显示 1M 窗口。
3. active provider 不支持 1M 时,两个 key 都被移除,本应用生成的 catalog 文件被删除。
4. restore 后,用户原本的 `model_catalog_json` 能恢复;如果用户原本没有该 key,则 key 不残留。
5. proxy 收到 `gpt-5.5` 等槽位模型时,上游实际收到 provider 真实模型。
6. 上游实际收到的模型名不包含 `[1m]`。
7. reasoning-only completed 流的行为保持不变,不在本修复中注入可见 assistant fallback。
8. 相关 Rust 单测和 CI 通过。

## 7. 建议实施顺序

1. 新增 `model_catalog.rs` 和 `CodexPaths::model_catalog_json`,先写 catalog schema 单测。
2. 扩展 `ApplyConfig` / `ApplyResult`,实现 apply 写入 catalog 和 `model_catalog_json`。
3. 完成 restore / 非 1M 切换的文件清理逻辑。
4. 修改 Tauri admin handler,把 provider name、default model、model mappings 传入 integration 层。
5. 修改 proxy resolver,完成 Codex 槽位模型到 provider 真实模型的映射。
6. 抽出并复用 `[1m]` 清洗函数,补 proxy forward 最终清洗。
7. 用 `/opt/homebrew/bin/codex debug models` 做 0.128 实机验收。
8. 跑 affected Rust tests 和 PR CI。

## 8. 非目标

本方案不处理以下内容:

- 不改 release 流程。
- 不改 provider 预设列表。
- 不引入新的上游模型自动发现机制。
- 不实现 reasoning-only final summary fallback。
- 不修改 `docs/litellm/` 或 `litellm/` 参考目录。

## 9. PR #12 整改推进

### 9.1 当前分支状态

PR #12 已拉回本地分支:

```text
codex/pr-12-codex-128-config-catalog
```

当前 PR 已实现:

- `crates/codex_integration/src/model_catalog.rs`:新增 model catalog 生成逻辑。
- `crates/codex_integration/src/apply.rs`:在 `supports_1m = true` 时写入 `model_context_window` 与 `model_catalog_json`。
- `crates/proxy/src/resolver.rs`:把 Codex 侧模型槽位映射到 provider 真实模型。
- `crates/proxy/src/forward.rs`:转发前清理模型名尾部方括号后缀。
- `crates/adapters/src/responses/converter.rs`:P5 已移出 PR 中新增的 reasoning-only completed fallback message。

已验证:

- `cargo fmt --all -- --check` 通过。
- `cargo test -p codex-app-transfer-adapters -p codex-app-transfer-proxy -p codex-app-transfer-registry -p codex-app-transfer-codex-integration` 在允许本地 TCP listener 的环境下通过。
- `cargo check -p codex-app-transfer` 通过。
- `/opt/homebrew/bin/codex debug models -c model_context_window=1000000` 下,`gpt-5.5` 仍为内置窗口 `272000`。
- `/opt/homebrew/bin/codex debug models -c 'model_catalog_json="<catalog path>"'` 下,当 catalog 声明 `gpt-5.5.context_window = 1000000` 时,Codex 0.128 会把 `gpt-5.5` 显示为 1M。

### 9.2 待修问题清单

- [x] P1:修正 catalog entry 生成方式,避免覆盖 Codex 内置 `gpt-5.5` 的工具、搜索、reasoning summary 等能力字段。
- [x] P2:补齐 catalog 生命周期。切换到非 1M provider 或 restore 时,要清理或恢复本应用写入的 catalog 数据,不能只删除 TOML key。
- [x] P3:让 catalog 与 provider slot mapping 对齐。不能把所有 Codex 槽位无条件声明成同一个 1M default model。
- [x] P4:收窄并复用 `[1m]` 清洗逻辑。只清理内部兼容标记 `[1m]`,避免误删合法模型 ID 的其它尾部方括号内容。
- [x] P5:移出或延后 reasoning-only fallback。该行为会改变 Responses 输出语义,不应混在 model catalog 修复里。

### 9.3 P1 判断:为什么会影响 `gpt-5.5`

作者说“只是把原来的 1M 上下文判断逻辑改为 model catalog”这个方向本身成立,但 PR 当前实现不是只给某个真实 provider model 增加 1M 信息,而是在 catalog 中直接声明了 Codex 侧模型槽位:

```rust
catalog_model("gpt-5.5", provider_name, default_model, context_window)
catalog_model("gpt-5.4", provider_name, default_model, context_window)
catalog_model("gpt-5.4-mini", provider_name, default_model, context_window)
catalog_model("gpt-5.3-codex", provider_name, default_model, context_window)
catalog_model("gpt-5.2", provider_name, default_model, context_window)
```

并且 `context_window` 的计算是:

```rust
let context_window = if supports_1m { 1_000_000 } else { 258_400 };
```

因此,只要 active provider 被判断为 `supports_1m = true`,PR 当前 catalog 就会把上述所有 Codex 侧槽位都声明为 1M。这里的 `gpt-5.5` 不是在表达“OpenAI 原生 GPT-5.5 默认支持 1M”,而是本项目用来给 Codex CLI 暴露的模型槽位名。代理层再把 Codex 发来的 `gpt-5.5` 映射到当前 provider 的真实模型:

```rust
"gpt-5.5" => Some("gpt_5_5")
```

槽位为空时再回退到 provider 的 `default` 模型。

实测确认 Codex 0.128 对 `model_catalog_json` 的处理不是“只覆盖窗口字段”,而是按 `slug` 使用 catalog 中的整条模型记录覆盖内置条目。对同一个 `gpt-5.5`:

- 不带 PR catalog 时:`context_window = 272000`,`apply_patch_tool_type = "freeform"`,`supports_parallel_tool_calls = true`,`supports_search_tool = true`。
- 带 PR catalog 时:`context_window = 1000000`,`apply_patch_tool_type = null`,`supports_parallel_tool_calls = false`,`supports_search_tool = false`。

结论:

- 问题不是 Codex 或上游 GPT-5.5 自己默认支持 1M。
- 问题也不是单纯“判断逻辑改为 model catalog”这个方向错。
- 真正的问题是 PR 当前把 `gpt-5.5` 等 Codex 内置 slug 作为 app catalog entry 重写了,并且 entry 里没有保留 Codex 原本的能力字段。
- 当前项目确实在 active provider 支持 1M 时,把所有 Codex 侧槽位都当作可路由到 1M provider 的别名来声明。这可以作为兼容策略,但必须以“不降级内置能力字段”和“按真实 slot mapping 声明窗口”为前提。

P1 修正方向:

1. 不要从零手写 `gpt-5.5` 等内置模型的完整能力记录。
2. 以 Codex bundled catalog 中的同 slug 条目为模板,只覆盖 `context_window`、`max_context_window`、`effective_context_window_percent`、`display_name`、`description` 等本项目确实需要改的字段。
3. 如果某个 provider slot 没有 1M 能力,该 slot 不应被无条件声明成 1M。

### 9.4 P1 执行记录

已修改 `crates/codex_integration/src/model_catalog.rs`:

- `model_to_json()` 对 Codex 内置槽位 `gpt-5.5`、`gpt-5.4`、`gpt-5.4-mini`、`gpt-5.3-codex`、`gpt-5.2` 使用内置能力模板,再覆盖本项目需要声明的窗口和展示字段。
- 内置槽位保留 `apply_patch_tool_type = "freeform"`、`supports_parallel_tool_calls = true`、`supports_search_tool = true`、`supports_reasoning_summaries = true` 等能力字段。
- 非 Codex 内置 slug 仍走原来的 generic catalog 形状,避免把 OpenAI/Codex 专属能力错误声明给 provider 真实模型。
- 新增 `builtin_slug_catalog_preserves_codex_capabilities` 回归测试,防止后续再把 `gpt-5.5` 的工具、搜索、reasoning summary 能力覆盖成默认空值。

验证结果:

- `cargo fmt --all -- --check`:通过。
- `cargo test -p codex-app-transfer-codex-integration`:通过,34 个测试通过。
- `cargo test -p codex-app-transfer-adapters -p codex-app-transfer-proxy -p codex-app-transfer-registry -p codex-app-transfer-codex-integration`:首次在沙箱内因代理测试需要本地 TCP listener 被拦截;按权限规则重跑后通过。
- `cargo check -p codex-app-transfer`:通过。

### 9.5 P2-P4 执行记录

已修改 catalog 生命周期:

- 当前 PR 将 model catalog 的顶层 `models` 合并进 `~/.codex-app-transfer/config.json`,该文件也是应用主配置。因此 P2 修复不是删除整个 `config.json`,而是通过 `clear_catalog_models()` 只移除本应用写入的顶层 `models` 字段。
- `apply_provider()` 在 `supports_1m = false` 时同时删除 `model_context_window`、`model_catalog_json`,并清理顶层 catalog `models`。
- `restore_codex_state()` 在有 snapshot 和无 snapshot 两条路径下都会清理顶层 catalog `models`;有 snapshot 时仍按 snapshot 恢复用户原本的 `model_catalog_json` key。
- 新增测试覆盖非 1M apply 清理、无 snapshot fallback 清理、restore 恢复用户自定义 `model_catalog_json` key 且不保留本应用 catalog `models`。

已修改 catalog 与 slot mapping 对齐:

- `ApplyConfig` 新增 `model_mappings` 和 `model_capabilities` 输入,`desktop_configure()` 从 active provider 传入 `models` 和 `modelCapabilities`。
- `catalog_models_for_provider()` 对每个 Codex 内置 slug 按 provider 对应 slot 的目标模型生成 display name 和 context window。
- 空 slot 仍记录 default fallback 目标,与 proxy 的 default fallback 路由保持一致;非空 slot 使用自己的目标模型,并按 `[1m]`、`deepseek-v4-*`、`qwen3.6-*`、`modelCapabilities[model].supports1m` 判断是否声明 1M。
- 新增测试确认 `gpt-5.5` 可以映射到非 1M 目标并保持 258400 窗口,同时其它 slot 可以按各自目标声明 1M。

已收窄并复用 `[1m]` 清洗:

- 在 `crates/registry/src/model_alias.rs` 新增 `strip_internal_model_suffix()`、`has_internal_one_m_suffix()`、`openai_model_slot()`。
- catalog、admin handler、proxy resolver、proxy forward 复用 registry 中的同一套规则。
- 清洗逻辑只处理尾部 `[1m]` / `[1M]`,不再删除 `[beta]`、`[1m-preview]` 等其它合法尾部方括号内容。
- proxy resolver 对 `<provider>/<model[1m]>` slash route 和 Codex 槽位映射结果都会清洗内部标记,forward 在最终上游 body 边界再做一次兜底清洗。

验证结果:

- `cargo fmt --all -- --check`:通过。
- `git diff --check`:通过。
- `cargo test -p codex-app-transfer-registry -p codex-app-transfer-codex-integration -p codex-app-transfer-proxy`:普通沙箱首次因代理测试需要本地 TCP listener 被拦截;按权限规则重跑后通过。
- `cargo test -p codex-app-transfer-adapters -p codex-app-transfer-proxy -p codex-app-transfer-registry -p codex-app-transfer-codex-integration`:在允许本地 TCP listener 的环境下通过。
- `cargo check -p codex-app-transfer`:通过。

### 9.6 P5 执行记录

已移出 reasoning-only final summary fallback:

- 删除 `ChatToResponsesConverter` 的 `final_summary_fallback` 字段和 `without_final_summary_fallback()` 开关。
- 删除 close 阶段在 reasoning-only completed 场景下注入 synthetic assistant message 的逻辑。
- 将回归测试改为 `reasoning_only_completed_turn_emits_reasoning_lifecycle_no_message`,明确要求 completed output 只包含 reasoning item,不伪造可见 assistant 文本。
- 移除 fallback 字符串 `上游已结束本轮，但没有返回最终可见文本。Upstream completed this turn without final visible text.` 的所有代码引用。

验证结果:

- `cargo fmt --all -- --check`:通过。
- `cargo test -p codex-app-transfer-adapters`:通过,56 个单元测试和 3 个 streaming 集成测试通过。
- `cargo test -p codex-app-transfer-adapters -p codex-app-transfer-proxy -p codex-app-transfer-registry -p codex-app-transfer-codex-integration`:在允许本地 TCP listener 的环境下通过。
- `cargo check -p codex-app-transfer`:通过。
- `git diff --check`:通过。
