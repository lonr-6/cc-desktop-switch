import copy
import os
import stat
import tempfile
import unittest

from backend import config as cfg


@unittest.skipUnless(os.name == "posix", "POSIX permission bits are not available")
class ConfigPermissionTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.old_config_dir = cfg.CONFIG_DIR
        self.old_config_file = cfg.CONFIG_FILE
        self.old_backup_dir = cfg.BACKUP_DIR
        cfg.CONFIG_DIR = os.path.join(self.temp_dir.name, "credentials")
        cfg.CONFIG_FILE = os.path.join(cfg.CONFIG_DIR, "config.json")
        cfg.BACKUP_DIR = os.path.join(cfg.CONFIG_DIR, "backups")

    def tearDown(self):
        cfg.CONFIG_DIR = self.old_config_dir
        cfg.CONFIG_FILE = self.old_config_file
        cfg.BACKUP_DIR = self.old_backup_dir
        self.temp_dir.cleanup()

    @staticmethod
    def _mode(path: str) -> int:
        return stat.S_IMODE(os.stat(path).st_mode)

    def _config_with_secret(self) -> dict:
        config = copy.deepcopy(cfg.DEFAULT_CONFIG)
        config["gatewayApiKey"] = "ccds_test_gateway_secret"
        config["providers"] = [
            {
                "id": "provider",
                "name": "Provider",
                "apiKey": "upstream-secret",
            }
        ]
        config["activeProvider"] = "provider"
        return config

    def test_save_and_backup_use_private_permissions(self):
        os.makedirs(cfg.CONFIG_DIR, mode=0o755)
        os.chmod(cfg.CONFIG_DIR, 0o755)

        cfg.save_config(self._config_with_secret())
        backup = cfg.create_backup("permissions")
        backup_path = os.path.join(cfg.BACKUP_DIR, backup["name"])

        self.assertEqual(self._mode(cfg.CONFIG_DIR), 0o700)
        self.assertEqual(self._mode(cfg.CONFIG_FILE), 0o600)
        self.assertEqual(self._mode(cfg.BACKUP_DIR), 0o700)
        self.assertEqual(self._mode(backup_path), 0o600)

    def test_load_repairs_existing_overly_broad_permissions(self):
        cfg.save_config(self._config_with_secret())
        os.chmod(cfg.CONFIG_DIR, 0o755)
        os.chmod(cfg.CONFIG_FILE, 0o644)

        loaded = cfg.load_config()

        self.assertEqual(loaded["gatewayApiKey"], "ccds_test_gateway_secret")
        self.assertEqual(self._mode(cfg.CONFIG_DIR), 0o700)
        self.assertEqual(self._mode(cfg.CONFIG_FILE), 0o600)

    def test_failed_save_does_not_leave_temporary_credentials_file(self):
        config = self._config_with_secret()
        config["settings"]["notJsonSerializable"] = object()

        with self.assertRaises(TypeError):
            cfg.save_config(config)

        leftovers = [
            name
            for name in os.listdir(cfg.CONFIG_DIR)
            if name.startswith(".config-") and name.endswith(".tmp")
        ]
        self.assertEqual(leftovers, [])


if __name__ == "__main__":
    unittest.main()
