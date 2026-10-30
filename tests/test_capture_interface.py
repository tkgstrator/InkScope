"""Exercise interface selection through the real launcher with isolated commands."""

import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "capture.sh"


class CaptureInterfaceTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / "InkScope"
        (self.repo / "crates").mkdir(parents=True)
        (self.repo / "target" / "debug").mkdir(parents=True)
        (self.root / "bin").mkdir()
        (self.repo / "capture.sh").write_bytes(SOURCE.read_bytes())
        self.route = self.root / "route.txt"
        self.links = self.root / "links.json"
        self.route.write_text("interface: en13\nflags: <UP,DONE,CLONING>\n")
        self.links.write_text(json.dumps({name: self.active(name) for name in ["en13", "en14"]}))
        self.command("route", "import os,pathlib,sys\ns=(pathlib.Path(os.environ['CAPTURE_TEST_ROOT'])/'route.txt').read_text()\nprint(s)\nsys.exit(1 if s == 'ERROR' else 0)\n")
        self.command("ifconfig", "import os,pathlib,json,sys\ns=json.loads((pathlib.Path(os.environ['CAPTURE_TEST_ROOT'])/'links.json').read_text()).get(sys.argv[-1])\nprint(s or '')\nsys.exit(0 if s else 1)\n")
        self.command("cargo", "import os,pathlib\n(pathlib.Path(os.environ['CAPTURE_TEST_ROOT'])/'built').touch()\n")
        fake = self.repo / "target" / "debug" / "inkscope"
        fake.write_text("#!/usr/bin/env python3\nimport json,os,pathlib,signal,sys,time\na=sys.argv[1:]\npathlib.Path(a[a.index('--output')+1]).write_bytes(bytes(24))\nwith open(pathlib.Path(os.environ['CAPTURE_TEST_ROOT'])/'calls.jsonl','a') as f: f.write(json.dumps({'args':a,'pid':os.getpid()})+'\\n')\nsignal.signal(signal.SIGTERM,lambda *_:sys.exit(0))\nwhile True: time.sleep(0.1)\n")
        fake.chmod(0o700)
        key = self.root / "game_key.bin"
        key.write_bytes(bytes(16))
        self.env = {**os.environ, "PATH": str(self.root / "bin") + os.pathsep + os.environ["PATH"],
                    "CAPTURE_TEST_ROOT": str(self.root), "INKSCOPE_CAPTURE_DIR": str(self.root / "captures"),
                    "INKSCOPE_GAME_KEY_FILE": str(key), "INKSCOPE_PEER": "192.168.1.180",
                    "INKSCOPE_MODE": "normal",
                    "INKSCOPE_VISUALIZER_TOKEN": "test-ingest-token", "INKSCOPE_CONTROL_TOKEN": "test-control-token-32-characters-minimum"}
        self.env.pop("INKSCOPE_INTERFACE", None)
        self.process = None
        self.log = (self.root / "watcher.log").open("w")
        self.addCleanup(self.cleanup_process)

    @staticmethod
    def active(name, address="192.168.1.119"):
        return f"{name}: flags=8863<UP,BROADCAST,RUNNING> mtu 1500\n    inet {address} netmask 0xffffff00\n    status: active\n"

    def command(self, name, body):
        path = self.root / "bin" / name
        path.write_text("#!/usr/bin/env python3\n" + body)
        path.chmod(0o700)

    def cleanup_process(self):
        if self.process is not None and self.process.poll() is None:
            self.process.send_signal(signal.SIGTERM)
            try:
                self.process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                os.killpg(self.process.pid, signal.SIGKILL)
                self.process.wait(timeout=3)
        self.log.close()

    def start(self, *args):
        self.process = subprocess.Popen(["/bin/bash", str(self.repo / "capture.sh"), *args],
                                        cwd=self.repo, env=self.env, stdout=self.log, stderr=self.log,
                                        start_new_session=True)

    def calls(self):
        path = self.root / "calls.jsonl"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def wait_for(self, predicate):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(0.05)
        self.fail((self.root / "watcher.log").read_text())

    def test_auto_selects_peer_route_instead_of_fixed_device(self):
        self.start()
        self.wait_for(lambda: len(self.calls()) == 1)
        args = self.calls()[0]["args"]
        self.assertEqual(args[args.index("--interface") + 1], "en13")

    def test_explicit_active_unnumbered_device_does_not_require_a_route(self):
        self.route.write_text("ERROR")
        self.links.write_text(json.dumps({"en18": "en18: flags=8863<UP,BROADCAST,RUNNING> mtu 1500\n    status: active\n"}))
        self.env["INKSCOPE_INTERFACE"] = "en18"
        self.start()
        self.wait_for(lambda: len(self.calls()) == 1)
        args = self.calls()[0]["args"]
        self.assertEqual(args[args.index("--interface") + 1], "en18")

    def assert_refused(self):
        self.start()
        self.wait_for(lambda: self.process.poll() is not None)
        self.assertNotEqual(self.process.returncode, 0)
        self.assertEqual(self.calls(), [])
        self.assertFalse((self.root / "built").exists())

    def test_inactive_route_is_rejected(self):
        self.links.write_text(json.dumps({"en13": self.active("en13").replace("status: active", "status: inactive")}))
        self.assert_refused()

    def test_inactive_explicit_device_is_not_silently_replaced(self):
        self.env["INKSCOPE_INTERFACE"] = "en18"
        self.links.write_text(json.dumps({"en13": self.active("en13"), "en18": self.active("en18").replace("status: active", "status: inactive")}))
        self.assert_refused()

    def test_missing_route_is_rejected(self):
        self.route.write_text("ERROR")
        self.assert_refused()

    def test_vpn_route_is_rejected(self):
        self.route.write_text("interface: utun3\nflags: <UP,DONE>\n")
        self.links.write_text(json.dumps({"utun3": self.active("utun3")}))
        self.assert_refused()

    def test_active_default_gateway_for_another_subnet_is_rejected(self):
        self.route.write_text("interface: en13\nflags: <UP,GATEWAY,DONE>\n")
        self.links.write_text(json.dumps({"en13": self.active("en13", "10.0.0.2")}))
        self.assert_refused()

    def test_gateway_route_is_rejected_even_on_the_same_subnet(self):
        self.route.write_text("interface: en13\nflags: <UP,GATEWAY,DONE>\n")
        self.assert_refused()

    def test_off_subnet_route_is_rejected_without_gateway_flag(self):
        self.links.write_text(json.dumps({"en13": self.active("en13", "10.0.0.2")}))
        self.assert_refused()

    def test_build_only_does_not_require_network(self):
        self.route.write_text("ERROR")
        self.start("--build-only")
        self.assertEqual(self.process.wait(timeout=3), 0)
        self.assertTrue((self.root / "built").exists())
        self.assertEqual(self.calls(), [])

    def test_failed_reselection_preserves_capture_then_retries_new_route(self):
        self.start()
        self.wait_for(lambda: len(self.calls()) == 1)
        old_pid = self.calls()[0]["pid"]
        self.route.write_text("ERROR")
        (self.repo / "crates" / "changed.rs").write_text("// trigger rebuild\n")
        self.wait_for(lambda: "Capture retry" in (self.root / "watcher.log").read_text())
        os.kill(old_pid, 0)
        self.assertEqual(len(self.calls()), 1)
        self.route.write_text("interface: en14\nflags: <UP,DONE,CLONING>\n")
        self.wait_for(lambda: len(self.calls()) == 2)
        args = self.calls()[1]["args"]
        self.assertEqual(args[args.index("--interface") + 1], "en14")


if __name__ == "__main__":
    unittest.main()
