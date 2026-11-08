//! Certificates laid out in DER by hand (P1a of `.sb/credential-masking-plan.md`): tarae's proxy CA and the leaves
//! it mints for a terminated tunnel (`terminate`), and the tests' own CAs and leaves (`web::fetch::tests`) — ECDSA
//! P-256 signed by ring, the one key type ring both makes and signs with. What a strict checker asks of them is laid
//! in (OpenSSL 3's `-x509_strict`, Python 3.13's `VERIFY_X509_STRICT`): a CA's basicConstraints and keyUsage
//! critical, its subject key identifier; a leaf's authority key identifier holding the CA's raw SKI bytes (srt's
//! bug: forge wrote its hex as text, and the chain broke); serials positive, encoded at their least.

use ring::signature::EcdsaKeyPair;

/// The most a TLV's body may be here (a length in at most 3 bytes past its first): more is refused, never cut.
const BODY_MAX: usize = 0xff_ffff;

/// A DER TLV: `tag`, its length, `body` — Err past `BODY_MAX`.
pub fn tlv(tag: u8, body: &[u8]) -> Result<Vec<u8>, String> {
    let n = body.len();
    let mut out = vec![tag];
    match n {
        0..128 => out.push(n as u8),
        128..256 => out.extend_from_slice(&[0x81, n as u8]),
        256..65_536 => out.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]),
        _ if n <= BODY_MAX => out.extend_from_slice(&[0x83, (n >> 16) as u8, (n >> 8) as u8, n as u8]),
        _ => return Err(format!("a DER value of {n} bytes — past what's laid out here")),
    }
    out.extend_from_slice(body);
    Ok(out)
}

/// A SEQUENCE of `parts`.
pub fn seq(parts: &[&[u8]]) -> Result<Vec<u8>, String> {
    tlv(0x30, &parts.concat())
}

