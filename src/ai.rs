//! "Describe a perspective": an AI turns a plain-language description into perspective rules.
//! Claude (Messages API, raw HTTP: there is no official Rust SDK), any OpenAI-compatible server
//! (OpenAI, Hermes on Ollama / LM Studio / hosted), or any Agent Client Protocol agent over stdio.
use crate::model::*;
use crate::rules::Rule;
use serde::Deserialize;
use serde_json::{json, Value};

pub const MODEL: &str = "claude-opus-5-5";

/// What Claude returns (a flat list keeps the schema simple; `negate` wraps a rule in "None of").
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Draft {
    pub name: String,
    pub hue: String,
    #[serde(rename = "match")]
    pub match_mode: String,
    pub rules: Vec<DraftRule>,
    pub group: String,
    pub sort: String,
    pub board: bool,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct DraftRule {
    pub kind: String,
    pub value: String,
    pub number: i64,
    pub negate: bool,
}

pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["name", "hue", "match", "rules", "group", "sort", "board"],
        "properties": {
            "name": {"type": "string"},
            "hue": {"type": "string", "enum": crate::theme::HUES},
            "match": {"type": "string", "enum": ["all", "any"]},
            "group": {"type": "string", "enum": ["none", "project", "tag", "folder", "due", "flagged"]},
            "sort": {"type": "string", "enum": ["none", "due", "defer", "planned", "added", "modified", "name", "flagged"]},
            "board": {"type": "boolean"},
            "rules": {"type": "array", "items": {
                "type": "object",
                "additionalProperties": false,
                "required": ["kind", "value", "number", "negate"],
                "properties": {
                    "kind": {"type": "string", "enum": ["availability", "status", "tag", "untagged", "project", "inbox", "folder", "due_within", "defer_within", "planned_within", "added_within", "modified_within", "completed_within", "has_due", "has_defer", "duration_at_most", "unestimated", "text", "repeats", "has_note"]},
                    "value": {"type": "string"},
                    "number": {"type": "integer"},
                    "negate": {"type": "boolean"}
                }
            }}
        }
    })
}

fn system_prompt(store: &Store) -> String {
    let tags: Vec<&str> = store.tags.iter().map(|t| t.name.as_str()).collect();
    let projects: Vec<&str> = store.projects.iter().map(|p| p.name.as_str()).collect();
    let mut folders: Vec<&str> = store.projects.iter().map(|p| p.folder.as_str()).filter(|f| !f.is_empty()).collect();
    folders.sort();
    folders.dedup();
    format!(
        "You design saved filters (\"perspectives\") for a GTD task manager. Turn the user's description into rules.\n\
Rule kinds and how to fill value/number:\n\
- availability: value one of first, available, remaining, completed, dropped. Add one availability rule unless the user wants everything.\n\
- status: value one of flagged, due_soon, overdue, due_soon_or_flagged.\n\
- tag / project / folder: value is an exact name from the lists below. untagged / inbox: no value.\n\
- due_within, defer_within, planned_within: number = days ahead (0 = today). added_within, modified_within, completed_within: number = days back.\n\
- has_due, has_defer: value \"yes\" or \"no\". duration_at_most: number = minutes. unestimated, repeats, has_note: value \"yes\" or \"no\".\n\
- text: value = words to find in the name or note.\n\
Set negate=true to exclude what a rule matches. Use number 0 and value \"\" where a field doesn't apply.\n\
match: all (every rule) or any (at least one). group: how to section the list. sort: initial order. board: true only if the user asks for a board or kanban.\n\
Pick a short name and a hue that fits. Only use names that exist below; if the user names one that doesn't, use text matching instead.\n\
Tags: {}\nProjects: {}\nFolders: {}",
        tags.join(", "),
        projects.join(", "),
        folders.join(", ")
    )
}

/// Blocking call; run it on a thread. Dispatches on Settings › Agent › AI helper.
pub fn draft(store: &Store, description: &str) -> Result<Draft, String> {
    let st = &store.settings;
    match st.ai_provider.as_str() {
        "openai" => draft_openai(store, description),
        "acp" => draft_acp(store, description),
        _ => draft_claude(store, description),
    }
}

