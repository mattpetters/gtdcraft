//! Built-in MCP server: `gtdcraft --mcp` speaks JSON-RPC 2.0 over stdio (one message per line).
//! It reads and writes the same database file as the window; the window reloads on change.
use crate::model::*;
use chrono::{NaiveDateTime, NaiveTime};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;

pub fn run(path: &Path) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(path, &msg),
            Err(e) => Some(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":e.to_string()}})),
        };
        if let Some(r) = reply {
            let _ = writeln!(out, "{r}");
            let _ = out.flush();
        }
    }
}

pub fn handle(path: &Path, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    let id = id?; // notifications get no reply
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").cloned().unwrap_or(json!("2025-06-18")),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "gtdcraft", "version": env!("CARGO_PKG_VERSION")},
            "instructions": format!("GTDCraft task manager. Tasks have a computed status: available, next, due_soon, overdue, blocked, completed, dropped. Dates are local 'YYYY-MM-DD' or 'YYYY-MM-DD HH:MM'. Start with gtd_overview. {}", crate::agent::FIELD_RULES)
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            Ok(match call(path, name, &args) {
                Ok(v) => json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&v).unwrap()}]}),
                Err(e) => json!({"content":[{"type":"text","text":e}],"isError":true}),
            })
        }
        _ => Err(json!({"code":-32601,"message":format!("unknown method {method}")})),
    };
    Some(match result {
        Ok(r) => json!({"jsonrpc":"2.0","id":id,"result":r}),
        Err(e) => json!({"jsonrpc":"2.0","id":id,"error":e}),
    })
}