pub const EC_KEY: &[u8] = &[0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
pub const P256: &[u8] = &[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
pub const ECDSA_SHA256: &[u8] = &[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
pub const COMMON_NAME: &[u8] = &[0x06, 0x03, 0x55, 0x04, 0x03];
pub const SUBJECT_KEY_ID: &[u8] = &[0x06, 0x03, 0x55, 0x1d, 0x0e];
pub const KEY_USAGE: &[u8] = &[0x06, 0x03, 0x55, 0x1d, 0x0f];
pub const ALT_NAMES: &[u8] = &[0x06, 0x03, 0x55, 0x1d, 0x11];
pub const BASIC_CONSTRAINTS: &[u8] = &[0x06, 0x03, 0x55, 0x1d, 0x13];
pub const AUTHORITY_KEY_ID: &[u8] = &[0x06, 0x03, 0x55, 0x1d, 0x23];
pub const KEY_PURPOSES: &[u8] = &[0x06, 0x03, 0x55, 0x1d, 0x25];
pub const SERVER_AUTH: &[u8] = &[0x06, 0x08, 0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01];

/// A Name of one RDN, its CN `cn` (UTF8String).
pub fn name(cn: &str) -> Result<Vec<u8>, String> {
    seq(&[&tlv(0x31, &seq(&[COMMON_NAME, &tlv(0x0c, cn.as_bytes())?])?)?])
}

/// An extension: its OID, critical or not (DER leaves a FALSE out), its value's DER.
pub fn extension(oid: &[u8], critical: bool, value: &[u8]) -> Result<Vec<u8>, String> {
    let flag = if critical { tlv(0x01, &[0xff])? } else { Vec::new() };
    seq(&[oid, &flag, &tlv(0x04, value)?])
}

/// A CA's extensions: basicConstraints `CA:TRUE` (`pathlen`, where given), critical; keyUsage `keyCertSign |
/// cRLSign`, critical; its subject key identifier `ski`.
pub fn ca_extensions(ski: &[u8], pathlen: Option<u8>) -> Result<Vec<Vec<u8>>, String> {
    let len = match pathlen {
        Some(n) => tlv(0x02, &[n])?,
        None => Vec::new(),
    };
    Ok(vec![
        extension(BASIC_CONSTRAINTS, true, &seq(&[&tlv(0x01, &[0xff])?, &len])?)?,
        // (a BIT STRING: bit 5 keyCertSign, bit 6 cRLSign — 0x06, its last bit unused)
        extension(KEY_USAGE, true, &[0x03, 0x02, 0x01, 0x06])?,
        extension(SUBJECT_KEY_ID, false, &tlv(0x04, ski)?)?,
    ])
}

/// A server leaf's extensions: basicConstraints `CA:FALSE` (an empty SEQUENCE: DER leaves the FALSE out);
/// keyUsage `digitalSignature`, critical; serverAuth; its DNS `names`; its issuer's key identifier `aki` (the CA's
/// raw SKI bytes). `alt_critical`: the names are critical — a leaf with an empty subject (RFC 5280 §4.2.1.6).
pub fn leaf_extensions(names: &[&str], aki: &[u8], alt_critical: bool) -> Result<Vec<Vec<u8>>, String> {
    let alt = names.iter().map(|n| tlv(0x82, n.as_bytes())).collect::<Result<Vec<_>, _>>()?;
    Ok(vec![
        extension(BASIC_CONSTRAINTS, false, &seq(&[])?)?,
        // (bit 0 digitalSignature — 0x80, its 7 last bits unused)
        extension(KEY_USAGE, true, &[0x03, 0x02, 0x07, 0x80])?,
        extension(KEY_PURPOSES, false, &seq(&[SERVER_AUTH])?)?,
        extension(ALT_NAMES, alt_critical, &seq(&alt.iter().map(Vec::as_slice).collect::<Vec<_>>())?)?,
        extension(AUTHORITY_KEY_ID, false, &seq(&[&tlv(0x80, aki)?])?)?,
    ])
}

/// A key identifier: SHA-1 of a public key's bits (RFC 5280 §4.2.1.2's first way).
pub fn key_id(public: &[u8]) -> [u8; 20] {
    let d = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, public);
    let mut id = [0u8; 20];
    id.copy_from_slice(d.as_ref());
    id
}

/// A serial: 16 random bytes, positive and at its least in DER (its first byte neither 0 nor past 0x7f).
pub fn serial() -> Result<[u8; 16], String> {
    let mut s = [0u8; 16];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut s)
        .map_err(|_| "no random bytes for a serial".to_string())?;
    s[0] = (s[0] & 0x7f).max(1);
    Ok(s)
}

/// A certificate time: UTCTime through 2049, GeneralizedTime after (RFC 5280 §4.1.2.5), of seconds since the epoch.
pub fn time(secs: i64) -> Result<Vec<u8>, String> {
    let (y, mo, d) = crate::log::civil(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    let (h, mi, se) = (s / 3600, s / 60 % 60, s % 60);
    match y {
        1950..=2049 => tlv(0x17, format!("{:02}{mo:02}{d:02}{h:02}{mi:02}{se:02}Z", y % 100).as_bytes()),
        2050..=9999 => tlv(0x18, format!("{y:04}{mo:02}{d:02}{h:02}{mi:02}{se:02}Z").as_bytes()),
        _ => Err(format!("the year {y} — not a certificate's")),
    }
}

/// What a certificate says: its serial, its subject's and issuer's Names (DER), when it's valid (seconds since the
/// epoch), its public key (an uncompressed P-256 point), its extensions.
pub struct Spec<'a> {
    pub serial: [u8; 16],
    pub subject: Vec<u8>,
    pub issuer: Vec<u8>,
    pub not_before: i64,
    pub not_after: i64,
    pub public: &'a [u8],
    pub extensions: Vec<Vec<u8>>,
}

/// The v3 certificate `s` says, signed with `signer` (ECDSA P-256 with SHA-256).
pub fn signed(s: &Spec<'_>, signer: &EcdsaKeyPair) -> Result<Vec<u8>, String> {
    let spki = seq(&[&seq(&[EC_KEY, P256])?, &tlv(0x03, &[&[0][..], s.public].concat())?])?;
    let validity = seq(&[&time(s.not_before)?, &time(s.not_after)?])?;
    let tbs = seq(&[
        &tlv(0xa0, &tlv(0x02, &[2])?)?,
        &tlv(0x02, &s.serial)?,
        &seq(&[ECDSA_SHA256])?,
        &s.issuer,
        &validity,
        &s.subject,
        &spki,
        &tlv(0xa3, &seq(&s.extensions.iter().map(Vec::as_slice).collect::<Vec<_>>())?)?,
    ])?;
    let signature = signer
        .sign(&ring::rand::SystemRandom::new(), &tbs)
        .map_err(|_| "a certificate couldn't be signed".to_string())?;
    seq(&[&tbs, &seq(&[ECDSA_SHA256])?, &tlv(0x03, &[&[0][..], signature.as_ref()].concat())?])
}

/// A new P-256 key: what `use_it` makes of it (it's handed the PKCS#8 and the pair parsed from it). The PKCS#8 is
/// dropped as it returns, not cleared: the key lives on in the pair for the CA's life anyway (threat model G32), and
/// clearing ring's `Document` would lean on its private layout.
pub fn with_new_key<T>(use_it: impl FnOnce(&[u8], EcdsaKeyPair) -> Result<T, String>) -> Result<T, String> {
    use ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING as ALG;
    let rng = ring::rand::SystemRandom::new();
    let doc = EcdsaKeyPair::generate_pkcs8(&ALG, &rng).map_err(|_| "no new key".to_string())?;
    EcdsaKeyPair::from_pkcs8(&ALG, doc.as_ref(), &rng)
        .map_err(|_| "a new key that doesn't parse".to_string())
        .and_then(|pair| use_it(doc.as_ref(), pair))
}

/// The 32-byte private scalar inside one of ring's P-256 PKCS#8 documents (RFC 5915's `privateKey`, after
/// `version 1`) — tests look for it on disk.
#[cfg(test)]
pub fn scalar(pkcs8: &[u8]) -> Option<Vec<u8>> {
    let at = pkcs8.windows(5).position(|w| w == [0x02, 0x01, 0x01, 0x04, 0x20])? + 5;
    pkcs8.get(at..at + 32).map(<[u8]>::to_vec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tlv_past_its_most_is_refused_never_cut() {
        assert_eq!(tlv(0x04, &[7; 70_000]).unwrap()[..4], [0x04, 0x83, 0x01, 0x11]);
        assert_eq!(tlv(0x04, &[7; 300]).unwrap()[..4], [0x04, 0x82, 0x01, 0x2c]);
        assert!(tlv(0x04, &vec![0; BODY_MAX + 1]).is_err());
    }

    #[test]
    fn times_are_utc_through_2049_generalized_after() {
        assert_eq!(time(0).unwrap(), tlv(0x17, b"700101000000Z").unwrap());
        // 2049-12-31 23:59:59 and a second after
        assert_eq!(time(2_524_607_999).unwrap(), tlv(0x17, b"491231235959Z").unwrap());
        assert_eq!(time(2_524_608_000).unwrap(), tlv(0x18, b"20500101000000Z").unwrap());
    }

    #[test]
    fn a_serial_is_positive_at_its_least() {
        for _ in 0..200 {
            let s = serial().unwrap();
            assert!(s[0] != 0 && s[0] < 0x80, "{s:?}");
        }
    }
}
