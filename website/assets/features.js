(function () {
  const GUIDE = "https://github.com/lvndry/euphoria/blob/main/docs/guides/";
  const FEATURES = [
    {
      id: "library", name: "Library and analysis", ratio: [3, 2], hue: 240,
      line: "Key, tempo and energy for every track, in one table.",
      pitch: "Add a folder and Euphoria reads every track once: its key, its tempo, its energy and its audio quality, cached so the next launch is instant.",
      figures: [{ src: "images/desktop-app/main-window-paper.png", alt: "The Euphoria library in the Paper theme: a folder tree, a table of tracks with Camelot key chips, tempo and energy, and the inspector of the selected track." }],
      text: ["Keys show in Camelot and in musical notation, tempos come from the beat grid, and energy uses Euphoria's own documented scale from 1 to 10.", "Select a track and the inspector shows its place on the Camelot wheel, its waveform with cues, what else its key and tempo could be, and the tracks that mix well with it."],
      can: ["Filter by tempo, energy, quality and stage", "Sort and resize every column", "See the tracks that mix well with the selected one", "Look for new and removed files with one rescan"],
      keys: [["⌘O", "Add a folder"], ["⌘R", "Rescan"]],
      guide: "desktop-app.md#the-track-table",
    },
    {
      id: "review", name: "Needs review", ratio: [2, 1], hue: 30,
      line: "The unsure tracks, gathered for you to check by ear.",
      pitch: "A key or tempo rated low confidence, a key that disagrees with the file's tags, or a file that could not be analyzed lands in one list.",
      figures: [{ src: "images/desktop-app/needs-review-list.png", alt: "The Needs review list with tracks marked Key unsure or Tempo unsure, and a piano keyboard that marks the notes of 10A." }],
      text: ["Each track says why it is there: Key unsure, Tempo unsure, or a key that differs from the one in its tags.", "Play the notes of the key on the keyboard in the inspector and hear which fit, shift the key to hear what it mixes with there, then correct it in one click."],
      can: ["Check a key by ear with the marked notes", "See the runner-up key and tempo", "Settle a track once it is right", "Corrections are used by every export"],
      keys: [],
      guide: "desktop-app.md#settle-a-track-under-needs-review",
    },
    {
      id: "palette", name: "Command palette", ratio: [3, 1], hue: 60,
      line: "Find any track or command with ⌘K.",
      pitch: "One box finds tracks and runs commands. It understands what a DJ types: 8A 124 deep finds tracks in 8A or a key that mixes with it, near 124 BPM, with deep in the name.",
      figures: [{ src: "images/desktop-app/command-palette.png", alt: "The command palette open over the library, listing recent tracks and common commands." }],
      text: ["Type a key, a tempo, a tempo range or an energy, mixed with words in any order. The exact key comes first, then one step on the wheel and the relative key.", "Under the tracks, every menu command is listed with its shortcut, so the palette teaches the keyboard as you go."],
      can: ["Keys: 8A, Am, A minor", "Tempos: 124, 120-126", "Energy: e6, e7-9, energy 6+", "Enter selects, ⌘Enter plays"],
      keys: [["⌘K", "Open the palette"]],
      guide: "desktop-app.md#find-anything-with-k",
    },
    {
      id: "focus", name: "Focus view", ratio: [4, 3], hue: 150,
      line: "One track, every lane, and the plan for the next.",
      pitch: "The focus view gives the selected track the whole window: its sections, a zoomed waveform with cues, its energy and where each stem sounds, on one time axis.",
      figures: [{ src: "images/desktop-app/focus-view.png", alt: "The focus view: the track's sections, detailed waveform with cue flags, energy lane and stem lanes on one timeline, with the tracks it mixes into and the Plan card." }],
      text: ["Drag cue flags onto beats, draw a loop with Command held, and change the playing tempo with key lock. Nothing is written to the file.", "Pick a track in Mixes into and the Plan card writes the best transition in a sentence: which sections to mix, how many bars, where to swap the bass, and at what tempo."],
      can: ["Loops of ½ beat to 16 bars, saved with the track", "Tempo control of 16% either way, with key lock", "Set the planned cues on both tracks in one click", "Open the transition on the decks"],
      keys: [["Enter", "Open on a row"], ["L", "Loop 4 bars"], ["Esc", "Back to the library"]],
      guide: "desktop-app.md#focus-view",
    },
    {
      id: "decks", name: "Decks and mashups", ratio: [1, 1], hue: 180,
      line: "Try a transition on two decks before the set.",
      pitch: "Two decks under the table, with sync, key lock, loops and a crossfader, so you can hear a transition before you play it out.",
      figures: [{ src: "images/desktop-app/decks.png", alt: "Two decks under the library table, each with its key, tempo, waveform, cues and loop buttons, and a mixer with volume faders and a crossfader." }],
      text: ["Each deck shows its key, its tempo, its waveform with bars and cues, and its energy over time. Playback lines up the downbeats of both decks.", "Next on deck B lists what mixes well after deck A, ranked by key move, tempo and the energy you want next. Mashup mode swaps it for tracks that layer over deck A, with a key shift and stems."],
      can: ["Sync to A and key lock, on by default", "Loop 1 to 16 bars on the beat", "Narrow suggestions by key move, tempo, energy and genre", "Export a mashup to a WAV file"],
      keys: [["⌘⇧D", "Show or hide the decks"]],
      guide: "desktop-app.md#decks",
    },
    {
      id: "set", name: "Set builder and playlists", ratio: [5, 3], hue: 270,
      line: "Draw an energy curve. Get a set in key.",
      pitch: "Draw the energy you want across the night, choose a pool of tracks and a length, and Euphoria orders a set along the curve with every mix in a compatible key.",
      figures: [{ src: "images/desktop-app/set-builder.png", alt: "The set builder after building an 11 track set on the Journey curve: a bar per track against the curve, chips for how far it strays, one rule break and 22 minutes, and the set listed with key moves and tempo steps." }],
      text: ["Each track becomes a bar on the curve, so you see at once where the set fits and where it does not. A chip row sums up how far it strays, how many rules it breaks and how long it runs.", "Lock tracks, reorder them, play any transition, then render the whole set as one continuous mix or save it as a playlist with its key path, tempo range and energy flow."],
      can: ["Presets: Warm up, Peak time, Journey", "From 2 to 60 tracks, from a folder or a playlist", "Render to FLAC, MP3 with chapters, or WAV", "Export playlists to rekordbox or M3U8"],
      keys: [["⌘⇧B", "Open the set builder"], ["⌘Enter", "Build the set"]],
      guide: "desktop-app.md#build-a-set",
    },
    {
      id: "live", name: "Live", ratio: [4, 1], hue: 300,
      line: "The key of whatever your Mac is playing.",
      pitch: "A small panel stays on top and shows the key and tempo of whatever the Mac is playing, with the tracks of your library that mix in.",
      figures: [{ src: "images/live/live-panel-paper.png", alt: "The Live panel showing the key and tempo of the audio playing on the Mac and the library tracks that mix with it." }],
      text: ["Play a track in rekordbox, Spotify or a browser, or a turntable through an audio interface, and Live names its key, tempo and energy as it plays. Nothing is recorded.", "It keeps running in the menu bar when you close the window."],
      can: ["Key and tempo of anything playing", "Tracks from your library that mix in", "Stays on top of other windows", "Lives in the menu bar"],
      keys: [["⌘⇧L", "Open Live"]],
      guide: "euphoria-live.md",
    },
    {
      id: "stems", name: "Stems", ratio: [5, 2], hue: 120,
      line: "Hear the vocals, drums, bass and the rest on their own.",
      pitch: "Separate a track into its four stems with the Demucs model, then play the full track, the acapella or the instrumental, or any stem alone.",
      figures: [{ src: "images/lanes/lanes.svg", alt: "A chart of the lanes under the waveform: where vocals, drums, bass and the rest sound over the length of a track.", chart: true, caption: "The lanes the stems feed: where each part sounds, second by second." }],
      text: ["Stems feed the lanes of the focus view and the planner, which use them to find windows where two tracks never sing or play basslines at once.", "Export stems, the acapella or the instrumental as WAV files, also in a shifted key."],
      can: ["Full track, acapella or instrumental", "Solo or mute any stem in the focus view", "Vocals and bass overlap check in the planner", "Export stems as WAV"],
      keys: [],
      guide: "desktop-app.md#stems",
    },
    {
      id: "tags", name: "Tags", ratio: [5, 4], hue: 330,
      line: "Write keys into your files, after seeing every change.",
      pitch: "Write key, tempo and energy into the tags your DJ software reads, or edit title, artist, genre and other fields across thousands of files at once.",
      figures: [
        { src: "images/desktop-app/write-tags-preview.png", alt: "The Write tags for rekordbox preview: each file with its key, BPM and comment changes, and the rules in use." },
      ],
      text: ["Every file and every field is listed before anything is written. Keys and tempos already in your files are kept, and unsure values are skipped.", "Undo puts the old tags back byte for byte, from the app or with euphoria tags undo."],
      can: ["Key notation of your choice, Camelot by default", "Find and replace, prefixes and suffixes", "Fill empty key and BPM fields from the analysis", "Undo the last write"],
      keys: [["⌘⇧T", "Write tags"], ["⌘I", "Edit tags"]],
      guide: "write-tags.md",
    },
    {
      id: "quality", name: "Audio quality", ratio: [7, 4], hue: 0,
      line: "Find the fake lossless and the files to replace.",
      pitch: "Euphoria checks the codec, the real bit rate and where the content stops, and flags files likely re-encoded from a lower quality source.",
      figures: [{ src: "images/audio-quality/verdicts.svg", alt: "A scatter of each encoding's lowest cutoff against its measured bit rate, over the zones of the weak, acceptable and good verdicts.", chart: true, caption: "How the real bit rate and the cutoff decide the verdict." }],
      text: ["euphoria quality lists the files to replace, worst first, with the reason: low bit rate, fake lossless or damaged. It writes the list as CSV or as an M3U8 playlist too.", "Prepare files writes copies that play evenly on a club system: to a loudness target, under a true-peak ceiling, with tuning corrected if you want."],
      can: ["Real bit rate and cutoff, per file", "Fake lossless detection", "Loudness targets from -14 to -7 LUFS", "ReplayGain tags without touching the audio"],
      keys: [],
      guide: "prepare-files.md",
    },
    {
      id: "export", name: "Exports and command line", ratio: [7, 5], hue: 210,
      line: "Send it all to rekordbox, Traktor, Serato and more.",
      pitch: "Analyse once, then send keys, tempos, beat grids, cues and playlists to the library you play from, from the app or the command line.",
      term: true,
      text: ["rekordbox gets an XML file with keys, tempos, energy, saved loops and a playlist per folder. Traktor gets an NML collection. Serato, Engine DJ and VirtualDJ get their cues and grids written in, after a copy is saved.", "Every export and analysis is also a command, with table or JSON output you can script."],
      can: ["rekordbox XML, tested", "Traktor NML, not yet tested", "Serato tags, not yet tested", "Engine DJ and VirtualDJ, not yet tested"],
      keys: [],
      guide: "dj-programs.md",
    },
  ];

  const point = (larger, smaller, turn) => [32 + 24 * Math.sin(larger * 2 * Math.PI * turn), 32 + 24 * Math.cos(smaller * 2 * Math.PI * turn)];
  function spanFor(larger, smaller) {
    for (let step = 0; step <= 200; step++) {
      const turn = 0.25 + 0.5 * step / 200;
      const [ax, ay] = point(larger, smaller, turn);
      const [bx, by] = point(larger, smaller, 1.5 - turn);
      if (Math.abs(ax - bx) > 1e-3 || Math.abs(ay - by) > 1e-3) return [0.25, 1.25];
    }
    return [0.25, 0.75];
  }
  function curvePath(larger, smaller, from, to) {
    const [start, end] = spanFor(larger, smaller);
    const count = Math.max(2, Math.ceil((to - from) / (end - start) * Math.max(300, (larger + smaller) * 70) * (end - start) / 0.5));
    let path = "";
    for (let index = 0; index < count; index++) {
      const [across, down] = point(larger, smaller, from + (to - from) * index / (count - 1));
      path += (index ? "L" : "M") + across.toFixed(2) + " " + down.toFixed(2);
    }
    return path;
  }
  const fullCurve = (larger, smaller) => curvePath(larger, smaller, ...spanFor(larger, smaller));
  const strokeFor = (larger, smaller, base) => (base * Math.min(1, Math.sqrt(7 / (larger + smaller)))).toFixed(2);
  document.querySelectorAll("path.full").forEach((path) => path.setAttribute("d", fullCurve(3, 2)));
  const tick = fullCurve(3, 2);

  const rail = document.getElementById("rail");
  const panels = document.getElementById("panels");
  rail.innerHTML = FEATURES.map((feature) => `<button class="tab" role="tab" type="button" id="tab-${feature.id}" aria-controls="panel-${feature.id}" aria-selected="false" style="--hue:${feature.hue}"><span class="icon"><svg viewBox="0 0 64 64" aria-hidden="true"><path class="mark-path" data-curve="${feature.ratio.join(":")}" stroke-width="${strokeFor(...feature.ratio, 3.4)}" d="${fullCurve(...feature.ratio)}"></path></svg></span><span><b>${feature.name}</b><span>${feature.line}</span></span></button>`).join("");

  panels.innerHTML = FEATURES.map((feature, index) => {
    const figures = feature.term
      ? `<pre class="term"><span class="p">$</span> euphoria analyze "~/Music/DJ tracks" --json
<span class="p">$</span> euphoria quality ~/Music --m3u8 replace.m3u8
<span class="p">$</span> euphoria export rekordbox ~/Music --beat-grid --cues
<span class="p">$</span> euphoria export traktor ~/Music --beat-grid --cues
<span class="p">$</span> euphoria tags write --serato-markers ~/Music
<span class="p">$</span> euphoria tags undo</pre>`
      : `<div class="${feature.figures.length > 1 ? "figures-two" : ""}">${feature.figures.map((figure) => `<figure class="figure${figure.chart ? " chart" : ""}" style="margin:0"><img src="${figure.src}" alt="${figure.alt}" loading="${index ? "lazy" : "eager"}">${figure.caption ? `<figcaption>${figure.caption}</figcaption>` : ""}</figure>`).join("")}</div>`;
    const keys = feature.keys.length ? `<div class="keys">${feature.keys.map(([key, what]) => `<span class="kbd"><kbd>${key}</kbd>${what}</span>`).join("")}</div>` : "";
    const previous = FEATURES[(index - 1 + FEATURES.length) % FEATURES.length];
    const next = FEATURES[(index + 1) % FEATURES.length];
    return `<section class="panel" role="tabpanel" id="panel-${feature.id}" aria-labelledby="tab-${feature.id}" style="--hue:${feature.hue}" hidden>
      <div class="panel-title"><h2>${feature.name}</h2><p class="pitch">${feature.pitch}</p></div>
      ${figures}
      <div class="body">
        <div>${feature.text.map((paragraph) => `<p>${paragraph}</p>`).join("")}${keys}<a class="guide" href="${GUIDE}${feature.guide}">Read the guide</a></div>
        <div><h3>What you can do</h3><ul>${feature.can.map((item) => `<li><svg viewBox="0 0 64 64" aria-hidden="true"><path class="mark-path" stroke-width="7" d="${tick}"></path></svg><span>${item}</span></li>`).join("")}</ul></div>
      </div>
      <div class="next"><button type="button" data-go="${previous.id}">← ${previous.name}</button><button type="button" data-go="${next.id}">${next.name} →</button></div>
    </section>`;
  }).join("");

  function show(id, scroll) {
    const feature = FEATURES.find((candidate) => candidate.id === id) || FEATURES[0];
    FEATURES.forEach((candidate) => {
      const selected = candidate === feature;
      document.getElementById(`tab-${candidate.id}`).setAttribute("aria-selected", String(selected));
      document.getElementById(`panel-${candidate.id}`).hidden = !selected;
    });
    const tab = document.getElementById(`tab-${feature.id}`);
    if (tab.scrollIntoView && window.matchMedia("(max-width: 980px)").matches) tab.scrollIntoView({ inline: "center", block: "nearest" });
    if (location.hash.slice(1) !== feature.id) history.replaceState(null, "", `#${feature.id}`);
    if (scroll) document.getElementById("panels").scrollIntoView({ block: "start" });
  }
  rail.addEventListener("click", (event) => { const tab = event.target.closest(".tab"); if (tab) show(tab.id.slice(4), window.matchMedia("(max-width: 980px)").matches); });
  rail.addEventListener("keydown", (event) => {
    if (!["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight"].includes(event.key)) return;
    event.preventDefault();
    const tabs = Array.from(rail.querySelectorAll(".tab"));
    const index = tabs.indexOf(document.activeElement);
    const step = event.key === "ArrowDown" || event.key === "ArrowRight" ? 1 : -1;
    const target = tabs[(index + step + tabs.length) % tabs.length];
    target.focus();
    show(target.id.slice(4), false);
  });
  panels.addEventListener("click", (event) => { const button = event.target.closest("[data-go]"); if (button) show(button.dataset.go, true); });
  window.addEventListener("hashchange", () => show(location.hash.slice(1), false));
  show(location.hash.slice(1), false);
})();
