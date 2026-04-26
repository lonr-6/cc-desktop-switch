#!/usr/bin/env python3
"""CC Desktop Switch - 启动入口"""

import argparse
import sys
import threading
import time
import traceback
from pathlib import Path
import webbrowser
from urllib.error import URLError
from urllib.request import urlopen

import uvicorn

from backend.main import create_admin_app, _start_proxy_server, _stop_proxy_server
from backend import config as cfg
from backend import registry


APP_NAME = "CC Desktop Switch"
APP_VERSION = "1.0.6"
TRAY_OPEN_LABEL = "打开 CC Desktop Switch"
TRAY_QUIT_LABEL = "退出"


def safe_print(message: str):
    """windowed exe 没有控制台时，print 不能影响主流程。"""
    stream = getattr(sys, "stdout", None)
    if not stream:
        return
    try:
        print(message)
    except OSError:
        return


def write_crash_log():
    """打包为 windowed exe 后没有控制台，崩溃信息写入本机日志。"""
    try:
        cfg.ensure_config_dir()
        log_path = Path(cfg.CONFIG_DIR) / "ccds-crash.log"
        log_path.write_text(traceback.format_exc(), encoding="utf-8")
    except Exception:
        return


def parse_args():
    """解析启动参数。默认走桌面窗口，浏览器模式只作为备用。"""
    parser = argparse.ArgumentParser(description=APP_NAME)
    parser.add_argument(
        "--browser",
        action="store_true",
        help="Open the system browser instead of the desktop window.",
    )
    parser.add_argument(
        "--server-only",
        action="store_true",
        help="Start the local admin server without opening any UI.",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=None,
        help="Override the admin server port.",
    )
    return parser.parse_args()


def wait_for_admin(url: str, timeout: float = 12.0) -> bool:
    """等待管理后台可访问，避免窗口先打开后白屏。"""
    deadline = time.time() + timeout
    status_url = f"{url}/api/status"

    while time.time() < deadline:
        try:
            with urlopen(status_url, timeout=0.6) as response:
                if response.status < 500:
                    return True
        except (OSError, URLError):
            time.sleep(0.2)

    return False


def build_admin_server(admin_app, port: int) -> uvicorn.Server:
    """创建可由桌面窗口生命周期控制的管理后台服务器。"""
    server_config = uvicorn.Config(
        admin_app,
        host="127.0.0.1",
        port=port,
        log_level="warning",
        access_log=False,
        log_config=None,
    )
    return uvicorn.Server(server_config)


def start_admin_server(admin_app, port: int):
    """后台线程启动管理后台，供 WebView 或浏览器访问。"""
    server = build_admin_server(admin_app, port)
    thread = threading.Thread(target=server.run, daemon=True)
    thread.start()
    return server, thread


def open_browser_when_ready(url: str):
    if wait_for_admin(url):
        webbrowser.open(url)


class DesktopTrayController:
    """系统托盘控制器：关闭窗口时隐藏，托盘菜单里显式退出。"""

    def __init__(self, window, icon_path: Path):
        self.window = window
        self.icon_path = Path(icon_path)
        self.icon = None
        self.thread = None
        self.pystray = None
        self.exit_requested = False
        self._notified = False

    def start(self) -> bool:
        """启动系统托盘图标。依赖缺失时返回 False，不影响主窗口打开。"""
        try:
            import pystray
            from PIL import Image
        except Exception as exc:
            safe_print(f"system tray unavailable: {exc}")
            return False

        try:
            self.pystray = pystray
            image = Image.open(self.icon_path)
            self.icon = pystray.Icon(APP_NAME, image, APP_NAME, self.build_menu())
            self.thread = threading.Thread(target=self.icon.run, daemon=True)
            self.thread.start()
            return True
        except Exception as exc:
            safe_print(f"system tray failed: {exc}")
            return False

    def build_menu(self):
        """构建托盘菜单，包含 provider 快速切换项。"""
        if not self.pystray:
            return None
        items = [
            self.pystray.MenuItem(TRAY_OPEN_LABEL, self.show_window, default=True),
            self.pystray.Menu.SEPARATOR,
        ]
        items.extend(self.provider_menu_items())
        items.extend([
            self.pystray.Menu.SEPARATOR,
            self.pystray.MenuItem(TRAY_QUIT_LABEL, self.quit_app),
        ])
        return self.pystray.Menu(*items)

    def provider_menu_items(self):
        """返回 provider 切换菜单项。"""
        if not self.pystray:
            return []
        config = cfg.load_config()
        active_id = config.get("activeProvider")
        providers = config.get("providers", [])
        if not providers:
            return [self.pystray.MenuItem("暂无提供商", None, enabled=False)]

        items = [self.pystray.MenuItem("切换提供商", None, enabled=False)]
        for provider in providers:
            provider_id = provider.get("id")
            name = provider.get("name", "Unnamed Provider")
            label = f"✓ {name}" if provider_id == active_id else name
            items.append(self.pystray.MenuItem(
                label,
                self._make_provider_switcher(provider_id),
                checked=lambda item, pid=provider_id: cfg.load_config().get("activeProvider") == pid,
            ))
        return items

    def _make_provider_switcher(self, provider_id: str):
        def switch(icon=None, item=None):
            self.switch_provider(provider_id)
        return switch

    def switch_provider(self, provider_id: str) -> bool:
        """从托盘菜单切换默认 provider。"""
        if not provider_id:
            return False
        if not cfg.set_active_provider(provider_id):
            return False
        provider = cfg.get_provider(provider_id)
        desktop_message = ""
        try:
            status = registry.get_config_status()
            if status.get("configured") and provider:
                proxy_port = cfg.get_settings().get("proxyPort", 18080)
                result = registry.apply_config(
                    f"http://127.0.0.1:{proxy_port}",
                    gateway_api_key=cfg.get_or_create_gateway_api_key(),
                    provider=provider,
                )
                desktop_message = "，桌面版模型已同步" if result.get("success") else "，请重新一键应用到 Claude 桌面版"
        except Exception as exc:
            safe_print(f"sync desktop config failed: {exc}")
            desktop_message = "，请重新一键应用到 Claude 桌面版"
        self.refresh_menu()
        try:
            if self.icon and provider:
                self.icon.notify(f"已切换到 {provider.get('name', provider_id)}{desktop_message}", APP_NAME)
        except Exception:
            pass
        return True

    def refresh_menu(self):
        """provider 变化后刷新托盘菜单。"""
        if not self.icon or not self.pystray:
            return
        try:
            self.icon.menu = self.build_menu()
            if hasattr(self.icon, "update_menu"):
                self.icon.update_menu()
        except Exception as exc:
            safe_print(f"refresh tray menu failed: {exc}")

    def handle_window_closing(self):
        """pywebview closing 事件：返回 False 表示取消关闭。"""
        if self.exit_requested:
            return None

        self.hide_window()
        self.notify_hidden()
        return False

    def hide_window(self):
        self.window.hide()

    def show_window(self, icon=None, item=None):
        try:
            self.window.show()
            self.window.restore()
        except Exception as exc:
            safe_print(f"show window failed: {exc}")

    def notify_hidden(self):
        if self._notified or not self.icon:
            return
        self._notified = True
        try:
            self.icon.notify(
                "程序仍在后台运行。右键托盘图标可打开或退出。",
                APP_NAME,
            )
        except Exception:
            return

    def quit_app(self, icon=None, item=None):
        self.exit_requested = True
        try:
            self.window.destroy()
        except Exception as exc:
            safe_print(f"quit failed: {exc}")

    def stop(self):
        if not self.icon:
            return
        try:
            self.icon.stop()
        except Exception:
            return


