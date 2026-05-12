use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();

    match args.as_slice() {
        [command, flag] if command == "verify" && flag == "--all" => run_all(),
        [command, flag, stage] if command == "verify" && flag == "--stage" => {
            println!("verify stage: {stage}");
            match stage.as_str() {
                "apply-flow" => run("cargo", &["test", "-p", "cc-desktop-switch", "apply_flow"]),
                "app-shell" => run("cargo", &["check", "-p", "cc-desktop-switch"]),
                "config" => run("cargo", &["test", "-p", "cc-desktop-switch", "config"]),
                "config-backup" => run(
                    "cargo",
                    &["test", "-p", "cc-desktop-switch", "config_backup"],
                ),
                "desktop" => run("cargo", &["test", "-p", "cc-desktop-switch", "desktop"]),
                "desktop-writer" => run(
                    "cargo",
                    &["test", "-p", "cc-desktop-switch", "desktop_writer"],
                ),
                "desktop-config" => run(
                    "cargo",
                    &["test", "-p", "cc-desktop-switch", "desktop_config"],
                ),
                "diagnostics" => run("cargo", &["test", "-p", "cc-desktop-switch", "diagnostics"]),
                "file-picker" => run("cargo", &["check", "-p", "cc-desktop-switch"]),
                "gateway" => run("cargo", &["test", "-p", "cc-desktop-switch", "gateway"]),
                "gateway-lifecycle" => run(
                    "cargo",
                    &["test", "-p", "cc-desktop-switch", "gateway_lifecycle"],
                ),
                "model-catalog" => run(
                    "cargo",
                    &["test", "-p", "cc-desktop-switch", "model_catalog"],
                ),
                "model-mapping" => run(
                    "cargo",
                    &["test", "-p", "cc-desktop-switch", "model_mapping"],
                ),
                "provider-parity" => run(
                    "cargo",
                    &["test", "-p", "cc-desktop-switch", "provider_parity"],
                ),
                "provider-import" => run(
                    "cargo",
                    &["test", "-p", "cc-desktop-switch", "provider_import"],
                ),
                "provider-preset" => run(
                    "cargo",
                    &["test", "-p", "cc-desktop-switch", "provider_preset"],
                ),
                "provider-service" => run("cargo", &["test", "-p", "cc-desktop-switch", "state"]),
                "release" => run(
                    "cargo",
                    &["test", "-p", "cc-desktop-switch", "release_gate"],
                ),
                "rc-readiness" => run_rc_readiness(),
                "ui-spike" => {
                    println!("required: cargo fmt --all -- --check");
                    println!("required: cargo test --workspace");
                    println!("required: cd ui && trunk build --release");
                    println!("required: cargo tauri build");
                    ExitCode::SUCCESS
                }
                _ => {
                    eprintln!("unknown verify stage: {stage}");
                    ExitCode::from(2)
                }
            }
        }
        _ => {
            eprintln!("usage: cargo xtask verify --stage <stage> | cargo xtask verify --all");
            ExitCode::from(2)
        }
    }
}

fn run_rc_readiness() -> ExitCode {
    let root = match env::current_dir() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("failed to determine current directory: {error}");
            return ExitCode::from(1);
        }
    };
    let checks = rc_readiness_checks(&root);

    println!("RC1 readiness audit");
    println!("worktree: {}", root.display());
    println!();
    println!("Prompt-to-artifact checklist:");
    let mut failed = 0usize;
    for check in &checks {
        let status = if check.passed { "PASS" } else { "MISSING" };
        println!("- [{status}] {}", check.requirement);
        println!("  evidence: {}", check.evidence);
        if !check.passed {
            failed += 1;
            println!("  next: {}", check.next);
        }
    }

    println!();
    if failed == 0 {
        println!("RC1 readiness audit passed.");
        ExitCode::SUCCESS
    } else {
        println!("RC1 readiness audit incomplete: {failed} missing requirement(s).");
        ExitCode::from(1)
    }
}

struct RcReadinessCheck {
    requirement: &'static str,
    passed: bool,
    evidence: String,
    next: &'static str,
}

