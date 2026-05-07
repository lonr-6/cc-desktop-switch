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

CC Desktop Switch は、公式 Claude Desktop クライアント向けの軽量なデスクトップ設定ツールです。DeepSeek、Kimi、Zhipu GLM、Alibaba Cloud Bailian、Xiaomi MiMo などの Anthropic 互換 API プロバイダーを管理し、Claude Desktop のサードパーティ推論設定をワンクリックで適用できます。

このプロジェクトは Windows と macOS の Claude Desktop ユーザー向けです。Claude Code / CLI 向けの `farion1231/cc-switch` とは異なり、通常のデスクトップ利用者が provider 設定、モデルマッピング、ヘルスチェック、ローカル gateway 互換を扱いやすくすることを目的としています。

v1.0.18 以降、Claude Desktop は `127.0.0.1` のローカル CC Desktop Switch gateway に接続します。サードパーティ provider を使う間は、CC Desktop Switch をバックグラウンドで起動したままにしてください。Windows ではウィンドウを閉じてもトレイに残り、macOS ではウィンドウを閉じるとアプリが非表示になります。

## Preview

<table>
  <tr>
    <td width="50%">
      <img src="docs/promo/screenshots/readme-provider-list.png" alt="Provider management">
    </td>
    <td width="50%">
      <img src="docs/promo/screenshots/readme-deepseek-config.png" alt="DeepSeek provider setup">
    </td>
  </tr>
  <tr>
    <td align="center">Provider の管理とクイック切り替え</td>
    <td align="center">プリセットから API URL と推奨モデルを入力</td>
  </tr>
  <tr>
    <td width="50%">
      <img src="docs/promo/screenshots/readme-deepseek-options.png" alt="DeepSeek options">
    </td>
    <td width="50%">
      <img src="docs/promo/screenshots/readme-qwen-1m-menu.png" alt="Qwen 1M menu">
    </td>
  </tr>
  <tr>
    <td align="center">DeepSeek 1M context と Max reasoning</td>
    <td align="center">Qwen 1M context を Claude Desktop に表示</td>
  </tr>
</table>

## 主な機能

- DeepSeek、Kimi、Zhipu GLM、Alibaba Cloud Bailian、Xiaomi MiMo、カスタム provider を管理。
- Windows / macOS の Claude Desktop 3P 設定をワンクリックで適用。
- ローカル gateway でモデルマッピング、プロトコル互換、追加ヘッダー、上流 API Key を安全に管理。
- Claude Desktop には明示的にマッピングした Claude-safe route だけを表示。
- 未設定の Claude model route は内部 Default に黙ってフォールバックせず、明確にエラーを返す。
- Anthropic 互換の CC-Switch 設定をインポート。
- provider 接続チェック、モデル疎通チェック、SSE streaming、上流 HTTP proxy に対応。
- Windows ではショートカットを再度起動しても既存ウィンドウを前面に戻し、二重起動を防止。

## Download

最新リリース：

```text
https://github.com/lonr-6/cc-desktop-switch/releases/latest
```

推奨ファイル：

- `CC-Desktop-Switch-v<version>-Windows-Setup.exe`：Windows installer
- `CC-Desktop-Switch-v<version>-Windows-Portable.zip`：Windows portable package
- `CC-Desktop-Switch-v<version>-macOS-arm64.pkg`：macOS installer
- `CC-Desktop-Switch-v<version>-macOS-arm64.dmg`：macOS drag-and-drop package

Windows build にはまだ Authenticode code signing certificate がありません。そのため、Windows が unknown publisher warning を表示する場合があります。Release assets には `.sha256`、`.sig`、public key が含まれています。

## Quick Start

1. CC Desktop Switch をダウンロードして起動します。
2. provider preset を選ぶか、custom provider を追加します。
3. 自分の API Key を入力します。
4. 必要に応じて model mapping を調整します。
5. `Apply to Claude Desktop` をクリックします。
6. Claude Desktop を完全に再起動します。

デスクトップウィンドウが開かない場合は、以下のローカル UI を使えます。

```text
http://127.0.0.1:18081
```

Default ports:

- Admin UI: `18081`
- Local gateway: `18080`

## Model Mapping

Claude Desktop は Claude 系の model name を期待します。一方、多くの third-party provider は `deepseek-v4-pro`、`kimi-k2.6`、`glm-5.1`、`qwen3.6-plus` のような実際の上流 model ID を使います。

CC Desktop Switch は実際の上流 model ID をローカル gateway の内部に保持し、Claude Desktop には Claude-safe route name だけを公開します。v1.0.19 以降、Claude Desktop の model menu には明示的にマッピングした Claude slot だけが表示されます。`Default` は内部 fallback として保存されますが、menu item としては表示されません。

## Development

```powershell
git clone https://github.com/lonr-6/cc-desktop-switch.git
cd cc-desktop-switch
pip install -r requirements.txt
python main.py
```

Browser fallback:

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

### Claude Desktop が古い provider を使い続ける

Claude Desktop は起動時に third-party inference configuration を読み込みます。provider を適用した後は、Claude Desktop を完全に終了してから再起動してください。

### Claude Desktop が接続できない

third-party provider はデフォルトで local gateway に依存します。CC Desktop Switch がバックグラウンドで動作していることを確認し、必要なら port を確認してください。

```powershell
netstat -ano | findstr :18081
netstat -ano | findstr :18080
```

### Claude Code attribution header

`CLAUDE_CODE_ATTRIBUTION_HEADER=0` は Claude Code の prompt cache 互換用です。Claude Desktop の 3P 設定ではなく、local gateway の代わりにはなりません。

### GitHub Copilot サブスクリプション

GitHub Copilot のサブスクリプションは、CC Desktop Switch の provider API として直接サポートされません。Copilot や別サービスを OpenAI Chat または Anthropic 互換 endpoint として中継するものを自分で用意する場合は、custom third-party provider として自己責任で試せます。利用規約、アカウントリスク、API format、Base URL、streaming、tool-call compatibility を事前に確認してください。

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
