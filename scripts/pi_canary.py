#!/usr/bin/env python3
"""Pi RPC live canary — protocol verification using line-buffered reads."""
import subprocess, json, sys, tempfile, os, time, threading

tmp = tempfile.mkdtemp()
print(f"=== Pi RPC Canary ===")
print(f"Temp: {tmp}")

proc = subprocess.Popen(
    ["pi", "--mode", "rpc", "--no-context-files", "--no-extensions", "--thinking", "off",
     "--session-dir", tmp],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    text=True
)

# Thread to read stdout lines into a queue
import queue
lines = queue.Queue()
done_reading = threading.Event()

def reader():
    for line in proc.stdout:
        line = line.strip()
        if line:
            try:
                frame = json.loads(line)
                lines.put(frame)
            except json.JSONDecodeError:
                pass
    done_reading.set()

reader_thread = threading.Thread(target=reader, daemon=True)
reader_thread.start()

def read_frame(timeout=10):
    try:
        return lines.get(timeout=timeout)
    except queue.Empty:
        return None

def read_response(expected_id, timeout=10):
    deadline = time.time() + timeout
    while time.time() < deadline:
        frame = read_frame(timeout=2)
        if frame is None:
            raise TimeoutError(f"No response for {expected_id}")
        if frame.get("id") == expected_id and frame.get("type") == "response":
            return frame
        sys.stderr.write(f"  (ignored: {frame.get('type')})\n")
    raise TimeoutError(f"No response for {expected_id}")

# 1. get_state
proc.stdin.write(json.dumps({"id": "t1", "type": "get_state"}) + "\n")
proc.stdin.flush()
state = read_response("t1")
session_file = state["data"].get("sessionFile")
assert session_file and session_file.startswith("/"), f"sessionFile: {session_file}"
ac = state["data"].get("autoCompactionEnabled", False)
print(f"sessionFile: {session_file}")
print(f"autoCompactionEnabled: {ac}")
print("PASS: get_state")

# 2. Disable auto-compaction
proc.stdin.write(json.dumps({"id": "t2", "type": "set_auto_compaction", "enabled": False}) + "\n")
proc.stdin.flush()
resp = read_response("t2")
assert resp.get("success"), f"set_auto_compaction failed: {resp}"
print("PASS: auto-compaction disabled")

# 3. Send prompt
output_file = os.path.join(tmp, "pi-canary.txt")
msg = (
    f'Write exactly the word ACCEPTED to the file {output_file}. '
    f'Do nothing else. Do not read any files. Just write the single word. '
    f'When done, output:\n## RESULT\nDONE\n## SUMMARY\nFile written.'
)
proc.stdin.write(json.dumps({"id": "t3", "type": "prompt", "message": msg}) + "\n")
proc.stdin.flush()

# Wait for prompt acceptance
resp = read_response("t3")
assert resp.get("success"), f"prompt not accepted: {resp}"

# Read events until agent_settled
found_settled = False
final_text = ""
while True:
    frame = read_frame(timeout=180)
    if frame is None:
        break
    t = frame.get("type", "")
    if t == "agent_settled":
        found_settled = True
        print("PASS: agent_settled received")
        break
    elif t == "turn_end":
        msg_block = frame.get("message", {})
        blocks = msg_block.get("content", [])
        text = " ".join(b.get("text", "") for b in blocks if b.get("type") == "text")
        if text.strip():
            final_text = text
            print(f"turn_end: {final_text[:200]}")
    elif t == "tool_execution_start":
        print(f"tool: {frame.get('toolName', '?')}")
    elif t == "agent_start":
        print("agent_start")

assert found_settled, "agent_settled not received"

# 4. Verify outputs
if os.path.exists(output_file):
    content = open(output_file).read()
    print(f"output file: '{content.strip()}'")
    if "ACCEPTED" in content:
        print("\n=== CANARY PASSED ===")
    else:
        print(f"FAIL: output doesn't contain ACCEPTED")
        sys.exit(1)
else:
    print(f"WARN: output file not found at {output_file}")

print(f"final_text: {final_text[:300]}")
assert "RESULT" in final_text or "SUMMARY" in final_text, f"missing report: {final_text}"
print("PASS: report format validated")

def contains_persisted_thinking(value):
    if isinstance(value, dict):
        if value.get("type") == "thinking" or "thinkingSignature" in value:
            return True
        return any(contains_persisted_thinking(item) for item in value.values())
    if isinstance(value, list):
        return any(contains_persisted_thinking(item) for item in value)
    return False

with open(session_file, encoding="utf-8") as native_session:
    for line_number, line in enumerate(native_session, 1):
        if not line.strip():
            continue
        entry = json.loads(line)
        assert not contains_persisted_thinking(entry), (
            f"native session line {line_number} persisted thinking"
        )
print("PASS: native session contains no persisted thinking")

# Cleanup
proc.stdin.close()
proc.kill()
proc.wait()
import shutil
shutil.rmtree(tmp)
