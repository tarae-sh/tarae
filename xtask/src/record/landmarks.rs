//! A take's landmarks: tarae's own state as it changes, read from its UI state socket (`docs/design/ui-state.md`,
//! "Events") on a thread of the recorder's — the cut's edges (V1's edit list). Nothing of tarae changed for them: what
//! its events say, and where one doesn't say enough, a snapshot asked after it.
//!
//! - `turn-started`, `turn-ended` — the `turn` event (session, calls, took_ms).
//! - `card-shown` — a card's first `card-up`, its `kind` from the snapshot asked after it (the event carries none);
//!   `card-answered` — its `card-gone`, `how` (`answered y`, `declined`, `turn-ended` …), its kind as it was shown.
//! - `run-started`, `run-ended` — the `run` event's `spawned` step (the command started: an asked run's earlier steps,
//!   `card up` … `answered yes` … `saved`, are its card's, already `card-shown`/`card-answered`) and the one with
//!   `ended`, its step's words (`exit 0` …) as tarae says them.
//! - `review-opened` — the `review` event's `began` (its kind).
//! - `front` — the workspace in front changed (`front` in a snapshot asked after a `keys` or `session` event; no event
//!   says it).
//! - `bring` — the `bring` event (motion's, 3dfd1088): a bring ended — `how` it did (`landed`, `preflight-failed`,
//!   `conflict`, `declined`), the workspace's `name`, its `base`, and what came with it (`tip`, `commits`, `conflicts`,
//!   `preflight`).
//! - `mark` — the driver's note at that moment (`record mark`, `hands.rs`), not tarae's: written by the worker.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::Log;
use super::hands::Hub;
use crate::shot;

/// How often a socket not bound yet (tarae starting) is looked for again.
const LOOK_AGAIN: Duration = Duration::from_millis(20);

/// The watcher: connected to `sock` once it's there (until the take ends, `stopped`), every line it's told handed to
/// the worker — and after the ones a landmark needs a snapshot for (`wants_state`), that snapshot's part (`part`).
pub(super) fn watch(sock: &Path, tx: &Sender<Log>, stopped: &AtomicBool, hub: &Hub) {
    let stream = loop {
        match shot::ui_connect(sock) {
            Ok(s) => break s,
            Err(e) if shot::not_there_yet(&e) && !stopped.load(Ordering::Relaxed) => {
                std::thread::sleep(LOOK_AGAIN)
            }
            Err(e) => {
                if !shot::not_there_yet(&e) {
                    let said = format!("{}: {e}", sock.display());
                    let _ = tx.send(Log::Told(Instant::now(), json!({ "event": "unwatched", "said": said })));
                }
                return;
            }
        }
    };
    if writeln!(&stream, "{}", json!({ "watch": [] })).is_err() {
        return;
    }
    for line in BufReader::new(&stream).lines() {
        let Ok(line) = line else { return };
        let Ok(ev) = serde_json::from_str::<Value>(&line) else { continue };
        let at = Instant::now();
        let wants = wants_state(&ev);
        hub.told(at, &ev);
        if tx.send(Log::Told(at, ev)).is_err() {
            return;
        }
        if !wants {
            continue;
        }
        let ask = json!({ "get": "state" });
        let state = shot::ui_connect(sock).ok().and_then(|s| shot::ui_ask_on(s, sock, &ask).ok());
        if let Some(state) = state {
            let _ = tx.send(Log::State(Instant::now(), part(&state)));
        }
    }
}

/// Whether a landmark needs a snapshot after `ev`: a card come up (its kind), where the keys went or what a session
/// does changed (which workspace is in front).
fn wants_state(ev: &Value) -> bool {
    matches!(ev["event"].as_str(), Some("card-up" | "keys" | "session"))
}

/// What of a snapshot the landmarks read, as `events.jsonl` keeps it: the workspace in front, the workspaces' ids and
/// names, the cards' ids, kinds, sessions and asks.
pub(super) fn part(state: &Value) -> Value {
    let list = |k: &str, fields: &[&str]| -> Vec<Value> {
        (state[k].as_array().into_iter().flatten())
            .map(|x| fields.iter().map(|f| (f.to_string(), x[*f].clone())).collect())
            .collect()
    };
    json!({
        "front": state["front"],
        "workspaces": list("workspaces", &["id", "name"]),
        "cards": list("cards", &["id", "kind", "session", "asks"]),
    })
}

/// A landmark: its time (ms since the take's start) and what it says.
pub(crate) type Mark = (u64, Value);

/// A landmark as `landmarks.jsonl`'s line: `{"ms":…, "what":…, …}`.
pub(super) fn line((ms, mut v): Mark) -> String {
    v["ms"] = json!(ms);
    v.to_string()
}

