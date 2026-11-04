use crate::check;
use crate::codec::{Decoder, Error, MSG_MAX, Message, encode};
use crate::mailbox::{IN_FLIGHT, Next, Order, Slot};
use crate::msg::*;
use tarae_testkit::mutate::{self, Rng};

fn bytes<M: Message>(m: &M) -> Vec<u8> {
    let mut out = Vec::new();
    encode(m, &mut out).unwrap();
    out
}

fn decode_all<M: Message>(b: &[u8]) -> Result<Vec<M>, Error> {
    let mut d = Decoder::<M>::new();
    d.push(b);
    let mut out = Vec::new();
    while let Some(m) = d.message()? {
        out.push(m);
    }
    Ok(out)
}

/// A message of `kind` with `body`, framed.
fn raw(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut b = ((body.len() + 1) as u32).to_le_bytes().to_vec();
    b.push(kind);
    b.extend_from_slice(body);
    b
}

fn refused<M: Message + std::fmt::Debug>(b: &[u8]) -> String {
    match decode_all::<M>(b) {
        Err(e) => e.said,
        Ok(m) => panic!("read {m:?} from {b:?}"),
    }
}

#[test]
fn every_message_reads_back_as_it_was_written() {
    for m in check::to_host() {
        assert_eq!(decode_all::<ToHost>(&bytes(&m)).unwrap(), vec![m.clone()], "{m:?}");
    }
    for m in check::to_interface() {
        assert_eq!(decode_all::<ToInterface>(&bytes(&m)).unwrap(), vec![m.clone()], "{m:?}");
    }
    // (the samples cover every kind of each side: a new message is a sample too)
    let kinds = |k: Vec<u8>| k.into_iter().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(kinds(check::to_host().iter().map(Message::kind).collect()), (1..=8).collect());
    assert_eq!(kinds(check::to_interface().iter().map(Message::kind).collect()), (0x81..=0x86).collect());
}

#[test]
fn a_stream_read_a_byte_at_a_time_reads_the_same_messages() {
    let (host, iface) = check::streams();
    for chunk in [1, 2, 3, 7, 64, host.len()] {
        let mut d = Decoder::<ToHost>::new();
        let mut got = Vec::new();
        for piece in host.chunks(chunk) {
            d.push(piece);
            while let Some(m) = d.message().unwrap() {
                got.push(m);
            }
        }
        assert_eq!(got, check::to_host(), "{chunk} at a time");
        assert_eq!(d.pending(), 0);
        check::decode::<ToInterface>(&iface, chunk);
    }
}

#[test]
fn the_decoder_reads_from_a_stream_and_says_a_message_cut_at_its_end() {
    let (host, _) = check::streams();
    let cut = &host[..host.len() - 1];
    let mut r = std::io::Cursor::new(cut);
    let mut d = Decoder::<ToHost>::new();
    let mut got = 0;
    while d.read_from(&mut r).unwrap() > 0 {
        while d.message().unwrap().is_some() {
            got += 1;
        }
    }
    assert_eq!(got, check::to_host().len() - 1, "the last message (a Ping) cut");
    assert_eq!(d.pending(), 8, "the Ping's length, kind and 3 of its 4 bytes");
}