fn tools() -> Value {
    let date = json!({"type":"string","description":"'YYYY-MM-DD' or 'YYYY-MM-DD HH:MM' (local), or '' to clear"});
    let task_fields = json!({
        "name":{"type":"string"},
        "note":{"type":"string"},
        "project":{"type":"string","description":"project name or id; '' = Inbox"},
        "parent_id":{"type":"integer","description":"make it a sub-action of this task"},
        "tags":{"type":"array","items":{"type":"string"},"description":"tag names (created if missing)"},
        "flagged":{"type":"boolean"},
        "defer":date, "planned":date, "due":date,
        "duration_minutes":{"type":"integer"},
        "repeat":{"type":"string","description":"'<n> <minutes|hours|days|weeks|months|years> [regularly|from_completion] [catch_up] [based_on:defer|planned|due] [end_after:N]', e.g. '1 week', '3 days from_completion', '1 month catch_up'; '' stops repeating"}
    });
    let mut add_props = task_fields.clone();
    add_props["name"] = json!({"type":"string"});
    let mut upd_props = task_fields;
    upd_props["id"] = json!({"type":"integer"});
    let mut v = json!([
        {"name":"gtd_overview","description":"Counts for Inbox, Flagged, Forecast (overdue / due soon), projects due for review.","inputSchema":{"type":"object","properties":{}}},
        {"name":"gtd_list_tasks","description":"List tasks with computed status. view: inbox | available | flagged | forecast | completed | changed | all. Optional project, tag, query filters.",
         "inputSchema":{"type":"object","properties":{"view":{"type":"string"},"project":{"type":"string"},"tag":{"type":"string"},"query":{"type":"string"},"limit":{"type":"integer"}}}},
        {"name":"gtd_add_task","description":"Add a task (to the Inbox unless project or parent_id is given).","inputSchema":{"type":"object","properties":add_props,"required":["name"]}},
        {"name":"gtd_update_task","description":"Change fields of a task. Only the given fields change.","inputSchema":{"type":"object","properties":upd_props,"required":["id"]}},
        {"name":"gtd_set_task_status","description":"status: completed | dropped | dropped_completely | active. Completing or dropping a repeating task creates its next occurrence (skips this one); dropped_completely stops the repetition.",
         "inputSchema":{"type":"object","properties":{"id":{"type":"integer"},"status":{"type":"string"}},"required":["id","status"]}},
        {"name":"gtd_delete_task","description":"Delete a task and its sub-actions.","inputSchema":{"type":"object","properties":{"id":{"type":"integer"}},"required":["id"]}},
        {"name":"gtd_list_projects","description":"Projects with type, status, folder, review dates and remaining counts.","inputSchema":{"type":"object","properties":{"include_done":{"type":"boolean"}}}},
        {"name":"gtd_add_project","description":"Add a project. kind: parallel | sequential | single_actions.",
         "inputSchema":{"type":"object","properties":{"name":{"type":"string"},"kind":{"type":"string"},"folder":{"type":"string"},"note":{"type":"string"},"review_every_days":{"type":"integer"}},"required":["name"]}},
        {"name":"gtd_update_project","description":"Change a project. status: active | on_hold | completed | dropped.",
         "inputSchema":{"type":"object","properties":{"id":{"type":"integer"},"name":{"type":"string"},"kind":{"type":"string"},"status":{"type":"string"},"folder":{"type":"string"},"note":{"type":"string"},"flagged":{"type":"boolean"},"review_every_days":{"type":"integer"}},"required":["id"]}},
        {"name":"gtd_review","description":"Projects due for review today, or mark one reviewed with mark_reviewed_id.",
         "inputSchema":{"type":"object","properties":{"mark_reviewed_id":{"type":"integer"}}}},
        {"name":"gtd_list_tags","description":"Tags with their available-action counts.","inputSchema":{"type":"object","properties":{}}},
        {"name":"gtd_create_perspective","description":"Create a custom perspective (saved filter) shown in GTDCraft's sidebar. rules use kinds: availability (value first|available|remaining|completed|dropped), status (flagged|due_soon|overdue|due_soon_or_flagged), tag/project/folder (value = exact name), untagged, inbox, due_within/defer_within/planned_within (number = days ahead), added_within/modified_within/completed_within (number = days back), has_due/has_defer/repeats/has_note/unestimated (value yes|no), duration_at_most (number = minutes), text (value = words). negate=true excludes matches.",
         "inputSchema": crate::ai::schema()},
        {"name":"gtd_list_perspectives","description":"Perspectives in sidebar order, with their rules.","inputSchema":{"type":"object","properties":{}}},
        {"name":"gtd_web_activity","description":"What the person did in Chrome (from the GTDCraft extension): time on task vs distraction, top sites, searches and YouTube videos, for the last N days. Use it to help them notice procrastination or summarize their day.","inputSchema":{"type":"object","properties":{"days":{"type":"integer","description":"1-90, default 1 (today)"}}}},
        {"name":"gtd_calendar_events","description":"The person's macOS Calendar events (same calendars and all-day setting as GTDCraft's Forecast), so you can plan around meetings. Needs Calendar access granted to GTDCraft.","inputSchema":{"type":"object","properties":{"from":{"type":"string","description":"'YYYY-MM-DD', default today"},"days":{"type":"integer","description":"1-35, default 1"}}}},
        {"name":"gtd_link_linear","description":"Link a Linear issue (ID like ENG-123 or a linear.app URL) to a task or project; '' unlinks.",
         "inputSchema":{"type":"object","properties":{"task_id":{"type":"integer"},"project_id":{"type":"integer"},"issue":{"type":"string"}},"required":["issue"]}},
        {"name":"gtd_decide","description":"Ask the System One decision model the person turned on (Clef, Jev, Rune…) typed questions about a state or about tasks; it returns a probability per answer (fast: about 0.2 s for a short state on local Clef). questions: {name: {type, instructions, criteria}} with type noul (yes/no; answer {noul: chance of yes}), choice (criteria = {option: description}; answer {choice, confidence, probabilities}) or score (criteria = [ordered levels]; answer {score, confidence, probabilities}). Give state (any JSON or text, e.g. an email or a message thread) or task_ids (each task is judged on its name, note, project, tags, dates and references). Reads only; changes nothing.",
         "inputSchema":{"type":"object","properties":{"questions":{"type":"object"},"state":{},"task_ids":{"type":"array","items":{"type":"integer"}}},"required":["questions"]}},
        {"name":"gtd_batch","description":"Run several gtd_* tool calls in order (a plan of changes in one step); each result is returned in order, and the batch stops at the first error. calls: [{name, arguments}].",
         "inputSchema":{"type":"object","properties":{"calls":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"arguments":{"type":"object"}},"required":["name"]}}},"required":["calls"]}}
    ]);
    // TEMPORARY: remove this push with src/decisions_url_set.rs.
    v.as_array_mut().expect("tool list").push(crate::decisions_url_set::tool_spec());
    v
}

/// Dates as people type them ("tomorrow 5pm", "fri", "+3d", "2026-11-15 17:00"); see dates.rs.
pub fn parse_date(s: &str, default_time: NaiveTime) -> Result<Option<NaiveDateTime>, String> {
    crate::dates::parse_when(s, now(), default_time)
}

