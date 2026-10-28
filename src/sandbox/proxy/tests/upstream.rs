//! The proxy chaining to the one tarae itself goes out by (step 5g): a stand-in upstream on loopback, named as
//! tarae's environment would name it, records every byte it gets and answers as the test says. Hosts are
//! `*.example.invalid`, credentials fake ones; nothing leaves this machine.

use std::path::Path;

use super::*;
use crate::sandbox::proxy::upstream::{self as up, Setting, Upstreams};

/// A head, read to its blank line.
fn read_head(s: &mut impl Read) -> Vec<u8> {
    let (mut got, mut b) = (Vec::new(), [0u8; 1]);
    while !got.ends_with(b"\r\n\r\n") {
        match s.read(&mut b) {
            Ok(1) => got.push(b[0]),
            _ => break,
        }
    }
    got
}

/// An upstream that opens every tunnel: its CONNECT read, 200, then what comes echoed — all it got.
fn opens(mut s: TcpStream) -> Vec<u8> {
    let mut got = read_head(&mut s);
    let _ = s.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
    got.extend(echo(s));
    got
}

/// Answers `answer` to the first head, then reads to the end: all it got.
fn answering(mut s: TcpStream, answer: &[u8]) -> Vec<u8> {
    let mut got = read_head(&mut s);
    let _ = s.write_all(answer);
    let mut rest = Vec::new();
    let _ = s.read_to_end(&mut rest);
    got.extend(rest);
    got
}

/// Words of the upstream's own, which never reach a run or its report.
const ITS_WORDS: &str = "upstream-words-never-passed";

fn asks_basic(s: TcpStream) -> Vec<u8> {
    answering(
        s,
        b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"corp\"\r\n\
          Content-Length: 28\r\n\r\nupstream-words-never-passed\n",
    )
}

fn asks_negotiate(s: TcpStream) -> Vec<u8> {
    answering(
        s,
        b"HTTP/1.1 407 upstream-words-never-passed\r\nProxy-Authenticate: Negotiate\r\nProxy-Authenticate: NTLM\r\n\
          Content-Length: 0\r\n\r\n",
    )
}

fn forbids(s: TcpStream) -> Vec<u8> {
    answering(s, b"HTTP/1.1 403 upstream-words-never-passed\r\nContent-Length: 0\r\n\r\n")
}

/// Takes the request and never answers.
fn silent(mut s: TcpStream) -> Vec<u8> {
    let mut got = read_head(&mut s);
    let mut rest = Vec::new();
    let _ = s.read_to_end(&mut rest);
    got.extend(rest);
    got
}

/// A loopback address that refuses every connection while what it returns lives: the local port of a connection
/// of ours — nothing listens there, and on Linux the client's socket holds the port (no `SO_REUSEADDR`), so no other
/// bind takes it meanwhile. A port bound and let go at once was closed only until the next listener got it — under
/// a full Linux run another test's `forbids` did, and "connection refused" read as its 403. (macOS lets a listener's
/// `SO_REUSEADDR` bind take a connected socket's port, and drops a connection to one bound and never listening
/// instead of refusing it: there the port is as free as a dropped one's.)
fn refused_at() -> ((TcpStream, TcpStream), SocketAddr) {
    let l = TcpListener::bind((LO, 0)).unwrap();
    let client = TcpStream::connect(l.local_addr().unwrap()).unwrap();
    let (server, _) = l.accept().unwrap();
    let at = client.local_addr().unwrap();
    ((client, server), at)
}

/// `refused_at`'s address refuses a connection, and on Linux no listener's bind (`SO_REUSEADDR`, as std's) takes
/// its port while it's held.
#[test]
fn a_refusing_port_is_taken_by_no_other_bind() {
    let (_held, closed) = refused_at();
    let refused = TcpStream::connect(closed).map(|_| ());
    assert_eq!(refused.map_err(|e| e.kind()), Err(std::io::ErrorKind::ConnectionRefused), "{closed}");
    #[cfg(target_os = "linux")]
    {
        let again = TcpListener::bind(closed).map(|_| ());
        assert_eq!(again.map_err(|e| e.kind()), Err(std::io::ErrorKind::AddrInUse), "{closed}");
    }
}

/// `%XX` for every byte but the URL's unreserved ones — as a proxy URL holds credentials.
fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => char::from(b).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn env(vars: &[(&str, String)]) -> HashMap<String, std::ffi::OsString> {
    vars.iter().map(|(k, v)| (k.to_string(), v.into())).collect()
}

/// tarae's environment naming the upstream at `at` for both kinds (with these credentials).
fn via(at: SocketAddr, creds: Option<(&str, &str)>) -> Upstreams {
    let creds = creds.map(|(u, p)| format!("{}:{}@", pct(u), pct(p))).unwrap_or_default();
    let url = format!("http://{creds}{at}");
    up::of(&env(&[("HTTPS_PROXY", url.clone()), ("HTTP_PROXY", url)]), Setting::Env)
}

fn rules_via(hosts: &[&str], upstream: Upstreams) -> Egress {
    Egress { upstream, ..rules(hosts) }
}

/// `o` took `n` connections — waited for: its own thread counts them.
fn took(o: &Outside, n: usize) -> bool {
    let by = Instant::now() + WAIT;
    while o.accepted.load(Ordering::SeqCst) < n && Instant::now() < by {
        std::thread::sleep(Duration::from_millis(5));
    }
    o.accepted.load(Ordering::SeqCst) == n
}

/// Through an upstream, an allowed host's tunnel is asked of it — `CONNECT host:port`, tarae's credentials at
/// once — and the run's ClientHello follows after the upstream's 200, checked as ever. The run's name is never
/// looked up by tarae: the upstream does that, which is why tarae's address checks can't hold there. A host the
/// rules don't let go never reaches the upstream; a ClientHello naming another server never goes past the CONNECT.
#[test]
fn an_upstream_proxy_is_chained() {
    let upstream = outside(opens);
    let (resolve, lookups) = resolver(Vec::new());
    let f = start(resolve, Limits { hold: Duration::from_millis(300), ..limits() });
    let rules = || rules_via(&["example.invalid"], via(upstream.at, Some(("u", "tarae-fake-token-1"))));
    let (c, _) = tunnel(&f.proxy, 1, 1, rules(), "example.invalid:443");
    drop(c);
    let basic = parse::base64(b"u:tarae-fake-token-1");
    let head = format!(
        "CONNECT example.invalid:443 HTTP/1.1\r\nHost: example.invalid:443\r\nProxy-Authorization: Basic {basic}\r\n\r\n"
    );
    let got = upstream.got.recv_timeout(WAIT).unwrap();
    assert_eq!(got, [head.as_bytes(), curl_hello()].concat(), "{}", String::from_utf8_lossy(&got));
    assert_eq!(lookups.load(Ordering::SeqCst), 0, "the run's name is the upstream's to look up");
    // A host the rules don't name: asked, unanswered, refused — the upstream never hears of it
    let (mut c, d) = connection(&f.proxy, 1, 1, rules());
    assert_eq!(ask(&mut c, &connect_head("refused.example.invalid:443")), 403);
    assert!(said_host(&d).contains("didn't answer"));
    // A ClientHello naming another server: the tunnel was asked, the hello never goes on
    let (mut c, d) = connection(&f.proxy, 1, 1, rules());
    assert_eq!(ask(&mut c, &connect_head("example.invalid:443")), 200);
    c.write_all(cargo_hello()).unwrap();
    assert_eq!(to_close(&mut c), b"");
    assert!(said(&d).contains("names index.crates.io"));
    assert_eq!(upstream.got.recv_timeout(WAIT).unwrap(), head.as_bytes(), "the CONNECT, nothing after");
    assert!(took(&upstream, 2), "the refused host never reached it");
}