/// What the landmarks so far remember: the workspace in front and the workspaces' names, each card's kind, the cards
/// come up whose kind the next snapshot tells (each with when it came), the runs begun.
#[derive(Default)]
pub(crate) struct Landmarks {
    front: Option<String>,
    names: HashMap<String, String>,
    kinds: HashMap<String, String>,
    pending: Vec<(u64, String, Value)>,
    shown: HashSet<String>,
    runs: HashSet<String>,
}

impl Landmarks {
    /// The landmarks an event at `ms` makes.
    pub(crate) fn told(&mut self, ms: u64, ev: &Value) -> Vec<Mark> {
        let mut out = Vec::new();
        let id = ev["id"].as_str().unwrap_or_default().to_string();
        match ev["event"].as_str().unwrap_or_default() {
            "turn" => {
                let what = format!("turn-{}", ev["what"].as_str().unwrap_or("?"));
                out.push((
                    ms,
                    json!({ "what": what, "session": ev["session"], "calls": ev["calls"], "took_ms": ev["took_ms"] }),
                ));
            }
            "card-up" if self.shown.insert(id.clone()) => {
                self.pending.push((ms, id, json!({ "session": ev["session"], "asks": ev["asks"] })));
            }
            "card-gone" => {
                // (gone before a snapshot said its kind: shown all the same, its kind not known)
                out.extend(self.unpend(&id, None));
                let kind = self.kinds.get(&id).cloned();
                out.push((ms, json!({ "what": "card-answered", "id": id, "kind": kind, "how": ev["how"] })));
            }
            "run" if ev["ended"] == true => {
                self.runs.insert(id.clone());
                out.push((
                    ms,
                    json!({ "what": "run-ended", "id": id, "run": ev["what"], "step": ev["step"] }),
                ));
            }
            "run" if ev["step"] == "spawned" && self.runs.insert(id.clone()) => {
                out.push((
                    ms,
                    json!({ "what": "run-started", "id": id, "run": ev["what"], "step": ev["step"] }),
                ));
            }
            "bring" => {
                let mut v = json!({ "what": "bring", "session": ev["session"], "name": ev["name"],
                                    "base": ev["base"], "how": ev["how"] });
                for k in ["tip", "commits", "conflicts", "preflight"] {
                    if !ev[k].is_null() {
                        v[k] = ev[k].clone();
                    }
                }
                out.push((ms, v));
            }
            "review" if ev["what"] == "began" => {
                out.push((
                    ms,
                    json!({ "what": "review-opened", "session": ev["session"], "kind": ev["kind"] }),
                ));
            }
            _ => {}
        }
        out
    }

    /// The landmarks a snapshot's part (`part`) at `ms` makes: the cards come up since, with their kinds, at the times
    /// they came; the workspace in front, where it changed (the first one said too: where the take began).
    pub(crate) fn state(&mut self, ms: u64, s: &Value) -> Vec<Mark> {
        for w in s["workspaces"].as_array().into_iter().flatten() {
            if let (Some(id), Some(name)) = (w["id"].as_str(), w["name"].as_str()) {
                self.names.insert(id.to_string(), name.to_string());
            }
        }
        for c in s["cards"].as_array().into_iter().flatten() {
            if let (Some(id), Some(kind)) = (c["id"].as_str(), c["kind"].as_str()) {
                self.kinds.insert(id.to_string(), kind.to_string());
            }
        }
        let mut out = Vec::new();
        let ids: Vec<String> = self.pending.iter().map(|p| p.1.clone()).collect();
        for id in ids {
            let kind = self.kinds.get(&id).cloned();
            out.extend(self.unpend(&id, kind));
        }
        if let Some(front) = s["front"].as_str()
            && self.front.as_deref() != Some(front)
        {
            let was = self.front.replace(front.to_string());
            let name = |id: Option<&str>| id.and_then(|i| self.names.get(i)).cloned();
            let v = json!({ "what": "front", "to": front, "name": name(Some(front)), "was": was,
                            "was_name": name(was.as_deref()) });
            out.push((ms, v));
        }
        out.sort_by_key(|m| m.0);
        out
    }

    /// Card `id`'s `card-shown`, where it waited for its kind, at the time it came.
    fn unpend(&mut self, id: &str, kind: Option<String>) -> Option<Mark> {
        let at = self.pending.iter().position(|p| p.1 == id)?;
        let (ms, id, mut v) = self.pending.remove(at);
        v["what"] = json!("card-shown");
        v["id"] = json!(id);
        v["kind"] = json!(kind);
        Some((ms, v))
    }
}
