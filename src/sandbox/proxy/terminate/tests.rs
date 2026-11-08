use super::*;

use std::net::{Ipv4Addr, TcpListener};

/// The session numbers these tests use, apart from every other test's (the registry is the process's).
const SESSION: u64 = 0x7e12_a000;

/// `der` as PEM in `dir/name`.
fn pem_file(dir: &std::path::Path, name: &str, der: &[u8]) -> PathBuf {
    let f = dir.join(name);
    std::fs::write(&f, crate::sandbox::cabundle::pem(der)).unwrap();
    f
}

/// A program's version line, where it's on PATH and answers.
fn version(program: &str, arg: &str) -> Option<String> {
    let out = std::process::Command::new(program).arg(arg).output().ok()?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    out.status.success().then(|| text.trim().to_string())
}

/// The first number after `prefix` in `text`, and the one after its dot.
fn major_minor(text: &str, prefix: &str) -> Option<(u32, u32)> {
    let rest = text.split(prefix).nth(1)?;
    let mut parts = rest.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty());
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// tarae's leaf passes the strict checkers (OpenSSL 3's `-x509_strict`, Python 3.13's default context, which is
/// strict): its AKI the CA's raw SKI bytes (srt's forge wrote hex text, and strict refused it), the CA's
/// basicConstraints and keyUsage critical, serials positive. Each checker skipped, and said, where it isn't there
/// or is older (LibreSSL's `-x509_strict` checks neither key identifier).
#[test]
fn the_leaf_passes_the_strict_checkers() {
    let ca = of_session(SESSION + 1).unwrap();
    let leaf = ca.leaf("localhost").unwrap();
    let scratch = crate::scratch::Scratch::new("p1a-strict");
    let ca_pem = pem_file(&scratch, "ca.pem", ca.der());
    let leaf_pem = pem_file(&scratch, "leaf.pem", leaf.cert[0].as_ref());
    let mut checked = 0;
    // OpenSSL
    match version("openssl", "version") {
        Some(v) if v.starts_with("OpenSSL ") && major_minor(&v, "OpenSSL ").is_some_and(|(m, _)| m >= 3) => {
            let out = std::process::Command::new("openssl")
                .args(["verify", "-x509_strict", "-CAfile"])
                .args([&ca_pem, &leaf_pem])
                .output()
                .unwrap();
            let said =
                format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            assert!(out.status.success() && said.contains(": OK"), "{v}: {said}");
            checked += 1;
        }
        v => {
            eprintln!("the_leaf_passes_the_strict_checkers: openssl skipped — not OpenSSL 3 or later: {v:?}")
        }
    }
    // Python: a real handshake against tarae's leaf on loopback, its default context (strict from 3.13)
    match version("python3", "--version") {
        Some(v) if major_minor(&v, "Python ").is_some_and(|(m, n)| (m, n) >= (3, 13)) => {
            let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = l.local_addr().unwrap().port();
            let config = server_config("localhost", leaf.clone()).unwrap();
            let served = std::thread::spawn(move || {
                let (s, _) = l.accept().unwrap();
                s.set_read_timeout(Some(crate::test_results::PATIENCE)).unwrap();
                let mut t = rustls::StreamOwned::new(rustls::ServerConnection::new(config).unwrap(), s);
                let mut b = [0u8; 16];
                let n = t.read(&mut b).unwrap_or(0);
                let _ = t.write_all(&b[..n]);
                t.conn.send_close_notify();
                let _ = t.flush();
            });
            let script = "import socket, ssl, sys\n\
                          ctx = ssl.create_default_context(cafile=sys.argv[1])\n\
                          assert ctx.verify_flags & ssl.VERIFY_X509_STRICT\n\
                          s = ctx.wrap_socket(socket.create_connection(('127.0.0.1', int(sys.argv[2]))), \
                          server_hostname='localhost')\n\
                          s.sendall(b'strict-ok')\n\
                          print(s.recv(16).decode())\n";
            let out = std::process::Command::new("python3")
                .args(["-I", "-c", script])
                .arg(&ca_pem)
                .arg(port.to_string())
                .output()
                .unwrap();
            let said =
                format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            assert!(out.status.success() && said.contains("strict-ok"), "{v}: {said}");
            served.join().unwrap();
            checked += 1;
        }
        v => {
            eprintln!("the_leaf_passes_the_strict_checkers: python skipped — not Python 3.13 or later: {v:?}")
        }
    }
    eprintln!("the_leaf_passes_the_strict_checkers: {checked} of 2 checkers ran");
}

