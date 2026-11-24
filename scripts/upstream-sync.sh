#!/usr/bin/env bash
# Poll upstream RoALT for new main-branch commits and, only when there is work,
# run the upstream-sync agent. Polling itself uses no AI model. We never push.
set -euo pipefail

UPSTREAM="${UPSTREAM_URL:-https://github.com/inspectsoftware/RoALT.git}"
# Last upstream commit already folded into this fork at the time this was set up.
BASELINE="${UPSTREAM_SYNC_BASELINE:-d633a57fcd5ef58752510f9debd46caca86dd59f}"
DRY_RUN="${UPSTREAM_SYNC_DRY_RUN:-0}"

ROOT="$(git -C "$(dirname "$0")/.." rev-parse --show-toplevel)"
cd "$ROOT"
GIT_DIR="$(git rev-parse --git-dir)"
STATE_DIR="$GIT_DIR/upstream-sync"
mkdir -p "$STATE_DIR"
exec 9>"$STATE_DIR/lock"
flock -n 9 || { echo "another upstream sync is still running"; exit 0; }

STATE="$STATE_DIR/state.json"
LOG="$STATE_DIR/agent.log"
if [[ ! -f "$STATE" ]]; then
  jq -n --arg upstream "$UPSTREAM" --arg baseline "$BASELINE" --arg at "$(date -u +%FT%TZ)" \
    '{upstream:$upstream, branch:"main", last_processed:$baseline, pending_to:null, updated_at:$at}' \
    > "$STATE"
fi

head_sha="$(git ls-remote "$UPSTREAM" refs/heads/main | awk '{print $1}')"
[[ -n "$head_sha" ]] || { echo "could not read upstream HEAD"; exit 1; }
processed="$(jq -r '.last_processed // ""' "$STATE")"

if [[ "$head_sha" == "$processed" ]]; then
  echo "no new upstream commits ($head_sha)"
  exit 0
fi

echo "new upstream commits: ${processed:-baseline}..$head_sha"
if [[ "$DRY_RUN" == "1" ]]; then
  echo "dry run: no fetch, no agent"
  exit 0
fi

git fetch --no-tags "$UPSTREAM" main:refs/remotes/upstream/main >/dev/null
git log --oneline "${processed:-$BASELINE}..refs/remotes/upstream/main" > "$STATE_DIR/pending-commits.txt"

jq -n --arg upstream "$UPSTREAM" --arg from "$processed" --arg to "$head_sha" \
  --arg at "$(date -u +%FT%TZ)" \
  '{upstream:$upstream, branch:"main", last_processed:$from, pending_to:$to, updated_at:$at}' \
  > "$STATE.tmp"
mv "$STATE.tmp" "$STATE"

prompt="New upstream RoALT commits are pending on refs/remotes/upstream/main: ${processed:-baseline}..$head_sha (list in $GIT_DIR/upstream-sync/pending-commits.txt). Follow your agent instructions exactly: triage the diff, merge locally with --no-commit --no-ff, never commit or push, refresh docs/PROGRESS.md and the SVG with scripts/render-port-status.py, and write the dated report under docs/upstream-sync/."

if opencode run --auto --agent upstream-sync --title "Upstream sync ${head_sha:0:7}" "$prompt" >>"$LOG" 2>&1; then
  jq --arg sha "$head_sha" --arg at "$(date -u +%FT%TZ)" \
    '.last_processed=$sha | .pending_to=null | .updated_at=$at' "$STATE" > "$STATE.tmp"
  mv "$STATE.tmp" "$STATE"
  echo "processed upstream $head_sha (agent log: $LOG)"
else
  echo "upstream sync failed for $head_sha; see $LOG"
  exit 1
fi
