# CC Desktop Switch v1.0.9

本次版本修复 Claude 桌面版配置写入权限问题，并优化安装更新体验。

## 主要变化

- 当 `HKCU\SOFTWARE\Policies\Claude` 只有管理员可写时，切换 provider 或一键应用会触发 UAC 管理员写入，而不是只提示同步失败。
- 提权写入会定位当前登录用户的 registry hive，避免管理员进程写到错误用户。
- 托盘切换 provider 后的“重启 Claude 桌面版”提示改为非阻塞弹窗，点击“确定”会立即关闭。
- 安装器会自动识别上一次安装目录，避免升级时重复选择目录。
- 安装和卸载前会自动关闭正在运行的 `CC-Desktop-Switch.exe`，减少文件占用导致的安装失败。

## 隐私说明

- 本版本没有提交本机 `~/.cc-desktop-switch/config.json`、配置备份、`.env`、PFX、私钥或真实 API Key。
- 提权写入只写入 Claude 桌面版需要的本地 gateway 地址、gateway key 和模型列表，不写入上游 provider API Key。

## 验证

- `python -m compileall -q backend main.py tests`
- `node --check frontend/js/api.js`
- `node --check frontend/js/app.js`
- `node --check frontend/js/i18n.js`
- `python -m unittest discover -s tests -v`

## 下载建议

- 普通用户优先下载 `CC-Desktop-Switch-v1.0.9-Windows-Setup.exe`。
- 不想安装可以下载 `CC-Desktop-Switch-v1.0.9-Windows-Portable.zip`。
