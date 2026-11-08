//! TLS terminated at tarae's proxy (P1a of `.sb/credential-masking-plan.md`; `.sb/tls-p1a-step1.md` as the design
//! session answered it, 2026-11-08): for a host a run's rules name (`Egress.terminate`, CONNECT only), tarae's proxy
//! is the TLS server the run talks to — under a leaf its own CA minted for the CONNECT target — and the TLS client
//! of the host, verified as the system verifies (and `llm.sandbox-ca`'s roots), through the upstream proxy where
//! tarae itself goes out by one. Between the two legs the stage on the connection's thread (`stage`, P1b): each
//! request read and decided, each answer followed, the bytes as they came.
//!
//! The CA: one per session (`of_session`), ECDSA P-256, made in tarae's memory when a run's policy names a host to
//! terminate — its certificate at the head of that run's trust bundle (`cabundle`), and of every later one's. Its key
//! never leaves memory: nothing writes it and `Ca` has no method that gives it; not cleared when dropped, not locked
//! against swap (threat model G32). Valid 7 days, a new one once less than a day is left
//! (a run holding the old keeps it). Its leaves share one key and live a day (never past the CA), the last 256 names
//! kept.
//!
//! What a terminated tunnel never is: tarae's own fetch's (`Conn.ours`), nor a name the resolver wasn't made for —
//! the run's leg answers only the CONNECT target, a second ClientHello too.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ring::signature::{EcdsaKeyPair, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::sign::{CertifiedKey, SigningKey};

use super::bridge::{self, Out};
use super::egress::Pattern;
use super::upstream::{self, Upstream};
use super::{Conn, Limits, OP, UPSTREAM_OP, cert, issuer, parse, stage, tell_op};

/// A handshake's most time, either leg (srt's).
pub const HANDSHAKE: Duration = Duration::from_secs(10);
/// How long a CA is valid.
const CA_LIFE: i64 = 7 * 86_400;
/// A CA with less than this left is replaced for the next runs.
const CA_RENEW: i64 = 86_400;
/// How long a leaf is valid (never past its CA).
const LEAF_LIFE: i64 = 86_400;
/// A leaf with less than this left is minted again.
const LEAF_RENEW: i64 = 3600;
/// How far back validity starts: a VM's clock behind the host's.
const SKEW: i64 = 3600;
/// The leaves a CA keeps.
const LEAVES_MAX: usize = 256;
/// The CA's name.
const CA_NAME: &str = "tarae sandbox proxy CA";

/// Seconds since the epoch, now.
fn now_secs() -> i64 {
    crate::clock::system().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// A session's CA: its certificate, its key (never handed out), its leaves' one key and the leaves minted.
pub struct Ca {
    der: Vec<u8>,
    subject: Vec<u8>,
    key: EcdsaKeyPair,
    ski: [u8; 20],
    not_after: i64,
    leaf_key: Arc<dyn SigningKey>,
    leaf_public: Vec<u8>,
    leaves: Mutex<Leaves>,
    /// Tests: the keys' PKCS#8 and their scalars, to look for where they must never be (`the_ca_key_is_never_written`).
    #[cfg(test)]
    pub(crate) secrets: Vec<Vec<u8>>,
}

impl std::fmt::Debug for Ca {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ca(…)")
    }
}

/// The last `LEAVES_MAX` names' leaves, oldest first, each with its end.
#[derive(Default)]
struct Leaves {
    order: VecDeque<String>,
    by: HashMap<String, (Arc<CertifiedKey>, i64)>,
}

impl Leaves {
    fn get(&mut self, name: &str, now: i64) -> Option<Arc<CertifiedKey>> {
        let (k, end) = self.by.get(name)?;
        if end - now < LEAF_RENEW {
            return None;
        }
        let k = k.clone();
        self.order.retain(|n| n != name);
        self.order.push_back(name.to_string());
        Some(k)
    }

    fn put(&mut self, name: &str, k: Arc<CertifiedKey>, end: i64) {
        self.order.retain(|n| n != name);
        self.order.push_back(name.to_string());
        self.by.insert(name.to_string(), (k, end));
        while self.order.len() > LEAVES_MAX {
            if let Some(old) = self.order.pop_front() {
                self.by.remove(&old);
            }
        }
    }
}