/// A plain request goes to the upstream in absolute form — `GET http://host/path` as proxies take it — with
/// tarae's credentials, the run's own `Proxy-*` gone, `Connection: close`, its body to its end; its name never
/// looked up by tarae.
#[test]
fn plain_http_goes_upstream_in_absolute_form() {
    let upstream = outside(web);
    let (resolve, lookups) = resolver(Vec::new());
    let f = start(resolve, limits());
    let hosts = ["web.example.invalid", "web.example.invalid:8080"];
    let creds = Some(("u", "tarae-fake-token-2"));
    let basic = parse::base64(b"u:tarae-fake-token-2");
    let go = |head: &str| {
        let (mut c, _) = connection(&f.proxy, 1, 1, rules_via(&hosts, via(upstream.at, creds)));
        let s = ask(&mut c, head);
        (s, String::from_utf8(upstream.got.recv_timeout(WAIT).unwrap()).unwrap())
    };
    let (s, got) = go("GET http://web.example.invalid/x?y=1 HTTP/1.1\r\nHost: web.example.invalid\r\n\
         Proxy-Authorization: Basic cnVuOng=\r\n\r\n");
    assert_eq!(s, 200);
    assert_eq!(
        got,
        format!(
            "GET http://web.example.invalid/x?y=1 HTTP/1.1\r\nHost: web.example.invalid\r\nConnection: close\r\n\
             Proxy-Authorization: Basic {basic}\r\n\r\n"
        )
    );
    let (s, got) = go("POST http://web.example.invalid:8080/f HTTP/1.1\r\nContent-Length: 3\r\n\r\nx=1");
    assert_eq!(s, 200);
    assert!(got.starts_with(
        "POST http://web.example.invalid:8080/f HTTP/1.1\r\nHost: web.example.invalid:8080\r\n"
    ));
    assert!(got.ends_with(&format!("Proxy-Authorization: Basic {basic}\r\n\r\nx=1")), "{got}");
    assert_eq!(lookups.load(Ordering::SeqCst), 0);
}

/// What failed at the upstream is said as the upstream's (`network-upstream`), apart from tarae's own refusals:
/// unreachable, silent, its 407 three ways (credentials refused, none given, a scheme tarae doesn't speak), its
/// own refusal, a SOCKS4 one tarae can't chain to, a plain request's 407. The run gets 502 — never the
/// upstream's challenge (it would answer it with its own token) nor its words; the report names the variable,
/// never where the upstream is; `:sandbox-blocked` offers nothing, as no answer of the user's would lift it.
#[test]
fn the_upstreams_failures_are_said_as_its_own() {
    let (_held, closed) = refused_at();
    let (basic, negotiate, refusing, quiet) =
        (outside(asks_basic), outside(asks_negotiate), outside(forbids), outside(silent));
    let f = start(resolver(Vec::new()).0, Limits { connect: Duration::from_millis(400), ..limits() });
    let creds = Some(("u", "tarae-fake-token-3"));
    let socks4 = format!("socks4a://u:tarae-fake-token-3@{}", basic.at);
    let get = "GET http://example.invalid/ HTTP/1.1\r\n\r\n".to_string();
    let connect = connect_head("example.invalid:443");
    let cases: Vec<(Upstreams, &String, &str)> = vec![
        (via(closed, creds), &connect, "it can't be reached (connection refused)"),
        (via(quiet.at, creds), &connect, "it didn't answer within 0.4 s"),
        (via(basic.at, creds), &connect, "it refused the credentials in tarae's HTTPS_PROXY (407)"),
        (via(basic.at, None), &connect, "it asks for credentials (407), and tarae's HTTPS_PROXY gives none"),
        (
            via(negotiate.at, creds),
            &connect,
            "it asks for Negotiate or NTLM (407), which tarae doesn't speak",
        ),
        (via(refusing.at, creds), &connect, "it answered 403 (Forbidden — its own refusal)"),
        (
            up::of(&env(&[("HTTPS_PROXY", socks4)]), Setting::Env),
            &connect,
            "tarae's own HTTPS_PROXY names a SOCKS proxy other than SOCKS5",
        ),
        (via(basic.at, creds), &get, "it refused the credentials in tarae's HTTP_PROXY (407)"),
    ];
    for (upstream, request, why) in cases {
        let (mut c, d) = connection(&f.proxy, 1, 1, rules_via(&["example.invalid"], upstream));
        c.write_all(request.as_bytes()).unwrap();
        let head = head_of(&mut c);
        assert!(head.starts_with("HTTP/1.1 502 "), "{why}: {head}");
        assert!(!head.to_ascii_lowercase().contains("proxy-authenticate"), "{why}: {head}");
        let body = String::from_utf8(to_close(&mut c)).unwrap();
        let denial = d.recv_timeout(WAIT).expect("said");
        assert_eq!(denial.op, UPSTREAM_OP, "{why}: {}", denial.target);
        let port = if request.starts_with("GET") { 80 } else { 443 };
        assert!(
            denial
                .target
                .starts_with(&format!("example.invalid:{port}: through the proxy tarae itself uses"))
                || denial.target.starts_with(&format!("example.invalid:{port}: tarae's own")),
            "{}",
            denial.target
        );
        assert!(denial.target.contains(why), "{why}: {}", denial.target);
        for said in [&denial.target, &denial.reported(), &body] {
            assert!(!said.contains(ITS_WORDS) && !said.contains("127.0.0.1"), "{why}: {said}");
        }
        let home = Path::new("/nonexistent-home");
        assert_eq!(
            crate::blocked::choices(&[denial], &crate::caps::Caps::at(home, Path::new("/w/p"))),
            [],
            "{why}"
        );
    }
}

