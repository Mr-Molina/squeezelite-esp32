"""Sample DEVBENCH /status.json every 10 s; write a CSV and a summary.
usage: monitor.py <seconds> <out.csv>"""
import json, sys, time, urllib.request
dur = float(sys.argv[1]); out = sys.argv[2]
def get():
    for _ in range(3):
        try:
            with urllib.request.urlopen("http://172.25.6.102/status.json", timeout=5) as r:
                return json.load(r)
        except Exception as e:
            err = e; time.sleep(1)
    return None
rows = []; t0 = time.time(); misses = 0
with open(out, "w") as f:
    f.write("t_s,uptime_ms,frames_played,fps_10s,codec,decode_us_max,int_heap_free,int_heap_min_free,psram_free,disconnects,i2c_nacks,vol\n")
    prev = None
    while time.time() - t0 < dur:
        d = get()
        if d is None:
            misses += 1; f.write(f"{time.time()-t0:.0f},MISS\n"); f.flush(); time.sleep(10); continue
        fps = ""
        if prev and d["uptime_ms"] > prev["uptime_ms"]:
            fps = (d["frames_played"] - prev["frames_played"]) * 1000.0 / (d["uptime_ms"] - prev["uptime_ms"])
        rows.append((d["uptime_ms"], d["frames_played"], d["codec"], d["decode_us_max"], d.get("int_heap_free_bytes"), d.get("int_heap_min_free_bytes")))
        f.write(f"{time.time()-t0:.0f},{d['uptime_ms']},{d['frames_played']},{fps if fps=='' else round(fps,1)},{d['codec']},{d['decode_us_max']},{d.get('int_heap_free_bytes')},{d.get('int_heap_min_free_bytes')},{d['psram_free_bytes']},{d['disconnect_count']},{d['i2c_nacks']},{d['vol_pct']}\n"); f.flush()
        prev = d
        time.sleep(10)
# summary: overall rate, worst 60 s window (6 samples apart), resets (uptime going backwards)
resets = sum(1 for a, b in zip(rows, rows[1:]) if b[0] < a[0])
overall = (rows[-1][1] - rows[0][1]) * 1000.0 / (rows[-1][0] - rows[0][0]) if len(rows) > 1 and not resets else float("nan")
w = [(rows[i+6][1] - rows[i][1]) * 1000.0 / (rows[i+6][0] - rows[i][0]) for i in range(len(rows) - 6)]
print(f"samples={len(rows)} misses={misses} resets={resets} overall_fps={overall:.2f}")
if w: print(f"60s windows: min={min(w):.1f} max={max(w):.1f} n={len(w)}")
print("codecs:", sorted(set(r[2] for r in rows)), "decode_us_max peak:", max(r[3] for r in rows))
print("int_heap_free min sample:", min(r[4] for r in rows if r[4] is not None), "reported low-water:", rows[-1][5])
