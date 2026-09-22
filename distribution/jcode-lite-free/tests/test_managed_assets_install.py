import subprocess
import unittest

import test_install_flow


class ManagedAssetsInstallTest(test_install_flow.MacInstallFlowTest):
    def test_assets_installed_and_local_edits_survive_upgrade(self):
        role = self.package / "preset/roles/implement.md"
        role.parent.mkdir(exist_ok=True)
        role.write_text("Packaged implementation role v1\n")
        skill = self.package / "preset/skills/upgrade-fixture/SKILL.md"
        skill.parent.mkdir()
        skill.write_text("Packaged skill v1\n")
        prompt = self.package / "preset/swarm-prompt.md"
        prompt.write_text("Provider-neutral task-specific routing v1\n")
        # This fixture exercises installer state migration, not MCP execution.
        (self.package / "preset/mcp/node_modules").mkdir(exist_ok=True)

        def install():
            files = sorted(str(p.relative_to(self.package)) for p in self.package.rglob("*")
                           if p.is_file() and p.name != "allowlist.txt")
            (self.package / "allowlist.txt").write_text("\n".join(files + ["allowlist.txt"]) + "\n")
            env = dict(self.env, JCODE_LITE_FREE_INSTALL_NO_LAUNCH="1")
            subprocess.run([str(self.package / "install.command")], env=env,
                           cwd=self.package, check=True, capture_output=True, text=True)

        install()
        home = self.home / "Library/Application Support/LeGrin/JcodeLiteFree/home"
        self.assertEqual((home / "roles/implement.md").read_text(), "Packaged implementation role v1\n")
        self.assertEqual((home / "swarm-prompt.md").read_text(), "Provider-neutral task-specific routing v1\n")
        custom = home / "skills/my-custom/SKILL.md"
        custom.parent.mkdir()
        custom.write_text("Recipient's own skill\n")
        (home / "roles/implement.md").write_text("Recipient customized role\n")
        role.write_text("Packaged implementation role v2\n")
        skill.write_text("Packaged skill v2\n")
        prompt.write_text("Provider-neutral task-specific routing v2\n")
        install()
        self.assertEqual(custom.read_text(), "Recipient's own skill\n")
        self.assertEqual((home / "roles/implement.md").read_text(), "Recipient customized role\n")
        self.assertEqual((home / "skills/upgrade-fixture/SKILL.md").read_text(), "Packaged skill v2\n")
        self.assertEqual((home / "swarm-prompt.md").read_text(), "Provider-neutral task-specific routing v2\n")
        self.assertFalse((self.home / ".jcode").exists())
        install()
        self.assertEqual(custom.read_text(), "Recipient's own skill\n")
        self.assertEqual((home / "roles/implement.md").read_text(), "Recipient customized role\n")


if __name__ == "__main__":
    unittest.main()
