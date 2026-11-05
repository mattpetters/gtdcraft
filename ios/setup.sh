#!/bin/sh
# One-time setup for building GTDCraft onto your own iPhone or iPad: writes ios/Config/Local.xcconfig (git-ignored)
# with your Apple team ID, a bundle ID and an App Group that are yours.
#
# Usage (from app/):  ./ios/setup.sh [--team TEAMID] [--bundle-id com.you.gtdcraft] [--dry-run] [--force]
#   --team       your Apple Developer team ID; default: found from the "Apple Development" certificate in your keychain
#                (Xcode makes one when you sign in under Xcode › Settings › Accounts; a free Apple ID works)
#   --bundle-id  default: dev.<your user name>.gtdcraft (any unique reverse-DNS name; free accounts can't reuse others')
#   --dry-run    print what would be written, change nothing
#   --force      replace an existing Local.xcconfig
set -e
DIR=$(cd "$(dirname "$0")" && pwd)
OUT="$DIR/Config/Local.xcconfig"
TEAM="" BUNDLE="" DRY=0 FORCE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --team) TEAM="$2"; shift 2 ;;
    --bundle-id) BUNDLE="$2"; shift 2 ;;
    --dry-run) DRY=1; shift ;;
    --force) FORCE=1; shift ;;
    -h|--help) sed -n '2,11p' "$0"; exit 0 ;;
    *) echo "Unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
done

# team IDs are the OU of the signing certificates Xcode created
teams() {
  security find-certificate -a -c "Apple Development" -p 2>/dev/null |
    awk '/BEGIN CERT/{c=""} {c=c $0 "\n"} /END CERT/{printf "%s", c | "openssl x509 -noout -subject"; close("openssl x509 -noout -subject")}' |
    sed -n 's/.*OU *= *\([A-Z0-9]\{10\}\).*/\1/p' | sort -u
}

if [ -z "$TEAM" ]; then
  FOUND=$(teams)
  COUNT=$(printf '%s' "$FOUND" | grep -c . || true)
  if [ "$COUNT" -eq 1 ]; then
    TEAM="$FOUND"
    echo "Found your Apple team: $TEAM"
  elif [ "$COUNT" -gt 1 ]; then
    echo "More than one Apple team in your keychain; pick one with --team:" >&2
    printf '  %s\n' $FOUND >&2
    exit 1
  else
    echo "No Apple Development certificate found. In Xcode › Settings › Accounts, sign in with your Apple ID," >&2
    echo "select your team and click Manage Certificates › + › Apple Development. Then run this again, or pass --team." >&2
    exit 1
  fi
fi
case "$TEAM" in
  [A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9]) ;;
  *) echo "'$TEAM' doesn't look like a team ID (10 letters and digits)." >&2; exit 1 ;;
esac

if [ -z "$BUNDLE" ]; then
  USERPART=$(id -un | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9')
  BUNDLE="dev.${USERPART:-me}.gtdcraft"
fi
case "$BUNDLE" in
  *[!A-Za-z0-9.-]*|.*|*.|*..*) echo "'$BUNDLE' isn't a valid bundle ID (letters, digits, dots and hyphens)." >&2; exit 1 ;;
esac

CONTENT="// Written by ios/setup.sh. Git-ignored: your own signing settings.
DEVELOPMENT_TEAM = $TEAM
GTD_BUNDLE_ID = $BUNDLE
GTD_APP_GROUP = group.$BUNDLE"

if [ "$DRY" -eq 1 ]; then
  echo "Would write $OUT:"
  printf '%s\n' "$CONTENT"
  exit 0
fi
if [ -e "$OUT" ] && [ "$FORCE" -ne 1 ]; then
  echo "$OUT already exists; pass --force to replace it. It has:" >&2
  cat "$OUT" >&2
  exit 1
fi
printf '%s\n' "$CONTENT" > "$OUT"
echo "Wrote $OUT. Next: ./make-ios-device.sh (or open the project after: cd ios && xcodegen generate)."