// --- An https:// upstream (TLS to the proxy itself, srt's recipe) ---

type Tls = rustls::StreamOwned<rustls::ServerConnection, TcpStream>;

/// A stand-in https:// upstream: TLS under `config` (its handshake done by the first read), then `serve` inside
/// it — what it got, after a first line naming the server name the client's hello carried.
fn over_tls(s: TcpStream, config: Arc<rustls::ServerConfig>, serve: fn(&mut Tls) -> Vec<u8>) -> Vec<u8> {
    let _ = s.set_read_timeout(Some(WAIT));
    let mut t = Tls::new(rustls::ServerConnection::new(config).unwrap(), s);
    let got = serve(&mut t);
    let name = t.conn.server_name().unwrap_or("none").to_string();
    [format!("server name {name}\n").into_bytes(), got].concat()
}

/// Inside: the CONNECT read, 200, then what comes echoed until the client's close_notify — an end without one
/// (the connection cut) said as ` (cut)` after what came.
fn opens_inside(t: &mut Tls) -> Vec<u8> {
    let mut got = read_head(t);
    let _ = t.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
    let mut b = [0u8; 16 * 1024];
    loop {
        match t.read(&mut b) {
            Ok(0) => break,
            Ok(n) => {
                got.extend_from_slice(&b[..n]);
                if t.write_all(&b[..n]).is_err() {
                    break;
                }
            }
            Err(_) => {
                got.extend_from_slice(b" (cut)");
                break;
            }
        }
    }
    got
}

/// Inside: a plain request's head (no body) read, its answer, close_notify.
fn web_inside(t: &mut Tls) -> Vec<u8> {
    let got = read_head(t);
    let _ = t.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    t.conn.send_close_notify();
    let _ = t.flush();
    got
}

/// Inside: a 407 to the first head.
fn asks_basic_inside(t: &mut Tls) -> Vec<u8> {
    let got = read_head(t);
    let _ = t.write_all(
        b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"corp\"\r\n\
          Content-Length: 0\r\n\r\n",
    );
    let _ = t.flush();
    got
}

/// A proxy that speaks no TLS: plain HTTP's refusal at once, whatever comes.
fn speaks_plain(mut s: TcpStream) -> Vec<u8> {
    let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
    let mut got = Vec::new();
    let _ = s.read_to_end(&mut got);
    got
}

fn tls_opens(s: TcpStream) -> Vec<u8> {
    over_tls(s, crate::web::fetch::tests::server_tls(), opens_inside)
}

fn tls_web(s: TcpStream) -> Vec<u8> {
    over_tls(s, crate::web::fetch::tests::server_tls(), web_inside)
}

fn tls_asks_basic(s: TcpStream) -> Vec<u8> {
    over_tls(s, crate::web::fetch::tests::server_tls(), asks_basic_inside)
}

fn tls_stranger(s: TcpStream) -> Vec<u8> {
    over_tls(s, crate::web::fetch::tests::stranger_tls(), opens_inside)
}

/// tarae's environment naming an https:// upstream `name:port` for both kinds, with these credentials.
fn via_https(name: &str, port: u16, creds: (&str, &str)) -> Upstreams {
    let url = format!("https://{}:{}@{name}:{port}", pct(creds.0), pct(creds.1));
    up::of(&env(&[("HTTPS_PROXY", url.clone()), ("HTTP_PROXY", url)]), Setting::Env)
}

/// Through an https:// upstream (srt's recipe): TLS to it first — its name the server name, its certificate
/// checked as the system checks one —, then the tunnel asked inside, `CONNECT host:port` with tarae's credentials,
/// and the run's own ClientHello after it, checked as ever; what the host sends back reaches the run, and the run's
/// close ends the bridge (the upstream reads its close_notify). A plain request goes inside in absolute form.
/// tarae looks up the upstream's own name only, never the run's.
#[test]
fn an_https_upstream_is_spoken_to_inside_tls() {
    let (tunnels, web) = (outside(tls_opens), outside(tls_web));
    let (resolve, lookups) =
        resolver(vec![("localhost", vec![vec![tunnels.at]]), ("page.test", vec![vec![web.at]])]);
    let f = start(resolve, limits());
    let creds = ("u", "tarae-fake-token-8");
    let basic = parse::base64(b"u:tarae-fake-token-8");
    let rules = rules_via(&["example.invalid"], via_https("localhost", tunnels.at.port(), creds));
    let (mut c, _) = connection(&f.proxy, 1, 1, rules);
    assert_eq!(ask(&mut c, &connect_head("example.invalid:443")), 200);
    c.write_all(curl_hello()).unwrap();
    let mut back = vec![0u8; curl_hello().len()];
    c.read_exact(&mut back).unwrap();
    assert_eq!(back, curl_hello(), "the host's side comes back through the bridge");
    drop(c);
    let head = format!(
        "CONNECT example.invalid:443 HTTP/1.1\r\nHost: example.invalid:443\r\nProxy-Authorization: Basic {basic}\r\n\r\n"
    );
    let got = tunnels.got.recv_timeout(WAIT).expect("the run's close reached the upstream");
    let want = [b"server name localhost\n", head.as_bytes(), curl_hello()].concat();
    assert_eq!(got, want, "{}", String::from_utf8_lossy(&got));
    assert_eq!(lookups.load(Ordering::SeqCst), 1, "the upstream's own name, never the run's");
    // A plain request: inside, in absolute form, its answer back whole
    let rules = rules_via(&["web.example.invalid"], via_https("page.test", web.at.port(), creds));
    let (mut c, _) = connection(&f.proxy, 1, 1, rules);
    c.write_all(b"GET http://web.example.invalid/x HTTP/1.1\r\nHost: web.example.invalid\r\n\r\n").unwrap();
    let answer = String::from_utf8(to_close(&mut c)).unwrap();
    assert!(answer.starts_with("HTTP/1.1 200 ") && answer.ends_with("\r\n\r\nok"), "{answer}");
    let got = String::from_utf8(web.got.recv_timeout(WAIT).unwrap()).unwrap();
    assert_eq!(
        got,
        format!(
            "server name page.test\nGET http://web.example.invalid/x HTTP/1.1\r\nHost: web.example.invalid\r\n\
             Connection: close\r\nProxy-Authorization: Basic {basic}\r\n\r\n"
        )
    );
}

