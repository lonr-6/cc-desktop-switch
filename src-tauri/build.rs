fn main() {
    // **关键**:`tauri_build::build()` 默认不把 ../frontend 加进 cargo rerun-if-changed,
    // 导致前端代码改了 binary 不重 build。实测 2026-05-10 用户截图显示前端协议名 fallback
    // 错误显示 "OpenAI Chat" 而不是 "Gemini Native",root cause 就是 binary 用的是
    // 5月9日的旧版本(cargo 没探测到 frontend 改动)。
    //
    // 显式声明 frontend 改动 + presets_data.json(embed 进 binary)→ 触发 rerun build。
    println!("cargo:rerun-if-changed=../frontend");
    println!("cargo:rerun-if-changed=../crates/registry/src/presets_data.json");

    // 让 updateUrl 默认值“跟随当前发布仓库”（任务 1）。
    // - CI release 里通过 GITHUB_REPOSITORY 注入真实 owner/repo，binary 里 baked 的
    //   默认 latest.json URL 就指向该仓库的 releases。
    // - 本地 dev / 普通 cargo build 没有该 env 时，fallback 到 Cmochance（统一为官方源）。
    // - 这样 fork 的人只要复用同样的 release workflow + xtask，就能自动得到正确的更新源。
    let repo = std::env::var("CODEX_APP_TRANSFER_REPO")
        .unwrap_or_else(|_| "Cmochance/codex-app-transfer".to_string());
    let update_url = format!(
        "https://github.com/{}/releases/latest/download/latest.json",
        repo
    );
    println!(
        "cargo:rustc-env=CODEX_APP_TRANSFER_DEFAULT_UPDATE_URL={}",
        update_url
    );
    println!("cargo:rerun-if-env-changed=CODEX_APP_TRANSFER_REPO");

    tauri_build::build()
}
