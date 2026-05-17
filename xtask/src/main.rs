//! xtask — 仓库内部工具集 (Phase 3 起替代 scripts/*.py).
//!
//! 子命令:
//! - `gen-fixtures` — 生成 `tests/replay/fixtures/registry/` 下 4 份 JSON
//!   fixture (替代 `scripts/gen_registry_fixtures.py`)
//! - `release-bundle` — Commit D 启用,RSA-3072 签名 + latest.json (替代
//!   `scripts/release_assets.py`)
//!
//! 用法: `cargo run -p xtask --release -- <sub> [args]`
//! CI 反向 diff: `cargo run -p xtask -- gen-fixtures && \
//!                git diff --exit-code -- tests/replay/fixtures/registry/`

use anyhow::Result;
use clap::{Parser, Subcommand};

mod gen_fixtures;
mod release_bundle;

#[derive(Parser)]
#[command(name = "xtask", about = "Codex App Transfer 仓库内部工具集", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 生成 registry fixture JSON (字节级与 commit golden 一致)
    GenFixtures(gen_fixtures::Args),
    /// 签 release artifact + 出 latest.json (替代 scripts/release_assets.py)
    ReleaseBundle(release_bundle::Args),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::GenFixtures(args) => gen_fixtures::run(args),
        Command::ReleaseBundle(args) => release_bundle::run(args),
    }
}
