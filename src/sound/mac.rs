//! macOS's output: the default output AudioUnit (AUHAL) — it follows the system's output device, volume and mute
//! by itself — fed 48 kHz mono f32 from a render callback (`Player::fill`). AudioToolbox through a few `extern "C"`
//! declarations of our own, no crate: the prototype measured cpal +84 KB and 13 crates for the same (sound-plan §2).
//! Made, started and stopped on the sound thread only; the callback runs on the system's real-time IO thread.

use std::ffi::c_void;
use std::sync::Arc;

use super::engine::{Output, Player, Ring};

// The C types the calls below take, as AudioToolbox's headers lay them out (AudioComponent.h, CoreAudioTypes.h,
// AUComponent.h): only their fields, no behaviour.
#[repr(C)]
struct ComponentDescription {
    kind: u32,
    sub: u32,
    maker: u32,
    flags: u32,
    mask: u32,
}

#[repr(C)]
struct StreamFormat {
    rate: f64,
    format: u32,
    flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels: u32,
    bits: u32,
    reserved: u32,
}

#[repr(C)]
struct Buffer {
    channels: u32,
    size: u32,
    data: *mut c_void,
}

#[repr(C)]
struct BufferList {
    n: u32,
    // (a C flexible array: `n` of them; ours is mono interleaved, so one)
    buffers: [Buffer; 1],
}

type Render = extern "C" fn(*mut c_void, *mut u32, *const c_void, u32, u32, *mut BufferList) -> i32;

#[repr(C)]
struct RenderCallback {
    render: Render,
    refcon: *mut c_void,
}

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioComponentFindNext(after: *mut c_void, desc: *const ComponentDescription) -> *mut c_void;
    fn AudioComponentInstanceNew(component: *mut c_void, out: *mut *mut c_void) -> i32;
    fn AudioComponentInstanceDispose(unit: *mut c_void) -> i32;
    fn AudioUnitSetProperty(
        unit: *mut c_void,
        id: u32,
        scope: u32,
        el: u32,
        data: *const c_void,
        size: u32,
    ) -> i32;
    fn AudioUnitInitialize(unit: *mut c_void) -> i32;
    fn AudioOutputUnitStart(unit: *mut c_void) -> i32;
    fn AudioOutputUnitStop(unit: *mut c_void) -> i32;
}

