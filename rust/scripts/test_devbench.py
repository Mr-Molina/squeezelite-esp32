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

class ReviewFixTests(unittest.TestCase):
    """Cases added after review; the seven above are the plan's."""

    def setUp(self):
        self.lock = Path(tempfile.mkdtemp()) / "dev-module.lock"

    def test_attempt_is_refused_when_lock_changed_hands(self):
        devbench.acquire(self.lock, "feat/a", T0)
        devbench.acquire(self.lock, "feat/b", T0 + timedelta(hours=3))
        with self.assertRaises(devbench.LockNotHeld):
            devbench.record_attempt(self.lock, "feat/a")
        self.assertEqual(devbench.record_attempt(self.lock, "feat/b"), 1)

    def test_mutation_waits_for_the_guard(self):
        devbench.acquire(self.lock, "feat/a", T0)
        guard = self.lock.with_name(self.lock.name + ".guard")
        guard.write_text("held by another process", encoding="utf-8")
        with self.assertRaises(devbench.GuardBusy):
            devbench.acquire(self.lock, "feat/b", T0 + timedelta(hours=3), guard_timeout=0.2)
        guard.unlink()
        self.assertEqual(devbench.acquire(self.lock, "feat/b", T0 + timedelta(hours=3)), "takeover")
        self.assertFalse(guard.exists())

    def test_timestamp_without_offset_is_read_as_utc(self):
        self.lock.write_text(
            '{"branch": "feat/a", "timestamp": "2026-11-17T12:00:00", "attempts": 0}',
            encoding="utf-8",
        )
        self.assertEqual(devbench.acquire(self.lock, "feat/b", T0 + timedelta(minutes=5)), "busy")

class OtadataTests(unittest.TestCase):
    def test_select_sector_matches_the_firmware_entry(self):
        # Same 32 bytes as OtaSelectEntry::new_ota_0().to_bytes() in ota_safety.rs.
        sector = devbench.otadata_ota0_sector()
        self.assertEqual(len(sector), 0x1000)
        self.assertEqual(sector[0:4], bytes([1, 0, 0, 0]))
        self.assertEqual(sector[4:24], b"\xff" * 20)
        self.assertEqual(sector[24:28], bytes([2, 0, 0, 0]))
        self.assertEqual(int.from_bytes(sector[28:32], "little"), 0x4743989A)
        self.assertEqual(sector[32:], b"\xff" * (0x1000 - 32))
