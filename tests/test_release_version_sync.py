import unittest
from pathlib import Path

import main as app_main
from backend import config as cfg
from backend.main import create_admin_app
from backend.proxy import create_proxy_app


ROOT = Path(__file__).resolve().parents[1]
EXPECTED_VERSION = "1.0.26"


class ReleaseVersionSyncTests(unittest.TestCase):
    def test_runtime_and_packaging_versions_are_synchronized(self):
        self.assertEqual(app_main.APP_VERSION, EXPECTED_VERSION)
        self.assertEqual(cfg.DEFAULT_CONFIG["version"], EXPECTED_VERSION)
        self.assertEqual(create_admin_app().version, EXPECTED_VERSION)
        self.assertEqual(create_proxy_app().version, EXPECTED_VERSION)

        checks = {
            "frontend/index.html": f"<strong>{EXPECTED_VERSION}</strong>",
            "installer.nsi": f'!define PRODUCT_VERSION "{EXPECTED_VERSION}"',
            "scripts/New-Release.ps1": f'[string]$Version = "{EXPECTED_VERSION}"',
            "start.bat": f"CC Desktop Switch v{EXPECTED_VERSION}",
            "build.bat": f"CC Desktop Switch v{EXPECTED_VERSION} - 构建工具",
            "docs/CHANGELOG.md": f"## v{EXPECTED_VERSION}",
            f"docs/release-notes-v{EXPECTED_VERSION}.md": f"# CC Desktop Switch v{EXPECTED_VERSION}",
        }
        for relative_path, expected in checks.items():
            text = (ROOT / relative_path).read_text(encoding="utf-8")
            self.assertIn(expected, text, relative_path)


if __name__ == "__main__":
    unittest.main()
