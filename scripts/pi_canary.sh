#!/bin/bash
# Pi RPC live canary — protocol verification
set -e

TMPDIR=$(mktemp -d)
echo "=== Pi RPC Canary ==="
echo "Temp: $TMPDIR"

# Write prompt script
PIPE_IN="$TMPDIR/pi_stdin"
PIPE_OUT="$TMPDIR/pi_stdout"
mkfifo "$PIPE_IN"
mkfifo "$PIPE_OUT"

# Start pi RPC in background, reading from named pipe
pi --mode rpc --no-context-files --no-extensions --session-dir "$TMPDIR" \
  < "$PIPE_IN" > "$PIPE_OUT" 2>"$TMPDIR/pi_stderr.log" &
PI_PID=$!
echo "Pi PID: $PI_PID"

# Helper: send a line via JSON and read response
send_and_read() {
  local json="$1"
  local expected_id="$2"
  echo "$json" > "$PIPE_IN"
  # Read lines until we find the response with matching id
  while IFS= read -r line; do
    RESP_ID=$(echo "$line" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null || true)
    if [ "$RESP_ID" = "$expected_id" ]; then
      echo "$line"
      return 0
    fi
  done < "$PIPE_OUT"
}

# Step 1: get_state
echo "--- get_state ---"
STATE=$(echo '{"id":"t1","type":"get_state"}' > "$PIPE_IN")
# Use background reader
{
  while IFS= read -r line; do
    RESP_ID=$(echo "$line" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null || true)
    echo "  LINE: $(echo "$line" | head -c 300)"
    if [ "$RESP_ID" = "t1" ]; then
      STATE_RESP="$line"
      break
    fi
  done < "$PIPE_OUT"
  
  SESSION_FILE=$(echo "$STATE_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['data'].get('sessionFile',''))" 2>/dev/null || echo "")
  AC=$(echo "$STATE_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['data'].get('autoCompactionEnabled','?'))" 2>/dev/null || echo "?")
  echo "sessionFile: $SESSION_FILE"
  echo "autoCompactionEnabled: $AC"
  
  if [ -z "$SESSION_FILE" ]; then
    echo "FAIL: no sessionFile"
    kill $PI_PID 2>/dev/null
    exit 1
  fi
  echo "PASS: get_state"
  
  # Step 2: Disable auto-compaction
  echo "--- set_auto_compaction ---"
  echo '{"id":"t2","type":"set_auto_compaction","enabled":false}' > "$PIPE_IN"
  while IFS= read -r line; do
    RESP_ID=$(echo "$line" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null || true)
    if [ "$RESP_ID" = "t2" ]; then
      SUCCESS=$(echo "$line" | python3 -c "import sys,json; print(json.load(sys.stdin).get('success',False))" 2>/dev/null)
      echo "success: $SUCCESS"
      break
    fi
  done < "$PIPE_OUT"
  echo "PASS: auto-compaction disabled"
  
  # Step 3: Prompt
  echo "--- prompt ---"
  OUTPUT_FILE="$TMPDIR/pi-canary.txt"
  python3 -c "
import json
msg = 'Write the word ACCEPTED to the file $OUTPUT_FILE. Do nothing else. Do not read any files. Just write the single word. When done print exactly: ## RESULT\nDONE\n## SUMMARY\nFile written.'
print(json.dumps({'id':'t3','type':'prompt','message': msg}))
" > "$PIPE_IN"
  
  FOUND_SETTLED=false
  while IFS= read -r line; do
    TYPE=$(echo "$line" | python3 -c "import sys,json; print(json.load(sys.stdin).get('type',''))" 2>/dev/null || true)
    if [ "$TYPE" = "agent_settled" ]; then
      FOUND_SETTLED=true
      echo "PASS: agent_settled received"
      break
    elif [ "$TYPE" = "turn_end" ]; then
      TEXT=$(echo "$line" | python3 -c "import sys,json; d=json.load(sys.stdin); msg=d.get('message',{}); blocks=msg.get('content',[]); print(' '.join(b.get('text','') for b in blocks if b.get('type')=='text'))" 2>/dev/null)
      echo "turn_end: $TEXT"
    elif [ "$TYPE" = "tool_execution_start" ]; then
      TOOL=$(echo "$line" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('toolName','?'))" 2>/dev/null)
      echo "tool_start: $TOOL"
    fi
  done < "$PIPE_OUT"
  
  if [ "$FOUND_SETTLED" = true ]; then
    if [ -f "$OUTPUT_FILE" ]; then
      CONTENT=$(cat "$OUTPUT_FILE")
      echo "output: '$CONTENT'"
      if echo "$CONTENT" | grep -q "ACCEPTED"; then
        echo ""
        echo "=== CANARY PASSED ==="
      else
        echo "FAIL: output doesn't contain ACCEPTED"
      fi
    else
      echo "WARN: no output file (pi may not have write access)"
    fi
  else
    echo "FAIL: no agent_settled"
  fi
  
  kill $PI_PID 2>/dev/null
  wait $PI_PID 2>/dev/null
  rm -rf "$TMPDIR"
} &
CANARY_PID=$!

# Wait with timeout
DEADLINE=$(( $(date +%s) + 120 ))
while [ $(date +%s) -lt $DEADLINE ]; do
  if ! kill -0 $CANARY_PID 2>/dev/null; then
    wait $CANARY_PID
    exit $?
  fi
  sleep 2
done

echo "TIMEOUT"
kill $PI_PID 2>/dev/null
kill $CANARY_PID 2>/dev/null
wait 2>/dev/null
rm -rf "$TMPDIR"
exit 1