/// One CA a session, the same for each of its runs until it ends; another session's is another.
#[test]
fn a_sessions_ca_is_one_until_the_session_ends() {
    let (a, b) = (of_session(SESSION + 2).unwrap(), of_session(SESSION + 3).unwrap());
    assert!(Arc::ptr_eq(&a, &of_session(SESSION + 2).unwrap()), "the same session's again");
    assert!(!Arc::ptr_eq(&a, &b) && a.der() != b.der(), "another session's is its own");
    forget(SESSION + 2);
    let again = of_session(SESSION + 2).unwrap();
    assert!(!Arc::ptr_eq(&a, &again) && a.der() != again.der(), "a new one once its session ended");
    forget(SESSION + 2);
    forget(SESSION + 3);
}

/// A name's leaf is kept, the last `LEAVES_MAX` names' — the oldest minted again once past them; issued by the
/// session's CA, its key identifier the CA's.
#[test]
fn a_leaf_is_kept_for_its_name_the_last_256() {
    let ca = of_session(SESSION + 4).unwrap();
    let first = ca.leaf("a.test").unwrap();
    assert!(Arc::ptr_eq(&first, &ca.leaf("a.test").unwrap()));
    for n in 0..LEAVES_MAX {
        ca.leaf(&format!("n{n}.test")).unwrap();
    }
    assert!(!Arc::ptr_eq(&first, &ca.leaf("a.test").unwrap()), "past the last 256: minted again");
    let issuer = issuer::of(first.cert[0].as_ref()).unwrap();
    assert_eq!(issuer.cn.as_deref(), Some(CA_NAME));
    // (the leaf's AKI holds the CA's SKI bytes as they are)
    let ski = cert::tlv(0x80, &ca.ski).unwrap();
    assert!(first.cert[0].as_ref().windows(ski.len()).any(|w| w == ski), "its AKI is the CA's raw SKI");
    // (a name past a CN's 64: the SAN's alone, critical)
    let long = format!("{}.test", "x".repeat(70));
    assert!(ca.leaf(&long).is_ok());
    forget(SESSION + 4);
}

/// What a run terminates is said by its count alone — never its hosts — and a CA never says its key.
#[test]
fn what_a_run_terminates_is_said_by_its_count_alone() {
    let ca = of_session(SESSION + 5).unwrap();
    let hosts = vec![Pattern::parse("api.secret-host.test").unwrap()];
    let t = Terminate::new(hosts.clone(), ca.clone(), Vec::new());
    assert_eq!(format!("{t:?}"), "Terminate(1 hosts)");
    assert_eq!(format!("{ca:?}"), "Ca(…)");
    let e = super::super::egress::Egress { terminate: Some(t.clone()), ..Default::default() };
    assert!(!format!("{e:?}").contains("secret-host"), "{e:?}");
    assert!(t.names("api.secret-host.test", 443) && !t.names("api.secret-host.test", 8443));
    assert_eq!(e.terminates("api.secret-host.test", 443), Some(&t));
    assert_eq!(t, Terminate::new(hosts.clone(), ca.clone(), Vec::new()), "the same hosts and CA");
    forget(SESSION + 5);
    let other = of_session(SESSION + 5).unwrap();
    assert_ne!(t, Terminate::new(hosts, other, Vec::new()), "another CA");
    forget(SESSION + 5);
}
