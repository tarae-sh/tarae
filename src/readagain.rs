//! A read that can't sleep through its stream's end (#390, #392). macOS can leave a blocking `recv` asleep through its
//! peer's end: the end is there (`poll` says `POLLIN | POLLHUP`, a fresh `recv` reads 0), and the sleeping `recv` is
//! never woken (4 runs in 4 under stress; a test run's 7 h 51 m). So a reader that waits on a socket has its reads timed
//! (`timed`, `EVERY`, set as the socket is had) and reads again at each timeout (`ReadAgain`): an end the kernel
//! didn't wake it for comes at most `EVERY` late. Only the reading thread wakes. What it read goes on only when bytes
//! or the end came, so an idle tarae is still never woken.

use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

/// How long one read of a socket sleeps before it looks again.
pub const EVERY: Duration = Duration::from_secs(1);

/// `fd`'s reads timed at `EVERY` (`SO_RCVTIMEO`). A pipe (`ENOTSOCK`) stays as it is: no such loss was seen there.
/// XNU refuses the option once a socket can neither receive nor send more. Its end is already there then, and a read
/// sees it at once, so a refusal leaves nothing to sleep through: callers go on either way.
pub fn timed(fd: &impl AsRawFd) -> io::Result<()> {
    let tv = libc::timeval { tv_sec: EVERY.as_secs() as _, tv_usec: EVERY.subsec_micros() as _ };
    // SAFETY: one timeval of ours, its size; setsockopt only reads it.
    let r = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (&raw const tv).cast(),
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        )
    };
    if r == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// `R`'s reads (its socket `timed`) read again at each timeout until bytes or the end come, or until `until` has
/// passed (then `TimedOut`).
pub struct ReadAgain<R> {
    r: R,
    until: Option<Instant>,
}

impl<R> ReadAgain<R> {
    /// Read until bytes or the end come.
    pub fn new(r: R) -> ReadAgain<R> {
        ReadAgain { r, until: None }
    }

    /// Read until bytes or the end come, or until `until` has passed.
    pub fn until(r: R, until: Instant) -> ReadAgain<R> {
        ReadAgain { r, until: Some(until) }
    }
}

impl<R: Read> Read for ReadAgain<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.r.read(buf) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                    ) =>
                {
                    if self.until.is_some_and(|u| crate::clock::now() >= u) {
                        return Err(io::ErrorKind::TimedOut.into());
                    }
                }
                got => return got,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A timeout is read again, never said; bytes, the end and any other error are said as they come; past `until`,
    /// `TimedOut`.
    #[test]
    fn a_timeout_is_read_again_and_only_bytes_or_the_end_come_out() {
        struct Said(Vec<io::Result<usize>>);
        impl Read for Said {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                self.0.remove(0)
            }
        }
        let late = || Err(io::ErrorKind::WouldBlock.into());
        let mut r = ReadAgain::new(Said(vec![
            late(),
            Err(io::ErrorKind::TimedOut.into()),
            late(),
            Ok(3),
            late(),
            Ok(0),
        ]));
        assert_eq!(r.read(&mut []).unwrap(), 3);
        assert_eq!(r.read(&mut []).unwrap(), 0, "the end");
        let mut r = ReadAgain::new(Said(vec![late(), Err(io::ErrorKind::BrokenPipe.into())]));
        assert_eq!(r.read(&mut []).unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        let mut r = ReadAgain::until(Said(vec![late(), Ok(1)]), crate::clock::now());
        assert_eq!(r.read(&mut []).unwrap_err().kind(), io::ErrorKind::TimedOut, "past its time");
    }

    /// A socket's reads come back at `EVERY` with nothing there; a pipe's can't be timed and are left as they are.
    #[test]
    fn a_sockets_reads_are_timed_and_a_pipes_left_alone() {
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        timed(&a).unwrap();
        assert_eq!(a.read_timeout().unwrap(), Some(EVERY));
        let (r, _w) = std::io::pipe().unwrap();
        assert_eq!(timed(&r).unwrap_err().raw_os_error(), Some(libc::ENOTSOCK));
    }
}
