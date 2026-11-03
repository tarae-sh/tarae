//! What a remote interface's wire would carry — the remote split's spike (`.sb/remote-plan.md`). tarae already writes
//! its terminal only what changed (`screen.rs`: cell runs, cursor moves, SGR), so a recorded scene's pty output, frame
//! by frame, is what a host would send an interface today: `cargo xtask scenes --wire scene …` prints it. Frames that
//! change nothing in sight (the resting redraw that heals a real terminal's drift) are counted apart — a wire to a
//! client whose grid is exact wouldn't need them.

use flate2::{Compress, Compression, FlushCompress};

/// Framing and encryption a frame on the wire, assumed: a WebSocket frame's header (2–4 bytes) and a TLS 1.3 record's
/// (5 header + 1 type + 16 tag), rounded up.
pub(crate) const PER_FRAME: u64 = 30;

/// A recording's wire, summed.
#[derive(Debug, PartialEq)]
pub(crate) struct Report {
    pub(crate) secs: f64,
    pub(crate) frames: usize,
    pub(crate) bytes: u64,
    /// The most bytes in any one second (a window from a frame's time).
    pub(crate) peak_second: u64,
    pub(crate) median: u64,
    pub(crate) p90: u64,
    pub(crate) max: u64,
    /// Frames that changed nothing in sight, and their bytes.
    pub(crate) same: usize,
    pub(crate) same_bytes: u64,
    /// The frames that changed something, deflated as one stream flushed after each frame (WebSocket's
    /// permessage-deflate with its context kept).
    pub(crate) deflated: u64,
}

/// `frames`: each frame's time (ms from the recording's start), its bytes, and whether it changed nothing in sight.
pub(crate) fn report(frames: &[(u64, Vec<u8>, bool)]) -> Report {
    let mut sizes: Vec<u64> = frames.iter().map(|f| f.1.len() as u64).collect();
    let bytes = sizes.iter().sum();
    let peak_second = (0..frames.len())
        .map(|i| frames[i..].iter().take_while(|f| f.0 < frames[i].0 + 1000).map(|f| f.1.len() as u64).sum())
        .max()
        .unwrap_or(0);
    sizes.sort_unstable();
    let at = |q: f64| sizes.get(((sizes.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0);
    let (same, same_bytes) =
        frames.iter().filter(|f| f.2).fold((0, 0), |(n, b), f| (n + 1, b + f.1.len() as u64));
    let mut z = Compress::new(Compression::default(), false);
    let mut out = Vec::with_capacity(1 << 16);
    for (_, b, _) in frames.iter().filter(|f| !f.2) {
        out.clear();
        out.reserve(b.len() + 64);
        let _ = z.compress_vec(b, &mut out, FlushCompress::Sync);
    }
    Report {
        secs: frames.last().map_or(0, |f| f.0) as f64 / 1000.0,
        frames: frames.len(),
        bytes,
        peak_second,
        median: at(0.5),
        p90: at(0.9),
        max: sizes.last().copied().unwrap_or(0),
        same,
        same_bytes,
        deflated: z.total_out(),
    }
}

impl Report {
    /// One line: the scene's numbers, bytes a second over its length and over its busiest second.
    pub(crate) fn line(&self, name: &str, size: (u16, u16)) -> String {
        let per = |b: u64| if self.secs > 0.0 { b as f64 / self.secs } else { 0.0 };
        let kept = self.frames - self.same;
        let framed = self.bytes - self.same_bytes + PER_FRAME * kept as u64;
        format!(
            "{name} {}×{}: {:.1} s, {} frames ({:.1}/s), {} B = {:.0} B/s, busiest second {} B; a frame median {} B, p90 {}, \
             max {}; {} changed nothing ({} B); without them, +{PER_FRAME} B a frame framed: {:.0} B/s; deflated: {} B = \
             {:.0} B/s",
            size.0,
            size.1,
            self.secs,
            self.frames,
            per(self.frames as u64),
            self.bytes,
            per(self.bytes),
            self.peak_second,
            self.median,
            self.p90,
            self.max,
            self.same,
            self.same_bytes,
            per(framed),
            self.deflated,
            per(self.deflated + PER_FRAME * kept as u64),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_a_recordings_frames_its_busiest_second_and_what_changed_nothing() {
        let f = |t, n: usize, same| (t, vec![b'x'; n], same);
        let r = report(&[f(0, 100, false), f(500, 300, false), f(900, 50, true), f(1600, 10, false)]);
        assert_eq!((r.frames, r.bytes, r.same, r.same_bytes), (4, 460, 1, 50));
        // (0..1000: 450; 500..1500: 350; 900..1900: 60)
        assert_eq!(r.peak_second, 450);
        assert_eq!((r.median, r.p90, r.max), (100, 300, 300));
        assert_eq!(r.secs, 1.6);
        assert!(r.deflated > 0 && r.deflated < 410, "repeated bytes deflate: {}", r.deflated);
    }
}
