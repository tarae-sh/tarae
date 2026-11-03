//! What a host and its interface say (the remote plan's §8b, "Protocol v0, exactly"): the interface's `ToHost`, the
//! host's `ToInterface`. Keys and the mouse mirror tarae's `key::Key` and `event::Mouse` tag for tag, in their order;
//! tarae turns each into its own with a match that a new variant breaks.

use crate::codec::{Error, Get, Message, Put, bad_tag};

/// The protocol's version. Host and interface are the same build (`Hello.build` must match too), so it goes up with
/// any change to a message's bytes.
pub const V: u16 = 0;

/// What the interface says.
#[derive(Clone, Debug, PartialEq)]
pub enum ToHost {
    /// First, within 5 s: what the interface is.
    Hello(Hello),
    /// An input event, with the client's own count and the last frame it presented as it sent it.
    Input {
        seq: u64,
        seen: u64,
        event: Input,
    },
    Resize {
        w: u16,
        h: u16,
    },
    /// Frame `frame` was written to its terminal and flushed: presented.
    Ack {
        frame: u64,
    },
    /// What an `Ask` came to.
    Answer {
        id: u32,
        answer: Answer,
    },
    /// The interface's time zone moved (summer time, the laptop moved): checked hourly and on focus.
    Zone {
        zone: String,
        offset: i32,
    },
    /// The client is going (its SIGHUP, `q` on its host-ended line).
    Bye,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hello {
    pub v: u16,
    /// `CARGO_PKG_VERSION` of the client's build.
    pub build: String,
    pub w: u16,
    pub h: u16,
    pub ground: Ground,
    pub undercurl: bool,
    /// Inside zellij: the keys it takes first have their twins.
    pub zellij: bool,
    pub clipboard: Clip,
    pub sound: bool,
    /// It can open an address in a browser.
    pub open: bool,
    /// An IANA zone's name (may be empty) and its offset now, seconds east of UTC.
    pub zone: String,
    pub offset: i32,
}

/// The terminal's ground, as its OSC 11 answer said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ground {
    Unsaid,
    Dark,
    Light,
}

/// What the interface's clipboard can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    None,
    CopyAndPaste,
    CopyOnly,
}

/// An input event: tarae's `Event::{Key, Mouse, Hover, Paste, Focus}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Key(Key),
    Mouse(Mouse),
    Hover { x: u16, y: u16 },
    Paste(String),
    Focus(bool),
}

/// tarae's `key::Key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub code: Code,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// tarae's `key::Code`, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    Char(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
}

/// tarae's `event::Mouse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mouse {
    pub kind: MouseKind,
    pub x: u16,
    pub y: u16,
    pub alt: bool,
    pub ctrl: bool,
}

/// tarae's `event::MouseKind`, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseKind {
    Down,
    Drag,
    Up,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
    ShiftScrollUp,
    ShiftScrollDown,
    Middle,
}

/// Why the clipboard is the terminal's (tarae's `clipboard::Why`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    Ssh,
    NoProgram,
}

/// What an `Ask` came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Copied: by a program (`None`), else through the terminal (OSC 52), and why that way.
    Copied(Option<Why>),
    Pasted(String),
    /// The clipboard is the terminal's: its own paste brings it in.
    PasteUnreadable(Why),
    Opened,
    Failed(String),
}

