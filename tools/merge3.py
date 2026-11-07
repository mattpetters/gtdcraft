#!/usr/bin/env python3
"""3-way merge of GTDCraft databases: base (last common sync) + mac + phone -> merged + conflicts report.

Rules, in order (anything not covered lands in conflicts.json for a human or agent):
  1. Record changed on one side only -> take that side.
  2. Both changed -> field by field: take whichever side changed the field.
     Same field changed differently on both:
       - id-set fields (tags): 3-way set merge (adds and removes from both sides)
       - clock: union by start time
       - anything else: newer `modified` wins, logged as a conflict
  3. Deleted on one side, edited on the other -> keep the edit (preserve work), drop the tombstone, log it.
  4. Added on either side -> keep (device ID ranges never collide); order = mac's, phone-only after their neighbour.
  5. Up Next queue: 3-way; both changed -> mac's order plus phone's additions.
  6. Shared settings groups (layout, connections): newer stamp wins (the app's own rule).
  7. Tombstones, pomodoros, link cards, Linear cache: union. next_id: max.
Records whose result differs from a device's copy get `modified = now`, so the app's newest-wins sync keeps them.
usage: merge3.py BASE MAC PHONE OUT.(json|jsonl) [CONFLICTS.json]
"""
import json, sys
from datetime import datetime

KINDS = {"task": "tasks", "project": "projects", "tag": "tags", "folder": "folders"}
NOW = datetime.now().strftime("%Y-%m-%dT%H:%M:%S")
LAYOUT = ["perspectives", "inspector_order", "layout_modified"]
CONNECTIONS = ["linear_key", "github_token", "ai_provider", "ai_key", "ai_model", "ai_base_url", "decisions_on",
               "decisions_provider", "decisions_url", "decisions_model", "decisions_key", "decisions_account",
               "agent_templates", "connections_modified"]


def load(p):
    if not p.endswith(".jsonl"):
        return json.load(open(p))
    s = {}
    for line in open(p):
        if line.strip():
            r = json.loads(line)
            k = r.pop("_record")
            s.update(r) if k == "meta" else s.setdefault(KINDS[k], []).append(r)
    return s


def save(s, p):
    if not p.endswith(".jsonl"):
        return json.dump(s, open(p, "w"), indent=2)
    with open(p, "w") as f:
        f.write(json.dumps({"_record": "meta", **{k: v for k, v in s.items() if k not in KINDS.values()}}) + "\n")
        for rec, key in KINDS.items():
            for r in s.get(key, []):
                f.write(json.dumps({"_record": rec, **r}) + "\n")


def set3(b, m, p):
    b, m, p = set(b or []), set(m or []), set(p or [])
    return sorted((b | (m - b) | (p - b)) - (b - m) - (b - p))


def merge_record(b, m, p, conflicts, kind):
    out = dict(m)
    for f in (set(b) | set(m) | set(p)) - {"modified"}:
        bv, mv, pv = b.get(f), m.get(f), p.get(f)
        if mv == pv or pv == bv:
            out[f] = mv
        elif mv == bv:
            out[f] = pv
        elif f == "tags":
            out[f] = set3(bv, mv, pv)
        elif f == "clock":
            seen = {c["start"]: c for c in (mv or []) + (pv or [])}
            out[f] = [seen[k] for k in sorted(seen)]
        else:
            newer = m if (m.get("modified") or "") >= (p.get("modified") or "") else p
            out[f] = newer.get(f)
            conflicts.append({"kind": kind, "id": m["id"], "name": m.get("name"), "field": f, "rule": "newer modified wins",
                              "base": bv, "mac": mv, "phone": pv, "chosen": out[f]})
    out["modified"] = NOW
    return out


