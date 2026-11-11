//! A key script played into a take (`record --keys FILE`): the shots' key syntax where it fits (`shot.rs`'s top), a
//! key a line — the spike's own take, its tests, and the driver's typing later (V3).
//!
//! A line is a key as a shot sends it (`\r` enter, `\t` tab, `\x1b` esc, `\xHH` any byte, the rest as UTF-8 — a
//! word typed at once, 0.35 s after it as a shot does), or one of: `@type:<text>` typed with a person's rhythm (the
//! scenes' typist, `shot::typed`); `@3` (`@0.5`) a pause; `@?text` (`@30?text`) until `text` is on the screen, `@!text`
//! (`@30!text`) until it's gone — a wait that times out is said in the take (`missed`) and the script goes on. An
//! empty line, or one starting `#`, is nothing (a key `#` is `\x23`; a space is a line of one space).
//!
//! Where it doesn't fit, said: a scene's marks for its video (`@rec`, `@beat`, `@zoom`, `@sound`, `@note`, `@hook`,
//! `@cut?`) belong to a scripted take and a live one has none (its landmarks are tarae's own state); the mouse's
//! (`@press?`, `@hover?` …) and `@=done` aren't built here yet — each refused by its line.

use std::time::{Duration, Instant};

use super::Seen;
use crate::Result;
use crate::shot;

/// The pause after a key sent whole, as a shot's.
const AFTER_KEY: Duration = Duration::from_millis(350);

/// One step of a key script.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Step {
    Send(Vec<u8>),
    Type(String),
    Pause(Duration),
    Wait { text: String, max: Duration, gone: bool },
}

/// A key script's text as its steps; a line it can't take refused, by its number.
pub(crate) fn parse(text: &str) -> Result<Vec<Step>> {
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut steps = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        steps.push(step(line).map_err(|e| format!("line {}: {e}", n + 1))?);
    }
    Ok(steps)
}

/// A scene's keys (a shot's arguments) as steps.
pub(crate) fn of_keys(keys: &[&str]) -> Result<Vec<Step>> {
    keys.iter().map(|k| step(k).map_err(|e| format!("{k:?}: {e}"))).collect()
}

fn secs(s: f64) -> Duration {
    Duration::from_secs_f64(s.max(0.0))
}

fn step(k: &str) -> Result<Step> {
    if let Some(text) = k.strip_prefix("@type:") {
        return Ok(Step::Type(text.to_string()));
    }
    if let Some((max, text)) = shot::wait_key(k) {
        return Ok(Step::Wait { text: text.to_string(), max: secs(max), gone: false });
    }
    if let Some((max, text)) = shot::gone_key(k) {
        return Ok(Step::Wait { text: text.to_string(), max: secs(max), gone: true });
    }
    if let Some(s) = k.strip_prefix('@').and_then(|s| s.parse::<f64>().ok()) {
        return Ok(Step::Pause(secs(s)));
    }
    if k.starts_with('@') && k.len() > 1 {
        let mark = k.split([':', '?', '!', '=', '+', '*']).next().unwrap_or(k);
        return Err(format!(
            "`{mark}` isn't a live take's step: a scene's video marks (@rec @beat @zoom @sound @note @hook @cut) belong \
             to a scripted take, and the mouse's and @=done aren't built here — keys, @type:, @N, @?text, @!text"
        ));
    }
    Ok(Step::Send(shot::unescape(k)))
}

/// `steps` played through `send` (each write handed on to the take's log by it), the waits on `seen` — stopped where
/// the screen ends. Each wait that timed out, said.
pub(crate) fn play(steps: &[Step], send: &mut dyn FnMut(&[u8]), seen: &Seen) -> Vec<String> {
    let mut missed = Vec::new();
    for s in steps {
        if seen.ended() {
            missed.push(format!("the screen ended before {s:?}"));
            break;
        }
        match s {
            Step::Send(bytes) => {
                send(bytes);
                seen.pause(AFTER_KEY);
            }
            Step::Type(text) => {
                for (i, (c, gap)) in shot::typed(text).into_iter().enumerate() {
                    if i > 0 && !seen.pause(secs(gap)) {
                        break;
                    }
                    send(c.to_string().as_bytes());
                }
                seen.pause(AFTER_KEY);
            }
            Step::Pause(d) => {
                seen.pause(*d);
            }
            Step::Wait { text, max, gone } => {
                let until = Instant::now() + *max;
                if !seen.wait(until, |t| t.contains(text.as_str()) != *gone) {
                    let what = if *gone { "to go" } else { "on screen" };
                    missed.push(format!("waited {:.1} s for {text:?} {what}", max.as_secs_f64()));
                }
            }
        }
    }
    missed
}

/// tarae quit as a shot quits it (`shot::quit`): esc alone (`ESC :` in one read is Alt-:), `:q!`, and a kill of `pid`
/// where the screen hasn't ended two seconds later.
pub(crate) fn quit(send: &mut dyn FnMut(&[u8]), seen: &Seen, pid: u32) {
    send(b"\x1b");
    seen.pause(Duration::from_millis(60));
    send(b":q!\r");
    if !seen.wait(Instant::now() + Duration::from_secs(2), |_| false) && !seen.ended() {
        // SAFETY: a signal to the take's own child, not yet waited for (its pid still its own).
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
    }
}
