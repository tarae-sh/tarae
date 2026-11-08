//! P2 on loopback: a masked value through the proxy's real paths — swapped toward its host only, the origin seeing
//! the real value, an echo of it scrubbed back to the sentinel; another secret's sentinel refused with its name and
//! nothing reaching the origin (laundering); a sentinel in plain http, or in the host a CONNECT asks for, refused
//! before anything is reached; through a company's intercepting proxy with credentials, the swap still made and those
//! credentials nowhere a run or the origin sees. No value in what the report says.

use super::*;
use crate::sandbox::proxy::masked::Masked;

/// Stand-in values — no provider's form (`concat!` in parts): A's toward `NAME`, its sentinel as long; B's elsewhere.
const REAL_A: &str = concat!("tarae-fake-", "token-0123456789abcdef");
const SENT_A: &str = "fake_value_0123456789abcdefghijkl";
const REAL_B: &str = concat!("tarae-fake-", "other-0123456789");
const SENT_B: &str = "fake_value_bbbbbbbbbbbbbbbb";

fn masked(a_to: &str) -> Arc<Masked> {
    Arc::new(Masked::of(&[("A_TOKEN", SENT_A, REAL_A, &[a_to]), ("B_KEY", SENT_B, REAL_B, &["other.test"])]))
}

/// An origin on loopback under a leaf its public CA issued for `NAME`, echoing each request whole as its answer's body
/// — its `X-Key` field as `X-Echo` — and sending it on `got` as it came; an empty one when a connection ends.
fn echoing() -> (SocketAddr, Authority, Receiver<Vec<u8>>) {
    let public = authority("tarae test public CA");
    let config = issued(&public, &[NAME]);
    let l = TcpListener::bind((LO, 0)).unwrap();
    let at = l.local_addr().unwrap();
    let (tx, got) = mpsc::channel();
    crate::event::thread("tarae-p2-test", move || {
        for s in l.incoming() {
            let (Ok(s), config, tx) = (s, config.clone(), tx.clone()) else { break };
            crate::event::thread("tarae-p2-test", move || {
                let _ = s.set_read_timeout(Some(WAIT));
                let mut t = rustls::StreamOwned::new(rustls::ServerConnection::new(config).unwrap(), s);
                loop {
                    let mut request = read_head(&mut t);
                    if !request.ends_with(b"\r\n\r\n") || !read_body(&mut t, &mut request) {
                        let _ = tx.send(Vec::new());
                        return;
                    }
                    let _ = tx.send(request.clone());
                    let text = String::from_utf8_lossy(&request).into_owned();
                    let key = text.split("\r\n").find_map(|l| l.strip_prefix("X-Key: ")).unwrap_or("-");
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nX-Echo: {key}\r\nContent-Length: {}\r\n\r\n",
                        request.len()
                    );
                    if t.write_all(&[head.as_bytes(), &request].concat()).and_then(|()| t.flush()).is_err() {
                        return;
                    }
                }
            });
        }
    });
    (at, public, got)
}

/// What the origin got on one connection, to its end.
fn requests(got: &Receiver<Vec<u8>>) -> Vec<String> {
    let mut all = Vec::new();
    loop {
        match got.recv_timeout(WAIT).expect("the origin's connection ended") {
            r if r.is_empty() => return all,
            r => all.push(String::from_utf8(r).unwrap()),
        }
    }
}

