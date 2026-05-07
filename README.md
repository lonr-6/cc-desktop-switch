# CC Desktop Switch

<p align="center">
  <a href="README.md">English</a> |
  <a href="README.zh-CN.md">简体中文</a> |
  <a href="README.ja.md">日本語</a> |
  <a href="docs/CHANGELOG.md">Changelog</a>
</p>

<p align="center">
  <a href="https://github.com/lonr-6/cc-desktop-switch/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/lonr-6/cc-desktop-switch?style=social"></a>
  <a href="LICENSE.txt"><img alt="License" src="https://img.shields.io/github/license/lonr-6/cc-desktop-switch"></a>
  <a href="https://www.python.org/"><img alt="Python" src="https://img.shields.io/badge/Python-3.11%2B-blue?logo=python"></a>
  <a href="https://github.com/lonr-6/cc-desktop-switch/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/lonr-6/cc-desktop-switch/total?label=downloads"></a>
</p>

CC Desktop Switch is a lightweight desktop app for the official Claude Desktop client. It lets you manage third-party Anthropic-compatible API providers such as DeepSeek, Kimi, Zhipu GLM, Alibaba Cloud Bailian, and Xiaomi MiMo, then apply the right Claude Desktop 3P configuration with one click.

This project is focused on Claude Desktop on Windows and macOS. It is different from CLI-oriented tools such as `farion1231/cc-switch`: the goal here is to give regular desktop users a simple UI for provider setup, model mapping, health checks, and local gateway compatibility.

Since v1.0.18, Claude Desktop is configured to call the local CC Desktop Switch gateway at `127.0.0.1`. Keep CC Desktop Switch running in the background when using third-party providers. Closing the window keeps the app available in the tray on Windows or hidden in the Dock app lifecycle on macOS.

## Preview

<table>
  <tr>
    <td width="50%">
      <img src="docs/promo/screenshots/readme-provider-list.png" alt="Provider management page">
    </td>
    <td width="50%">
      <img src="docs/promo/screenshots/readme-deepseek-config.png" alt="Add DeepSeek provider">
    </td>
  </tr>
  <tr>
    <td align="center">Provider management and quick switching</td>
    <td align="center">Preset-based setup with recommended API URLs and models</td>
  </tr>
  <tr>
    <td width="50%">
      <img src="docs/promo/screenshots/readme-deepseek-options.png" alt="DeepSeek options">
    </td>
    <td width="50%">
      <img src="docs/promo/screenshots/readme-qwen-1m-menu.png" alt="Qwen 1M model menu">
    </td>
  </tr>
  <tr>
    <td align="center">DeepSeek 1M context and Max reasoning options</td>
    <td align="center">Qwen 1M context exposed in Claude Desktop</td>
  </tr>
</table>

## What It Does

- Manages DeepSeek, Kimi, Zhipu GLM, Alibaba Cloud Bailian, Xiaomi MiMo, and custom third-party providers.
- Applies Claude Desktop third-party inference settings on Windows and macOS.
- Uses a local gateway to keep model mapping, protocol compatibility, extra headers, and upstream keys under local control.
- Shows only explicitly mapped Claude-safe model routes in Claude Desktop.
- Rejects unmapped Claude model routes instead of silently falling back to an internal default.
- Imports Anthropic-compatible CC-Switch configurations while leaving OpenAI-format entries opt-in.
- Provides provider connectivity checks, model availability checks, SSE streaming, and custom upstream HTTP proxy support.
- Prevents duplicate Windows app instances: launching the shortcut again brings the existing window forward.

## Download

Get the latest release from:

```text
https://github.com/lonr-6/cc-desktop-switch/releases/latest
```

Recommended downloads:

- `CC-Desktop-Switch-v<version>-Windows-Setup.exe` for the Windows installer.
- `CC-Desktop-Switch-v<version>-Windows-Portable.zip` for the Windows portable package.
- `CC-Desktop-Switch-v<version>-macOS-arm64.pkg` for the macOS installer.
- `CC-Desktop-Switch-v<version>-macOS-arm64.dmg` for the macOS drag-and-drop package.

Windows builds are not Authenticode-signed yet, so Windows may show an unknown publisher warning. Release assets include `.sha256`, `.sig`, and the public key for integrity checks.

## Quick Start

1. Download and open CC Desktop Switch.
2. Pick a provider preset or add a custom provider.
3. Enter your own API key.
4. Adjust model mappings if needed.
5. Click `Apply to Claude Desktop`.
6. Fully restart Claude Desktop.

If the desktop window cannot open, use the fallback local UI:

```text
http://127.0.0.1:18081
```

Default ports:

- Admin UI: `18081`
- Local gateway: `18080`

## Model Mapping

Claude Desktop expects Claude-family model names. Many third-party providers use model IDs such as `deepseek-v4-pro`, `kimi-k2.6`, `glm-5.1`, or `qwen3.6-plus`.

CC Desktop Switch keeps those real upstream model IDs inside the local gateway and exposes Claude-safe route names to Claude Desktop. Since v1.0.19, only explicitly mapped Claude slots appear in the Claude Desktop model menu. `Default` remains an internal fallback and is not shown as a menu item.

## Development

```powershell
git clone https://github.com/lonr-6/cc-desktop-switch.git
cd cc-desktop-switch
pip install -r requirements.txt
python main.py
```

Browser fallback for development:

```powershell
python main.py --browser
```

Verification:

```powershell
python -m compileall -q backend main.py tests
python -m unittest discover -s tests -v
node --check frontend/js/api.js
node --check frontend/js/app.js
node --check frontend/js/i18n.js
```

## Troubleshooting

### Claude Desktop still uses the old provider

Claude Desktop reads third-party inference configuration during startup. After applying a provider, fully quit Claude Desktop and open it again. Closing only the chat window is often not enough.

### Claude Desktop cannot connect

Third-party providers use the local gateway by default. Make sure CC Desktop Switch is still running in the background, and check whether the local ports are occupied:

```powershell
netstat -ano | findstr :18081
netstat -ano | findstr :18080
```

### Claude Code attribution header

`CLAUDE_CODE_ATTRIBUTION_HEADER=0` is only a Claude Code prompt-cache compatibility option. It is not a Claude Desktop 3P setting and does not replace the local gateway.

### GitHub Copilot subscription

GitHub Copilot subscriptions are not directly supported as a provider API in CC Desktop Switch. If you bring your own OpenAI Chat or Anthropic-compatible endpoint that proxies Copilot or another service, you can try it as a custom third-party provider at your own risk. Verify the endpoint's terms, account risk, API format, base URL, streaming behavior, and tool-call compatibility before relying on it.

## Star History

<a href="https://www.star-history.com/#lonr-6/cc-desktop-switch&Date">
  <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=lonr-6/cc-desktop-switch&type=Date">
</a>

## Tech Stack

- Backend: Python, FastAPI, httpx, uvicorn
- Frontend: HTML, CSS, vanilla JavaScript, Bootstrap 5.3 CDN
- Storage: `~/.cc-desktop-switch/config.json`
- Packaging: PyInstaller, NSIS, macOS pkg/dmg scripts

## Disclaimer

This project is not affiliated with Anthropic, Claude, CC-Switch, or any third-party model provider. Your upstream API keys are stored locally on your machine.
