# Sync recovery: two devices that drifted apart

When the Mac and the iPhone stop reaching the sync server, both keep editing offline. Normal sync keeps the newer
copy of each record, which loses work when both sides edited the same record. It also loses changes to the Up Next
queue, which has no timestamp. Merge the copies 3-way instead, using the server's copy as the common base.

First case: 2026-11-05. The Mac's `.lan` name stopped resolving and both devices were set to it. Fix: `.local` addresses
(Bonjour), which `phone_server()` already uses.

## 1. Diagnose

```sh
curl -s -m5 http://<mac>.local:8765/v1/health      # the server itself
curl -s -m5 http://<what-devices-use>:8765/v1/health  # the URL in Settings › Sync
tail ~/Library/Application\ Support/GTDCraft/sync-server.log
```

## 2. Capture all three copies (nothing mutates yet)

```sh
B=~/gtdcraft-backups/merge-$(date +%Y%m%d); mkdir -p $B
S=~/Library/Application\ Support/GTDCraft
cp -Rp "$S/sync-server" $B/server-data                   # base: the last common sync
cp -p "$(cat "$S/current-database")" $B/mac.jsonl         # the Mac's open database
xcrun devicectl device copy from --device <UDID> --domain-type appDataContainer \
  --domain-identifier <your GTD_BUNDLE_ID from ios/Config/Local.xcconfig> \
  --source "Library/Application Support/GTDCraft/gtdcraft.json" --destination $B/phone.json
```

To be a valid base, the server copy must be at least as old as both devices' last sync. Each device writes its
`sync-db` file when it syncs; check those times.

## 3. Merge

```sh
tools/merge3.py --test
tools/merge3.py $B/server-data/gtdcraft.jsonl $B/mac.jsonl $B/phone.json $B/merged.jsonl $B/conflicts.json
```

The rules are listed at the top of `tools/merge3.py`. `conflicts.json` lists only the same-field clashes and the
delete-versus-edit cases. Have a person or an agent review those. Everything else merges mechanically.

## 4. Reset the server and the Mac, then re-pair the phone

```sh
launchctl bootout gui/$(id -u)/local.gtdcraft.syncserver
cp $B/merged.jsonl "$S/sync-server/gtdcraft.jsonl"
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/local.gtdcraft.syncserver.plist
```

On the Mac, set the database's `settings.sync_url` to the `.local` address and put the merged file where
`current-database` points. On the phone, scan the QR code from Settings › Sync on the Mac and tap Replace Settings.
This replaces only the server address and token. The phone then syncs and receives the merged data.

## Next step

The server can't run this merge itself, because it doesn't know each device's base. To fix that, a device would keep
a copy of what it last synced and send it with `/v1/sync`, and `sync.rs` would run these rules in Rust.
