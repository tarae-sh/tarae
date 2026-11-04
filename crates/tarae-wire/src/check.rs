//! The decoder's property, for the crate's tests and `fuzz/parsers`' `wire_decode`: any bytes, read a few at a time,
//! never panic, and every message read is written back as the very bytes it was read from (one encoding a value).

use crate::codec::{Decoder, Message, encode};
use crate::msg::*;

/// libFuzzer's input: the first byte says which side's messages (its low bit) and how many bytes come at a time (the
/// rest), the others are the stream.
pub fn decode_input(b: &[u8]) {
    let Some((&first, stream)) = b.split_first() else { return };
    let chunk = 1 + (first >> 1) as usize;
    if first & 1 == 0 {
        decode::<ToHost>(stream, chunk);
    } else {
        decode::<ToInterface>(stream, chunk);
    }
}

/// `stream` read `chunk` bytes at a time until it ends or is wrong: what was read, written again, is where it
/// started.
pub fn decode<M: Message + std::fmt::Debug>(stream: &[u8], chunk: usize) {
    let mut d = Decoder::<M>::new();
    let mut again = Vec::new();
    'read: for piece in stream.chunks(chunk.max(1)) {
        d.push(piece);
        loop {
            match d.message() {
                Ok(Some(m)) => {
                    encode(&m, &mut again).expect("a message read is within MSG_MAX");
                    assert!(
                        stream.starts_with(&again),
                        "{m:?} written again isn't the bytes it was read from"
                    );
                }
                Ok(None) => break,
                Err(_) => break 'read,
            }
        }
    }
}

/// One of every message an interface sends, every variant of what's inside.
pub fn to_host() -> Vec<ToHost> {
    let hello = Hello {
        v: V,
        build: "0.1.0".into(),
        w: 120,
        h: 34,
        ground: Ground::Dark,
        undercurl: true,
        zellij: false,
        clipboard: Clip::CopyAndPaste,
        sound: true,
        open: false,
        zone: "Asia/Seoul".into(),
        offset: 9 * 3600,
    };
    let mut m = vec![
        ToHost::Hello(hello.clone()),
        ToHost::Hello(Hello {
            ground: Ground::Unsaid,
            clipboard: Clip::None,
            zone: String::new(),
            offset: -18000,
            ..hello.clone()
        }),
        ToHost::Hello(Hello { ground: Ground::Light, clipboard: Clip::CopyOnly, ..hello }),
    ];
    let codes = [
        Code::Char('a'),
        Code::Char('한'),
        Code::Char('\u{10FFFF}'),
        Code::Enter,
        Code::Esc,
        Code::Tab,
        Code::BackTab,
        Code::Backspace,
        Code::Delete,
        Code::Left,
        Code::Right,
        Code::Up,
        Code::Down,
        Code::Home,
        Code::End,
        Code::PageUp,
        Code::PageDown,
        Code::F(2),
    ];
    let mut seq = 0;
    let mut input = |event| {
        seq += 1;
        ToHost::Input { seq, seen: seq / 2, event }
    };
    for (i, code) in codes.into_iter().enumerate() {
        m.push(input(Input::Key(Key { code, ctrl: i % 2 == 0, alt: i % 3 == 0, shift: i % 5 == 0 })));
    }
    let kinds = [
        MouseKind::Down,
        MouseKind::Drag,
        MouseKind::Up,
        MouseKind::ScrollUp,
        MouseKind::ScrollDown,
        MouseKind::ScrollLeft,
        MouseKind::ScrollRight,
        MouseKind::ShiftScrollUp,
        MouseKind::ShiftScrollDown,
        MouseKind::Middle,
    ];
    for (i, kind) in kinds.into_iter().enumerate() {
        m.push(input(Input::Mouse(Mouse { kind, x: i as u16, y: 300, alt: i == 1, ctrl: i == 2 })));
    }
    m.extend([
        input(Input::Hover { x: 3, y: u16::MAX }),
        input(Input::Paste("pasted\n여러 줄".into())),
        input(Input::Focus(true)),
        input(Input::Focus(false)),
        ToHost::Resize { w: 200, h: 50 },
        ToHost::Ack { frame: u64::MAX },
        ToHost::Answer { id: 1, answer: Answer::Copied(None) },
        ToHost::Answer { id: 2, answer: Answer::Copied(Some(Why::Ssh)) },
        ToHost::Answer { id: 3, answer: Answer::Copied(Some(Why::NoProgram)) },
        ToHost::Answer { id: 4, answer: Answer::Pasted("text".into()) },
        ToHost::Answer { id: 5, answer: Answer::PasteUnreadable(Why::Ssh) },
        ToHost::Answer { id: 6, answer: Answer::PasteUnreadable(Why::NoProgram) },
        ToHost::Answer { id: 7, answer: Answer::Opened },
        ToHost::Answer { id: u32::MAX, answer: Answer::Failed("no xdg-open here".into()) },
        ToHost::Zone { zone: "Europe/London".into(), offset: 3600 },
        ToHost::Bye,
        ToHost::Ping { n: 1 },
        ToHost::Ping { n: u32::MAX },
    ]);
    m
}

/// One of every message a host sends, every variant of what's inside.
pub fn to_interface() -> Vec<ToInterface> {
    vec![
        ToInterface::Welcome { v: V, build: "0.1.0".into(), pid: 4242, project: "/p/tarae".into() },
        ToInterface::Frame {
            n: 1,
            input: 0,
            w: 120,
            h: 34,
            bytes: b"\x1b[?2026h\x1b[1;1Hhello\x1b[?2026l".to_vec(),
        },
        ToInterface::Frame { n: 2, input: 7, w: 1, h: 1, bytes: Vec::new() },
        ToInterface::Ask { id: 1, ask: Ask::Copy("a selection".into()) },
        ToInterface::Ask { id: 2, ask: Ask::Paste },
        ToInterface::Ask { id: 3, ask: Ask::Open("https://tarae.sh".into()) },
        ToInterface::Sound(Sound::Wake),
        ToInterface::Sound(Sound::Play { kind: 3, gain: 0.25, seed: 99, delay: 0.125 }),
        ToInterface::Sound(Sound::Play { kind: 0, gain: -0.0, seed: 0, delay: 0.0 }),
        ToInterface::Sound(Sound::Volume(70)),
        ToInterface::Gone { why: Gone::Detached, said: String::new() },
        ToInterface::Gone { why: Gone::Quit, said: "src/a.rs written\n".into() },
        ToInterface::Gone { why: Gone::TakenOver, said: String::new() },
        ToInterface::Gone {
            why: Gone::Restarting { bin: "/usr/local/bin/tarae".into() },
            said: String::new(),
        },
        ToInterface::Gone { why: Gone::Refused { said: "another version".into() }, said: String::new() },
        ToInterface::Gone { why: Gone::Protocol { said: "a bad bool".into() }, said: String::new() },
        ToInterface::Pong { n: 1 },
        ToInterface::Pong { n: u32::MAX },
    ]
}

/// Every sample message's bytes, one stream a side.
pub fn streams() -> (Vec<u8>, Vec<u8>) {
    let mut host = Vec::new();
    for m in to_host() {
        encode(&m, &mut host).expect("a sample is small");
    }
    let mut iface = Vec::new();
    for m in to_interface() {
        encode(&m, &mut iface).expect("a sample is small");
    }
    (host, iface)
}