impl Ca {
    /// A new CA, valid from `now`, with its leaves' key.
    fn new(now: i64) -> Result<Ca, String> {
        #[cfg(test)]
        let mut secrets = Vec::new();
        let (der, subject, key, ski) = cert::with_new_key(|pkcs8, key| {
            #[cfg(test)]
            secrets.extend([pkcs8.to_vec(), cert::scalar(pkcs8).unwrap_or_default()]);
            let _ = pkcs8;
            let public = key.public_key().as_ref().to_vec();
            let ski = cert::key_id(&public);
            let subject = cert::name(CA_NAME)?;
            let spec = cert::Spec {
                serial: cert::serial()?,
                subject: subject.clone(),
                issuer: subject.clone(),
                not_before: now - SKEW,
                not_after: now + CA_LIFE,
                public: &public,
                extensions: cert::ca_extensions(&ski, Some(0))?,
            };
            Ok((cert::signed(&spec, &key)?, subject, key, ski))
        })?;
        let (leaf_key, leaf_public) = cert::with_new_key(|pkcs8, pair| {
            #[cfg(test)]
            secrets.extend([pkcs8.to_vec(), cert::scalar(pkcs8).unwrap_or_default()]);
            let der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(pkcs8));
            let signer = rustls::crypto::ring::sign::any_ecdsa_type(&der)
                .map_err(|e| format!("the leaves' key: {e}"))?;
            Ok((signer, pair.public_key().as_ref().to_vec()))
        })?;
        Ok(Ca {
            der,
            subject,
            key,
            ski,
            not_after: now + CA_LIFE,
            leaf_key,
            leaf_public,
            leaves: Mutex::new(Leaves::default()),
            #[cfg(test)]
            secrets,
        })
    }

    /// Its certificate, DER — what a run's trust bundle carries.
    pub fn der(&self) -> &[u8] {
        &self.der
    }

    /// The leaf for `name` (the CONNECT target, as `parse::name` made it): kept, else minted.
    pub fn leaf(&self, name: &str) -> Result<Arc<CertifiedKey>, String> {
        let now = now_secs();
        let mut leaves = self.leaves.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(k) = leaves.get(name, now) {
            return Ok(k);
        }
        let end = (now + LEAF_LIFE).min(self.not_after);
        // (a CN holds 64 at most, RFC 5280's bound: a longer name is the SAN's alone, the subject empty)
        let short = name.len() <= 64;
        let spec = cert::Spec {
            serial: cert::serial()?,
            subject: if short { cert::name(name)? } else { cert::seq(&[])? },
            issuer: self.subject.clone(),
            not_before: now - SKEW,
            not_after: end,
            public: &self.leaf_public,
            extensions: cert::leaf_extensions(&[name], &self.ski, !short)?,
        };
        let der = cert::signed(&spec, &self.key)?;
        let k = Arc::new(CertifiedKey::new(vec![CertificateDer::from(der)], self.leaf_key.clone()));
        leaves.put(name, k.clone(), end);
        Ok(k)
    }
}

/// The sessions' CAs (the proxy's session: a checkout's, `workspace_of`).
static CAS: Mutex<Vec<(u64, Arc<Ca>)>> = Mutex::new(Vec::new());

/// Session `session`'s CA — made the first time, made anew once less than `CA_RENEW` is left. Signs: a worker's (a
/// run's policy is made on one).
pub fn of_session(session: u64) -> Result<Arc<Ca>, String> {
    let now = now_secs();
    let mut cas = CAS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, ca)) = cas.iter().find(|(s, _)| *s == session)
        && ca.not_after - now >= CA_RENEW
    {
        return Ok(ca.clone());
    }
    let ca = Arc::new(Ca::new(now)?);
    cas.retain(|(s, _)| *s != session);
    cas.push((session, ca.clone()));
    Ok(ca)
}

/// The session ended: its CA goes (runs still holding it keep theirs).
pub fn forget(session: u64) {
    CAS.lock().unwrap_or_else(|e| e.into_inner()).retain(|(s, _)| *s != session);
}

