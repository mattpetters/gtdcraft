//! System One decisions: a state plus typed questions in, a probability for every allowed answer out, in one forward
//! pass. Question types: `noul` (yes/no, answered with the chance of yes), `choice` (one of `criteria`'s keys) and
//! `score` (ordered `criteria` levels).
//!
//! Off until the person turns it on (Settings › Agent › Decisions) and picks a provider: Clef on this Mac
//! (`clef_mlx.py serve`), Cloudflare's Clef, TypeSafe's Jev, Fireworks' Rune, or any other `/v1/systemone` server.
//! The settings sync, so iPhone and iPad call the same cloud provider; for the local model they go through the sync
//! server, which forwards to the sidecar on the Mac it runs on (`--clef-url`).
use crate::model::{Settings, Store, Task};
use serde_json::{json, Value};

/// The `clef_mlx.py serve --port 8001` sidecar (8000 is usually taken by oMLX).
pub const LOCAL: &str = "http://127.0.0.1:8001";
const TIMEOUT_SECS: u64 = 120;

pub struct Provider {
    pub key: &'static str,
    pub name: &'static str,
    /// The decision endpoint; `{model}` and `{account}` are filled in.
    pub url: &'static str,
    pub model: &'static str,
    /// Where the API key comes from when Settings has none ("" = no key needed).
    pub key_env: &'static str,
    pub about: &'static str,
}

pub const PROVIDERS: [Provider; 5] = [
    Provider { key: "local", name: "Clef on this Mac", url: "http://127.0.0.1:8001/v1/systemone", model: "clef-flash", key_env: "",
        about: "Clef Flash (MLX) on this Mac: run `python clef_mlx.py serve --port 8001` from the model's folder. Free and private. iPhone and iPad reach it through your sync server." },
    Provider { key: "cloudflare", name: "Cloudflare Clef", url: "https://api.cloudflare.com/client/v4/accounts/{account}/ai/run/@cf/cloudflare/{model}", model: "clef-flash", key_env: "CLOUDFLARE_API_TOKEN",
        about: "Clef (27B) or Clef Flash (9B) on Workers AI, billed per input token. Model: clef or clef-flash. Needs your account ID and a Workers AI token." },
    Provider { key: "jev", name: "TypeSafe Jev", url: "https://api.typesafe.ai/v1/systemone", model: "jev-latest", key_env: "TYPESAFE_API_KEY",
        about: "TypeSafe's native System One model, billed per input token. Images and box questions are preview features GTDCraft doesn't use." },
    Provider { key: "rune", name: "Fireworks Rune", url: "https://api.fireworks.ai/inference/v1/systemone", model: "accounts/fireworks/models/rune", key_env: "FIREWORKS_API_KEY",
        about: "Fireworks' System One model: text only, up to 256 questions per request, billed per input token." },
    Provider { key: "custom", name: "Other server", url: "", model: "", key_env: "",
        about: "Any server that answers POST /v1/systemone, e.g. Clef on another Mac." },
];

pub fn provider(key: &str) -> &'static Provider {
    PROVIDERS.iter().find(|p| p.key == key).unwrap_or(&PROVIDERS[0])
}

/// Where one device sends its decisions.
#[derive(Debug, PartialEq)]
pub struct Route {
    pub url: String,
    pub model: String,
    pub key: Option<String>,
}

/// A base URL gets `/v1/systemone`; a full decision endpoint (System One or Cloudflare's) is kept.
pub fn endpoint_url(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    if u.ends_with("/systemone") || u.contains("/ai/run/") { u.to_string() } else { format!("{u}/v1/systemone") }
}

fn nonempty(s: &str) -> Option<String> {
    Some(s.trim().to_string()).filter(|s| !s.is_empty())
}

/// This device's route, or why there is none (off, or a missing URL, model, account or key).
pub fn route(st: &Settings) -> Result<Route, String> {
    if !st.decisions_on {
        return Err("Decisions are off. Turn them on in Settings › Agent › Decisions.".into());
    }
    let p = provider(&st.decisions_provider);
    let model = nonempty(&st.decisions_model).or_else(|| nonempty(p.model)).ok_or("Enter the decision model's name in Settings › Agent › Decisions.")?;
    // the local model lives on the Mac: a phone or tablet reaches it through the sync server
    if p.key == "local" && cfg!(target_os = "ios") {
        let sync = nonempty(&st.sync_url).ok_or("Decisions use Clef on your Mac, which this device reaches through the sync server: set up Sync first.")?;
        return Ok(Route { url: endpoint_url(&sync), model, key: nonempty(&st.sync_token) });
    }
    let raw = nonempty(&st.decisions_url).or_else(|| nonempty(p.url)).ok_or("Enter the decision server's URL in Settings › Agent › Decisions.")?;
    let mut url = endpoint_url(&raw).replace("{model}", &model);
    if url.contains("{account}") {
        let account = nonempty(&st.decisions_account).or_else(|| std::env::var("CLOUDFLARE_ACCOUNT_ID").ok().and_then(|a| nonempty(&a))).ok_or("Enter your Cloudflare account ID in Settings › Agent › Decisions.")?;
        url = url.replace("{account}", &account);
    }
    let key = nonempty(&st.decisions_key).or_else(|| (!p.key_env.is_empty()).then(|| std::env::var(p.key_env).ok()).flatten().and_then(|k| nonempty(&k)));
    if key.is_none() && !p.key_env.is_empty() {
        return Err(format!("Add your {} API key in Settings › Agent › Decisions, or set {}.", p.name, p.key_env));
    }
    Ok(Route { url, model, key })
}

