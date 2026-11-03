// A recording played (`recording::page`): the homepage player's decoding (tarae.sh's `public/player.js` —
// `patchRow`, `renderRowFromGrid`, keyframes) with a clock, play and pause, a scrubber and the time.
(() => {
  const rec = JSON.parse(document.getElementById("rec").textContent);
  const grid = document.getElementById("grid");
  const screenBox = document.getElementById("screen");
  const playBtn = document.getElementById("play");
  const zoomBtn = document.getElementById("zoom");
  const seek = document.getElementById("seek");
  const timeEl = document.getElementById("time");
  const captionEl = document.getElementById("caption");

  const cols = rec.cols || 80;
  const rows = rec.rows || 24;
  const frames = rec.frames || [];
  const beats = rec.beats || [];
  const end = frames.length ? frames[frames.length - 1].t : 0;
  const pal = rec.palette || [];
  document.documentElement.style.setProperty("--bg", pal[0] || "#07080b");
  document.documentElement.style.setProperty("--fg", pal[1] || "#e8e3d8");
  const styleCss = (rec.styles || []).map(([fg, bg, flags]) => {
    let css = `color:${pal[fg] || "inherit"}`;
    if (bg !== 0 && pal[bg]) css += `;background-color:${pal[bg]}`;
    if (flags & 1) css += ";font-weight:700";
    if (flags & 2) css += ";font-style:italic";
    if (flags & 4) css += ";text-decoration:underline";
    return css;
  });

  const rowEls = [];
  for (let r = 0; r < rows; r++) {
    const div = document.createElement("div");
    div.className = "r";
    grid.appendChild(div);
    rowEls.push(div);
  }
  const cursorEl = document.createElement("div");
  cursorEl.className = "cur";
  cursorEl.hidden = true;
  grid.appendChild(cursorEl);

  let zoomed = false;
  function fit() {
    const room = Math.max(200, screenBox.clientWidth - 16);
    let cellW = room / cols;
    // (zoomed: cells readable on a phone, the screen panned across)
    if (zoomed) cellW = Math.max(cellW, 7.5);
    cellW = Math.floor(cellW * 64) / 64;
    grid.style.setProperty("--cell-w", `${cellW}px`);
    grid.style.setProperty("--cols", String(cols));
  }
  window.addEventListener("resize", fit);
  zoomBtn.addEventListener("click", () => {
    zoomed = !zoomed;
    zoomBtn.textContent = zoomed ? "fit" : "zoom";
    fit();
  });

  function makeGrid() {
    const g = [];
    for (let r = 0; r < rows; r++) {
      const row = [];
      for (let c = 0; c < cols; c++) row.push({ s: 0, ch: " ", w: 0 });
      g.push(row);
    }
    return g;
  }
  let cells = makeGrid();

  function isWideChar(code) {
    return (
      code >= 0x1100 &&
      (code <= 0x115f ||
        code === 0x2329 ||
        code === 0x232a ||
        (code >= 0x2e80 && code <= 0xa4cf && code !== 0x303f) ||
        (code >= 0xac00 && code <= 0xd7a3) ||
        (code >= 0xf900 && code <= 0xfaff) ||
        (code >= 0xfe10 && code <= 0xfe6f) ||
        (code >= 0xff00 && code <= 0xff60) ||
        (code >= 0xffe0 && code <= 0xffe6))
    );
  }

  function patchRow(r, runs) {
    const row = cells[r];
    if (!row) return;
    for (const [startCol, styleIdx, text] of runs) {
      let c = startCol;
      for (const ch of text) {
        if (c >= cols) break;
        if (row[c].w === 2 && c > 0) row[c - 1] = { s: row[c - 1].s, ch: " ", w: 0 };
        if (isWideChar(ch.codePointAt(0))) {
          if (c + 1 < cols && row[c + 1].w === 1 && c + 2 < cols) row[c + 2] = { s: row[c + 2].s, ch: " ", w: 0 };
          row[c] = { s: styleIdx, ch, w: 1 };
          if (c + 1 < cols) row[c + 1] = { s: styleIdx, ch: "", w: 2 };
          c += 2;
        } else {
          if (row[c].w === 1 && c + 1 < cols) row[c + 1] = { s: row[c + 1].s, ch: " ", w: 0 };
          row[c] = { s: styleIdx, ch, w: 0 };
          c += 1;
        }
      }
    }
  }

  function box(cls, text, n) {
    const b = document.createElement("span");
    b.className = cls;
    b.textContent = text;
    if (n > 1) b.style.setProperty("--n", String(n));
    return b;
  }

  function renderRow(r) {
    const row = cells[r];
    const frag = document.createDocumentFragment();
    let c = 0;
    while (c < cols) {
      if (row[c].w === 2) {
        c++;
        continue;
      }
      const s = row[c].s;
      const span = document.createElement("span");
      if (styleCss[s]) span.style.cssText = styleCss[s];
      while (c < cols && row[c].s === s) {
        const cell = row[c];
        if (cell.w === 2) {
          c++;
        } else if (cell.w === 1) {
          span.appendChild(box("c w", cell.ch, 1));
          c += 2;
        } else if (cell.ch === " ") {
          let n = 0;
          while (c < cols && row[c].s === s && row[c].w === 0 && row[c].ch === " ") {
            n++;
            c++;
          }
          span.appendChild(box("c", " ".repeat(n), n));
        } else {
          span.appendChild(box("c", cell.ch, 1));
          c++;
        }
      }
      frag.appendChild(span);
    }
    rowEls[r].replaceChildren(frag);
  }

  function placeCursor(cursor) {
    if (!cursor) {
      cursorEl.hidden = true;
      return;
    }
    cursorEl.hidden = false;
    cursorEl.style.left = `calc(var(--cell-w) * ${cursor[0]})`;
    cursorEl.style.top = `${cursor[1] * 1.45}em`;
  }

  // (the frame drawn last, and the playback's own clock in ms)
  let at = -1;
  let t = 0;
  let playing = false;
  let last = 0;
  let dragging = false;

  function apply(f, dirty) {
    if (f.key) {
      cells = makeGrid();
      for (let r = 0; r < rows; r++) dirty.add(r);
    }
    for (const [r, runs] of Object.entries(f.rows || {})) {
      patchRow(Number(r), runs);
      dirty.add(Number(r));
    }
  }

  // The screen at `ms`: from the keyframe at or before it, or on from the frame drawn last when that's on the way.
  function show(ms) {
    let idx = -1;
    while (idx + 1 < frames.length && frames[idx + 1].t <= ms) idx++;
    if (idx < 0) idx = 0;
    if (idx === at) return;
    const dirty = new Set();
    let from = at + 1;
    if (idx < at || at < 0 || frames.slice(at + 1, idx + 1).some((f) => f.key)) {
      from = idx;
      while (from > 0 && !frames[from].key) from--;
    }
    for (let i = from; i <= idx; i++) apply(frames[i], dirty);
    for (const r of dirty) renderRow(r);
    placeCursor(frames[idx].cursor);
    at = idx;
  }

  function fmt(ms) {
    return `${(ms / 1000).toFixed(1)} s`;
  }

  function said() {
    show(t);
    timeEl.textContent = `${fmt(t)} / ${fmt(end)}`;
    if (!dragging) seek.value = String(Math.round(t));
    let label = "";
    for (const b of beats) if (t >= b.at) label = b.label;
    captionEl.textContent = label;
  }

  function setPlaying(p) {
    playing = p;
    playBtn.textContent = playing ? "pause" : "play";
    if (playing) {
      if (t >= end) t = 0;
      last = performance.now();
      requestAnimationFrame(tick);
    }
  }

  function tick(now) {
    if (!playing) return;
    t = Math.min(end, t + (now - last));
    last = now;
    said();
    if (t >= end) setPlaying(false);
    else requestAnimationFrame(tick);
  }

  seek.max = String(end);
  seek.addEventListener("input", () => {
    dragging = true;
    t = Number(seek.value);
    last = performance.now();
    said();
  });
  seek.addEventListener("change", () => {
    dragging = false;
  });
  playBtn.addEventListener("click", () => setPlaying(!playing));
  grid.addEventListener("click", () => setPlaying(!playing));
  document.addEventListener("keydown", (e) => {
    if (e.key === " ") {
      e.preventDefault();
      setPlaying(!playing);
    } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      t = Math.max(0, Math.min(end, t + (e.key === "ArrowLeft" ? -2000 : 2000)));
      last = performance.now();
      said();
    }
  });
  document.addEventListener("visibilitychange", () => {
    if (document.hidden && playing) setPlaying(false);
  });

  fit();
  said();
})();
