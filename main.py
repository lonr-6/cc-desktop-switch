#!/usr/bin/env python3
"""CC Desktop Switch - 启动入口"""

import sys
import uvicorn
import webbrowser
import threading
import time

from backend.main import create_admin_app, _start_proxy_server
from backend import config as cfg


def main():
    # 确保配置目录存在
    cfg.ensure_config_dir()

    # 读取设置
    settings = cfg.get_settings()
    admin_port = settings.get("adminPort", 18081)
    proxy_port = settings.get("proxyPort", 18080)
    auto_start_proxy = settings.get("autoStart", False)

    # 如果开启了自动启动代理
    if auto_start_proxy:
        print(f"  自动启动代理 (端口 {proxy_port})...")
        _start_proxy_server(proxy_port)

    # 创建管理后台应用
    admin_app = create_admin_app()

    # 启动后打开浏览器
    def open_browser():
        time.sleep(1.5)
        webbrowser.open(f"http://127.0.0.1:{admin_port}")

    threading.Thread(target=open_browser, daemon=True).start()

    print(f"""
╔══════════════════════════════════════════╗
║       CC Desktop Switch v1.0.0          ║
║                                          ║
║  管理后台: http://127.0.0.1:{admin_port}     ║
║  代理端口: {proxy_port}                          ║
║                                          ║
║  按 Ctrl+C 停止                          ║
╚══════════════════════════════════════════╝
    """)

    # 启动管理后台
    uvicorn.run(
        admin_app,
        host="127.0.0.1",
        port=admin_port,
        log_level="info",
    )


if __name__ == "__main__":
    main()