/// "<n> <unit> [regularly|from_completion] [catch_up] [based_on:defer|planned|due] [end_after:N]"
pub fn parse_repeat(s: &str) -> Result<Option<Repeat>, String> {
    let w: Vec<&str> = s.split_whitespace().collect();
    if w.is_empty() {
        return Ok(None);
    }
    let mut r = Repeat::default();
    let rest = match w[0].parse::<u32>() {
        Ok(n) => {
            r.every = n.max(1);
            &w[1..]
        }
        Err(_) => &w[..],
    };
    r.unit = match rest.first().map(|u| u.trim_end_matches('s')) {
        Some("minute") => RepeatUnit::Minute,
        Some("hour") => RepeatUnit::Hour,
        Some("day") => RepeatUnit::Day,
        Some("week") => RepeatUnit::Week,
        Some("month") => RepeatUnit::Month,
        Some("year") => RepeatUnit::Year,
        _ => return Err(format!("can't read repeat '{s}', e.g. '2 weeks from_completion'")),
    };
    for word in &rest[1..] {
        match *word {
            "regularly" => r.schedule = RepeatSchedule::Regularly,
            "from_completion" => r.schedule = RepeatSchedule::FromCompletion,
            "catch_up" => r.catch_up = true,
            "based_on:defer" => r.based_on = BasedOn::Defer,
            "based_on:planned" => r.based_on = BasedOn::Planned,
            "based_on:due" => r.based_on = BasedOn::Due,
            x if x.starts_with("end_after:") => r.end_after = Some(x[10..].parse().map_err(|_| format!("bad {x}"))?),
            x => return Err(format!("unknown repeat option '{x}'")),
        }
    }
    Ok(Some(r))
}

fn resolve_project(s: &Store, v: &Value) -> Result<Option<Id>, String> {
    if let Some(n) = v.as_u64() {
        return s.project(n).map(|p| Some(p.id)).ok_or(format!("no project {n}"));
    }
    let name = v.as_str().unwrap_or("").trim();
    if name.is_empty() || name.eq_ignore_ascii_case("inbox") {
        return Ok(None);
    }
    if let Ok(n) = name.parse::<u64>() {
        if s.project(n).is_some() {
            return Ok(Some(n));
        }
    }
    s.project_by_name(name).map(Some).ok_or(format!("no project named '{name}' (add it with gtd_add_project)"))
}

fn tag_ids(s: &mut Store, v: &Value) -> Vec<Id> {
    let mut out = vec![];
    for n in v.as_array().into_iter().flatten().filter_map(Value::as_str) {
        // "Parent : Child" nests
        if let Some(id) = s.tag_by_name(n).or_else(|| s.tag_for_path(n)) {
            out.push(id);
        }
    }
    out
}

fn fmt(d: &Option<NaiveDateTime>) -> Value {
    d.map(|d| json!(d.format("%Y-%m-%d %H:%M").to_string())).unwrap_or(Value::Null)
}

pub fn task_json(s: &Store, d: &Derived, t: &Task) -> Value {
    json!({
        "id": t.id, "name": t.name, "status": d.status.get(&t.id),
        "project": t.project.and_then(|p| s.project(p)).map(|p| p.name.clone()),
        "parent_id": t.parent, "flagged": t.flagged,
        "tags": t.tags.iter().filter_map(|g| s.tag(*g)).map(|g| g.name.clone()).collect::<Vec<_>>(),
        "defer": fmt(&t.defer), "planned": fmt(&t.planned), "due": fmt(&t.due),
        "completed": fmt(&t.completed), "dropped": fmt(&t.dropped),
        "duration_minutes": t.duration_min, "repeat": t.repeat.map(|r| r.label()),
        "note": if t.note.is_empty() { Value::Null } else { json!(t.note) },
        "linear": t.linear.as_ref().map(|k| match s.linear.get(k) { Some(i) => json!({"id": k, "state": i.state.name, "title": i.title, "url": i.url}), None => json!({"id": k}) }),
    })
}

