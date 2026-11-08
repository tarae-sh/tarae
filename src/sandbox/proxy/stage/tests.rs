//! The stage's pure part: `Exchange` fed a script of both sides' bytes at every split — each step cut once at each
//! place, and in random pieces — the bytes out each way always the bytes in, but for masking's named edits (P2:
//! the swaps, the scrubs, `Accept-Encoding`, `TE`); `decide`'s rules; the idle limits' phases and the tie rule. The
//! loop on loopback: `proxy::tests::terminate`.

use super::*;
use crate::sandbox::proxy::masked::Masked;
use crate::sandbox::proxy::parse::tests::Rng;

const NAME: &str = "origin.test";

/// Each step: the side its bytes come from (0 the run, 1 the host), and the bytes.
type Script<'a> = &'a [(usize, &'a [u8])];

/// What came of a script fed in pieces — every piece, past a stop too: the bytes out — [to the run, to the host] —,
/// the exchange, and its first stop.
struct Fed {
    out: [Vec<u8>; 2],
    x: Exchange,
    stop: Result<(), Stop>,
}

/// `script` fed to a new exchange (`port` its tunnel's), each step cut where `cuts(step, len)` says (ascending).
fn fed_at(script: Script, port: u16, cuts: impl FnMut(usize, usize) -> Vec<usize>) -> Fed {
    fed_in(script, port, None, cuts)
}

/// `fed_at`, masking `m`'s variables (its view toward the tunnel's host).
fn fed_in(
    script: Script,
    port: u16,
    m: Option<&Masked>,
    mut cuts: impl FnMut(usize, usize) -> Vec<usize>,
) -> Fed {
    let mut x = Exchange::new(NAME, port, m.map(|m| m.toward(NAME, port)));
    let (mut out, mut first) = ([Vec::new(), Vec::new()], Ok(()));
    for (k, &(side, bytes)) in script.iter().enumerate() {
        let mut at = 0;
        for cut in cuts(k, bytes.len()).into_iter().chain([bytes.len()]) {
            let piece = &bytes[at..cut];
            at = cut;
            let stop = match side {
                0 => x.run_sent(piece, &mut out[1]),
                _ => x.host_sent(piece, &mut out[0]),
            };
            if first.is_ok() {
                first = stop;
            }
        }
    }
    Fed { out, x, stop: first }
}

fn fed(script: Script) -> Fed {
    fed_at(script, 443, |_, _| Vec::new())
}

/// `script` fed whole, each step cut once at every place, and in 1–7-byte pieces 300 times: each time `same` holds
/// of what came against what came of it whole — which it returns.
fn at_any_split(script: Script, same: impl Fn(&Fed, &Fed, &str)) -> Fed {
    at_any_split_in(script, None, same)
}

/// `at_any_split`, masking `m`'s variables.
fn at_any_split_in(script: Script, m: Option<&Masked>, same: impl Fn(&Fed, &Fed, &str)) -> Fed {
    let whole = fed_in(script, 443, m, |_, _| Vec::new());
    for k in 0..script.len() {
        for c in 0..=script[k].1.len() {
            let f = fed_in(script, 443, m, |j, _| if j == k { vec![c] } else { Vec::new() });
            same(&whole, &f, &format!("step {k} cut at {c}"));
        }
    }
    let mut rng = Rng(0x57a6_e5ee_d1ce_0001);
    for n in 0..300 {
        let f = fed_in(script, 443, m, |_, len| {
            let mut cuts = vec![0];
            while *cuts.last().unwrap() < len {
                cuts.push((cuts.last().unwrap() + 1 + rng.below(7)).min(len));
            }
            cuts
        });
        same(&whole, &f, &format!("pieces, round {n}"));
    }
    whole
}

/// Every byte goes on as it came, each way — the run's to the host, the host's to the run — at any split, and the
/// exchange ends where it ends fed whole. The exchange fed whole.
fn passes(script: Script) -> Exchange {
    let side =
        |s: usize| script.iter().filter(|(k, _)| *k == s).flat_map(|(_, b)| b.to_vec()).collect::<Vec<u8>>();
    let (run, host) = (side(0), side(1));
    let whole = at_any_split(script, |whole, f, at| {
        assert_eq!(f.stop, Ok(()), "{at}");
        assert!(f.out[1] == run, "{at}: to the host {:?}", String::from_utf8_lossy(&f.out[1]));
        assert!(f.out[0] == host, "{at}: to the run {:?}", String::from_utf8_lossy(&f.out[0]));
        let state = |f: &Fed| (f.x.owed(), f.x.phase(), f.x.over());
        assert_eq!(state(f), state(whole), "{at}");
    });
    whole.x
}

const GET_A: &[u8] = b"GET /a HTTP/1.1\r\nHost: origin.test\r\nAccept: */*\r\n\r\n";
const GET_B: &[u8] = b"GET /b?q=1 HTTP/1.1\r\nHost: origin.test:443\r\n\r\n";
const OK_2: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";

/// A chunked upload (extensions, trailers) and a Content-Length download go on byte for byte, one after the other on
/// one connection — at rest after, nothing owed, the connection kept.
#[test]
fn a_chunked_upload_and_a_length_download_go_on_byte_for_byte() {
    let up: &[u8] = b"POST /up HTTP/1.1\r\nHost: origin.test\r\nTransfer-Encoding: chunked\r\n\r\n5;x=1\r\nhello\r\n3\r\n wo\r\n0\r\nX-T: 1\r\n\r\n";
    let down: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n0123456789";
    let x = passes(&[(0, up), (1, OK_2), (0, GET_B), (1, down)]);
    assert_eq!((x.owed(), x.phase(), x.over()), (0, Phase::Idle, false));
}

/// A HEAD's answer has no body whatever its length says: the next answer, pipelined behind it, is the next request's.
#[test]
fn a_heads_answer_has_no_body() {
    let head: &[u8] = b"HEAD /big HTTP/1.1\r\nHost: origin.test\r\n\r\n";
    let answer: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n";
    let x = passes(&[(0, head), (0, GET_A), (1, answer), (1, OK_2)]);
    assert_eq!((x.owed(), x.phase()), (0, Phase::Idle));
}

/// A 204 and a 304 have no body whatever their fields say (a length, chunks): the answers after them are the next
/// requests'.
#[test]
fn a_204_or_304_has_no_body_whatever_it_says() {
    let none: &[u8] = b"HTTP/1.1 204 No Content\r\nContent-Length: 5\r\n\r\n";
    let same: &[u8] = b"HTTP/1.1 304 Not Modified\r\nTransfer-Encoding: chunked\r\n\r\n";
    let x = passes(&[(0, GET_A), (0, GET_B), (0, GET_A), (1, none), (1, same), (1, OK_2)]);
    assert_eq!((x.owed(), x.phase()), (0, Phase::Idle));
}

/// `Expect: 100-continue`: the head goes on, the host's 100 comes back (interim: the request still owed), then the
/// body, then the final answer.
#[test]
fn a_100_continue_comes_back_before_the_body() {
    let head: &[u8] =
        b"PUT /f HTTP/1.1\r\nHost: origin.test\r\nContent-Length: 5\r\nExpect: 100-continue\r\n\r\n";
    let go: &[u8] = b"HTTP/1.1 100 Continue\r\n\r\n";
    let done: &[u8] = b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n";
    let x = passes(&[(0, head), (1, go), (0, b"hello"), (1, done)]);
    assert_eq!((x.owed(), x.phase()), (0, Phase::Idle));
    let f = fed(&[(0, head), (1, go)]);
    assert_eq!((f.x.owed(), f.x.phase()), (1, Phase::UnderWay), "its body still to come");
}

/// Two requests pipelined in one write are each answered in order (a chunked answer, then a length's).
#[test]
fn two_pipelined_requests_are_answered_in_order() {
    let both = [GET_A, GET_B].concat();
    let chunked: &[u8] = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nab\r\n0\r\n\r\n";
    let f = fed(&[(0, &both)]);
    assert_eq!((f.x.owed(), f.x.phase()), (2, Phase::Waiting));
    let x = passes(&[(0, &both), (1, chunked), (1, OK_2)]);
    assert_eq!((x.owed(), x.phase()), (0, Phase::Idle));
}

/// An answer with neither a length nor chunks is the host's until it closes: passed on to the end, then the exchange
/// is over (the run's side closes with close_notify) — at any split — where the host's TLS ended with close_notify;
/// without it, cut (it may be cut short). One whose length or chunks the close cuts short is said as cut.
#[test]
fn an_answer_with_neither_length_nor_chunks_ends_with_the_close() {
    let open: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nall of this, until the close";
    at_any_split(&[(0, GET_A), (1, open)], |_, f, at| {
        assert_eq!(f.stop, Ok(()), "{at}");
        assert!(f.out[0] == open && f.out[1] == GET_A, "{at}");
        let mut x = fed_at(&[(0, GET_A), (1, open)], 443, |_, _| Vec::new()).x;
        assert_eq!((x.owed(), x.phase(), x.over()), (1, Phase::UnderWay, false), "{at}");
        assert_eq!(x.host_ended(true, &mut Vec::new()), Ok(()), "{at}");
        assert_eq!((x.owed(), x.over()), (0, true), "{at}");
    });
    let mut x = fed(&[(0, GET_A), (1, open)]).x;
    let unclean =
        "its TLS ended without close_notify in an answer that runs to its close (it may be cut short)";
    assert_eq!(x.host_ended(false, &mut Vec::new()), Err(Stop::Cut(unclean.into())));
    for clean in [true, false] {
        let mut x = fed(&[(0, GET_A), (1, &OK_2[..OK_2.len() - 1])]).x;
        assert_eq!(
            x.host_ended(clean, &mut Vec::new()),
            Err(Stop::Cut("it closed part-way through an answer's body".into()))
        );
        let mut x = fed(&[(0, GET_A), (1, b"HTTP/1.1 200 OK\r\nCont")]).x;
        assert_eq!(
            x.host_ended(clean, &mut Vec::new()),
            Err(Stop::Failed("it closed part-way through an answer's head".into()))
        );
        // (between answers: an end — a request still owed sees the close, and its client may try again)
        let mut x = fed(&[(0, GET_A), (1, OK_2), (0, GET_B)]).x;
        assert_eq!((x.host_ended(clean, &mut Vec::new()), x.over()), (Ok(()), true));
    }
}

/// A request refused — a TRACE between two GETs pipelined in one write — goes nowhere, nor the one behind it: only
/// what came before it went on, at any split, and it's refused with its status (what's behind it dropped, not counted:
/// the report's "bytes sent after its last request" are the last request's).
#[test]
fn a_refused_request_and_what_follows_it_never_go_on() {
    let trace: &[u8] = b"TRACE / HTTP/1.1\r\nHost: origin.test\r\n\r\n";
    let all = [GET_A, trace, GET_B].concat();
    at_any_split(&[(0, &all)], |_, f, at| {
        assert!(f.out[1] == GET_A, "{at}: {:?}", String::from_utf8_lossy(&f.out[1]));
        assert!(
            matches!(&f.stop, Err(Stop::Refused(405, why)) if why.contains("echoes the request back")),
            "{at}"
        );
        assert_eq!(f.x.owed(), 1, "{at}: the GET before it still owed its answer");
        assert_eq!(f.x.dropped, 0, "{at}: what's behind a refusal isn't counted as after the last request");
    });
}

/// A request's head refused by the parser is refused with its status and words — the framing two ways, a coding
/// other than chunked, an absolute-form target, no Host — and nothing of it went on.
#[test]
fn a_head_the_parser_refuses_is_refused_with_its_words() {
    for (head, status, words) in [
        (&b"POST / HTTP/1.1\r\nHost: origin.test\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n"[..], 400, "both Content-Length and Transfer-Encoding"),
        (b"POST / HTTP/1.1\r\nHost: origin.test\r\nTransfer-Encoding: gzip\r\n\r\n", 400, "a Transfer-Encoding that isn't one `chunked`"),
        (b"GET https://origin.test/ HTTP/1.1\r\nHost: origin.test\r\n\r\n", 400, "an absolute-form target inside a tunnel"),
        (b"GET / HTTP/1.1\r\nAccept: */*\r\n\r\n", 400, "an HTTP/1.1 request without a Host"),
    ] {
        let f = fed(&[(0, head)]);
        assert!(matches!(&f.stop, Err(Stop::Refused(s, why)) if *s == status && why.starts_with(words)), "{:?}", f.stop);
        assert!(f.out[1].is_empty());
    }
}

/// What the host sends that no reader can frame one way stops the exchange: an answer nothing asked for and a 101 the
/// run never asked are cut; an answer framed two ways, before any byte of it went, fails (a 502 for the run).
#[test]
fn what_the_host_sends_unasked_or_framed_two_ways_stops_it() {
    assert_eq!(fed(&[(1, OK_2)]).stop, Err(Stop::Cut("it sent an answer nothing asked for".into())));
    let switched: &[u8] = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n";
    assert!(matches!(fed(&[(0, GET_A), (1, switched)]).stop, Err(Stop::Cut(why)) if why.contains("101")));
    let two: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\n";
    let f = fed(&[(0, GET_A), (1, two)]);
    assert_eq!(f.stop, Err(Stop::Failed("its answer: both Content-Length and Transfer-Encoding".into())));
    assert!(f.out[0].is_empty(), "none of it went");
}

/// The connection's last request (`Connection: close`, or HTTP/1.0 without keep-alive): over once it's answered,
/// whatever the run sent behind it dropped and counted. An answer saying `close` ends it too.
#[test]
fn the_last_request_ends_the_exchange_once_answered() {
    let last: &[u8] = b"GET /a HTTP/1.1\r\nHost: origin.test\r\nConnection: close\r\n\r\n";
    let f = fed(&[(0, &[last, GET_B].concat()), (1, OK_2)]);
    assert!(f.out[1] == last && f.x.over() && f.x.owed() == 0);
    assert_eq!(f.x.dropped, GET_B.len());
    let old: &[u8] = b"GET /a HTTP/1.0\r\n\r\n";
    assert!(fed(&[(0, old), (1, OK_2)]).x.over(), "1.0 without keep-alive");
    let kept: &[u8] = b"GET /a HTTP/1.0\r\nConnection: keep-alive\r\n\r\n";
    assert!(!fed(&[(0, kept), (1, OK_2)]).x.over(), "1.0 with keep-alive");
    let closes: &[u8] = b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 2\r\n\r\nok";
    assert!(fed(&[(0, GET_A), (1, closes)]).x.over(), "the answer says close");
}

/// Each request is decided on the tunnel's CONNECT target: a `Host` naming another host, or the same on another port
/// (none: 443), is 421; TRACE and TRACK 405, said as echoes; CONNECT and a method outside plain http's 405; an
/// upgrade 501 — and 1.0 without a Host goes.
#[test]
fn each_request_is_decided_on_the_connect_target() {
    let decided = |head: &str, port: u16| {
        let o = parse::origin(head.as_bytes()).unwrap().unwrap();
        decide(&o, NAME, port).map_err(|(s, why)| format!("{s} {why}"))
    };
    let get = |host: &str| format!("GET / HTTP/1.1\r\nHost: {host}\r\n\r\n");
    assert_eq!(decided(&get("origin.test"), 443), Ok(()));
    assert_eq!(decided(&get("Origin.Test.:443"), 443), Ok(()));
    assert_eq!(decided(&get("origin.test:8443"), 8443), Ok(()));
    assert_eq!(decided("GET / HTTP/1.0\r\n\r\n", 443), Ok(()));
    assert_eq!(decided(&get("other.test"), 443), Err("421 a request's Host names other.test".into()));
    assert_eq!(
        decided(&get("origin.test:8443"), 443),
        Err("421 a request's Host names origin.test:8443".into())
    );
    assert_eq!(decided(&get("origin.test"), 8443), Err("421 a request's Host names origin.test".into()));
    assert_eq!(decided(&get("127.0.0.1"), 443), Err("421 a request's Host names 127.0.0.1".into()));
    for m in ["TRACE", "TRACK"] {
        let why = decided(&format!("{m} / HTTP/1.1\r\nHost: origin.test\r\n\r\n"), 443).unwrap_err();
        assert_eq!(
            why,
            format!("405 a {m} request — refused on a host tarae terminates: it echoes the request back")
        );
    }
    let connect = decided("CONNECT / HTTP/1.1\r\nHost: origin.test\r\n\r\n", 443);
    assert_eq!(connect, Err("405 a CONNECT inside a tunnel".into()));
    let dav = decided("PROPFIND / HTTP/1.1\r\nHost: origin.test\r\n\r\n", 443);
    assert_eq!(dav, Err("405 a method other than GET HEAD POST PUT PATCH DELETE OPTIONS".into()));
    for up in ["Upgrade: websocket\r\nConnection: Upgrade", "Connection: keep-alive, upgrade"] {
        let why = decided(&format!("GET / HTTP/1.1\r\nHost: origin.test\r\n{up}\r\n\r\n"), 443).unwrap_err();
        assert!(why.starts_with("501 an Upgrade — WebSocket"), "{why}");
    }
    for m in parse::METHODS {
        assert_eq!(decided(&format!("{m} / HTTP/1.1\r\nHost: origin.test\r\n\r\n"), 443), Ok(()), "{m}");
    }
}

/// The idle limits by phase: the first head within `head_time` of the start, the next within `keep_alive` of the last
/// byte moved, anything part-way within `stalled` — none while a whole request waits. Past a limit where a head is
/// waited for, bytes the run sent that aren't read yet win (the tie rule); part-way, they don't.
#[test]
fn the_idle_limits_by_phase_and_the_tie_rule() {
    let l = Limits {
        head_time: Duration::from_secs(10),
        keep_alive: Duration::from_secs(6),
        stalled: Duration::from_secs(300),
        ..Limits::default()
    };
    let t0 = crate::clock::now();
    let moved = t0 + Duration::from_secs(20);
    assert_eq!(due(Phase::First, t0, moved, &l), Some(t0 + Duration::from_secs(10)));
    assert_eq!(due(Phase::Idle, t0, moved, &l), Some(moved + Duration::from_secs(6)));
    assert_eq!(due(Phase::UnderWay, t0, moved, &l), Some(moved + Duration::from_secs(300)));
    assert_eq!(due(Phase::Waiting, t0, moved, &l), None);
    let at = moved + Duration::from_secs(6);
    for (phase, unread, closes) in [
        (Phase::Idle, false, true),
        (Phase::Idle, true, false),
        (Phase::First, true, false),
        (Phase::UnderWay, true, true),
    ] {
        assert_eq!(expired(phase, Some(at), at, || unread), closes, "{phase:?}, unread {unread}");
    }
    assert!(!expired(Phase::Idle, Some(at), at - Duration::from_millis(1), || false), "not before");
    assert!(!expired(Phase::Waiting, None, at, || false), "never while a request waits");
    // (the phases as an exchange goes: first, part-way, waiting, at rest)
    let mut x = Exchange::new(NAME, 443, None);
    assert_eq!(x.phase(), Phase::First);
    let mut out = Vec::new();
    x.run_sent(&GET_A[..5], &mut out).unwrap();
    assert_eq!(x.phase(), Phase::First, "a first head part-way: still its own limit");
    x.run_sent(&GET_A[5..], &mut out).unwrap();
    assert_eq!(x.phase(), Phase::Waiting);
    x.host_sent(&OK_2[..3], &mut out).unwrap();
    assert_eq!(x.phase(), Phase::UnderWay);
    x.host_sent(&OK_2[3..], &mut out).unwrap();
    assert_eq!(x.phase(), Phase::Idle);
    x.run_sent(&GET_B[..3], &mut out).unwrap();
    assert_eq!(x.phase(), Phase::UnderWay, "a next head part-way");
}

// --- P2: masking's named edits ---

/// Stand-in values — no provider's form (`concat!` in parts): A's toward `origin.test` (33 bytes, its sentinel as
/// long), B's toward another host, SHORT's under 20 bytes (srt's 47-byte sentinel).
const REAL_A: &str = concat!("tarae-fake-", "token-0123456789abcdef");
const SENT_A: &str = "fake_value_0123456789abcdefghijkl";
const REAL_B: &str = concat!("tarae-fake-", "other-0123456789");
const SENT_B: &str = "fake_value_bbbbbbbbbbbbbbbb";
const REAL_S: &str = "short-v8x";
const SENT_S: &str = "fake_value_00000000-1111-4222-8333-444444444444";

fn masked() -> Masked {
    Masked::of(&[
        ("A_TOKEN", SENT_A, REAL_A, &[NAME]),
        ("B_KEY", SENT_B, REAL_B, &["other.test"]),
        ("SHORT", SENT_S, REAL_S, &[NAME]),
    ])
}

/// P1b's property with masking on: the bytes out each way are the bytes in but for the named edits, at any split —
/// A's sentinel swapped in the target, a field and a chunked body (split across its chunks: the framing as it came,
/// the body as long), `Accept-Encoding` one `identity` (added where none was), `TE` dropped; the answer's real value
/// back to its sentinel, head and body (its length kept). The expected bytes are the input with those edits made.
#[test]
fn masking_changes_only_its_named_edits_at_any_split() {
    let head = |s: &str, accept: &str, te: &str| {
        format!(
            "POST /up?t={s} HTTP/1.1\r\nHost: origin.test\r\nAuthorization: Bearer {s}\r\n{accept}{te}\
             Transfer-Encoding: chunked\r\n\r\n"
        )
    };
    let body = |s: &str| {
        let p = format!("{{\"t\":\"{s}\"}}");
        format!("14\r\n{}\r\n15\r\n{}\r\n0\r\n\r\n", &p[..20], &p[20..])
    };
    let answer = |v: &str| format!("HTTP/1.1 200 OK\r\nX-Echo: {v}\r\nContent-Length: 39\r\n\r\necho {v}!");
    let up = head(SENT_A, "Accept-Encoding: gzip\r\n", "TE: trailers\r\n") + &body(SENT_A);
    let back = answer(REAL_A);
    let script: Script = &[(0, up.as_bytes()), (1, back.as_bytes()), (0, GET_A), (1, OK_2)];
    let get_a = String::from_utf8_lossy(GET_A).replace("\r\n\r\n", "\r\nAccept-Encoding: identity\r\n\r\n");
    let to_host = head(REAL_A, "Accept-Encoding: identity\r\n", "") + &body(REAL_A) + &get_a;
    let to_run = answer(SENT_A) + &String::from_utf8_lossy(OK_2);
    let m = masked();
    let whole = at_any_split_in(script, Some(&m), |whole, f, at| {
        assert_eq!(f.stop, Ok(()), "{at}");
        assert_eq!(String::from_utf8_lossy(&f.out[1]), to_host, "{at}: to the host");
        assert_eq!(String::from_utf8_lossy(&f.out[0]), to_run, "{at}: to the run");
        let state = |f: &Fed| (f.x.owed(), f.x.phase(), f.x.over());
        assert_eq!(state(f), state(whole), "{at}");
    });
    assert_eq!((whole.x.owed(), whole.x.phase()), (0, Phase::Idle));
    // (no masking: every byte as it came — P1b's own)
    let f = fed(script);
    assert!(f.out[1] == [up.as_bytes(), GET_A].concat() && f.out[0] == [back.as_bytes(), OK_2].concat());
}

/// An answer streamed event by event (server-sent events) reaches the run event by event: nothing held for the next
/// one, which may be seconds away — values look random, so what no value starts with goes at once.
#[test]
fn an_answer_streamed_event_by_event_arrives_event_by_event() {
    let m = masked();
    let mut x = Exchange::new(NAME, 443, Some(m.toward(NAME, 443)));
    let mut out = Vec::new();
    x.run_sent(GET_A, &mut out).unwrap();
    let head = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";
    let mut to_run = Vec::new();
    x.host_sent(head, &mut to_run).unwrap();
    assert_eq!(to_run, head);
    for k in 0..10 {
        let event = format!("data: {{\"delta\":\"part {k}\"}}\n\n");
        let chunk = format!("{:x}\r\n{event}\r\n", event.len());
        let mut to_run = Vec::new();
        x.host_sent(chunk.as_bytes(), &mut to_run).unwrap();
        assert_eq!(String::from_utf8_lossy(&to_run), chunk, "event {k} reached the run whole, at once");
    }
}

/// A request body with `Content-Encoding` isn't searched: A's sentinel in it goes as its fake, said. An answer with one
/// passes unscrubbed, said — each once a tunnel.
#[test]
fn an_encoded_body_goes_as_it_came_and_is_said() {
    let m = masked();
    let up = format!(
        "POST / HTTP/1.1\r\nHost: origin.test\r\nContent-Encoding: gzip\r\nContent-Length: 33\r\n\r\n{SENT_A}"
    );
    let back = format!("HTTP/1.1 200 OK\r\nContent-Encoding: br\r\nContent-Length: 33\r\n\r\n{REAL_A}");
    let mut f = fed_in(&[(0, up.as_bytes()), (1, back.as_bytes())], 443, Some(&m), |_, _| Vec::new());
    assert_eq!(f.stop, Ok(()));
    assert!(String::from_utf8_lossy(&f.out[1]).ends_with(&format!("\r\n\r\n{SENT_A}")), "the fake went");
    assert!(String::from_utf8_lossy(&f.out[0]).ends_with(&format!("\r\n\r\n{REAL_A}")), "passed as it came");
    assert_eq!(
        f.x.said(),
        [
            "a request body with Content-Encoding: gzip isn't searched — a masked value in it went as its fake",
            "an answer with Content-Encoding: br passed unscrubbed — a real value in its body would reach the run",
        ]
    );
}

/// D5 in a head: B's sentinel toward A's host is refused 403 with its name, and nothing of the request goes on.
/// Part-way through a body: what went before it went, none of it, and the tunnel is cut — at any split.
#[test]
fn another_secrets_sentinel_is_refused_in_a_head_and_cuts_a_body() {
    let m = masked();
    let why = "a run sent B_KEY's masked value toward origin.test:443 — it goes only to B_KEY's own hosts";
    let head = format!("GET / HTTP/1.1\r\nHost: origin.test\r\nX-Key: {SENT_B}\r\n\r\n");
    let f = fed_in(&[(0, head.as_bytes())], 443, Some(&m), |_, _| Vec::new());
    assert_eq!(f.stop, Err(Stop::Masked { why: why.into(), cut: false }));
    assert!(f.out[1].is_empty());
    let up = format!(
        "POST / HTTP/1.1\r\nHost: origin.test\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n1d\r\nb={SENT_B}\r\n0\r\n\r\n"
    );
    at_any_split_in(&[(0, up.as_bytes())], Some(&m), |_, f, at| {
        assert_eq!(f.stop, Err(Stop::Masked { why: why.into(), cut: true }), "{at}");
        let went = String::from_utf8_lossy(&f.out[1]);
        assert!(
            went.contains("Accept-Encoding: identity\r\n\r\n") && !went.contains("fake_value_"),
            "{at}: {went}"
        );
    });
}

/// U1: a value under 20 bytes is swapped in a head — its 47-byte sentinel, a field's value as long as it needs — and
/// left in a body, said by name.
#[test]
fn a_short_values_sentinel_is_swapped_in_a_head_and_left_in_a_body() {
    let m = masked();
    let up = format!(
        "POST / HTTP/1.1\r\nHost: origin.test\r\nX-S: {SENT_S}\r\nContent-Length: 47\r\n\r\n{SENT_S}"
    );
    let mut f = fed_in(&[(0, up.as_bytes())], 443, Some(&m), |_, _| Vec::new());
    let went = String::from_utf8_lossy(&f.out[1]).into_owned();
    assert!(
        went.contains(&format!("X-S: {REAL_S}\r\n")) && went.ends_with(&format!("\r\n\r\n{SENT_S}")),
        "{went}"
    );
    assert_eq!(
        f.x.said(),
        [
            "SHORT's masked value in a request body went as its fake: the value is under 20 bytes, and a body's length \
          can't change"
        ]
    );
}

/// A body held at what may be a value's start: held only that far, and a pause lets it go as it is — the stage's
/// `release_run`, after `Limits::hold_pause` in the loop (`tests/terminate/masked.rs` on loopback).
#[test]
fn a_held_request_body_goes_on_a_pause() {
    let m = masked();
    let mut x = Exchange::new(NAME, 443, Some(m.toward(NAME, 443)));
    let mut out = Vec::new();
    let head = "POST / HTTP/1.1\r\nHost: origin.test\r\nContent-Length: 40\r\n\r\n";
    x.run_sent(format!("{head}stream fake_val").as_bytes(), &mut out).unwrap();
    assert!(String::from_utf8_lossy(&out).ends_with("\r\n\r\nstream "), "held from the value's start");
    assert!(x.run_holds() && x.phase() == Phase::UnderWay);
    x.release_run(&mut out).unwrap();
    assert!(String::from_utf8_lossy(&out).ends_with("stream fake_val") && !x.run_holds());
}

/// P2b: a card's mask in the target or a field but `Authorization` goes as its fake, and the head's notes
/// (`Head.left`) reach the report — each once a tunnel, at any split; `Authorization`'s swapped.
#[test]
fn a_cards_mask_left_in_a_head_reaches_the_report() {
    let m = Masked::of(&[("A_TOKEN", SENT_A, REAL_A, &[NAME])]).card_marked(&["A_TOKEN"]);
    let up = format!(
        "GET /gists?t={SENT_A} HTTP/1.1\r\nHost: origin.test\r\nAuthorization: Bearer {SENT_A}\r\n\
         X-Token: {SENT_A}\r\n\r\n"
    );
    let left = |within: &str| {
        format!(
            "A_TOKEN's masked value in {within} went as its fake: a mask from its card goes only in the \
             Authorization header (a value named in llm.sandbox-secrets goes everywhere toward its hosts)"
        )
    };
    let said = [left("the request's target"), left("a header field but Authorization")];
    let script: Script = &[(0, up.as_bytes()), (1, OK_2), (0, up.as_bytes()), (1, OK_2)];
    let mut whole = at_any_split_in(script, Some(&m), |_, f, at| {
        assert_eq!(f.stop, Ok(()), "{at}");
        let went = String::from_utf8_lossy(&f.out[1]);
        assert!(
            went.contains(&format!("t={SENT_A} ")) && went.contains(&format!("X-Token: {SENT_A}\r\n")),
            "{at}: {went}"
        );
        assert!(went.contains(&format!("Bearer {REAL_A}\r\n")), "{at}: {went}");
        assert_eq!(f.x.notes, said, "{at}");
    });
    assert_eq!(whole.x.said(), said);
    assert_eq!(whole.x.said(), Vec::<String>::new(), "said once");
}
