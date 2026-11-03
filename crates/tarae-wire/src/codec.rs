//! The messages' bytes: `len: u32` (LE; the kind byte and the body), `kind: u8`, the body. Body types: `u8 u16 u32 u64
//! i32 f32` (LE; a float finite), `bool` (one byte, 0 or 1), `str` (`u32` length + UTF-8), `bytes` (`u32` length +
//! raw). Each value has one encoding, so a message read and written again is the same bytes (`check`'s property).

use std::fmt;
use std::io;
use std::marker::PhantomData;

/// The longest message, its kind byte and body: a whole 200×50 screen is ~35 KB, a paste or a copy of a big
/// selection the most there is.
pub const MSG_MAX: usize = 16 << 20;

/// What was wrong with a message: the connection ends with it (`Gone::Protocol`), said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub said: String,
}

impl Error {
    pub(crate) fn new(said: impl Into<String>) -> Error {
        Error { said: said.into() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.said)
    }
}

/// One side's messages: what each kind byte is and how its body reads.
pub trait Message: Sized {
    fn kind(&self) -> u8;
    fn put(&self, w: &mut Put<'_>);
    /// The message of `kind` from its whole body (`r.end()` after: nothing left over).
    fn get(kind: u8, r: &mut Get<'_>) -> Result<Self, Error>;
}

/// `m`'s bytes on the end of `out`. A message past `MSG_MAX` is refused and `out` left as it was.
pub fn encode<M: Message>(m: &M, out: &mut Vec<u8>) -> Result<(), Error> {
    let start = out.len();
    out.extend_from_slice(&[0; 4]);
    out.push(m.kind());
    m.put(&mut Put { out });
    let len = out.len() - start - 4;
    if len > MSG_MAX {
        out.truncate(start);
        return Err(Error::new(format!(
            "a message of {len} bytes, past the {} MiB a message may be",
            MSG_MAX >> 20
        )));
    }
    out[start..start + 4].copy_from_slice(&(len as u32).to_le_bytes());
    Ok(())
}

/// A body being written.
pub struct Put<'a> {
    out: &'a mut Vec<u8>,
}

impl Put<'_> {
    pub fn u8(&mut self, v: u8) {
        self.out.push(v);
    }
    pub fn u16(&mut self, v: u16) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn bool(&mut self, v: bool) {
        self.out.push(v as u8);
    }
    pub fn str(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }
    pub fn bytes(&mut self, v: &[u8]) {
        // (a length past u32 is past MSG_MAX too: `encode` refuses the message)
        self.u32(v.len().min(u32::MAX as usize) as u32);
        self.out.extend_from_slice(v);
    }
}

/// A body being read: every read takes what's there or says the body was cut short — never an index guessed.
pub struct Get<'a> {
    b: &'a [u8],
}

impl<'a> Get<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if n > self.b.len() {
            return Err(Error::new(format!("a message cut short: {n} bytes wanted, {} there", self.b.len())));
        }
        let (head, rest) = self.b.split_at(n);
        self.b = rest;
        Ok(head)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut a = [0; N];
        a.copy_from_slice(self.take(N)?);
        Ok(a)
    }
    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.array::<1>()?[0])
    }
    pub fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.array()?))
    }
    pub fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    pub fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    pub fn i32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.array()?))
    }
    /// A finite float (a sound's gain and delay): NaN or an infinity is no value either side sends.
    pub fn f32(&mut self) -> Result<f32, Error> {
        let v = f32::from_le_bytes(self.array()?);
        if !v.is_finite() {
            return Err(Error::new(format!("a number that isn't finite ({v})")));
        }
        Ok(v)
    }
    pub fn bool(&mut self) -> Result<bool, Error> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            b => Err(Error::new(format!("a yes or no of {b} (0 or 1)"))),
        }
    }
    pub fn bytes(&mut self) -> Result<Vec<u8>, Error> {
        // (the length is checked against what's there before anything is made of it)
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    pub fn str(&mut self) -> Result<String, Error> {
        let n = self.u32()? as usize;
        let b = self.take(n)?;
        match std::str::from_utf8(b) {
            Ok(s) => Ok(s.to_owned()),
            Err(e) => Err(Error::new(format!("a text that isn't UTF-8 (at byte {})", e.valid_up_to()))),
        }
    }
    /// A character: a Unicode scalar value.
    pub fn char(&mut self) -> Result<char, Error> {
        let v = self.u32()?;
        char::from_u32(v).ok_or_else(|| Error::new(format!("a character of {v:#x}, no Unicode scalar value")))
    }
    /// The body read whole.
    pub fn end(&self) -> Result<(), Error> {
        match self.b.len() {
            0 => Ok(()),
            n => Err(Error::new(format!("{n} bytes left over after a message"))),
        }
    }
}