fn apply_fields(s: &mut Store, id: Id, a: &Value) -> Result<(), String> {
    let st = s.settings.clone();
    if let Some(v) = a.get("project") {
        let p = resolve_project(s, v)?;
        s.set_project(id, p);
    }
    if let Some(pid) = a.get("parent_id").and_then(Value::as_u64) {
        if pid == id {
            return Err("a task can't be its own parent".into());
        }
        let proj = s.task(pid).ok_or(format!("no task {pid}"))?.project;
        s.set_project(id, proj);
        s.task_mut(id).unwrap().parent = Some(pid);
    }
    let tags = a.get("tags").map(|v| tag_ids(s, v));
    let mut dates = vec![];
    for (k, def) in [("defer", st.default_defer_time), ("planned", st.default_planned_time), ("due", st.default_due_time)] {
        if let Some(v) = a.get(k) {
            dates.push((k, parse_date(v.as_str().unwrap_or(""), def)?));
        }
    }
    let repeat = match a.get("repeat") {
        Some(v) => Some(parse_repeat(v.as_str().unwrap_or(""))?),
        None => None,
    };
    let t = s.task_mut(id).ok_or(format!("no task {id}"))?;
    if let Some(v) = a.get("name").and_then(Value::as_str) {
        t.name = v.into();
    }
    if let Some(v) = a.get("note").and_then(Value::as_str) {
        t.note = v.into();
    }
    if let Some(v) = a.get("flagged").and_then(Value::as_bool) {
        t.flagged = v;
    }
    if let Some(v) = a.get("duration_minutes") {
        t.duration_min = v.as_u64().map(|m| m as u32);
    }
    if let Some(tags) = tags {
        t.tags = tags;
    }
    for (k, d) in dates {
        match k {
            "defer" => t.defer = d,
            "planned" => t.planned = d,
            _ => t.due = d,
        }
    }
    if let Some(r) = repeat {
        t.repeat = r;
    }
    t.modified = now();
    Ok(())
}

fn id_arg(a: &Value) -> Result<Id, String> {
    a.get("id").and_then(Value::as_u64).ok_or("id is required".into())
}

