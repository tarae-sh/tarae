//! `:web` reading saved pages on a worker, simulated: a page asked (again: read anew, another part), a file that
//! isn't a page asked, the session's copy of a fetched page asked (read on a worker, no network), a page fetched from
//! a server of the test's own on this machine (its way a stand-in, its proxy made before the simulation: a proxy's
//! threads outlive a step), an address the fetch refuses, `C-c` on the page in front, the buffer in front closed, the
//! user taking their time, a frame; Claude's `read` of a page (P4): the session's copy, this machine's page at a port
//! runs reach, a host not let through (its card, answered `y`, `s` or `n`) — the reads' and fetches' jobs landing
//! among them in any order; Claude's `web_search` (its first card answered `y`, `s` or `n`, a helper's list, cost and
//! end landing whenever, the turn's end); Claude's read of your wiki's page (a copy fetched with your token: its own
//! card, and after it each search asks). After every step: one buffer at most per page's address, what isn't a page
//! never read into its buffer, no step left by a read, no card for a host answered for the session or the
//! conversation, a wiki's card saying the token, no search card once the chat or the session answered but W3's own
//! after a token read, no search's helper twice; and B5's count of the turn's pages not read the same as the pages
//! its reads' answers say didn't come (a host refused, the user's `n`), never one the turn's end stopped.

use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;

const SEEDS: &[u64] = &[0, 1, 2];