/// An unknown tag of a kind of value.
pub(crate) fn bad_tag<T>(what: &str, tag: u8) -> Result<T, Error> {
    Err(Error::new(format!("{what} of tag {tag}, none such")))
}

/// Reads one side's messages from a stream's bytes as they come: `push` (or `read_from`) what arrived, then
/// `message` until it says there's no whole message yet. It holds at most one message and what came after it in the
/// same read; a length past `MSG_MAX` is refused before its body is waited for. After an error every `message` says
/// it again.
pub struct Decoder<M> {
    buf: Vec<u8>,
    /// Where the next message starts in `buf` (what's before it was read; dropped once it's most of `buf`).
    at: usize,
    failed: Option<Error>,
    of: PhantomData<M>,
}

impl<M: Message> Default for Decoder<M> {
    fn default() -> Self {
        Decoder { buf: Vec::new(), at: 0, failed: None, of: PhantomData }
    }
}

/// How much one `read_from` reads at most.
const READ: usize = 64 << 10;

impl<M: Message> Decoder<M> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, b: &[u8]) {
        self.compact();
        self.buf.extend_from_slice(b);
    }

    /// One read of `r` into the decoder: how many bytes came (0 = the stream ended).
    pub fn read_from(&mut self, r: &mut impl io::Read) -> io::Result<usize> {
        self.compact();
        let have = self.buf.len();
        self.buf.resize(have + READ, 0);
        let got = r.read(&mut self.buf[have..]);
        let n = *got.as_ref().unwrap_or(&0);
        self.buf.truncate(have + n);
        got
    }

    /// Bytes held that aren't a whole message yet: a stream that ends with some ended in the middle of one.
    pub fn pending(&self) -> usize {
        self.buf.len() - self.at
    }

    fn compact(&mut self) {
        if self.at > 0 && self.at * 2 >= self.buf.len() {
            self.buf.drain(..self.at);
            self.at = 0;
        }
    }

    /// The next whole message, if one is there.
    pub fn message(&mut self) -> Result<Option<M>, Error> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        match self.read_one() {
            Ok(m) => Ok(m),
            Err(e) => {
                self.failed = Some(e.clone());
                Err(e)
            }
        }
    }

    fn read_one(&mut self) -> Result<Option<M>, Error> {
        let rest = &self.buf[self.at..];
        let Some(len) = rest.get(..4) else { return Ok(None) };
        let len = u32::from_le_bytes([len[0], len[1], len[2], len[3]]) as usize;
        if len > MSG_MAX {
            return Err(Error::new(format!(
                "a message of {len} bytes, past the {} MiB a message may be",
                MSG_MAX >> 20
            )));
        }
        if len == 0 {
            return Err(Error::new("an empty message, with no kind"));
        }
        let Some(whole) = rest.get(4..4 + len) else { return Ok(None) };
        let mut r = Get { b: &whole[1..] };
        let m = M::get(whole[0], &mut r)?;
        r.end()?;
        self.at += 4 + len;
        Ok(Some(m))
    }
}
