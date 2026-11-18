# F3 soak evidence (raw, copied 2026-11-18)

Raw files from the F3 (`foundation/memory-rebudget`) playback checks on
DEVBENCH, copied unchanged from the F3 session's temporary folder so they are
not lost. They were copied by a different session, which did not run the
checks. F3's own work-order file does not exist yet.

| File | What it is |
|---|---|
| `soak-i1.csv` | One-hour soak, 2026-11-18 18:15 to 19:16 UTC, one sample every 10 s from `/status.json` |
| `slim-events.log` | SlimProto events seen by the relay during the baseline runs and the soak |
| `soak.sh` | The script that drove the soak (playlist, volume, monitor) |
| `base-mp3.csv`, `base-flac.csv`, `base-aac.csv` | Short per-codec runs before the soak |
| `monitor.py`, `st.sh` | Poll DEVBENCH `/status.json` (`172.25.6.102`) and write the CSV |
| `lms.sh` | Send one JSON-RPC command to the LMS at `172.25.8.40:9000` for the DEVBENCH player |
| `relay.py`, `listen.py` | The TCP relay on the PC through which DEVBENCH reached the LMS |

How to read `soak-i1.csv`, and what it shows (two device restarts, no AAC
samples), is in `docs/mk5-rust-handoff.md`, section 0.4.

These are one-off scripts, kept as found and not reviewed. The serial capture
script and the audio test files (`soak.mp3`, `soak.flac`, `soak.m4a`, served
from the PC on port 8123) were not copied.
