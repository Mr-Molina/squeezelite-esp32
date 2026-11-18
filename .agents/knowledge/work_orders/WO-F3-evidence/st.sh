#!/bin/bash
curl -s -m 6 http://172.25.6.102/status.json | python -c "import json,sys; d=json.load(sys.stdin); print({k:d.get(k) for k in ['uptime_ms','frames_played','codec','codec_sample_rate','vol_pct','decode_us_max','decode_us_first','int_heap_free_bytes','int_heap_min_free_bytes','psram_free_bytes','i2c_nacks','tas_automuted','disconnect_count','amp_mode']})"