/// One decision; returns the response's `answers` (name → answer). Blocking: run it on a thread from the UI.
pub fn decide(r: &Route, state: &Value, questions: &Value) -> Result<Value, String> {
    let body = json!({"model": r.model, "state": state, "questions": questions});
    let mut req = ureq::post(&r.url).set("content-type", "application/json").timeout(std::time::Duration::from_secs(TIMEOUT_SECS));
    if let Some(k) = &r.key {
        req = req.set("authorization", &format!("Bearer {k}"));
    }
    let v: Value = match req.send_string(&body.to_string()) {
        Ok(resp) => serde_json::from_reader(resp.into_reader()).map_err(|e| format!("Decision reply: {e}"))?,
        Err(ureq::Error::Status(code, resp)) => return Err(format!("Decision server error {code}: {}", resp.into_string().unwrap_or_default())),
        Err(e) => return Err(format!("Couldn't reach the decision server at {}: {e}", r.url)),
    };
    // Cloudflare wraps the System One reply: {"result": {...}, "success": true, "errors": [...]}
    if v["success"] == false {
        return Err(format!("Decision server error: {}", v["errors"]));
    }
    let reply = if v["result"].is_object() { &v["result"] } else { &v };
    reply.get("answers").cloned().ok_or_else(|| format!("The decision server sent no answers: {v}"))
}

/// The sync server's `/v1/systemone`: the caller's body to `upstream` as it is, its reply (or error status) back.
/// An upstream that can't be reached or doesn't answer within `secs` is a 502 (`subsystems.rs` trips on it).
pub fn forward(upstream: &str, body: &str, secs: u64) -> Result<String, (u16, String)> {
    let r = ureq::post(&endpoint_url(upstream)).set("content-type", "application/json").timeout(std::time::Duration::from_secs(secs)).send_string(body);
    match r {
        Ok(r) => r.into_string().map_err(|e| (502, e.to_string())),
        Err(ureq::Error::Status(code, r)) => Err((code, r.into_string().unwrap_or_default())),
        Err(e) => Err((502, format!("Couldn't reach Clef at {upstream}: {e}"))),
    }
}

/// The Test button's question: one yes/no about a sentence that is clearly a commitment.
pub fn test(r: &Route) -> Result<String, String> {
    let q = json!({"promise": {"type": "noul", "instructions": "Did the second person commit to doing something?"}});
    let started = std::time::Instant::now();
    let a = decide(r, &json!("Sam: can you send me the slides by Friday? Me: sure, I'll send them Thursday night."), &q)?;
    let p = a["promise"]["noul"].as_f64().ok_or_else(|| format!("Unexpected answer: {a}"))?;
    Ok(format!("Works: \"I'll send them Thursday night\" is a commitment with probability {p:.2} ({} ms).", started.elapsed().as_millis()))
}