/// A tunnel through an https:// upstream carries more than the bridge and the relay hold at once (64 KiB each),
/// both ways at the same time — every byte back in order, none lost to either one's limit (Linux: the relay
/// splices the bridge's end).
#[test]
fn a_big_tunnel_goes_whole_through_the_bridge() {
    let upstream = outside(tls_opens);
    let (resolve, _) = resolver(vec![("localhost", vec![vec![upstream.at]])]);
    let f = start(resolve, limits());
    let rules = rules_via(&["example.invalid"], via_https("localhost", upstream.at.port(), ("u", "p")));
    let (mut c, _) = connection(&f.proxy, 1, 1, rules);
    assert_eq!(ask(&mut c, &connect_head("example.invalid:443")), 200);
    // (a ClientHello first, as the proxy checks it; then 4 MiB of a pattern no shift can match)
    let mut sent = curl_hello().to_vec();
    sent.extend((0..4 << 20).map(|i: u32| (i % 251) as u8));
    let mut w = c.try_clone().unwrap();
    let all = sent.clone();
    let writer = crate::event::thread("tarae-writer", move || w.write_all(&all));
    let mut back = vec![0u8; sent.len()];
    c.read_exact(&mut back).unwrap();
    writer.join().unwrap().unwrap();
    assert!(
        back == sent,
        "{} bytes back, the first wrong at {:?}",
        back.len(),
        back.iter().zip(&sent).position(|(a, b)| a != b)
    );
}

/// The burst `burst_inside` sends after the run's ClientHello: past what the bridge and the relay hold at once
/// (64 KiB each) many times over, short of what the sockets between hold, so the upstream is done and closed
/// before the run reads a byte — a pattern no shift can match.
fn burst() -> Vec<u8> {
    (0..384u32 << 10).map(|i| (i % 251) as u8).collect()
}

/// Inside: the CONNECT opened, the run's ClientHello read, then that hello back and `burst()`, its close_notify and
/// the connection closed at once.
fn burst_inside(t: &mut Tls) -> Vec<u8> {
    let mut got = read_head(t);
    let _ = t.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
    let mut hello = vec![0u8; curl_hello().len()];
    let _ = t.read_exact(&mut hello);
    got.extend(&hello);
    let _ = t.write_all(&[&hello[..], &burst()].concat());
    t.conn.send_close_notify();
    let _ = t.flush();
    got
}

fn tls_burst(s: TcpStream) -> Vec<u8> {
    over_tls(s, crate::web::fetch::tests::server_tls(), burst_inside)
}

/// An https:// upstream that sends far more than the bridge holds and closes at once has every byte reach the run,
/// the last too: what rustls decrypted before the socket's end is still taken out after it. (The run reads only
/// once the upstream is done, so the bridge meets the socket's end with its 64 KiB waiting and more in rustls.)
#[test]
fn an_https_upstreams_last_burst_reaches_the_run_whole() {
    let upstream = outside(tls_burst);
    let (resolve, _) = resolver(vec![("localhost", vec![vec![upstream.at]])]);
    let f = start(resolve, limits());
    let rules = rules_via(&["example.invalid"], via_https("localhost", upstream.at.port(), ("u", "p")));
    let (mut c, _) = connection(&f.proxy, 1, 1, rules);
    assert_eq!(ask(&mut c, &connect_head("example.invalid:443")), 200);
    c.write_all(curl_hello()).unwrap();
    // (the buffers between hold the burst: the upstream ends before the run reads — if one didn't, the run's
    // reading lets it end, and the test still checks every byte)
    let ended = upstream.got.recv_timeout(HOLD).is_ok();
    let back = to_close(&mut c);
    let want = [curl_hello(), &burst()].concat();
    assert!(
        back == want,
        "{} of {} bytes back, the first wrong at {:?} (the upstream ended first: {ended})",
        back.len(),
        want.len(),
        back.iter().zip(&want).position(|(a, b)| a != b)
    );
    assert!(ended, "the burst fit in the sockets between: the upstream ended before the run read");
}

/// Upstreams that answered and keep their connection open, silent — held here, so nothing of theirs closes it.
static HELD: Mutex<Vec<Tls>> = Mutex::new(Vec::new());

/// An https:// upstream that answers a plain request's first head with a 407 and keeps the connection open, as one
/// waiting for the credentials it asked for does — never closing it, never sending more. What it gives back: its
/// peer's address (the bridge's end).
fn tls_asks_and_stays(s: TcpStream) -> Vec<u8> {
    let peer = s.peer_addr().unwrap();
    let _ = s.set_read_timeout(Some(WAIT));
    let mut t = Tls::new(rustls::ServerConnection::new(crate::web::fetch::tests::server_tls()).unwrap(), s);
    read_head(&mut t);
    let _ = t.write_all(
        b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"corp\"\r\n\
          Content-Length: 0\r\n\r\n",
    );
    let _ = t.flush();
    HELD.lock().unwrap_or_else(|e| e.into_inner()).push(t);
    peer.to_string().into_bytes()
}

/// The bridge lives no longer than the relay it serves: once the relay is done (here: an upstream's 407 to a plain
/// request, never passed on), the bridge's thread ends and its sockets close — an upstream that keeps its side open
/// and silent doesn't hold them, as the relay's own life is its two sides' (srt ends a tunnel's one side with the
/// other's close).
#[test]
fn the_bridge_ends_with_its_relay_whatever_the_upstream_keeps_open() {
    let upstream = outside(tls_asks_and_stays);
    let (resolve, _) = resolver(vec![("localhost", vec![vec![upstream.at]])]);
    let f = start(resolve, limits());
    let rules = rules_via(&["web.example.invalid"], via_https("localhost", upstream.at.port(), ("u", "p")));
    let (mut c, d) = connection(&f.proxy, 1, 1, rules);
    c.write_all(b"GET http://web.example.invalid/x HTTP/1.1\r\nHost: web.example.invalid\r\n\r\n").unwrap();
    assert!(head_of(&mut c).starts_with("HTTP/1.1 502 "));
    assert!(d.recv_timeout(WAIT).expect("said").target.contains("(407)"));
    let peer: SocketAddr =
        String::from_utf8(upstream.got.recv_timeout(WAIT).unwrap()).unwrap().parse().unwrap();
    let ended = crate::sandbox::proxy::bridge::ended(peer);
    HELD.lock().unwrap_or_else(|e| e.into_inner()).retain(|t| t.sock.peer_addr().ok() != Some(peer));
    assert!(ended, "the bridge's thread outlived its relay");
}

/// An answer to CONNECT that ends exactly where one read of it does (4,096 bytes), then bytes of the upstream's own.
fn edge_answer() -> Vec<u8> {
    let (head, end) = (&b"HTTP/1.1 200 Connection established\r\nX-Pad: "[..], &b"\r\n\r\n"[..]);
    [head, &vec![b'a'; 4096 - head.len() - end.len()], end, b"early"].concat()
}

