#!/bin/bash
# Starts the RustDesk Server (hbbr relay, hbbs ID/rendezvous) and the RemoteCraft API in one
# container. hbbs and hbbr face the internet: they run as the `rustdesk` user, own only
# /data/rustdesk and never see the desk's service token. The API runs as `remotecraft`, which
# alone owns /data/desk and reads only the RustDesk public key. If any of the three stops, the
# container stops too, and Docker's restart policy brings everything back.
set -euo pipefail

RUSTDESK_DIR=/data/rustdesk
DESK_DIR=/data/desk
mkdir -p "$RUSTDESK_DIR" "$DESK_DIR"
# Earlier setups left the directories to root, or both to one user: each to its own user,
# nothing readable by the others.
chown -R rustdesk:rustdesk "$RUSTDESK_DIR"
chown -R remotecraft:remotecraft "$DESK_DIR"
chmod -R go-rwx "$RUSTDESK_DIR" "$DESK_DIR"

if [ -z "${REMOTECRAFT_TOKEN:-}" ]; then
  echo "REMOTECRAFT_TOKEN is not set: the desk API needs a service token (see docs/deploy.md)" >&2
  exit 1
fi
if [ "${#REMOTECRAFT_TOKEN}" -lt 32 ]; then
  echo "REMOTECRAFT_TOKEN is too short: use at least 32 random characters, e.g. \$(openssl rand -hex 32)" >&2
  exit 1
fi

# The desk API, as `remotecraft`, with its home in the current directory.
as_desk() { exec env HOME="$PWD" setpriv --reuid=remotecraft --regid=remotecraft --init-groups -- "$@"; }
# A RustDesk relay, as `rustdesk`, without the service token, its files private.
as_rustdesk() {
  umask 077
  exec env -u REMOTECRAFT_TOKEN HOME="$PWD" setpriv --reuid=rustdesk --regid=rustdesk --init-groups -- "$@"
}

# hbbs tells the clients which relay to use: the public name, when given.
HBBS_ARGS=(-k _)
if [ -n "${RUSTDESK_HOST:-}" ]; then
  HBBS_ARGS+=(-r "${RUSTDESK_HOST}")
fi

cd "$RUSTDESK_DIR"
(as_rustdesk hbbr -k _) &
(as_rustdesk hbbs "${HBBS_ARGS[@]}") &

# hbbs writes its key pair on the first start: wait for it so the desk shows it.
for _ in $(seq 1 30); do
  [ -s "$RUSTDESK_DIR/id_ed25519.pub" ] && break
  sleep 1
done
# The desk may read the public key (and nothing else: the directory cannot be listed, the
# private key and hbbs's database stay 0600).
if [ -f "$RUSTDESK_DIR/id_ed25519.pub" ]; then
  chmod 0644 "$RUSTDESK_DIR/id_ed25519.pub"
fi
chmod 0711 "$RUSTDESK_DIR"

cd "$DESK_DIR"
(as_desk remotecraft-cli serve) &

trap 'kill -TERM $(jobs -p) 2>/dev/null; wait' TERM INT
wait -n
status=$?
echo "a RemoteCraft process stopped (status $status): stopping the container" >&2
kill -TERM $(jobs -p) 2>/dev/null || true
wait || true
exit "$status"