def merge(base, mac, phone):
    conflicts, out = [], dict(mac)
    deleted = {**base.get("deleted", {})}
    for s in (mac, phone):
        for k, v in s.get("deleted", {}).items():
            deleted[k] = max(v, deleted.get(k, v))
    for kind, key in KINDS.items():
        B = {r["id"]: r for r in base.get(key, [])}
        M = {r["id"]: r for r in mac.get(key, [])}
        P = {r["id"]: r for r in phone.get(key, [])}
        result = {}
        for i in set(B) | set(M) | set(P):
            b, m, p = B.get(i), M.get(i), P.get(i)
            if b is None:  # added (or both added the same id: treat like a both-changed record)
                result[i] = merge_record({}, m, p, conflicts, kind) if m and p and m != p else (m or p)
            elif m is None and p is None:
                continue
            elif m is None or p is None:
                alive = m or p
                if alive != b:  # rule 3
                    result[i] = dict(alive, modified=NOW)
                    deleted.pop(str(i), None)
                    conflicts.append({"kind": kind, "id": i, "name": b.get("name"), "rule": "deleted on one side, edited on the other: kept the edit",
                                      "deleted_on": "mac" if m is None else "phone"})
            elif m == b:
                result[i] = p
            elif p == b or m == p:
                result[i] = m
            else:
                result[i] = merge_record(b, m, p, conflicts, kind)
        order = [r["id"] for r in mac.get(key, []) if r["id"] in result]
        prev = None
        for r in phone.get(key, []):
            if r["id"] in result and r["id"] not in order:
                order.insert(order.index(prev) + 1 if prev in order else len(order), r["id"])
            prev = r["id"]
        order += [i for i in result if i not in order]  # deleted-on-mac edits
        out[key] = [result[i] for i in order]
    out["deleted"] = deleted
    # Up Next (rule 5)
    bq, mq, pq = base.get("queue", []), mac.get("queue", []), phone.get("queue", [])
    q = mq if (mq == pq or pq == bq) else pq if mq == bq else mq + [i for i in pq if i not in mq]
    live = {t["id"] for t in out["tasks"]}
    out["queue"] = [i for i in q if i in live]
    if out["queue"] != mq or out["queue"] != pq:
        out["queue_modified"] = NOW
    # shared settings (rule 6): out has mac's settings; take phone's groups where newer
    st = out["settings"] = dict(mac["settings"])
    for group, stamp in ((LAYOUT, "layout_modified"), (CONNECTIONS, "connections_modified")):
        if (phone["settings"].get(stamp) or "") > (st.get(stamp) or ""):
            st.update({k: phone["settings"].get(k) for k in group})
    # rule 7
    out["pomodoros"] = sorted({json.dumps(p): p for p in mac.get("pomodoros", []) + phone.get("pomodoros", [])}.values(), key=lambda p: p[0])
    for k in ("link_cards", "linear"):
        out[k] = {**phone.get(k, {}), **mac.get(k, {})}
    out["next_id"] = max(base.get("next_id", 0), mac.get("next_id", 0), phone.get("next_id", 0))
    return out, conflicts


def test():
    t = lambda i, **kw: {"id": i, "name": f"t{i}", "tags": [], "clock": [], "modified": "2026-01-01T00:00:00", **kw}
    base = {"tasks": [t(1), t(2), t(3), t(4)], "deleted": {}, "queue": [], "settings": {}}
    mac = {"tasks": [t(1, name="mac", tags=[7], modified="2026-01-02T00:00:00"), t(3, note="m", modified="2026-01-02T00:00:00"), t(4), t(10)],
           "deleted": {"2": "2026-01-02T00:00:00"}, "queue": [1], "settings": {}}
    phone = {"tasks": [t(1, note="p", tags=[8], modified="2026-01-03T00:00:00"), t(2, name="edited", modified="2026-01-03T00:00:00"),
                       t(3, note="p", modified="2026-01-03T00:00:00"), t(20)], "deleted": {"4": "2026-01-03T00:00:00"}, "queue": [], "settings": {}}
    out, c = merge(base, mac, phone)
    by = {r["id"]: r for r in out["tasks"]}
    assert by[1]["name"] == "mac" and by[1]["note"] == "p" and by[1]["tags"] == [7, 8]  # disjoint fields + set merge
    assert by[2]["name"] == "edited" and "2" not in out["deleted"]  # delete vs edit keeps the edit
    assert by[3]["note"] == "p" and any(x.get("field") == "note" for x in c)  # true conflict: newer wins, logged
    assert 4 not in by and 10 in by and 20 in by  # clean delete, both additions
    assert out["queue"] == [1] and out["queue_modified"] == NOW  # timestamp-less queue isn't lost
    print("merge3 self-test ok")


if __name__ == "__main__":
    if sys.argv[1:] == ["--test"]:
        test(); sys.exit()
    base, mac, phone = (load(p) for p in sys.argv[1:4])
    out, conflicts = merge(base, mac, phone)
    save(out, sys.argv[4])
    json.dump(conflicts, open(sys.argv[5] if len(sys.argv) > 5 else "conflicts.json", "w"), indent=2, default=str)
    print(f"tasks {len(out['tasks'])}, projects {len(out['projects'])}, tags {len(out['tags'])}, conflicts {len(conflicts)}")
