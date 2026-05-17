# Phase 2 起 release pipeline 全部走 GitHub Actions (.github/workflows/release.yml)。
# 本地 Makefile 只保留两个 target:
#   mac-app  - 本地自测出 .app
#   clean    - 清理 build/dist/release/.tmp
# 三平台 release 触发: gh workflow run release.yml -f version=2.0.1
# 详见 docs/build.md。

.PHONY: help mac-app clean

help:
	@echo "Targets:"
	@echo "  mac-app   Build Tauri unsigned macOS .app into dist/mac/ (本地自测用)"
	@echo "  clean     Remove build/, dist/, release/, .release-signing/, .tmp/"
	@echo ""
	@echo "Release: 三平台 release 由 GitHub Actions 出, 不再走本地 Makefile."
	@echo "         手动触发: gh workflow run release.yml -f version=<x.y.z>"
	@echo "         tag 触发: git tag v<x.y.z> && git push --tags"

mac-app:
	CARGO_TARGET_DIR=target cargo tauri build --bundles app
	mkdir -p dist/mac
	rm -rf "dist/mac/Codex App Transfer.app"
	cp -R "src-tauri/target/release/bundle/macos/Codex App Transfer.app" "dist/mac/Codex App Transfer.app"
	@echo ""
	@echo "✓ Built: dist/mac/Codex App Transfer.app"

clean:
	rm -rf build dist release .release-signing .tmp