/// What the host says.
#[derive(Clone, Debug, PartialEq)]
pub enum ToInterface {
    Welcome {
        v: u16,
        build: String,
        pid: u32,
        project: String,
    },
    /// Frame `n`, drawn once input `input` (the last `Input.seq` handled) was, at `w`×`h`: tarae's escape subset, the
    /// diff against the last frame sent (the first after `Welcome` whole).
    Frame {
        n: u64,
        input: u64,
        w: u16,
        h: u16,
        bytes: Vec<u8>,
    },
    /// Something only the interface's machine can do, answered by `Answer` with the same `id`.
    Ask {
        id: u32,
        ask: Ask,
    },
    Sound(Sound),
    /// The host is done with this interface, and why; `said` is what a quit didn't write, for the client to print once
    /// its terminal is back.
    Gone {
        why: Gone,
        said: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    Copy(String),
    Paste,
    Open(String),
}

/// tarae's `sound::engine::Msg`, one for one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sound {
    Wake,
    Play { kind: u8, gain: f32, seed: u32, delay: f32 },
    Volume(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gone {
    /// `:detach`.
    Detached,
    /// `:q`.
    Quit,
    /// Another interface attached.
    TakenOver,
    /// `:restart`: the client execs `bin attach --pid <pid>`.
    Restarting { bin: String },
    /// Not let in (a version, a peer under Claude, an attach from inside the host's own terminal panel).
    Refused { said: String },
    /// Its bytes were wrong.
    Protocol { said: String },
}

impl Message for ToHost {
    fn kind(&self) -> u8 {
        match self {
            ToHost::Hello(_) => 0x01,
            ToHost::Input { .. } => 0x02,
            ToHost::Resize { .. } => 0x03,
            ToHost::Ack { .. } => 0x04,
            ToHost::Answer { .. } => 0x05,
            ToHost::Zone { .. } => 0x06,
            ToHost::Bye => 0x07,
        }
    }

    fn put(&self, w: &mut Put<'_>) {
        match self {
            ToHost::Hello(h) => {
                w.u16(h.v);
                w.str(&h.build);
                w.u16(h.w);
                w.u16(h.h);
                w.u8(h.ground as u8);
                w.bool(h.undercurl);
                w.bool(h.zellij);
                w.u8(h.clipboard as u8);
                w.bool(h.sound);
                w.bool(h.open);
                w.str(&h.zone);
                w.i32(h.offset);
            }
            ToHost::Input { seq, seen, event } => {
                w.u64(*seq);
                w.u64(*seen);
                put_input(w, event);
            }
            ToHost::Resize { w: cols, h } => {
                w.u16(*cols);
                w.u16(*h);
            }
            ToHost::Ack { frame } => w.u64(*frame),
            ToHost::Answer { id, answer } => {
                w.u32(*id);
                match answer {
                    Answer::Copied(how) => {
                        w.u8(0);
                        w.u8(match how {
                            None => 0,
                            Some(Why::Ssh) => 1,
                            Some(Why::NoProgram) => 2,
                        });
                    }
                    Answer::Pasted(t) => {
                        w.u8(1);
                        w.str(t);
                    }
                    Answer::PasteUnreadable(why) => {
                        w.u8(2);
                        w.u8(*why as u8);
                    }
                    Answer::Opened => w.u8(3),
                    Answer::Failed(said) => {
                        w.u8(4);
                        w.str(said);
                    }
                }
            }
            ToHost::Zone { zone, offset } => {
                w.str(zone);
                w.i32(*offset);
            }
            ToHost::Bye => {}
        }
    }

    fn get(kind: u8, r: &mut Get<'_>) -> Result<Self, Error> {
        Ok(match kind {
            0x01 => ToHost::Hello(Hello {
                v: r.u16()?,
                build: r.str()?,
                w: r.u16()?,
                h: r.u16()?,
                ground: match r.u8()? {
                    0 => Ground::Unsaid,
                    1 => Ground::Dark,
                    2 => Ground::Light,
                    t => return bad_tag("a ground", t),
                },
                undercurl: r.bool()?,
                zellij: r.bool()?,
                clipboard: match r.u8()? {
                    0 => Clip::None,
                    1 => Clip::CopyAndPaste,
                    2 => Clip::CopyOnly,
                    t => return bad_tag("a clipboard", t),
                },
                sound: r.bool()?,
                open: r.bool()?,
                zone: r.str()?,
                offset: r.i32()?,
            }),
            0x02 => ToHost::Input { seq: r.u64()?, seen: r.u64()?, event: get_input(r)? },
            0x03 => ToHost::Resize { w: r.u16()?, h: r.u16()? },
            0x04 => ToHost::Ack { frame: r.u64()? },
            0x05 => {
                let id = r.u32()?;
                let answer = match r.u8()? {
                    0 => Answer::Copied(match r.u8()? {
                        0 => None,
                        1 => Some(Why::Ssh),
                        2 => Some(Why::NoProgram),
                        t => return bad_tag("a copy's way", t),
                    }),
                    1 => Answer::Pasted(r.str()?),
                    2 => Answer::PasteUnreadable(get_why(r)?),
                    3 => Answer::Opened,
                    4 => Answer::Failed(r.str()?),
                    t => return bad_tag("an answer", t),
                };
                ToHost::Answer { id, answer }
            }
            0x06 => ToHost::Zone { zone: r.str()?, offset: r.i32()? },
            0x07 => ToHost::Bye,
            k => return Err(Error::new(format!("a message of kind {k:#04x}, none such from an interface"))),
        })
    }
}

fn get_why(r: &mut Get<'_>) -> Result<Why, Error> {
    match r.u8()? {
        0 => Ok(Why::Ssh),
        1 => Ok(Why::NoProgram),
        t => bad_tag("a reason", t),
    }
}

fn put_input(w: &mut Put<'_>, e: &Input) {
    match e {
        Input::Key(k) => {
            w.u8(0);
            w.u8(match k.code {
                Code::Char(_) => 0,
                Code::Enter => 1,
                Code::Esc => 2,
                Code::Tab => 3,
                Code::BackTab => 4,
                Code::Backspace => 5,
                Code::Delete => 6,
                Code::Left => 7,
                Code::Right => 8,
                Code::Up => 9,
                Code::Down => 10,
                Code::Home => 11,
                Code::End => 12,
                Code::PageUp => 13,
                Code::PageDown => 14,
                Code::F(_) => 15,
            });
            match k.code {
                Code::Char(c) => w.u32(c as u32),
                Code::F(n) => w.u8(n),
                _ => {}
            }
            w.bool(k.ctrl);
            w.bool(k.alt);
            w.bool(k.shift);
        }
        Input::Mouse(m) => {
            w.u8(1);
            w.u8(m.kind as u8);
            w.u16(m.x);
            w.u16(m.y);
            w.bool(m.alt);
            w.bool(m.ctrl);
        }
        Input::Hover { x, y } => {
            w.u8(2);
            w.u16(*x);
            w.u16(*y);
        }
        Input::Paste(t) => {
            w.u8(3);
            w.str(t);
        }
        Input::Focus(g) => {
            w.u8(4);
            w.bool(*g);
        }
    }
}

fn get_input(r: &mut Get<'_>) -> Result<Input, Error> {
    Ok(match r.u8()? {
        0 => {
            let code = match r.u8()? {
                0 => Code::Char(r.char()?),
                1 => Code::Enter,
                2 => Code::Esc,
                3 => Code::Tab,
                4 => Code::BackTab,
                5 => Code::Backspace,
                6 => Code::Delete,
                7 => Code::Left,
                8 => Code::Right,
                9 => Code::Up,
                10 => Code::Down,
                11 => Code::Home,
                12 => Code::End,
                13 => Code::PageUp,
                14 => Code::PageDown,
                15 => Code::F(r.u8()?),
                t => return bad_tag("a key", t),
            };
            Input::Key(Key { code, ctrl: r.bool()?, alt: r.bool()?, shift: r.bool()? })
        }
        1 => {
            let kind = match r.u8()? {
                0 => MouseKind::Down,
                1 => MouseKind::Drag,
                2 => MouseKind::Up,
                3 => MouseKind::ScrollUp,
                4 => MouseKind::ScrollDown,
                5 => MouseKind::ScrollLeft,
                6 => MouseKind::ScrollRight,
                7 => MouseKind::ShiftScrollUp,
                8 => MouseKind::ShiftScrollDown,
                9 => MouseKind::Middle,
                t => return bad_tag("a mouse action", t),
            };
            Input::Mouse(Mouse { kind, x: r.u16()?, y: r.u16()?, alt: r.bool()?, ctrl: r.bool()? })
        }
        2 => Input::Hover { x: r.u16()?, y: r.u16()? },
        3 => Input::Paste(r.str()?),
        4 => Input::Focus(r.bool()?),
        t => return bad_tag("an input", t),
    })
}

impl Message for ToInterface {
    fn kind(&self) -> u8 {
        match self {
            ToInterface::Welcome { .. } => 0x81,
            ToInterface::Frame { .. } => 0x82,
            ToInterface::Ask { .. } => 0x83,
            ToInterface::Sound(_) => 0x84,
            ToInterface::Gone { .. } => 0x85,
        }
    }

    fn put(&self, w: &mut Put<'_>) {
        match self {
            ToInterface::Welcome { v, build, pid, project } => {
                w.u16(*v);
                w.str(build);
                w.u32(*pid);
                w.str(project);
            }
            ToInterface::Frame { n, input, w: cols, h, bytes } => {
                w.u64(*n);
                w.u64(*input);
                w.u16(*cols);
                w.u16(*h);
                w.bytes(bytes);
            }
            ToInterface::Ask { id, ask } => {
                w.u32(*id);
                match ask {
                    Ask::Copy(t) => {
                        w.u8(0);
                        w.str(t);
                    }
                    Ask::Paste => w.u8(1),
                    Ask::Open(url) => {
                        w.u8(2);
                        w.str(url);
                    }
                }
            }
            ToInterface::Sound(s) => match *s {
                Sound::Wake => w.u8(0),
                Sound::Play { kind, gain, seed, delay } => {
                    w.u8(1);
                    w.u8(kind);
                    w.f32(gain);
                    w.u32(seed);
                    w.f32(delay);
                }
                Sound::Volume(v) => {
                    w.u8(2);
                    w.u8(v);
                }
            },
            ToInterface::Gone { why, said } => {
                match why {
                    Gone::Detached => w.u8(0),
                    Gone::Quit => w.u8(1),
                    Gone::TakenOver => w.u8(2),
                    Gone::Restarting { bin } => {
                        w.u8(3);
                        w.str(bin);
                    }
                    Gone::Refused { said } => {
                        w.u8(4);
                        w.str(said);
                    }
                    Gone::Protocol { said } => {
                        w.u8(5);
                        w.str(said);
                    }
                }
                w.str(said);
            }
        }
    }

    fn get(kind: u8, r: &mut Get<'_>) -> Result<Self, Error> {
        Ok(match kind {
            0x81 => ToInterface::Welcome { v: r.u16()?, build: r.str()?, pid: r.u32()?, project: r.str()? },
            0x82 => ToInterface::Frame {
                n: r.u64()?,
                input: r.u64()?,
                w: r.u16()?,
                h: r.u16()?,
                bytes: r.bytes()?,
            },
            0x83 => {
                let id = r.u32()?;
                let ask = match r.u8()? {
                    0 => Ask::Copy(r.str()?),
                    1 => Ask::Paste,
                    2 => Ask::Open(r.str()?),
                    t => return bad_tag("an ask", t),
                };
                ToInterface::Ask { id, ask }
            }
            0x84 => ToInterface::Sound(match r.u8()? {
                0 => Sound::Wake,
                1 => Sound::Play { kind: r.u8()?, gain: r.f32()?, seed: r.u32()?, delay: r.f32()? },
                2 => Sound::Volume(r.u8()?),
                t => return bad_tag("a sound", t),
            }),
            0x85 => {
                let why = match r.u8()? {
                    0 => Gone::Detached,
                    1 => Gone::Quit,
                    2 => Gone::TakenOver,
                    3 => Gone::Restarting { bin: r.str()? },
                    4 => Gone::Refused { said: r.str()? },
                    5 => Gone::Protocol { said: r.str()? },
                    t => return bad_tag("a reason to go", t),
                };
                ToInterface::Gone { why, said: r.str()? }
            }
            k => return Err(Error::new(format!("a message of kind {k:#04x}, none such from a host"))),
        })
    }
}