/// What a run's rules terminate: the hosts, the session's CA its leaves come from, and `llm.sandbox-ca`'s files (the
/// host's certificate is checked against their roots too, the upstream proxy's or none). Made only where a run's
/// policy is (`sandbox::for_its_run`).
#[derive(Clone)]
pub struct Terminate {
    hosts: Vec<Pattern>,
    ca: Arc<Ca>,
    ca_files: Vec<PathBuf>,
    /// Tests: the roots the host's leg trusts besides `ca_files` (the "public" CA, as `Here.system_roots` stands
    /// for the system's in a bundle) — None: the system's own verifier.
    #[cfg(test)]
    pub(crate) system: Option<Vec<Vec<u8>>>,
}

/// The count of its hosts only — a report or a log never lists what a run terminates from here.
impl std::fmt::Debug for Terminate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Terminate({} hosts)", self.hosts.len())
    }
}

impl PartialEq for Terminate {
    fn eq(&self, o: &Terminate) -> bool {
        self.hosts == o.hosts && Arc::ptr_eq(&self.ca, &o.ca) && self.ca_files == o.ca_files
    }
}

impl Eq for Terminate {}

impl Terminate {
    pub fn new(hosts: Vec<Pattern>, ca: Arc<Ca>, ca_files: Vec<PathBuf>) -> Terminate {
        Terminate {
            hosts,
            ca,
            ca_files,
            #[cfg(test)]
            system: None,
        }
    }

    /// Whether it names `host:port` (a CONNECT's: 443 its default).
    pub fn names(&self, host: &str, port: u16) -> bool {
        self.hosts.iter().any(|p| p.matches(host, port, 443))
    }

    #[cfg(test)]
    pub(crate) fn ca(&self) -> &Arc<Ca> {
        &self.ca
    }
}

// --- The host's leg: its TLS client ---

thread_local! {
    /// The certificate the last failed verify on this thread saw — the leg's handshake runs on the connection's
    /// thread, and its failure names that certificate's issuer (`issuer_said`).
    static SHOWN: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// A verifier that remembers the certificate it refused (`SHOWN`), its answer the inner one's.
#[derive(Debug)]
struct Noting(Arc<dyn rustls::client::danger::ServerCertVerifier>);

impl rustls::client::danger::ServerCertVerifier for Noting {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let r = self.0.verify_server_cert(end_entity, intermediates, server_name, ocsp, now);
        if r.is_err() {
            SHOWN.with(|s| *s.borrow_mut() = Some(end_entity.to_vec()));
        }
        r
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

fn ring() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The host's leg's TLS: rustls on ring, http/1.1, verified as the system verifies with `t`'s `llm.sandbox-ca` roots
/// besides (`Noting` around it). Made again only when those files change (as `upstream::tls`). Tests: `t.system`'s
/// roots and the files' by webpki where given, else the system's verifier.
fn client_config(t: &Terminate) -> Result<Arc<rustls::ClientConfig>, String> {
    type Made = (Vec<(PathBuf, Option<std::time::SystemTime>)>, Result<Arc<rustls::ClientConfig>, String>);
    static TLS: Mutex<Option<Made>> = Mutex::new(None);
    let stamps: Vec<_> = t
        .ca_files
        .iter()
        .map(|f| (f.clone(), std::fs::metadata(f).and_then(|m| m.modified()).ok()))
        .collect();
    #[cfg(test)]
    let kept_ok = t.system.is_none();
    #[cfg(not(test))]
    let kept_ok = true;
    let mut kept = TLS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((was, made)) = kept.as_ref()
        && *was == stamps
        && kept_ok
    {
        return made.clone();
    }
    let (roots, missed) = crate::sandbox::cabundle::roots(&t.ca_files);
    for m in &missed {
        crate::log::line(
            "sandbox",
            "warning",
            &format!("llm.sandbox-ca, for a terminated host's certificate: {m}"),
        );
    }
    let made = verifier(t, roots).and_then(|v| {
        let mut c = rustls::ClientConfig::builder_with_provider(ring())
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("TLS: {e}"))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Noting(v)))
            .with_no_client_auth();
        c.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(c))
    });
    if kept_ok {
        *kept = Some((stamps, made.clone()));
    }
    made
}