#[test]
fn every_malformed_message_ends_the_stream_with_what_was_wrong() {
    let hello = bytes(&check::to_host()[0]);
    let key = |code: &[u8]| {
        let mut body = vec![0; 16];
        body.push(0);
        body.extend_from_slice(code);
        body.extend_from_slice(&[0, 0, 0]);
        raw(0x02, &body)
    };
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (raw(0x09, &[]), "kind 0x09"),
        (raw(0x81, &[]), "kind 0x81, none such from an interface"),
        (vec![0, 0, 0, 0], "an empty message"),
        (((MSG_MAX + 1) as u32).to_le_bytes().to_vec(), "past the 16 MiB"),
        (raw(0x04, &[1, 2, 3]), "cut short"),
        (raw(0x04, &[0; 9]), "1 bytes left over"),
        (raw(0x07, &[0]), "1 bytes left over"),
        (key(&[0, 0, 0xD8, 0, 0]), "character of 0xd800"),
        (key(&[0, 0, 0, 0x11, 0]), "character of 0x110000"),
        (key(&[16]), "a key of tag 16"),
        (raw(0x02, &[[0; 16].as_slice(), &[5]].concat()), "an input of tag 5"),
        (raw(0x02, &[[0; 16].as_slice(), &[1, 10, 0, 0, 0, 0, 0, 0]].concat()), "a mouse action of tag 10"),
        (raw(0x02, &[[0; 16].as_slice(), &[4, 2]].concat()), "a yes or no of 2"),
        (raw(0x02, &[[0; 16].as_slice(), &[3, 2, 0, 0, 0, 0xC3, 0x28]].concat()), "isn't UTF-8"),
        (raw(0x02, &[[0; 16].as_slice(), &[3, 255, 255, 255, 255]].concat()), "cut short"),
        (raw(0x05, &[0, 0, 0, 0, 5]), "an answer of tag 5"),
        (raw(0x05, &[0, 0, 0, 0, 0, 3]), "a copy's way of tag 3"),
        (raw(0x05, &[0, 0, 0, 0, 2, 2]), "a reason of tag 2"),
    ];
    for (b, says) in cases {
        let said = refused::<ToHost>(&b);
        assert!(said.contains(says), "{b:?}: {said:?}, wanted {says:?}");
        // (the same after a good message, and again on the next call)
        let mut d = Decoder::<ToHost>::new();
        d.push(&[hello.clone(), b].concat());
        assert!(d.message().unwrap().is_some());
        assert_eq!(d.message().unwrap_err().said, said);
        assert_eq!(d.message().unwrap_err().said, said);
    }
    // A Hello's ground and clipboard
    let mut bad = hello.clone();
    let ground = 4 + 1 + 2 + 4 + 5 + 4;
    bad[ground] = 3;
    assert!(refused::<ToHost>(&bad).contains("a ground of tag 3"));
    let mut bad = hello.clone();
    // (past the ground, undercurl and `taken`'s length — empty in the sample)
    bad[ground + 6] = 3;
    assert!(refused::<ToHost>(&bad).contains("a clipboard of tag 3"));

    let frame = |tail: &[u8]| raw(0x82, &[[0; 20].as_slice(), tail].concat());
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (raw(0x01, &[]), "kind 0x01, none such from a host"),
        (frame(&[9, 0, 0, 0, 1]), "cut short"),
        (raw(0x83, &[0, 0, 0, 0, 3]), "an ask of tag 3"),
        (raw(0x84, &[3]), "a sound of tag 3"),
        (raw(0x84, &[1, 0, 0, 0, 0xC0, 0x7F, 0, 0, 0, 0, 0, 0, 0, 0]), "isn't finite"),
        (raw(0x84, &[1, 0, 0, 0, 0x80, 0x7F, 0, 0, 0, 0, 0, 0, 0, 0]), "isn't finite"),
        (raw(0x85, &[6, 0, 0, 0, 0]), "a reason to go of tag 6"),
        (raw(0x85, &[0]), "cut short"),
    ];
    for (b, says) in cases {
        let said = refused::<ToInterface>(&b);
        assert!(said.contains(says), "{b:?}: {said:?}, wanted {says:?}");
    }
}

#[test]
fn a_length_past_the_limit_is_refused_before_its_body_comes() {
    // (four bytes are enough to say no: nothing is waited for, nothing made of that size)
    let mut d = Decoder::<ToInterface>::new();
    d.push(&u32::MAX.to_le_bytes());
    assert!(d.message().unwrap_err().said.contains("4294967295 bytes"));
    // and a message that would be past it isn't written
    let big = ToInterface::Ask { id: 1, ask: Ask::Copy("x".repeat(MSG_MAX)) };
    let mut out = b"kept".to_vec();
    assert!(encode(&big, &mut out).unwrap_err().said.contains("past the 16 MiB"));
    assert_eq!(out, b"kept");
}

/// The decoder's never-panic loop: the sample streams mutated, read every way the fuzz target reads them.
#[test]
fn the_decoder_never_panics_on_mutated_streams() {
    let (host, iface) = check::streams();
    let words: &[&[u8]] =
        &[&[0xff, 0xff, 0xff, 0xff], &[0, 0, 0, 0], &[1, 0, 0, 0], &[0x82], &[0xD8], &[0xC3]];
    let mut rng = Rng(0x7a7a_e3e3);
    for i in 0..4000 {
        let mut b = if i % 2 == 0 { host.clone() } else { iface.clone() };
        mutate::bytes(&mut rng, &mut b, words);
        b.insert(0, rng.next() as u8 & !1 | (i % 2) as u8);
        check::decode_input(&b);
    }
}

