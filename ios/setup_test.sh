#!/bin/sh
# Tests for ios/setup.sh with a fake `security` and `openssl` on PATH and a throwaway copy of ios/. Run from app/.
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
fail() { echo "FAIL: $1" >&2; exit 1; }

# a sandbox copy of ios/ so the real Config/Local.xcconfig is never touched
mkdir -p "$T/ios/Config" "$T/bin"
cp "$ROOT/ios/setup.sh" "$T/ios/setup.sh"

# fakes: each "certificate" is one PEM block whose body names the team; openssl prints a subject with that OU
fake() {
  cat > "$T/bin/security" <<EOF
#!/bin/sh
for t in $*; do printf -- '-----BEGIN CERTIFICATE-----\n%s\n-----END CERTIFICATE-----\n' "\$t"; done
EOF
  cat > "$T/bin/openssl" <<'EOF'
#!/bin/sh
t=$(sed -n '2p'); echo "subject=UID=X, CN=Apple Development: someone (ZZZZZZZZZZ), OU=$t, O=Someone, C=US"
EOF
  chmod +x "$T/bin/security" "$T/bin/openssl"
}
run() { PATH="$T/bin:$PATH" sh "$T/ios/setup.sh" "$@"; }
OUT="$T/ios/Config/Local.xcconfig"

# one team found: dry run writes nothing and shows it
fake ABCDE12345
run --dry-run --bundle-id com.example.gtd | grep -q "DEVELOPMENT_TEAM = ABCDE12345" || fail "dry run shows the found team"
[ ! -e "$OUT" ] || fail "dry run wrote a file"

# real run writes team, bundle ID and the matching App Group
run --bundle-id com.example.gtd >/dev/null
grep -q "^DEVELOPMENT_TEAM = ABCDE12345$" "$OUT" || fail "team written"
grep -q "^GTD_BUNDLE_ID = com.example.gtd$" "$OUT" || fail "bundle ID written"
grep -q "^GTD_APP_GROUP = group.com.example.gtd$" "$OUT" || fail "App Group derived from bundle ID"

# an existing file is kept unless --force
if run --bundle-id com.other.gtd >/dev/null 2>&1; then fail "overwrote without --force"; fi
grep -q "com.example.gtd" "$OUT" || fail "existing file kept"
run --force --bundle-id com.other.gtd >/dev/null && grep -q "com.other.gtd" "$OUT" || fail "--force replaces"
rm -f "$OUT"

# two teams: refuses and lists them; --team picks one
fake ABCDE12345 FGHIJ67890
if run 2>"$T/err"; then fail "picked a team when there were two"; fi
grep -q "FGHIJ67890" "$T/err" || fail "lists the teams"
run --team FGHIJ67890 --bundle-id com.example.gtd >/dev/null && grep -q "FGHIJ67890" "$OUT" || fail "--team chooses"
rm -f "$OUT"

# no certificate: explains how to get one
fake
if run 2>"$T/err"; then fail "succeeded with no certificate"; fi
grep -q "Manage Certificates" "$T/err" || fail "explains how to make a certificate"

# bad input is refused
if run --team short 2>/dev/null; then fail "accepted a bad team ID"; fi
if run --team ABCDE12345 --bundle-id "bad id" 2>/dev/null; then fail "accepted a bad bundle ID"; fi

echo "ios/setup.sh: all tests passed"
