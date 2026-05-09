#!/usr/bin/env bash
set -euo pipefail

mode="preflight"
allow_real_desktop_write="false"
output_directory=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      mode="${2:-}"
      shift 2
      ;;
    --allow-real-desktop-write)
      allow_real_desktop_write="true"
      shift
      ;;
    --output-directory)
      output_directory="${2:-}"
      shift 2
      ;;
    --help|-h)
      cat <<'EOF'
Usage:
  scripts/macos/run-real-desktop-smoke.sh [--mode preflight|run] [--allow-real-desktop-write] [--output-directory PATH]

Default mode is read-only preflight. Real Desktop writes require:
  --mode run --allow-real-desktop-write
EOF
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [[ "$mode" != "preflight" && "$mode" != "run" ]]; then
  echo "mode must be preflight or run" >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

if [[ -z "$output_directory" ]]; then
  output_directory="$repo_root/target/real-desktop-smoke"
fi
mkdir -p "$output_directory"

timestamp="$(date +"%Y%m%d-%H%M%S")"
evidence_path="$output_directory/macos-real-desktop-smoke-evidence.md"
log_path="$output_directory/macos-real-desktop-smoke-$timestamp.log"
log_field="$(basename "$log_path")"
platform="$(uname -s 2>/dev/null || echo unknown)"
arch="$(uname -m 2>/dev/null || echo unknown)"
config_library_path="${HOME:-}/Library/Application Support/Claude-3p/configLibrary"
config_library_exists="False"
if [[ -d "$config_library_path" ]]; then
  config_library_exists="True"
fi

write_evidence() {
  local result="$1"
  local command_text="$2"
  local exit_code="$3"

  cat > "$evidence_path" <<EOF
# macOS Real Desktop Smoke Evidence

## Result

$result

fingerprint: desktop.real_macos_local_config_smoke
description: macOS real Claude Desktop local config smoke
test_name: macos_real_desktop_local_config_smoke
mode: $mode
command: $command_text
exit_code: $exit_code
log: $log_field
platform: $platform
arch: $arch
configLibraryPath: $config_library_path

## Preflight

~~~text
platform=$platform
arch=$arch
configLibraryExists=$config_library_exists
configLibraryPath=$config_library_path
~~~

## Pass Criteria

- test output ends with test result: ok
- configLibrary is restored after the test
- loopback gateway smoke passes
- safe route entries use claude-* aliases
- Default is absent
- raw upstream model routes are absent

## Notes

- preflight is read-only and must not be treated as pass evidence.
- run requires --allow-real-desktop-write and sets CCDS_ALLOW_REAL_DESKTOP_WRITE=1 only for the test process.

## Readiness Markers

- macOS real Claude Desktop local config smoke
- configLibrary
- safe route
EOF
}

command_text="not-run"
exit_code="not-run"
result="Preflight"

if [[ "$mode" == "run" && "$allow_real_desktop_write" != "true" ]]; then
  echo "run mode requires --allow-real-desktop-write" >&2
  exit 2
fi

if [[ "$platform" != "Darwin" ]]; then
  result="UnsupportedPlatform"
  write_evidence "$result" "$command_text" "$exit_code"
  echo "result=$result"
  echo "evidence=$evidence_path"
  echo "platform=$platform"
  echo "configLibraryExists=$config_library_exists"
  exit 0
fi

if [[ "$mode" == "run" ]]; then
  command_text="cargo test -p cc-desktop-switch --lib macos_real_desktop_local_config_smoke -- --ignored --nocapture"
  set +e
  CCDS_ALLOW_REAL_DESKTOP_WRITE=1 cargo test -p cc-desktop-switch --lib macos_real_desktop_local_config_smoke -- --ignored --nocapture >"$log_path" 2>&1
  exit_code="$?"
  set -e
  if [[ "$exit_code" == "0" ]]; then
    result="Pass"
  else
    result="Fail"
  fi
fi

write_evidence "$result" "$command_text" "$exit_code"

echo "result=$result"
echo "evidence=$evidence_path"
echo "platform=$platform"
echo "arch=$arch"
echo "configLibraryExists=$config_library_exists"

if [[ "$mode" == "run" && "$exit_code" != "0" ]]; then
  exit "$exit_code"
fi
