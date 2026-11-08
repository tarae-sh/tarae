//! The scan as it streams: a sentinel split anywhere swapped in place, framing untouched, nothing held that no pattern
//! starts with (a streamed answer goes event by event), a stray stopped before any of it goes, the longest of two
//! starting together, a pause's release — and the property (`check::streamed`) at every split and at random.

use super::check::{patterns, scan_input, streamed};
use super::*;
use crate::sandbox::proxy::parse::tests::Rng;

const ONE: &[u8] = b"fake_value_a1b2c3d4e5";

/// A body holding ONE's sentinel, cut once at every place with a chunk's framing between the halves: ONE's real value
/// in its place, every other byte and the framing as they came.
#[test]
fn a_sentinel_split_anywhere_is_swapped_in_place() {
    let payload = [&b"{\"token\":\""[..], ONE, b"\"}"].concat();
    let want = [&b"{\"token\":\""[..], b"REAL-VALUE-ONE-ABCDEF", b"\"}"].concat();
    for cut in 0..=payload.len() {
        let mut scan = Scan::new(patterns(), true);
        let mut out = Vec::new();
        scan.data(&payload[..cut], &mut out).unwrap();
        scan.frame(b"\r\n5\r\n", &mut out);
        scan.data(&payload[cut..], &mut out).unwrap();
        scan.end(&mut out).unwrap();
        let framed = [&want[..cut], b"\r\n5\r\n", &want[cut..]].concat();
        assert_eq!(String::from_utf8_lossy(&out), String::from_utf8_lossy(&framed), "cut at {cut}");
        assert_eq!(scan.swapped, 1);
        streamed(&patterns(), &payload, &[cut], b"\r\n");
    }
}

/// Values look random: an answer streamed event by event (server-sent events, none ending in a value's start) goes
/// on whole as each event comes — nothing held for the next, which may be seconds away.
#[test]
fn a_streamed_answer_goes_event_by_event() {
    let real =
        |s: &str| Pattern { from: s.into(), to: vec![b'x'; s.len()], does: Does::Swap, name: "R".into() };
    let mut scan = Scan::new(vec![real("Qz7rT0kLm2vX9pWs4hNc"), real("Jd8uYb3eAq6oFi1gZt5w")], true);
    for k in 0..20 {
        let event = format!("event: delta\ndata: {{\"text\":\"part {k} of the answer\"}}\n\n");
        let mut out = Vec::new();
        scan.data(event.as_bytes(), &mut out).unwrap();
        assert_eq!(out, event.as_bytes(), "event {k} went whole");
        assert!(!scan.holding());
    }
}

/// Another secret's sentinel stops the scan before a byte of it goes — split anywhere, its start held until it's
/// known; what went before it went.
#[test]
fn a_stray_stops_before_any_of_it_goes() {
    let payload = b"before fake_value_zz9 after";
    for cut in 0..=payload.len() {
        let mut scan = Scan::new(patterns(), true);
        let mut out = Vec::new();
        let first = scan.data(&payload[..cut], &mut out);
        let stopped = first.and_then(|()| scan.data(&payload[cut..], &mut out));
        assert_eq!(stopped, Err("OTHER".to_string()), "cut at {cut}");
        assert!(b"before ".starts_with(&out), "cut at {cut}: {:?}", String::from_utf8_lossy(&out));
    }
}

/// Two patterns starting at one place: the longer wins, even when the shorter is whole first and the longer comes in
/// the next piece; the body ending after the shorter decides it. The heads' `replaced` alike.
#[test]
fn the_longer_of_two_starting_together_wins() {
    let long = b"fake_value_a1b2c3d4e5f6g7";
    for (payload, want) in [
        (&long[..], &b"REAL-VALUE-LONGER-OF-THEM"[..]),
        (ONE, b"REAL-VALUE-ONE-ABCDEF"),
        (b"fake_value_a1b2c3d4e5f6", b"REAL-VALUE-ONE-ABCDEFf6"),
    ] {
        for cut in 0..=payload.len() {
            let mut scan = Scan::new(patterns(), true);
            let mut out = Vec::new();
            scan.data(&payload[..cut], &mut out).unwrap();
            scan.data(&payload[cut..], &mut out).unwrap();
            scan.end(&mut out).unwrap();
            assert_eq!(String::from_utf8_lossy(&out), String::from_utf8_lossy(want), "cut at {cut}");
        }
    }
    let pats: [(&[u8], &[u8]); 2] = [(b"abcd", b"1"), (b"abcdef", b"2")];
    assert_eq!(replaced(b"x abcdef abcd", &pats).unwrap(), b"x 2 1");
    assert_eq!(replaced(b"nothing here", &pats), None);
}