const INPUTS: usize = 30;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_page_read_twice_is_one_buffer() {
    super::seeds("a_page_read_twice_is_one_buffer", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    // (the calls' ids start again each seed: the answers of the last one go)
    crate::tools::ANSWERED.with(|a| a.borrow_mut().clear());
    let dir = crate::scratch::Scratch::new("sim-web");
    std::fs::write(
        dir.join("a.html"),
        "<html><head><title>A page</title></head><body><main><h1>A</h1><p>Its text.</p></main></body></html>",
    )
    .unwrap();
    std::fs::write(dir.join("b.pdf"), "%PDF-1.7\n").unwrap();
    use crate::web::fetch::tests::{ok, server, way, yours_rules};
    let (at, _) =
        server(false, |_| ok("<title>Fetched</title><main><h1>Fetched</h1><p>Its text.</p></main>"));
    let base = way(at, yours_rules());
    let mut sim = Sim::new("a_page_read_twice_is_one_buffer", choices, LIMIT);
    let mut ed = Editor::new(crate::config::Config::default());
    ed.workspace.root = dir.path().to_path_buf();
    ed.workspace.claude.chat = Some(crate::chat::Chat::new());
    ed.workspace.claude.chat.as_mut().unwrap().web_search = true;
    // (the search's helper a stand-in: what it's told goes here, its stream the steps')
    let (told, _heard) = std::sync::mpsc::channel();
    ed.search_lines = Some(told);
    ed.config.llm.sandbox_ports.push(at.port());
    ed.workspace.web_way =
        Some(std::sync::Arc::new(move |stop| crate::web::fetch::Way { stop, ..base.clone() }));
    let fetched = format!("web http://localhost:{}/", at.port());
    let kept = crate::web::fetch::Fetched {
        asked: "https://kept.test/".into(),
        url: "https://kept.test/".into(),
        status: 200,
        content_type: Some("text/html".into()),
        bytes: b"<title>Kept</title><main><h1>Kept</h1><p>Its text.</p></main>".to_vec(),
        from: Vec::new(),
        hops: 0,
        upgraded: false,
        said: None,
        cut: false,
        took: Duration::from_millis(30),
        elsewhere: None,
        signed: false,
    };
    let wiki = crate::web::fetch::Fetched {
        asked: WIKI.into(),
        url: WIKI.into(),
        content_type: Some("text/html".into()),
        bytes: b"<title>Runbook</title><main><h1>Runbook</h1><p>Roll back.</p></main>".to_vec(),
        signed: true,
        ..kept.clone()
    };
    let _ = ed.workspace.pages.put(std::sync::Arc::new(kept), crate::clock::now());
    let _ = ed.workspace.pages.put(std::sync::Arc::new(wiki), crate::clock::now());
    let t: toml::Table = toml::from_str(
        r#"v = [{ base = "https://wiki.test/confluence", kind = "confluence-dc", token-env = "TARAE_TEST_PAT" }]"#,
    )
    .unwrap();
    ed.config.llm.signed_in =
        crate::web::signed::with_env(crate::web::signed::wikis_of(&t["v"]).unwrap(), |_| {
            Some("tarae-fake-token".into())
        });
    let mut n = 0u64;
    let input = move |sim: &mut Sim, ed: &mut Editor| {
        n += 1;
        input(sim, ed, &fetched, n)
    };
    sim.walk(&mut ed, INPUTS, input, held, |ed| {
        let titles: Vec<&str> = ed.docs.iter().filter_map(|d| d.title.as_deref()).collect();
        format!("front {:?} · titled {titles:?}", ed.doc().title)
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

/// A wiki page's copy, fetched with your token (`web.signed-in` names its base).
const WIKI: &str = "https://wiki.test/confluence/display/OPS/Runbook";

fn input(sim: &mut Sim, ed: &mut Editor, fetched: &str, n: u64) -> String {
    let c = &mut sim.choices;
    // (by arm: the page 3, another part of it 2, the PDF 1, the session's copy 2, a fetch 2, a refused address 1, a
    // stop 1, a close 1, time 2, a frame 1; Claude reading the copy 1, this machine's page 1, a new host 2, a card's
    // answer 2; Claude searching 3, a helper's stream 2, a search card's answer 2, the turn's end 1; Claude reading the
    // wiki's copy 2)
    match c.weighted(&[3, 2, 1, 2, 2, 1, 1, 1, 2, 1, 1, 1, 2, 2, 3, 2, 2, 1, 2]) {
        18 => claude_reads(ed, n, WIKI),
        14 => {
            let Some(generation) = ed.workspace.claude.chat.as_ref().map(|c| c.generation()) else {
                return "(no chat)".into();
            };
            let call = crate::tools::Call::of(generation, "", serde_json::json!(1000 + n));
            let words = format!("tokio release {n}");
            match crate::web::search::call(ed, call, &serde_json::json!({ "query": words })) {
                Ok(()) => format!("Claude searches {words:?}"),
                Err(e) => format!("Claude searches {words:?} — {e}"),
            }
        }
        15 => {
            let Some(s) = ed.workspace.claude.searches.first() else { return "(no search runs)".into() };
            // (named by its words: its id is a process-wide count, never the same run to run)
            let (id, answered, words) = (s.id, s.answered, s.query.words.clone());
            use crate::llm::StreamEv;
            let ev = match *c.pick(&[0, 1, 2]) {
                0 if !answered => StreamEv::Searched(Box::new(crate::web::search::Found {
                    links: vec![crate::web::search::Link {
                        title: "Tokio".into(),
                        url: "https://tokio.test/".into(),
                    }],
                    summary: String::new(),
                })),
                0 | 1 => StreamEv::Spent(crate::usage::Spent {
                    total_cost: Some(0.01),
                    at_ms: n,
                    ..Default::default()
                }),
                _ => StreamEv::Done(Err("error_max_turns".into())),
            };
            let what = match &ev {
                StreamEv::Searched(_) => "its list",
                StreamEv::Spent(_) => "its cost",
                _ => "its end",
            };
            crate::web::search::on_event(ed, id, ev);
            format!("search {words:?}: {what}")
        }
        16 => {
            if !ed.offers.first().is_some_and(|o| o.kind() == "web-search") {
                return "(no search's card up)".into();
            }
            let key = *c.pick(&['y', 's', 'n']);
            match key {
                'y' => ed.offer_accept(),
                's' => ed.offer_for_session(),
                _ => ed.offer_dismiss(),
            }
            format!("answered a search's card {key}")
        }
        17 => {
            crate::tools::turn_over(ed);
            "the turn ends".into()
        }
        10 => claude_reads(ed, n, "https://kept.test/"),
        11 => {
            let url = fetched.replace("web ", "");
            claude_reads(ed, n, &url).replace(&url, "http://localhost/")
        }
        // (a host the stand-in's resolver doesn't know: refused at once once let through — the server is plain http)
        12 => claude_reads(ed, n, "https://new.test/p"),
        13 => {
            let Some(o) = ed.offers.first().filter(|o| o.kind() == "web-read") else {
                return "(no read's card up)".into();
            };
            let title = o.title.clone();
            match *c.pick(&['y', 's', 'n']) {
                'y' => ed.offer_accept(),
                's' => ed.offer_for_session(),
                _ => ed.offer_dismiss(),
            }
            format!("answered {title:?}")
        }
        0 => typed(ed, "web a.html"),
        1 => typed(ed, "web a.html page"),
        2 => typed(ed, "web b.pdf"),
        3 => typed(ed, "web https://kept.test/"),
        // (this machine's page, never kept: every pick goes to the server — the kept copy is arm 3's and the fetch
        // tests')
        4 => typed(ed, fetched).replace(fetched, "web http://localhost/"),
        5 => typed(ed, "web https://u:p@kept.test/"),
        6 => {
            let reading = crate::web::page::takes(ed);
            ed.handle_key(crate::key::Key {
                code: crate::key::Code::Char('c'),
                ctrl: true,
                alt: false,
                shift: false,
            });
            format!("C-c (a read under way: {reading})")
        }
        7 if ed.docs.len() > 1 => typed(ed, "buffer-close"),
        7 => "(one buffer: none closed)".into(),
        8 => {
            let ms = *c.pick(&[1, 10, 50]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => {
            crate::term::render(ed, &mut Vec::new(), 100, 30).unwrap();
            "a frame".into()
        }
    }
}

/// Claude's `read` of `url`, call `n`.
fn claude_reads(ed: &mut Editor, n: u64, url: &str) -> String {
    let Some(generation) = ed.workspace.claude.chat.as_ref().map(|c| c.generation()) else {
        return "(no chat)".into();
    };
    let call = crate::tools::Call::of(generation, "", serde_json::json!(n));
    match crate::readtool::call(ed, call, &serde_json::json!({ "files": [url] })) {
        Ok(()) => format!("Claude reads {url}"),
        Err(e) => format!("Claude reads {url} — {e}"),
    }
}

fn typed(ed: &mut Editor, line: &str) -> String {
    match crate::typed::execute(ed, line) {
        Ok(()) => format!(":{line}"),
        Err(e) => format!(":{line} — {e}"),
    }
}

/// One buffer at most a page's address; the PDF's never read into; no step left by a read.
fn held(ed: &Editor) -> Result<(), String> {
    if ed.toasts.iter().any(|t| t.step == Some("web")) {
        return Err("a read's step left".into());
    }
    let pages: Vec<&crate::document::Document> =
        ed.docs.iter().filter(|d| crate::web::page::of(d).is_some()).collect();
    let mut seen = std::collections::HashSet::new();
    if let Some(twice) = pages.iter().filter_map(|d| d.virtual_uri.as_deref()).find(|u| !seen.insert(*u)) {
        return Err(format!("two buffers of {twice:?}"));
    }
    let pdf = pages.iter().find(|d| d.virtual_uri.as_deref().is_some_and(|u| u.ends_with(".pdf")));
    if let Some(d) = pdf.filter(|d| crate::web::page::of(d).is_some_and(|p| p.read.is_some())) {
        return Err(format!("what isn't a page read into {:?}", d.title));
    }
    // (a card a call; but none for a host this session or conversation answered)
    let answered = |h: &str| {
        let at = (h.to_string(), 443);
        ed.workspace.claude.web_hosts.contains(&at)
            || ed
                .workspace
                .claude
                .chat
                .as_ref()
                .is_some_and(|c| c.web_refused.contains(&at) || c.web_hosts.contains(&at))
    };
    let cards = ed.offers.iter().filter_map(|o| match &o.what {
        crate::offer::What::WebRead(a) => Some(a.host.as_str()),
        _ => None,
    });
    if let Some(h) = cards.into_iter().find(|h| answered(h)) {
        return Err(format!("a card for {h}, which was answered for good"));
    }
    // (a token read's card says so)
    let unsaid = ed
        .offers
        .iter()
        .any(|o| matches!(&o.what, crate::offer::What::WebRead(a) if a.host == "wiki.test" && !a.signed));
    if unsaid {
        return Err("a card for the wiki's page that doesn't say the token".into());
    }
    // (a search's card only while neither the chat nor the session answered — but W3's, each search's own after a
    // token read)
    let searched = ed.workspace.claude.searches_ok
        || ed.workspace.claude.chat.as_ref().is_some_and(|c| c.searches.is_some());
    let unguarded =
        ed.offers.iter().any(|o| matches!(&o.what, crate::offer::What::WebSearch(a) if !a.guarded));
    if searched && unguarded {
        return Err("a search's card after the chat or the session answered".into());
    }
    let read_signed = ed.workspace.claude.chat.as_ref().is_some_and(|c| c.signed_read);
    if !read_signed
        && ed.offers.iter().any(|o| matches!(&o.what, crate::offer::What::WebSearch(a) if a.guarded))
    {
        return Err("W3's search card in a chat that read no page with your token".into());
    }
    // (three helpers at most, and one waits only while three run)
    let (running, queued) = (ed.workspace.claude.searches.len(), ed.workspace.claude.searches_queued.len());
    if running > crate::web::search::RUNNING_MAX || queued > 0 && running < crate::web::search::RUNNING_MAX {
        return Err(format!("{running} searches run, {queued} wait"));
    }
    let mut ids: Vec<u64> = ed.workspace.claude.searches.iter().map(|s| s.id).collect();
    ids.dedup();
    if ids.len() != ed.workspace.claude.searches.len() {
        return Err("a search twice".into());
    }
    // (B5: a read's line `== <address>: why` is a page that didn't come — here's pages are all HTML, none redirects —,
    // `== <address> (…)` one read; one the turn's end stopped isn't counted)
    let said = crate::tools::ANSWERED.with(|a| {
        let a = a.borrow();
        let reads = a.iter().filter(|(id, _)| id.as_u64().is_some_and(|n| n < 1000));
        let lines = reads.filter_map(|(_, r)| r.as_ref().ok()).flat_map(|t| t.lines().map(String::from));
        let no_page = |l: &str| {
            l.strip_prefix("== ").and_then(|l| l.split(' ').next()).is_some_and(|w| w.ends_with(':'))
                && !l.contains("stopped")
        };
        lines.filter(|l| no_page(l)).count() as u64
    });
    let counted = ed.workspace.claude.usage.tally.sums().web_failed;
    match counted == said {
        true => Ok(()),
        false => Err(format!("{counted} pages counted not read, the answers say {said}")),
    }
}
