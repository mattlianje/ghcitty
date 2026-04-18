#!/bin/sh
set -e

cd "$(dirname "$0")/.."

echo "Building ghcitty..."
make build

pids=""
for tape in demos/*.tape; do
  name=$(basename "$tape" .tape)
  echo "Recording $name..."
  vhs "$tape" &
  pids="$pids $!"
done

for pid in $pids; do
  wait "$pid"
done

echo "Done. GIFs in demos/"
ls -lh demos/*.gif
