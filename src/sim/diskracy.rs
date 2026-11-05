//! The disk watch and a write that keeps the stamp, simulated (#286): an open file, and at random another program's
//! write of the same size — within `disk::RACY` of the file's mtime at that very mtime (a coarse clock's same tick),
//! past it at the clock's now (a later write's mtime always moves) —, a disk round asked (a focus regain's), time
//! passing past the grain or not, a frame. The file's times are the simulated wall clock's (`sim::write`), so whether a
//! stamp is racy is the seed's. At the end: the buffer holds the file wherever a round was asked after its last write.

use std::cell::Cell;
use std::path::Path;
use std::time::Duration;

use super::{Choices, Sim};
use crate::editor::Editor;

// (6: two rounds' looks land out of order, the older laid over the newer — the same stamp, #381)
const SEEDS: &[u64] = &[0, 1, 2, 3, 6];

const INPUTS: usize = 40;

const LIMIT: Duration = crate::test_results::PATIENCE;

#[test]
fn a_write_that_keeps_the_stamp_is_read() {
    super::seeds("a_write_that_keeps_the_stamp_is_read", SEEDS, run);
}

fn run(choices: Choices) -> Vec<String> {
    let dir = crate::scratch::Scratch::new("sim-diskracy");
    let path = dir.join("a.txt");
    let mut sim = Sim::new("a_write_that_keeps_the_stamp_is_read", choices, LIMIT);
    super::write(&path, "one\n");
    let mut ed = Editor::new(crate::config::Config::default());
    ed.open(&path).unwrap();
    // (a round asked since the last write)
    let looked = Cell::new(true);
    sim.walk(
        &mut ed,
        INPUTS,
        |sim, ed| input(sim, ed, &path, &looked),
        |_| Ok(()),
        |ed| {
            format!("buffer {:?} · file {:?}", ed.doc().text.to_string(), std::fs::read_to_string(&path).ok())
        },
    );
    sim.at_end(|| {
        let file = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        match (looked.get(), ed.doc().text.to_string()) {
            (true, held) if held != file => {
                Err(format!("a round after the last write, the buffer {held:?}, the file {file:?}"))
            }
            _ => Ok(()),
        }
    });
    let trace = sim.trace().to_vec();
    drop(sim);
    drop(ed);
    trace
}

fn input(sim: &mut Sim, ed: &mut Editor, path: &Path, looked: &Cell<bool>) -> String {
    let c = &mut sim.choices;
    // (by arm: a write 3, a round 3, time 2, a frame 1)
    match c.weighted(&[3, 3, 2, 1]) {
        0 => {
            let text = *c.pick(&["one\n", "two\n", "six\n"]);
            let was = std::fs::metadata(path).and_then(|m| m.modified()).unwrap();
            looked.set(false);
            // (within the grain the filesystem's clock may not have moved: the same mtime)
            if was + crate::disk::RACY > crate::clock::system() {
                std::fs::write(path, text).unwrap();
                let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
                file.set_modified(was).unwrap();
                format!("{text:?} written in its stamp's tick")
            } else {
                super::write(path, text);
                format!("{text:?} written")
            }
        }
        1 => {
            ed.on_focus(true);
            // (this round only: its landing carries on no waiting round — the OS's wait, not modelled)
            ed.disk_poll_gen += 1;
            looked.set(true);
            "a disk round".into()
        }
        2 => {
            let ms = *c.pick(&[1, 300, 2500]);
            sim.wait(Duration::from_millis(ms));
            format!("{ms} ms pass")
        }
        _ => {
            crate::term::render(ed, &mut Vec::new(), 100, 30).unwrap();
            "a frame".into()
        }
    }
}