const fn code(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

/// `kAudioUnitProperty_StreamFormat`, `kAudioUnitProperty_SetRenderCallback`, `kAudioUnitScope_Input`.
const STREAM_FORMAT: u32 = 8;
const SET_RENDER_CALLBACK: u32 = 23;
const SCOPE_INPUT: u32 = 1;
/// `kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked`.
const FLOAT_PACKED: u32 = 1 | 8;

const RATE: f64 = 48_000.0;

pub struct Mac {
    unit: *mut c_void,
    /// What the callback's `refcon` points at: owned here, freed only after the unit is gone (`Drop`).
    player: *mut Player,
    running: bool,
}

/// The default output, made and initialized (not started: no callback, nothing running until `start`).
pub fn open(ring: Arc<Ring>) -> Result<Mac, String> {
    let desc = ComponentDescription {
        kind: code(b"auou"),
        sub: code(b"def "),
        maker: code(b"appl"),
        flags: 0,
        mask: 0,
    };
    // SAFETY: `desc` is a valid description for the call's duration; a null `after` asks for the first match.
    let component = unsafe { AudioComponentFindNext(std::ptr::null_mut(), &desc) };
    if component.is_null() {
        return Err("no default output on this Mac".into());
    }
    let mut unit = std::ptr::null_mut();
    // SAFETY: `component` came from AudioComponentFindNext; `unit` is ours to be written.
    let made = unsafe { AudioComponentInstanceNew(component, &mut unit) };
    if made != 0 || unit.is_null() {
        return Err(format!("the output wouldn't open (OSStatus {made})"));
    }
    let mac = Mac { unit, player: Box::into_raw(Box::new(Player::new(ring, RATE as f32))), running: false };
    let format = StreamFormat {
        rate: RATE,
        format: code(b"lpcm"),
        flags: FLOAT_PACKED,
        bytes_per_packet: 4,
        frames_per_packet: 1,
        bytes_per_frame: 4,
        channels: 1,
        bits: 32,
        reserved: 0,
    };
    let callback = RenderCallback { render, refcon: mac.player.cast() };
    // SAFETY: the unit is a live instance; each property is passed as its documented struct, by pointer and size,
    // read during the call (the callback's `refcon` outlives the unit: `Drop` disposes the unit first).
    let status = unsafe {
        match AudioUnitSetProperty(
            unit,
            STREAM_FORMAT,
            SCOPE_INPUT,
            0,
            (&raw const format).cast(),
            size_of_val(&format) as u32,
        ) {
            0 => match AudioUnitSetProperty(
                unit,
                SET_RENDER_CALLBACK,
                SCOPE_INPUT,
                0,
                (&raw const callback).cast(),
                size_of_val(&callback) as u32,
            ) {
                0 => AudioUnitInitialize(unit),
                e => e,
            },
            e => e,
        }
    };
    // (dropped on failure: the unit disposed, the player freed)
    match status {
        0 => Ok(mac),
        e => Err(format!("the output wouldn't take 48 kHz mono (OSStatus {e})")),
    }
}

impl Output for Mac {
    fn start(&mut self) -> Result<(), String> {
        // SAFETY: an initialized unit, started from the thread that owns it.
        match unsafe { AudioOutputUnitStart(self.unit) } {
            0 => {
                self.running = true;
                Ok(())
            }
            e => Err(format!("OSStatus {e}")),
        }
    }

    fn stop(&mut self) {
        // SAFETY: as `start`; stopping returns once the callback no longer runs.
        unsafe { AudioOutputUnitStop(self.unit) };
        self.running = false;
    }
}

impl Drop for Mac {
    fn drop(&mut self) {
        // SAFETY: the unit is stopped and disposed first, so the callback can't run again; only then is the player
        // it pointed at freed (made by `Box::into_raw` in `open`, freed once here).
        unsafe {
            if self.running {
                AudioOutputUnitStop(self.unit);
            }
            AudioComponentInstanceDispose(self.unit);
            drop(Box::from_raw(self.player));
        }
    }
}

/// The render callback: the next `frames` samples into the unit's buffer.
extern "C" fn render(
    refcon: *mut c_void,
    _flags: *mut u32,
    _time: *const c_void,
    _bus: u32,
    frames: u32,
    data: *mut BufferList,
) -> i32 {
    if refcon.is_null() || data.is_null() {
        return 0;
    }
    // SAFETY: `refcon` is the `Player` `open` gave the unit, alive until the unit is disposed, and only this callback
    // touches it while the unit runs; `data` is the unit's buffer list for this call, its first buffer `size` bytes
    // of f32 (the mono float format we set) — never more than that is written.
    unsafe {
        let player = &mut *refcon.cast::<Player>();
        if (*data).n == 0 {
            return 0;
        }
        let buf = &mut (*data).buffers[0];
        if buf.data.is_null() {
            return 0;
        }
        let n = (frames as usize).min(buf.size as usize / size_of::<f32>());
        player.fill(std::slice::from_raw_parts_mut(buf.data.cast::<f32>(), n));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::super::engine::{Msg, spawn};
    use super::super::said::{Kind, Said};

    /// This Mac's output, opened by the thread as tarae opens it, a phrase played: heard by hand.
    #[test]
    #[ignore = "by hand: plays sounds on this Mac's output"]
    fn the_output_plays_a_phrase_by_hand() {
        let tx = spawn(30).expect("the output opens");
        for (i, kind) in
            [Kind::Key, Kind::Key, Kind::Space, Kind::Key, Kind::Return, Kind::Settle].into_iter().enumerate()
        {
            tx.send(Msg::Play(Said { kind, gain: 1.0, seed: i as u32 + 1, delay: 0.0 })).unwrap();
            crate::event::sleep(std::time::Duration::from_millis(110));
        }
        crate::event::sleep(std::time::Duration::from_millis(600));
    }
}
