"""Sample DEVBENCH /status.json every 10 s; write a CSV and a summary.

usage: monitor.py <seconds> <out.csv> [--url URL | --host HOST]
"""

import argparse
import json
import time
import urllib.request

DEFAULT_URL = "http://172.25.6.102/status.json"


def parse_args():
    parser = argparse.ArgumentParser(
        description="Sample /status.json every 10 s; write a CSV and a summary."
    )
    parser.add_argument("seconds", type=float, help="Sampling duration in seconds")
    parser.add_argument("out", help="Output CSV path")
    parser.add_argument(
        "--url",
        default=None,
        help=f"Full status.json URL (default: {DEFAULT_URL})",
    )
    parser.add_argument(
        "--host",
        default=None,
        help="Device host/IP to query at http://<host>/status.json",
    )
    return parser.parse_args()


def resolve_url(args):
    if args.url:
        return args.url
    if args.host:
        return f"http://{args.host}/status.json"
    return DEFAULT_URL


def get(url):
    for _ in range(3):
        try:
            with urllib.request.urlopen(url, timeout=5) as r:
                return json.load(r)
        except Exception:
            time.sleep(1)
    return None


def summarize(rows, misses):
    if not rows:
        print(f"samples=0 misses={misses} resets=0 overall_fps=nan")
        return

    resets = sum(
        1
        for a, b in zip(rows, rows[1:])
        if a[0] is not None and b[0] is not None and b[0] < a[0]
    )
    first_uptime, first_frames = rows[0][0], rows[0][1]
    last_uptime, last_frames = rows[-1][0], rows[-1][1]
    if (
        len(rows) > 1
        and not resets
        and first_uptime is not None
        and last_uptime is not None
        and first_frames is not None
        and last_frames is not None
        and last_uptime > first_uptime
    ):
        overall = (last_frames - first_frames) * 1000.0 / (last_uptime - first_uptime)
    else:
        overall = float("nan")

    w = [
        (rows[i + 6][1] - rows[i][1]) * 1000.0 / (rows[i + 6][0] - rows[i][0])
        for i in range(len(rows) - 6)
        if rows[i][0] is not None
        and rows[i + 6][0] is not None
        and rows[i][1] is not None
        and rows[i + 6][1] is not None
        and rows[i + 6][0] > rows[i][0]
    ]
    print(f"samples={len(rows)} misses={misses} resets={resets} overall_fps={overall:.2f}")
    if w:
        print(f"60s windows: min={min(w):.1f} max={max(w):.1f} n={len(w)}")
    codecs = sorted({r[2] for r in rows if r[2] is not None})
    decode_peak = max((r[3] for r in rows if r[3] is not None), default=None)
    min_heap = min((r[4] for r in rows if r[4] is not None), default=None)
    print("codecs:", codecs, "decode_us_max peak:", decode_peak)
    print("int_heap_free min sample:", min_heap, "reported low-water:", rows[-1][5])


def main():
    args = parse_args()
    dur = args.seconds
    out = args.out
    url = resolve_url(args)

    rows = []
    t0 = time.time()
    misses = 0
    with open(out, "w", encoding="utf-8", newline="") as f:
        f.write(
            "t_s,uptime_ms,frames_played,fps_10s,codec,decode_us_max,"
            "int_heap_free,int_heap_min_free,psram_free,disconnects,i2c_nacks,vol\n"
        )
        prev = None
        while time.time() - t0 < dur:
            d = get(url)
            if d is None:
                misses += 1
                f.write(f"{time.time() - t0:.0f},MISS,,,,,,,,,,\n")
                f.flush()
                time.sleep(10)
                continue
            uptime_ms = d.get("uptime_ms")
            frames_played = d.get("frames_played")
            codec = d.get("codec")
            decode_us_max = d.get("decode_us_max")
            int_heap_free = d.get("int_heap_free_bytes")
            int_heap_min_free = d.get("int_heap_min_free_bytes")
            psram_free = d.get("psram_free_bytes")
            disconnect_count = d.get("disconnect_count")
            i2c_nacks = d.get("i2c_nacks")
            vol_pct = d.get("vol_pct")

            fps = ""
            if (
                prev
                and uptime_ms is not None
                and prev.get("uptime_ms") is not None
                and frames_played is not None
                and prev.get("frames_played") is not None
                and uptime_ms > prev["uptime_ms"]
            ):
                fps = (
                    (frames_played - prev["frames_played"])
                    * 1000.0
                    / (uptime_ms - prev["uptime_ms"])
                )
            rows.append(
                (
                    uptime_ms,
                    frames_played,
                    codec,
                    decode_us_max,
                    int_heap_free,
                    int_heap_min_free,
                )
            )
            fps_str = "" if fps == "" else round(fps, 1)
            f.write(
                f"{time.time() - t0:.0f},{uptime_ms},{frames_played},{fps_str},"
                f"{codec},{decode_us_max},{int_heap_free},{int_heap_min_free},"
                f"{psram_free},{disconnect_count},{i2c_nacks},{vol_pct}\n"
            )
            f.flush()
            prev = d
            time.sleep(10)

    summarize(rows, misses)


if __name__ == "__main__":
    main()
