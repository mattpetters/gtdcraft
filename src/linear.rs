//! Linear issues linked to tasks and projects: fetch at-a-glance details, open in Linear, change status.
//! GraphQL at https://api.linear.app/graphql; a personal API key goes in `Authorization` as-is (no "Bearer").
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct State {
    pub id: String,
    pub name: String,
    pub color: String,
    /// triage | backlog | unstarted | started | completed | canceled
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub position: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Issue {
    pub identifier: String,
    pub title: String,
    pub url: String,
    pub state: State,
    #[serde(default)]
    pub priority_label: String,
    #[serde(default)]
    pub assignee: String,
    #[serde(default)]
    pub due: String,
    #[serde(default)]
    pub estimate: Option<f64>,
    #[serde(default)]
    pub labels: Vec<(String, String)>,
    #[serde(default)]
    pub team: String,
    /// The team's workflow states, for the status picker.
    #[serde(default)]
    pub states: Vec<State>,
    #[serde(default)]
    pub updated: String,
}

/// "ENG-123" from an identifier or a linear.app issue URL.
pub fn identifier(input: &str) -> Option<String> {
    let s = input.trim();
    let cand = if s.contains("linear.app/") { s.split("/issue/").nth(1)?.split(['/', '?', '#']).next()? } else { s };
    let (team, num) = cand.split_once('-')?;
    let ok = !team.is_empty() && team.chars().all(|c| c.is_ascii_alphanumeric()) && !num.is_empty() && num.chars().all(|c| c.is_ascii_digit());
    ok.then(|| format!("{}-{}", team.to_uppercase(), num))
}

pub fn api_key(stored: &str) -> Option<String> {
    Some(stored.trim().to_string()).filter(|k| !k.is_empty()).or_else(|| std::env::var("LINEAR_API_KEY").ok().filter(|k| !k.is_empty()))
}

fn gql(key: &str, query: &str, vars: Value) -> Result<Value, String> {
    let resp = ureq::post("https://api.linear.app/graphql")
        .set("Authorization", key)
        .set("Content-Type", "application/json")
        .set("User-Agent", crate::notes::UA)
        .timeout(std::time::Duration::from_secs(20))
        .send_string(&json!({"query": query, "variables": vars}).to_string());
    let text = match resp {
        Ok(r) => r.into_string().map_err(|e| e.to_string())?,
        Err(ureq::Error::Status(code, r)) => {
            let body = r.into_string().unwrap_or_default();
            return Err(match code {
                401 | 403 => "Linear rejected the API key.".into(),
                _ => format!("Linear error {code}: {}", first_error(&body).unwrap_or(body)),
            });
        }
        Err(e) => return Err(format!("Couldn't reach Linear: {e}")),
    };
    if let Some(e) = first_error(&text) {
        return Err(format!("Linear: {e}"));
    }
    serde_json::from_str::<Value>(&text).map(|v| v["data"].clone()).map_err(|e| e.to_string())
}

fn first_error(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body).ok()?["errors"][0]["message"].as_str().map(String::from)
}

const ISSUE_FIELDS: &str = "identifier title url priorityLabel dueDate estimate updatedAt
  state { id name color type position }
  assignee { name }
  labels { nodes { name color } }
  team { key states { nodes { id name color type position } } }";

pub fn parse_issue(v: &Value) -> Option<Issue> {
    let state = |s: &Value| State {
        id: s["id"].as_str().unwrap_or("").into(),
        name: s["name"].as_str().unwrap_or("").into(),
        color: s["color"].as_str().unwrap_or("#888888").into(),
        kind: s["type"].as_str().unwrap_or("").into(),
        position: s["position"].as_f64().unwrap_or(0.0),
    };
    let mut states: Vec<State> = v["team"]["states"]["nodes"].as_array().into_iter().flatten().map(state).collect();
    states.sort_by(|a, b| order(&a.kind).cmp(&order(&b.kind)).then(a.position.total_cmp(&b.position)));
    Some(Issue {
        identifier: v["identifier"].as_str()?.into(),
        title: v["title"].as_str().unwrap_or("").into(),
        url: v["url"].as_str().unwrap_or("").into(),
        state: state(&v["state"]),
        priority_label: v["priorityLabel"].as_str().unwrap_or("").into(),
        assignee: v["assignee"]["name"].as_str().unwrap_or("").into(),
        due: v["dueDate"].as_str().unwrap_or("").into(),
        estimate: v["estimate"].as_f64(),
        labels: v["labels"]["nodes"].as_array().into_iter().flatten().map(|l| (l["name"].as_str().unwrap_or("").into(), l["color"].as_str().unwrap_or("#888888").into())).collect(),
        team: v["team"]["key"].as_str().unwrap_or("").into(),
        states,
        updated: v["updatedAt"].as_str().unwrap_or("").into(),
    })
}

