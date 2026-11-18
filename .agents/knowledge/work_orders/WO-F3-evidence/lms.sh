#!/bin/bash
# lms.sh <json array of command terms>   e.g. lms.sh '["mixer","volume",8]'
curl -s -m 8 -H 'Content-Type: application/json' -d "{\"id\":1,\"method\":\"slim.request\",\"params\":[\"ac:15:18:fd:93:9c\",$1]}" http://172.25.8.40:9000/jsonrpc.js
echo
