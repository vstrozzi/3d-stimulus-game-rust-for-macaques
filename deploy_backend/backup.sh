#!/usr/bin/env bash
# Nightly backup of the trial logs via restic (e.g. to Cloudflare R2).
#
# Secrets are NOT in this file — put them in /etc/monkey-backup.env (chmod 600):
#   RESTIC_REPOSITORY=s3:https://<accountid>.r2.cloudflarestorage.com/<bucket>
#   RESTIC_PASSWORD=<backup-encryption-password>
#   AWS_ACCESS_KEY_ID=<r2-key>
#   AWS_SECRET_ACCESS_KEY=<r2-secret>
#
# Install:  apt install -y restic
# Cron:     15 3 * * * /srv/monkey_3d_game/deploy_backend/backup.sh >> /var/log/monkey-backup.log 2>&1
set -euo pipefail

# Docker bind-mount path (see docker-compose.yml). For a non-docker host install
# point DATA_DIR at /srv/monkey_3d_game/out/server_logs instead.
DATA_DIR="${DATA_DIR:-/srv/monkey_3d_game/data/server_logs}"
ENV_FILE="${ENV_FILE:-/etc/monkey-backup.env}"

# Load RESTIC_REPOSITORY / RESTIC_PASSWORD / R2 keys.
[ -f "$ENV_FILE" ] && { set -a; . "$ENV_FILE"; set +a; }
[ -d "$DATA_DIR" ] || { echo "$(date -Is) no data dir: $DATA_DIR — nothing to back up"; exit 0; }

echo "$(date -Is) starting backup of $DATA_DIR"

# Initialize the repo on first run (no-op afterwards).
restic snapshots >/dev/null 2>&1 || restic init

# Snapshot, then keep a rolling history so the repo doesn't grow unbounded.
restic backup --tag monkey "$DATA_DIR"
restic forget --tag monkey --keep-daily 7 --keep-weekly 8 --keep-monthly 12 --prune

echo "$(date -Is) backup done"