/// A masked value toward its own host: the origin gets the real value in the path and the query (as it is: its value
/// is unreserved), a `Basic` credential (decoded, swapped, encoded again), a field and the body (its length kept),
/// and `Accept-Encoding: identity` in place of the run's; the echo comes back with every real value its sentinel again
/// — the field's, the body's (a re-encoded one, the `Basic` credential's base64, is the case left: threat model
/// §1.5). Nothing is said: nothing was refused.
#[test]
fn a_masked_value_is_swapped_toward_its_host_and_scrubbed_back() {
    let (at, public, got) = echoing();
    let session = SESSION + 40;
    let (f, rules, ca) = terminating_to(at, &public, session, limits());
    let rules = Egress { masked: Some(masked(NAME)), ..rules };
    let (mut t, d) = tunnel_tls(&f, session, 1, &rules, &ca);
    let ask = |v: &str, accept: &str| {
        let basic = parse::base64(format!("x-access-token:{v}").as_bytes());
        let body = format!("{{\"token\":\"{v}\"}}");
        format!(
            "POST /repos/{v}/x?token={v} HTTP/1.1\r\nHost: {NAME}\r\nAuthorization: Basic {basic}\r\nX-Key: {v}\r\n\
             Accept-Encoding: {accept}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    };
    t.write_all(ask(SENT_A, "gzip, br").as_bytes()).unwrap();
    let (head, echoed) = answer_of(&mut t);
    let came = ask(REAL_A, "identity");
    assert_eq!(requests(&got), std::slice::from_ref(&came), "the origin got the real value, and identity");
    assert!(head.starts_with("HTTP/1.1 200 ") && head.contains(&format!("X-Echo: {SENT_A}\r\n")), "{head}");
    assert_eq!(
        String::from_utf8(echoed).unwrap(),
        came.replace(REAL_A, SENT_A),
        "the echo scrubbed, as long"
    );
    assert!(closed(&mut t));
    assert!(d.recv_timeout(Duration::from_millis(100)).is_err(), "nothing said");
    terminate::forget(session);
}

/// Laundering (D5, srt's case left as the fake there): B's sentinel sent to A's host is refused 403 inside the run's
/// TLS, said with B's name as an attempt (`MASKED_OP`) — and nothing of the request reaches the origin.
#[test]
fn another_secrets_sentinel_toward_a_host_is_refused_with_its_name() {
    let (at, public, got) = echoing();
    let session = SESSION + 41;
    let (f, rules, ca) = terminating_to(at, &public, session, limits());
    let rules = Egress { masked: Some(masked(NAME)), ..rules };
    let (mut t, d) = tunnel_tls(&f, session, 1, &rules, &ca);
    t.write_all(format!("GET / HTTP/1.1\r\nHost: {NAME}\r\nX-Key: {SENT_B}\r\n\r\n").as_bytes()).unwrap();
    let (head, body) = answer_of(&mut t);
    let why = "a run sent B_KEY's masked value toward origin.test:443 — it goes only to B_KEY's own hosts";
    assert!(head.starts_with("HTTP/1.1 403 Forbidden\r\n"), "{head}");
    assert_eq!(String::from_utf8(body).unwrap(), format!("tarae's proxy: origin.test:443: {why}\n"));
    assert!(closed(&mut t));
    let r = d.recv_timeout(WAIT).unwrap();
    assert_eq!(
        (r.op.as_str(), r.denial.target.as_str()),
        (MASKED_OP, format!("origin.test:443: {why} — refused 403").as_str())
    );
    assert_eq!(r.denial.plain(), format!("{why} — refused 403"));
    assert!(requests(&got).is_empty(), "nothing reached the origin");
    terminate::forget(session);
}

/// A sentinel where tarae reads plaintext outside a terminated tunnel — a plain request's target or field, the host a
/// CONNECT asks for — is refused 403 before anything is reached, said with its name; in a plain request's body, the
/// connection closes there, said. No masked value goes in clear.
#[test]
fn a_sentinel_in_plain_http_or_a_connects_host_is_refused() {
    let out = outside(web);
    let (resolve, _) = resolver(vec![("web.test", vec![vec![out.at]])]);
    let f = start(resolve, limits());
    let named = format!("{SENT_A}.test");
    let rules = Egress { masked: Some(masked(NAME)), ..rules(&["web.test", &named]) };
    for (k, (head, name, how)) in [
        (
            format!("GET http://web.test/x?t={SENT_A} HTTP/1.1\r\nHost: web.test\r\n\r\n"),
            "A_TOKEN",
            "in plain http",
        ),
        (
            format!(
                "GET http://web.test/x HTTP/1.1\r\nHost: web.test\r\nAuthorization: Bearer {SENT_B}\r\n\r\n"
            ),
            "B_KEY",
            "in plain http",
        ),
        (
            format!("CONNECT {named}:443 HTTP/1.1\r\nHost: {named}:443\r\n\r\n"),
            "A_TOKEN",
            "in the host it asked for",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let (mut c, d) = connection(&f.proxy, 1, 1 + k as u64, rules.clone());
        assert_eq!(ask(&mut c, &head), 403, "{head}");
        let r = d.recv_timeout(WAIT).unwrap();
        assert_eq!(r.op, MASKED_OP);
        assert!(
            r.denial.target.contains(&format!("a run sent {name}'s masked value")),
            "{}",
            r.denial.target
        );
        assert!(r.denial.target.contains(how), "{}", r.denial.target);
    }
    assert_eq!(out.accepted.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing reached");
    let (mut c, d) = connection(&f.proxy, 1, 9, rules.clone());
    let body = format!("x={SENT_A}");
    let post = format!(
        "POST http://web.test/f HTTP/1.1\r\nHost: web.test\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    c.write_all(post.as_bytes()).unwrap();
    let r = d.recv_timeout(WAIT).unwrap();
    assert_eq!(r.op, MASKED_OP);
    assert!(
        r.denial.target.ends_with("in plain http — found part-way through its body: the connection closed"),
        "{}",
        r.denial.target
    );
}

/// An origin on loopback under a leaf its public CA issued for `NAME`: each read of a request on `got` as it comes, with
/// when; its body whole by its Content-Length, a 200 and close_notify.
fn timing() -> (SocketAddr, Authority, Receiver<(Instant, Vec<u8>)>) {
    let public = authority("tarae test public CA");
    let config = issued(&public, &[NAME]);
    let l = TcpListener::bind((LO, 0)).unwrap();
    let at = l.local_addr().unwrap();
    let (tx, got) = mpsc::channel();
    crate::event::thread("tarae-p2-test", move || {
        let Some(Ok(s)) = l.incoming().next() else { return };
        let _ = s.set_read_timeout(Some(WAIT));
        let mut t = rustls::StreamOwned::new(rustls::ServerConnection::new(config).unwrap(), s);
        let (mut all, mut b) = (Vec::new(), [0u8; 4096]);
        loop {
            let n = match t.read(&mut b) {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            all.extend_from_slice(&b[..n]);
            let _ = tx.send((Instant::now(), b[..n].to_vec()));
            let text = String::from_utf8_lossy(&all).to_ascii_lowercase();
            let whole = text.find("\r\n\r\n").is_some_and(|end| {
                let length = text[..end]
                    .split("\r\n")
                    .find_map(|l| l.strip_prefix("content-length:")?.trim().parse().ok());
                all.len() >= end + 4 + length.unwrap_or(0)
            });
            if whole {
                let _ = t.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                t.conn.send_close_notify();
                let _ = t.flush();
                return;
            }
        }
    });
    (at, public, got)
}

/// i through the stage's loop: a request body ending in the start of the run's own sentinel holds that start back;
/// with nothing more from the run within `Limits::hold_pause`, it goes as it is, and the sentinel's rest after it — the
/// fake went, said. The origin gets the held bytes before the run sends anything more (waited for), and never sooner
/// than the pause past the run's write (a lower bound, never a deadline).
#[test]
fn a_held_request_body_goes_on_a_pause_through_the_stage() {
    let (at, public, got) = timing();
    let session = SESSION + 43;
    let pause = Duration::from_millis(300);
    let (f, rules, ca) = terminating_to(at, &public, session, Limits { hold_pause: pause, ..limits() });
    let rules = Egress { masked: Some(masked(NAME)), ..rules };
    let (mut t, d) = tunnel_tls(&f, session, 1, &rules, &ca);
    let (start, rest) = (&SENT_A[..8], &format!("{} end", &SENT_A[8..]));
    let body = format!("stream {start}{rest}");
    let head = format!("POST /s HTTP/1.1\r\nHost: {NAME}\r\nContent-Length: {}\r\n\r\n", body.len());
    let sent = Instant::now();
    t.write_all(format!("{head}stream {start}").as_bytes()).unwrap();
    // (what the origin got, read by read until it ends with `upto`: when its last read came)
    let mut came = Vec::new();
    let mut until = |upto: &str, what: &str| loop {
        let (when, chunk) =
            got.recv_timeout(WAIT).unwrap_or_else(|_| panic!("{what}: {:?}", String::from_utf8_lossy(&came)));
        came.extend_from_slice(&chunk);
        if came.ends_with(upto.as_bytes()) {
            break when;
        }
    };
    let held = until(start, "the held start went on its pause");
    assert!(held >= sent + pause, "not before the pause: {:?}", held - sent);
    t.write_all(rest.as_bytes()).unwrap();
    until(rest, "the rest went after it");
    let at = came.windows(4).position(|w| w == b"\r\n\r\n").expect("a head") + 4;
    assert_eq!(
        String::from_utf8_lossy(&came[at..]),
        body,
        "the body as the run sent it, its fake whole, in order"
    );
    let (answer, _) = answer_of(&mut t);
    assert!(answer.starts_with("HTTP/1.1 200 "), "{answer}");
    let r = d.recv_timeout(WAIT).unwrap();
    assert_eq!(
        (r.op.as_str(), r.denial.target.as_str()),
        (
            MASKED_OP,
            "origin.test:443: A_TOKEN's masked value in a request body went as its fake: the body paused part-way \
             through it"
        )
    );
    terminate::forget(session);
}

/// Behind a company's intercepting proxy that tarae reaches with credentials (its `HTTPS_PROXY` naming a user and a
/// password): the swap is still made — the origin behind the company gets the real value — and those credentials are
/// nowhere the run or the origin sees.
#[test]
fn through_an_authenticating_company_proxy_the_swap_is_made_and_its_credentials_stay_taraes() {
    let company = Company::start();
    let scratch = crate::scratch::Scratch::new("p2-company");
    let root = super::super::super::mitm_tests::root_file(&company, &scratch);
    let f = start(resolver(Vec::new()).0, limits());
    let session = SESSION + 42;
    let ca = terminate::of_session(session).unwrap();
    let secret = concat!("corp-", "pass-7f3a");
    let url = format!("http://corp-user:{secret}@{}", company.at);
    let env: HashMap<String, std::ffi::OsString> = [("HTTPS_PROXY".to_string(), url.into())].into();
    let mut t = Terminate::new(vec![Pattern::parse(ORIGIN).unwrap()], ca.clone(), vec![root]);
    t.system = Some(vec![authority("unrelated").der]);
    let rules = Egress {
        terminate: Some(t),
        upstream: up::of(&env, Setting::Env),
        masked: Some(masked(ORIGIN)),
        ..rules(&[ORIGIN])
    };
    let (c, _) = connection(&f.proxy, session, 1, rules);
    let c = opened(c, &connect_head(&format!("{ORIGIN}:443")));
    let server = rustls::pki_types::ServerName::try_from(ORIGIN).unwrap();
    let mut tls =
        rustls::StreamOwned::new(rustls::ClientConnection::new(trusting(&[ca.der()]), server).unwrap(), c);
    let ask = format!(
        "GET /x?t={SENT_A} HTTP/1.1\r\nHost: {ORIGIN}\r\nX-Key: {SENT_A}\r\nConnection: close\r\n\r\n"
    );
    tls.write_all(ask.as_bytes()).unwrap();
    let mut answer = Vec::new();
    tls.read_to_end(&mut answer).unwrap();
    let answer = String::from_utf8_lossy(&answer).into_owned();
    assert!(answer.ends_with(ANSWER), "{answer}");
    let heads: Vec<String> =
        company.heads.lock().unwrap().iter().map(|h| String::from_utf8_lossy(h).into_owned()).collect();
    assert_eq!(heads.len(), 1, "{heads:?}");
    assert!(
        heads[0].contains(&format!("?t={REAL_A} ")) && heads[0].contains(&format!("X-Key: {REAL_A}\r\n")),
        "{heads:?}"
    );
    let encoded = parse::base64(format!("corp-user:{secret}").as_bytes());
    for seen in [&answer, &heads[0]] {
        assert!(!seen.contains(secret) && !seen.contains(&encoded) && !seen.contains("Proxy-"), "{seen}");
    }
    terminate::forget(session);
}
