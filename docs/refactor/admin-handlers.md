# `admin/handlers.rs` 拆分推进文档

## 背景

`src-tauri/src/admin/handlers.rs` 当前 5229 行 / 151 函数,所有 `/api/*` 路由 handler 都堆在这一个文件里。
调研报告见对话上下文(归属分布表 + helper 引用统计)。Codex 提了 8 子模块拆分方案,经实际数据验证后修订为本计划。

## 决策(已经定下,不再讨论)

1. **拆分方向沿用 codex 提议**:`handlers/{common, instance合并到common, proxy, settings, update, feedback, desktop, providers}.rs`
2. **`instance` 不独立**(只 13 行 2 个 trivial handler)→ 合进 `common.rs`
3. **`providers` 二级再拆**(53 函数 1500 行,单文件仍超大):
   - `handlers/providers/{mod, crud, test, models, balance, presets}.rs`
4. **不在 `handlers/mod.rs` 做 `pub use` 平铺 re-export** —— routing table 直接用 `handlers::providers::crud::list_providers` 全限定路径,自带域注释,加新 handler 不用改索引
5. **helper 归属规则**:
   - 跨域共享(`err` 引用 104 次、`open_directory`、`current_epoch_secs`)→ `common.rs` `pub(super)`
   - 域内 helper(`provider_test_headers` 等)→ 域内 private fn
6. **分两轮搬,降低单 PR 风险**

## Round 1 范围(本次 PR)

行数小 + 自洽强 + 跨域耦合低 的 5 块,总 ~1430 行:

| 子模块 | 文件 | 函数数 | 估算行数 |
|---|---|---:|---:|
| common(含 instance) | `handlers/common.rs` | 14+2 | ~380 |
| update | `handlers/update.rs` | 14 | ~273 |
| feedback | `handlers/feedback.rs` | 8 | ~258 |
| settings | `handlers/settings.rs` | 17 | ~417 |
| proxy | `handlers/proxy.rs` | 11 | ~110 |

完成后 `handlers.rs` 剩 ~3800 行(只剩 desktop + providers,Round 2 处理)。

### Round 1 哪些函数搬到哪儿(权威清单)

下面所有行号对齐当前 main HEAD `2c685ef` 的 `src-tauri/src/admin/handlers.rs`。

#### `handlers/common.rs`

跨域 helper + 顶层 status/version 类 handler + instance(合并):

- L141 `err`(104 次引用,**必须** `pub(super)`)
- L148 `open_directory`(2 处用,proxy + ?)
- L455 `current_epoch_secs`(2 处用)
- L1901 `read_setting_bool`(被 desktop / settings 共用)
- L2358 `generate_gateway_key_value`(`random_hex` 的 wrapper,跨域)
- L2364 `random_hex`(跨域)
- L2704 `instance_info`(handler)
- L2712 `instance_show_window`(handler)
- L2719 `status`(handler,~54 行)
- L3300 `version`(handler)
- L3890 `_state_typecheck`(lib internal typecheck shim,**别删**)

#### `handlers/update.rs`

升级检查 + 安装包下载/解析 + 平台判断:

- L517-728 一整段:`current_update_platform` / `_for` / `version_parts` / `is_newer_version` / `validate_update_url` / `safe_asset_name` / `asset_filename_from_url` / `file_sha256` / `pick_platform_data` / `allowed_install_extensions` / `pick_windows_installer` / `pick_macos_installer` / `launch_update_installer` 以及 `pick_platform_installer` / `install_command_parts` / `configured_update_url` / `fetch_latest_json` / `check_update_impl` / `download_asset_impl` / `download_update_impl` / `install_after_quit_command_parts`
- L3470 `update_check`(handler)
- L3505 `update_install`(handler)

⚠️ `install_after_quit_command_parts` 与 desktop 的 quit 流程紧耦合,但调用图上是 update 主导的,放 update.rs。

#### `handlers/feedback.rs`

反馈提交 + 节流 + 附件打包:

- L51 `FEEDBACK_WORKER_URL`(const,搬过来 + 模块内 private)
- L124 `FEEDBACK_THROTTLE`(static)
- L126 `feedback_throttle`
- L130 `feedback_worker_url`
- L397 `multipart_text_part`
- L422 `feedback_proxy_tail_content`
- L434 `feedback_proxy_tail_part`
- L462 `feedback_attachments`
- L3577 `submit_feedback`(handler)
- L3581 `submit_feedback_with_body`(internal handler impl)

