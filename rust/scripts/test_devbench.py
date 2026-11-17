# rust/scripts/test_devbench.py
import tempfile, unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
import devbench

T0 = datetime(2026, 11, 17, 12, 0, tzinfo=timezone.utc)

class LockTests(unittest.TestCase):
    def setUp(self):
        self.lock = Path(tempfile.mkdtemp()) / "dev-module.lock"

    def test_second_branch_is_busy(self):
        self.assertEqual(devbench.acquire(self.lock, "feat/a", T0), "ok")
        self.assertEqual(devbench.acquire(self.lock, "feat/b", T0 + timedelta(minutes=5)), "busy")

    def test_same_branch_reacquires(self):
        devbench.acquire(self.lock, "feat/a", T0)
        self.assertEqual(devbench.acquire(self.lock, "feat/a", T0 + timedelta(minutes=5)), "ok")

    def test_stale_lock_is_taken_over(self):
        devbench.acquire(self.lock, "feat/a", T0)
        self.assertEqual(devbench.acquire(self.lock, "feat/b", T0 + timedelta(hours=2, seconds=1)), "takeover")

    def test_release_only_by_holder(self):
        devbench.acquire(self.lock, "feat/a", T0)
        self.assertFalse(devbench.release(self.lock, "feat/b"))
        self.assertTrue(devbench.release(self.lock, "feat/a"))
        self.assertFalse(self.lock.exists())

    def test_third_attempt_is_refused(self):
        devbench.acquire(self.lock, "feat/a", T0)
        self.assertEqual(devbench.record_attempt(self.lock), 1)
        self.assertEqual(devbench.record_attempt(self.lock), 2)
        with self.assertRaises(devbench.TooManyAttempts):
            devbench.record_attempt(self.lock)

class MacTests(unittest.TestCase):
    def test_basement_and_cyd_are_refused(self):
        self.assertFalse(devbench.mac_allowed("c8:2e:18:be:b8:34"))
        self.assertFalse(devbench.mac_allowed("D4:D4:DA:5A:E7:3C"))

    def test_only_recorded_mac_is_allowed(self):
        self.assertTrue(devbench.mac_allowed(devbench.DEVBENCH_MAC.upper()))
        self.assertFalse(devbench.mac_allowed("00:11:22:33:44:55"))
