#!/bin/sh
# Copy new commits from the private main line to the public repo, one public commit per first-parent commit, with the
# same files. The public repo starts from a parentless snapshot, so private history never goes out: each public commit
# is rebuilt with `git commit-tree` from the private commit's tree alone.
#
# Usage (from the repo):  tools/mirror-public.sh [--push] [--squash] [--source REF] [--remote NAME]
#   --source  the private line to mirror (default: origin/main)
#   --remote  the public remote (default: public)
#   --squash  publish one commit with the source's latest files instead of one per commit: use it when a commit in
#             between leaked something that a later commit removed, so the leaked state never goes out
#   --push    push the result; without it, only print what would be published (dry run)
#
# Where to resume is found by tree: the newest first-parent commit of the source whose files equal the public head.
#
# Leak guard: every tree about to be published is searched for each line (a regex) of .git/info/public-denylist,
# which lives in the git directory and is never committed. A match stops the run before anything is pushed. The file
# must exist, so a missing list can't silently skip the check.
#
# Author emails are replaced with $MIRROR_EMAIL (default: the committer's git config mirror.email); names and dates
# are kept. A "Merge pull request #N from …" subject is dropped in favour of the PR title below it, since private PR
# numbers mean nothing in the public repo.
set -e
SOURCE=origin/main REMOTE=public PUSH=0 SQUASH=0
while [ $# -gt 0 ]; do
  case "$1" in
    --push) PUSH=1; shift ;;
    --squash) SQUASH=1; shift ;;
    --source) SOURCE="$2"; shift 2 ;;
    --remote) REMOTE="$2"; shift 2 ;;
    -h|--help) sed -n '2,23p' "$0"; exit 0 ;;
    *) echo "Unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
done

DENY="$(git rev-parse --git-common-dir)/info/public-denylist"
[ -f "$DENY" ] || { echo "No leak denylist at $DENY (one regex per line). Create it before mirroring." >&2; exit 1; }
EMAIL="${MIRROR_EMAIL:-$(git config mirror.email || true)}"
[ -n "$EMAIL" ] || { echo "Set MIRROR_EMAIL or git config mirror.email (e.g. your GitHub no-reply address)." >&2; exit 1; }

git fetch -q "$REMOTE"
SRC_REMOTE=${SOURCE%%/*}
[ "$SRC_REMOTE" != "$SOURCE" ] && git remote | grep -qx "$SRC_REMOTE" && git fetch -q "$SRC_REMOTE"
HEAD_PUB=$(git rev-parse "refs/remotes/$REMOTE/main")
TREE_PUB=$(git rev-parse "$HEAD_PUB^{tree}")

# newest first-parent source commit with the public head's files
BASE=""
for c in $(git rev-list --first-parent "$SOURCE"); do
  if [ "$(git rev-parse "$c^{tree}")" = "$TREE_PUB" ]; then BASE=$c; break; fi
done
[ -n "$BASE" ] || { echo "No commit on $SOURCE has the files of $REMOTE/main; can't tell where to resume." >&2; exit 1; }

NEW=$(git rev-list --reverse --first-parent "$BASE..$SOURCE")
[ -n "$NEW" ] || { echo "$REMOTE/main is up to date with $SOURCE."; exit 0; }

# a commit's public message: a GitHub merge's PR title instead of its private "Merge pull request #N" subject
message() {
  case "$(git log -1 --format=%s "$1")" in
    "Merge pull request #"*) m=$(git log -1 --format=%b "$1"); [ -n "$m" ] || m=$(git log -1 --format=%s "$1"); printf "%s\n" "$m" ;;
    *) git log -1 --format=%B "$1" ;;
  esac
}
TIP=$(git rev-parse "$SOURCE")
# the trees that will be published: every new commit's, or only the tip's when squashing
if [ "$SQUASH" -eq 1 ]; then TREES=$TIP; else TREES=$NEW; fi

# leak guard over every tree and message first, so nothing is built or pushed if any of them leaks
HITS=$(mktemp)
label() { while IFS= read -r line; do printf 'LEAK [%s] %s%s\n' "$1" "$2" "$line"; done; }
grep -v '^[[:space:]]*\(#\|$\)' "$DENY" | while IFS= read -r pat; do
  for c in $TREES; do git grep -nIE -e "$pat" "$c" -- . 2>/dev/null | label "$pat" ""; done
  for c in $NEW; do message "$c" | grep -nE -e "$pat" | label "$pat" "message of $(git rev-parse --short "$c"): "; done
done > "$HITS"
if [ -s "$HITS" ]; then
  cat "$HITS" >&2; rm -f "$HITS"
  echo "Leak guard: nothing published. Fix the matches above on $SOURCE first (then --squash if they're in old commits)." >&2
  exit 1
fi
rm -f "$HITS"

build() { # build TREE-COMMIT MESSAGE-SOURCE-COMMIT MESSAGE: a public commit on $PARENT
  PARENT=$(GIT_AUTHOR_NAME="$(git log -1 --format=%an "$2")" GIT_AUTHOR_EMAIL="$EMAIL" \
    GIT_AUTHOR_DATE="$(git log -1 --format=%aD "$2")" GIT_COMMITTER_EMAIL="$EMAIL" \
    GIT_COMMITTER_DATE="$(git log -1 --format=%cD "$2")" \
    git commit-tree "$(git rev-parse "$1^{tree}")" -p "$PARENT" -m "$3")
  echo "$(git rev-parse --short "$1") -> $(git rev-parse --short "$PARENT")  $(printf '%s\n' "$3" | head -1)"
}
PARENT=$HEAD_PUB
if [ "$SQUASH" -eq 1 ]; then
  build "$TIP" "$TIP" "$(printf 'Changes from %s commits\n\n' "$(echo "$NEW" | wc -l | tr -d ' ')"; for c in $NEW; do echo "- $(message "$c" | head -1)"; done)"
else
  for c in $NEW; do build "$c" "$c" "$(message "$c")"; done
fi

if [ "$PUSH" -eq 1 ]; then
  git push "$REMOTE" "$PARENT:refs/heads/main"
else
  echo "Dry run: $REMOTE/main would move to $(git rev-parse --short "$PARENT"). Pass --push to publish."
fi