fn rc_readiness_checks(root: &Path) -> Vec<RcReadinessCheck> {
    vec![
        check_paths(
            "Cargo workspace / src-tauri / ui / xtask structure exists",
            root,
            &[
                "Cargo.toml",
                "src-tauri/Cargo.toml",
                "src-tauri/src/lib.rs",
                "ui/Cargo.toml",
                "ui/Trunk.toml",
                "xtask/Cargo.toml",
                "xtask/src/main.rs",
            ],
            "Restore the missing workspace skeleton files before continuing RC verification.",
        ),
        check_paths(
            "Pure Rust UI surface exists without hand-written JS in ui/src",
            root,
            &["ui/src/main.rs", "ui/src/app.rs"],
            "Restore Leptos UI source files and keep product UI logic in Rust.",
        ),
        check_file_contains(
            "Tauri app identity, Windows installer inheritance, and tray icon binding match stable-line continuity",
            &root.join("src-tauri").join("tauri.conf.json"),
            &[
                "\"identifier\": \"io.github.lonr6.ccdesktopswitch\"",
                "\"installMode\": \"perMachine\"",
                "\"template\": \"../windows/nsis-installer.nsi\"",
                "\"installerHooks\": \"../windows/nsis-hooks.nsh\"",
            ],
            "Restore the stable bundle identifier and NSIS inheritance hooks so Windows upgrades can reuse the old install location.",
        ),
        check_file_contains(
            "Tauri tray uses a stable id, tooltip, and bundled default icon",
            &root.join("src-tauri").join("src").join("lib.rs"),
            &[
                "TrayIconBuilder::with_id(\"cc-desktop-switch\")",
                ".tooltip(\"CC Desktop Switch\")",
                "app.default_window_icon().cloned()",
            ],
            "Bind the tray icon to the bundled application icon instead of relying on platform defaults.",
        ),
        RcReadinessCheck {
            requirement: "No hand-written JavaScript business logic under ui/src",
            passed: !contains_extension(&root.join("ui").join("src"), "js"),
            evidence: "ui/src recursively checked for .js files".to_owned(),
            next: "Move UI behavior into Rust/Leptos and remove hand-written JS business logic.",
        },
        check_file_contains(
            "ModelCatalog keeps Default out of runtime and rejects unmapped routes",
            &root.join("src-tauri").join("src").join("model_catalog.rs"),
            &[
                "default_mapping_is_not_desktop_visible_or_resolvable",
                "unmapped_route_is_rejected_without_default_fallback",
                "explicit_raw_route_id_is_rejected",
            ],
            "Restore ModelCatalog boundary tests for Default suppression, unmapped 400 behavior, and raw route rejection.",
        ),
        check_file_contains(
            "Apply flow cannot report applied unless write/readback passes",
            &root.join("src-tauri").join("src").join("desktop.rs"),
            &[
                "dry_run_never_reports_applied_success",
                "readback_mismatch_blocks_apply_success",
            ],
            "Restore Apply/readback tests that block false success.",
        ),
        check_file_contains(
            "Release gate rejects macOS x64 gaps and invalid metadata/hash/signature content",
            &root.join("src-tauri").join("src").join("release_gate.rs"),
            &[
                "macos-x64-pkg",
                "release_directory_rejects_latest_json_sha256_mismatch",
                "release_directory_rejects_signature_mismatch_and_invalid_signature",
                "release_directory_verifies_powershell_manifest_signatures",
            ],
            "Restore release gate tests for macOS x64, sha256 mismatch, signature mismatch, and PowerShell compatibility.",
        ),
        check_file_contains(
            "macOS platform smoke workflow emits reusable evidence artifacts",
            &root
                .join(".github")
                .join("workflows")
                .join("rust-mainline-platform-smoke.yml"),
            &[
                "platform-smoke-evidence.md",
                "fingerprint: platform.macos_arm64_x64_smoke_path",
                "macos-14",
                "macos-15-intel",
                "actions/upload-artifact@v4",
            ],
            "Restore the macOS platform smoke workflow evidence artifact so a successful run can be archived in handoff.",
        ),
        check_file_contains(
            "macOS platform smoke workflow verifies runner architecture, build gates, and bundle artifacts",
            &root
                .join(".github")
                .join("workflows")
                .join("rust-mainline-platform-smoke.yml"),
            &[
                "workflow_dispatch",
                "push:",
                "codex/**",
                "expected_uname: arm64",
                "expected_uname: x86_64",
                "uname -m",
                "cargo fmt --all -- --check",
                "cargo test --workspace",
                "cargo clippy --workspace --all-targets -- -D warnings",
                "trunk build --release",
                "cargo tauri build",
                "plutil -lint",
                "hdiutil verify",
                "pkgbuild --install-location",
                "pkgutil --expand",
                "rust-mainline-macos-${{ matrix.arch }}",
                "retention-days: 7",
            ],
            "Restore the macOS workflow architecture checks, build gates, bundle smoke, and retained evidence artifacts.",
        ),
        check_file_contains(
            "macOS platform smoke workflow runs real Desktop local config smoke and uploads its evidence",
            &root
                .join(".github")
                .join("workflows")
                .join("rust-mainline-platform-smoke.yml"),
            &[
                "Run macOS real Desktop local config smoke",
                "scripts/macos/run-real-desktop-smoke.sh --mode run --allow-real-desktop-write",
                "REAL_SMOKE_HOME",
                "real-desktop-smoke",
                "macos-real-desktop-smoke-evidence.md",
                "macos-real-desktop-smoke-*.log",
            ],
            "Restore the macOS real Desktop smoke workflow step and include its evidence/log files in retained artifacts.",
        ),
        check_file_contains(
            "macOS platform smoke evidence collector validates both workflow artifacts before handoff",
            &root
                .join("scripts")
                .join("macos")
                .join("Collect-PlatformSmokeEvidence.ps1"),
            &[
                "platform-smoke-evidence.md",
                "platform.macos_arm64_x64_smoke_path",
                "platform: macOS arm64",
                "platform: macOS x64",
                "runner: macos-14",
                "runner: macos-15-intel",
                "actual_uname: arm64",
                "actual_uname: x86_64",
                "## Result",
                "Pass",
                "OutputPath",
            ],
            "Restore the macOS platform evidence collector so downloaded workflow artifacts can be checked before writing handoff evidence.",
        ),
        check_file_contains(
            "Windows real Desktop smoke wrapper defaults to evidence preflight and explicit write opt-in",
            &root
                .join("scripts")
                .join("windows")
                .join("run-real-desktop-smoke.ps1"),
            &[
                "desktop.real_windows_local_config_smoke",
                "windows_real_desktop_local_config_smoke",
                "-AllowRealDesktopWrite",
                "CCDS_ALLOW_REAL_DESKTOP_WRITE",
                "Readiness Markers",
            ],
            "Restore the Windows real Desktop smoke wrapper so real runs can produce evidence without default writes.",
        ),
        check_file_contains(
            "Windows real Desktop smoke evidence collector validates run evidence before handoff",
            &root
                .join("scripts")
                .join("windows")
                .join("Collect-RealDesktopSmokeEvidence.ps1"),
            &[
                "windows-real-desktop-smoke-evidence.md",
                "desktop.real_windows_local_config_smoke",
                "windows_real_desktop_local_config_smoke",
                "mode: run",
                "exit_code: 0",
                "loopback gateway",
                "restored",
                "test result: ok",
                "## Result",
                "Pass",
                "OutputPath",
            ],
            "Restore the Windows real Desktop smoke evidence collector so run evidence can be checked before writing handoff evidence.",
        ),
        check_file_contains(
            "macOS real Desktop smoke wrapper defaults to evidence preflight and explicit write opt-in",
            &root
                .join("scripts")
                .join("macos")
                .join("run-real-desktop-smoke.sh"),
            &[
                "desktop.real_macos_local_config_smoke",
                "--allow-real-desktop-write",
                "UnsupportedPlatform",
                "CCDS_ALLOW_REAL_DESKTOP_WRITE=1",
            ],
            "Restore the macOS real Desktop smoke wrapper so real runs can produce evidence without default writes.",
        ),
        check_file_contains(
            "macOS real Desktop smoke evidence collector validates run evidence before handoff",
            &root
                .join("scripts")
                .join("macos")
                .join("Collect-RealDesktopSmokeEvidence.ps1"),
            &[
                "macos-real-desktop-smoke-evidence.md",
                "desktop.real_macos_local_config_smoke",
                "macos_real_desktop_local_config_smoke",
                "platform: Darwin",
                "mode: run",
                "exit_code: 0",
                "configLibrary",
                "safe route",
                "test result: ok",
                "## Result",
                "Pass",
                "OutputPath",
            ],
            "Restore the macOS real Desktop smoke evidence collector so run evidence can be checked before writing handoff evidence.",
        ),
        check_file_contains(
            "Windows packaged app smoke evidence exists",
            &root
                .join("project-docs")
                .join("handoff")
                .join("2026-05-09-p43-windows-packaged-app-smoke-rerun-summary.md"),
            &[
                "## Result",
                "Pass.",
                "second launch exited",
                "close request hid",
            ],
            "Rerun packaged app smoke and write a handoff with result evidence.",
        ),
        check_handoff_contains(
            "Windows real Claude Desktop local config smoke passed with backup/readback/gateway/restore evidence",
            root,
            &[
                "## Result\n\nPass",
                "fingerprint: desktop.real_windows_local_config_smoke",
                "test_name: windows_real_desktop_local_config_smoke",
                "windows_real_desktop_local_config_smoke",
                "loopback gateway",
                "restored",
                "evidence:",
                "log:",
            ],
            "Run P36 cleanup with explicit approval or use an unmanaged profile, rerun the opt-in real smoke, and record pass evidence.",
        ),
        check_handoff_contains(
            "macOS arm64 and macOS x64 build/smoke workflow evidence exists",
            root,
            &[
                "## Result\n\nPass",
                "platform.macos_arm64_x64_smoke_path",
                "macos-14",
                "macos-15-intel",
                "workflow_run_arm64:",
                "workflow_run_x64:",
                "artifact_arm64: rust-mainline-macos-arm64",
                "artifact_x64: rust-mainline-macos-x64",
            ],
            "Run the non-publishing macOS platform smoke workflow and record both runner results.",
        ),
        check_handoff_contains(
            "macOS real Claude Desktop local config smoke passed",
            root,
            &[
                "## Result\n\nPass",
                "fingerprint: desktop.real_macos_local_config_smoke",
                "test_name: macos_real_desktop_local_config_smoke",
                "platform: Darwin",
                "macOS real Claude Desktop local config smoke",
                "configLibrary",
                "safe route",
                "evidence:",
                "log:",
            ],
            "Run real Claude Desktop local config smoke on macOS and record readback evidence.",
        ),
    ]
}

