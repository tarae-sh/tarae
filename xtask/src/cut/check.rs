//! What `record check` found inside the cut (the design session, after take 1's try): no login on screen, and a weak
//! hit — a two-letter login as a bare word, `wq wheel` in an `ls -la` — is still one (it starts the user's mail too).
//! The take's `check.txt` read (else the check run on its screens), and every hit, strong or weak, any moment of which a
//! span of the cut shows — the take part's stretches not left out, where it ends, the last screen held — named in
//! `cut.txt`: its video time, its take time, its row and its words. The cut hides none: the driver shows what's there to
//! the user, who decides.

use std::path::Path;

use super::{Pace, Program, What, clock, video_time};
use crate::Result;
use crate::record::check::{self, Hit, Yours};

/// The take's hits and where they came from: its `check.txt`, else the check run now on its screens.
pub(crate) fn read(dir: &Path) -> Result<(Vec<Hit>, &'static str)> {
    let p = dir.join("check.txt");
    match std::fs::read_to_string(&p) {
        Ok(text) => {
            Ok((check::parse(&text).map_err(|e| format!("{}: {e}", p.display()))?, "the take's check.txt"))
        }
        Err(_) => Ok((check::scan(dir, &Yours::here())?, "no check.txt: the check run now")),
    }
}

/// Where the video shows any moment of the take's `from`..`to` (ms), each the first such moment (µs): where it ends,
/// the take part's first stretch not left out, else the last screen held.
pub(crate) fn shown(prog: &Program, from: u64, to: u64) -> Vec<u64> {
    let cut = prog.cut;
    let mut at = Vec::new();
    let o = &cut.opening;
    let a = from.max(o.from);
    if a < to.min(o.from + o.len)
        && let Some(p) = prog.parts.iter().find(|p| matches!(p.what, What::Ends { .. }))
    {
        at.push(p.at + (a - o.from) * 1000);
    }
    let map = &cut.map;
    let first = (map.segs().iter())
        .filter(|s| s.pace != Pace::Out)
        .map(|s| (from.max(s.from), to.min(s.to)))
        .find(|(a, b)| a < b)
        .and_then(|(a, _)| prog.video_of(map.video(a)));
    let held = map.take(map.len()).0;
    let last = || prog.parts.iter().find(|p| p.what == What::Last).map(|p| p.at);
    at.extend(first.or_else(|| (from <= held && held <= to).then(last).flatten()));
    at.sort_unstable();
    at
}

/// `cut.txt`'s lines: every hit (`source` said) the cut shows, by its first video time — or none inside the cut.
pub(crate) fn said(prog: &Program, hits: &[Hit], source: &str) -> Vec<String> {
    let take = |ms: u64| format!("{}.{:02}", clock(ms), ms % 1000 / 10);
    let mut inside: Vec<(u64, String, bool)> = Vec::new();
    for h in hits {
        let at = shown(prog, h.from, h.to);
        let Some(&first) = at.first() else { continue };
        let when: Vec<String> = at.iter().map(|&v| video_time(v)).collect();
        inside.push((
            first,
            format!(
                "  {} in the video (take {}–{}), row {}: {} {}{} │ {}",
                when.join(" and "),
                take(h.from),
                take(h.to),
                h.row,
                h.kind,
                h.found,
                if h.weak { " (weak)" } else { "" },
                h.text.trim_end()
            ),
            h.weak,
        ));
    }
    inside.sort_by_key(|i| (i.0, i.1.clone()));
    let weak = inside.iter().filter(|i| i.2).count();
    // (a crossing draws only frames the cut shows anyway — the one before it and the one after: its span's)
    let head = format!(
        "check ({source}): {} strong, {weak} weak inside the cut's spans, crossings included — of the take's {} hits",
        inside.len() - weak,
        hits.len()
    );
    match inside.is_empty() {
        true => vec![format!("{head}; none inside the cut")],
        false => std::iter::once(format!("{head}; for the user to decide:"))
            .chain(inside.into_iter().map(|i| i.1))
            .collect(),
    }
}
