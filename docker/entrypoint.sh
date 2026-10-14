#!/bin/bash
# Starts the RustDesk Server (hbbr relay, hbbs ID/rendezvous) and the RemoteCraft API in one
# container. hbbs and hbbr face the internet: they run as the `rustdesk` user, own only
# /data/rustdesk and never see the desk's service token. The API runs as `remotecraft`, which
# alone owns /data/desk and reads only the RustDesk public key.
#
# RustDesk comes first and never depends on the desk. If hbbs or hbbr stops, the container
# stops too, and Docker's restart policy brings everything back. If the desk cannot start (a
# missing or too short REMOTECRAFT_TOKEN, a damaged desk.json) or stops, hbbs and hbbr keep
# running and the log says why; a desk that stopped is started again 10 s later.
set -euo pipefail

RUSTDESK_DIR=/data/rustdesk
DESK_DIR=/data/desk
mkdir -p "$RUSTDESK_DIR" "$DESK_DIR"
# Earlier setups left the directories to root, or both to one user: each to its own user,
# nothing readable by the others. (Recursive chown and chmod never follow the links inside.)
chown -R rustdesk:rustdesk "$RUSTDESK_DIR"
chown -R remotecraft:remotecraft "$DESK_DIR"
chmod -R go-rwx "$RUSTDESK_DIR" "$DESK_DIR"

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
relays=("$!")
(as_rustdesk hbbs "${HBBS_ARGS[@]}") &
relays+=("$!")
trap 'kill -TERM $(jobs -p) 2>/dev/null || true; wait || true; exit 0' TERM INT

# hbbs writes its key pair on the first start: wait for it so the desk shows it.
pub="$RUSTDESK_DIR/id_ed25519.pub"
for _ in $(seq 1 30); do
  [ -s "$pub" ] && break
  sleep 1
done
# The desk may read the public key (and nothing else: the directory cannot be listed, the
# private key and hbbs's database stay 0600). The relays own the directory and could have
# put a link there: never followed as root, and the file is made readable by its owner.
if [ -f "$pub" ] && [ ! -L "$pub" ]; then
  setpriv --reuid=rustdesk --regid=rustdesk --init-groups -- chmod 0644 "$pub" || true
fi
chmod 0711 "$RUSTDESK_DIR"

# The desk's service token, checked as `remotecraft-cli serve` checks it (32 characters).
desk_problem=""
if [ -z "${REMOTECRAFT_TOKEN:-}" ]; then
  desk_problem="REMOTECRAFT_TOKEN is not set"
elif [ "$(LC_ALL=C.UTF-8; echo "${#REMOTECRAFT_TOKEN}")" -lt 32 ]; then
  desk_problem="REMOTECRAFT_TOKEN is too short (an older .env?): it needs at least 32 random characters"
fi

# The desk API, started again 10 s after it stops.
run_desk() {
  desk_pid=""
  trap 'if [ -n "$desk_pid" ]; then kill -TERM "$desk_pid" 2>/dev/null || true; fi; exit 0' TERM INT
  cd "$DESK_DIR"
  while :; do
    (as_desk remotecraft-cli serve) &
    desk_pid=$!
    status=0
    wait "$desk_pid" || status=$?
    desk_pid=""
    echo "ERROR: the desk API stopped (status $status), see above: starting it again in 10 s (RustDesk keeps running)" >&2
    sleep 10 &
    wait $! || true
  done
}

if [ -n "$desk_problem" ]; then
  {
    echo "ERROR: $desk_problem."
    echo "ERROR: the desk API (port 7979: web app, framework, MCP) is NOT running; RustDesk (hbbs, hbbr) is."
    echo "ERROR: put REMOTECRAFT_TOKEN=\$(openssl rand -hex 32) in .env, give the framework the same token,"
    echo "ERROR: then run: docker compose up -d   (see 'Aggiornamento' in docs/deploy.md)"
  } >&2
else
  run_desk &
fi

# Until hbbs or hbbr stops. Checked every 2 s rather than with `wait -n`: bash forgets a
# background job that ended while it ran something else, such as the wait for the key above.
while :; do
  for pid in "${relays[@]}"; do
    if ! kill -0 "$pid" 2>/dev/null; then
      status=0
      wait "$pid" || status=$?
      echo "a RustDesk process stopped (status $status): stopping the container" >&2
      kill -TERM $(jobs -p) 2>/dev/null || true
      wait || true
      exit "$status"
    fi
  done
  sleep 2 &
  wait $! || true
done