fn env_or(key: &str, var: &str) -> Option<String> {
    Some(key.trim().to_string()).filter(|k| !k.is_empty()).or_else(|| std::env::var(var).ok().filter(|k| !k.is_empty()))
}

fn http_error(e: ureq::Error, who: &str) -> String {
    match e {
        ureq::Error::Status(code, r) => {
            let msg = r.into_string().unwrap_or_default();
            let detail = serde_json::from_str::<Value>(&msg).ok().and_then(|v| v["error"]["message"].as_str().map(String::from)).unwrap_or(msg);
            format!("{who} error {code}: {detail}")
        }
        e => format!("Couldn't reach {who}: {e}"),
    }
}

fn draft_claude(store: &Store, description: &str) -> Result<Draft, String> {
    let st = &store.settings;
    let key = env_or(&st.ai_key, "ANTHROPIC_API_KEY").ok_or("Add an Anthropic API key in Settings › Agent, or set ANTHROPIC_API_KEY.")?;
    let model = Some(st.ai_model.trim()).filter(|m| !m.is_empty() && m.starts_with("claude")).unwrap_or(MODEL);
    let body = json!({
        "model": model,
        "max_tokens": 16000,
        "fallbacks": "default",
        "output_config": {"effort": "low", "format": {"type": "json_schema", "schema": schema()}},
        "system": system_prompt(store),
        "messages": [{"role": "user", "content": description}]
    });
    let text = ureq::post("https://api.anthropic.com/v1/messages")
        .set("x-api-key", &key)
        .set("anthropic-version", "2026-10-21")
        .set("anthropic-beta", "server-side-fallback-2026-07-01")
        .set("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(120))
        .send_string(&body.to_string())
        .map_err(|e| http_error(e, "the Claude API"))?
        .into_string()
        .map_err(|e| e.to_string())?;
    parse_response(&text)
}

/// Any OpenAI-compatible Chat Completions server (OpenAI, or Hermes models on Ollama / LM Studio / a hosted endpoint).
fn draft_openai(store: &Store, description: &str) -> Result<Draft, String> {
    let st = &store.settings;
    let base = Some(st.ai_base_url.trim().trim_end_matches('/')).filter(|b| !b.is_empty()).unwrap_or("https://api.openai.com/v1").to_string();
    let model = Some(st.ai_model.trim()).filter(|m| !m.is_empty()).ok_or("Enter a model name in Settings › Agent.")?;
    let key = env_or(&st.ai_key, "OPENAI_API_KEY");
    let instructions = format!("{}\n\nReply with only a JSON object matching this JSON Schema:\n{}", system_prompt(store), schema());
    let mut body = json!({
        "model": model,
        "messages": [{"role": "system", "content": instructions}, {"role": "user", "content": description}],
        "response_format": {"type": "json_schema", "json_schema": {"name": "perspective", "strict": true, "schema": schema()}}
    });
    let send = |body: &Value| {
        let mut req = ureq::post(&format!("{base}/chat/completions")).set("content-type", "application/json").timeout(std::time::Duration::from_secs(180));
        if let Some(k) = &key {
            req = req.set("authorization", &format!("Bearer {k}"));
        }
        req.send_string(&body.to_string())
    };
    let resp = match send(&body) {
        // servers without structured outputs (common for local Hermes models): retry with the schema in the prompt only
        Err(ureq::Error::Status(400, _)) => {
            body.as_object_mut().unwrap().remove("response_format");
            send(&body)
        }
        r => r,
    };
    let text = resp.map_err(|e| http_error(e, "the model server"))?.into_string().map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if v["choices"][0]["finish_reason"] == "length" {
        return Err("The answer was cut off; try a shorter description.".into());
    }
    let content = v["choices"][0]["message"]["content"].as_str().ok_or("The model server sent no text.")?;
    parse_json_text(content)
}

/// Any Agent Client Protocol agent: JSON-RPC 2.0 over the agent's stdin/stdout
/// (initialize → session/new → session/prompt; text arrives as session/update agent_message_chunk).
fn draft_acp(store: &Store, description: &str) -> Result<Draft, String> {
    use std::io::{BufRead, Write};
    let cmd = store.settings.acp_command.trim();
    if cmd.is_empty() {
        return Err("Enter the ACP agent's command in Settings › Agent.".into());
    }
    let mut child = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(cmd)
        .env("PATH", crate::acpfind::login_path()) // so `npx`/`node` resolve when the app was opened from Finder
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Couldn't start the agent: {e}"))?;
    let mut stdin = child.stdin.take().unwrap();
    let stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in stdout.lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut send = |v: Value| writeln!(stdin, "{v}").and_then(|_| stdin.flush()).map_err(|e| format!("Agent closed: {e}"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    let mut text = String::new();
    // waits for the response to `id`, answering agent requests and collecting message chunks meanwhile
    let wait = |id: i64, send: &mut dyn FnMut(Value) -> Result<(), String>, text: &mut String| -> Result<Value, String> {
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let line = rx.recv_timeout(left).map_err(|_| "The agent didn't answer in time.".to_string())?;
            let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
            if let Some(m) = msg["method"].as_str() {
                if m == "session/update" && msg["params"]["update"]["sessionUpdate"] == "agent_message_chunk" {
                    if let Some(t) = msg["params"]["update"]["content"]["text"].as_str() {
                        text.push_str(t);
                    }
                } else if msg.get("id").is_some() {
                    // no file or terminal access is offered; permission requests are declined
                    let reply = if m == "session/request_permission" {
                        let reject = msg["params"]["options"].as_array().into_iter().flatten().find(|o| o["kind"].as_str().is_some_and(|k| k.starts_with("reject")));
                        match reject {
                            Some(o) => json!({"jsonrpc": "2.0", "id": msg["id"], "result": {"outcome": {"outcome": "selected", "optionId": o["optionId"]}}}),
                            None => json!({"jsonrpc": "2.0", "id": msg["id"], "result": {"outcome": {"outcome": "cancelled"}}}),
                        }
                    } else {
                        json!({"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32601, "message": "not supported by GTDCraft"}})
                    };
                    send(reply)?;
                }
                continue;
            }
            if msg["id"].as_i64() == Some(id) {
                if let Some(e) = msg.get("error") {
                    return Err(format!("Agent error: {}", e["message"].as_str().unwrap_or("unknown")));
                }
                return Ok(msg["result"].clone());
            }
        }
    };
    let result = (|| {
        send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": 1, "clientCapabilities": {}, "clientInfo": {"name": "gtdcraft", "title": "GTDCraft", "version": env!("CARGO_PKG_VERSION")}}}))?;
        wait(1, &mut send, &mut text)?;
        let cwd = std::env::temp_dir().canonicalize().unwrap_or_else(|_| std::env::temp_dir()).display().to_string();
        send(json!({"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {"cwd": cwd, "mcpServers": []}}))?;
        let session = wait(2, &mut send, &mut text)?["sessionId"].as_str().map(String::from).ok_or("The agent didn't open a session.")?;
        let prompt = format!(
            "{}\n\nReply with only a JSON object matching this JSON Schema, no other text and no tool use:\n{}\n\nThe user's description: {}",
            system_prompt(store),
            schema(),
            description
        );
        send(json!({"jsonrpc": "2.0", "id": 3, "method": "session/prompt", "params": {"sessionId": session, "prompt": [{"type": "text", "text": prompt}]}}))?;
        let r = wait(3, &mut send, &mut text)?;
        if r["stopReason"] == "refusal" {
            return Err("The agent declined this request.".to_string());
        }
        parse_json_text(&text)
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}

/// Reads the first {...} object out of free text (agents and local models may wrap JSON in prose or fences).
pub fn parse_json_text(text: &str) -> Result<Draft, String> {
    let start = text.find('{').ok_or("The answer had no JSON in it.")?;
    let end = text.rfind('}').ok_or("The answer had no JSON in it.")?;
    serde_json::from_str(&text[start..=end]).map_err(|e| format!("Unexpected answer: {e}"))
}

pub fn parse_response(text: &str) -> Result<Draft, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    match v["stop_reason"].as_str() {
        Some("refusal") => return Err("Claude declined this request.".into()),
        Some("max_tokens") => return Err("The answer was cut off; try a shorter description.".into()),
        _ => {}
    }
    let out = v["content"].as_array().into_iter().flatten().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<String>();
    serde_json::from_str(&out).map_err(|e| format!("Unexpected answer: {e}"))
}

impl Draft {
    /// Builds the rule tree, resolving names to ids (unknown names fall back to text matching).
    pub fn to_rule(&self, store: &Store) -> Rule {
        let mut rules = vec![];
        for r in &self.rules {
            let yes = !matches!(r.value.to_lowercase().as_str(), "no" | "false");
            let days = r.number.clamp(-3650, 3650) as i32;
            let leaf = match r.kind.as_str() {
                "availability" => Rule::Availability { value: r.value.to_lowercase() },
                "status" => Rule::Status { value: r.value.to_lowercase() },
                "tag" => match store.tag_by_name(&r.value) {
                    Some(id) => Rule::Tag { id: Some(id) },
                    None => Rule::Text { contains: r.value.clone() },
                },
                "untagged" => Rule::Tag { id: None },
                "project" => match store.project_by_name(&r.value) {
                    Some(id) => Rule::Project { id: Some(id) },
                    None => Rule::Text { contains: r.value.clone() },
                },
                "inbox" => Rule::Project { id: None },
                "folder" => Rule::Folder { name: r.value.clone() },
                "due_within" => Rule::DateWithin { field: "due".into(), days: days.max(0) },
                "defer_within" => Rule::DateWithin { field: "defer".into(), days: days.max(0) },
                "planned_within" => Rule::DateWithin { field: "planned".into(), days: days.max(0) },
                "added_within" => Rule::DateWithin { field: "added".into(), days: -days.abs() },
                "modified_within" => Rule::DateWithin { field: "modified".into(), days: -days.abs() },
                "completed_within" => Rule::DateWithin { field: "completed".into(), days: -days.abs() },
                "has_due" => Rule::HasDate { field: "due".into(), has: yes },
                "has_defer" => Rule::HasDate { field: "defer".into(), has: yes },
                "duration_at_most" => Rule::Duration { max_min: Some(r.number.clamp(0, 100_000) as u32) },
                "unestimated" => Rule::Duration { max_min: None },
                "repeats" => Rule::Repeats { value: yes },
                "has_note" => Rule::HasNote { value: yes },
                _ => Rule::Text { contains: r.value.clone() },
            };
            rules.push(if r.negate { Rule::None { rules: vec![leaf] } } else { leaf });
        }
        if self.match_mode == "any" {
            // keep availability outside the "any" group so "flagged or due" still hides completed items
            let (avail, rest): (Vec<Rule>, Vec<Rule>) = rules.into_iter().partition(|r| matches!(r, Rule::Availability { .. }));
            let mut all = avail;
            all.push(Rule::Any { rules: rest });
            Rule::All { rules: all }
        } else {
            Rule::All { rules }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_to_rules() {
        let mut s = Store::default();
        let id = s.new_id();
        s.tags.push(Tag { id, name: "Errands".into(), on_hold: false, place: None, parent: None, modified: None });
        let draft = r#"{"name":"Quick errands","hue":"green","match":"all","group":"none","sort":"due","board":false,
            "rules":[{"kind":"availability","value":"available","number":0,"negate":false},
                     {"kind":"tag","value":"errands","number":0,"negate":false},
                     {"kind":"duration_at_most","value":"","number":15,"negate":false},
                     {"kind":"has_note","value":"yes","number":0,"negate":true}]}"#;
        let resp = json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": draft}]}).to_string();
        let d = parse_response(&resp).unwrap();
        assert_eq!(d.name, "Quick errands");
        let r = d.to_rule(&s);
        let Rule::All { rules } = r else { panic!() };
        assert_eq!(rules[1], Rule::Tag { id: Some(id) });
        assert_eq!(rules[2], Rule::Duration { max_min: Some(15) });
        assert!(matches!(rules[3], Rule::None { .. }));
        let refused = json!({"stop_reason": "refusal", "content": []}).to_string();
        assert!(parse_response(&refused).is_err());
        assert!(schema()["properties"]["rules"]["items"]["required"].as_array().unwrap().len() == 4);
        let fenced = format!("Here you go:\n```json\n{draft}\n```");
        assert_eq!(parse_json_text(&fenced).unwrap(), d);
    }
}
