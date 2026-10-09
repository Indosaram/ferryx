#!/usr/bin/env bash
set -u
ROOT="$HOME/code/project/ferryx-p0/svc"
UNIT_DIR="$HOME/.config/systemd/user"
UNIT="ferryx-p0-host@.service"
mkdir -p "$ROOT" "$UNIT_DIR"
for id in a1 b2; do
  mkdir -p "$ROOT/$id"
  cat > "$ROOT/$id/ferryx-p0-host" <<'EOF'
#!/usr/bin/env bash
id="$1"
echo "$$ $(cut -d' ' -f22 /proc/$$/stat) $PPID" > "$HOME/code/project/ferryx-p0/svc/$id/pid"
trap 'echo stopped >> "$HOME/code/project/ferryx-p0/svc/$id/events"; exit 0' TERM
while :; do
  if [ -f "$HOME/code/project/ferryx-p0/svc/$id/retire" ]; then
    echo retired-exit >> "$HOME/code/project/ferryx-p0/svc/$id/events"
    systemctl --user disable "ferryx-p0-host@$id.service" >/dev/null 2>&1
    exit 0
  fi
  sleep 0.2
done
EOF
  chmod +x "$ROOT/$id/ferryx-p0-host"
done
cat > "$UNIT_DIR/$UNIT" <<EOF
[Unit]
Description=Ferryx P0 host experiment instance %i

[Service]
Type=simple
ExecStart=$ROOT/%i/ferryx-p0-host %i
Restart=on-failure
KillMode=process

[Install]
WantedBy=default.target
EOF
systemctl --user daemon-reload
echo "STEP start a1"; systemctl --user enable --now ferryx-p0-host@a1.service; sleep 1
read A1 A1START A1PPID < "$ROOT/a1/pid"
echo "a1 pid=$A1 ppid=$A1PPID ppid_comm=$(cat /proc/$A1PPID/comm) cgroup=$(cut -d: -f3 /proc/$A1/cgroup)"
echo "STEP start b2 while a1 runs"; systemctl --user enable --now ferryx-p0-host@b2.service; sleep 1
read B2 B2START B2PPID < "$ROOT/b2/pid"
echo "b2 pid=$B2 ppid_comm=$(cat /proc/$B2PPID/comm)"
echo "STEP restart b2 only"; systemctl --user restart ferryx-p0-host@b2.service; sleep 1
read A1N A1NSTART _ < "$ROOT/a1/pid"
echo "a1_unchanged_after_b2_restart=$([ "$A1N $A1NSTART" = "$A1 $A1START" ] && kill -0 $A1 2>/dev/null && echo yes || echo NO)"
echo "STEP simulate GUI/daemon death: start a throwaway parent shell that exits"
bash -c 'sleep 0.1' ; echo "a1_alive_after_unrelated_parent_exit=$(kill -0 $A1 2>/dev/null && echo yes || echo NO)"
echo "STEP retire a1 (self-exit + self-disable)"; touch "$ROOT/a1/retire"; sleep 1.5
echo "a1_active=$(systemctl --user is-active ferryx-p0-host@a1.service) a1_enabled=$(systemctl --user is-enabled ferryx-p0-host@a1.service 2>&1) a1_events=$(cat $ROOT/a1/events 2>/dev/null)"
echo "b2_active=$(systemctl --user is-active ferryx-p0-host@b2.service)"
echo "STEP cleanup"
systemctl --user disable --now ferryx-p0-host@b2.service >/dev/null 2>&1
systemctl --user disable --now ferryx-p0-host@a1.service >/dev/null 2>&1
rm -f "$UNIT_DIR/$UNIT"; systemctl --user daemon-reload
echo "cleanup unit_present=$([ -f "$UNIT_DIR/$UNIT" ] && echo yes || echo no) a1=$(systemctl --user is-active ferryx-p0-host@a1.service) b2=$(systemctl --user is-active ferryx-p0-host@b2.service) linger=$(loginctl show-user $USER -p Linger --value)"
