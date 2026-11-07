#!/bin/sh
# Tests for tools/mirror-public.sh against throwaway repos: a private origin with old history, and a bare public repo.
set -e
SCRIPT="$(cd "$(dirname "$0")" && pwd)/mirror-public.sh"
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
fail() { echo "FAIL: $1" >&2; exit 1; }
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=Dev GIT_AUTHOR_EMAIL=dev@private.example \
  GIT_COMMITTER_NAME=Dev GIT_COMMITTER_EMAIL=dev@private.example MIRROR_EMAIL=dev@users.noreply.example
g() { git -C "$T/work" "$@"; }

git init -q --bare -b main "$T/origin.git"; git init -q --bare -b main "$T/public.git"
git init -q -b main "$T/work"
g remote add origin "$T/origin.git"; g remote add public "$T/public.git"

# old private history, including a secret that was cleaned up later
echo "secret-codename" > "$T/work/notes.txt"; g add -A; g commit -qm "old"
echo "clean" > "$T/work/notes.txt"; g commit -qam "cleanup"
CLEAN=$(g rev-parse HEAD)
# public starts as a parentless snapshot of the clean tree
ROOT=$(g commit-tree "$(g rev-parse "HEAD^{tree}")" -m "first public release")
g push -q public "$ROOT:refs/heads/main"
# new private work: a plain commit and a PR-style merge
echo one > "$T/work/a.txt"; g add -A; g commit -qm "add a"
g checkout -qb feat; echo two > "$T/work/b.txt"; g add -A; g commit -qm "add b"
g checkout -q main; g merge -q --no-ff feat -m "Merge pull request #7 from someone/feat" -m "Add b, nicely"
g push -q origin main
printf '# comment\nsecret-codename\nnot/a|match\n' > "$T/work/.git/info/public-denylist"
run() { (cd "$T/work" && sh "$SCRIPT" "$@"); }

# dry run: lists both, pushes nothing
OUT=$(run) || fail "dry run failed"
echo "$OUT" | grep -q "add a" || fail "dry run lists the plain commit"
echo "$OUT" | grep -q "Add b, nicely" || fail "merge uses the PR title"
[ "$(git -C "$T/public.git" rev-parse main)" = "$ROOT" ] || fail "dry run pushed"

# push: two new public commits, same files, no private history
run --push >/dev/null 2>&1 || fail "push failed"
P=$(git -C "$T/public.git" rev-parse main)
[ "$(git -C "$T/public.git" rev-list --count main)" -eq 3 ] || fail "public has root + 2 commits"
[ "$(git -C "$T/public.git" rev-parse "$P^{tree}")" = "$(g rev-parse "main^{tree}")" ] || fail "same files as private main"
git -C "$T/public.git" cat-file -e "$CLEAN" 2>/dev/null && fail "private history reached public"
git -C "$T/public.git" log --format=%ae%n%ce "$ROOT..main" | grep -q private.example && fail "private email published"
git -C "$T/public.git" log -1 --format=%s main | grep -q "Merge pull request" && fail "private PR number published"

# up to date
run | grep -q "up to date" || fail "second run is a no-op"

# a leak in new work blocks everything
echo "secret-codename again" > "$T/work/c.txt"; g add -A; g commit -qm "oops"; g push -q origin main
if run --push >"$T/out" 2>&1; then fail "published a leak"; fi
grep -q "LEAK" "$T/out" || fail "names the leak"
[ "$(git -C "$T/public.git" rev-parse main)" = "$P" ] || fail "pushed despite a leak"
g rm -q c.txt; echo four > "$T/work/d.txt"; g add -A; g commit -qm "remove, add d"; g push -q origin main
# removed later, but the in-between commit still has it: one-per-commit stays blocked, --squash publishes only the fix
if run >"$T/o2" 2>&1; then cat "$T/o2"; fail "published the in-between leaky tree"; fi
run --squash --push >/dev/null 2>&1 || fail "squash after the fix"
[ "$(git -C "$T/public.git" rev-list --count main)" -eq 4 ] || fail "squash adds one commit"
git -C "$T/public.git" grep -q secret-codename main && fail "squash published the leak"
git -C "$T/public.git" log -1 --format=%B main | grep -q "^- oops" || fail "squash lists the commits"

# a leak only in a commit message also blocks, squashed or not
echo five > "$T/work/e.txt"; g add -A; g commit -qm "mention secret-codename"; g push -q origin main
if run >/dev/null 2>&1; then fail "published a leaking message"; fi
if run --squash >/dev/null 2>&1; then fail "squash published a leaking message"; fi

# no denylist: refuses
rm "$T/work/.git/info/public-denylist"
if run >/dev/null 2>&1; then fail "ran without a denylist"; fi

echo "tools/mirror-public.sh: all tests passed"
