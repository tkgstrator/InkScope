"""Check the development watcher launches the local Visualizer control API."""

import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest


SOURCE = Path(__file__).resolve().parents[1] / "capture.sh"


class CaptureControlTest(unittest.TestCase):
    def test_sibling_visualizer_token_enables_control_listener(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            repo = root / "InkScope"
            (repo / "target" / "debug").mkdir(parents=True)
            (repo / "crates").mkdir()
            (root / "InkScope-Visualizer").mkdir()
            (root / "bin").mkdir()
            (repo / "capture.sh").write_bytes(SOURCE.read_bytes())
            dotenv = root / "InkScope-Visualizer" / ".env.local"
            dotenv.write_text(
                "INKSCOPE_VISUALIZER_TOKEN=test-ingest-token\n"
                "INKSCOPE_CONTROL_TOKEN=test-control-token-32-characters-minimum\n"
            )
            dotenv.chmod(0o600)
            game_key = root / "game_key.bin"
            game_key.write_bytes(bytes(16))
            cargo = root / "bin" / "cargo"
            cargo.write_text("#!/bin/sh\nexit 0\n")
            cargo.chmod(0o700)
            route = root / "bin" / "route"
            route.write_text("#!/bin/sh\nprintf 'interface: en13\\nflags: <UP,DONE,CLONING>\\n'\n")
            route.chmod(0o700)
            ifconfig = root / "bin" / "ifconfig"
            ifconfig.write_text("#!/bin/sh\nprintf '%s: flags=8863<UP,BROADCAST,RUNNING> mtu 1500\\n    inet 192.168.1.119 netmask 0xffffff00\\n    status: active\\n' \"$1\"\n")
            ifconfig.chmod(0o700)
            fake = repo / "target" / "debug" / "inkscope"
            fake.write_text(
                "#!/usr/bin/env python3\n"
                "import json, os, pathlib, signal, sys, time\n"
                "args = sys.argv[1:]\n"
                "out = pathlib.Path(args[args.index('--output') + 1])\n"
                "out.write_bytes(bytes(24))\n"
                "pathlib.Path(os.environ['CAPTURE_TEST_REPORT']).write_text(json.dumps({"
                "'args': args, 'control_token': os.environ.get('INKSCOPE_CONTROL_TOKEN')}))\n"
                "signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))\n"
                "while True: time.sleep(0.1)\n"
            )
            fake.chmod(0o700)
            report = root / "report.json"
            env = {
                **os.environ,
                "PATH": str(root / "bin") + os.pathsep + os.environ["PATH"],
                "INKSCOPE_CAPTURE_DIR": str(root / "captures"),
                "INKSCOPE_GAME_KEY_FILE": str(game_key),
                "INKSCOPE_PEER": "192.168.1.180",
                "INKSCOPE_MODE": "normal",
                "CAPTURE_TEST_REPORT": str(report),
            }
            env.pop("INKSCOPE_VISUALIZER_TOKEN", None)
            env.pop("INKSCOPE_CONTROL_TOKEN", None)
            env.pop("INKSCOPE_INTERFACE", None)
            process = subprocess.Popen(
                ["/bin/bash", str(repo / "capture.sh")], cwd=repo, env=env,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            try:
                deadline = time.monotonic() + 8
                while not report.exists() and time.monotonic() < deadline:
                    if process.poll() is not None:
                        self.fail("capture watcher exited before launching the capture")
                    time.sleep(0.1)
                self.assertTrue(report.exists(), "capture was not launched")
                invocation = json.loads(report.read_text())
                self.assertEqual(
                    invocation["control_token"],
                    "test-control-token-32-characters-minimum",
                )
                args = invocation["args"]
                self.assertEqual(args[args.index("--control-port") + 1], "10254")
                self.assertEqual(args[args.index("--interface") + 1], "en13")
            finally:
                if process.poll() is None:
                    process.send_signal(signal.SIGTERM)
                process.wait(timeout=8)


if __name__ == "__main__":
    unittest.main()
