#!/bin/sh
# Ensure the mounted volumes exist and seed the trial library on first run.
set -e

mkdir -p /app/out/server_logs /app/trials_config/trials

# If the trials volume is empty (first deploy), seed it with the default library
# baked into the image at build time.
if [ -z "$(ls -A /app/trials_config/trials 2>/dev/null)" ]; then
  echo "[entrypoint] seeding trials library from image"
  cp -a /app/seed_trials/. /app/trials_config/trials/
fi

exec "$@"
