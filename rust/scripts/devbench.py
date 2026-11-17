#!/usr/bin/env python3
"""Lock and guarded flash helper for the shared test board DEVBENCH.

DEVBENCH is the Louder-ESP32 on COM13 of the owner's Windows PC. Agents may
flash it without asking, one at a time. This script is the only way they do:

    python rust/scripts/devbench.py acquire <branch>
    python rust/scripts/devbench.py flash <branch> <image.bin>
    python rust/scripts/devbench.py release <branch>
    python rust/scripts/devbench.py check-mac <mac>

The lock is `dev-module.lock` in the shared git directory, which every
worktree on the PC sees and which is never committed. It holds the branch, a
UTC timestamp and the number of flash attempts made under it (at most two).

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
from datetime import datetime, timedelta, timezone
from pathlib import Path

# Read from the module with `espflash board-info --port COM13` on 2026-11-17
# (work order WO-F1-dev-module-bringup).
DEVBENCH_MAC = "ac:15:18:fd:93:9c"
BASEMENT_MAC = "c8:2e:18:be:b8:34"
CYD_MAC = "d4:d4:da:5a:e7:3c"

PORT = "COM13"
# `ota_0` in the C partition layout (`partitions.csv`): recovery ends at
# 0x10000 + 0x150000.
OTA_0_OFFSET = 0x160000
OTA_0_SIZE = 0x290000

LOCK_NAME = "dev-module.lock"
STALE_AFTER = timedelta(hours=2)
MAX_ATTEMPTS = 2

EXIT_OK = 0
EXIT_FAILED = 1
EXIT_LOCK = 3
EXIT_MAC = 4
EXIT_ATTEMPTS = 5


class TooManyAttempts(Exception):
    """A third flash attempt was asked for under one lock."""


class LockNotHeld(Exception):
    """An operation needed the lock and there is none."""


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
        return {
            "branch": str(data["branch"]),
            "timestamp": datetime.fromisoformat(data["timestamp"]),
            "attempts": int(data.get("attempts", 0)),
        }
    except (ValueError, KeyError, TypeError):
        mtime = datetime.fromtimestamp(lock_path.stat().st_mtime, tz=timezone.utc)
        return {"branch": "<unreadable lock>", "timestamp": mtime, "attempts": 0}


def _payload(branch: str, now: datetime, attempts: int) -> str:
    stamp = now.astimezone(timezone.utc).isoformat()
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
    """Return (status, previous holder or None)."""
    if _create(lock_path, branch, now):
        return "ok", None
    held = _read(lock_path)
    if held is None:
        # Released between our create and our read. One more try.
        return ("ok", None) if _create(lock_path, branch, now) else ("busy", None)
    if held["branch"] == branch:
        # Same holder: refresh the timestamp, keep the attempt count, so
        # re-acquiring is not a way round the two-attempt limit.
        _rewrite(lock_path, branch, now, held["attempts"])
        return "ok", None
    if now - held["timestamp"] <= STALE_AFTER:
        return "busy", held["branch"]
    # Stale. Remove it and create ours exclusively, so that of two agents
    # taking over at the same moment only one succeeds.
    try:
        lock_path.unlink()
    except FileNotFoundError:
        pass
    if _create(lock_path, branch, now):
        return "takeover", held["branch"]
    return "busy", held["branch"]


def acquire(lock_path: Path, branch: str, now: datetime) -> str:
    """Take the lock for `branch`. Returns "ok", "busy" or "takeover"."""
    return _acquire(lock_path, branch, now)[0]


def release(lock_path: Path, branch: str) -> bool:
    """Remove the lock when `branch` holds it. False otherwise."""
    held = _read(lock_path)
    if held is None or held["branch"] != branch:
        return False
    lock_path.unlink()
    return True


def record_attempt(lock_path: Path) -> int:
    """Count one flash attempt under the current lock and return the count.

    Raises TooManyAttempts instead of counting a third.
    """
    held = _read(lock_path)
    if held is None:
        raise LockNotHeld("no dev-module lock")
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
    ).stdout.strip()
    common = Path(out)
    if not common.is_absolute():
        common = here / common
    return common.resolve() / LOCK_NAME


def read_board_mac() -> str:
    """Ask the board on PORT for its MAC. Resets it into its bootloader."""
    proc = subprocess.run(
        ["espflash", "board-info", "--port", PORT, "--non-interactive"],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"espflash board-info failed:\n{proc.stdout}{proc.stderr}")
    match = re.search(r"MAC address:\s*([0-9A-Fa-f:]{17})", proc.stdout)
    if not match:
        raise RuntimeError(f"no MAC in espflash board-info output:\n{proc.stdout}")
    return _normalize(match.group(1))


def cmd_acquire(lock_path: Path, branch: str) -> int:
    status, previous = _acquire(lock_path, branch, datetime.now(timezone.utc))
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
    held = _read(lock_path)
    holder = held["branch"] if held else "nobody"
    print(f"NOT RELEASED: lock is held by {holder}, not {branch}")
    return EXIT_LOCK


def cmd_check_mac(mac: str) -> int:
    if mac_allowed(mac):
        print(f"OK: {_normalize(mac)} is DEVBENCH")
        return EXIT_OK
    print(mac_refusal(mac))
    return EXIT_MAC


def cmd_flash(lock_path: Path, branch: str, image: Path) -> int:
    held = _read(lock_path)
    if held is None or held["branch"] != branch:
        holder = held["branch"] if held else "nobody"
        print(f"NOT FLASHED: lock is held by {holder}, not {branch}. Run acquire first.")
        return EXIT_LOCK
    if not image.is_file():
        print(f"NOT FLASHED: no such image {image}")
        return EXIT_FAILED
    size = image.stat().st_size
    if size > OTA_0_SIZE:
        print(f"NOT FLASHED: image is {size} B, ota_0 holds {OTA_0_SIZE} B")
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
        attempt = record_attempt(lock_path)
    except TooManyAttempts as err:
        print(f"NOT FLASHED: {err}. Release the lock, restore the last good image and report.")
        return EXIT_ATTEMPTS
    print(f"flash attempt {attempt} of {MAX_ATTEMPTS}: {image} -> {PORT} @ {OTA_0_OFFSET:#x}")
    proc = subprocess.run(
        [
            "espflash",
            "write-bin",
            "--port",
            PORT,
            "--non-interactive",
            f"{OTA_0_OFFSET:#x}",
            str(image),
        ]
    )
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
    args = parser.parse_args(argv)

    if args.command == "check-mac":
        return cmd_check_mac(args.mac)
    lock_path = default_lock_path()
    if args.command == "acquire":
        return cmd_acquire(lock_path, args.branch)
    if args.command == "release":
        return cmd_release(lock_path, args.branch)
    return cmd_flash(lock_path, args.branch, args.image)


if __name__ == "__main__":
    sys.exit(main())