`active_provider_name`(L403)被 feedback + desktop 都用,我**先放 common.rs**(避免重复)。

#### `handlers/settings.rs`

应用配置文件 / 备份 / 导入导出:

- L2319 `ensure_settings_object`
- L2370 `app_config_dir` / L2374 `app_config_file` / L2378 `app_backup_dir`
- L2382 `system_time_iso_seconds`
- L2387 `default_config_value`
- L2407 `normalize_imported_provider`
- L2458 `normalize_imported_config`
- L2574 `preserve_existing_provider_secrets`
- L2635 `create_config_backup`
- L2670 `list_config_backups`
- L3379 `get_settings`(handler)
- L3388 `save_settings`(handler)
- L3408 `create_backup`(handler)
- L3415 `list_backups`(handler)
- L3422 `export_config`(handler)
- L3432 `import_config`(handler)

#### `handlers/proxy.rs`

代理生命周期 + 网关密钥 + 端口:

- L1886 `read_proxy_port`
- L1894 `read_gateway_key`
- L1908 `ensure_gateway_key`
- L2177 `start_proxy_if_needed`(被 desktop / startup 调用,**必须 `pub(super)`** 让 desktop.rs 能用)
- L3311 `start_proxy`(handler)
- L3330 `stop_proxy`(handler)
- L3335 `proxy_status`(handler)
- L3351 `proxy_logs`(handler)
- L3355 `proxy_logs_clear`(handler)
- L3360 `proxy_logs_open_dir`(handler)

## 实施步骤(Round 1)

1. 起 worktree:`git worktree add -b refactor/admin-handlers-round-1 /tmp/cas-handlers-r1 origin/main`
2. 在新 worktree 创建目录:`src-tauri/src/admin/handlers/{common,update,feedback,settings,proxy}.rs`
3. 创建 `src-tauri/src/admin/handlers.rs` 替换为 `src-tauri/src/admin/handlers/mod.rs`(声明子模块 + re-export 仍留在 handlers.rs 的剩余函数)
   - 实际操作:`mv handlers.rs handlers/_remaining.rs`,然后写 `handlers/mod.rs` 声明所有子模块 + `pub use _remaining::*`(让 admin/mod.rs routing table 暂时仍能引用所有 handler)
   - 或者更简洁:`handlers.rs` 保留作为 mod 文件,**手动删除**已搬走的函数
4. **逐个子模块迁移**(顺序:common → proxy → feedback → settings → update,从依赖少的开始):
   - 在子模块声明所需 `use` 语句(只 import 域内用到的,不复制全部)
   - 把函数体复制到子模块
   - 把可见性调整为 `pub(super)`(供 admin/mod.rs 路由表 + 其他子模块用)
   - 在 `handlers.rs`(或 `_remaining.rs`)删除原函数
   - 跑 `cargo check -p codex-app-transfer` 确保编译通过
5. 修改 `admin/mod.rs` 路由表:`handlers::xxx` → `handlers::子模块::xxx`(全限定路径)
6. **不能改函数签名 / 行为** —— 纯文本搬家
7. 全 workspace `cargo test` + `cargo fmt --all` + `cargo check --workspace`
8. commit + push + PR + CI + 本地打包

## Round 1 验证清单

- [ ] `cargo check --workspace` 全过
- [ ] `cargo test --workspace --exclude codex-app-transfer` 全过
- [ ] `cargo test -p codex-app-transfer` 全过(Tauri 部分)
- [ ] `cargo fmt --all -- --check` 无 diff
- [ ] `git diff --stat` 显示**只动 `src-tauri/src/admin/`**(handlers.rs 缩水 + 新增 5 个文件 + admin/mod.rs 路由表更新)
- [ ] 函数总数不变(151)
- [ ] 公开 API 签名不变(`pub async fn xxx(...)` 一一对齐)
- [ ] 本地 `make mac-app` 启动后所有 `/api/*` 路由响应正常(目测启动 + 切 provider 不报错就行)

## Round 1 风险点

