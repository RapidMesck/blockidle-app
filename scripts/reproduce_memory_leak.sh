#!/usr/bin/env bash
set -euo pipefail

# Redact any secrets if present
# Feedback loop for tray memory consumption (Phase 1 & 5)
# Measures RSS, PSS, and Private Dirty memory of the client shell and WebKit processes in tray at 0 FPS.

cleanup() {
    if [[ -n "${APP_PID:-}" ]] && kill -0 "$APP_PID" 2>/dev/null; then
        kill "$APP_PID" 2>/dev/null || true
        wait "$APP_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# 1. Launch with auto-tray after 3 seconds
export BLOCKIDLE_AUTO_TRAY_SECS=3
./target/release/blockidle-app &
APP_PID=$!

# Wait 6 seconds: 3s foreground loading + 3s in tray at 0 FPS
sleep 6

# 2. Collect all descendant processes
PIDS=$(pgrep -P "$APP_PID" || true)
ALL_PIDS="$APP_PID $PIDS"

echo "=== Process Tree & Memory Metrics in Tray (0 FPS) ==="
printf "%-10s %-8s %-12s %-12s %-12s %s\n" "PID" "PPID" "RSS (KB)" "PSS (KB)" "DIRTY (KB)" "COMMAND"

TOTAL_RSS_KB=0
TOTAL_PSS_KB=0
TOTAL_DIRTY_KB=0

for p in $ALL_PIDS; do
    if [[ -d "/proc/$p" ]]; then
        cmd=$(cat /proc/$p/comm 2>/dev/null || true)
        ppid=$(awk '/PPid:/ {print $2}' /proc/$p/status 2>/dev/null || true)
        rss=$(awk '/Rss:/ {print $2}' /proc/$p/smaps_rollup 2>/dev/null || true)
        pss=$(awk '/^Pss:/ {print $2}' /proc/$p/smaps_rollup 2>/dev/null || true)
        dirty=$(awk '/Private_Dirty:/ {print $2}' /proc/$p/smaps_rollup 2>/dev/null || true)
        
        rss=${rss:-0}
        pss=${pss:-0}
        dirty=${dirty:-0}

        TOTAL_RSS_KB=$((TOTAL_RSS_KB + rss))
        TOTAL_PSS_KB=$((TOTAL_PSS_KB + pss))
        TOTAL_DIRTY_KB=$((TOTAL_DIRTY_KB + dirty))

        printf "%-10s %-8s %-12s %-12s %-12s %s\n" "$p" "$ppid" "$rss" "$pss" "$dirty" "$cmd"
    fi
done

TOTAL_RSS_MB=$((TOTAL_RSS_KB / 1024))
TOTAL_PSS_MB=$((TOTAL_PSS_KB / 1024))
TOTAL_DIRTY_MB=$((TOTAL_DIRTY_KB / 1024))

echo "------------------------------------------------------------"
echo "Total RSS (summed with shared libs): ${TOTAL_RSS_MB} MB"
echo "Total PSS (proportional physical RAM): ${TOTAL_PSS_MB} MB"
echo "Total Private Dirty (exclusive heap): ${TOTAL_DIRTY_MB} MB"
echo "------------------------------------------------------------"

# Memory leak check: Private Dirty (exclusive game heap) should be under 400 MB in tray
DIRTY_BUDGET_MB=400
if [[ "$TOTAL_DIRTY_MB" -gt "$DIRTY_BUDGET_MB" ]]; then
    echo "FAIL: Private Dirty memory (${TOTAL_DIRTY_MB} MB) exceeds budget (${DIRTY_BUDGET_MB} MB)"
    exit 1
else
    echo "PASS: Private Dirty memory is ${TOTAL_DIRTY_MB} MB (within ${DIRTY_BUDGET_MB} MB budget)"
    exit 0
fi