#[test]
#[ignore = "writes fuzz/parsers' seeds: run by hand when a message changes"]
fn writes_fuzz_seeds() {
    let (host, iface) = check::streams();
    mutate::fuzz_seeds(
        "wire_decode",
        [
            ("to-host".into(), [&[6u8][..], &host].concat()),
            ("to-interface".into(), [&[7u8][..], &iface].concat()),
        ],
    );
}

fn frames(slot: &mut Slot<&'static str, &'static str>) -> Vec<String> {
    let mut out = Vec::new();
    loop {
        match slot.take() {
            Next::Frame(n, f) => out.push(format!("{n}:{f}")),
            Next::Msg(m) => out.push(m.to_string()),
            Next::Wait => break,
            Next::Closed => {
                out.push("closed".into());
                break;
            }
        }
    }
    out
}

#[test]
fn a_frame_not_sent_is_replaced_and_frames_wait_for_credit() {
    let mut s = Slot::default();
    assert_eq!(s.put_frame(1, "a"), None);
    assert_eq!(s.put_frame(2, "b"), Some("a"), "replaced, its buffer back");
    assert_eq!(frames(&mut s), ["2:b"]);
    s.put_frame(3, "c");
    assert_eq!(frames(&mut s), ["3:c"]);
    assert_eq!(s.in_flight(), IN_FLIGHT);
    s.put_frame(4, "d");
    assert_eq!(s.put_frame(5, "e"), Some("d"));
    assert_eq!(frames(&mut s), Vec::<String>::new(), "no credit");
    s.ack(2);
    assert_eq!(frames(&mut s), ["5:e"], "the newest only");
    assert_eq!(s.skipped(), 2);
    s.ack(5);
    assert_eq!(s.in_flight(), 0);
}

#[test]
fn an_ask_goes_after_its_frame_and_a_sound_at_once() {
    let mut s = Slot::default();
    s.put_frame(1, "a");
    s.put_frame(2, "b");
    assert_eq!(frames(&mut s), ["2:b"]);
    s.put_frame(3, "c");
    s.push("copy", Order::AfterFrame, 4);
    s.push("wake", Order::AtOnce, 1);
    assert_eq!(s.pending(), 5);
    assert_eq!(frames(&mut s), ["wake", "3:c", "copy"]);
    s.put_frame(4, "d");
    s.push("paste", Order::AfterFrame, 0);
    s.put_frame(5, "e");
    s.push("play", Order::AtOnce, 0);
    assert_eq!(frames(&mut s), ["play"], "no credit: the ask waits behind its frame, the sound doesn't");
    s.ack(3);
    assert_eq!(frames(&mut s), ["5:e", "paste"], "after the frame that replaced its own");
    assert_eq!(s.pending(), 0);
}

#[test]
fn closed_the_queue_goes_and_the_frame_doesnt() {
    let mut s = Slot::default();
    s.put_frame(1, "a");
    s.put_frame(2, "b");
    assert_eq!(frames(&mut s), ["2:b"]);
    s.put_frame(3, "c");
    s.put_frame(4, "d");
    s.push("gone", Order::AfterFrame, 0);
    s.close();
    assert_eq!(s.put_frame(5, "e"), Some("e"));
    assert_eq!(frames(&mut s), ["gone", "closed"]);
    assert_eq!(s.take_spare(), Some("d"));
}

#[test]
fn the_mailbox_wakes_its_writer() {
    let m = std::sync::Arc::new(crate::Mailbox::<u64, &'static str>::new());
    let w = std::thread::spawn({
        let m = m.clone();
        move || {
            let mut got = Vec::new();
            loop {
                match m.wait() {
                    Next::Frame(n, _) => {
                        got.push(n);
                        m.with(|s| s.ack(n));
                    }
                    Next::Msg(_) => {}
                    Next::Closed => return got,
                    Next::Wait => unreachable!("wait returns only what goes"),
                }
            }
        }
    });
    for n in 1..=50 {
        m.with(|s| s.put_frame(n, n));
    }
    // (the last frame goes before close: once the writer took it)
    while m.with(|s| s.last_sent()) != 50 {
        std::thread::yield_now();
    }
    m.with(|s| s.close());
    let got = w.join().unwrap();
    assert!(got.windows(2).all(|p| p[0] < p[1]), "in order: {got:?}");
    assert_eq!(got.last(), Some(&50));
}