fn check_paths(
    requirement: &'static str,
    root: &Path,
    relative_paths: &[&str],
    next: &'static str,
) -> RcReadinessCheck {
    let missing = relative_paths
        .iter()
        .filter(|relative| !root.join(relative).exists())
        .copied()
        .collect::<Vec<_>>();
    RcReadinessCheck {
        requirement,
        passed: missing.is_empty(),
        evidence: if missing.is_empty() {
            format!("found {}", relative_paths.join(", "))
        } else {
            format!("missing {}", missing.join(", "))
        },
        next,
    }
}

fn check_file_contains(
    requirement: &'static str,
    path: &Path,
    needles: &[&str],
    next: &'static str,
) -> RcReadinessCheck {
    match fs::read_to_string(path) {
        Ok(content) => {
            let missing = needles
                .iter()
                .filter(|needle| !content.contains(**needle))
                .copied()
                .collect::<Vec<_>>();
            RcReadinessCheck {
                requirement,
                passed: missing.is_empty(),
                evidence: if missing.is_empty() {
                    format!("{} contains {}", path.display(), needles.join(", "))
                } else {
                    format!("{} missing {}", path.display(), missing.join(", "))
                },
                next,
            }
        }
        Err(error) => RcReadinessCheck {
            requirement,
            passed: false,
            evidence: format!("{} could not be read: {error}", path.display()),
            next,
        },
    }
}