/// A task as a decision's state: what a person would read to judge it, with names instead of IDs.
pub fn task_state(s: &Store, t: &Task) -> Value {
    let when = |d: &Option<crate::model::Dt>| d.map(|d| d.format("%Y-%m-%d %H:%M").to_string());
    json!({
        "task": t.name,
        "note": t.note,
        "project": t.project.and_then(|p| s.project(p)).map(|p| p.name.clone()),
        "tags": t.tags.iter().filter_map(|g| s.tag(*g)).map(|g| g.name.clone()).collect::<Vec<_>>(),
        "flagged": t.flagged,
        "defer": when(&t.defer),
        "planned": when(&t.planned),
        "due": when(&t.due),
        "estimate_minutes": t.duration_min,
        "references": t.refs.iter().map(|r| if r.title.is_empty() { r.url.clone() } else { r.title.clone() }).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A stand-in decision server: answers every question `noul` 0.75 and records the bodies (and auth) it was sent.
    /// With `envelope`, it wraps the reply the way Cloudflare does.
    pub(crate) fn fake_server(envelope: bool) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Value>>>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let log = seen.clone();
        std::thread::spawn(move || {
            for mut req in server.incoming_requests() {
                let mut body = String::new();
                std::io::Read::read_to_string(req.as_reader(), &mut body).unwrap();
                let mut v: Value = serde_json::from_str(&body).unwrap_or_default();
                let answers: serde_json::Map<String, Value> = v["questions"].as_object().into_iter().flatten().map(|(k, _)| (k.clone(), json!({"type": "noul", "noul": 0.75}))).collect();
                let reply = json!({"model": v["model"], "answers": answers});
                let reply = if envelope { json!({"result": reply, "success": true, "errors": []}) } else { reply };
                v["auth"] = json!(req.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.as_str().to_string()));
                v["path"] = json!(req.url());
                log.lock().unwrap().push(v);
                let _ = req.respond(tiny_http::Response::from_string(reply.to_string()));
            }
        });
        (format!("http://127.0.0.1:{port}"), seen)
    }

    pub(crate) fn fake_clef() -> (String, std::sync::Arc<std::sync::Mutex<Vec<Value>>>) {
        fake_server(false)
    }

    /// Settings with decisions on, pointed at `url`.
    pub(crate) fn on(url: &str) -> Settings {
        Settings { decisions_on: true, decisions_url: url.into(), ..Settings::default() }
    }

    #[test]
    fn off_until_turned_on() {
        assert!(route(&Settings::default()).unwrap_err().contains("off"));
        let r = route(&Settings { decisions_on: true, ..Settings::default() }).unwrap();
        assert_eq!(r, Route { url: "http://127.0.0.1:8001/v1/systemone".into(), model: "clef-flash".into(), key: None });
    }

    #[test]
    fn cloud_providers_need_a_key_and_fill_their_url() {
        let mut st = Settings { decisions_on: true, decisions_provider: "rune".into(), ..Settings::default() };
        if std::env::var("FIREWORKS_API_KEY").is_err() {
            assert!(route(&st).unwrap_err().contains("FIREWORKS_API_KEY"));
        }
        st.decisions_key = "fw".into();
        let r = route(&st).unwrap();
        assert_eq!((r.url.as_str(), r.model.as_str(), r.key.as_deref()), ("https://api.fireworks.ai/inference/v1/systemone", "accounts/fireworks/models/rune", Some("fw")));
        st.decisions_provider = "cloudflare".into();
        st.decisions_account = "acc123".into();
        st.decisions_model = "clef".into();
        assert_eq!(route(&st).unwrap().url, "https://api.cloudflare.com/client/v4/accounts/acc123/ai/run/@cf/cloudflare/clef");
        st.decisions_provider = "custom".into();
        st.decisions_model.clear();
        assert!(route(&st).unwrap_err().contains("model"));
    }

    #[test]
    fn endpoint_url_keeps_full_endpoints() {
        assert_eq!(endpoint_url("http://studio.local:8001/"), "http://studio.local:8001/v1/systemone");
        assert_eq!(endpoint_url("https://api.fireworks.ai/inference/v1/systemone"), "https://api.fireworks.ai/inference/v1/systemone");
        assert!(endpoint_url("https://api.cloudflare.com/client/v4/accounts/a/ai/run/@cf/cloudflare/clef").ends_with("/clef"));
    }

    #[test]
    fn decide_sends_model_state_key_and_unwraps_envelopes() {
        let (url, seen) = fake_server(false);
        let mut st = on(&url);
        st.decisions_key = "k1".into();
        let a = decide(&route(&st).unwrap(), &json!("I'll send the slides Friday"), &json!({"promised": {"type": "noul"}})).unwrap();
        assert_eq!(a["promised"]["noul"], 0.75);
        let sent = &seen.lock().unwrap()[0];
        assert_eq!((sent["model"].as_str(), sent["state"].as_str(), sent["auth"].as_str(), sent["path"].as_str()), (Some("clef-flash"), Some("I'll send the slides Friday"), Some("Bearer k1"), Some("/v1/systemone")));
        let (url, _) = fake_server(true);
        assert!(test(&route(&on(&url)).unwrap()).unwrap().contains("0.75"));
    }

    #[test]
    fn unreachable_server_names_the_url() {
        let e = decide(&route(&on("http://127.0.0.1:9")).unwrap(), &json!("x"), &json!({})).unwrap_err();
        assert!(e.contains("127.0.0.1:9"), "{e}");
    }

    #[test]
    fn task_state_uses_names() {
        let s = Store::sample(crate::model::now());
        let t = s.tasks.iter().find(|t| t.project.is_some() && !t.tags.is_empty()).unwrap();
        let v = task_state(&s, t);
        assert_eq!(v["task"], t.name);
        assert!(v["project"].is_string() && !v["tags"].as_array().unwrap().is_empty());
    }
}
