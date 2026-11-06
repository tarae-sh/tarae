//! A loopback address that refuses a connection — and keeps refusing, which a port bound and let go doesn't.

use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};

/// A loopback address that refuses every connection while what it returns lives: the local port of a connection
/// of ours — nothing listens there, and on Linux the client's socket holds the port (no `SO_REUSEADDR`), so no other
/// bind takes it meanwhile, a port-0 one's pick included. A port bound and let go at once was closed only until the
/// next listener got it — under a full Linux run another test's `forbids` did, and "connection refused" read as its
/// 403 (#430: the kernel's next pick may be the very number just let go). (macOS lets a listener's `SO_REUSEADDR`
/// bind take a connected socket's port, and drops a connection to one bound and never listening instead of refusing
/// it: there the port is as free as a dropped one's.)
pub fn refused_at() -> ((TcpStream, TcpStream), SocketAddr) {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let client = TcpStream::connect(l.local_addr().unwrap()).unwrap();
    let (server, _) = l.accept().unwrap();
    let at = client.local_addr().unwrap();
    ((client, server), at)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `refused_at`'s address refuses a connection, and on Linux no listener's bind (`SO_REUSEADDR`, as std's) takes
    /// its port while it's held.
    #[test]
    fn a_refusing_port_is_taken_by_no_other_bind() {
        let (_held, closed) = refused_at();
        let refused = TcpStream::connect(closed).map(|_| ());
        assert_eq!(refused.map_err(|e| e.kind()), Err(std::io::ErrorKind::ConnectionRefused), "{closed}");
        #[cfg(target_os = "linux")]
        {
            let again = TcpListener::bind(closed).map(|_| ());
            assert_eq!(again.map_err(|e| e.kind()), Err(std::io::ErrorKind::AddrInUse), "{closed}");
        }
    }
}