/// A pause lets what's held go as it is: the sentinel it began then goes as its fake (left, said by name) — and a
/// stray completed after a release still stops the scan.
#[test]
fn a_release_lets_what_is_held_go() {
    let mut scan = Scan::new(patterns(), true);
    let mut out = Vec::new();
    scan.data(b"x=fake_value_a1", &mut out).unwrap();
    assert_eq!(out, b"x=");
    assert!(scan.holding());
    assert_eq!(scan.release(&mut out), Ok(true));
    assert_eq!(out, b"x=fake_value_a1");
    scan.data(b"b2c3d4e5 y", &mut out).unwrap();
    scan.end(&mut out).unwrap();
    assert_eq!(out, b"x=fake_value_a1b2c3d4e5 y", "its start had gone: the fake goes");
    assert_eq!(scan.left(), [(Does::Swap, "ONE".to_string())]);
    assert_eq!(scan.left(), [], "said once");
    let mut scan = Scan::new(patterns(), true);
    scan.data(b"fake_value_z", &mut out).unwrap();
    scan.release(&mut out).unwrap();
    assert_eq!(scan.data(b"z9", &mut out), Err("OTHER".to_string()));
}

/// A value whose sentinel isn't as long (under 20 bytes, U1) is left in a body, and said by name.
#[test]
fn a_pattern_not_as_long_is_left_and_said() {
    let mut scan = Scan::new(patterns(), true);
    let mut out = Vec::new();
    scan.data(b"a fake_value_aaaa b", &mut out).unwrap();
    scan.end(&mut out).unwrap();
    assert_eq!(out, b"a fake_value_aaaa b");
    assert_eq!(scan.left(), [(Does::Note, "SHORT".to_string())]);
}

/// Watching (plain http): every byte goes at once, a stray split across pieces still found.
#[test]
fn watching_holds_nothing_and_still_finds_a_stray() {
    let mut scan = Scan::new(patterns(), false);
    let mut out = Vec::new();
    scan.data(b"q=fake_va", &mut out).unwrap();
    assert_eq!(out, b"q=fake_va");
    scan.frame(b"\r\n9\r\n", &mut out);
    assert_eq!(scan.data(b"lue_zz9", &mut out), Err("OTHER".to_string()));
}

/// The property at random (the fuzz target's own, `scan_input`): payloads of the patterns' own letters with patterns
/// planted, overlapping and back to back, cut in random pieces with framing between — each pattern swapped exactly
/// where the whole payload's reading swaps it, every other byte and the framing untouched, a stray before any of it.
#[test]
fn the_scan_holds_its_property_at_random() {
    let mut rng = Rng(0x5ca9_0000_7a5e_0001);
    let letters = b"fake_vlu12345abcdzxyg \r\n";
    for _ in 0..3000 {
        let mut payload = Vec::new();
        let n = rng.below(120);
        while payload.len() < n {
            match rng.below(6) {
                0 => payload.extend_from_slice(&patterns()[rng.below(patterns().len())].from),
                1 => {
                    let p = &patterns()[rng.below(patterns().len())].from;
                    payload.extend_from_slice(&p[..rng.below(p.len())]);
                }
                _ => payload.push(letters[rng.below(letters.len())]),
            }
        }
        let mut cuts = Vec::new();
        let mut at = 0;
        while at < payload.len() {
            at += 1 + rng.below(9);
            cuts.push(at.min(payload.len()));
        }
        streamed(&patterns(), &payload, &cuts, b"\r\n1f\r\n");
    }
    // (the fuzz target's reading of its input: planted by 0xff, pieces by the first byte)
    for b in [&b"\x03ab\xff\x00cd\xff\x04\xff\x01"[..], b"\x00\xff\x03", b"", b"\x0f\xff"] {
        scan_input(b);
    }
}
