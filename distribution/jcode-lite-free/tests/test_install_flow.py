import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(sys.platform == "darwin", "macOS install flow test")
class MacInstallFlowTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="jcode-lite-free-install.")
        self.base = Path(self.temp.name)
        self.package = self.base / "downloaded-package"
        self.home = self.base / "recipient-home"
        self.package.mkdir()
        self.home.mkdir()
        shutil.copytree(ROOT / "preset", self.package / "preset")
        shared = ROOT.parent / "jcode-lite" / "common" / "preset"
        shutil.copytree(shared / "skills", self.package / "preset" / "skills")
        shutil.copytree(
            shared / "knowledge-os", self.package / "preset" / "knowledge-os"
        )
        # Vendored MCP surface (share with Lite: idempotent templates and
        # Node wrappers under preset/mcp/bin/, build with npm ci at install
        # time). Free ships an empty placeholder dir to keep the build script's
        # preset layout consistent with Lite; populate it from the shared
        # source for the install simulation.
        shutil.copytree(
            shared / "mcp", self.package / "preset" / "mcp", dirs_exist_ok=True
        )
        shutil.copytree(ROOT / "macos", self.package, dirs_exist_ok=True)
        shutil.copy2(ROOT / "release.json", self.package / "release.json")
        self.version = json.loads((ROOT / "release.json").read_text())["version"]
        binary = self.package / "bin" / "jcode"
        binary.parent.mkdir()
        binary.write_text(
            '#!/bin/bash\nprintf "FAKE_JCODE %s\\n" "$*"\nprintf "FAKE_CWD %s\\n" "$PWD"\n',
            encoding="utf-8",
        )
        binary.chmod(0o755)
        for script in self.package.glob("*.command"):
            script.chmod(0o755)
        (self.package / "jcode-free").chmod(0o755)
        files = sorted(
            str(path.relative_to(self.package))
            for path in self.package.rglob("*")
            if path.is_file() and path.name != "allowlist.txt"
        )
        (self.package / "allowlist.txt").write_text(
            "\n".join(files + ["allowlist.txt"]) + "\n", encoding="utf-8"
        )
        self.env = os.environ.copy()
        self.env["HOME"] = str(self.home)
        self.env["SHELL"] = "/bin/zsh"
        self.env["JCODE_LITE_FREE_UPDATE_CHECK"] = "0"

    def tearDown(self):
        self.temp.cleanup()

    def test_install_is_isolated_provider_free_and_cwd_aware(self):
        output = subprocess.run(
            [str(self.package / "jcode-free.command"), "version"],
            cwd=self.package,
            env=self.env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=True,
        ).stdout
        self.assertIn(f"Installed Jcode Lite Free {self.version}.", output)
        self.assertIn("FAKE_JCODE --no-update version", output)

        install_root = (
            self.home / "Library" / "Application Support" / "LeGrin" / "JcodeLiteFree"
        )
        self.assertFalse((install_root / "home" / "stables.env").exists())
        config = (install_root / "home" / "config.toml").read_text()
        self.assertNotIn("providers.stables", config)
        self.assertNotIn("default_provider", config)

        cli = self.home / ".local" / "bin" / "jcodef"
        project = self.base / "project-context"
        project.mkdir()
        cli_output = subprocess.run(
            [str(cli), "version"],
            cwd=project,
            env=self.env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=True,
        ).stdout
        self.assertIn(f"FAKE_CWD {project}", cli_output)
        self.assertFalse((self.home / ".local" / "bin" / "jcodel").exists())

        config_path = install_root / "home" / "config.toml"
        configured = config_path.read_text() + '\n[providers.example]\ntype = "openai-compatible"\n'
        config_path.write_text(configured)
        update_env = self.env.copy()
        update_env["JCODE_LITE_FREE_INSTALL_NO_LAUNCH"] = "1"
        subprocess.run(
            [str(self.package / "install.command")],
            cwd=self.package,
            env=update_env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=True,
        )
        self.assertEqual(config_path.read_text(), configured)

    def test_remove_preserves_sessions_by_default_and_purge_is_explicit(self):
        install_env = self.env.copy()
        install_env["JCODE_LITE_FREE_INSTALL_NO_LAUNCH"] = "1"
        subprocess.run(
            [str(self.package / "install.command")],
            cwd=self.package,
            env=install_env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=True,
        )
        install_root = (
            self.home / "Library" / "Application Support" / "LeGrin" / "JcodeLiteFree"
        )
        session = install_root / "home" / "sessions" / "friend-session.json"
        session.parent.mkdir(parents=True)
        session.write_text('{"kept":true}\n', encoding="utf-8")
        cli = self.home / ".local" / "bin" / "jcodef"

        removed = subprocess.run(
            [str(cli), "remove"],
            cwd=self.base,
            env=self.env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=True,
        ).stdout
        self.assertIn("Kept sessions, memory, credentials, and config", removed)
        self.assertTrue(session.is_file())
        self.assertFalse((install_root / "state.json").exists())
        self.assertFalse(cli.exists())

        subprocess.run(
            [str(self.package / "install.command")],
            cwd=self.package,
            env=install_env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=True,
        )
        self.assertTrue(session.is_file())
        purge_env = self.env.copy()
        purge_env["JCODE_LITE_FREE_REMOVE_NO_CONFIRM"] = "1"
        subprocess.run(
            [str(cli), "remove", "--purge-data"],
            cwd=self.base,
            env=purge_env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=True,
        )
        self.assertFalse(install_root.exists())
        self.assertFalse(cli.exists())

    def test_failed_candidate_health_check_keeps_active_version(self):
        install_root = (
            self.home / "Library" / "Application Support" / "LeGrin" / "JcodeLiteFree"
        )
        install_root.mkdir(parents=True)
        state = install_root / "state.json"
        state.write_text('{"current":"old-version","previous":""}\n', encoding="utf-8")
        binary = self.package / "bin" / "jcode"
        binary.write_text("#!/bin/bash\nexit 23\n", encoding="utf-8")
        binary.chmod(0o755)

        result = subprocess.run(
            [str(self.package / "install.command")],
            cwd=self.package,
            env={**self.env, "JCODE_LITE_FREE_INSTALL_NO_LAUNCH": "1"},
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(json.loads(state.read_text())["current"], "old-version")
        self.assertFalse((install_root / self.version).exists())
        self.assertEqual(list(install_root.glob(".staging-*")), [])


if __name__ == "__main__":
    unittest.main()