/// The same with the bytes in the read the answer's end is in.
const EARLY_ANSWER: &[u8] = b"HTTP/1.1 200 Connection established\r\n\r\nearly";

fn early(s: TcpStream) -> Vec<u8> {
    answering(s, EARLY_ANSWER)
}

fn edge(s: TcpStream) -> Vec<u8> {
    answering(s, &edge_answer())
}

/// Inside: `answer` to the head, in one write (one record), then what comes to the end.
fn answering_inside(t: &mut Tls, answer: &[u8]) -> Vec<u8> {
    let mut got = read_head(t);
    let _ = t.write_all(answer);
    let _ = t.flush();
    let mut rest = Vec::new();
    let _ = t.read_to_end(&mut rest);
    got.extend(rest);
    got
}

fn tls_early(s: TcpStream) -> Vec<u8> {
    over_tls(s, crate::web::fetch::tests::server_tls(), |t| answering_inside(t, EARLY_ANSWER))
}

fn tls_edge(s: TcpStream) -> Vec<u8> {
    over_tls(s, crate::web::fetch::tests::server_tls(), |t| answering_inside(t, &edge_answer()))
}

/// What an upstream sends after its 2xx to a CONNECT never reaches the run ahead of the run's own first bytes: the
/// tunnel is refused (502), said as the upstream's — whether the read that took the answer took them too, or the
/// answer ended where that read did (4,096 bytes) and they were left behind it: on the socket (http://) or
/// decrypted in rustls (https://).
#[test]
fn an_upstreams_bytes_after_its_2xx_are_refused() {
    type Serve = fn(TcpStream) -> Vec<u8>;
    let cases: [(Serve, bool, &str); 4] = [
        (early, false, "in the answer's read"),
        (edge, false, "past the answer's read"),
        (tls_early, true, "in the answer's read, inside TLS"),
        (tls_edge, true, "past the answer's read, inside TLS"),
    ];
    for (serve, tls, case) in cases {
        let upstream = outside(serve);
        let (resolve, _) = resolver(vec![("localhost", vec![vec![upstream.at]])]);
        let f = start(resolve, limits());
        let way =
            if tls { via_https("localhost", upstream.at.port(), ("u", "p")) } else { via(upstream.at, None) };
        let (mut c, d) = connection(&f.proxy, 1, 1, rules_via(&["example.invalid"], way));
        c.write_all(connect_head("example.invalid:443").as_bytes()).unwrap();
        let head = head_of(&mut c);
        assert!(head.starts_with("HTTP/1.1 502 "), "{case}: {head}");
        let denial = d.recv_timeout(WAIT).expect("said");
        assert!(denial.target.contains("it sent bytes before the client's own"), "{case}: {}", denial.target);
    }
}

/// What fails at an https:// upstream's TLS is said as the upstream's, in tarae's words — never the
/// certificate's: one no CA of the system's issued, one for another name, a proxy that speaks no TLS, one that
/// never answers the handshake; and a 407 inside the TLS is read as ever. The run gets 502.
#[test]
fn an_https_upstreams_tls_failures_are_said() {
    let (good, stranger, plain, quiet, basic) = (
        outside(tls_opens),
        outside(tls_stranger),
        outside(speaks_plain),
        outside(silent),
        outside(tls_asks_basic),
    );
    let (resolve, _) = resolver(vec![
        ("localhost", vec![vec![stranger.at]]),
        ("proxy.example.invalid", vec![vec![good.at]]),
        ("plain.test", vec![vec![plain.at]]),
        ("quiet.test", vec![vec![quiet.at]]),
        ("page.test", vec![vec![basic.at]]),
    ]);
    let f = start(resolve, Limits { connect: Duration::from_millis(400), ..limits() });
    let creds = ("u", "tarae-fake-token-9");
    let cases = [
        (
            via_https("localhost", stranger.at.port(), creds),
            "TLS to it: its certificate isn't one this system trusts",
        ),
        (
            via_https("proxy.example.invalid", good.at.port(), creds),
            "TLS to it: its certificate is for another name",
        ),
        (via_https("plain.test", plain.at.port(), creds), "TLS to it: the handshake failed"),
        (via_https("quiet.test", quiet.at.port(), creds), "it didn't answer within 0.4 s"),
        (
            via_https("page.test", basic.at.port(), creds),
            "it refused the credentials in tarae's HTTPS_PROXY (407)",
        ),
    ];
    for (upstream, why) in cases {
        let (mut c, d) = connection(&f.proxy, 1, 1, rules_via(&["example.invalid"], upstream));
        c.write_all(connect_head("example.invalid:443").as_bytes()).unwrap();
        let head = head_of(&mut c);
        assert!(head.starts_with("HTTP/1.1 502 "), "{why}: {head}");
        let denial = d.recv_timeout(WAIT).expect("said");
        assert_eq!(denial.op, UPSTREAM_OP, "{why}: {}", denial.target);
        assert!(denial.target.contains(why), "{why}: {}", denial.target);
        assert!(!denial.target.contains("tarae-fake-token-9") && !denial.target.contains("stranger"));
    }
}

/// `NO_PROXY`'s hosts go direct — looked up by tarae, every address checked: one resolving to a metadata server is
/// refused again; the upstream hears of none of them. Another host goes through the upstream.
#[test]
fn no_proxy_hosts_go_direct_with_the_address_checks() {
    let (upstream, out) = (outside(opens), outside(echo));
    let meta: SocketAddr = "169.254.169.254:443".parse().unwrap();
    let (resolve, lookups) = resolver(vec![
        ("direct.example.invalid", vec![vec![out.at]]),
        ("meta.direct.example.invalid", vec![vec![meta]]),
    ]);
    let f = start(resolve, limits());
    let named = [
        ("HTTPS_PROXY", format!("http://{}", upstream.at)),
        ("NO_PROXY", "localhost,.direct.example.invalid".to_string()),
    ];
    let rules = || rules_via(&["*.example.invalid"], up::of(&env(&named), Setting::Env));
    let (mut c, _) = connection(&f.proxy, 1, 1, rules());
    assert_eq!(ask(&mut c, &connect_head("direct.example.invalid:443")), 200);
    assert!(took(&out, 1), "straight to the address tarae checked");
    let (mut c, d) = connection(&f.proxy, 1, 1, rules());
    assert_eq!(ask(&mut c, &connect_head("meta.direct.example.invalid:443")), 403);
    assert!(said(&d).contains("never reached"), "the address checks are back");
    assert!(took(&upstream, 0) && lookups.load(Ordering::SeqCst) == 2);
    let (mut c, _) = connection(&f.proxy, 1, 1, rules());
    assert_eq!(ask(&mut c, &connect_head("other.example.invalid:443")), 200);
    assert!(took(&upstream, 1), "a host NO_PROXY doesn't name: through the upstream");
    assert_eq!(lookups.load(Ordering::SeqCst), 2, "and not looked up by tarae");
}