def open_desktop_window(url: str) -> bool:
    """打开原生桌面窗口。失败时返回 False，让调用方退回浏览器模式。"""
    try:
        import webview
    except Exception as exc:
        safe_print(f"pywebview unavailable, fallback to browser: {exc}")
        return False

    try:
        window = webview.create_window(
            APP_NAME,
            url,
            width=1240,
            height=820,
            min_size=(980, 680),
            text_select=True,
        )
        tray = DesktopTrayController(
            window,
            Path(__file__).resolve().parent / "frontend" / "assets" / "app-icon.png",
        )
        if tray.start():
            window.events.closing += tray.handle_window_closing
            window.events.closed += tray.stop
        webview.start(debug=False)
        return True
    except Exception as exc:
        safe_print(f"desktop window failed, fallback to browser: {exc}")
        return False


def run_browser_mode(admin_app, admin_port: int, open_ui: bool = True):
    url = f"http://127.0.0.1:{admin_port}"
    if open_ui:
        threading.Thread(target=open_browser_when_ready, args=(url,), daemon=True).start()

    safe_print(f"""
╔══════════════════════════════════════════╗
║       {APP_NAME} v{APP_VERSION}          ║
║                                          ║
║  管理后台: {url}     ║
║                                          ║
║  按 Ctrl+C 停止                          ║
╚══════════════════════════════════════════╝
    """)

    uvicorn.run(
        admin_app,
        host="127.0.0.1",
        port=admin_port,
        log_level="warning",
        access_log=False,
        log_config=None,
    )


def run_desktop_mode(admin_app, admin_port: int):
    url = f"http://127.0.0.1:{admin_port}"
    server, server_thread = start_admin_server(admin_app, admin_port)

    try:
        if not wait_for_admin(url):
            safe_print(f"admin server is not ready, fallback to browser: {url}")
            webbrowser.open(url)
            while server_thread.is_alive() and not server.should_exit:
                time.sleep(0.5)
            return

        if not open_desktop_window(url):
            webbrowser.open(url)
            while server_thread.is_alive() and not server.should_exit:
                time.sleep(0.5)
    finally:
        server.should_exit = True
        _stop_proxy_server()


def main():
    args = parse_args()

    # 确保配置目录存在
    cfg.ensure_config_dir()

    # 读取设置
    settings = cfg.get_settings()
    admin_port = args.port or settings.get("adminPort", 18081)
    proxy_port = settings.get("proxyPort", 18080)
    auto_start_proxy = settings.get("autoStart", False)

    # 如果开启了自动启动代理
    if auto_start_proxy:
        safe_print(f"  自动启动代理 (端口 {proxy_port})...")
        _start_proxy_server(proxy_port)

    # 创建管理后台应用
    admin_app = create_admin_app()

    if args.server_only:
        run_browser_mode(admin_app, admin_port, open_ui=False)
    elif args.browser:
        run_browser_mode(admin_app, admin_port, open_ui=True)
    else:
        run_desktop_mode(admin_app, admin_port)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        write_crash_log()
        raise
