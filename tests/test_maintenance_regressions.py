import unittest
from pathlib import Path

from backend.proxy import _is_max_unsupported_error


ROOT = Path(__file__).resolve().parents[1]


class MaxUnsupportedErrorTests(unittest.TestCase):
    def test_detects_rejected_thinking_or_effort_fields(self):
        self.assertTrue(
            _is_max_unsupported_error(
                400,
                '{"error":{"message":"output_config.effort=max is not supported"}}',
            )
        )
        self.assertTrue(_is_max_unsupported_error(422, "Invalid parameter: thinking"))
        self.assertTrue(
            _is_max_unsupported_error(400, "reasoning_effort has an invalid value: max")
        )

    def test_preserves_unrelated_upstream_errors(self):
        self.assertFalse(_is_max_unsupported_error(400, "invalid parameter: temperature"))
        self.assertFalse(_is_max_unsupported_error(400, "maximum context length exceeded"))
        self.assertFalse(_is_max_unsupported_error(400, "model is not supported"))
        self.assertFalse(_is_max_unsupported_error(401, "thinking is not supported"))


class FrontendPresetFormatTests(unittest.TestCase):
    def test_lowercase_openai_preset_is_passed_to_the_normalizer(self):
        source = (ROOT / "frontend/js/app.js").read_text(encoding="utf-8")
        self.assertIn("setFormApiFormat(preset.apiFormat);", source)
        self.assertNotIn('preset.apiFormat === "OpenAI"', source)


class InstallerCleanupTests(unittest.TestCase):
    def test_upgrade_keeps_policy_and_real_uninstall_clears_only_ccds_values(self):
        source = (ROOT / "installer.nsi").read_text(encoding="utf-8")
        self.assertIn("/S /UPGRADE", source)
        self.assertIn('${GetOptions} "$R0" "/UPGRADE" $R1', source)
        self.assertIn('ReadRegStr $0 HKCU "${CLAUDE_POLICY_KEY}" "ccds_managed"', source)
        self.assertIn('StrCmp $0 "true" 0 done_clear_policy', source)
        self.assertIn('${If} $UninstallIsUpgrade != "1"', source)
        for value_name in (
            "inferenceProvider",
            "inferenceGatewayBaseUrl",
            "inferenceGatewayApiKey",
            "inferenceGatewayAuthScheme",
            "inferenceGatewayHeaders",
            "inferenceModels",
            "isClaudeCodeForDesktopEnabled",
            "coworkEgressAllowedHosts",
            "ccds_managed",
        ):
            self.assertIn(
                f'DeleteRegValue HKCU "${{CLAUDE_POLICY_KEY}}" "{value_name}"',
                source,
            )


if __name__ == "__main__":
    unittest.main()