pub fn call(path: &Path, name: &str, a: &Value) -> Result<Value, String> {
    if name == "gtd_batch" {
        let mut out = vec![];
        for (i, c) in a.get("calls").and_then(Value::as_array).ok_or("calls: a list of {name, arguments}")?.iter().enumerate() {
            let n = c.get("name").and_then(Value::as_str).ok_or(format!("call {i}: no name"))?;
            // agents often pass the name they see ("mcp__gtdcraft__gtd_add_task"): the part after the last "__"
            let n = if n.starts_with("mcp__") { n.rsplit("__").next().unwrap_or(n) } else { n };
            if n == "gtd_batch" {
                return Err("a batch can't contain a batch".into());
            }
            let r = call(path, n, c.get("arguments").unwrap_or(&json!({}))).map_err(|e| format!("call {i} ({n}): {e}"))?;
            out.push(r);
        }
        return Ok(json!(out));
    }
    // TEMPORARY: remove this dispatch with src/decisions_url_set.rs. It must stay ahead of
    // load_or_new so a URL write cannot create a database or rewrite any other field.
    if name == crate::decisions_url_set::TOOL_NAME {
        return crate::decisions_url_set::set_decisions_url(path, a);
    }
    let mut s = Store::load_or_new(path);
    // this device's ID range (sync); an agent thread on the sync server mints from the server's (GTDCRAFT_ID_BASE)
    s.id_base = std::env::var("GTDCRAFT_ID_BASE").ok().and_then(|v| v.parse().ok()).unwrap_or_else(crate::sync::device_base);
    let n = now();
    let mut dirty = true;
    let out = match name {
        "gtd_overview" => {
            dirty = false;
            let d = s.derive(n);
            let rem = |t: &&Task| d.st(t.id).remaining();
            json!({
                "inbox": s.tasks.iter().filter(|t| t.project.is_none()).filter(rem).count(),
                "flagged_available": s.tasks.iter().filter(|t| t.flagged && d.st(t.id).available()).count(),
                "overdue": d.status.values().filter(|x| **x == Status::Overdue).count(),
                "due_soon": d.status.values().filter(|x| **x == Status::DueSoon).count(),
                "available": d.status.values().filter(|x| x.available()).count(),
                "projects_to_review": s.projects.iter().filter(|p| p.next_review <= n.date() && matches!(p.status, ProjectStatus::Active | ProjectStatus::OnHold)).count(),
                "due_soon_until": s.due_soon_cutoff(n).format("%Y-%m-%d %H:%M:%S").to_string(),
            })
        }
        "gtd_list_tasks" => {
            dirty = false;
            let d = s.derive(n);
            let view = a.get("view").and_then(Value::as_str).unwrap_or("available");
            let proj = match a.get("project") {
                Some(v) if !v.is_null() => Some(resolve_project(&s, v)?),
                _ => None,
            };
            let tag = a.get("tag").and_then(Value::as_str).map(|t| s.tag_by_name(t).ok_or(format!("no tag '{t}'"))).transpose()?;
            let q = a.get("query").and_then(Value::as_str).unwrap_or("").to_lowercase();
            let limit = a.get("limit").and_then(Value::as_u64).unwrap_or(200) as usize;
            let cutoff = s.due_soon_cutoff(n);
            let mut v: Vec<&Task> = s
                .tasks
                .iter()
                .filter(|t| {
                    let st = d.st(t.id);
                    let keep = match view {
                        "inbox" => t.project.is_none() && st.remaining(),
                        "flagged" => t.flagged && st.available(),
                        "forecast" => st.remaining() && (t.due.is_some_and(|x| x < cutoff + chrono::Duration::days(5)) || t.planned.is_some_and(|x| x.date() <= n.date())),
                        "completed" => st == Status::Completed,
                        "changed" => true,
                        "all" => true,
                        _ => st.available(),
                    };
                    keep && proj.is_none_or(|p| t.project == p)
                        && tag.is_none_or(|g| t.tags.contains(&g))
                        && (q.is_empty() || t.name.to_lowercase().contains(&q) || t.note.to_lowercase().contains(&q))
                })
                .collect();
            match view {
                "changed" => v.sort_by_key(|t| std::cmp::Reverse(t.modified)),
                "completed" => v.sort_by_key(|t| std::cmp::Reverse(t.completed)),
                "forecast" => v.sort_by_key(|t| t.due.or(t.planned)),
                _ => {}
            }
            json!(v.into_iter().take(limit).map(|t| task_json(&s, &d, t)).collect::<Vec<_>>())
        }
        "gtd_add_task" => {
            let name = a.get("name").and_then(Value::as_str).filter(|x| !x.trim().is_empty()).ok_or("name is required")?;
            let id = s.add_task(name, None, None);
            if let Err(e) = apply_fields(&mut s, id, a) {
                return Err(e); // nothing saved
            }
            let d = s.derive(n);
            task_json(&s, &d, s.task(id).unwrap())
        }
        "gtd_update_task" => {
            let id = id_arg(a)?;
            apply_fields(&mut s, id, a)?;
            let d = s.derive(n);
            task_json(&s, &d, s.task(id).unwrap())
        }
        "gtd_set_task_status" => {
            let id = id_arg(a)?;
            s.task(id).ok_or(format!("no task {id}"))?;
            match a.get("status").and_then(Value::as_str).unwrap_or("") {
                "completed" => s.complete(id, n),
                "dropped" => s.drop_task(id, n),
                "dropped_completely" => s.drop_completely(id, n),
                "active" => s.reactivate(id, n),
                x => return Err(format!("unknown status '{x}'")),
            }
            let d = s.derive(n);
            task_json(&s, &d, s.task(id).unwrap())
        }
        "gtd_delete_task" => {
            let id = id_arg(a)?;
            s.task(id).ok_or(format!("no task {id}"))?;
            s.delete_task(id);
            json!({"deleted": id})
        }
        "gtd_list_projects" => {
            dirty = false;
            let d = s.derive(n);
            let all = a.get("include_done").and_then(Value::as_bool).unwrap_or(false);
            json!(s.projects.iter().filter(|p| all || matches!(p.status, ProjectStatus::Active | ProjectStatus::OnHold)).map(|p| project_json(&s, &d, p)).collect::<Vec<_>>())
        }
        "gtd_add_project" => {
            let name = a.get("name").and_then(Value::as_str).filter(|x| !x.trim().is_empty()).ok_or("name is required")?;
            let kind = parse_kind(a.get("kind").and_then(Value::as_str).unwrap_or("parallel"))?;
            let id = s.add_project(name, kind);
            update_project(&mut s, id, a)?;
            let d = s.derive(n);
            project_json(&s, &d, s.project(id).unwrap())
        }
        "gtd_update_project" => {
            let id = id_arg(a)?;
            s.project(id).ok_or(format!("no project {id}"))?;
            update_project(&mut s, id, a)?;
            let d = s.derive(n);
            project_json(&s, &d, s.project(id).unwrap())
        }
        "gtd_review" => {
            if let Some(id) = a.get("mark_reviewed_id").and_then(Value::as_u64) {
                s.project(id).ok_or(format!("no project {id}"))?;
                s.mark_reviewed(id, n);
            } else {
                dirty = false;
            }
            let d = s.derive(n);
            json!(s.projects.iter().filter(|p| p.next_review <= n.date() && matches!(p.status, ProjectStatus::Active | ProjectStatus::OnHold)).map(|p| project_json(&s, &d, p)).collect::<Vec<_>>())
        }
        "gtd_list_tags" => {
            dirty = false;
            let d = s.derive(n);
            json!(s.tags.iter().map(|g| json!({"id":g.id,"name":g.name,"on_hold":g.on_hold,
                "available": s.tasks.iter().filter(|t| t.tags.contains(&g.id) && d.st(t.id).available()).count()})).collect::<Vec<_>>())
        }
        "gtd_create_perspective" => {
            let draft: crate::ai::Draft = serde_json::from_value(a.clone()).map_err(|e| format!("bad perspective: {e}"))?;
            let rule = draft.to_rule(&s);
            let next = s.settings.perspectives.iter().filter_map(|p| p.key.strip_prefix("custom-")?.parse::<u32>().ok()).max().map(|m| m + 1).unwrap_or(1);
            let key = format!("custom-{next}");
            s.settings.perspectives.push(PerspectiveCfg {
                key: key.clone(),
                name: draft.name.clone(),
                hue: draft.hue.clone(),
                filter: Some(rule.clone()),
                group: Some(draft.group.clone()),
                sort: Some(draft.sort.clone()),
                board: draft.board,
                ..PerspectiveCfg::blank()
            });
            let d = s.derive(n);
            let matches = s.tasks.iter().filter(|t| rule.matches(&s, &d, t, n)).count();
            json!({"key": key, "name": draft.name, "rules": rule, "matching_tasks": matches})
        }
        "gtd_web_activity" => {
            dirty = false;
            let days = a.get("days").and_then(Value::as_i64).unwrap_or(1).clamp(1, 90);
            let today = crate::model::now().date();
            let from = (today - chrono::Duration::days(days - 1)).and_hms_opt(0, 0, 0).unwrap();
            use chrono::TimeZone;
            let ms = |t: chrono::NaiveDateTime| chrono::Local.from_local_datetime(&t).earliest().map(|x| x.timestamp_millis()).unwrap_or(0);
            let s = crate::webact::summarize(&crate::webact::load(), ms(from), chrono::Local::now().timestamp_millis() + 1);
            json!({
                "days": days,
                "minutes": (s.seconds / 60.0).round(),
                "on_task_minutes": (s.focus / 60.0).round(),
                "distraction_minutes": (s.distraction / 60.0).round(),
                "neutral_minutes": (s.neutral / 60.0).round(),
                "top_sites": s.sites.iter().take(15).map(|(d, x)| json!({"site": d, "minutes": (x / 60.0).round()})).collect::<Vec<_>>(),
                "searches": s.searches.iter().rev().take(50).map(|(q, e)| json!({"query": q, "engine": e})).collect::<Vec<_>>(),
                "videos": s.videos.iter().map(|v| json!({"title": v.title, "minutes": (v.seconds / 60.0).round(), "url": v.url})).collect::<Vec<_>>(),
            })
        }
        "gtd_calendar_events" => {
            dirty = false;
            let from = match a.get("from").and_then(Value::as_str).filter(|x| !x.trim().is_empty()) {
                Some(x) => chrono::NaiveDate::parse_from_str(x.trim(), "%Y-%m-%d").map_err(|_| format!("'{x}' isn't YYYY-MM-DD"))?,
                None => crate::model::now().date(),
            };
            let days = a.get("days").and_then(Value::as_i64).unwrap_or(1).clamp(1, crate::calendar::DAYS_AHEAD);
            let evs = crate::calendar::events(from, days, &s.settings.hidden_calendars, s.settings.calendar_all_day)?;
            json!(evs.iter().map(|e| json!({"title": e.title, "start": e.start.format("%Y-%m-%d %H:%M").to_string(), "end": e.end.format("%Y-%m-%d %H:%M").to_string(), "all_day": e.all_day, "calendar": e.calendar, "location": e.location})).collect::<Vec<_>>())
        }
        "gtd_list_perspectives" => {
            dirty = false;
            json!(s.settings.perspectives.iter().map(|p| json!({"key": p.key, "name": p.name, "visible": p.visible, "custom": p.is_custom(), "rules": if p.is_custom() { serde_json::to_value(p.rule()).unwrap_or_default() } else { Value::Null }, "board": p.board})).collect::<Vec<_>>())
        }
        "gtd_decide" => {
            dirty = false;
            let questions = a.get("questions").filter(|q| q.as_object().is_some_and(|o| !o.is_empty())).ok_or("questions is required: {name: {type, instructions, criteria}}")?;
            let route = crate::systemone::route(&s.settings)?;
            let ask = |state: &Value| crate::systemone::decide(&route, state, questions);
            match a.get("task_ids").and_then(Value::as_array) {
                Some(ids) => {
                    let mut out = vec![];
                    for id in ids {
                        let id = id.as_u64().ok_or("task_ids are integers")?;
                        let t = s.task(id).ok_or(format!("no task {id}"))?;
                        out.push(json!({"task_id": id, "name": t.name, "answers": ask(&crate::systemone::task_state(&s, t))?}));
                    }
                    json!(out)
                }
                None => ask(a.get("state").filter(|v| !v.is_null()).ok_or("give state or task_ids")?)?,
            }
        }
        "gtd_link_linear" => {
            let raw = a.get("issue").and_then(Value::as_str).unwrap_or("");
            let link = if raw.trim().is_empty() { None } else { Some(crate::linear::identifier(raw).ok_or(format!("'{raw}' isn't a Linear issue ID or link"))?) };
            if let Some(t) = a.get("task_id").and_then(Value::as_u64) {
                let task = s.task_mut(t).ok_or(format!("no task {t}"))?;
                task.linear = link.clone();
                task.modified = n;
            } else if let Some(p) = a.get("project_id").and_then(Value::as_u64) {
                let pr = s.project_mut(p).ok_or(format!("no project {p}"))?;
                pr.linear = link.clone();
                pr.modified = n;
            } else {
                return Err("give task_id or project_id".into());
            }
            json!({"linked": link})
        }
        _ => return Err(format!("unknown tool {name}")),
    };
    if dirty {
        s.save(path).map_err(|e| format!("couldn't save: {e}"))?;
        // an agent thread's staged copy: each change is listed for the person to approve (GTDCRAFT_MCP_LOG)
        if let Ok(log) = std::env::var("GTDCRAFT_MCP_LOG") {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
                let _ = writeln!(f, "{}", json!({"tool": name, "args": a, "result": out}));
            }
        }
    }
    Ok(out)
}

