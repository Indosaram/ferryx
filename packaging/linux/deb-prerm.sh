#!/bin/sh
# Debian prerm for the Ferryx desktop package (bundle.linux.deb.preRemoveScript).
#
# On removal (`apt remove`/`apt purge`/`dpkg -r`) every user whose systemd user manager is running
# gets `ferryx-cli service uninstall`, which disables and deletes that user's daemon units (every
# runtime generation and the legacy ferryx-daemon.service) and owns the unit naming. A running daemon
# is never stopped (no --force, no `systemctl stop`): it keeps its terminals until it exits and
# nothing starts it at the next login. An upgrade keeps the units, since the new version
# re-registers its own generation. A user with no running user manager is left as is and named
# below; they can run `ferryx-cli service uninstall` themselves before removing the package.
# Never fails the removal.
set -u

case "${1:-}" in
  remove) ;;
  *) exit 0 ;;
esac

CLI=/usr/bin/ferryx-cli
if [ ! -x "$CLI" ]; then
  echo "ferryx prerm: $CLI not found; daemon user units were not removed" >&2
  exit 0
fi

for manager in /run/user/*/systemd/private; do
  [ -S "$manager" ] || continue
  runtime_dir=${manager%/systemd/private}
  uid=${runtime_dir##*/}
  entry=$(getent passwd "$uid") || continue
  user=$(printf '%s\n' "$entry" | cut -d: -f1)
  home=$(printf '%s\n' "$entry" | cut -d: -f6)
  echo "ferryx prerm: removing the Ferryx daemon user units of $user" >&2
  if command -v runuser >/dev/null 2>&1; then
    runuser -u "$user" -- env -i HOME="$home" USER="$user" LOGNAME="$user" \
      XDG_RUNTIME_DIR="$runtime_dir" PATH=/usr/sbin:/usr/bin:/sbin:/bin \
      "$CLI" service uninstall >&2 || echo "ferryx prerm: service uninstall failed for $user" >&2
  else
    su -s /bin/sh "$user" -c "env -i HOME='$home' USER='$user' LOGNAME='$user' XDG_RUNTIME_DIR='$runtime_dir' PATH=/usr/sbin:/usr/bin:/sbin:/bin '$CLI' service uninstall" >&2 \
      || echo "ferryx prerm: service uninstall failed for $user" >&2
  fi
done

for home_units in /home/*/.config/systemd/user /root/.config/systemd/user; do
  [ -d "$home_units" ] || continue
  if ls "$home_units" 2>/dev/null | grep -q '^ferryx-daemon'; then
    owner=$(stat -c %U "$home_units" 2>/dev/null || echo "?")
    if [ ! -S "/run/user/$(id -u "$owner" 2>/dev/null || echo none)/systemd/private" ]; then
      echo "ferryx prerm: $owner has Ferryx daemon units but no running user manager; run 'ferryx-cli service uninstall' as $owner" >&2
    fi
  fi
done

exit 0
