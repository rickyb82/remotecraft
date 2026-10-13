#!/bin/bash
# Starts the RustDesk Server (hbbr relay, hbbs ID/rendezvous) and the RemoteCraft API in one
# container, as the unprivileged `remotecraft` user. If any of the three stops, the container
# stops too, and Docker's restart policy brings everything back.
set -euo pipefail

RUSTDESK_DIR=/data/rustdesk
DESK_DIR=/data/desk
mkdir -p "$RUSTDESK_DIR" "$DESK_DIR"
# Directories from the old two-container setup belong to root: hand them over.
chown -R remotecraft:remotecraft "$RUSTDESK_DIR" "$DESK_DIR"
chmod 0700 "$RUSTDESK_DIR" "$DESK_DIR"

if [ -z "${REMOTECRAFT_TOKEN:-}" ]; then
  echo "REMOTECRAFT_TOKEN is not set: the desk API needs a service token (see docs/deploy.md)" >&2
  exit 1
fi

# Runs a program as `remotecraft`, with its home in the current directory.
as_user() { exec env HOME="$PWD" setpriv --reuid=remotecraft --regid=remotecraft --init-groups -- "$@"; }

# hbbs tells the clients which relay to use: the public name, when given.
HBBS_ARGS=(-k _)
if [ -n "${RUSTDESK_HOST:-}" ]; then
  HBBS_ARGS+=(-r "${RUSTDESK_HOST}")
fi

cd "$RUSTDESK_DIR"
(as_user hbbr -k _) &
(as_user hbbs "${HBBS_ARGS[@]}") &

# hbbs writes its key pair on the first start: wait for it so the desk shows it.
for _ in $(seq 1 30); do
  [ -s "$RUSTDESK_DIR/id_ed25519.pub" ] && break
  sleep 1
done

cd "$DESK_DIR"
(as_user remotecraft-cli serve) &

trap 'kill -TERM $(jobs -p) 2>/dev/null; wait' TERM INT
wait -n
status=$?
echo "a RemoteCraft process stopped (status $status): stopping the container" >&2
kill -TERM $(jobs -p) 2>/dev/null || true
wait || true
exit "$status"
