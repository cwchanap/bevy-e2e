#!/usr/bin/env bash
# Fail if any bevy-e2e-fixture child processes remain after tests.
set -euo pipefail

if command -v pgrep >/dev/null 2>&1; then
  if pgrep -f '(^|/)bevy-e2e-fixture( |$)' >/dev/null 2>&1; then
    echo "error: leaked bevy-e2e-fixture process(es):" >&2
    pgrep -af '(^|/)bevy-e2e-fixture( |$)' >&2 || true
    exit 1
  fi
else
  # Fallback when pgrep is unavailable.
  if ps -eo args= 2>/dev/null | grep -E '(^|/)bevy-e2e-fixture( |$)' | grep -v grep >/dev/null 2>&1; then
    echo "error: leaked bevy-e2e-fixture process(es):" >&2
    ps -eo pid=,args= 2>/dev/null | grep -E '(^|/)bevy-e2e-fixture( |$)' | grep -v grep >&2 || true
    exit 1
  fi
fi

echo "ok: no bevy-e2e-fixture survivor processes"
