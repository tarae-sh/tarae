use std::collections::HashMap;

use super::*;
use crate::web::fetch::tests::authority;

/// `text` in file `name` of `dir`.
fn put(dir: &Path, name: &str, text: &str) -> PathBuf {
    let f = dir.join(name);
    std::fs::write(&f, text).unwrap();
    f
}

/// A key's body (a test's own bytes, as base64).
const KEY_BODY: &[u8] = b"tarae-fake-key-never-in-a-bundle";

/// A private key's block, as a CA variable's file may hold one beside its certificate.
fn key() -> String {
    let body = crate::sandbox::proxy::parse::base64(KEY_BODY);
    format!("-----BEGIN PRIVATE KEY-----\n{body}\n-----END PRIVATE KEY-----\n")
}

/// The bundle holds the system's roots, then your CA variables' certificates, then `llm.sandbox-ca`'s — each once,
/// counted for the source that gave it first; what can't be read is said with why, the rest still made.
#[test]
fn the_bundle_is_the_systems_then_your_variables_then_your_settings() {
    let scratch = crate::scratch::Scratch::new("cabundle-order");
    let (sys, company, extra) = (authority("sys"), authority("company"), authority("extra"));
    let var = put(&scratch, "company.pem", &format!("{}{}", pem(&company.der), pem(&sys.der)));
    let yours = put(&scratch, "extra.pem", &format!("{}{}", pem(&extra.der), pem(&company.der)));
    let s = Sources {
        vars: vec![("SSL_CERT_FILE".into(), var)],
        yours: vec![
            Ok(yours),
            Ok(scratch.join("gone.pem")),
            Err("x.pem (not an absolute path, nor one from ~/)".into()),
        ],
        system: vec![sys.der.clone()],
        system_fails: None,
    };
    let m = made(&s);
    assert_eq!(m.pem, [pem(&sys.der), pem(&company.der), pem(&extra.der)].concat());
    assert_eq!((m.system, m.vars, m.yours, m.cut), (1, 1, 1, 0));
    assert_eq!(m.missed.len(), 2, "{:?}", m.missed);
    assert!(
        m.missed[0].starts_with("llm.sandbox-ca's ")
            && m.missed[0].ends_with("gone.pem: No such file or directory (os error 2)")
    );
    assert_eq!(m.missed[1], "llm.sandbox-ca's x.pem (not an absolute path, nor one from ~/)");
    let said = m.said();
    assert!(
        said.starts_with("TLS: your commands trust the system's roots + 1 from your CA variables + 1 from llm.sandbox-ca — "),
        "{said}"
    );
    assert!(said.contains("Not read: llm.sandbox-ca's "), "{said}");
}

/// srt's rule: a CA variable's file holding a private key beside its certificate gives the bundle the certificate
/// alone — the key never reaches it.
#[test]
fn a_key_beside_a_certificate_never_reaches_the_bundle() {
    let scratch = crate::scratch::Scratch::new("cabundle-key");
    let company = authority("company");
    let both = put(&scratch, "both.pem", &format!("{}{}", key(), pem(&company.der)));
    let alone = put(&scratch, "key.pem", &key());
    let s = Sources {
        vars: vec![("SSL_CERT_FILE".into(), both), ("NODE_EXTRA_CA_CERTS".into(), alone)],
        ..Sources::default()
    };
    let m = made(&s);
    assert_eq!(m.pem, pem(&company.der));
    let body = crate::sandbox::proxy::parse::base64(KEY_BODY);
    assert!(!m.pem.contains("PRIVATE") && !m.pem.contains(&body), "{}", m.pem);
    assert_eq!(m.vars, 1);
    assert_eq!(m.missed.len(), 1, "{:?}", m.missed);
    assert!(
        m.missed[0].starts_with("NODE_EXTRA_CA_CERTS's ") && m.missed[0].ends_with(": holds no certificate")
    );
}