/// A cloud metadata server's own name is refused by name whichever way it would go — through an upstream, which
/// would look it up where tarae can't check the address, or direct, for any host allowed — said; nothing is looked
/// up or reached. (Partial: a name that only leads there isn't caught through an upstream — the report says so.)
#[test]
fn metadata_names_are_refused_by_name() {
    let upstream = outside(opens);
    let (resolve, lookups) = resolver(Vec::new());
    let f = start(resolve, limits());
    let any = || Egress { any: true, ..rules(&[]) };
    let through = || Egress { upstream: via(upstream.at, None), ..any() };
    for (rules, request) in [
        (through(), connect_head("metadata.google.internal:443")),
        (through(), "GET http://metadata.google.internal/computeMetadata/v1/ HTTP/1.1\r\n\r\n".into()),
        (through(), connect_head("metadata.goog:443")),
        (any(), connect_head("instance-data.ec2.internal:443")),
        (any(), "GET http://Metadata./latest HTTP/1.1\r\n\r\n".into()),
    ] {
        let (mut c, d) = connection(&f.proxy, 1, 1, rules);
        assert_eq!(ask(&mut c, &request), 403, "{request}");
        assert!(said(&d).contains("a cloud metadata server's own name"), "{request}");
    }
    assert_eq!(lookups.load(Ordering::SeqCst), 0);
    assert!(took(&upstream, 0));
}

/// `n` bytes from `s` into `got` — false: it ended first.
fn take(s: &mut TcpStream, n: usize, got: &mut Vec<u8>) -> bool {
    let mut b = vec![0u8; n];
    let ok = s.read_exact(&mut b).is_ok();
    if ok {
        got.extend_from_slice(&b);
    }
    ok
}

/// A stand-in SOCKS5 server (RFC 1928, 1929) answering as told: the way it signs in (`way`; None: a user and
/// password where offered, else none), whether it takes the credentials, its CONNECT's reply code — then, its
/// tunnel open, `then` serves what comes. All it got, in order.
fn socks(mut s: TcpStream, way: Option<u8>, takes: bool, rep: u8, then: fn(TcpStream) -> Vec<u8>) -> Vec<u8> {
    let mut got = Vec::new();
    let rest = |mut s: TcpStream, mut got: Vec<u8>| {
        let _ = s.read_to_end(&mut got);
        got
    };
    if !take(&mut s, 2, &mut got) || !take(&mut s, usize::from(got[1]), &mut got) {
        return got;
    }
    let way = way.unwrap_or(if got[2..].contains(&2) { 2 } else { 0 });
    let _ = s.write_all(&[5, way]);
    if way == 0xff {
        return rest(s, got);
    }
    if way == 2 {
        let at = got.len();
        let sign_in = take(&mut s, 2, &mut got)
            && take(&mut s, usize::from(got[at + 1]), &mut got)
            && take(&mut s, 1, &mut got)
            && take(&mut s, usize::from(got[got.len() - 1]), &mut got);
        let _ = s.write_all(&[1, if takes { 0 } else { 1 }]);
        if !sign_in || !takes {
            return rest(s, got);
        }
    }
    let at = got.len();
    if !take(&mut s, 5, &mut got) || !take(&mut s, usize::from(got[at + 4]) + 2, &mut got) {
        return got;
    }
    let _ = s.write_all(&[5, rep, 0, 1, 0, 0, 0, 0, 0, 0]);
    if rep != 0 {
        return rest(s, got);
    }
    got.extend(then(s));
    got
}

fn socks_echo(s: TcpStream) -> Vec<u8> {
    socks(s, None, true, 0, echo)
}

fn socks_web(s: TcpStream) -> Vec<u8> {
    socks(s, None, true, 0, web)
}

fn socks_refuses_sign_in(s: TcpStream) -> Vec<u8> {
    socks(s, None, false, 0, echo)
}

fn socks_takes_no_way(s: TcpStream) -> Vec<u8> {
    socks(s, Some(0xff), true, 0, echo)
}

/// Its rules don't allow the host (RFC 1928's reply 2).
fn socks_refuses_host(s: TcpStream) -> Vec<u8> {
    socks(s, None, true, 2, echo)
}

/// `socks_refuses_host`, 0.6 s late — an upstream on a loaded machine standing in: later than a 0.4 s limit.
fn socks_refuses_host_late(s: TcpStream) -> Vec<u8> {
    std::thread::sleep(Duration::from_millis(600));
    socks_refuses_host(s)
}

/// tarae's environment naming a SOCKS5 upstream at `at` (`scheme`: socks5h or socks5) for everything, as a local
/// client's `ALL_PROXY` does.
fn via_socks(scheme: &str, at: SocketAddr, creds: Option<(&str, &str)>) -> Upstreams {
    let creds = creds.map(|(u, p)| format!("{}:{}@", pct(u), pct(p))).unwrap_or_default();
    up::of(&env(&[("ALL_PROXY", format!("{scheme}://{creds}{at}"))]), Setting::Env)
}

/// RFC 1928's CONNECT for `name:port` by its name, and RFC 1929's sign-in — as the stand-in gets them.
fn socks_connect(name: &str, port: u16) -> Vec<u8> {
    [&[5, 1, 0, 3, name.len() as u8][..], name.as_bytes(), &port.to_be_bytes()].concat()
}