/// The verifier the host's leg checks with (`client_config`).
fn verifier(
    t: &Terminate,
    roots: Vec<CertificateDer<'static>>,
) -> Result<Arc<dyn rustls::client::danger::ServerCertVerifier>, String> {
    #[cfg(test)]
    if let Some(system) = &t.system {
        let mut store = rustls::RootCertStore::empty();
        for r in system.iter().map(|d| CertificateDer::from(d.clone())).chain(roots) {
            let _ = store.add(r);
        }
        let v = rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(store), ring())
            .build()
            .map_err(|e| format!("the test's roots: {e}"))?;
        return Ok(v);
    }
    let _ = t;
    let v = match roots.is_empty() {
        true => rustls_platform_verifier::Verifier::new(ring()),
        false => rustls_platform_verifier::Verifier::new_with_extra_roots(roots, ring()),
    };
    Ok(Arc::new(v.map_err(|e| format!("the system's certificates: {e}"))?))
}

/// The run's leg's TLS for one tunnel: its certificate the CONNECT target's leaf, answered for that name alone
/// (`Only`); http/1.1; no resumption, no tickets, no early data.
fn server_config(name: &str, leaf: Arc<CertifiedKey>) -> Result<Arc<rustls::ServerConfig>, String> {
    let mut c = rustls::ServerConfig::builder_with_provider(ring())
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("TLS: {e}"))?
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(Only { name: name.to_string(), leaf }));
    c.alpn_protocols = vec![b"http/1.1".to_vec()];
    c.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    c.send_tls13_tickets = 0;
    Ok(Arc::new(c))
}

/// A resolver for one name: any other server name — none, or a second ClientHello's after a HelloRetryRequest that
/// names another — gets no certificate, and the handshake fails.
#[derive(Debug)]
struct Only {
    name: String,
    leaf: Arc<CertifiedKey>,
}

impl rustls::server::ResolvesServerCert for Only {
    fn resolve(&self, hello: rustls::server::ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        hello.server_name().filter(|n| n.eq_ignore_ascii_case(&self.name)).map(|_| self.leaf.clone())
    }
}

// --- The handshakes ---

/// How a handshake failed.
#[derive(Debug)]
enum Shake {
    /// Not done within its time.
    Late,
    /// The other side closed.
    Closed,
    Tls(rustls::Error),
    Io(std::io::Error),
}

fn io_shake(e: std::io::Error) -> Shake {
    match e.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => Shake::Late,
        _ => Shake::Io(e),
    }
}

/// `io`'s reads and writes to time out at `by` — Err once it's past.
fn timed(io: &Out, by: Instant) -> Result<(), Shake> {
    let left = by.saturating_duration_since(crate::clock::now());
    if left.is_zero() {
        return Err(Shake::Late);
    }
    io.set_timeouts(Some(left)).map_err(Shake::Io)
}

/// `bytes` given to `conn` as if read (the ClientHello the proxy read already).
fn feed(conn: &mut rustls::Connection, io: &Out, mut bytes: &[u8]) -> Result<(), Shake> {
    while !bytes.is_empty() {
        match conn.read_tls(&mut bytes) {
            Ok(0) => break,
            Ok(_) => processed(conn, io)?,
            Err(e) => return Err(Shake::Io(e)),
        }
    }
    Ok(())
}

/// What `conn` read processed — a failure's alert sent where it can be.
fn processed(conn: &mut rustls::Connection, io: &Out) -> Result<(), Shake> {
    if let Err(e) = conn.process_new_packets() {
        let _ = conn.write_tls(&mut &*io);
        return Err(Shake::Tls(e));
    }
    Ok(())
}

/// `conn`'s handshake over `io`, done by `by` (blocking, each read and write timed to what's left).
fn shake(conn: &mut rustls::Connection, io: &Out, by: Instant) -> Result<(), Shake> {
    loop {
        while conn.wants_write() {
            timed(io, by)?;
            conn.write_tls(&mut &*io).map_err(io_shake)?;
        }
        if !conn.is_handshaking() {
            return Ok(());
        }
        timed(io, by)?;
        match conn.read_tls(&mut &*io) {
            Ok(0) => return Err(Shake::Closed),
            Ok(_) => processed(conn, io)?,
            Err(e) => return Err(io_shake(e)),
        }
    }
}