/// A file the bundle can't take whole is said, never waited on: a FIFO isn't opened, a file past `FILE_MAX` isn't read.
#[test]
fn what_a_bundle_cant_read_is_said_never_waited_on() {
    let scratch = crate::scratch::Scratch::new("cabundle-odd");
    let fifo = scratch.join("fifo.pem");
    let c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: a path of the test's own scratch folder, live for the call.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let big = scratch.join("big.pem");
    std::fs::File::create(&big).unwrap().set_len(FILE_MAX + 1).unwrap();
    let s = Sources { yours: vec![Ok(fifo), Ok(big)], ..Sources::default() };
    let m = made(&s);
    assert!(m.pem.is_empty());
    assert!(m.missed[0].ends_with("fifo.pem: not a plain file"), "{:?}", m.missed);
    assert!(m.missed[1].ends_with("big.pem: past 4 MB"), "{:?}", m.missed);
}

/// At most `CERTS_MAX` certificates: the rest left out, counted and said.
#[test]
fn a_bundle_holds_at_most_its_limit() {
    let system: Vec<Vec<u8>> = (0..CERTS_MAX + 3).map(|i| format!("stand-in {i}").into_bytes()).collect();
    let m = made(&Sources { system, ..Sources::default() });
    assert_eq!((m.system, m.cut), (CERTS_MAX, 3));
    assert!(m.said().contains(" 3 certificates past 2000 were left out."), "{}", m.said());
}

/// What a policy reads a bundle from: the packs' CA variables in tarae's environment (each file once), and
/// `llm.sandbox-ca` — `~/` from home, an absolute path as it is, anything else said; the system's roots only as a
/// test gives them (`Here.system_roots`), never the store's.
#[test]
fn the_sources_are_your_variables_and_your_setting() {
    let env = HashMap::from([
        ("HOME".to_string(), "/h".into()),
        ("SSL_CERT_FILE".to_string(), "/corp/root.pem".into()),
        ("CURL_CA_BUNDLE".to_string(), "/corp/root.pem".into()),
        ("NODE_EXTRA_CA_CERTS".to_string(), "/corp/node.pem".into()),
    ]);
    let here = Here { env, ..Here::default() };
    let s = Sources::of(&here, &["~/ca/a.pem".into(), "/etc/b.pem".into(), "c.pem".into()]);
    // (in the packs' order; `CURL_CA_BUNDLE`'s file is `SSL_CERT_FILE`'s: once)
    let mut vars: Vec<(&str, &Path)> = s.vars.iter().map(|(k, f)| (k.as_str(), f.as_path())).collect();
    vars.sort_unstable();
    assert_eq!(
        vars,
        [
            ("NODE_EXTRA_CA_CERTS", Path::new("/corp/node.pem")),
            ("SSL_CERT_FILE", Path::new("/corp/root.pem"))
        ]
    );
    assert_eq!(
        s.yours,
        [
            Ok(PathBuf::from("/h/ca/a.pem")),
            Ok(PathBuf::from("/etc/b.pem")),
            Err("c.pem (not an absolute path, nor one from ~/)".into())
        ]
    );
    assert!(s.system.is_empty(), "a test's bundle never starts from the real store");
}

/// The seam's real side: the system's roots are read (`rustls-native-certs` — macOS's trust settings, which never
/// ask; a distro's bundle) — the one load of the real store in tarae's tests. (macOS: not inside a sandbox, which
/// keeps the keychain's services from its runs.)
#[test]
fn the_systems_roots_are_read() {
    #[cfg(target_os = "macos")]
    if crate::sandbox::skipped_inside_for(
        "sandbox::cabundle::tests::the_systems_roots_are_read",
        "a sandbox keeps the keychain's services from its runs",
    ) {
        return;
    }
    let roots = system_read().expect("the system's roots");
    assert!(roots.len() >= 10, "{} roots", roots.len());
}