/// Through a SOCKS5 upstream — `socks5h://` with credentials, `socks5://` without: the same, since tarae never
/// looks the name up — an allowed host's tunnel is asked of it by name (ATYP 3), signed in with tarae's
/// credentials (RFC 1929) where it has any, and the run's ClientHello follows, checked as ever; the run's name is
/// never looked up by tarae. A host the rules don't let go never reaches it; a ClientHello naming another server
/// never goes past the CONNECT.
#[test]
fn a_socks5_upstream_is_chained() {
    let upstream = outside(socks_echo);
    let (resolve, lookups) = resolver(Vec::new());
    let f = start(resolve, Limits { hold: Duration::from_millis(300), ..limits() });
    let signed = [&[5, 2, 0, 2, 1, 1, b'u', 18][..], b"tarae-fake-token-6"].concat();
    for (scheme, creds, hello) in
        [("socks5h", Some(("u", "tarae-fake-token-6")), signed.clone()), ("socks5", None, vec![5, 1, 0])]
    {
        let rules = rules_via(&["example.invalid"], via_socks(scheme, upstream.at, creds));
        let (c, _) = tunnel(&f.proxy, 1, 1, rules, "example.invalid:443");
        drop(c);
        let got = upstream.got.recv_timeout(WAIT).unwrap();
        assert_eq!(
            got,
            [&hello[..], &socks_connect("example.invalid", 443), curl_hello()].concat(),
            "{scheme}"
        );
    }
    assert_eq!(lookups.load(Ordering::SeqCst), 0, "the run's name is the upstream's to look up");
    let rules = || {
        rules_via(&["example.invalid"], via_socks("socks5h", upstream.at, Some(("u", "tarae-fake-token-6"))))
    };
    let (mut c, d) = connection(&f.proxy, 1, 1, rules());
    assert_eq!(ask(&mut c, &connect_head("refused.example.invalid:443")), 403);
    assert!(said_host(&d).contains("didn't answer"));
    let (mut c, d) = connection(&f.proxy, 1, 1, rules());
    assert_eq!(ask(&mut c, &connect_head("example.invalid:443")), 200);
    c.write_all(cargo_hello()).unwrap();
    assert_eq!(to_close(&mut c), b"");
    assert!(said(&d).contains("names index.crates.io"));
    let got = upstream.got.recv_timeout(WAIT).unwrap();
    assert_eq!(
        got,
        [&signed[..], &socks_connect("example.invalid", 443)].concat(),
        "the CONNECT, nothing after"
    );
    assert!(took(&upstream, 3), "the refused host never reached it");
}

/// A plain request through a SOCKS5 upstream goes as it would to the host: CONNECT host:80 by name, then the head
/// in origin form — its `Host`, `Connection: close`, no `Proxy-*` of the run's or tarae's (SOCKS5 carries the
/// credentials itself) — and the host's answer back as it came.
#[test]
fn plain_http_goes_through_a_socks5_upstream_in_origin_form() {
    let upstream = outside(socks_web);
    let (resolve, lookups) = resolver(Vec::new());
    let f = start(resolve, limits());
    let rules = rules_via(&["web.example.invalid"], via_socks("socks5h", upstream.at, None));
    let (mut c, _) = connection(&f.proxy, 1, 1, rules);
    let head = "GET http://web.example.invalid/x HTTP/1.1\r\nHost: web.example.invalid\r\n\
                Proxy-Authorization: Basic cnVuOng=\r\n\r\n";
    assert_eq!(ask(&mut c, head), 200);
    let got = upstream.got.recv_timeout(WAIT).unwrap();
    let origin = b"GET /x HTTP/1.1\r\nHost: web.example.invalid\r\nConnection: close\r\n\r\n";
    assert_eq!(
        String::from_utf8_lossy(&got),
        String::from_utf8_lossy(
            &[&[5, 1, 0][..], &socks_connect("web.example.invalid", 80), origin].concat()
        )
    );
    assert_eq!(lookups.load(Ordering::SeqCst), 0);
}

/// What failed at a SOCKS5 upstream is said as the upstream's (`network-upstream`), as an http one's: its sign-in
/// refused, no way of signing in it takes (without credentials, and with), the host refused by its rules (a tunnel
/// and a plain request), silent, unreachable — the run gets 502, the report names the variable, never where it is,
/// and `:sandbox-blocked` offers nothing. Only the silent one is waited on to the proxy's limit (0.4 s, a proxy of
/// its own); the others answer, and their proxy waits for that up to `HOLD` — under load a 0.4 s limit had one
/// that answers said as silent (#232).
#[test]
fn a_socks5_upstreams_failures_are_said_as_its_own() {
    let (_held, closed) = refused_at();
    let (refusing, no_way, no_host, late, quiet) = (
        outside(socks_refuses_sign_in),
        outside(socks_takes_no_way),
        outside(socks_refuses_host),
        outside(socks_refuses_host_late),
        outside(silent),
    );
    let quick = start(resolver(Vec::new()).0, Limits { connect: Duration::from_millis(400), ..limits() });
    let f = start(resolver(Vec::new()).0, Limits { connect: HOLD, ..limits() });
    let creds = Some(("u", "tarae-fake-token-7"));
    let get = "GET http://example.invalid/ HTTP/1.1\r\n\r\n".to_string();
    let connect = connect_head("example.invalid:443");
    let cases: Vec<(Upstreams, &String, &str)> = vec![
        (
            via_socks("socks5h", refusing.at, creds),
            &connect,
            "it refused the credentials in tarae's ALL_PROXY",
        ),
        (
            via_socks("socks5h", no_way.at, None),
            &connect,
            "it asks for credentials (or a way tarae doesn't speak), and tarae's ALL_PROXY gives none",
        ),
        (via_socks("socks5h", no_way.at, creds), &connect, "a way to sign in tarae doesn't speak"),
        (
            via_socks("socks5", no_host.at, creds),
            &connect,
            "it refused it: its rules don't allow it (SOCKS5 reply 2)",
        ),
        (
            via_socks("socks5", no_host.at, creds),
            &get,
            "it refused it: its rules don't allow it (SOCKS5 reply 2)",
        ),
        (
            via_socks("socks5", late.at, creds),
            &connect,
            "it refused it: its rules don't allow it (SOCKS5 reply 2)",
        ),
        (via_socks("socks5h", quiet.at, creds), &connect, "it didn't answer within 0.4 s"),
        (via_socks("socks5h", closed, creds), &connect, "it can't be reached (connection refused)"),
    ];
    for (upstream, request, why) in cases {
        let proxy = if why.contains("didn't answer") { &quick.proxy } else { &f.proxy };
        let (mut c, d) = connection(proxy, 1, 1, rules_via(&["example.invalid"], upstream));
        c.write_all(request.as_bytes()).unwrap();
        let head = head_of(&mut c);
        assert!(head.starts_with("HTTP/1.1 502 "), "{why}: {head}");
        let denial = d.recv_timeout(WAIT).expect("said");
        assert_eq!(denial.op, UPSTREAM_OP, "{why}: {}", denial.target);
        assert!(denial.target.contains(why), "{why}: {}", denial.target);
        assert!(!denial.target.contains("127.0.0.1"), "{}", denial.target);
        let home = Path::new("/nonexistent-home");
        assert_eq!(
            crate::blocked::choices(&[denial], &crate::caps::Caps::at(home, Path::new("/w/p"))),
            [],
            "{why}"
        );
    }
}

/// A small fixed-seed generator: the same credentials every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// `n` characters that a URL must encode, or that end a part of it if left bare.
    fn awkward(&mut self, n: usize, from: &[u8]) -> String {
        (0..n).map(|_| char::from(from[(self.next() % from.len() as u64) as usize])).collect()
    }
}