fn parse_kind(k: &str) -> Result<ProjectKind, String> {
    Ok(match k {
        "parallel" => ProjectKind::Parallel,
        "sequential" => ProjectKind::Sequential,
        "single_actions" | "single" => ProjectKind::SingleActions,
        x => return Err(format!("unknown kind '{x}'")),
    })
}

fn update_project(s: &mut Store, id: Id, a: &Value) -> Result<(), String> {
    let kind = a.get("kind").and_then(Value::as_str).map(parse_kind).transpose()?;
    let status = match a.get("status").and_then(Value::as_str) {
        None => None,
        Some("active") => Some(ProjectStatus::Active),
        Some("on_hold") => Some(ProjectStatus::OnHold),
        Some("completed") => Some(ProjectStatus::Completed),
        Some("dropped") => Some(ProjectStatus::Dropped),
        Some(x) => return Err(format!("unknown status '{x}'")),
    };
    if let Some(st) = status {
        s.set_project_status(id, st, now());
    }
    let p = s.project_mut(id).unwrap();
    if let Some(k) = kind {
        p.kind = k;
    }
    if let Some(v) = a.get("name").and_then(Value::as_str) {
        p.name = v.into();
    }
    if let Some(v) = a.get("folder").and_then(Value::as_str) {
        p.folder = v.into();
    }
    if let Some(v) = a.get("note").and_then(Value::as_str) {
        p.note = v.into();
    }
    if let Some(v) = a.get("flagged").and_then(Value::as_bool) {
        p.flagged = v;
    }
    if let Some(v) = a.get("review_every_days").and_then(Value::as_u64) {
        p.review_every_days = v.max(1) as u32;
    }
    p.modified = now();
    Ok(())
}

