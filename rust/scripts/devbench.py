#!/usr/bin/env python3
"""Lock and guarded flash helper for the shared test board DEVBENCH.

DEVBENCH is the Louder-ESP32 on COM13 of the owner's Windows PC. Agents may
flash it without asking, one at a time. This script is the only way they do:

    python rust/scripts/devbench.py acquire <branch>
    python rust/scripts/devbench.py flash <branch> <image.bin>
    python rust/scripts/devbench.py select-ota0 <branch> [--no-reset]
    python rust/scripts/devbench.py release <branch>
    python rust/scripts/devbench.py check-mac <mac>

The lock is `dev-module.lock` in the shared git directory, which every
worktree on the PC sees and which is never committed. It holds the branch, a
UTC timestamp and the number of flash attempts made under it (at most two).
One lock is one check: `release` ends it, and the next `acquire` starts a new
count.

`flash` writes only the application image to `ota_0`, and only to the board
whose MAC is `DEVBENCH_MAC`. The live device BASEMENT and the CYD display
board use the same kind of USB serial adapter; their MACs are refused by name.

Exit codes: 0 success, 1 espflash or file error, 2 usage, 3 lock not
available or not held, 4 wrong board, 5 attempt limit reached.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import zlib
from contextlib import contextmanager
from datetime import UTC, datetime, timedelta
from pathlib import Path

# Read from the module with `espflash board-info --port COM13` on 2026-11-17
# (work order WO-F1-dev-module-bringup).
DEVBENCH_MAC = "ac:15:18:fd:93:9c"
BASEMENT_MAC = "c8:2e:18:be:b8:34"
CYD_MAC = "d4:d4:da:5a:e7:3c"

PORT = "COM13"
# `ota_0` as read from DEVBENCH's own partition table on 2026-11-17
# (WO-F1-dev-module-bringup): offset 0x150000, size 0x2A0000. BASEMENT's
# `ota_0` is recorded as 0x160000 / 0x290000, so the size limit here is the
# smaller of the two: an image that fits DEVBENCH must also fit BASEMENT.
# If the owner has the module repartitioned to BASEMENT's layout, change the
# offset to 0x160000.
OTA_0_OFFSET = 0x150000
OTA_0_SIZE = 0x290000

LOCK_NAME = "dev-module.lock"
STALE_AFTER = timedelta(hours=2)
MAX_ATTEMPTS = 2
# Every read-check-write of the lock runs under a short-lived guard file, so
# two agents cannot both act on the same reading of it.
GUARD_TIMEOUT_S = 10.0
GUARD_STALE_S = 60.0

EXIT_OK = 0
EXIT_FAILED = 1
EXIT_LOCK = 3
EXIT_MAC = 4
EXIT_ATTEMPTS = 5

MAC_LINE = re.compile(r"MAC address:\s*([0-9A-Fa-f:]{17})")


OTADATA_OFFSET = 0xD000
SECTOR = 0x1000

ESP_IMAGE_MIN_SIZE = 0x120
ESP_IMAGE_MAGIC = 0xE9
ESP_APP_DESC_MAGIC_OFFSET = 0x20
ESP_APP_DESC_MAGIC = b"\x32\x54\xcd\xab"  # 0xABCD5432 little-endian

GIT_TIMEOUT_S = 10
ESPFLASH_INFO_TIMEOUT_S = 10
ESPFLASH_FLASH_TIMEOUT_S = 120


def otadata_ota0_sector() -> bytes:
    """One `otadata` sector that makes the bootloader start `ota_0`.

    The 32-byte entry is the one the firmware writes itself
    (`OtaSelectEntry::new_ota_0()` in `ota_safety.rs`): sequence 1, state
    OTA_IMG_VALID, CRC over the sequence. The rest of the sector is erased.
    """
    seq = (1).to_bytes(4, "little")
    crc = zlib.crc32(seq, 0xFFFFFFFF) & 0xFFFFFFFF  # ESP-IDF esp_crc32_le(~0, seq)
    entry = seq + b"\xff" * 20 + (2).to_bytes(4, "little") + crc.to_bytes(4, "little")
    return entry + b"\xff" * (SECTOR - len(entry))


class TooManyAttempts(Exception):
    """A third flash attempt was asked for under one lock."""


class LockNotHeld(Exception):
    """An operation needed the lock and the caller does not hold it."""


class GuardBusy(Exception):
    """Another process is changing the lock right now."""


@contextmanager
def _guard(lock_path: Path, timeout: float):
    """Hold `<lock>.guard` for the duration of one lock operation.

    The guard is held for milliseconds. One older than GUARD_STALE_S was left
    by a process that died, and is removed.
    """
    guard = lock_path.with_name(lock_path.name + ".guard")
    deadline = time.monotonic() + timeout
    while True:
        try:
            with open(guard, "x", encoding="utf-8") as f:
                f.write(str(os.getpid()))
            break
        except FileExistsError:
            try:
                if time.time() - guard.stat().st_mtime > GUARD_STALE_S:
                    guard.unlink()
                    continue
            except FileNotFoundError:
                continue
            except PermissionError:
                pass
        except PermissionError:
            pass  # Windows: the other process is deleting it right now.
        if time.monotonic() >= deadline:
            raise GuardBusy(f"{guard} is held by another process")
        time.sleep(0.05)
    try:
        yield
    finally:
        try:
            guard.unlink()
        except FileNotFoundError:
            pass


def _read(lock_path: Path):
    """Return the lock's contents, or None when there is no lock.

    A lock that cannot be parsed still counts as held: its holder is unknown
    and its age comes from the file's modification time.
    """
    try:
        text = lock_path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return None
    try:
        data = json.loads(text)
        stamp = datetime.fromisoformat(data["timestamp"])
        if stamp.tzinfo is None:
            stamp = stamp.replace(tzinfo=UTC)
        return {
            "branch": str(data["branch"]),
            "timestamp": stamp,
            "attempts": max(0, int(data.get("attempts", 0))),
        }
    except (ValueError, KeyError, TypeError):
        mtime = datetime.fromtimestamp(lock_path.stat().st_mtime, tz=UTC)
        return {"branch": "<unreadable lock>", "timestamp": mtime, "attempts": 0}


def _payload(branch: str, now: datetime, attempts: int) -> str:
    stamp = now.astimezone(UTC).isoformat()
    return json.dumps({"branch": branch, "timestamp": stamp, "attempts": attempts})


def _create(lock_path: Path, branch: str, now: datetime) -> bool:
    """Create the lock; False when another process created it first."""
    try:
        with open(lock_path, "x", encoding="utf-8") as f:
            f.write(_payload(branch, now, 0))
        return True
    except FileExistsError:
        return False


def _rewrite(lock_path: Path, branch: str, now: datetime, attempts: int) -> None:
    tmp = lock_path.with_name(lock_path.name + ".tmp")
    tmp.write_text(_payload(branch, now, attempts), encoding="utf-8")
    os.replace(tmp, lock_path)


def _acquire(lock_path: Path, branch: str, now: datetime):
    """Return (status, previous holder or None). Caller holds the guard."""
    held = _read(lock_path)
    if held is None:
        return ("ok", None) if _create(lock_path, branch, now) else ("busy", None)
    if held["branch"] == branch:
        # Same holder: refresh the timestamp, keep the attempt count, so
        # re-acquiring is not a way round the two-attempt limit.
        _rewrite(lock_path, branch, now, held["attempts"])
        return "ok", None
    if now - held["timestamp"] <= STALE_AFTER:
        return "busy", held["branch"]
    # Stale: replace it. The guard makes this read-and-replace atomic, so of
    # two agents taking over at the same moment the second sees a fresh lock.
    _rewrite(lock_path, branch, now, 0)
    return "takeover", held["branch"]


def acquire(
    lock_path: Path, branch: str, now: datetime, guard_timeout: float = GUARD_TIMEOUT_S
) -> str:
    """Take the lock for `branch`. Returns "ok", "busy" or "takeover"."""
    with _guard(lock_path, guard_timeout):
        return _acquire(lock_path, branch, now)[0]


def release(lock_path: Path, branch: str, guard_timeout: float = GUARD_TIMEOUT_S) -> bool:
    """Remove the lock when `branch` holds it. False otherwise."""
    with _guard(lock_path, guard_timeout):
        held = _read(lock_path)
        if held is None or held["branch"] != branch:
            return False
        lock_path.unlink()
        return True


def holder(lock_path: Path, guard_timeout: float = GUARD_TIMEOUT_S):
    """The branch holding the lock, or None."""
    with _guard(lock_path, guard_timeout):
        held = _read(lock_path)
    return held["branch"] if held else None


def record_attempt(
    lock_path: Path, branch: str | None = None, guard_timeout: float = GUARD_TIMEOUT_S
) -> int:
    """Count one flash attempt under the current lock and return the count.

    Raises TooManyAttempts instead of counting a third. With `branch`, raises
    LockNotHeld unless that branch still holds the lock.
    """
    with _guard(lock_path, guard_timeout):
        held = _read(lock_path)
        if held is None:
            raise LockNotHeld("no dev-module lock")
        if branch is not None and held["branch"] != branch:
            raise LockNotHeld(f"dev-module lock is held by {held['branch']}, not {branch}")
        if held["attempts"] >= MAX_ATTEMPTS:
            raise TooManyAttempts(
                f"{held['attempts']} flash attempts already made under this lock"
            )
        attempts = held["attempts"] + 1
        _rewrite(lock_path, held["branch"], held["timestamp"], attempts)
        return attempts


def _normalize(mac: str) -> str:
    return mac.strip().lower().replace("-", ":")


def mac_allowed(mac: str) -> bool:
    """True only for the recorded DEVBENCH MAC."""
    return _normalize(mac) == DEVBENCH_MAC


def mac_refusal(mac: str) -> str:
    """Why `mac` may not be flashed."""
    m = _normalize(mac)
    if m == BASEMENT_MAC:
        return f"REFUSED: {m} is BASEMENT, the live device. It is never flashed."
    if m == CYD_MAC:
        return f"REFUSED: {m} is the CYD display board, not the dev module."
    return f"REFUSED: {m} is not DEVBENCH ({DEVBENCH_MAC}). Wrong board on {PORT}?"


def default_lock_path() -> Path:
    here = Path(__file__).resolve().parent
    out = subprocess.run(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=here,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=GIT_TIMEOUT_S,
    ).stdout.strip()
    common = Path(out)
    if not common.is_absolute():
        common = here / common
    return common.resolve() / LOCK_NAME


def read_board_mac() -> str:
    """Ask the board on PORT for its MAC. Resets it into its bootloader."""
    try:
        proc = subprocess.run(
            ["espflash", "board-info", "--port", PORT, "--non-interactive"],
            check=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=ESPFLASH_INFO_TIMEOUT_S,
        )
    except subprocess.TimeoutExpired as err:
        raise RuntimeError(
            f"espflash board-info timed out after {ESPFLASH_INFO_TIMEOUT_S}s"
        ) from err
    output = proc.stdout + proc.stderr
    if proc.returncode != 0:
        raise RuntimeError(f"espflash board-info failed:\n{output}")
    macs = {_normalize(m) for m in MAC_LINE.findall(output)}
    if len(macs) != 1:
        raise RuntimeError(f"expected exactly one MAC from espflash board-info:\n{output}")
    return macs.pop()


def cmd_acquire(lock_path: Path, branch: str) -> int:
    with _guard(lock_path, GUARD_TIMEOUT_S):
        status, previous = _acquire(lock_path, branch, datetime.now(UTC))
    if status == "busy":
        print(f"BUSY: dev module is held by {previous or 'another agent'}")
        return EXIT_LOCK
    if status == "takeover":
        print(f"TAKEOVER from {previous}")
    print(f"acquired {lock_path} for {branch}")
    return EXIT_OK


def cmd_release(lock_path: Path, branch: str) -> int:
    if release(lock_path, branch):
        print(f"released {lock_path}")
        return EXIT_OK
    print(f"NOT RELEASED: lock is held by {holder(lock_path) or 'nobody'}, not {branch}")
    return EXIT_LOCK


def cmd_check_mac(mac: str) -> int:
    if mac_allowed(mac):
        print(f"OK: {_normalize(mac)} is DEVBENCH")
        return EXIT_OK
    print(mac_refusal(mac))
    return EXIT_MAC


def validate_firmware_image(image: Path) -> str | None:
    """Return an error reason if `image` is not a valid ESP32 app binary."""
    if not image.is_file():
        return f"no such image {image}"
    size = image.stat().st_size
    if size > OTA_0_SIZE:
        return f"image is {size} B, ota_0 holds {OTA_0_SIZE} B"
    if size < ESP_IMAGE_MIN_SIZE:
        return f"image is {size} B, minimum valid ESP32 app image is {ESP_IMAGE_MIN_SIZE} B"
    try:
        with image.open("rb") as f:
            header = f.read(ESP_APP_DESC_MAGIC_OFFSET + len(ESP_APP_DESC_MAGIC))
    except OSError as err:
        return f"cannot read {image}: {err}"
    if len(header) < ESP_APP_DESC_MAGIC_OFFSET + len(ESP_APP_DESC_MAGIC):
        return f"truncated image header in {image}"
    if header[0] != ESP_IMAGE_MAGIC:
        return (
            f"invalid ESP32 image magic 0x{header[0]:02x} in {image} "
            f"(expected 0x{ESP_IMAGE_MAGIC:02x})"
        )
    app_magic = header[
        ESP_APP_DESC_MAGIC_OFFSET : ESP_APP_DESC_MAGIC_OFFSET + len(ESP_APP_DESC_MAGIC)
    ]
    if app_magic != ESP_APP_DESC_MAGIC:
        return (
            f"invalid esp_app_desc magic 0x{int.from_bytes(app_magic, 'little'):08x} in {image} "
            f"(expected 0x{int.from_bytes(ESP_APP_DESC_MAGIC, 'little'):08x})"
        )
    return None


def cmd_flash(lock_path: Path, branch: str, image: Path) -> int:
    current = holder(lock_path)
    if current != branch:
        print(
            f"NOT FLASHED: lock is held by {current or 'nobody'}, not {branch}. "
            "Run acquire first."
        )
        return EXIT_LOCK
    err = validate_firmware_image(image)
    if err is not None:
        print(f"NOT FLASHED: {err}")
        return EXIT_FAILED
    try:
        mac = read_board_mac()
    except RuntimeError as err:
        print(f"NOT FLASHED: {err}")
        return EXIT_FAILED
    if not mac_allowed(mac):
        print(mac_refusal(mac))
        return EXIT_MAC
    try:
        # Checks the holder again: the lock may have changed hands while
        # espflash was talking to the board.
        attempt = record_attempt(lock_path, branch)
    except LockNotHeld as err:
        print(f"NOT FLASHED: {err}")
        return EXIT_LOCK
    except TooManyAttempts as err:
        print(f"NOT FLASHED: {err}. Release the lock, restore the last good image and report.")
        return EXIT_ATTEMPTS
    print(f"flash attempt {attempt} of {MAX_ATTEMPTS}: {image} -> {PORT} @ {OTA_0_OFFSET:#x}")
    try:
        proc = subprocess.run(
            [
                "espflash",
                "write-bin",
                "--port",
                PORT,
                "--non-interactive",
                f"{OTA_0_OFFSET:#x}",
                str(image),
            ],
            check=False,
            timeout=ESPFLASH_FLASH_TIMEOUT_S,
        )
    except subprocess.TimeoutExpired:
        print(f"NOT FLASHED: espflash write-bin timed out after {ESPFLASH_FLASH_TIMEOUT_S}s")
        return EXIT_FAILED
    return EXIT_OK if proc.returncode == 0 else EXIT_FAILED


def cmd_select_ota0(lock_path: Path, branch: str, no_reset: bool) -> int:
    """Write the `ota_0` select entry, so the bootloader leaves C recovery.

    Needed after the first install and whenever the module has fallen back to
    recovery: the firmware erases `otadata` early in boot and only pins
    `ota_0` again once the boot lease has passed.
    """
    current = holder(lock_path)
    if current != branch:
        print(
            f"NOT WRITTEN: lock is held by {current or 'nobody'}, not {branch}. "
            "Run acquire first."
        )
        return EXIT_LOCK
    try:
        mac = read_board_mac()
    except RuntimeError as err:
        print(f"NOT WRITTEN: {err}")
        return EXIT_FAILED
    if not mac_allowed(mac):
        print(mac_refusal(mac))
        return EXIT_MAC
    if holder(lock_path) != branch:
        print("NOT WRITTEN: the lock changed hands while talking to the board")
        return EXIT_LOCK
    with tempfile.TemporaryDirectory() as tmp:
        blob = Path(tmp) / "otadata-ota0.bin"
        blob.write_bytes(otadata_ota0_sector())
        command = ["espflash", "write-bin", "--port", PORT, "--non-interactive"]
        if no_reset:
            command += ["--after", "no-reset"]
        command += [f"{OTADATA_OFFSET:#x}", str(blob)]
        print(f"select ota_0: {SECTOR} B -> {PORT} @ {OTADATA_OFFSET:#x}")
        try:
            proc = subprocess.run(command, check=False, timeout=ESPFLASH_FLASH_TIMEOUT_S)
        except subprocess.TimeoutExpired:
            print(f"NOT WRITTEN: espflash write-bin timed out after {ESPFLASH_FLASH_TIMEOUT_S}s")
            return EXIT_FAILED
    return EXIT_OK if proc.returncode == 0 else EXIT_FAILED


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("acquire", "release"):
        p = sub.add_parser(name)
        p.add_argument("branch")
    p = sub.add_parser("check-mac")
    p.add_argument("mac")
    p = sub.add_parser("flash")
    p.add_argument("branch")
    p.add_argument("image", type=Path)
    p = sub.add_parser("select-ota0")
    p.add_argument("branch")
    p.add_argument(
        "--no-reset",
        action="store_true",
        help="leave the chip in its serial bootloader afterwards, so the next "
        "reset is the caller's and a log capture sees the whole boot",
    )
    args = parser.parse_args(argv)

    if args.command == "check-mac":
        return cmd_check_mac(args.mac)
    lock_path = default_lock_path()
    try:
        if args.command == "acquire":
            return cmd_acquire(lock_path, args.branch)
        if args.command == "release":
            return cmd_release(lock_path, args.branch)
        if args.command == "select-ota0":
            return cmd_select_ota0(lock_path, args.branch, args.no_reset)
        return cmd_flash(lock_path, args.branch, args.image)
    except GuardBusy as err:
        print(f"BUSY: {err}. Try again.")
        return EXIT_LOCK


if __name__ == "__main__":
    sys.exit(main())