/// The credentials in tarae's proxy variable go to the upstream and nowhere else. Random fake ones — `@ : % /`
/// and more in them, percent-encoded as a URL holds them — through every way a connection goes: let through,
/// refused before the upstream, a ClientHello for another server, the upstream's 407s, its 403, unreachable,
/// silent, a plain request and its 407, an https:// one (inside its TLS), a SOCKS4 one; through SOCKS5, a tunnel, a plain
/// request, its sign-in refused. What the run gets back, every refusal as its report says it, tarae's log, the
/// run's rules line, the policy's `Debug` and key, and the run's environment hold none of the user, the password
/// (as written or encoded) or the `Basic` value; the upstream gets them once a request — as `Basic` (http), as
/// RFC 1929's sign-in (SOCKS5).
#[test]
fn upstream_credentials_go_only_to_the_upstream() {
    let (opening, basic, negotiate, refusing, quiet, webs) = (
        outside(opens),
        outside(asks_basic),
        outside(asks_negotiate),
        outside(forbids),
        outside(silent),
        outside(web),
    );
    let (socks_tunnel, socks_plain, socks_refusing) =
        (outside(socks_echo), outside(socks_web), outside(socks_refuses_sign_in));
    let tls_opening = outside(tls_opens);
    let (_held, closed) = refused_at();
    let limits = Limits { hold: Duration::from_millis(200), connect: Duration::from_millis(300), ..limits() };
    // (an https:// one by its name: its certificate's)
    let f = start(resolver(vec![("localhost", vec![vec![tls_opening.at]])]).0, limits);
    let tls_at = format!("localhost:{}", tls_opening.at.port());
    let mut rng = Rng(0x5ec2_e75e_ed00_0005);
    let mut seen: Vec<String> = Vec::new();
    let mut forms: Vec<String> = Vec::new();
    let captured = crate::log::capture();
    for _ in 0..6 {
        let (u, p) = (format!("{:016x}", rng.next()), format!("{:016x}", rng.next()));
        let user = format!("tarae-fake-user-{u}{}", rng.awkward(3, b"@%/?# "));
        let password = format!("tarae-fake-token-{p}{}", rng.awkward(4, b"@:%/?#& "));
        let value = parse::base64(format!("{user}:{password}").as_bytes());
        let header = format!("Proxy-Authorization: Basic {value}\r\n").into_bytes();
        let sign_in =
            [&[1, user.len() as u8][..], user.as_bytes(), &[password.len() as u8], password.as_bytes()]
                .concat();
        let url = |scheme: &str, at: &dyn std::fmt::Display| {
            format!("{scheme}://{}:{}@{at}", pct(&user), pct(&password))
        };
        let named = |scheme: &str, at: &dyn std::fmt::Display| {
            let u = url(scheme, at);
            up::of(&env(&[("HTTPS_PROXY", u.clone()), ("http_proxy", u)]), Setting::Env)
        };
        let connect = connect_head("example.invalid:443");
        let hello = [connect.as_bytes(), curl_hello()].concat();
        let other = [connect.as_bytes(), cargo_hello()].concat();
        let get = b"GET http://example.invalid/x HTTP/1.1\r\nContent-Length: 2\r\n\r\nok".to_vec();
        // (the request, the upstream it names, and the stand-in that gets it with what carries the credentials
        // there — None: it reaches none)
        let (http, socks) = (&header[..], &sign_in[..]);
        type Reaches<'a> = Option<(&'a Outside, &'a [u8])>;
        let paths: Vec<(Vec<u8>, Upstreams, Reaches)> = vec![
            (hello.clone(), named("http", &opening.at), Some((&opening, http))),
            (connect_head("refused.example.invalid:443").into_bytes(), named("http", &opening.at), None),
            (other, named("http", &opening.at), Some((&opening, http))),
            (hello.clone(), named("http", &basic.at), Some((&basic, http))),
            (hello.clone(), named("http", &negotiate.at), Some((&negotiate, http))),
            (hello.clone(), named("http", &refusing.at), Some((&refusing, http))),
            (hello.clone(), named("http", &closed), None),
            (hello.clone(), named("http", &quiet.at), Some((&quiet, http))),
            (get.clone(), named("http", &webs.at), Some((&webs, http))),
            (get.clone(), named("http", &basic.at), Some((&basic, http))),
            (hello.clone(), named("https", &tls_at), Some((&tls_opening, http))),
            (hello.clone(), named("socks4a", &opening.at), None),
            (hello.clone(), named("socks5h", &socks_tunnel.at), Some((&socks_tunnel, socks))),
            (get, named("socks5", &socks_plain.at), Some((&socks_plain, socks))),
            (hello, named("socks5h", &socks_refusing.at), Some((&socks_refusing, socks))),
        ];
        for (request, upstream, reaches) in paths {
            seen.push(format!("{upstream:?}"));
            let (mut c, d) = connection(&f.proxy, 1, 1, rules_via(&["example.invalid"], upstream));
            c.write_all(&request).unwrap();
            c.shutdown(std::net::Shutdown::Write).unwrap();
            let mut back = Vec::new();
            let _ = c.read_to_end(&mut back);
            seen.push(String::from_utf8_lossy(&back).into_owned());
            seen.extend(d.try_iter().flat_map(|d| [d.target.clone(), d.reported()]));
            if let Some((o, carried)) = reaches {
                let got = o.got.recv_timeout(WAIT).expect("it reached the upstream");
                let n = got.windows(carried.len()).filter(|w| w == &carried).count();
                assert_eq!(n, 1, "once a request: {}", String::from_utf8_lossy(&got));
            }
        }
        // The run's side of it: a policy made with this upstream — its rules line, `Debug`, key, environment
        let policy = crate::sandbox::Policy::for_project(&crate::sandbox::Env {
            root: "/w/p".into(),
            network: true,
            upstream: named("http", &opening.at),
            ..Default::default()
        });
        seen.push(crate::sandbox::Guard::In(Box::new(policy.clone())).rules());
        seen.push(format!("{policy:?}"));
        seen.push(crate::sandbox::policy_key(&policy));
        let cmd = crate::sandbox::command(Some(&policy), "true", &[]);
        seen.extend(cmd.get_envs().map(|(k, v)| format!("{k:?}={v:?}")));
        // (each whole, and its random part alone: a part of it anywhere is a leak too)
        forms.extend([user.clone(), password.clone(), pct(&user), pct(&password), value, u, p]);
    }
    seen.extend(captured.lines());
    drop(captured);
    assert!(seen.iter().any(|s| s.contains("the upstream proxy 127.0.0.1:")), "the log says where it is");
    for form in &forms {
        for s in &seen {
            assert!(!s.contains(form.as_str()), "{form:?} in {s:?}");
        }
    }
}