fn project_json(s: &Store, d: &Derived, p: &Project) -> Value {
    let tasks = s.tasks.iter().filter(|t| t.project == Some(p.id));
    let (mut rem, mut avail) = (0, 0);
    for t in tasks {
        let st = d.st(t.id);
        rem += st.remaining() as usize;
        avail += st.available() as usize;
    }
    json!({"id":p.id,"name":p.name,"kind":p.kind,"status":p.status,"folder":p.folder,"flagged":p.flagged,
        "remaining":rem,"available":avail,"stalled": p.status == ProjectStatus::Active && avail == 0,
        "review_every_days":p.review_every_days,"next_review":p.next_review.to_string(),
        "last_review":fmt(&p.last_review),"note":p.note})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_roundtrip() {
        let dir = std::env::temp_dir().join(format!("gtdcraft-mcp-{}", std::process::id()));
        let path = dir.join("db.json");
        // new databases start empty: this test works on the sample one (its Errands project, Home tag…)
        std::fs::create_dir_all(&dir).unwrap();
        crate::model::Store::sample(crate::model::now()).save(&path).unwrap();
        let r = handle(&path, &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}})).unwrap();
        assert_eq!(r["result"]["serverInfo"]["name"], "gtdcraft");
        assert!(handle(&path, &json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
        let r = handle(&path, &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
        assert!(r["result"]["tools"].as_array().unwrap().len() >= 10);

        let added = call(&path, "gtd_add_task", &json!({"name":"Agent task","project":"Errands","tags":["Agent"],"due":"2020-01-01","flagged":true})).unwrap();
        assert_eq!(added["status"], "overdue");
        assert_eq!(added["due"], "2020-01-01 17:00"); // DefaultDueTime 17:00
        let id = added["id"].as_u64().unwrap();
        let done = call(&path, "gtd_set_task_status", &json!({"id":id,"status":"completed"})).unwrap();
        assert_eq!(done["status"], "completed");
        assert!(call(&path, "gtd_add_task", &json!({"name":"x","project":"Nope"})).is_err());
        let before = Store::load(&path).unwrap().tasks.len();
        assert!(call(&path, "gtd_add_task", &json!({"name":"x","due":"garbage"})).is_err());
        assert_eq!(Store::load(&path).unwrap().tasks.len(), before, "failed add must not save");
        let rev = call(&path, "gtd_review", &json!({})).unwrap();
        let pid = rev[0]["id"].as_u64().unwrap();
        call(&path, "gtd_review", &json!({"mark_reviewed_id":pid})).unwrap();
        assert_eq!(parse_repeat("2 weeks from_completion catch_up end_after:3").unwrap().unwrap().end_after, Some(3));
        assert!(parse_repeat("2 fortnights").is_err());
        let made = call(&path, "gtd_create_perspective", &json!({"name":"Quick wins","hue":"green","match":"all","group":"none","sort":"due","board":true,
            "rules":[{"kind":"availability","value":"available","number":0,"negate":false},{"kind":"duration_at_most","value":"","number":15,"negate":false}]})).unwrap();
        assert!(made["key"].as_str().unwrap().starts_with("custom-"));
        let list = call(&path, "gtd_list_perspectives", &json!({})).unwrap();
        assert!(list.as_array().unwrap().iter().any(|p| p["name"] == "Quick wins" && p["board"] == true));
        let linked = call(&path, "gtd_link_linear", &json!({"task_id": id, "issue": "https://linear.app/x/issue/ENG-9/thing"})).unwrap();
        assert_eq!(linked["linked"], "ENG-9");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn decide_asks_clef_about_tasks_and_states() {
        let (url, seen) = crate::systemone::tests::fake_clef();
        let dir = std::env::temp_dir().join(format!("gtdcraft-mcp-decide-{}", std::process::id()));
        let path = dir.join("db.json");
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = crate::model::Store::sample(crate::model::now());
        s.settings = crate::systemone::tests::on(&url);
        s.save(&path).unwrap();
        let id = s.tasks[0].id;
        let q = json!({"today": {"type": "noul", "instructions": "Should this be done today?"}});
        let r = call(&path, "gtd_decide", &json!({"questions": q, "task_ids": [id]})).unwrap();
        assert_eq!((r[0]["task_id"].as_u64(), r[0]["answers"]["today"]["noul"].as_f64()), (Some(id), Some(0.75)));
        assert_eq!(seen.lock().unwrap()[0]["state"]["task"], s.tasks[0].name);
        let r = call(&path, "gtd_decide", &json!({"questions": q, "state": "Can you call me back?"})).unwrap();
        assert_eq!(r["today"]["noul"], 0.75);
        assert!(call(&path, "gtd_decide", &json!({"questions": q})).unwrap_err().contains("state"));
        assert!(call(&path, "gtd_decide", &json!({"questions": {}, "state": "x"})).unwrap_err().contains("questions"));
        // off until turned on
        s.settings.decisions_on = false;
        s.save(&path).unwrap();
        assert!(call(&path, "gtd_decide", &json!({"questions": q, "state": "x"})).unwrap_err().contains("off"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