fn order(kind: &str) -> u8 {
    ["triage", "backlog", "unstarted", "started", "completed", "canceled"].iter().position(|k| *k == kind).unwrap_or(9) as u8
}

pub fn fetch(key: &str, id: &str) -> Result<Issue, String> {
    let q = format!("query($id: String!) {{ issue(id: $id) {{ {ISSUE_FIELDS} }} }}");
    let d = gql(key, &q, json!({"id": id}))?;
    parse_issue(&d["issue"]).ok_or_else(|| format!("No Linear issue {id}"))
}

pub fn set_state(key: &str, id: &str, state_id: &str) -> Result<Issue, String> {
    let q = format!("mutation($id: String!, $state: String!) {{ issueUpdate(id: $id, input: {{ stateId: $state }}) {{ success issue {{ {ISSUE_FIELDS} }} }} }}");
    let d = gql(key, &q, json!({"id": id, "state": state_id}))?;
    if d["issueUpdate"]["success"] != true {
        return Err("Linear didn't accept the change.".into());
    }
    parse_issue(&d["issueUpdate"]["issue"]).ok_or_else(|| "Linear sent no issue back.".into())
}

/// "#RRGGBB" from Linear to a colour.
pub fn color(hex: &str) -> eframe::egui::Color32 {
    crate::theme::hex(hex).unwrap_or(eframe::egui::Color32::GRAY)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifiers_and_parsing() {
        assert_eq!(identifier("eng-42").as_deref(), Some("ENG-42"));
        assert_eq!(identifier("https://linear.app/acme/issue/ENG-123/fix-the-thing?x=1").as_deref(), Some("ENG-123"));
        assert_eq!(identifier("hello world"), None);
        let v = json!({"identifier": "ENG-1", "title": "Fix", "url": "https://linear.app/a/issue/ENG-1",
            "state": {"id": "s2", "name": "In Progress", "color": "#f2c94c", "type": "started", "position": 1},
            "assignee": {"name": "Sam"}, "priorityLabel": "High", "labels": {"nodes": [{"name": "bug", "color": "#eb5757"}]},
            "team": {"key": "ENG", "states": {"nodes": [
                {"id": "s3", "name": "Done", "color": "#5e6ad2", "type": "completed", "position": 0},
                {"id": "s1", "name": "Todo", "color": "#e2e2e2", "type": "unstarted", "position": 0},
                {"id": "s2", "name": "In Progress", "color": "#f2c94c", "type": "started", "position": 1}]}}});
        let i = parse_issue(&v).unwrap();
        assert_eq!(i.state.name, "In Progress");
        assert_eq!(i.states.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Todo", "In Progress", "Done"]);
        assert_eq!(i.labels[0].0, "bug");
    }
}

/// Linear's mark for the inspector section header: an indigo disc with diagonal cuts across its lower-left half.
pub fn logo(p: &eframe::egui::Painter, c: eframe::egui::Pos2, r: f32) {
    use eframe::egui::{vec2, Color32, Stroke};
    p.circle_filled(c, r, Color32::from_rgb(0x5E, 0x6A, 0xD2));
    let along = vec2(1.0, 1.0) / 2f32.sqrt(); // the cuts run like "\"
    let toward = vec2(-1.0, 1.0) / 2f32.sqrt(); // stacked toward the lower left
    let cut = Stroke::new(r * 0.16, crate::th().inspector);
    for f in [0.2, 0.5, 0.8] {
        let d = r * f;
        let half = (r * r - d * d).max(0.0).sqrt();
        let m = c + toward * d;
        p.line_segment([m - along * half, m + along * half], cut);
    }
}