/// The issuer of the certificate the host's leg refused, in words: its O and CN, framed as its own (`issuer`) —
/// neither there, its SHA-256's start.
fn issuer_said() -> Option<String> {
    let der = SHOWN.with(|s| s.borrow_mut().take())?;
    Some(issuer::of(&der).and_then(|i| i.said()).unwrap_or_else(|| {
        let hash = crate::sha256::hex(&der);
        format!("its issuer names no O or CN; the certificate's SHA-256 begins {}", &hash[..16])
    }))
}

/// Why the host's leg failed, in a report's words (after `host:port: `), and its op: the upstream's where it went
/// through one (no answer of the user's lifts it), else the proxy's.
fn host_leg_said(e: &Shake, through: Option<&Upstream>) -> (&'static str, String) {
    use rustls::{CertificateError as C, Error as E};
    let by = issuer_said().map(|s| format!(" ({s})")).unwrap_or_default();
    let via = through.map(|u| format!(" through tarae's {}", u.var)).unwrap_or_default();
    let op = if through.is_some() { UPSTREAM_OP } else { OP };
    let said = match e {
        Shake::Tls(E::InvalidCertificate(C::UnknownIssuer | C::BadSignature)) => match through {
            Some(_) => format!(
                "the certificate it showed{via} isn't one this system trusts{by} — if that proxy re-signs TLS, add \
                 its root to llm.sandbox-ca"
            ),
            None => format!(
                "the certificate it showed isn't one this system trusts{by} — a company's root goes in the system's \
                 store, or in llm.sandbox-ca"
            ),
        },
        Shake::Tls(e) => format!("TLS to it{via}: {}{by}", upstream::tls_said(e)),
        Shake::Late => format!("no whole TLS handshake with it{via} within {} s", HANDSHAKE.as_secs()),
        Shake::Closed => format!("it closed during the TLS handshake{via}"),
        Shake::Io(e) => format!("the TLS handshake with it{via} failed ({})", e.kind()),
    };
    (op, said)
}

/// Why the run's leg failed, in a report's words (after `host:port: `).
fn run_leg_said(e: &Shake) -> String {
    use rustls::{AlertDescription as A, Error as E};
    match e {
        Shake::Tls(E::AlertReceived(A::UnknownCA | A::BadCertificate | A::CertificateUnknown)) => {
            "the run's TLS client refused tarae's certificate — it doesn't read the run's trust bundle \
             (SSL_CERT_FILE and the rest)"
                .into()
        }
        Shake::Tls(E::General(_) | E::NoCertificatesPresented) | Shake::Tls(E::PeerIncompatible(_)) => {
            "the run's ClientHello named another host, or nothing tarae's leaf could answer".into()
        }
        Shake::Tls(e) => format!("the run's TLS handshake with tarae failed: {}", upstream::tls_said(e)),
        Shake::Late => format!("no whole TLS handshake from the run within {} s", HANDSHAKE.as_secs()),
        Shake::Closed => "the run closed during its TLS handshake with tarae".into(),
        Shake::Io(e) => format!("the run's TLS handshake with tarae failed ({})", e.kind()),
    }
}

