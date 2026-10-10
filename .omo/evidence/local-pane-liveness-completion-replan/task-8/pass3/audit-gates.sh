#!/bin/sh
cd "$(dirname "$0")"
echo "=== CRITERION 5: UI gates ==="
for h in mac linux windows; do
  for f in ui-build ui-split ui-lifecycle runner full-ui; do
    printf "%-8s %-12s " "$h" "$f"
    if [ -s "$h/logs/$f.log" ]; then
      grep -E "built in|Tests  " "$h/logs/$f.log" | tail -1 | sed 's/^ *//' | sed "s/^/OK  /"
    else
      echo "MISSING"
    fi
  done
done
echo
echo "=== CRITERION 5: Rust selector gates ==="
for h in mac linux windows; do
  for f in split-list split pane-list pane; do
    printf "%-8s %-10s " "$h" "$f"
    if [ -s "$h/logs/$f.log" ]; then
      res=$(grep -E "^test result:" "$h/logs/$f.log" | tail -1)
      sel=$(grep -cE ": test$" "$h/logs/$f.log")
      echo "sel=$sel $res"
    else
      echo MISSING
    fi
  done
done
echo
echo "=== CRITERION 5: linux-only gates ==="
for f in qa_barrier daemon_handover_contract daemon_persistence_contract unix-suspension journal zero_config_gen4_audit zero_config_gen5_regression ipc_hardening_contract; do
  printf "%-34s " "$f"
  if [ -s "linux/logs/$f.log" ]; then
    grep -E "^test result:" "linux/logs/$f.log" | tail -1
  else
    echo MISSING
  fi
done
echo
echo "=== transfer contract gates ==="
for h in mac linux windows; do
  for f in transfer-list transfer; do
    printf "%-8s %-14s " "$h" "$f"
    if [ -s "$h/logs/$f.log" ]; then
      grep -E "^test result:" "$h/logs/$f.log" | tail -1 || echo "(compile gate)"
    else
      echo MISSING
    fi
  done
done