1. **跨域 helper 搬错位置** → 编译错误 `cannot find function`,fix:加进 common.rs
2. **forgot `pub(super)`** → admin/mod.rs 路由表 import 失败,fix:对所有 handler + 跨域 helper 加 `pub(super)` 或 `pub`
3. **`use` 语句去重不彻底** → dead_code warning,fix:只 import 子模块实际用到的
4. **`active_provider_name` 跨 feedback/desktop 共享** → 放 common.rs(已决定)
5. **`hide_console_window` cfg-gated 双定义** → 这是 desktop 域的,**Round 1 不动**(留 handlers.rs 剩余部分)
6. **`switch_provider_and_sync` / `auto_apply_on_startup_if_enabled`** → desktop 域,Round 1 不动(留 handlers.rs)

## Round 2 范围(下个 PR,本计划暂不做)

| 子模块 | 文件 | 函数数 | 估算行数 |
|---|---|---:|---:|
| desktop | `handlers/desktop.rs` | 30 | ~668 |
| providers/crud | `handlers/providers/crud.rs` | ~12 | ~400 |
| providers/test | `handlers/providers/test.rs` | ~10 | ~470 |
| providers/models | `handlers/providers/models.rs` | ~8 | ~330 |
| providers/balance | `handlers/providers/balance.rs` | ~6 | ~230 |
| providers/presets | `handlers/providers/presets.rs` | ~3 | ~80 |

Round 2 完成后 `handlers.rs` 应可**删除**(剩余的 helper 都进 `common.rs`),只留 `handlers/mod.rs` 作为模块声明文件。

## 命名一致性约定

- 子模块文件 → snake_case 单词(`update.rs` 不是 `updates.rs`)
- handler 函数名 **不变**(避免影响 admin/mod.rs 路由表)
- helper 函数可重命名,但 Round 1 一律 **不动名字**(纯搬家)

## 操作禁忌

1. ❌ 改函数签名 / 行为(包括加 `Result` 包装、改 error type、调整参数顺序)
2. ❌ 把同一类逻辑的函数拆到多文件(如 `update_check` + `update_check_impl` 不要分家)
3. ❌ 在 `handlers/mod.rs` 做 `pub use 子模块::*` 平铺 re-export(增加间接层 + 路由表看不出域)
4. ❌ 一次性搬完 5 块再编译(必须每搬一块就 cargo check 一次,出错好定位)

## 当前进度

### Round 1(已完成 2026-05-08)

- [x] 计划文档建立
- [x] worktree 创建 / common / proxy / feedback / settings / update 子模块迁移
- [x] admin/mod.rs 路由表全限定路径
- [x] cargo test / fmt / check 全过(Tauri 31 + workspace 250+)
- [x] PR #54 创建 + CI 全绿 + 本地打包 + 用户手测通过
- [x] squash merge → main `f83d1c2`

实际产出:common 172 / proxy 125 / feedback 360 / settings 431 / update 594 / mod 18 / _legacy 3738。

### Round 2(进行中)

- [x] 基于 main `f83d1c2` 起 worktree(`refactor/admin-handlers-round-2`)
- [ ] `handlers/desktop.rs`(~668 行,30 函数)
- [ ] `handlers/providers/{mod,crud,test,models,balance,presets}.rs` 二级拆分(~1500 行,53 函数)
- [ ] `_legacy.rs` 删除(剩余应全部归 desktop / providers / common)
- [ ] `_legacy.rs` 的 test mod 散到各域
- [ ] `handlers/mod.rs` 删除 `pub use _legacy::*`(所有 handler 走全限定路径)
- [ ] admin/mod.rs 路由表更新
- [ ] cargo test / fmt / check 全过
- [ ] PR + CI + 本地打包 + 用户手测
- [ ] squash merge

### Round 2 风险点(新增)

7. **`hide_console_window` cfg-gated 双定义** → desktop.rs 必须一起搬,**别 `#[cfg]` 跨文件**
8. **`switch_provider_and_sync` / `auto_apply_on_startup_if_enabled` / `restore_codex_if_enabled`** → 这三个被 admin/mod.rs / lib.rs startup 路径调用,**必须保持 `pub`** (不只 `pub(super)`)
9. **providers 二级拆分时,`provider_test_*` 系列(test.rs)和 `model_endpoint_*` / `extract_model_ids` 系列(models.rs)有交叉调用** → 谁调谁的 helper 要明确(我倾向 helper 留在被调用最多的域,跨域用 `super::xxx::yyy`)
10. **`_legacy.rs` 的 test mod 用 `super::*` import 大量被搬走的函数** → 散到各域 `#[cfg(test)] mod tests` 时要重 import,可能要拆几十个测试到 5-6 个文件
- [ ] squash merge

文档创建时间:2026-05-08