/// A terminated tunnel, after its `200` and its ClientHello (`hello`, its name the CONNECT target's): the host's leg
/// first over `up` (the run's ClientHello waits), then the run's under tarae's leaf for `name`; both up, a bridge
/// each and the stage between them (`stage::run`). The host's leg failed: the run's handshake still finishes,
/// its request head is read and answered 502 inside TLS with why — the report says the same.
#[allow(clippy::too_many_arguments)]
pub(super) fn serve(
    c: &Conn,
    t: &Terminate,
    client: TcpStream,
    hello: &[u8],
    up: Out,
    through: Option<&Upstream>,
    name: &str,
    port: u16,
    limits: Limits,
) {
    let tell = |op: &str, why: &str| tell_op(c, op, &format!("{name}:{port}: {why}"));
    let by = crate::clock::now() + HANDSHAKE;
    // The host's leg, identity pinned to the CONNECT target (never a Host header)
    let host_leg = (|| {
        let config = client_config(t).map_err(|e| (OP, e))?;
        let server =
            ServerName::try_from(name.to_string()).map_err(|_| (OP, "a name TLS doesn't take".into()))?;
        let cc = rustls::ClientConnection::new(config, server).map_err(|e| (OP, e.to_string()))?;
        let mut conn = rustls::Connection::Client(cc);
        let _ = up.set_nonblocking(false);
        shake(&mut conn, &up, by).map_err(|e| host_leg_said(&e, through))?;
        let _ = up.set_timeouts(None);
        Ok::<_, (&str, String)>(conn)
    })();
    // The run's leg, under the leaf for the CONNECT target alone
    let run = Out::Tcp(client);
    let leaf = match t.ca.leaf(name) {
        Ok(l) => l,
        Err(e) => {
            return tell(
                OP,
                &format!("tarae's certificate for it couldn't be made ({e}) — the tunnel closed"),
            );
        }
    };
    let sc = match server_config(name, leaf)
        .and_then(|s| rustls::ServerConnection::new(s).map_err(|e| e.to_string()))
    {
        Ok(s) => s,
        Err(e) => return tell(OP, &format!("tarae's TLS for it couldn't start ({e}) — the tunnel closed")),
    };
    let mut sc = rustls::Connection::Server(sc);
    let _ = run.set_nonblocking(false);
    if let Err(e) = feed(&mut sc, &run, hello).and_then(|()| shake(&mut sc, &run, by)) {
        // (the host's failure first: it's why there's nothing to give the run)
        if let Err((op, why)) = &host_leg {
            return tell(op, &format!("{why} — the tunnel closed"));
        }
        return tell(OP, &format!("{} — the tunnel closed", run_leg_said(&e)));
    }
    let _ = run.set_timeouts(None);
    let host = match host_leg {
        Ok(h) => h,
        Err((op, why)) => {
            let body = format!("{name}:{port}: {why}");
            refused_inside(&mut sc, &run, crate::clock::now() + limits.head_time, &body);
            return tell(op, &format!("{why} — answered 502"));
        }
    };
    // (the host's end with or without close_notify told apart — a close-delimited answer is whole only by it; the
    // run's requests are always framed, `parse::origin`: its end is its close either way)
    let (run_plain, (host_plain, ending)) = match (bridge::start(sc, run), bridge::start_leg(host, up)) {
        (Ok(r), Ok(h)) => (r, h),
        _ => return tell(OP, "no thread for its TLS — the tunnel closed"),
    };
    // The stage: each request read and decided, each answer followed — never spliced (tarae's plaintext, which P2
    // swaps in)
    stage::run([run_plain, host_plain], &ending, name, port, &limits, |why| tell(OP, why));
}

/// The run's request head read (to its blank line, `parse::HEAD_MAX` at most, by `by`), then a 502 saying `why`
/// inside its TLS, and close_notify — a refusal the run's own client reads (its 200 was sent already).
fn refused_inside(conn: &mut rustls::Connection, io: &Out, by: Instant, why: &str) {
    let (mut head, mut b) = (Vec::new(), [0u8; 4096]);
    while head.len() < parse::HEAD_MAX && !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match conn.reader().read(&mut b) {
            Ok(0) => break,
            Ok(n) => {
                head.extend_from_slice(&b[..n]);
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        if timed(io, by).is_err() {
            break;
        }
        match conn.read_tls(&mut &*io) {
            Ok(0) | Err(_) => break,
            Ok(_) if processed(conn, io).is_err() => return,
            Ok(_) => {}
        }
    }
    let _ = conn.writer().write_all(&super::answer_bytes(502, why, ""));
    conn.send_close_notify();
    let by = crate::clock::now() + HANDSHAKE;
    while conn.wants_write() && timed(io, by).is_ok() {
        if conn.write_tls(&mut &*io).is_err() {
            break;
        }
    }
}

#[cfg(test)]
#[path = "terminate/tests.rs"]
mod tests;