fn check_handoff_contains(
    requirement: &'static str,
    root: &Path,
    needles: &[&str],
    next: &'static str,
) -> RcReadinessCheck {
    let handoff_dir = root.join("project-docs").join("handoff");
    let matches = find_markdown_files_containing(&handoff_dir, needles);
    RcReadinessCheck {
        requirement,
        passed: !matches.is_empty(),
        evidence: if matches.is_empty() {
            format!(
                "no handoff under {} contains all of: {}",
                handoff_dir.display(),
                display_needles(needles)
            )
        } else {
            format!(
                "matched handoff evidence: {}",
                matches
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
        next,
    }
}

fn find_markdown_files_containing(dir: &Path, needles: &[&str]) -> Vec<PathBuf> {
    let mut matches = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return matches;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if needles.iter().all(|needle| content.contains(needle)) {
            matches.push(path);
        }
    }
    matches
}

fn display_needles(needles: &[&str]) -> String {
    needles
        .iter()
        .map(|needle| needle.replace('\n', "\\n"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn contains_extension(dir: &Path, extension: &str) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if contains_extension(&path, extension) {
                return true;
            }
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
            return true;
        }
    }
    false
}

fn run_all() -> ExitCode {
    for (program, args, cwd) in [
        ("cargo", vec!["fmt", "--all", "--", "--check"], None),
        ("cargo", vec!["test", "--workspace"], None),
        (
            "cargo",
            vec![
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            None,
        ),
        ("trunk", vec!["build", "--release"], Some("ui")),
        ("cargo", vec!["tauri", "build"], None),
    ] {
        let status = run_in(program, &args, cwd);
        if status != ExitCode::SUCCESS {
            return status;
        }
    }

    ExitCode::SUCCESS
}

fn run(program: &str, args: &[&str]) -> ExitCode {
    run_in(program, args, None)
}

fn run_in(program: &str, args: &[&str], cwd: Option<&str>) -> ExitCode {
    println!("running: {program} {}", args.join(" "));
    let mut command = Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    match command.status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(error) => {
            eprintln!("failed to run {program}: {error}");
            ExitCode::from(1)
        }
    }
}
