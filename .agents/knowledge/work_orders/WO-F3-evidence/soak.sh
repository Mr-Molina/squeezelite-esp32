#!/bin/bash
# soak.sh <tag> <seconds>: one-hour mixed playback on DEVBENCH with monitoring.
S="$(dirname "$0")"; TAG=$1; SECS=$2
echo "=== soak $TAG start $(date +%H:%M:%S)" >> "$S/slim-events.log"
bash "$S/lms.sh" '["mixer","volume",8]' > /dev/null
bash "$S/lms.sh" '["playlist","repeat",2]' > /dev/null
bash "$S/lms.sh" '["playlist","play","http://172.25.6.122:8123/soak.mp3"]' > /dev/null
bash "$S/lms.sh" '["playlist","add","http://172.25.6.122:8123/soak.flac"]' > /dev/null
bash "$S/lms.sh" '["playlist","add","http://172.25.6.122:8123/soak.m4a"]' > /dev/null
sleep 10
bash "$S/st.sh"
python "$S/monitor.py" "$SECS" "$S/soak-$TAG.csv"
echo "=== soak $TAG end $(date +%H:%M:%S)" >> "$S/slim-events.log"
bash "$S/st.sh"
curl -s -m 6 http://172.25.6.102/messages.json | python -c "import json,sys; [print(m['uptime_ms'],m['level'],m['msg']) for m in json.load(sys.stdin)['messages']]"
echo "--- SlimProto events during the run"
sed -n "/=== soak $TAG start/,\$p" "$S/slim-events.log"
