# kimo features

New in v8.3.0: montage mode for non-dialogue footage (travel, sports, pets, raw phone clips) --
`score_windows`/`best_segments` score and cut down to the strongest moments (sharpness, exposure,
subject framing, motion, shake), `beat_sync_montage` cuts on real detected beats with shot length
varying by musical energy and a clean phrase-aligned fade-out, `montage_color_match` ties a whole
montage's clips to one consistent look. No voice/audio-leveling or caption work this pass (steered
off); the talking-head path (`auto_edit`, `auto_rough_cut`) and its benchmark are untouched. Tools:
522 -> 526.

New in v8.2.0: visual finish quality -- a slow Ken Burns push-in on static b-roll cutaways
(`add_smart_punch`, already in the registry, now wired into `auto_edit` as `push_in`, default on,
zoom 5%); the talking-head jump-cut punch-ins now force a split on any uncovered stretch that would
otherwise sit static past `punch_max_static` seconds (default 4.5), so a continuous take with no
natural pause still alternates wide/tight; captions default to `auto` placement (`caption_position`),
which steers around a low face/skin band instead of `auto_edit`'s old hardcoded bottom; a short
whip/zoom-blur transition into each b-roll cutaway (`broll_transitions`, default on), never on the
talking-head's own jump-cuts. No voice/audio work this pass (steered off). No new tools -- every piece
wires an existing, already-tested engine tool into `auto_edit`'s defaults. Tools: 522 (unchanged).

New in v8.1.0: `level_dynamics` (the master-bus speech leveler) scoped to the dialogue span instead of
the whole mix, so an end card or music-only tail can no longer be reached by it; the benchmark's
whisper-recall gate now scores dialogue-span recall separately from a whole-file number (the real
cause of v8.0.0's "56.7% collapse" -- a whisper whole-file transcription artifact, not mix damage, not
speechnorm); the end-card swell raised 3dB -> 11dB now that both fixes are in place (-18.4 LUFS,
inside the originally-asked range). Voice dynamics (LRA) investigated further and still short of the
>=3 target -- see below. No new tools.

New in v8.0.0: the 3.0s dead-air tail at the end of every render fixed (the music bed now covers the
full export including the end screen, looped when short); a new `match_shot_look` tool pulls b-roll
color/exposure toward the talking head's own look (wired into `auto_edit` as `broll_color_match`,
default on); an end-card music swell once dialogue ends, capped at a measured-safe 3dB after 8dB
silently collapsed whisper recall via `finish_audio`'s master-bus speechnorm. Tools: 521 -> 522.

New in v7.9.0: a real-footage voice-dynamics pass (honest, partial -- see below), plus a new `whoosh_sfx`
option on `place_broll`'s b-roll cut-ins and `auto_edit`'s hook title, synthesized at call time like the
existing pop SFX -- built, TDD'd, and left OFF by default after it measurably cost whisper word recall on
real footage even ducked low. Tools: 521 -> 523.

## v8.3-M: montage mode for non-dialogue footage

Scope per the brief: editing features for footage with no dialogue (travel, sports, pets, raw phone
clips), "top YouTube editor quality" regardless of clip type. Voice/audio-leveling and captions were
out of scope this pass (steered off).

- [x] **Best-moment selection (`score_windows`, `best_segments`).** There was no path for non-dialogue
  footage before this -- `auto_edit` hard-errors without `talking_head_asset_ids`. Scores every window
  on sharpness (`focus_series`), exposure (`frame_quality`), subject framing (`pick_thumbnail`'s own
  scorer), motion (`frozen_frames`), and per-clip shake (`stabilize_clip`'s own read-only correction) --
  every signal reused, nothing re-derived -- with hard per-axis floors on top of the blended score so a
  badly blurred window can't pass on a good blend alone. `best_segments` cuts the result with
  `split_clip`/`delete_clip`. Verified on real b-roll (not just synthetic clips): real jitter reads
  ~25px shake and is dropped (threshold 12px), a static shot reads ~0.01px and is kept.
- [x] **Beat-synced montage cuts with energy-varied shot length (`beat_sync_montage`).** Reuses
  `beat_cut` for the actual cutting; reads `audio_transients`' beat grid, picks a shorter or longer shot
  at each beat from that beat's own strength, ends on a phrase-aligned beat, lays and normalizes the
  music (-14 LUFS default) with a compressor-retry fallback for peaky beds, fades out cleanly. Verified
  on real footage: cuts land within 1 frame of real beats (34/35, median offset 0.3ms), shot length
  visibly tracks a two-tier energy music bed (0.5s loud / 2.0s quiet), final export -14.29 LUFS / -4.05
  dBTP / zero silences.
- [x] **Shot-to-shot colour consistency (`montage_color_match`).** A thin wrapper around the existing
  `match_shot_look`: pick an anchor clip and pull every other listed clip toward its look in one call.
  Verified on real footage: a dark b-roll anchor pulls a warmer desk-shot clip's exposure/color down to
  match.

## v8.2-V: visual finish quality -- push-in, forced punch-in splits, face-aware captions, b-roll whip transitions

Scope per the brief: finished-video LOOK only, no audio/voice work. Every item below wires an
existing, already-tested engine tool into `auto_edit`'s own defaults -- nothing here duplicates
another tool's logic, matching this file's own `auto_edit` design note.

- [x] **Slow Ken Burns push-in on static b-roll (`push_in`, default on).** `add_smart_punch`
  (saliency-driven continuous push-in, already in the registry, never previously called from
  `auto_edit`) now runs on every static b-roll cutaway (no pre-existing scale/x/y animation) at least
  1.2s long, `push_in_zoom` default 1.05 (5%, inside the 3-6% ask), focus following the engine's own
  spectral-residual saliency so the eye stays put while the shot pushes in. Scoped to b-roll only: the
  talking head gets its own cut-zoom treatment (next item), so the two never compete. Verified on the
  real benchmark footage: `real_v41.mp4` vs the prior render at the identical timestamp shows the
  push-in clearly by eye, and audio stayed bit-for-bit unchanged (-13.996 LUFS, 90.0%/91.67% recall).
- [x] **Punch-ins now force a split on a long uncut take (`punch_max_static`, default 4.5s).**
  Natural-pause splitting (`auto_rough_cut`, `tighten_pauses`) already breaks most real takes short
  enough for the existing `add_punch_ins` to alternate wide/tight on (checked: this benchmark's
  longest natural stretch was already 3.2s) -- but a continuous sentence with no breath, or a single
  long uncut take, could leave one giant clip with nothing to alternate against. A new step splits any
  uncovered dialogue stretch longer than `punch_max_static` at the nearest real word-pause to each
  ~4.5s mark (falls back to a plain interval cut when no clean pause is nearby). Verified on the real
  footage with `pacing` disabled (so the usual natural-pause splitting couldn't paper over the gap):
  the one long ~12.5s stretch broke into 4.5/4.4/4.5/0.5s pieces, each getting its own wide/tight zoom,
  confirmed by eye at one of the new cuts. The standard (pacing-on) real-benchmark render is unaffected
  byte-for-byte.
- [x] **Captions default to `auto` placement (`caption_position`, default `auto`), avoiding a low
  face.** `auto_captions` already has a real face-aware `auto` mode (samples the source footage for a
  skin-toned band and moves the caption strip to the top when a face/body reaches into the normal
  bottom band) -- `auto_edit` hardcoded `"bottom"` unconditionally regardless of framing. Switched the
  default and exposed `caption_position` so a caller can still force bottom/middle/top. Proved with a
  synthetic low-face ("selfie") shot: captions correctly jump to the top band by default; an explicit
  override still works. On the real benchmark footage the speaker's face sits clear of the bottom band
  already, so `auto` resolves to `bottom` exactly as before -- audio and visuals unaffected. The
  word-by-word caption pop itself (active word coloured + scaled up to 1.22x, synced to exact word
  timing) already existed in the styles `auto_edit` uses; this was the one real placement gap.
- [x] **A short whip/zoom-blur transition into each b-roll cutaway (`broll_transitions`, default
  on).** `add_transition`'s `zoom_blur`/`whip_left` kinds already existed in the engine and were never
  called from `auto_edit`. Applied only at the b-roll boundary (alternating the two kinds across
  multiple cutaways for variety), never on the talking-head's own jump-cut punch-ins, which stay hard
  cuts on purpose. `broll_transition_duration` default 0.25s. Verified by eye mid-transition on the
  real footage: a clean, tasteful blur-blend, not jarring; audio unaffected.
- Not attempted this pass: any voice/audio work (explicitly steered off -- "Work on more features
  except voice now").

## v8.1-Q: level_dynamics scoped to dialogue, the real cause of the recall "collapse" found and fixed, end-card swell raised to spec

- [x] **`level_dynamics` scoped to the dialogue span, not the whole mix.** New
  `Project.master_speech_level_scope_end` (set by `finish_audio` from its own classified dialogue
  clips' latest end) narrows ffmpeg `speechnorm` to `[0, dialogue_end)` with a 50ms crossfade at the
  boundary, leaving an end card or music-only tail untouched. `0` = unscoped (old behaviour), so any
  caller that doesn't use this stays byte-identical. Disproves v8.0.0's own diagnosis: with speechnorm
  now provably never seeing the end card, an 8dB swell still reproduced the same 56.7% recall
  "collapse" -- the dialogue audio was never damaged by the swell at any size (byte-identical
  dialogue-span transcripts at 3dB and 8dB, proven directly). See the benchmark fix below for the real
  mechanism.
- [x] **`examples/auto_edit_benchmark.py` reports dialogue-span AND full-file whisper recall, gates on
  dialogue-span by default (`--recall-span`).** The old whole-file number was the real source of the
  "collapse": whisper transcribes the whole exported file in one pass with no per-segment reset, so a
  loud segment later in that pass degrades its read of EARLIER dialogue even though that dialogue's own
  audio never changed. Also fixed a second artifact found along the way: a dialogue-span crop with no
  lead-in silence drops whisper's read of the clip's opening sentence (66.7% -> 90.0% recall just from
  padding 0.5s before transcribing).
- [x] **End-card music swell raised 3dB -> 11dB.** With both fixes above in place, 11dB measures clean:
  dialogue-span recall 90.0%, full-file recall 91.7%, end-card LUFS **-18.4** (was -23.4 at 3dB under
  the old unscoped/leaky behaviour, -26.7 with no swell) -- inside the originally-asked -18..-20 range.
- [~] **Voice dynamics (`loudness_range`): swept further, real ceiling found, still short of >=3.**
  `level_strength` 1.0 (speechnorm's floor) reaches 2.0 LU at 90.0% recall, a real gain, and disproves
  v7.9.0's "lower level_strength collapses recall" note (same benchmark crop artifact above, not a real
  regression) -- but 1.0 is a literal no-op for speechnorm and breaks its own regression test for
  footage that genuinely needs leveling, so the default stays 2.5 (1.0 is now a documented, tested,
  explicit override for already-even footage). With every leveling touch off at once, the honest
  ceiling measured was 2.6 LU. Reaching >=3 needs an active dynamics expander, not gentler leveling --
  out of scope this pass.
- [ ] A visible slow push-in (Ken Burns) on static b-roll -- not attempted this pass; time went to the
  two real bugs above instead.

## v8.0-Q: dead air fixed, a real b-roll color match shipped, end-card swell capped by a measured safety limit

- [x] **The 3.0s dead-air tail at the end of every render.** Every `real_*` render back to v15 ended
  on 3.0s of near-silence under the default end screen: the music clip's length only ever targeted the
  talking-head span, not the full export. Fixed by extending the music bed's own length to the full
  planned export (looping a too-short asset, capped at 200 repeats, with loop-seam fades and a true tail
  fade to silence over up to 0.5s). Verified with `audio-proof.mjs`: zero silences anywhere (was 3.0s),
  word recall unchanged at 90.0%.
- [x] **B-roll color/exposure match to the talking head** (new tool `match_shot_look`; `auto_edit` option
  `broll_color_match`, default on whenever there is b-roll). `auto_grade` only pulls a clip toward one
  fixed absolute target and never looks at another shot in the same edit, so a warm interview cut
  against cool or bright-daylight b-roll still clashed after it ran. `match_shot_look` measures a
  reference shot's own look and pulls target clips a `strength` fraction toward it. Before/after frame
  comparison in `v80_broll_compare.png`, looked at by hand -- a clearly visible warm push on the real
  benchmark's cool coffee-shop b-roll.
- [~] **End-card music swell once dialogue ends.** The end card played 12.6 LU quieter than the speech
  body with no voice left to duck for. A ramp-up keyed where the card starts brings the bed up --
  capped at a measured-safe 3dB, not the ~8dB originally targeted: 8dB collapsed whisper recall 90.0% ->
  56.7% on the real benchmark via `finish_audio`'s master-bus speechnorm reacting to the sudden loud
  segment (confirmed with `--no-level-dynamics` recovering to 93.3%); the boundary measured as a cliff
  (3dB safe at 91.7% recall, 5dB back at the 56.7% collapse). End-card LUFS -26.7 -> -23.4, short of the
  -18..-20 targeted -- the honest number, not a faked one.
- [ ] Voice dynamics (`loudness_range`) still at 1.4 vs the >=3 target -- not attempted this pass (see
  v7.9-Q's entry above for what was already tried and ruled out there); disclosed, not claimed.

## v7.9-Q: voice dynamics investigated honestly, whoosh SFX shipped off-by-default, b-roll visual gain not reached

- [~] **Loudness range (LRA): one real, verified gain shipped; the >=3 LU target not reached.** 7.8.0's
  suspected cause (`finish_audio`'s per-clip average-matching pulling every clip to the loudest) turned
  out to have only a small effect on this real footage -- clips from the same speaker were already
  within ~2.3 dB of each other there -- and scaling it down (new `match_strength` option) reproducibly
  cost whisper word recall (88.3% -> 56.7%, a hard threshold effect dropping the opening sentence
  entirely, not a gradual one). `match_strength` defaults back to 1.0 (old behaviour, recall-safe) and
  stays useful for its real case: two different speakers at very different mic levels. The actual
  dominant force was `finish_audio`'s own hardcoded dialogue compressor (ratio 3:1 @ -18dB, no knob at
  all) -- new `compress_strength` option scales it, and `compress_strength:0.0` (off) reproducibly
  measured LRA 1.2 -> 1.4 LU AND word recall 88.3% -> 90.0% (improved). The absolute ceiling (everything
  off) only reaches 2.3 LU and is not recall-safe. `compress_strength` defaults to 0.0: real, measured,
  recall-positive. Never faked a number to call this done -- see CHANGELOG.md for the full sweep.
- [~] **Whoosh SFX on b-roll cut-ins and the hook title-in** (new, off by default): a short procedural
  rising-pitch tone (ffmpeg-synthesized at call time, same pattern as the existing keyword-pop SFX, no
  bundled sample), on its own ducked SFX track. Built and TDD'd (real placement, real audibility, a
  caller can turn it on with `whoosh_sfx: true` on `place_broll` or `auto_edit`), but on the real
  benchmark it cost whisper word recall even at a low duck level -- 0.3 volume: 90.0% -> 86.7%; 0.15
  volume (tried as a fix): 90.0% -> 80.0%, WORSE, not better, so this is not a simple louder-is-worse
  curve and a third blind guess was not going to be a verified fix. Left at an audible 0.3 and defaulted
  to FALSE rather than ship a recall regression as the default behaviour -- same standard as
  `match_strength` above. A caller who wants the sound gets the real effect, with the cost disclosed in
  the tool's own description.
- [ ] **A visible, frame-verified b-roll gain** (colour/exposure match between the talking head and
  b-roll, or a slow push on static b-roll) did not happen this pass -- the two audio investigations
  above used the available time. Not claimed, not faked.

New in v7.8.0: `tighten_pauses` (new tool, 521) shrinks the dead air a rough cut leaves INSIDE its own kept
clips -- an interior breath between two phrases, or the lead-in pause before the first word -- down to a
natural pad, auto-calibrating what counts as silence from the real recording's own noise floor rather than a
fixed dB number (real room tone runs nowhere near a studio's); `auto_edit` runs it by default right after the
rough cut. Far more consequential: found and fixed a serious pre-existing bug in `finish_audio`'s own
dialogue-vs-ambience classifier -- real human speech was being misread as "ambience" and muted outright on
real interview footage, present since finish_audio was first written (294 commits back), affecting every real
render in this project's history, and invisible to every loudness/silence check because the mix's own gain
trim just cranks up whatever survived the mute to hit the LUFS target. Caught by transcribing a shipped render
and finding most of the real sentences missing; fixed, TDD'd against a real-footage excerpt, and given a
permanent regression gate (`examples/auto_edit_benchmark.py --whisper-recall-source`, word recall vs the
kept-span ceiling, default floor 90%). Also fixed a smaller real bug the same investigation turned up --
`auto_rough_cut`'s own speech detector was losing the first ~0.5s of real words on a cold-open recording.
Honest miss, disclosed not hidden: loudness range still reads ~1.2-1.4 LU against a 6-8 target on this one
real clip, against a ~2.4-3.6 LU natural ceiling in the raw source itself. See CHANGELOG.md for numbers.


## v7.8-Q: real-footage pacing + a serious pre-existing audio bug found and fixed

- [x] `tighten_pauses` (new tool): shrinks interior dead air a rough cut leaves INSIDE its own kept clips
  (a breath between two phrases, or the lead-in pause before the first word) to a natural pad, without
  ever cutting mid-word. Auto-calibrates its own silence threshold from the real recording's noise floor
  and quietest speech level (a fixed dB number missed real room tone entirely on the real benchmark).
  Never leaves a surviving piece shorter than `min_piece_s` (default 1.0s): a short clip is read less
  reliably by other per-clip tools downstream, and fragmenting too far raised the odds of exactly the bug
  below. `auto_edit` runs it by default (new `pacing` option) right after the rough cut.
- [x] **Found and fixed: `finish_audio` was silently muting most real dialogue as "ambience"** -- its own
  per-clip speech_ratio classifier (aggressiveness 2) misread real human speech often enough that, on the
  real benchmark interview, most of the spoken content came back muted while every loudness and silence
  check still passed (the final gain-trim loop only ever levels whatever survived the mute, so it always
  measures "on target"). Present since `finish_audio` was first written, confirmed by whisper transcripts
  of every real render in this project's history reading back only a fraction of the source sentences.
  Fixed by trying a less strict aggressiveness first (real speech reads confidently there) and rescuing
  anything still missed at an even less strict level, gated at a higher bar so it doesn't just relabel
  real background tone as dialogue. TDD against a real-footage excerpt sliced at test time (not committed
  media). New permanent regression gate: `examples/auto_edit_benchmark.py --whisper-recall-source` fails
  the run under 90% word recall (whisper) against the source dialogue.
- [x] **Found and fixed: `auto_rough_cut` lost real opening words on a cold-open recording** -- its speech
  detector (VAD) needs several consecutive confirming frames before declaring speech has started, costing
  the first ~0.5s of real words on a recording that begins talking immediately with no lead-in silence
  (confirmed: whisper hears real speech at the source's literal 0.0s). `recover_missed_onset` re-checks
  only the very first kept segment of the whole output with a plain RMS silence probe (no VAD confirmation
  delay) and recovers it when that window is not actually silence.
- [x] **Made the recall gate honest**: `--whisper-recall-source` now gates word recall against the kept-
  span CEILING (what `auto_rough_cut` itself decided to keep, re-derived and transcribed fresh), not the
  full untrimmed source -- dead-air/filler trimming removes real words on purpose, so gating against the
  untrimmed source punished correct editing alongside a real bug. Reports both numbers.
- [~] **Loudness range (LRA) still short of the 6-8 LU brief on this one real clip**: measured honestly at
  ~1.2-1.4 LU after this quarter's fixes, against a ~2.4-3.6 LU natural ceiling in the RAW unprocessed
  source itself (checked directly, before any finish_audio processing) -- this specific calm interview
  voice does not naturally carry 6-8 LU of dynamic range, and `finish_audio`'s per-clip gain-matching step
  (every dialogue clip leveled to the loudest clip's average, a deliberate consistency feature, not
  `level_dynamics`/speechnorm) is the dominant narrowing force, confirmed by testing with speech leveling
  fully disabled (LRA barely moves, 1.4 vs 1.2). The prior real_v14 render's 7.1 LU reading almost
  certainly reflects the finish_audio muting bug above (big silent-vs-loud swings from mostly-missing
  dialogue), not healthy vocal dynamics -- not re-chased further this quarter; disclosed, not hidden.
New in v7.7.0: another real-footage quality pass on `auto_edit`, not new tools -- `place_broll` now matches each b-roll cutaway to the words actually being said (a curated keyword table + filename fallback, falls back to given order when nothing matches, not a classifier) instead of just given order; `lint_edit`'s caption-over-face fix now nudges each page of a multi-page caption on its own sampled saliency peak instead of moving the whole group by one page's offset [~: the saliency check is a spectral-residual heuristic, not real face detection, so a busy background can still out-score a face and leave a partial overlap]; `auto_edit` gained a subscribe-style end screen (a pill badge that actually sizes to its own text, over a blurred freeze of the video's last frame, not flat black), on by default for a landscape edit and off for a vertical short. See CHANGELOG.md for the measured before/after on real footage.

New in v7.6.0: a quality pass on `auto_edit`'s real-footage output, not new tools -- a boxed caption's background pill now hugs the actual text width instead of the wrap width; `finish_audio`'s loudness range narrows for real now (a loudness-neutral master-bus speech leveler, opt-out via `level_dynamics`); `add_punch_ins` gained opt-in `variety` so consecutive punch-in zooms don't all land at the exact same amount; `add_keyword_pops`'s `sfx:true` plays a real synthesized pop instead of its old no-op. See CHANGELOG.md for the measured before/after on real footage.

New in v7.5.0: `close_gaps` (general ripple-close for any/all gaps in one call), `duck_timeline` (ducks every music track under every dialogue track's speech in one call), `import_clips` (copy specific clips, with their own effects/keyframes/grade, from another project file into the live one), `style_captions_batch` (one call restyles every caption group, or a named list), plus `kimo gui` copy/paste + ripple-delete, a marker/chapter panel, a render queue panel, and a keyframe lane. See the "v7.5 additions" section below and CHANGELOG.md for the full list.

New in v7.1.0: `kimo gui` is now an actually-editable local editor (clip/effect-params panel, drag-to-move/trim, scrub+play, undo/redo), plus 3 new tools (`project_merge`, `mark_scene_cuts`, `export_variants`) and a `has_effect` filter on `find`. See the "v7 additions" section below and CHANGELOG.md for the full list.

New in v7.0.0: a local [`kimo gui`](crates/kimo-cli/src/gui.rs) web UI (timeline view, real engine preview, a tool-invocation panel over the same registry the CLI and MCP server use), a MOGRT-style `export_template_package`/`import_template_package` bundle, an optional `gpu2d` wgpu compute path (blur, colour matrix), and new particle presets (bubbles, foam, card_dance). See CHANGELOG.md for the full list, honest gaps included.

Earlier: [transcript phrase search and revision-checked ripple deletion](docs/TRANSCRIPT_EDITING.md), including dry runs, caption/marker updates and one-step undo. Normal-speed, explicitly bound media only; see the support limits before applying.

MCP-first, AI-first video editor in Rust. Every operation (cut, transform, keyframe, effect, transition, grade, track, mix, export) is a registered command. The MCP server's tool list is generated from that registry, so a feature cannot exist without being AI-callable. The AI can also render a frame to PNG and read video scopes, so it can look at its own work and iterate.

Legend: [x] shipped and tested, [~] partial / approximate, [ ] pending. Catalog counts below are reconciled to engine checkpoint `dd9a517` on 2026-10-11; use the live server (`tools/list`, `list_effects`) for later counts. Timings and fixture measurements are historical checkpoint evidence, not current benchmarks. Mixed checklist rows use [~] when only a subset is shipped; later sections state the limits.

## Quickstart
```
cargo build --release
python3 examples/demo_grade.py        # graded 2-clip sequence with transition, built only via MCP calls
python3 examples/effects_gallery.py   # renders every effect
```
Needs `ffmpeg` and `ffprobe` on PATH (decode/encode).

## Use from an MCP client
```json
{ "mcpServers": { "kimo": { "command": "/path/to/kimo/target/release/kimo-mcp" } } }
```
Call `tools/list` to see everything. Start with `project_set`, `import_media`, `add_track`, `add_clip`, `add_effect`, `render_frame`, `export`.


## 10 starter tools

A quick working set from README, not a measured usage ranking. Use `tools/list` for the live parameter schemas.

- `project_set`
- `import_media`
- `add_track`
- `add_clip`
- `add_effect`
- `render_frame`
- `export`
- `set_property`
- `set_keyframe`
- `list_effects`

## Module guide

Each module separates the capability checklist, parameter/control notes, limits, and proof records. Mixed source records keep their parameters, measurements and caveats together. Older checklists and later corrections are retained as written; the dated checkpoint is not a live capability guarantee.

- [Timeline and editing](#timeline-and-editing)
- [Transitions](#transitions)
- [Color and grading](#color-and-grading)
- [Audio](#audio)
- [Text and titles](#text-and-titles)
- [Compositing and motion](#compositing-and-motion)
- [Tracking and rotoscope](#tracking-and-rotoscope)
- [Particles, shards and glass](#particles-shards-and-glass)
- [3D and physics](#3d-and-physics)
- [Effects catalog](#effects-catalog)
- [Export and delivery](#export-and-delivery)
- [Performance](#performance)
- [Sandbox](#sandbox)
- [BETA and honest limits](#beta-and-honest-limits)

## Timeline and editing

### What it does

#### Timeline and trimming
- [x] multi-track timeline, add/move/trim/split/delete/duplicate clip
- [x] ripple delete, ripple trim (head/tail), roll edit, slip, slide
- [x] razor all (split_all_at), insert gap, lift / extract range, remove silence ranges
- [x] rate stretch, speed, speed ramp (time remap curve), reverse, freeze frame
- [x] markers (point/range, color, name), snap_time, list_edit_points
- [x] track lock, rename, reorder, delete
- [x] undo/redo, save/load project
- [~] group clips via precomps; pending: track targeting, sync lock, linked audio+video clips, link/unlink
- [~] nested compositions via group_clips / ungroup_clip; no editing grouped clips in place or group time remap
- [~] adjustment layers [x], track mattes [x], sequence settings per sequence, multiple sequences
- [~] designed title/lower-third presets via add_title. MOGRT substitute: a real parametric template system already existed pre-v7 (template_define/template_apply/template_list/template_describe/template_delete -- named, parameterised op-lists, one atomic undo step, exportable/importable as a JSON library) plus set_essential_properties/list_essential_properties/clear_essential_properties (marking which params a MOGRT-style consumer should expose, FEATURES.md L239). NEW this pass: export_template_package/import_template_package (ext/templates.rs) bundle a named template with its clips' essential-property marks into one JSON file -- Kimo's own open one-file substitute for Adobe's closed .mogrt container, round-trippable within or across projects (import matches essential marks back onto clips by role first, falling back to the original clip_id; unmatched entries are reported, never silently dropped). 5 TDD tests. Honest gap: still no Adobe .mogrt IMPORT (that format is closed/proprietary; not attempted, as instructed) and no thumbnail/preview asset in the package, unlike a real MOGRT file.

### Parameters and controls

Use `tools/list` / `list_effects` for live schemas. Read the source parameter names, ranges and defaults alongside their evidence and limits in this module.

### Limits

- [x] insert vs overwrite with auto-ripple on add_clip, three/four-point edit, source in/out marks [new in v7]: add_clip gained an optional `mode` (insert: ripples track_id and any ripple_tracks forward by `duration` via the existing insert_gap machinery, for multi-track sync; overwrite: clears [start,start+duration) first via the existing lift_range machinery, nothing ripples) -- built on insert_gap/lift_range's own already-tested logic (factored into do_insert_gap/do_lift_range, reused directly, not re-dispatched, so there's one undo entry and one atomic all-or-nothing mutation even when a later validation fails); mode omitted behaves exactly as before (raw append, byte-identical). edit_points does three-point (any 3 of source_in/source_out/timeline_in/timeline_out, 4th derived) and four-point (all 4; speed is adjusted to fit when the spans differ and fit:true is passed, else a clear error) edits, delegating to add_clip for placement. mark_source/clear_source_marks/get_source_marks hold ephemeral per-asset in/out marks (Session-only, never saved in the project, like a real source viewer) that edit_points falls back to when source_in/source_out are omitted.
- [x] multicam: sync by audio/timecode, angle switching, multicam to flat: sync_clips (audio correlation or explicit SMPTE timecodes), add_multicam/switch_angle/list_multicam (persistent group, continuous fixed audio angle, generates normal timeline clips) all already shipped pre-v7. [new in v7] flatten_multicam: bakes the group's current cut into plain clips and discards the group's own metadata, so switch_angle can no longer silently rebuild/replace a track someone has since hand-edited -- the missing "to flat" piece.
- [x] subclips, clip labels/colors, bins and metadata, search [new in v7]: subclips are named saved in/out bookmarks on an asset (`create_subclip`/`list_subclips`/`delete_subclip`/`add_clip_from_subclip`, a thin wrapper over `add_clip` so placement is byte-identical to a hand-placed clip with the same numbers); clip colour labels are a reserved `color:<name>` tag on `set_meta` (red/orange/yellow/green/cyan/blue/purple/pink/gray), surfaced and filterable on `find`; bins organise media-pool assets into named folders independent of timeline tracks (`create_bin`/`list_bins`/`rename_bin`/`delete_bin`/`move_asset_to_bin`/`find_assets`); clip names/roles/tags and `find`'s other filters were already shipped (names.rs)
- [x] replace clip (keep effects): `replace_clip_media`; paste attributes: `paste_attributes` (already shipped, verified); effect presets (save/apply): the `template_define`/`template_apply`/`template_list`/`template_describe` system (already shipped, general-purpose -- covers a saved, reusable effect preset); copy/paste keyframes: `copy_keyframes` (property or effect-param keys, optional time offset, replace or merge)
- [x] auto reframe (cover-fit decode), scene edit detection, auto-cut to beats / to speech: auto_reframe (subject-tracking crop to the canvas aspect, decoded sharp at full resolution so the cover-fit crop is never soft), detect_cuts/scene_split (Premiere-style scene edit detection, advisory detect + an applying split) and cut_to_beat ("to beats") all already shipped pre-v7. [new in v7] cut_to_speech: the "to speech" sibling of cut_to_beat -- lays a narration/voiceover asset, finds its speech intervals with the existing audio_speech_intervals VAD, and cuts shots exactly where each new speech interval (or every Nth, via phrases_per_shot) starts, same guarantees as cut_to_beat (one undo step, clear error when there isn't enough speech to fill the requested shots).
- [x] proxy workflow (generate proxy, toggle), offline media relink: already fully shipped pre-v7, confirmed and verified during the v7 pass, no new code needed. proxy_generate/proxy_list/proxy_set_enabled/proxy_remove/proxy_validate/proxy_relink (proxy_tools.rs, 19 tests in tests/proxy_workflow.rs: generation is undoable and never touches originals, toggle falls back cleanly when a proxy is missing/stale/corrupt, managed-color projects bypass proxies, exports/audio/analysis always use the original) and relink_media (delivery.rs: searches search_dirs for moved/renamed files by sha256 > name+size > name, reports ambiguity rather than guessing, apply:true rewrites references as one undoable step; covered in tests/delivery.rs, tool_contracts.rs, replace_clip_media.rs, error_table.rs).
- [x] rolling/regular edit by timecode text ("+10", "-5 frames") helpers: `ripple_trim`, `roll_edit`, `slip_clip` and `slide_clip`'s `delta` takes a number (unchanged) or a string like "+10", "-5 frames", "3f" (unit defaults to seconds, converted via project fps for frames)
- [x] captions: SRT import [x], speech-to-text [x], caption styling presets [x], burn-in [x via text] -- all already fully shipped pre-v7, confirmed and verified during the v7 pass, no new code needed. speech-to-text IS auto_captions (auto_captions.rs): whisper.cpp's whisper-cli runs locally as a sidecar with a ggml model (MIT weights, fetched by tools/whisper/fetch.sh), word times refined against the audio, Hinglish-aware (Roman-script Hindi + English), a real named fallback when no recognizer is installed, 13 tests in tests/auto_captions.rs against a stubbed whisper binary plus 4 more in tests/caption_confidence.rs for caption_from_words' cross-check. Caption styling presets ARE auto_captions' `style`: pop/karaoke/clean/boxed/impact, each a full named look (font, weight, size, colour, stroke, shadow, case, background, active/keyword colours, reveal/pop/karaoke/page-pop animation, lines-per-page).

### Proof level

Use the quality-tier audit and test notes in the BETA appendix; checklist marks are not a professional-tool parity claim.

## Transitions

### What it does

#### Transitions (31 registered kinds)
- [x] crossfade, wipes x4, slides x4, zoom, spin, flash, iris, blinds, checker, diagonal
- [x] dip black/white/color, whip left/right, light leak, film burn, glitch, zoom blur, clock, split h/v, noise dissolve, shatter / shatter_cells
- [~] 10 transition easings; no centered/start/end alignment control

### Parameters and controls

Use `tools/list` / `list_effects` for live schemas. Read the source parameter names, ranges and defaults alongside their evidence and limits in this module.

### Limits

- [~] outgoing-clip push/slide-out, luma wipe from custom matte, page peel, morph cut-like, ripple dissolve, gradient wipe, barn doors [x 4 of 7 already shipped pre-v7, uncredited: push (pushwipe.rs -- outgoing slides out one edge while incoming slides in the opposite, true geometric motion not a wipe), gradient_wipe (pushwipe.rs -- linear/radial/angular ramp OR the luma of another clip kept hidden from the composite, exactly "luma wipe from custom matte"), morph_cut (transitions.rs/render.rs -- real dense optical flow between the outgoing clip's last frame and the incoming clip, Premiere's actual Morph Cut, not an approximation). New this pass (transitions.rs): ripple_dissolve (concentric sine-perturbed radial reveal expanding from centre) and barn_doors (the outgoing picture's two halves physically slide apart like double doors -- a geometric slide via a raw-pixel compose closure, same family as push, not a reveal mask, verified by checking the open gap widens monotonically with k). Honest [~]: page_peel is a simplified 2D corner peel (diagonal reveal with a curl-shadow band and a highlight running just behind the lifting edge) -- not a true 3D page-curl geometry, which this pass does not attempt; it reads as a page lifting, not merely a plain diagonal wipe, but it is a stand-in, stated as such. 4 new TDD tests (new_transitions_v7.rs). golden_registry.txt hand-updated for add_transition's grown kind enum.]

### Proof level

Use the quality-tier audit and test notes in the BETA appendix; checklist marks are not a professional-tool parity claim.

## Color and grading

### What it does

#### Color (Lumetri-style)
- [x] exposure, contrast, saturation, temperature, tint, color wheels (lift/gamma/gain), curves (luma S-curve), HSL secondary, LUT (.cube), teal/orange, vignette (shaped: centre, roundness, squareness, size, aspect, feather, edge colour; centre can follow a track_point clip or a tracked mask), color isolate, levels, brightness/contrast, channel mixer, gradient map, colorize, shadow/highlight, threshold
- [x] scopes (waveform/vectorscope/histogram data via get_scopes)
- [~] free-position master/RGB curves, hue curves, qualifier with per-effect masks and display-sRGB/D65 Lab shot match; pending: mask tracking binding, semantic match and automatic creative tone selection
- [~] opt-in scene-linear wide-gamut pipeline, explicit gamut/log transforms, supported pointwise grade LUT export, approximate film looks, optional pinned OCIO/ACES2 reference; no automatic camera IDT, measured film-stock calibration, production OCIO config import or certification

### Parameters and controls

Use `tools/list` / `list_effects` for live schemas. Read the source parameter names, ranges and defaults alongside their evidence and limits in this module.

### Limits

- [~] denoise (temporal/spatial, bilateral in its spatial pass) [x], sharpen [x] unsharp, dehaze [x] (verified, dehaze.rs tests), clarity [x] (= `midtone_detail`, Resolve-style local contrast in midtones, verified), bilateral_blur [x new in v7 -- its own registered effect kind, Resolve Surface Blur / Premiere Bilateral Blur style, radius capped 0..10px for CPU cost; distinct from denoise's internal noise-tuned bilateral pass]; pending: lens profile correction (no lens/camera database), and compound/smart/channel blur as their own standalone effect kinds

### Proof level

#### LUT export / color bake (.cube 17/33/65) - BETA
Explicit ordered-steps .cube export, with native OCIO space/view steps and managed/declared transforms. Spatial, masked and implicit effects fail closed (not baked). Every export reports max/p99/RMS error against the source chain. Verified against a real OCIO 2.5.1: affine roundtrip max 1.19e-7. Nonlinear bakes carry a tetrahedral approximation error: ACEScct max 0.0137 at 65, ACES2 view max 0.0364 at 65 (RMS 0.00074). A baked LUT is an approximation, not the live chain. Source: COLOR workstream; reviewed here (build, 3 focused tests with the default build, fuzz 167 tools / 6449 calls / 0 failures).

- HDR validation (color/hdr_validation.rs + tools/hdr_validate.py, COLOR; diagnostics only, no transfer edits): independent BT.2100 PQ/HLG reference math validator, HLG OOTF evaluator (opt-in), PQ excursion/metadata diagnostics, whole-clip MaxCLL/MaxFALL audit. 7 integration + module tests. Measured limits (from COLOR's report, docs/HDR_VALIDATION.md): FFV1 RGB16 error sits at the 16-bit quantization floor (0.046 nit); HEVC 10-bit CRF 0 is NOT lossless-equivalent (p95 144 nits, max 331 nits on the fixture). Not independently re-run by MOTION beyond the test suite.

- Native OCIO (opt-in cargo feature `native-ocio`; config parse, list spaces/views, domain-sandwich effects, inspect_ocio_config + transform_ocio_samples tools; BETA, COLOR): the native CPU path was built and run by COLOR on Linux x86_64 against OCIO 2.5.1 (suite 293/293; official built-in ACES cg config loaded through the C++ adapter; real 1280x720 ACEScg to ACEScct transform with alpha exact; Python same-library parity max RGB delta 0; ACEScct published values matched to 2e-6). MOTION re-ran only the default build (config-parse/registry tests, fuzz 162 tools / 6242 calls / 0 failures); no OCIO library on this box. NOT claimed: ACES display-view rendering API, full ACES suite, GPU, hardware display, other platforms, production throughput, external LUT files, renderer-wide domain propagation (docs/OCIO_NATIVE.md).

- OCIO display/view transform (`render_ocio_display_view`, `transform_ocio_view_samples`; BETA, COLOR): forward OCIO DisplayViewTransform with looks enabled, self-contained ext module, no render/project/output changes. COLOR's evidence: completed native full suite 308/308 pass against OCIO 2.5.1 with the official ACES config set (an earlier 309 figure was withdrawn), 3.69M RGBA float samples through the full native view match the Python same-library view processor with max delta 0, one real rendered frame visually inspected. MOTION re-ran: default build + 1 integration test + fuzz 165 tools / 6356 calls / 0 failures; native path not re-run here. Limits: self-contained configs only (external LUTs rejected), CPU only, preview PNG only with an explicit sRGB_SDR declaration, managed AP1/D60 required, no HDR preview/encoding proof, no automatic output changes.

- Display scopes (`render_display_scope` tool: waveform, RGB parade, vectorscope, histogram PNG; Rec709/2020, IRE + nits graticules, PQ/HLG aware; BETA, COLOR): 2 integration tests pass here plus COLOR's reported 271/271 suite and exact known-value proofs (Rec709 red vector, PQ 100/1000/10000 nit). Limits: IRE is full-range RGB, not legal YUV; HLG nits are assumption-based; PNGs about 1 MB. docs/DISPLAY_SCOPES.md.

- Subject-aware shot match (color/subject_match.rs, COLOR; BETA, library only): matte-driven per-region Lab statistics matching with safety bounds, 16 tests, one real-photo proof (RMSE 0.0585 to 0.0193). Registered as a module only: NOT exposed as a tool or effect yet, no video/temporal validation, no automatic subject detection.

- `preview_subject_match` (level 1, preview only, no project mutation; BETA, COLOR): matte-guided per-region matching between two identity-geometry clips at explicit times, returns before/after preview + diagnostics + plan; absent regions disabled, overlapping weights rejected, fit frozen at call time. 4 tests pass here, fuzz 166 tools / 6419 calls / 0 failures. COLOR's real-pair proof on film10 frames: weighted log-luma error 1.331 to 1.180 (a modest gain, one pair). Level 2 (persistent subject_match effect in the render path) is NOT built and needs its own review and render-lane GO.

- `camera_idt` effect + chart-fit IDT profiles (color/idt.rs, COLOR; BETA): gamma + ridge 3x3 matrix fitted from chart patches, save/load kimo-idt-v1, identity defaults, alpha untouched. 9 tests; fit error 1e-6 on SYNTHETIC 24-patch data only: no chart-image detection, no white adaptation, no real camera validated, catalog-only (no MCP profile tool). Hint-driven encoding picker selects only on explicit hint plus pixel sanity.

#### add_acescct_primaries (ACEScct primaries) - BETA
Native AP1 to ACEScct to AP1 sandwich with per-channel primaries and an inverse, neutral identity, bound gamut 3. Explicit documented formula, NOT Resolve LGG parity. Conflict guard runs before the OCIO domain wrapper dispatch (v2: no double-domain interpretation) with an atomic rejection test. COLOR's proof: known math and inverse, real OCIO reference within 2e-6, determinism, neutral bit-exact incl. signed zero and HDR alpha. Reviewed here: build, cct_grade test, fuzz 169 tools / 6555 calls / 0 failures.

- ASC CDL files (BETA, COLOR): strict .cc/.cdl import (roxmltree, DTD/entities rejected), canonical semantic export, native OCIO CDLTransform with explicit ASC clamped style, inspect/apply/export tools, optional ACEScct sandwich with bound guard. Verified by focused tests (cdl_xml) and fuzz (172 tools, 0 failures); not yet checked against third-party grading-app CDL files.

## Audio

### What it does

#### Audio
- [x] per-clip volume + keyframes, fades, mute, pitch, high/low pass, compressor, echo, denoise flags, reverse, beat detection, export_audio
- [x new in v7.6] `finish_audio`'s dialogue mix gets a gentle master-bus speech leveler (`level_dynamics`, default on): per-clip average-matching alone left a real speaker's own within-clip loudness swing untouched (measured 11.1 LU loudness range on a real interview edit, against an EBU target band of 6-8); one loudness-neutral `speechnorm` pass over the whole mix narrows it (measured 6.8 LU on the same footage) without fighting `finish_audio`'s own convergence loop or moving the mix's overall level

### Parameters and controls

Use `tools/list` / `list_effects` for live schemas. Read the source parameter names, ranges and defaults alongside their evidence and limits in this module.

### Limits

- [~] essential-sound style: dialogue loudness normalize (`normalize_loudness`) [x], auto-duck (`auto_duck`) [x], EQ bands/de-esser/reverb (`add_audio_effect`) [x], noise_gate and limiter (`add_audio_effect` kind=noise_gate/limiter, ffmpeg agate/alimiter) [x], stereo pan is now a keyframeable Prop like volume (`set_audio_pan` sets the static base, `set_audio_pan_key` keyframes it; old projects' bare-number pan still load, and it still serializes as a bare number until actually keyframed, so existing project JSON is untouched) [x new in v7]; pending: de-reverb, surround mix, audio transitions (constant-power/exponential crossfades on audio-only edges)
- [~] voiceover record, audio sync by waveform, loudness meter (LUFS) readout [x audio sync by waveform -- already shipped pre-v7 as sync_clips's by:"audio" mode (bounded RMS correlation, 5ms resolution, rejects silence/ambiguous repeats), credited here for the first time; x loudness meter (LUFS) readout -- already shipped pre-v7 as measure_loudness: EBU R128 integrated LUFS, momentary/short-term curves (EBU Tech 3341), loudness range (EBU Tech 3342), 4x-sinc true peak, clipping detection -- a genuinely comprehensive meter, far beyond a bare readout, credited here for the first time.] Honest gap, not attempted: voiceover record -- this is a headless server-side render engine with no microphone/audio-input device in its environment; a literal "press record, speak into the mic" feature has no hardware here to implement against. The adjacent real workflow (record elsewhere, bring the file in) is already fully supported: import_media/import_samples take any recorded audio file straight onto the timeline.

### Proof level

Use the quality-tier audit and test notes in the BETA appendix; checklist marks are not a professional-tool parity claim.

## Text and titles

### What it does

#### Titles / text
- [x] text clips with typewriter, font size/color, keyframed transforms, masks
- [~] char/word/line animators (position, scale, rotation, opacity, blur, tracking), path text, title presets, stroke/shadow/background styles and font-weight control; pending: per-character color, unrestricted variable-font support, per-span rich text and 3D extrusion

### Parameters and controls

Use `tools/list` / `list_effects` for live schemas. Read the source parameter names, ranges and defaults alongside their evidence and limits in this module.

### Limits

Read the gaps, pending items and scope qualifications in this module's records, together with the BETA appendix.

### Proof level

- Text on path + randomized selector (V1): set_text_path (points or circle/ellipse/figure8/wave/arc/spiral, per-glyph tangent rotation, animated start progress, flip) and add_text_animator random=true seed=N (AE Randomize Order). Motion-blur sampling includes the path travel. Gaps: no per-character colour animator (text is one coverage mask, one colour), no "based on" modes (words/lines are available via unit), path margin/perpendicular offset not exposed.

- Text fixes (from external review): add_prompt_box text now sits inside its plate (measured: text left edge 273 px = plate left 244 + padding; was 174, outside the plate). add_text gains `x` and `anchor` (center|left|right) so x refers to the block center, left or right edge (measured left edge 145 px vs target 140, glyph side bearing). New `text::measure`. `font` accepts a family name or a file path everywhere (it already did; descriptions now say so). Not yet done: non-snake-case warnings, gentler speed-line/detection-box defaults.

- Review cleanups: text.rs non-snake-case warnings removed (pure rename, prompt box output unchanged); add_speed_lines default ray count 70 to 48 (gentler; pass count to restore). Detection-box default softening not done.

## Compositing and motion

### What it does

#### B. After Effects: compositing and motion graphics core
- [x] layers: video, image, text, shape (rect, ellipse, polygon, line), adjustment layer, solid-like via shapes
- [x] transform keyframes: position, scale, rotation, opacity with ease (linear, in, out, in_out, hold)
- [x] blend modes (set_blend), masks (animated, feather), corner pin, track point, stabilize
- [~] perspective 3D layers/project camera and Scene3D geometry with lights, depth of field and parenting/nulls; these are not unrestricted AE layer/lighting features
- [~] bezier timing handles, spatial paths/auto-orient, fixed expression-op stacks (wiggle/loop/sin/inertia/clamp/round), AND a free-form expression interpreter (ext/expr_interp_v7.rs, set_expression_script/clear_expression_script/list_expression_scripts): arithmetic/comparisons/ternary/&&||!, sin/cos/tan/abs/sqrt/floor/ceil/round/min/max/pow/mod/clamp/linear(), time/value/index/PI/E variables, valueAtTime(t) and link("clip:prop",t) (pickwhip-style cross-clip/effect-param linkage, live like link_property); still no expression-control objects (Slider/Checkbox/Angle/Color/Point Control effects) and index is scoped to a clip's own track (no single stacked-layer order exists in this model)
- [~] precomps on a local clock; pending: group time remap, collapse transformations, continuous rasterization
- [~] rendered-layer alpha/luma/inverted track mattes; pending: general set-matte effect and stencil/silhouette blend modes
- [~] beta grid mesh pin/tracking for smooth deformation; no puppet rig, general bezier warp/liquify or content-aware object removal
- [~] transform shutter blur, echo/frame blend, speed/time-stretch and posterize time; no flow interpolation/retime or per-effect blur
- [~] planar homography tracking/corner pin; pending: 3D camera solve and face tracking

#### Motion program (queued, requested: all motion features of Premiere, After Effects, Resolve Studio, plus kimo-exclusive)
Status honest: nothing below is done unless marked. Existing: motion blur (shutter-angle, per layer), point/ring tracker, stabilizer, speed ramp, particles, physics, Scene3D.
- [~] 1. Track mattes (alpha/luma/inverted) shipped; collapse transforms pending. DONE V1: precomps (group_clips / ungroup_clip: nested tracks on a local clock, group transform/effects/mask/opacity/keyframes apply to the whole group; gaps: no editing of grouped clips in place, no time remap of the group, no continuous rasterization, nested audio not mixed)
- [~] 2. Expressions V1 DONE: op stack per property/effect param (wiggle w/ octaves+seed, loop_in/out cycle|pingpong|offset|continue, sin, inertia, posterize_time, clamp, round) via set_expression; cubic-bezier key handles via set_keyframe_bezier (overshoot ok). Expressions V2 DONE: free-form interpreter (set_expression_script) with linkage (link("clip:prop")), valueAtTime and index -- see FEATURES.md L210 for the full writeup and honest gaps (no expression-control objects, no per-axis 2D wiggle correlation, no handle UI/separate in/out influence). Spatial paths with auto-orient shipped; text animators still need polish
- [~] 3. Echo V1 DONE (set_echo: composite/add/screen/average frame-blend ghosts, +/- time; set_posterize_time on whole clip). Gaps: no optical-flow interpolation/retime (forward source blur shipped separately), no time displacement, no rolling-shutter repair. Original: Echo, posterize time, frame blending, optical-flow interpolation and slow-mo, optical-flow motion blur for footage, time displacement, rolling-shutter repair
- [~] 5. Planar/homography tracker and limited occlusion reacquisition shipped; pending: mask tracking, 3D camera solve, face tracking, independently measured warp stabilizer parity, object removal
- [~] 6. Beta mesh pin/tracking shipped; pending: ML roto/segmentation, puppet rig, liquify, smart reframe, speed warp, optical-flow retime

- [x] impact landing for detection boxes (impact:true), add_box_expand (constant-speed expansion + 2-frame flash), add_camera_shake / impact_shake (damped spring, measured from SCUMBLE), add_hud_frame, dynamic text tokens {tc} {bar} {t} {pct}. V1: measured from one reference, not generalized beyond it; see examples/scumble_*.py for known diffs.

### Parameters and controls

- add_speed_lines (manga radial rays, flicker/spin/bg). Gaps: flat-colored rays only, no per-ray gradient or noise streak variant, CPU O(pixels x rays) (~0.1 s/frame at 70 rays).
- add_camera_shake now uses a measured per-frame kick table ([-19,-10],[-11,-6],[-4,-2] px @720p, discrete frames, same as the SCUMBLE reference; pass profile for others, profile='spring' for the old damped spring). Verified frame by frame with tools/bracket_track.py + examples/shake_align.py (kick frames and values identical to the reference at 4.12/4.60/5.08). Gaps: per-hit amplitude differences in the ref (6.04 hit is ~1.3x) and later hits 6.5-7.5 not yet verified because the HUD bracket is covered there.

- [x] 2b. Spatial motion paths (set_motion_path / remove_motion_path): bezier or through-points (auto-smooth Catmull-Rom) or presets (circle, ellipse, figure8, wave, arc, spiral), constant-speed arc-length progress via keyframable property `path` (ease, bezier handles, expressions work), auto-orient with offset and corner smoothing, correct under motion blur and echo. Gaps: no per-point speed handles (use eased `path` keys), no 3D paths, no path editing UI.

### Limits

- [x] shape layer operators: fill, stroke, gradient fill, trim paths, repeater, merge/offset paths, zig zag, pucker/bloat, wiggle paths, round corners, twist; bezier paths -- fill, stroke, trim/repeater/merge/offset paths, gradient stroke and bezier paths (add_path_shape, SVG path syntax or points, every modifier) were already fully shipped pre-v7, as were zig_zag/pucker_bloat/wiggle/twist/round_corners (add_path_operator, pathops.rs: signed-distance-field path operators, composable with the other modifiers in effect order). [new in v7] gradient fill: shape_gradient_fill, the Gradient Stroke modifier's sibling for a shape's own fill (same linear r1/g1/b1->r2/g2/b2 blend along `angle`, independent colour pair, so a shape can carry a flat stroke over a gradient fill or vice versa) -- the one piece of this line that didn't already exist.
- [~] roto: AI object matte / roto brush (needs ML model), mask tracking, refine edge [x roto brush -- roto_propagate (roto.rs) already shipped pre-v7, uncredited: dense optical flow forward from each path-mask keyframe and backward from the next, blended by distance, snapped to the strongest image edge along each point's normal. This IS the engine's roto brush in the same sense After Effects' own Roto Brush is one -- edge/flow-based, not a deep model -- so it genuinely satisfies the non-ML half of this line. x mask tracking -- track_mask (AE Track Mask/Mocha equivalent: translation|similarity|affine|perspective) already shipped pre-v7, uncredited. x refine edge -- refine_matte already shipped and was credited at FEATURES.md's matte-refinement line (L479: trimap + per-pixel fg/bg colour-pair alpha estimation + edge-aware guided filter, explicitly classical/no ML); the same tool closes this item too. 2 new TDD tests (roto_propagate_v7.rs) against a real trackable video, verifying it fills every intermediate frame through the real MCP layer and is deterministic. Honest, stated gap: AI object matte (true one-click ML semantic segmentation, e.g. SAM-class) is NOT built -- there is no ML inference runtime anywhere in this engine (no ONNX/TF runtime, no model weights), and bolting one on is a different scale of work than this pass; the real, usable, non-ML path to the same end result is roto_propagate (seed a few keyframes) + refine_matte (classical edge-quality alpha) together, both already shipped.]
- [x] light/color: layer styles (drop shadow [x as effect], inner/outer glow [x outer], bevel, stroke [x outline], gradient overlay) [x bevel and inner shadow were already shipped pre-v7 as layer styles (layerstyles.rs: ls_bevel, ls_inner_shadow -- credited here for the first time, no checklist entry had named them); new this pass: ls_gradient_overlay (layerstyles.rs), a 2-colour linear gradient tint over any alpha layer along the same angle convention as Gradient Fill/Gradient Stroke (0 top->bottom, 90 left->right -- verified against render.rs' own Source::Gradient, which turned out to use sin/cos swapped from a naive reading; the first test draft caught this directly, not after), opacity, and strings.blend normal|multiply (multiply verified to darken toward the base colour rather than replacing it outright). Generic set_layer_style/list_layer_styles/remove_layer_style dispatch needed no changes -- adding one STYLES entry + one apply() match arm was the whole surface. 4 new TDD tests (layer_styles_v7.rs).]
- [~] responsive time / presets, essential properties, data-driven animation (JSON/CSV), scripting hooks -- presets already covered by template_define/template_apply (generic) and apply_motion_preset (animation-specific), not duplicated. New this pass (ext/data_driven_v7.rs): animate_from_data (keyframes from a JSON array or CSV -- inline, csv_path or json_path -- onto a transform property or effect param, explicit or auto-detected t/time column, scale/offset, clear_existing merge-or-replace); set/list/clear_essential_properties (reserved essential: tags marking which clip/effect params a template consumer should expose, mirrors the color: tag convention); apply_responsive_time (retime a clip to a new duration, protecting protect_in/protect_out seconds at each end -- After Effects Responsive Design-Time; 0/0 is a plain uniform keyframe retime); register_hook/list_hooks/remove_hook/run_hooks (named lifecycle events, each a list of template_apply calls, Project.hooks). Honest [~]: apply_responsive_time and the hooks are explicit calls, not auto-fired by trim_clip/set_speed or by the render/export pipeline -- wiring an automatic pipeline trigger was judged too risky for render determinism/perf in this pass and is left as a documented gap. 24 new tests (tests/data_driven_v7.rs), golden_registry.txt hand-updated for the 9 new tools, old project files unaffected (Project.hooks is skip_serializing_if empty).

- [x] 4. Motion blur for Scene3D geometry and 3D layers, per-effect blur [x all three already shipped pre-v7, uncredited until now: Scene3D motion blur -- scene3d.rs's render_blur (shutter_deg/blur_samples on the Scene3D config, averaging 2-32 re-posed instants across a real shutter window; shutter_deg=0 renders once, identical to a plain render, verified deterministic) is wired into the main render path for every Scene3D clip, so animated 3D objects AND camera moves both blur -- not a layer-level 2D approximation. Per-effect blur -- zoom_blur (radial) and directional_blur (motion blur along an angle) already exist as ordinary effect kinds. New this pass: add_scene3d's schema now documents shutter_deg/blur_samples (previously settable, since Scene3D's Rust struct has #[serde(default)] on both, but undocumented -- an undiscoverable capability reads the same as a missing one). 4 new TDD tests (scene3d_motion_blur_v7.rs): shutter=0 is deterministic single-sample; a fast-moving sphere's shutter=300 render visibly differs from its sharp render; coarser vs finer blur_samples of the same window land closer to each other than either does to the sharp render (proves blur_samples is live, not ignored); the MCP call itself accepts the config.]
- [x] 7. Kimo-exclusive ideas: beat-driven motion (bind any property to audio::analyze beats), motion-by-prompt presets (AI writes the curve set), physics-driven layers [x via circle-proxy simulate_physics], motion "diff" tool (compare two clips' motion curves and match one to the other), auto motion-match to reference footage via tracker. [x all pre-v7 (tools_4.rs/beatcut_tools.rs), uncredited and functionally untested by a dedicated test file until this pass: beat-driven motion -- track_beats (onset-envelope autocorrelation, returns bpm + beat times from any audio/video asset) feeding bind_beats (keyframes ANY numeric property -- scale, rotation, opacity, an effect param -- to hit on every beat with pulse/kick/swing/flash curves and per-beat decay; beat_cut, the pre-existing checklist item, only covers cut-editing on beats, a different tool). motion-by-prompt presets -- apply_motion_preset's `prompt` field (plain-language description -> a matched preset curve set) already does the "AI picks the curve set" half; the engine's job is accepting arbitrary keyframe curves (set_keyframe/animate_from_data), which any calling AI already has. motion diff tool / match one to the other -- motion_diff (per-channel RMS difference + correlation + worst-disagreement frame between two clips' tracked global motion) and motion_extract's target_clip_id (writes one clip's measured motion onto another's x/y/scale/rotation, invert=true for a stabiliser) cover diff and match respectively; copy_curve (pre-existing) is the general keyframe-copy case. auto motion-match via tracker -- motion_extract IS this: whole-frame tracker measures a clip's global motion and writes it onto any target_clip_id as a match-move. 5 new TDD tests (kimo_motion_v7.rs), verified against a real trackable video (a deterministic noise field panned by ffmpeg's crop filter -- a flat colour or pure ramp gives a whole-frame tracker nothing to lock onto) and a real synthetic 120bpm click track (same technique as beat_cut's own test). No source changed; this was pure investigation + test-writing.]
- [x] Infra: HUD timecode as one dynamic text source (currently 2 clips per frame, slow export) [x already resolved pre-v7 and re-verified this pass: add_hud_frame's running timecode/bpm readout is a single dynamic-text clip (render.rs `dyn_text`, token `{tc:S}`), not 2 clips per frame; this pass's own `burn_in_timecode` (FEATURES.md numbers/timecode line) uses the same mechanism and adds the drop-frame token `{tcd:S}` beside it.]

### Proof level

#### apply_motion_preset (motion presets and plain-language prompts)
21 named presets (pop_in, bounce_in, elastic_in, slide_in_*, whip_in, spin_in, zoom_punch, shake, glitch_jitter, wobble, float, pulse (bpm), heartbeat, fades, drop_out, drift_zoom) baked to dense per-frame keyframes on x/y/scale/rotation/opacity relative to the clip's current values. A keyword prompt matcher maps phrases like "bounce in from above, then shake" to presets in mention order (one entrance preset max) and the result lists what was applied. Tier: solid for the preset set; the prompt matcher is keyword-based, not an LLM, and returns the available names when nothing matches. Presets are hand-tuned curves, not yet cross-checked against CapCut's exact timing.

- Graph editor data tools (V1): get_curve (keys, ease, bezier handles + dense value and SPEED samples for a speed graph), move_key, remove_key, copy_curve (offset, time scale, value scale/offset, reverse, invert), simplify_curve (RDP to a tolerance, e.g. 101 tracker keys -> 45 within 1.0), smooth_curve (Gaussian). With set_keyframe_bezier and set_expression these cover the graph editor's data side. Gaps: no multi-property selection/batch edits, no tangent-mode presets (auto/flow/ease-in-out handle types), no value-graph overlay render.

- Beat-driven motion (kimo exclusive, V1): track_beats (BPM + beat grid from any audio/video asset: onset envelope, autocorrelation tempo, grid refinement) and bind_beats (keyframes any numeric property to hit on beats: pulse, kick, swing, flash curves; every-Nth beat; explicit beat lists). Measured on the SCUMBLE reference (true 125 BPM): estimate 127 BPM, beat phase about 90 ms late versus the known hit times, so for frame-tight sync pass explicit beats or an offset. Gaps: no downbeat/bar detection, no per-band (kick vs hat) envelopes, unreliable on rubato audio.

## Tracking and rotoscope

### What it does

#### Mesh tracking and mesh pin (curved / waving surfaces)
- `set_mesh_pin`: layer pinned onto an nx*ny vertex mesh, per-cell perspective, motion blur. Tier: solid (verified on a synthetic waving flag).
- `track_mesh`: engine-level 3-level image pyramid (coarse-to-fine, 4x/2x/1x) per-vertex photometric Gauss-Newton against the frame-0 patch, sequential with velocity prediction, aperture-safe ridge, neighbour-regularised. Tier: BETA+.
  Measured on a synthetic waving flag (9x6 grid, 100 frames, textured, motion blur): mean vertex error 3.3 px, median 2.2 px, p95 8.6 px (was 12.3 / 7.7 / 30 before pyramid + ridge). 40 of 54 vertices stay under 2 px. 3-4 vertices on horizontal stripes (aperture problem, x unconstrained) still slip 25-110 px late in the shot. Track 14 s for 100 frames.
  Known gaps: no per-vertex occlusion, no topology change, stripes-only regions drift along the stripe.

#### motion_extract / motion_diff (global motion match and compare)
`motion_extract` tracks the whole frame (inset quad, drift-free anchor tracker) and returns per-frame dx, dy, scale, rotation relative to the first frame; with `target_clip_id` it writes them as keyframes so text/graphics ride the footage (match-move), `invert` makes it a stabiliser, `gain` scales it. `motion_diff` compares two clips' global motion (RMS and correlation per channel, worst frame), e.g. a recreation vs its reference.
Measured on a synthetic handheld plate with known motion (100 frames): mean error dx 0.51 px, dy 0.73 px, scale 0.00 %, rotation 0.001 deg; max 1.1 px. Extract 7.3 s for 100 frames. Tier: BETA. Global similarity only: no parallax, no rolling shutter, no depth layers; a surface that changes appearance a lot over the shot (cuts, big occlusions) needs a restart range. Only synthetic ground truth so far, real handheld footage test still owed.

#### Long-gap tracking: track_long_gap (read-only) - BETA
Appearance-template tracking across occlusions (immutable seed template, whole-canvas multiscale search, ambiguity rejection, consecutive-frame reacquisition). NOT semantic re-ID. Workstream evidence: controlled 300-frame / 12 s real-footage blackout: zero invented boxes in the gap, +60 px displaced reacquisition within 2 frames, 5.55 px mean error after return, 3.61 s for 369 frames; natural faceocc2: 220/812 boxes, 6.60 px mean error, 7 reacquisitions, stays lost after frame 317 (rotation and appearance change). Limits: rotation, large appearance change and identical distractors. Reviewed here: build, 6 unit tests, fuzz 168 tools / 6512 calls / 0 failures.
Long-gap level 2 (opt-in `appearance_memory=true`, default false = v1 behavior; BETA): bank of 8 learned anchors, learns only from 3 spatially consistent strong observations and never while lost or tentative. Natural faceocc2: 528/812 boxes (v1 220), 9 reacquisitions past the old permanent-loss point, 8.15 px mean error, 0 boxes under 0.5 IoU vs ground truth (v1: 70/220 under 0.5, min 0.380, undersized boxes, not wrong-person locks; post-blackout 15/58 vs 0/56). Controlled blackout still clean (zero gap boxes, 3.24 px). Wording: v1 is center-accurate but boxes can shrink; memory mode gives tighter boxes and drift survival, opt-in because of contamination risk; neither is a general no-false-lock guarantee. Limit: frames 650+ at full cap still lost. Reviewed here: 8 unit tests, fuzz 168 tools / 6516 calls / 0 failures.

### Parameters and controls

- `track_planar` (homography, 4 corners) + `pin_layer` (perspective corner pin, keyframed, 3x3 supersampled AA edges, premultiplied) + motion blur on the pinned layer from the tracked motion + `light` brightness match.

### Limits

Read the gaps, pending items and scope qualifications in this module's records, together with the BETA appendix.

### Proof level

#### Plane-pose motion model (planar tracker core)
- `pose_predict`: constant-velocity extrapolation of a tracked plane in pose space (rotation vector + translation, plane size fixed), used to coast through occlusion and when the surface leaves the frame. Unit-tested exact (1e-12 px) on a synthetic rotating plane. Tier: BETA.
- Measured on the 3D overtake room: coast error at the end of the shot 1407 px -> 1179 px (the sign is fully off-frame there, so this is not visible in the composite). It does NOT extend how far the tracker survives: tracking still ends at about 3% visible, because the data is gone. Using the pose prediction as the always-on prior made tracking worse (noisy pose from 1-2 px quad noise), so it is only used while lost.
- Focal length is assumed 0.9 x frame width (env KIMO_F overrides, for tests). No focal self-calibration yet. Edge-on survival below about 3% visible is a data limit, not solved.

#### Real handheld footage test (motion_extract), honest result
Clip: Wikimedia Commons "Wzwz munich petuelpark 09d" (CC0, 540x720, 3.7 s, handheld tilt with a sudden bright burst filling the frame near the end). motion_extract measured a large move (dy 138 px, roll 13 deg, scale 0.72). Match-moved text sits on the rock through the shot, including the burst. Inverting the motion as a stabiliser cut the residual camera motion from std dx 24.5 / dy 36.8 px / rot 3.8 deg to dx 2.7 / dy 22.6 px / rot 0.89 deg, i.e. roll and sideways motion are mostly gone, but vertical motion is only reduced by ~40 %: the burst and heavy motion blur break the whole-frame track in the second half. Tier stays BETA. Needs a restart/segment mode and a robust exposure-jump gate before it can be called solid on real footage. Track 6.3 s for 93 frames at 540x720.

#### Planar tracker (mocha-style) - built, tiers

- Mocha/AE/Resolve-style algorithm (not validated against those products): sub-pixel planar track, perspective pin, occluder-robust (Tukey weights), exposure-invariant (zero-mean/unit-variance photometric model).

- Synthetic fixture measurement: anchor-relative by re-aligning to the frame-0 anchor each frame; coasts through full occlusion and re-acquires with a coarse search. Synthetic perspective move + h264 + 180deg blur + occluder: 0.33 px mean / 0.39 px p95 corner error vs ground truth (examples/planar_demo.py, tools/wall_plate.py).

- Shipped separately: beta mesh/grid tracking, optional reanchor segments and anchor/adaptive auto-mattes (tiers below). NOT YET: backward tracking from a middle frame, general multi-anchor management and grain/lens match on the pinned layer.

- Occluder auto-matte (`track_planar occluder_matte=true`): at render time the live frame is compared with the frame-0 surface (warped by the tracked quad, brightness-matched); wherever they differ the pinned layer is cut, with 2 px grow and feather. Tier: automated difference-matte workflow, not independently measured Mocha parity; limits: occluder with the same colour as the surface is not detected, persisted recipe rebuilds the anchor after reload, no temporal smoothing of the matte yet.

- Adaptive occluder matte (`occluder_mode=adaptive`, LIMITED-USE tier, measured on one real clip): rounded 0..8 plane-px min/max-envelope compare (default 1) (`occluder_tolerance`), chroma-dominant metric (`occluder_chroma`), island/hole cleanup by area (`occluder_min_area`), thin-structure cleanup (`occluder_thin`, two-stage so fragments are not merged), temporal median (`occluder_temporal`), grey closing (`occluder_close`, default off, did not fix notches connected to the occluder), persisted recipe that rebuilds after reload, follows constant clip speed and speed-ramp clips with the same frame selection as the renderer (decoder phase bug found and fixed on the real key). Real keyboard/finger result: finger protected, no flashes over all 110 frames, glyph islands and contour rings removed, residual small notches touching the finger edge at entry and mild edge scallop. NOT claimed: neutral or dark occluders (chroma 1 cannot cut them), universal matting, or a clean solid-key replacement. Synthetic leak 0.01% to 0.14%; synthetic wins did not transfer to the real clip, so only the real-clip numbers count.

- Rotoscope assist (`roto_propagate`, BETA, weak gain): fills a path mask between your keyframes with per-frame optical flow (robust affine fit to interior flow, forward from one key and backward from the next, falling back toward plain interpolation where the two chains disagree) and snaps the outline to the strongest edge within `snap` px. Measured: exact-GT synthetic (textured ellipse rotating and accelerating over real keyboard footage, mean IoU over held-out frames): keys 6 frames apart interpolation 0.837 vs roto 0.870 (snap 6), 10 apart 0.674 vs 0.708, 25 apart 0.141 vs 0.144 (flow chain not trusted, so it falls back; raising `trust` to 1.5 reaches 0.335 at 25 frames but drops 6-frame IoU to 0.72). Hand-traced real finger (keys 12 frames apart, one held-out frame): 0.732 interpolation vs 0.770 roto; that ground truth is hand-traced by eye (about +-5 px), so treat it as indicative only. Memory is half-resolution gray per segment (300 frame cap). Not a replacement for dense matting: gains over interpolation are small (+3 points) and fast large motion is unsolved. SCUMBLE independent real-finger 8-frame trace (keys .64/.96, held-out .80): interpolation IoU 0.727 vs roto 0.747 (+2 points, less background spill, mean snap 2.2 px), held-out fingertip and lower edge still visibly missed: NOT a production neutral matte even with 8-frame keys, and key spacing does not guarantee accuracy. Guided-filter refine (mask `refine` 4/8/16 px) on top of roto does not help on this crossing (IoU 0.745 vs 0.745/0.740/0.740, false px 556 vs 622/688/720): the error is shape/motion, not edge softness; hand-traced ground truth is about +-5 px on a ~40 px finger, so differences of 1-2 IoU points are inside the noise (examples/roto_refine_test.py).

- Speed ramps: planar tracking and the occluder matte read ramp clips at the displayed source frame (`Clip::source_index`, floor(source_time*fps+1e-6)); the pin itself is rendered from keyframes the tracker wrote at output times, so the pin path does not call source_index at render time and a ramp edit needs a re-track; decode is windowed (COLOR patches 127/128). Renderer-aligned tracker vs export max 1.41 px over 87 frames. Reverse/freeze time modes are not handled by the occluder.

- Off-frame prediction (planar): homography-extrapolated motion prior pulls corners toward the predicted quad in proportion to how much of the surface has left the frame. Overtake room test (sign leaves frame left), mean corner error vs ground truth: 33% visible 14.6 -> 5.6 px, 22% 18 -> 7.6 px, 12% 20 -> 14.6 px, 4% 61 -> 34 px. Track still declared lost at about 3% visible (frame 120 of 138). Not solved: true edge-on (width -> 0) needs a camera-motion model, not just a homography prior.

- Planar tracker robustness (real footage): quad validity gate (convex, bounded area/corner jump) and a confidence-drop rule (a sudden fall to under about 0.7-0.8 of the recent mean means occlusion: coast, do not follow). Real-footage case (SCUMBLE reference, overpass face 8.9-11.5 s): tracks the shake settle (about 15 px) at conf 0.87, then a full-frame white title slam covers the surface and the track holds the last good quad instead of latching onto the title (before the gate it diverged to -900 px). Does not re-acquire within that clip because the title stays up. Limit: real-footage coverage is one shot; no long handheld test yet.

## Particles, shards and glass

### What it does

#### Shard burst (add_shard_burst) - V2
3D glass-shard cloud generator: hundreds of prism shards, project-camera projection, internal depth sort, DOF softness, rim glints with dispersion, 4-sample motion blur. Gaps: background-aware screen-space displacement shipped (see below), not ray-traced refraction; shard shadows exist two ways: the burst's own contact_shadow (see Contact shadows for debris) and the generic cast_shadow effect, which also works on a shard layer (measured on a grey plate, 40 glass shards: 5090 plate pixels darkened outside the shards, 0 brightened, 0 shard pixels changed); no physical light calibration, not GPU.

#### fracture_glass (native effect, v1) - BETA
Procedural laminated-glass shatter, a normal clip effect (add_effect kind=fracture_glass), deterministic per seed, binds the live post-effects clip canvas (no re-grade, source time continues).
- Topology: irregular Voronoi cells from jittered rings around one impact point (t0, impact_x/y). Ring site counts step up outward (branching), plus `debris` fine fragments near the impact, so big shards and small debris mix. Crack edges zigzag and vary in width and brightness (dimmer near the centre, sparse glints).
- Dynamics: cracks grow outward at crack_speed (not all present at frame 1). Intact facets refract the live source with RGB dispersion. Cells release in a radial stagger (release_delay, release_spread, release_radius), then fall with gravity, push, 3D tumble about a random in-plane axis (perspective foreshortening, back face visible), slab thickness walls, and sub-frame motion blur (blur_samples x shutter_deg at `fps`). Fade is absolute from impact (fade_start, clear_time). Holes are transparent with a hole_dim black overlay, so the lower layer shows through.
Measured: 1280x720 about 0.23 s/frame intact, 0.4-0.65 s/frame while shards fall (5 blur samples, 2 vCPU).
Honest limits: single impact point; shards are flat polygons with a constant-thickness slab, no refraction through the slab or edge caustics; no shard-shard collisions or ground contact; glint is a fixed procedural light, not an environment; outer pane still reads as roughly even cells (no long hairline radial cracks); no spall or micro-cracks; look judged by eye on one contact sheet, not against a ground-truth reference.

### Parameters and controls

fracture_glass v1.1 (BETA): ring budget planner (rings always reach the frame edge, ring_growth is coarsened to fit max_cells, cap 2500, cost about N^2), edge tint params (edge_r/g/b, neutral-ish default), stronger per-shard tumble variance plus an initial per-shard tilt at release (pieces read as individuals), per-shard per-sample time jitter in motion blur (no stepped echoes), cracks masked out of holes (a released cell's cracks die with it) and optional `crack_clear` absolute fade for the remaining pane, glass material response (face_opacity, grazing-angle fresnel reflection, moving sheen streak, edge sheen). Limit: the effect cannot refract the layer below (it only sees its own canvas), so translucency is plain alpha over whatever is underneath.

fracture_glass v1.1.1: far slab face now shares face_opacity (it was an opaque grey polygon behind translucent faces), more ring jitter so the outer pane stops reading concentric.

- fracture_glass v1.2 (BETA): per-shard velocity decorrelation (`vel_noise` direction/speed/swirl, `release_jitter` lead/lag) so the burst is not a uniform funnel; shard faces use a lens/tilt-distorted, dispersed texture sample (`face_refraction`). Limit: the effect only sees its own canvas, so it cannot refract the layer beneath; size-dependent drag and shard collisions not done.

### Limits

Read the gaps, pending items and scope qualifications in this module's records, together with the BETA appendix.

### Proof level

#### SCUMBLE hero shatter recreation (examples/scumble_shatter.py) - V1 (recreation, not parity)
Composed from existing tools: keyboard bg with push-in and blur ramp, finger/key sprites, orange spark emitter, physics shard burst with camera fly-in and zoom-blur streaks, detection-box HUD. Gaps vs reference: finger/key/keyboard are drawn stand-ins (no real macro footage or 3D model), shards lack true refraction of the scene, no caustics, lighting is fake, no rigid-body interaction with the finger.

#### Background-aware refraction (shard burst refract:px) - V1
The shard pass writes a screen-space displacement field (tilt x thickness, stronger on side walls, motion-blurred with the shards); the compositor bends the canvas behind the layer before blending. refract_dispersion (0..1, default 0 = unchanged) now bends the background per channel (R, G, B offsets scaled 1-d, 1, 1+d, blue most). Proof tests/shard_refract_dispersion_proof.py on a luminance-ramp background with the shard layer at opacity 0: at d=1 the red channel equals the unbent render, green equals the d=0 render, and blue's change is 2.00x the d=0 change (median over 4082 px); d=0 is bit-identical to the default. Gaps: the per-channel scale is a linear offset split, not a measured glass dispersion curve (no Abbe number), displacement clamped, shard layer only (not a general refractive material for any layer), no caustics.

Shard geometry V1.5: 4-6 vertex irregular convex chunks with varied aspect and thickness. Gaps: still extruded polygons, not fractured volumes (no Voronoi-fractured solids, no bevel normals).

- Shard planes behind a moving occluder (`shard_burst` plane.occlusion_target + `occluder_expand`, LIMITED-USE tier): the shard plane consumes the target clip's adaptive matte in final screen space (scale, pos, rotation, motion path, 3D pose, project camera), evaluated on demand so track order and target opacity do not matter. Measured: synthetic magenta-over-skin 2476 -> 0 at target scale 1.12; real macro key shot, original order, 35 burst frames: interior finger protected, small boundary fringe remains, not a dense body mask. Neutral/dark occluders: `occluder_luma_gate` removes 100% core leak to 0.0% (false reject 0.0%) on COLOR's synthetic neutral fixture only, FAILS on real monochrome keyboard footage: gate-on cuts 5.2%/6.1%/1.7% of a clear key patch at 1/2/3 s (false rejects on printed key surface), gate-off cuts none; stays experimental, not neutral-occluder support. Mode 2 (`occluder_luma_gate=2`, brighter-than-surface only) cut that real false-reject to 0.0% at 0/1/2/3 s and keeps 0% core leak, 0% false reject on the synthetic fixture; on a REAL finger crossing over a dark key mode 2 FAILS the bar: skin leak 22.5% (1 s) and 38.7% (1.5 s) of bright skin ROI, plus 47% of dark surface wrongly cut on retreat (ghosting). Experimental, not usable for neutral occluders.

- Contact shadows for debris (`shard_burst` contact_shadow 0..1, shadow_reach, shadow_plane_z; BETA): each shard projects a soft shadow onto the receiving plane along the light vector, blur and opacity falling with height. Measured on a synthetic grey plate: shadows only darken (0 brightened px, 0 shard px changed), 1887 plate px darkened by mean 3/255 at t=1.0 strength 0.7. Not yet checked on a real shot; shadow direction follows the `light` vector and has not been validated against real footage lighting. Shadow respects the occluder matte (real key repro at 1.64, strength 0.6: 1632 plate px darkened, max 8.7 codes, 0 on the finger, 0 brightened; subtle on dark keys). Frame cost about +0 s on the synthetic scene (1.9 s on vs 2.3 s off, noise).
  CORRECTION (COLOR found it, fixed in this commit): before this fix the contact shadow was effectively only a thin halo around each shard, because the polygon signed distance had the wrong sign (two negatives cancelled), so the earlier measurements above (1887 darkened px / 1632 px on the real key) described that halo, not a grounded shadow, and ON vs OFF could render byte-identical in clean fixtures. film10 and earlier deliveries have no real contact shadows. After the fix (examples/shard_shadow_test.py): synthetic grey plate 76681 px darkened (mean 38.7 codes at strength 0.6, max 122), 0 brightened, finger preserved on the real key repro (40285 px darkened, mean 17.7, max 90 at strength 0.6; 2392 px differ upward by >1 code, under translucent shards, not investigated). 4 regression tests (contact_shadow_validation): ON differs from OFF, fades with height (0.51 near / 0.12 far peak alpha delta), reverses with light direction, exact zero when occluded. Still: fixed-z receiver, overlap summed and clamped, no real-light calibration; strength 0.6 is strong, tune down for subtle grounding.

- Shard motion blur controls (`shard_burst` shutter_deg default 180, 0 = off; blur_samples default 4, 8 with texture; BETA): the old fixed 4 time-samples left stepped ghost copies with visible wireframe outlines on fast shards. Measured on the real burst repro (1.28 s and 1.64 s, RMSE in the shard region vs a 32-sample reference): 4 samples 2.07/1.42, 8 samples 1.38/0.71, 16 samples 0.44/0.40. Cost is linear: 1.64 s frame 2.2 s at 4, 4.7 s at 16, 8.6 s at 32 (1280x720, one render_frame). Defaults unchanged so existing renders are byte-identical. Blur is time-sampled, not per-shard velocity streaks; sample count is manual (no automatic velocity-adaptive count yet).

- Shard trails (`shard_burst` trail 0..1, trail_length s default 0.12, trail_samples 2..24 default 8; BETA): faded earlier-time copies of every shard, drawn with the same shader and occluder behind the shard (older = fainter). Off by default; legacy renders byte-identical (md5 equal on the 1.64 s burst repro frame). Measured on that burst: trail 0.7 length 0.15 s changes 101,003 px, 0.0 diff in the finger region, costs about +8% render time in my run (21.1 s vs 19.5 s, first-frame decode included). Honest read: on this scene the shards are near-black over a dark keyboard so the trails are subtle; not judged on a bright plate, no per-shard velocity-based length, trails are time-lagged copies (not particle emitters).

- Shard looks (`shard_burst` material legacy|glass|concrete|metal, size_bias, size_scale, env_reflection, env_roughness; BETA, SHARD-LOOK workstream, 5 render tests + 9 module tests): opt-in only. Verified here: legacy configs render BYTE-IDENTICAL (md5 equal on the 1.28 s and 1.64 s burst repro frames before/after the patch); metal/glass on that burst change only the shard pixels (finger/background region diff 0.0) and render in 14-17 s/frame at 8 blur samples. Caveats from the authors: synthetic scene only, material is per burst not per shard, analytic sky/ground environment (not an HDRI), fine glass reads darker and more restrained than the legacy rims. No claim of photoreal glass.

- Backdrop access for effects (BETA, engine capability): an effect can declare `needs_backdrop` (effects2::needs_backdrop); the renderer then gives it a read-only copy of the composite below its layer via render::backdrop() (one frame copy, built only for clips carrying such an effect; thread-local so parallel export is safe). First user: fracture_glass `transmission` (0..1, default 0, so existing output is unchanged): shard faces mix in the real background, refracted with per-channel dispersion. Verified visually on the test rig (faces show the background instead of flat source chips); not yet verified on the film, and refraction is a distortion offset, not a ray-traced lens. Limit: only fracture_glass uses it so far.

- Backdrop fix: the backdrop is now set right before a layer's own effects, so it also works on precomp layers (nested render of children no longer clears it). Verified: a precomp-wrapped fracture_glass with transmission renders pixel-identical to the flat clip, and transmission 0.9 differs from 0.0 in the nested case.

## 3D and physics

### What it does

#### 3D suite status
Done: layer transforms, camera, shard-internal depth sort/DOF/motion blur. Scene3D additionally has materials, lights, optional shadows/AO/env and null parenting. Todo: general layer depth sorting, DOF/motion blur on flat 3D layers, extrusion and camera rigs (orbit, look-at).

#### Rigid-body physics (physics.rs, used by add_shard_burst physics:true) - V1
Deterministic 240 Hz baked sim: mass from size, linear/angular velocity, gravity, linear and angular drag, floor plane with restitution + Coulomb friction (spin from contact), body-body impulse collisions (sphere proxies, spatial-hash broadphase). Scrub-safe via cached trajectory. Gaps: sphere proxies not true convex hulls (no resting stability on faces), no joints/constraints/soft bodies/fluids, general-layer simulate_physics is a separate 480 Hz circle-proxy tool (see below), CPU.
Shard shading: Schlick fresnel and depth falloff params. Screen-space background displacement is applied by the compositor when refract is enabled. Gaps: no ray-traced refraction, caustics or area lights.

#### Scene3D (add_scene3d) - V1
Real geometry renderer: z-buffered triangles, box/rounded-box/cylinder/capsule/sphere/plane/null, parent transforms (nulls), keyframed pos/rot/scale per object, PBR-style materials (albedo, roughness, metal, emissive, opacity, Schlick fresnel, Blinn-Phong GGX-like spec), directional and point lights, ambient, 2x2 SSAA, depth-of-field from the depth buffer, transparent objects sorted back to front. Seen through the project camera, composes with shard bursts and everything else. Optional shadows/AO/procedural environment reflections are described below. Gaps: no global illumination or HDRI environment-map import, no textures/UVs, no mesh import (OBJ/glTF), no extrusion of text/shapes, no motion blur on scene geometry, transparency is per-object sorted (no per-pixel OIT), CPU only.

Scene3D additions (V1): shadows:true (1536 px shadow map per directional light, 3x3 PCF), ao:<strength> (screen-space ambient occlusion from the depth buffer), env:<strength> (procedural studio environment reflections with fresnel, driven by the lights). Known artifacts: faint facet banding on the curved capsule under shadowing (bias tuning), no soft area-light penumbra, shadows from opaque objects only, point-light shadows not done, env is procedural (no HDRI import).

#### simulate_physics (2D layer physics baked to keyframes)
Motion-graphics physics for layers: circle-proxy bodies (gravity, drag, restitution, friction, canvas walls, collisions, spin) simulated at 480 Hz and baked into dense x / y / rotation keyframes. Scrub-safe, editable in the graph editor, motion-blurs normally. Tier: solid for circle proxies; no polygon/concave collision shapes, no joints, no pins. Example: examples/physics_demo.py (14 shapes dropping and stacking, 5 s, 74 s render at 1280x720 with 180 degree motion blur).

### Parameters and controls

Use `tools/list` / `list_effects` for live schemas. Read the source parameter names, ranges and defaults alongside their evidence and limits in this module.

### Limits

Read the gaps, pending items and scope qualifications in this module's records, together with the BETA appendix.

### Proof level

Use the quality-tier audit and test notes in the BETA appendix; checklist marks are not a professional-tool parity claim.

## Effects catalog

### What it does

#### C. After Effects effect categories
(AE category -> kimo effect, based on Adobe's effect list)

#### Blur & Sharpen
- [x] Gaussian-style blur, directional blur, spin (radial) blur, zoom blur, sharpen (unsharp), tilt-shift (lens blur band)

#### Channel
- [x] channel mixer, invert, solid via gradient ramp

#### Color correction
- [x] brightness/contrast, curves (luma and free master/RGB), levels (master), exposure, hue/sat (HSL), tint, colorize, color balance via wheels, gamma/pedestal/gain, photo filter-like via temperature, shadow/highlight, vibrance-like via saturation

#### Distort
- [x] bulge, twirl, ripple, wave warp, turbulent displace, polar coordinates, offset, mirror, motion tile, flip (transform), lens distortion (optics compensation), kaleidoscope-style, corner pin, glitch blocks, pixel sort
- [~] beta mesh pin/tracking shipped; pending: bezier warp, general displacement map (using another layer), liquify, magnify, reshape, smear, spherize, warp (presets: arc, flag, fish...), warp stabilizer (stabilize exists via tracking) , detail-preserving upscale, transform effect, rolling shutter repair, CC (bend it, ripple pulse, page turn, lens, power pin, slant...), wave presets

#### Expression controls

#### Generate
- [x] checkerboard, grid, gradient ramp (linear/radial), fractal noise (via fractal_noise), lightning / energy beam, lens flare, circle/ellipse/rect via shape layers

#### Immersive / VR

#### Keying
- [x] chroma key (color, tolerance, softness), luma key, spill suppress, choker, feather alpha
- [~] keyer with screen gain/balance, clip black/white, despill and edge unmixing (synthetic tests, not Keylight parity); pending: color difference key, color range, difference matte, extract, inner/outer key, linear color key, spill suppressor for any color

#### Matte
- [x] matte choker (choker), feather alpha

#### Noise & Grain
- [x] film grain, noise via fractal noise, film damage (scratches, dust, flicker)

#### Perspective
- [x] corner pin, drop shadow (alpha), long shadow
- [~] project 3D camera shipped; pending: bevel alpha, bevel edges, radial shadow, CC cylinder/sphere/environment, extrude

#### Simulation
- [x] particles (embers, sparks, snow, rain, dust) via add_particles

#### Stylize
- [x] glow, outer glow, edge glow (neon), find edges, emboss, halftone (mosaic-like dots), pixelate (mosaic), posterize, threshold, strobe, scanlines, VHS, vignette, outline, sepia, invert

#### Text

#### Time
- [x] time remap via speed ramp, reverse, freeze
- [~] echo/frame blend, posterize time and forward optical-flow source blur shipped; pending: time displacement, flow interpolation/retime, slowmo and per-effect force blur

#### Transition effects (as effects, not clip transitions)

#### Utility
- [x] letterbox (cinema bars)

#### Obsolete / classic still used by pros
- [~] basic text, perspective 3D and path text shipped; pending: lens blur, lightning (classic), channel blur, bevel, bulge-style CC effects, reduce noise

#### Third-party style looks (Hollywood staples, implement natively)
- [~] light leaks, anamorphic streaks, lens flare, film grain, halation, bloom [glow], chromatic aberration, lens distortion, VHS, glitch
- [~] approximate halation, film-print and bleach-bypass effects shipped; pending: calibrated 35mm film emulation and film gate weave, day-for-night, volumetric light, fog / atmosphere, heat haze [x turbulent_displace], energy shield, hologram, screen replacement (corner pin + tracking), sky replacement (needs segmentation), digital clone/duplicate, speed-line, rain on lens, camera shake presets [x shake], rolling shutter, lens dirt

### Parameters and controls

- `silhouette_fill` (effect): flat-color silhouette of a clip's alpha, mix amount, optional outline ring. Tier: solid. Gap: no pattern fills, no auto-segmentation (mask comes from an RGBA cutout).

- `add_glitch_confetti` (generator): random flat squares + horizontal tear lines, reshuffled at flicker_fps. Tier: solid. Gap: no block displacement of the plate under it (use glitch_blocks).

- Masking stack (masks.rs): see docs/MASKING_ROADMAP.md for the full table and honest tiers (ML roto, mask tracking, per-point feather QUEUED).

### Limits

- [x] camera lens blur with depth map / bokeh shapes, bilateral, channel blur, compound blur (using layer map), fast box variants, smart blur, reduce interlace flicker [x already shipped pre-v7, uncredited: depth map (add_lens_blur's depth_clip_id -- another clip read per pixel, blur radius scales with its distance from focal_depth), bokeh shapes (lens.rs's n-blade iris taps with rotation/roundness -- a real golden-angle-spiral polygon iris, not just a round disc), bilateral (bilateral_blur, credited at FEATURES.md L131, a genuine edge-preserving blur), fast box variants (the base `blur` effect's box_blur3 -- the standard 3-pass box approximation of gaussian used throughout this engine). compound_blur (using layer map): not built as a separate tool -- add_lens_blur's depth_clip_id IS a compound/map-driven blur (set blades=0 for a plain round kernel, drive the radius from any map clip's luma); a second near-duplicate two-clip tool was judged not worth the added surface. New this pass (effects2.rs, through the generic add_effect/list_effects tools, no dedicated Command needed): channel_blur (R/G/B blurred independently by different radii -- chromatic-fringe look, alpha untouched), smart_blur (box-blurs flat/noisy areas but any pixel whose luma moved past `threshold` from the blur is restored to its sharp original -- real edges survive, flat areas and noise do not), reduce_interlace_flicker (a 3-tap 1-2-1 vertical-only low pass, the classic fix for thin horizontal detail that flickers on interlaced displays). 3 new TDD tests (lens_blur_extras_v7.rs). golden_registry.txt hand-updated for add_effect's grown kind enum.]

- [~] arithmetic, blend, calculations, channel combiner, compound arithmetic, minimax (erode/dilate, partial: choker), remove color matting, set channels, set matte, shift channels, solid composite [x blend -- every clip's existing `blend` field already carries the full 22-mode W3C-plus-AE-classics set (add/multiply/screen/overlay/difference/etc, render.rs BLEND_MODES), which is also the operator AE's own Arithmetic effect offers between two sources -- the single-source "arithmetic against a constant" half of that is just a solid-colour clip composited with one of these modes; x set matte [`set_matte`, layerfx.rs, pre-existing]; x shift_channels -- new effect kind (effects2.rs): reassigns each output channel from any input channel, a constant, or an inverted channel, with a real range-checked error; x solid_composite -- new effect kind: fills behind or in front of the layer with a flat colour at a given opacity, inside the effect itself; x arithmetic/calculations/compound_arithmetic/channel combiner -- new `add_channel_combiner` tool (layerfx.rs, reusing the set_matte/displacement_map clip-reference pattern): combines THIS clip with ANOTHER named clip (add/subtract/multiply/screen/difference/darken/lighten, opacity-mixed, a single channel of the reference can drive all three outputs), the genuine two-clip arithmetic the plain `blend` field (against whatever happens to sit below on the timeline) can't express. Hides the reference clip from the main composite by default, same convention as set_matte's hide_matte. Proven by test: add/multiply combine two DIFFERENT clips to the expected closed-form values, opacity 0 leaves the clip untouched, the reference clip stays hidden even when it sits on a track above, and self-reference is refused; x set_channels -- new `add_set_channels` tool: builds each output channel (red, green, blue, alpha) from an arbitrary source -- this clip's own data, a named channel of the referenced clip, or a flat on/off value -- closing the exact gap named below (a second clip as a channel source). Required a render.rs change beyond the existing clip-reference pattern: set_channels is useful even with NO reference clip (pure this/on/off sources), so it's now always routed through the special path instead of only when "driven" (a `ref_clip` string is present) -- discovered via a real test failure (on/off sources silently did nothing) before the fix, not assumed. Proven by test: pulling a channel from the reference clip, on/off forcing flat values, a real red<->blue swap, and refusing to run without a ref_clip_id when a source needs one; x remove_color_matting -- new effect kind: undoes a straight-alpha layer's fringe from having been composited against a known solid background before it reached you (`original = (observed - bg*(1-alpha))/alpha`), verified against a synthetic known-fringe case. 4 new TDD tests (channel swap, an out-of-range source rejected at render time, solid_composite's behind-fill blend, remove_color_matting recovering a known original colour -- the last one caught a real test-authoring mistake of mine: the clip's own alpha stays unchanged by the effect, so the final frame still blends through it against the project's own canvas, which the first draft of the test did not account for).] Honest remaining gap: minimax's true per-RGB-channel erode/dilate (not just alpha, which `choker` already covers) was not built this pass.

- [~] color balance (HLS) [x `hsl`/`hs`], black & white mixer [x `channel_mixer`], selective color [x `qualifier`, HSL secondary], tritone [x via gradient_map], leave color [x color_isolate], auto levels/contrast/color [x `auto_color`/`auto_grade`/`auto_balance`], vibrance [x `color_boost`, verified -- boosts weak colour only, strong colour/luma kept, exact Resolve-primaries-bar definition], match color [x `match_color`, Reinhard transfer], PS arbitrary map [x `point_curves`/`curves`]; pending: levels per channel (current `levels` is master-only), change to color / change color, CC tools (toner), color link, equalize, easy levels, output (clip) as a grade-node concept, color stabilizer, and a full Lumetri-parity single tool (the pieces exist separately)

- [~] slider, checkbox, color, point, angle, dropdown controls feeding any param [x `slider_control`/`checkbox_control`/`angle_control`/`point_control`/`color_control`/`dropdown_control` -- identity effects, keyframeable/animate_from_data-able/link()-able since they're ordinary effect params, crates/kimo-core/src/color/controls_v7.rs, 7 TDD tests]; layer control not shipped -- it would hold a reference to another clip, and link("clip:property") already takes a clip reference directly as its own argument, so a dedicated effect would duplicate that with no new capability

- [x] 4-color gradient, advanced lightning variants (strike, branch decay) [x `lightning`'s new strike/branch_decay params], audio spectrum, audio waveform, beam (laser), cell pattern, eyedropper fill, fill, fractal (Mandelbrot/Julia), paint bucket, radio waves, scribble, stroke, vegas (animated outline), write-on, caustics -- new crates/kimo-core/src/color/generators.rs module, 15 kinds registered via add_effect/catalog3, 16 TDD tests (tests/generators_and_lightning_v7.rs)

- [x] VR to sphere, VR rotate sphere, VR blur/sharpen/denoise/glow, VR chromatic aberrations, VR plane to sphere, VR convert projection -- 8 new effect kinds in vr.rs: vr_rotate_sphere (yaw/pitch/roll via direction-vector rotation), vr_convert_projection (equirect<->fisheye, covers VR to/from sphere), vr_plane_to_sphere (place flat footage onto its gnomonic patch of the sphere), vr_blur/vr_sharpen/vr_denoise/vr_glow/vr_chromatic_aberration (seam-wrapping variants of the ordinary effects: horizontal wrap at the 360 seam instead of clamping, proven against the ordinary non-wrapping kind on identical content). Honest limit, documented in vr.rs: vertical (the poles) is clamped, not wrapped -- a true pole crossing needs a reflect-and-rotate-180 that none of these do. 14 TDD tests (vr_suite_v7.rs).

- [x] refine soft matte, refine hard matte, simple choker variants, matte tools for tracked mattes [x `refine_matte` (classical trimap + colour-sampled alpha + guided filter, hair/edge quality) and `choker` (alpha erode/dilate) both pre-existed but had no functional test through the MCP command layer -- only incidental mentions in description-length/external-refs classification tests. Verified this pass with 4 new TDD tests: choker's positive amount shrinks and negative amount dilates a shape's alpha edge (confirmed against a background clip, since choker is alpha-only -- a newly-dilated pixel takes the shape's own unpainted colour, not its fill colour); refine_matte's `points` mode attaches a matte mask layer from a seed outline; refine_matte's `matte_dir` mode refines a real track_matte output across frames. "refine hard matte" reads as the same classical refine_matte path with a tight band/low samples for a crisper edge -- no separate tool, since the honest/soft-vs-hard distinction here is a parameter choice, not a different algorithm.]

- [~] add grain with presets, match grain, remove grain, dust & scratches (cleanup), median, noise HLS, noise alpha, noise (additive/mono), motion/temporal denoise [x add grain -- the pre-existing `grain` effect's Film-Look-Creator-style shaping (size, softness, per-channel colour, shadows/mids/highlights response) already covers the parameter space named presets would just be saved snapshots of; no separate "with presets" tool built, since presets are a param-set convenience, not new capability; x noise (additive/mono) -- `grain`'s own `color: 0` mode is literally mono additive noise, pre-existing, credited here for the first time; x remove grain -- the pre-existing `denoise` effect (color/denoise.rs, edge-aware spatial, draft-tier) is the same operation under its honest general name, already shipped; x median -- new effect kind (effects2.rs): per-channel neighbourhood median, a real windowed-median filter (not a blur -- verified it pulls an isolated small bright patch toward its surroundings while leaving a flat field exactly alone); x dust_scratches -- new effect kind: AE's despeckle, the SAME windowed median but applied only where a pixel differs from its local median by more than a threshold, so clean footage is untouched (verified); x match_grain -- new `add_match_grain` tool (layerfx.rs, same clip-reference pattern): estimates a REFERENCE clip's own grain amplitude (a high-pass residual -- the reference minus a small box blur of itself, RMS over luma) and reproduces equivalent synthetic grain on the target via the existing grain_look engine. Grain SIZE is not estimated from the reference, only amplitude -- an honest simplification, documented on the tool itself. Proven by test: a grainy reference measurably increases the target's pixel variance versus a flat reference, and strength 0 on a flat reference leaves the target bit-for-bit unchanged. 4 new TDD tests from the colour pass, 4 more from the two-clip-reference pass.] Honest remaining gaps: noise_hls (noise shaped in hue/saturation/lightness space rather than RGB) and noise_alpha (noise written to the alpha channel only) are both straightforward but weren't built this pass; motion/temporal denoise genuinely needs multi-frame access and the existing `denoise` effect's own doc comment already states it has none ("Single-frame: no temporal pass in the effect path") -- not attempted.

- [~] Particle Playground-style emitter with forces/gravity, particle trails, collisions, Foam / bubbles, Caustics / water surface, Card Dance, Shatter, Wave World, CC particle world/systems (fire, smoke, snow), cloth-ish cards, rigid-body sims (via integration with a physics crate), fluid/smoke sim, hair/rain on lens -- CORE IS THERE and tested: add_emitter already had forces/gravity/wind/drag, turbulence, attractor, vortex, trails (0..12), analytic + layer-silhouette colliders, and fire/smoke/snow/rain presets (pre-v7, now credited); simulate_physics gives real deterministic circle-proxy 2D rigid-body sims (gravity, drag, restitution, friction, walls, body-body collisions) baked to keyframes; shatter/shatter_cells transitions give a real glass-shard break. NEW this pass (TDD tests in particle_new_presets.rs): bubbles and foam presets, and a new `card` sprite kind (a flat rectangle whose half-width flips through a cosine phase of time+seed -- the 2D stand-in for a 3D card flip) with a card_dance preset built on it. HONEST GAPS needing a real physics/fluid engine, not attempted: Foam/Caustics/Wave World are AE's CC fluid-surface plugins (metaball/height-field fluid dynamics) -- our bubbles/foam presets are particle-based look-alikes, not a fluid solver; no cloth simulation (no particle-particle constraint solver); no polygon/concave rigid bodies or joints (simulate_physics is circles only); no true audio/cannon-driven Card Dance (ours is a particle preset, not a per-layer card-grid cannon); no grid-based fluid/smoke sim (the smoke preset is a particle look-alike); no lens-surface rain-droplet refraction effect.

- [~] brush strokes, cartoon, color emboss, roughen edges, scatter, texturize, CC glass/kaleida/mr. mercury/blobbylize/burn film, CC threshold RGB, dither, tile mosaic, trace/vector stylize, sketch/pencil, oil paint, anaglyph [x cartoon -- new effect kind (effects2.rs): posterizes colour into flat bands and darkens wherever the local luma gradient crosses a threshold, a real edge line (verified: a smooth gradient posterizes to a handful of distinct values; the exact boundary pixel is measurably darker with edge_strength=1 than with edge_strength=0, same posterization otherwise); x color_emboss -- new kind: the classic emboss relief added back onto the ORIGINAL colour (plain `emboss` goes to grey) -- verified a flat fill passes through unchanged (nothing to emboss) and a real hard edge visibly differs near vs far from it; x roughen_edges -- new kind: a jagged noise warp confined to the alpha-edge band via a local alpha-gradient mask, so a large flat shape's interior is untouched while its border gets ragged (verified); x scatter -- new kind: every pixel resampled from a random nearby offset, independent per pixel -- verified it raises local pixel-to-pixel variation on a smooth gradient (not a blur, which would do the opposite); x texturize -- new kind, procedural variant: multiplies by a synthetic fbm bump pattern, verified it introduces real variance on a flat fill. Honest scope note: AE's Texturize can also pull its bump from ANOTHER layer -- that two-clip-reference mode was not built (the single-clip procedural version was, this pass's effort went to the dedicated two-clip arithmetic line instead); x cc_glass -- new kind: refracts through a procedural fbm bump field via its own gradient, verified it visibly bends a hard edge in more than a few pixels (a real lens-like distortion, not a blur); x cc_kaleida -- new kind: true N-wedge radial mirror around a centre point. Debugging note: the first test design moved the test square via `set_property` and found 0/6 expected mirrored positions -- not a bug in the effect, but a known engine fact rediscovered the hard way (a clip's x/y offset applies AFTER its own effects run, so the effect never saw the square where the final composite would place it); redesigned the test to offset the kaleidoscope's own pivot instead, which confirmed the maths were right all along; verified 5-6 of 6 expected 60-degree-apart mirrored copies of an off-pivot square; x cc_threshold_rgb -- new kind: each channel independently hard-clipped at its own threshold, verified every output value is exactly 0 or 1; x dither -- new kind: real ordered Bayer-4x4 dithering down to N levels, verified the output only ever takes N distinct values on a smooth gradient; x tile_mosaic -- new kind: real NxN block colour averaging, verified every pixel within one block is identical and the next block over differs; x sketch_pencil -- new kind, the raster LOOK half of "sketch/pencil": the classic colour-dodge-of-an-inverted-blur technique, verified strength 0 is a no-op and strength 1 with color 0 desaturates toward true grey; x oil_paint -- new kind: a real windowed most-common-luma-bucket filter (not a blur), verified it measurably suppresses small-scale grain noise while a plain blur would only soften it; x anaglyph -- new kind: red channel from one horizontal shift, green+blue from the opposite shift, an honest monocular approximation of stereo separation (no real depth map) -- verified red and green visibly disagree right at a shifted edge; x cc_burn_film -- new kind: an animated fbm-noise-edged burn front eating the layer away as `progress` advances, with an ember glow band (`a_front*(1-a_front)`, peaking exactly at the front) -- verified progress 0 is untouched and progress 1 drops the layer to the canvas's own black background (checked via RGB, since the final composited frame's alpha is always fully opaque, a previously-documented engine fact, not a new one). 18 new TDD tests, two of which caught real test-authoring mistakes before they became false confidence: cc_kaleida's set_property-timing gotcha above, and cc_burn_film's first draft reading alpha instead of RGB to detect the burn.] Honest remaining gaps, not attempted this pass: brush_strokes (an oriented-smear painterly stroke effect); CC Mr. Mercury and CC Blobbylize (genuine particle/metaball simulations, materially harder than everything else on this line); "trace/vector stylize" as literal VECTOR PATH output (this is a raster pixel engine end to end -- sketch_pencil covers the visual LOOK half of that item, not true vector extraction, and no vector-path tool was faked to claim otherwise).

- [x] numbers, timecode (source timecode burn-in) [x new `burn_in_timecode` tool (ext/delivery_extras.rs): one dynamic-text clip over a range, resolved to the exact running SMPTE HH:MM:SS:FF (non-drop, the pre-existing `{tc:S}` render-time token) or HH:MM:SS;FF (drop-frame, a new `{tcd:S}` token added to render.rs' dyn_text, built on `timecode::label` -- the new seconds->label inverse of the pre-existing `timecode::seconds` parser, round-trip tested) at every frame; no per-frame clip, no range-length cap. Backing plate, position (corner/center), size/colour/font are one call. Numbers: `add_counter` (hud.rs) already covers an animated numeric readout, credited pre-existing, not rebuilt.]

- [~] block dissolve, card wipe, gradient wipe, iris wipe, linear wipe, radial wipe, venetian blinds, CC glass wipe, CC grid wipe, CC image wipe, CC jaws, CC light wipe, CC line sweep, CC radial scale wipe, CC scale wipe, CC twister, CC warpo [x block dissolve -- new `block_dissolve` transition kind (transitions.rs): discrete rectangular blocks pop to the incoming frame in a per-block random order, distinct from the existing per-pixel `noise_dissolve`; x card wipe -- new `card_wipe` transition kind: columns are dealt left to right with a brief per-column horizontal squeeze standing in for AE's 3D card turn (no real 3D geometry, an honest 2D approximation); x gradient wipe and x CC image wipe -- both already covered by the pre-existing `add_push_wipe` tool's `gradient_wipe` kind (a linear/radial/angular gradient OR a clip's own luma drives the reveal via `luma_clip_id`, which is exactly what AE's Image Wipe is); x iris wipe [`iris`], x linear wipe [`wipe`/`wipe_left`/`wipe_right`/`wipe_up`/`wipe_down`], x radial wipe [`clock`, a sweeping-hand radial reveal], x venetian blinds [`blinds`] all pre-existed and are being credited here for the first time. 2 new TDD tests confirm block_dissolve shows a real patchwork of both colours mid-transition (not a clean edge) and card_wipe reveals left columns before right ones -- written against a two-track (bottom clip always visible, top clip carries `transition_in`) scene, the pattern this engine's transitions actually need: same-track sequential clips don't give a true outgoing/incoming blend here, confirmed by probing even the pre-existing `crossfade` kind on adjacent non-overlapping clips (it fades from black, not from the prior clip, since nothing else is live at that render instant). Honest remaining gaps: CC glass wipe (refraction-based reveal), CC jaws (a specific zigzag shape), CC line sweep/radial scale wipe/scale wipe/twister/warpo -- each a narrow re-skin of concepts `zoom`/`spin`/`diagonal`/`clock` already cover under different marketing names from legacy AE CC plugins, not distinct algorithms, and not built as separate kinds this pass; CC light wipe (closest existing cousin: `light_leak`, a different effect, not rebuilt as a wipe variant).]

- [~] Cineon converter, color profile converter, grow bounds, HDR compander, apply color LUT [x as lut], tone mapping, compositing for log [x Cineon converter -- new `cineon_log` effect kind (color.rs): the Kodak Cineon printing-density log<->linear curve (10-bit black/white code points, density-per-code gamma), distinct from the LogC3/S-Log3 curves `color_space` already covered. Exactly invertible (decode then encode round-trips), the white-point code decodes to linear 1.0 by construction, and it's monotonic -- all three proven by test; x HDR compander -- new `hdr_compander` effect kind: a reversible mu-law-style curve for packing wide-range linear scene values into a bounded range and back, a data-packing round trip (not a perceptual tonemap, which is intentionally lossy -- see `tonemap` on the same line). Proven invertible including negative values; x color profile converter -- credited to the pre-existing `color_space` (linear/sRGB/Rec709/ACEScct/LogC3/S-Log3 transfer conversion) and `color_managed` (full gamut+transfer with exposure/tone-map/gamut-compress) effects, both already tested elsewhere (tests/color_pipeline.rs and friends); x tone mapping -- credited to the pre-existing `tonemap` effect (color.rs: ACES filmic fit, Reinhard, Hable filmic, exposure, optional sRGB encode) which had zero functional tests anywhere in the suite before this pass despite being fully implemented -- verified here for the first time (Reinhard compresses 9.0 to exactly 0.9 as x/(1+x) predicts, zero stays zero on every mode, a realistic 20x-over-white HDR value compresses well under its own linear value on every mode); x compositing for log -- credited to the pre-existing project-level `color_pipeline` (color/pipeline.rs), which already composites in float linear working space when enabled with an explicit input_transfer naming the source's log curve, so footage is decoded to linear before any blend math runs; ~ grow bounds -- honest gap, but not a missing capability: AE's Grow Bounds exists to stop a precomp's own (smaller) working buffer from clipping a blur/glow near its edge. This engine has no such edge to grow -- every clip, including a Precomp, is allocated at the full project canvas size regardless of source type (render.rs source_frame_h: `Frame::new(p.width, p.height)` runs for every Source variant, and a Precomp's own sub-project renders at the SAME width/height as its parent, never smaller), so there is no intermediate buffer boundary for a blur/glow to clip against in the first place. Proven by test: a glow on a clip inside a group_clips precomp, reaching all the way to the precomp's own edge, survives an outer blur unclipped. No dedicated tool was added for this, since there is nothing for it to do.]

### Proof level

- `denoise` effect (spatial, edge-aware, Rec709 Y'CbCr, alpha untouched, amount 0 = identity; BETA, from the DENOISE workstream, 11 module tests): 960x540 real frame + Gaussian noise sigma 12 codes: noisy 27.52 dB; denoise r2 33.80 dB (0.60 s incl. render); r3 luma .8 chroma .9 36.52 dB (0.74 s); OpenCV bilateral 36.28 dB (0.04 s), OpenCV NLM 32.71 dB (0.65 s). One soft macro frame, one noise model: a tie with a bilateral filter, not a claim of beating a production denoiser. No temporal pass in the effect path (the module supports neighbours; effects see one frame). examples/denoise_proof.py.

## Export and delivery

### What it does

#### Delivery
- [x] mp4 export (parallel, streaming), gif, audio export, still frame

### Parameters and controls

Native OCIO view video (explicit opt-in): `set_ocio_view_delivery` stores config,
source space, display/view, scene-linear input gamut, and actual output
gamut/transfer declaration. `export_ocio_view_video` applies the native OCIO
2.5.1 forward display processor including looks, bypasses project output, then
uses strict HEVC10/FFV1 16 delivery tags. No guessing from view names. Selection
must match the current working gamut and does not affect ordinary export.
Caller declares source gamut/white and view output encoding; tags do not prove
these declarations. Existing mastering metadata is NOT inherited. No native
HDR mastering certification. Integer outputs clip 0..1 and HEVC uses YUV420.

### Limits

- [x] export presets (YouTube, TikTok, ProRes, DNxHR, image sequence, WebM/VP9, AV1, HEVC), render queue, background render, in/out range export, alpha export, burn-in timecode, watermark, chapter markers [x 7 new export_preset kinds this pass -- youtube (.mp4 H.264 High CRF16 slow, native resolution, never resizes unlike web-1080), tiktok (.mp4 H.264 CRF20, always letterboxed/pillarboxed to exactly 1080x1920), prores422 (.mov ProRes 422 HQ yuv422p10le, has audio), dnxhr (.mov DNxHR HQ yuv422p, errors clearly under ffmpeg's real 256x120 floor instead of a cryptic encoder failure), webm-vp9 (.webm libvpx-vp9, video-only -- WebM doesn't take this engine's AAC mux and no Opus path was built this pass), hevc (.mp4 libx265 tagged hvc1 for Apple/QuickTime, +faststart) and av1 (.mp4 libsvtav1, +faststart); youtube/tiktok/hevc/av1 share a new `remux_faststart` helper (the audio mux rewrites the file and a prior +faststart flag doesn't survive that, same fix web-1080 already had inline). 10 TDD tests, ffprobe-verified per preset (codec, pix_fmt, resolution, moov-before-mdat where +faststart applies, muxed audio where promised, webm-vp9's documented absence of one). credit: image sequence already covered by export_alpha's png format (RGBA sequence, %05d pattern); render queue and background render already shipped pre-v7 as export_job_start/job_status/cancel_job/resume_job (chunked, resumable, crash-safe, dependency-hashed reuse -- a real background render queue, just not named one); in/out range export already covered by export's own start/end and every preset's start/end. New this pass, genuinely absent before: burn_in_timecode (ext/delivery_extras.rs) -- one labelled text clip per frame holding that frame's exact SMPTE label (timecode::label, the new inverse of the existing timecode::seconds parser, round-trip-tested), same per-frame-clip convention add_counter (hud.rs) already uses for a rolling number; capped at 6000 frames/call (document the cap, call again for longer), drop-frame needs 30000/1001 or 60000/1001 fps same as the parser. add_watermark -- text or logo-image pinned to a corner/center; for an image, `size` is the exact resulting fraction of canvas width regardless of the logo's own aspect or the canvas's (the engine has no width/height clip prop, only a uniform `scale` applied after `fit:contain`, so the needed scale is solved for rather than promising a size the engine can't address directly -- verified by measuring the actual rendered pixel bbox on a non-square-canvas/square-logo case, which would silently undershoot a naive fraction-of-fitted-width approach). mux_chapters -- writes the timeline's own `add_marker` markers into an already-exported file as real container chapters (ffmpeg metadata remux, stream copy, picture/audio bytes untouched), ffprobe-verified chapter count/titles/start times. 20 TDD tests total across export_presets.rs + delivery_extras.rs.]

### Proof level

#### Explicit ordinary-video delivery (COLOR132)
Ordinary H264 export now converts Rec709 RGB to BT709 limited-range YUV and writes matching primaries/matrix/range plus the exact linear/sRGB/BT709 transfer tag. Disabled projects retain display-sRGB rendering; managed ordinary export rejects other gamuts/transfers instead of leaving players to guess. BT601 delivery is unsupported. High-bit HLG, like PQ, requires Rec2020. Migration: legacy MP4 delivery pixels changed from implicit conversion; re-export older MP4s for the explicit contract. Project rendering/defaults are unchanged.

#### Export: start/end range + identity-transform fast path - BETA, measured
- `export` takes optional `start`/`end` seconds. Frame k renders at its absolute time, so range frames equal the same frames of a full export (verified at crf 0 by decoded frame md5: 30/30 frames identical) and audio is sliced from the same mix.
- Layers with an identity transform no longer copy the whole 1080p float frame (33 MB) before blending. Output is unchanged: a full export of the benchmark project is frame-md5-identical to the pre-change export.
- Measured (2 vCPU, 1080p30, 4 s project: image background with vignette+grain and 6 text layers, crf 0): full export 42.2 s to 32.0 s (1.32x); a 1 s range export takes 8.6 s.
- Per-frame cost is still dominated by full-frame float layers and the x264 encode; a static-layer cache is designed but NOT built (needs strict invalidation for time-varying effects and hash-verification against uncached output).

High-bit delivery contract (COLOR): disabled-project exports now explicitly use
Rec709/sRGB, BT709 limited-range YUV for HEVC10 or full-range identity RGB for
FFV1 16. This fixes the formerly untagged/default-matrix disabled HEVC path.
Managed high-bit exports accept Rec709/Rec2020 with exact linear/sRGB/BT709
transfer tags, plus Rec2020 PQ/HLG. Other gamuts and gamma2.4/log/ACEScct output
that previously exported untagged or partly tagged now ERROR before encoding.
Fix: select a supported delivery gamut/transfer, or use float EXR/TIFF for working
or log interchange. This is signal labeling/precision, not HDR display calibration.

- export_alpha (BETA): timeline or range with transparency as ProRes 4444 .mov (10-bit 4:4:4:4), WebM VP9 + alpha, or RGBA PNG sequence. Verified: all three decode clean, PNG sequence has alpha 0 at empty pixels and opaque text pixels, ffprobe reports prores yuva444p. Straight alpha, no audio, managed color pipeline must be off (errors otherwise). Named export presets are not done.

## Performance

### What it does

#### D. Infrastructure / performance
- [x] windowed streaming decode (24-frame window), eviction, parallel export, preview cache
- [~] CPU f32 renderer (about 2 fps at 720p for heavy stacks)
- [~] windowed tracking/stabilization shipped; pending: proxy-based preview and cached low-res scrubbing

### Parameters and controls

Use `tools/list` / `list_effects` for live schemas. Read the source parameter names, ranges and defaults alongside their evidence and limits in this module.

### Limits

- [~] wgpu pipeline: WGSL effect kernels, GPU compositing, frame textures in VRAM, async readback, hardware encode via ffmpeg/NVENC -- NEW this pass (src/gpu2d.rs, feature `gpu2d`, default OFF so the shipping binary is unchanged): a real opt-in GPU path running 2 effects (gaussian blur, a 4x5 AE-style colour matrix) as genuine WGSL compute shaders through wgpu, proven running on this box's lavapipe software-Vulkan adapter (llvmpipe/Mesa, confirmed live via AdapterInfo in tests/gpu2d_tolerance.rs, not just compiled). Registered as 3 MCP tools (gpu2d_status, gpu_gaussian_blur_frame, gpu_color_matrix_frame) with TDD tests at 3 levels: unit tests on the CPU reference math, an integration test running the actual GPU kernels and asserting the result matches the CPU reference within float-rounding tolerance (measured max_abs_diff = 0.0 on this box's adapter) with a clean non-failing skip when no adapter opens, and MCP-dispatch tests proving the tools are real registry entries that fall back to CPU automatically and never mutate the project. The CPU path (gpu2d::cpu_gaussian_blur / cpu_color_matrix) is untouched/unused by anything else in the engine and the default (non-gpu2d) binary behaves byte-identically to before -- goldens did not move. HONEST GAPS, not attempted: GPU compositing of the main render/effect pipeline (this is a standalone preview/diagnostic path, not wired into add_effect or render_frame's default path -- doing that safely needs a frame-texture/VRAM residency redesign and a full golden re-verification, out of scope for an opt-in add); frame textures staying resident in VRAM across frames (each call opens a fresh device/uploads/reads back -- no persistent GPU session); async readback (the readback here blocks on device.poll, which is fine for a diagnostic tool but not for a render loop); hardware encode via ffmpeg/NVENC (no NVENC hardware exists on this box, and ffmpeg's encode path is unrelated to this compute path); only 2 of the many CPU effects have a GPU counterpart. No hardware GPU exists on this box at all (lavapipe is Mesa's CPU-backed software Vulkan implementation), so no real hardware-acceleration speedup claim is made or measurable here -- the point proven is that the wgpu/WGSL plumbing is real, builds, and runs correct compute on whatever Vulkan device is present.
- [~] u8/half-res fast path for glow/blur, SIMD, tile-based rendering, dirty-region cache [x half-res fast path for glow mode 1 (deep glow) -- new `half_res` param (default 0, byte-identical to before this param existed), src/glow.rs: box-downsamples the bright-pass 2x, runs each Gaussian component at half resolution with half sigma, bilinear-upsamples the sum once before compositing. Measured (gate profile, glow mode 1, radius 24, 6 iterations): 1.3-1.9x faster depending on resolution (960x540 and 1920x1080) -- real but modest, since bright-pass extraction and final compositing still run at full res. 5 TDD tests (byte-identical at half_res:0, visibly different and still glowing at half_res:1, odd dimensions, keyframeable) + 1 #[ignore] benchmark]; [x dirty-region, in the temporal sense: export_manifest/explain_invalidation (pre-v7) already does dependency-hashed per-chunk invalidation -- "explain exactly which chunks went stale and why" -- functionally the dirty-region concept applied to export chunks rather than spatial pixels]. Honest gaps, not attempted this pass: u8 (int8) math path, blur (only glow got the half-res treatment), SIMD (the scalar per-pixel loops are golden-pinned bit-exact; vectorizing without disturbing floating-point rounding needs per-platform verification beyond this pass), tile-based rendering (frames render whole, not in spatial tiles), and a spatial (per-pixel) dirty-region cache for interactive scrubbing -- there is no GUI canvas yet for one to serve.
- [x] expression evaluator (small interpreter) shared by all params [x `set_expression_script`/`clear_expression_script`/`list_expression_scripts`, FEATURES.md L210/L220, ext/expr_interp_v7.rs: arithmetic/comparisons/ternary/functions, applies to any clip's any property or effect param]
- [~] project format v2 with compositions, versioned migrations [x compositions: nested compositions already existed pre-v7 (Source::Precomp, its own tracks on a local clock, FEATURES.md neighbourhood of L473) -- not new this pass; [x migrations: new (src/migrations.rs) -- schema_version bumped 1 -> 2, a real versioned-migration registry/engine wired into load_project (runs on the raw JSON before it becomes a Project, in order, from the file's own kimo.schema_version header up to this build's), one real migration proven on it (v1 files with blank track names get readable "Track N" defaults -- the first schema change that genuinely needed a transform rather than a #[serde(default)], since every prior addition has been purely additive). 9 TDD tests (migrations.rs unit tests + tests/project_format_v2.rs: a real v1 fixture, an already-current file left untouched, a too-new file refused not migrated, a migrated file re-saving at the current schema_version). Honest [~]: only one migration has ever been needed, so the registry has exactly one entry -- it is real infrastructure, not yet battle-tested by a second schema bump.
- [~] MCP: batch tool (apply many ops atomically) [x `batch`, already shipped pre-v7: atomic/dry_run/foreach/$calc/$ref]; describe_project summary for LLM context [x `timeline_summary`, already shipped -- "compact view of the edit... use instead of get_project"]; templates: create_title [x `add_title`'s style presets], create_lower_third [x `add_title` style=lower_third, and the separate `lower_third` primitive], create_trailer_titles [x `add_title` styles impact/hook_box/kinetic cover this -- no dedicated multi-card sequencer built]; make_montage_from_folder is new this pass (ext/montage_from_folder_v7.rs): scans a folder (not recursive) for recognized video/still files, imports each in filename order, lays them back to back on a track, clip_duration caps a longer clip but never stretches a shorter one, optional crossfade overlap via add_transition. 9 TDD tests. Honest [~]: create_trailer_titles has no dedicated tool, just existing title styles.
- [x] GUI, credited + new. Pre-existing, uncredited until this pass: **Kimo Spectator** (crates/kimo-gui, Tauri v2 desktop app, its own deliberately-separate Cargo workspace to keep the Tauri tree out of the gate build) -- a live timeline + preview-frame view streamed over loopback TCP while an agent edits via MCP tool calls, READ-ONLY BY CONSTRUCTION (no IPC commands exist at all; it can only watch). That covers 'watch the engine edit itself' but not 'a human drives the engine by hand', and being a Tauri desktop bundle it needs a system webview and is not buildable/testable headless on this box. NEW this pass, to close that gap: `kimo gui [--project p.kimo] [--port 4567] [--bind 127.0.0.1]` (crates/kimo-cli/src/gui.rs + gui_index.html) -- a minimal LOCAL WEB UI, std::net only, no new dependency, over the exact same registry/dispatch the CLI and MCP server already use (run_tool, no second dispatch path), that a human (or anything with a browser) can actually drive. Has the three pieces the brief asked for: a timeline view (tracks/clips with start/duration/end/name/role/source_kind, GET /api/timeline, drawn as a ruler of bars); a preview frame rendered by the real engine, not a mock (GET /preview.png?t=<seconds> calls the same render_png used by render_frame/export -- proven in a test by checking real PNG magic bytes on the response, and by hand against a live server: curl'd a genuine 64x36 PNG back); and a tool-invocation panel (GET /api/registry + /api/tool?name= list/describe every tool, POST /api/call runs one through run_tool exactly as `kimo call` does -- proven in a test that an add_track call through this endpoint actually adds a track). Headless-testable as the brief asked: handle_request(session, method, path, query, body) -> Response is a pure function with no socket in it, so all 12 tests (view-model shape, full registry listing, every route incl. the real rendered PNG and a real mutating tool call, a tool-level error returned as data not an HTTP error, malformed-body rejection, 404/405) run with no port ever bound; only the outer accept-loop touches a real TcpListener, smoke-tested by hand (started the server against a real project, curl'd `/`, `/api/timeline`, `/preview.png` and `/api/call`, killed it, cleaned up). Honest gaps: no JS build step or framework (one inline HTML/CSS/vanilla-JS page, deliberately small), no scrubbing/playback (one still frame per request, no video element), no drag-to-trim/drag-to-move on the timeline (read-only view, not yet an editable one), no auth (loopback tool, not meant to be exposed), no multi-client session sharing (one Session behind one Mutex), and no desktop packaging (it is a dev-server command, not an installable app like Spectator is). **v7.3 update (honest flip [~] -> [x]):** the editing-completeness gaps named above are now closed -- scrubbing/playback (togglePlay/scrubTo) and drag-to-move/drag-to-trim landed in an earlier pass before this release; THIS release added keyboard shortcuts (space/J/K/L/Delete/S/undo/redo), track add/remove, marker display + add, timeline snapping (to clip edges/markers/0), zoom (bounds-clamped, in/out/reset), export with a live progress bar (wired onto the existing chunked export/ETA system), and project open/save buttons. What remains unclosed is deliberate scope, not missing editing capability: no JS build step/framework (one inline vanilla-JS page, on purpose), no auth (a loopback dev tool, not meant to be exposed), no multi-client session sharing (one Session behind one Mutex), and no desktop packaging (Spectator already is the installable app; `kimo gui` is a dev-server command). On that basis this row is now [x]: a human can actually run a full basic edit -- cut, move, trim, mark, snap, zoom, save/open, export with visible progress -- through a browser alone, keyboard included.

### Proof level

#### CPU parallelism (std threads, no new dependency) - BETA, measured
`kimo_core::par` splits work by disjoint rows/column strips across `available_parallelism()` threads (override: `KIMO_THREADS`). Every output element is computed by one thread with the same arithmetic as before, so results are bit-identical (FNV hash of all output floats equal to the pre-change build).
Measured on this 2-vCPU box (examples/bench_cpu.rs, examples/bench2.rs, old = build of e9c4762):
- box blur x3 passes, 1080p: 0.53-0.60 s old, 0.12-0.13 s on 1 thread (4.1-4.6x: no per-pixel allocs, cache-friendly vertical pass), 0.06-0.07 s on 2 threads (8.5-9.1x).
- flow_blur 720p: 0.334 s -> 0.201 s (1.7x); optical flow 720p q5: 0.098 -> 0.085 s (1.15x, solver dominated by serial resampling); blur_cov sigma 12: 0.017 -> 0.006 s (2.8x).
- denoise 720p (spatial+1 temporal neighbour): 0.444 s -> 0.381 s on 1 thread (precomputed spatial weights), 0.216 s on 2 threads (2.1x). Temporal pass still serial.
Limit: speedup is capped by core count (2 here). This is CPU only; no GPU code exists yet.

#### Parallel color paths (COLOR) - BETA, measured
Disjoint-pixel threading (shared `par` module) on four color paths: immutable OCIO apply, LUT, linear grade, basic per-pixel ops. COLOR's median 1080p reruns, 8 workers: OCIO 1170 to 608 ms (1.93x), LUT 132 to 57 ms (2.31x), linear grade 145 to 87 ms (1.66x), saturation 6.72 to 5.21 ms (1.29x). All four paths hash-identical before and after at 1 and 8 workers. Extreme inputs fall back to the serial path (preserves old partial-write behavior). Our box has 2 vCPUs, so expect at most about 2x here. Reviewed here: build, color_parallel test (1/2/4/8/16 threads), fuzz 168 / 6516 / 0 failures; COLOR reports full native suite 326/326.

Static text-layer cache (export only, BETA): text clips with constant text (no `{}` tokens), no text animators/fx, no keyframes or expressions on reveal or effect params, no mask/echo/posterize, and only pure-pixel effects (exposure, contrast, saturation, temperature, tint, color_wheels, curves, teal_orange, vignette, blur, glow, invert, sharpen, posterize, sepia, outline, outer_glow, long_shadow, choker, feather_alpha, silhouette_fill) are rendered once per export and rebuilt by pasting a bit-exact non-zero crop (grain and anything seeded or time-varying is never cached; single-frame renders never use it). Measured, same 4 s 1080p30 benchmark: full export 42.2 s (before this session) to 29.9 s (1.41x); output frame-md5-identical to the pre-change export (tests/export_bench.py). Gain is modest because source image copy, background effects and the x264 encode dominate; media/still/solid layers are not cached yet.

- Layer transform parallelism (export/render speed, bit-identical): the scale/rotate/translate resampler splits rows over the `par` threads. Measured on 2 vCPU, 8 scaled+rotated 1280x720 layers: 0.942 s to 0.558 s per frame (1.69x), output PNG md5 identical for KIMO_THREADS=1 and 2. Real film profile showed transform as the dominant stage.

Film finishing speed (COLOR): scoped f32 sRGB transfer math for film_finish
only, decode |input|<=1, encode |linear|<=16; f64 reference fallback outside.
Bloom extraction and main pass share one decoded pixel; coordinates and halo
luma are hoisted. Print_stock hoists stock curve constants and combines two
powers into one exp, still f64. Managed transfer math/defaults unchanged.
Measured, not mathematical all-input bounds: million-sample signed transfer
sweep max decode error2.060e-7, encode5.649e-7; finish extreme signed[-4,4]
parameter grid max7.153e-7, real1080p max2.384e-7. Print_stock real frame and
stock/pivot/light grid[-16,16] are bit-identical after f32 output rounding.
Release isolated real1080p median3 runs: finish1.37-1.47x, print_stock3.25-3.60x faster on
this host. Zero-strength identity, alpha, seed determinism unchanged. Tiny
non-bit-identical finish changes are opt-in effect math, not a global pipeline
speed mode. These samples do not certify all finite inputs or export speed.

- Review cleanups: text.rs non-snake-case warnings removed (pure rename, prompt box output unchanged); add_speed_lines default ray count 70 to 48 (gentler; pass count to restore). Detection-box default softening not done.

- Export presets (BETA): inspect_export_preset and export_preset with web-1080 (.mp4 H.264 CRF18 medium, +faststart, AAC 192k, Rec709 only, fit 1920x1080/1080x1920, no upscale, even dims, lanczos scaling in the encoder), master (.mkv FFV1 16-bit RGB lossless via export_high_bit ffv1_16, whole timeline) and alpha-prores (.mov ProRes 4444 yuva444p10le, transparent canvas). Extension enforced. Verified: web-1080 2560x1440 gives 1920x1080 h264 yuv420p tv bt709, faststart (moov first), decodes clean; fit rules unit-tested (odd, portrait, no upscale); ordinary export frame md5s identical before/after the shared-loop refactor. Limits: alpha-prores has no audio yet and needs the managed color pipeline off; web-1080 audio mux goes through the existing AAC path and moov is re-faststarted by a copy remux; not tested: COLOR's ffprobe tag matrix for alpha, master exact-u16 proof (reuses existing export_high).
- Alpha export BUG FIXED: the first export_alpha frames had premultiplied color at anti-aliased edges (edge pixels about half brightness, measured 122 vs 255 on orange text). The transparent compositor keeps premultiplied rgb; alpha deliveries now un-premultiply. Re-measured: edge rgb 255,128,26 equals the opaque text color for PNG and ProRes. Other blend modes on a fully transparent canvas are not alpha-correct and are untested.

## Sandbox

### What it does

#### Sandbox layer (guard.rs, built into every tool call)
- [x] V1: argument sanitizer (finite numbers, magnitude +-1e8, array/string size, nesting), project limits checked after every mutation with automatic rollback (canvas pixels, fps, clip count, timeline duration), memory budget via counting allocator (default 55% of RAM or cgroup limit; Frame allocations and the render loop fail the call instead of OOMing; hard backstop refuses allocations far above budget), cooperative cancel plus wall-clock time limit per call (default 600 s, export 7200 s) with a last-resort clean server exit if a call cannot be stopped, panics caught with project rollback. Env overrides: KIMO_MEM_MB, KIMO_TIMEOUT_S, KIMO_EXPORT_TIMEOUT_S, KIMO_MAX_PIXELS, KIMO_MAX_FPS, KIMO_MAX_CLIPS, KIMO_MAX_DURATION_S, KIMO_MAX_NUM. Test: tests/sandbox_test.py.

### Parameters and controls

Use `tools/list` / `list_effects` for live schemas. Read the source parameter names, ranges and defaults alongside their evidence and limits in this module.

### Limits

- Gaps (honest): in-process, not OS-level isolation (no seccomp/namespaces/cgroups; a native-code bug that corrupts memory still kills the server); memory accounting covers Rust heap only, not ffmpeg child processes or GPU memory; cancellation is checked per clip/frame and inside several color, scope and process-wait loops, but not every legacy effect or synchronous file operation is interruptible; ffmpeg children get no separate CPU/memory limits yet; file paths are not jailed (tools can read/write any path the user can).

### Proof level

Use the quality-tier audit and test notes in the BETA appendix; checklist marks are not a professional-tool parity claim.

## BETA and honest limits

The audit below preserves checkpoint wording. Read it alongside the module records, including their later additions and corrections, rather than treating any earlier pending item or limitation as a new status update. BETA, BETA+, LIMITED-USE, V1 and V2 labels remain exactly as supplied; no claim is promoted by this reorganization.

### Target, sources and design rule

Target: every useful Premiere Pro editing tool and the full After Effects effect range, each callable as an MCP tool.

Sources: Adobe AE effect list and category pages (helpx.adobe.com/after-effects/desktop/apply-effects-and-animation-presets/...), AE help index (compositing, expressions, 3D, tracking, time remap), Premiere help (nested sequences, color, proxies), CapCut feature pages (see README).

Design rule: one registry in `crates/kimo-core/src/commands.rs` + `edit.rs`; MCP lists straight from it. New feature = new command or effect catalog row, so AI can discover it with `list_effects` / `tools/list`.

### Original checklist grouping

#### A. Premiere Pro: editing

### E. Quality tiers (honest audit)
User bar: every feature best in class. Tiers: **PRO** = matches or beats the pro tool, **V2** = correct maths, missing refinements, **V1** = works, simple maths. Nothing is PRO until verified on real footage against the reference tool's behavior.

| Area | Tier | What is missing for PRO |
|---|---|---|
| Timeline edit tools (ripple/roll/slip/slide/lift/extract) | V2 | linked A/V, track targeting, sync locks, multi-track ripple |
| Color primaries / log wheels / curves / hue curves | V2 | float pipeline is f32 per-pixel and display-referred by default; opt-in scene-linear working space, 2..32-point free curves and explicit gamut mapping shipped; no calibrated camera/display/film pipeline |
| Color space transforms (ACEScct, LogC3, S-Log3, sRGB, Rec.709) | V2 | legacy color_space is transfer-only; color_managed supports primaries/log gamuts. No certified RRT/ODT or production OCIO config import; pinned reference is opt-in |
| Qualifier (HSL key) + Power window | V2 | bezier/per-effect masks and guided qualifier cleanup shipped; mask expansion/choke belongs to the mask stack. No mask tracking binding or universal temporal denoise |
| Scopes (waveform, parade, vectorscope, histogram) | V2 | quantitative working-linear/caller-declared-nits analysis shipped; PNG scopes remain SDR display-domain. No calibrated IRE/HDR display scope or CIE chromaticity |
| Film print, halation, bleach bypass | V1 | no measured print-stock LUT/density curves (Kodak 2383 etc.), halation is box-blur based |
| White balance picker, color match | V1 | full-covariance sRGB/D65 Lab optimal transport with sample regions shipped; no semantic match, mask-selected matching or camera calibration |
| Chroma key (old `chroma_key`) | V1 | superseded by `keyer` |
| Keyer (`keyer`: screen gain+balance, clip black/white, core shrink, edge soften, despill + luma replace, edge unmix, matte view; matte_model 1 linear colour difference with clean plate (ref_clip/aux_path), garbage/core mattes, despill average/double/limit/min + bias + replace colour, status view) | V2 | motion-adaptive matte, hair-detail refinement (guided filter), tested on synthetic screen only |
| Luma key | V1 | |
| Stabilization (two-pass: 24-patch robust similarity fit with outlier rejection, Gaussian path smoothing, lock mode, adaptive zoom, translation+rotation+scale, windowed decode) | V2 | no rolling-shutter correction, no 3D/perspective (homography) model, no inpainting of borders instead of zoom, no tripod/pan-aware modes. Measured: frame-to-frame jitter 9.8 -> 3.5 on a synthetic shaky clip |
| Tracking (ring of 9 patches, translation + rotation + scale, windowed decode, refit from frame 0 each frame) | V2 | this row describes the point/ring tracker; separate planar tracker has homography/reacquisition with limited-use tiers below. No per-patch confidence UI |
| Motion blur (`set_motion_blur`: shutter-angle multi-sample of x/y/scale/rotation, 4-16 samples) | V2 | no per-effect/param blur, forward source optical-flow blur is separate and has occlusion/fast-motion gaps; adjustment layers/text animators not covered |
| Renderer | V1 | alpha-correct bilinear + 2x2..4x4 supersampling on transforms (done); still CPU f32, no mipmaps/Lanczos for large downscales, no GPU |
| Transitions | V2 | 31 kinds, 10 easings, soft angled wipes, clock, split, iris, noise dissolve, dips, light leak, film burn, glitch, whip pan, zoom blur. Missing: outgoing-clip push/slide-out, luma-matte wipes from image, GPU-shader transitions |
| Particles | V2 (emitter) | add_emitter: 14 presets (incl. dust, smoke, bubbles, foam, card_dance), forces/gravity/wind/drag, turbulence, attractor, vortex, floor bounce, analytic colliders (circle/rect/plane) + layer-silhouette colliders (static or `follow`), trails, fade_in, motion blur (shutter_deg), sprites soft/hard/mote/puff/streak/ring/star/spark/card (card = flat rectangle, flip driven by a time+seed cosine phase); simulate_physics gives circle-proxy 2D rigid-body (gravity/drag/restitution/friction/walls/body-body collisions) baked to keyframes; shatter/shatter_cells transitions give a glass-shard break. Still no fluid/smoke grid solver, no cloth, no polygon/joint rigid bodies, no true audio-reactive Card Dance cannon, no CC Foam/Caustics/Wave World fluid-surface sim |
| Text | V2 | kerned layout, word wrap, AE-style char/word/line animators with range selectors (6 shapes, keyframable) + 9 presets, stroke, blurred shadow, glow, gradient, rounded bg box. Missing: per-span rich text, unrestricted variable fonts, emoji; path text and font-weight control shipped |
| Masks | V2 | bezier path-shape keys and expansion shipped; no per-point feather, ML roto or automated mask tracking |
| Audio mixer | V1 | no EQ bands, no loudness normalize, no ducking |

- [x] HUD tools: add_detection_box, add_counter, add_prompt_box (V1)
- [x] shatter transition V2: radial + concentric spiderweb cracks that grow from an impact point, domain-warped irregular shards, impact flash and shockwave ring, shards of the outgoing frame tumble in true 3D perspective with refraction offset, edge glints and fade. Gaps: no depth-of-field blur on near shards, no secondary micro-fragment cascade, no sound. ("shatter_cells" keeps the old Voronoi reveal.)
- [x] 3D layer system V1 (three_d.rs): any layer becomes a perspective plane via rot_x, rot_y, rotation(z), pos_z, x, y, scale, backface; project camera via set_camera / set_camera_keyframe (cam_x/y/z, pan/tilt/roll, fov). Exact inverse perspective mapping with 2x2 supersampling. Shatter shards use the same Projector. Gaps: layers are composited in track order (no depth sort yet), no motion blur on 3D layers, no lights/shadows, no extruded 3D text, no mesh import.


### Tests
`python3 tests/tool_fuzz.py` reads the live MCP registry and exercises non-skipped tools with empty, null, wrong-type, negative, huge and real-id arguments, failing on crash, hang or malformed reply. The explicit skip set is export, export_gif, project_set, render_frame, export_audio, transcribe and download_stock, not every render/export tool. Call count varies with schemas; real-id coverage requires successful fixture setup.


### Fixes from repo issues #1-#7 (all verified)
- #1 add_clip fit: contain | cover | stretch (cover decodes at full res then center-crops).
- #2 shard DOF blur radius capped (10 px at 720) and dispersion fringe clamped, so defocus and glow no longer turn shards into fringed mosaics.
- #3 shard burst px units now scale with the SHORT edge/720 (portrait 1080x1920 gets 1.5x, not 2.67x).
- #4 export uses a work queue across cores (a slow shard frame no longer stalls the batch). Still CPU: real fix is a GPU path. Decode still serial between batches.
- #5 detect_beats / analyze: 11.6 ms resolution, onsets stamped at window centre, strongest-wins min_gap, bpm from autocorrelation. Test on a 140 BPM synthetic kick: first six onsets within 1-2 ms of truth, bpm 139.9.
- #6 add_counter hold default 0.
- #7 add_audio_clip (audio-only, never draws) and a lookahead peak limiter on export (ceiling -1.5 dBFS; tested: +80% gain input exports at -1.5 dBFS peak). Gap: sample-peak limiter, not oversampled true-peak.

- Detection box: default fill_opacity 0.08 to 0.05 (gentler) and the label tag width now uses measured text width (tag fits its text on the 720p check; the old 0.6 em guess could clip proportional fonts). Pass fill_opacity to restore.

- Alpha ProRes color fix (export_preset alpha-prores and export_alpha prores4444): they previously converted RGB to YUV with ffmpeg's default BT601 matrix and wrote no color tags (found by COLOR: Y12 2060 vs expected 2137). Now explicit RGB to BT709 limited-range conversion, colr atom written, and a ProRes bitstream metadata pass so ffprobe reports bt709 / iec61966-2-1 / bt709 / tv. Measured on RGB [0.2,0.6,230/255]: Y12 = 2136 (expected 2137, within 1 code). Input to the encoder is still RGBA8 (8-bit alpha and color precision, not the 16-bit pipe); the output is 10-bit ProRes 4444 (ffprobe reports a 12-bit pix_fmt, the decoder's container for it).

- Static shape cache (export only, bit-exact): constant shape layers (no keyframes/expressions, no mask/echo/posterize, effects from the same safe list as text) render once per export like static text. Verified frame-md5 identical on the film stand-in (10-12 s range). Measured effect: per-frame source-stage CPU 0.07 s to 0.026 s on that project; wall per frame 0.40 to 0.39 s (within noise), so this is a small win. Media/still layers are not cached.
Film-style response (explicit opt-in): film_soft_shoulder uses a per-channel
C1 exponential above knee, identity below; default knee0.75/peak1 maps1 to
0.90803014 and2 to0.99831551. Amount1 approaches peak without hard clipping;
partial amount is not bounded HDR delivery. RGB ratios/hue are not preserved.
film_luma_grain uses display-Rec709 luma and monotonically decreasing grain
occupancy: default shadow0.9/highlight0.08, falloff1.5. Independent seeded
amplitude is zero-mean before clipping, max0.025 default; output is NOT clipped.
Grain is monochrome and block-sized in output pixels, not a scanned film stock.
Generic add_effect exposes both; inspect_film_response exposes exact response.

On managed projects declare native OCIO effect-domain spaces explicitly for a
colorspace sandwich; do not invert an ACES display/view. For scene-linear
luminance/ray compression use existing luminance_shoulder. These new effects
may run after a real forward native display transform in an explicit finishing
workflow; ordinary export/nativeviewvideo is not automatically changed.
Measurements: 100001point shoulder sweep0..10 monotone/bounded, maxf32rounding
2.981e-8 vs f64curve on f32input. Grain512x512 occupancy at luma0.05/0.5/0.95
0.83850/0.36893/0.08911, within0.001 of declaredcurve; mean signednoise<1.44e-5.
Seed-repeat exact, transparent pixels/alpha/zeroamount exactidentity. Grain
sampling error is stochastic evidence, not an all-image occupancy guarantee.
No calibrated stock, ACES/look superiority or display certification claim.

## Gap #3: shape modifiers, layer styles, typed modifiers (all MCP)
- `add_modifier` / `list_modifiers` / `remove_modifier`: wiggle (per-layer derived seed), loop, bounce, overshoot on any property.
- `add_shape_modifier` / `remove_shape_modifier` / `list_shape_modifiers`: trim paths, repeater, offset paths, merge paths, gradient stroke (shape_* effects, keyframable).
- `set_layer_style` / `remove_layer_style` / `list_layer_styles`: inner shadow, bevel, stroke (inside/center/outside).
- Proof: `python3 tests/gap3_proof.py`.
Per-clip grading node DAG (explicit opt-in): grade nodes, normalized weighted
parallel RGB mixers and ordered opacity layer mixers. `input` is the rendered
clip after legacy effects, BEFORE clip masks/transforms. Serial graph matches
same ordered grades exactly. No graph = old path. Output selection is explicit;
parked nodes not rendered, all connections checked for cycles/missing IDs.
First-input alpha is retained; node masks blend RGB against first input. This
is a grading graph, NOT Fusion, shared/group/timeline grades or Resolve mixer
parity. Per-node mask/key coverage and imported tracker keyframes are supported;
tracking coordinates must come from an actual tracker, no node solver claimed.

MCP: inspect_grade_graph, set_grade_graph, add_node, connect_nodes,
disconnect_nodes, remove_grade_node, set_grade_output, set_grade_node,
set_node_param (full Prop animation/expression), set_node_mask,
grade_node_tracking and evaluate_grade_graph_samples (pixels/cache stats).
Complete node/graph JSON exposes strings/domain bindings, bypass, connection
order/weights, mask/path keys, parameters and cache budget; nothing GUI-only.

Node cache is evaluation-local: a shared dependency runs once per frame/eval,
Arc results evicted at last consumer. No persistent stale cross-frame cache.
Max64nodes/16inputs, default256MiB cached-frame budget configurable1..1024MiB;
over-budget fails transactionally, no quality downgrade. This budget measures
cached input/output frames, not effect/mask scratch or decode memory; global
memory guard remains active. Native OCIO per-effect strings work unchanged.
Managed stale-gamut/reference-white bindings checked before timeline render.
Tests: serial/real-frame exact parity, diamond single-evaluation/cache reuse,
parallel/layer weights, masks/keys, nativeOCIO parity, cycles/rollback/JSON
roundtrip, deterministic repeat and budget failures. No speed/hue/calibration
superiority claim.

## Gap #4: captions, audio effects, loudness, multicam (all MCP, BETA)
15 MCP tools: captions (add/list/style/edit/burn-in), audio (effects incl. Schroeder comb/allpass reverb, pan, loudness measure/normalize, ducking), multicam (sync/add/switch/list). Details and the tool list: docs/EDITORIAL_MCP.md. Measured: LUFS within 0.15 LU of FFmpeg loudnorm; audio sync recovers a 350 ms offset on real PCM.
Honest limits: sync is 5 ms RMS correlation, not sample-accurate; timecode labels are non-drop only; multicam uses the common overlap; pan is stereo balance; loudness uses sample peak, not true peak.

## Shared light and material model (BETA, all MCP)
Project-level lights (directional/point/spot, colour, intensity, falloff, cone, time keys) and named PBR materials (albedo, roughness, metal, ior, clearcoat, emissive; presets plastic, rubber, brushed_metal, chrome, gold, glass, concrete, wet_stone, car_paint, skin). Tools: set_light, remove_light, list_lights, set_material, remove_material, list_materials, set_relight, preview_material. First consumer: the `relight` effect, which builds a normal field from alpha and/or luma height and shades with GGX + Smith + Schlick, a clearcoat lobe and soft height-field self shadows, on any layer. Proof: tests/lighting_proof.py (moving the light moves the highlight, bad inputs rejected). Default render paths are unchanged (film frame md5s identical).
Honest limits: 2D layers only (a height-field relief, not real geometry); no environment/IBL reflections, so chrome and glass show only light highlights; no light-to-light shadows or bounce; self shadows are a 10-step height march; fracture_glass and shards do not read the shared lights yet (next step); sRGB-encoded values are shaded as given, not in linear light.
Adjust-stage CPU follow-up: film_finish caches the exact seeded grain lattice
and column interpolation terms once per frame. Color halo blur processes vertical
columns in64-column cache-friendly blocks, preserving each column's original
f64 running-sum update order. No math/transfer approximation added. Release
real1080p isolated median5: finish0.5443s->0.3730s (1.46x), exact float32bytes.
Prior concurrent run1.43x; host/export contention may differ. Small/signed/fractional
grain/seed grid and blocked-blur radius1..512 checks exact. Grain lattice adds
at most roughly one4bytevalue/pixel atgrain_size1 plus row/column terms.
Print_stock measured~0.112s/frame but unchanged in this follow-up; prior constants/
projection optimization already landed. No GPU/threading/default/output change.

- load_project no longer resets the id counter to 1000: it is set one past the largest id-like number in the loaded project (never below 1000), so resumed projects cannot overwrite existing clips/assets. Proof: tests/load_ids_proof.py.

## Camera solve, lens distortion, classical matting (SOLVE agent, BETA, all MCP)
Camera solve: KLT feature tracks, 2-view init, PnP, bundle adjustment (src sfm.rs, ext camera_solve.rs), with optional radial distortion k1/k2 and a rotation-only pan mode. refine_matte: classical (non-ML) trimap alpha matting.
Measured (author): synthetic solves 0.03-0.11 deg rotation error; real H.264 end-to-end 48/48 frames, 0.70 px RMS; distortion model takes rotation error 2.5 to 0.77 deg on a k1=-0.15 scene; pan mode on real video 48/48 frames, 0.012 deg mean error, focal 560.2 vs true 560 (examples/camera_solve_proof.py PAN=1); matting MAE x1000 14.04 to 6.67 on a synthetic hair test, 10.40 to 6.70 on real H.264 (compression noise limits it; examples/refine_matte_proof.py).
Honest limits: distortion is off by default when focal is unknown (k1 and focal are coupled); 0.77 deg is typical, not guaranteed (RANSAC sensitivity); no undistort tool, k1/k2 are reported only; matting needs a local fg/bg colour difference and is not ML; solves live in session memory only. I ran sfm_synthetic (1) and matting_synthetic (6) tests, film md5s unchanged and fuzz 229 tools / 8253 calls / 0 failures; I did not rerun the real-video proofs.

## Bezier paths (MCP)
- `add_path_shape` (SVG `d` or points+smooth), `set_path`, `path_info`. Trim uses exact arc length; offset, merge (path with path), repeater, gradient stroke, even-odd fill all work on paths.
- Proof: `python3 tests/bezier_paths_proof.py`.

- fracture_glass `scene_lights` (default 0, off = unchanged pixels) and `light_gain`: shades shard faces with the shared project lights (set_light). GGX specular on each shard's real 3D tumble normal with per-shard roughness 0.04-0.26, so highlights flash per shard with angle instead of a procedural sin-wave sheen. Needs at least one enabled light or it errors. Honest limits: specular only (no diffuse/thickness absorption), difference from the legacy look is modest at gentle light settings, no light-blocked shadows, per-shard thickness variation not done.
Node grading stage2: shared graph definitions, ordered clip refs, ordered
membership-based grade groups and a final timeline grade chain. Adjustment
layers can use local/shared graphs and keep their existing mask/opacity/duration.
Order: legacyeffects -> localgraph -> clipsharedrefs -> groups -> clipmask/
transform/composite -> timelinegraphs -> outputtransform. Clip/group clocks are
clip-local; timeline uses compositiontime. Nested/precomp/matte subrenders do
NOT apply the parent timeline chain again. Groups grade each member before
compositing, not a flattened grouped image. No inferred domains/conversion.

MCP: inspect_grade_scopes, set_grade_scopes (complete registry/membership/order/
bypass/timelineJSON), create_shared_grade_graph, bind_shared_grade_graphs,
remove_shared_grade_graph. All12node tools accept shared_graph_id instead of
clip_id, so every shared node/edge/param/key/mask/cache field stays editable.
Shareddefinition edits update all references. Results are NOT pixelcached across
clips. Referenced deletion errors until refs cleared; split_clip/split_all_at/
insert_gap copy cliprefs/group membership to the right segment. Other editorial
cut operations may create fresh unbound clip IDs: explicitly bind those IDs.
No-scope path preserves pixels; shared/group/timeline fixtures exact vs explicit
serialreference and deterministic. NativeOCIO bindings remain explicit. Stage2
is not Resolve shared-node/remote-grade semantic parity or a Fusion compositor.
## Path arcs and path morph keys (MCP)
- SVG `A`/`a` arcs in paths (endpoint to center conversion, radius scaling, glued flags).
- `add_path_key` / `clear_path_keys`: keyframe the whole path (per-point animation) with ease; keys must share the base path structure.
- Proof: `python3 tests/path_arcs_morph_proof.py`.

- Exact frame clock: media frame index now has a 1e-6 frame tolerance (media_index in render.rs). Before, times like 29/25 computed 28.99999999999996 and showed the previous source frame (a 1-frame plate lag against keyframed pins). Proof: tests/frame_clock_proof.py (98 exact frame times equal their epsilon renders); the film agent's phase_repro also reads exact_equals_previous false at 29/57/58. Not changed: speed-curve clips already had the tolerance; confetti/speedlines flicker steps use their own floor.

## Editorial follow-up (captions agent, BETA)
export_with_subtitles (selectable mov_text/SubRip streams next to burn-in, atomic output, range-relative cue times); measure_loudness true_peak=true (4x sinc reconstruction estimate, NOT certified, within 0.3 dB of FFmpeg ebur128 per the author); sync_clips drop_frame=true (29.97/59.94 DF labels, skipped labels rejected). Details in docs/EDITORIAL_MCP.md. I ran their MCP proofs (MP4/MKV streams, true peak, DF shift) and fuzz (240 tools / 8663 calls / 0 failures); the 0.3 dB figure is the author's.

Grade node widening: .cube LUT, blur and sharpen admitted. Existing RGB/point
curves, hue-vs-hue/sat/lum and log wheels already worked as grade nodes; added
exact direct-effect parity tests. File LUT uses strings.path; generated/persisted
LUT uses strings.cube_text (exactly one), intensity keyframable. generate_lut_node
bakes explicit pointwise/native OCIO chains to inline17/33 cubes, reports sampled
max/p99/RMS interpolation error, supports clip/shared targets. It does NOT auto
select output, infer domains or claim nonlinear LUTs lossless. Generated LUT
persisted with project, clamps input outside declared domain. Inline data parsed
each eval; file cache existing size/mtime bounded8entries.

Spatial nodes execute on each branch's full canvas in stated order, never moved
across mixers/qualifiers/curves. Blur/sharpen are NOT generally commutative with
nonlinear grades. Node RGB-only semantics preserve first-input alpha exactly;
this differs from alpha-blurring clip blur. Node masks blend after spatial op,
not before. Radius0..512, sharpenamount0..3 evaluated-time guards; effect scratch
still under globalmemory budget, not included in node cachedframecap.

## validate_planar_track (BETA, read-only diagnostic)
Re-tracks a plane from N independent anchor frames (each template built from the base track's own quad at that frame) and reports per-frame, per-corner px disagreement vs the base track (p50/p95/max, first flagged frame, full per-frame arrays for export). Measured on the licensed city plate (Mixkit 49844, 4 s, SHOWCASE fixture): base track confidence min 0.73 / mean 0.85, zero lost frames, yet anchored re-tracks disagree on the target quad (propagated from the tracked context quad) 0.9 px at frame 30, 3.0 at 40, 17.6 at 60, 46.9 at 90 (p50 17.3, p95 49.2, max 55.0; 56 of 75 tested frames over 3 px; 14.7 s for 100 frames). Frames before the first anchor (here 0-24) are reported as untested (tested=false, null distance), not as passes. Distances are on the target quad; context_max_px is reported separately. So a high-confidence track is flagged as unreliable. Usage: tests/validate_track_real_plate.py <plate.mp4>.
Honest limits: disagreement proves unreliability but does not say which track is right; agreement does NOT prove pixel lock (all anchors can share the same wrong, identical window); only forward re-tracks, no reverse or line-convergence constraint yet; this is a detector, not a better tracker. Constrained feature identities, hold-out patches and line constraints are not built.

## Track diagnostics (DIAG agent, BETA, read-only analysis, session-only storage)
11 MCP tools: track_features (raw tracks with train/heldout roles), fit_track_homography (soft-L1 LM, held-out residuals), decompose, jitter metrics, corner_disagreement, track_feature_residuals, lk_residual_audit, ecc_refine_track, render_error_field. See docs/TRACK_DIAGNOSTICS.md. Proof: tests/track_diag_proof.py on a synthetic plate with known corners (fit max corner error 0.28 px, ECC residual 0.018 px; I re-ran it on a freshly generated plate). Not run on the real city plate yet; features and fits live in session memory only. Run tests/track_diag_gen_plate.py from a scratch directory (it writes fr/, gt.json, plate.mp4 into the cwd).

- refine_matte rework (SOLVE): auto_bg (background colour model with flood-fill reclaim), exclude/include stroke polygons, bg_bounds colour-distance bounds (tool default true; core default off, set false on busy or similar-coloured backgrounds), foreground decontamination F=(C-(1-a)B)/a. Synthetic hair test (re-run by me): MAE x1000 14.04 to 6.67, partial-alpha error 0.398 to 0.094, passes. Wind-in-hair plate: visual judgment only, ~15k px reclaimed. Known flaw: ragged sleeve edges where shadow is near the wall colour.
- validate_planar_track `backward=true` (opt-in, about 6x slower: 94 s vs 14 s on 100 frames): also tracks backward in time from the last frame and each anchor (new planar::track_frames takes an explicit frame order; forward behaviour unchanged) and reports backward_max_target_corner_px separately. On the city plate the backward tracks started from the base track's end quad disagree 99 px at frame 0 falling to 32 px at frame 90: they inherit the base track's end-frame error, so this column is a stress signal, not a measurement of truth. Still no line-convergence or hold-out constraint.

- undistort_clip (SOLVE): removes radial Brown k1/k2 distortion from a clip (k1/k2 from a camera solve or explicit, H.264 crf 14 output, optional asset import). Measured by me via tests/undistort_proof.py on a synthetic k1 -0.15 / k2 0.02 clip: MAE vs ideal 0.0097 after undistort vs 0.0452 distorted (4.7x), coverage 1.0. Limits: the cargo round-trip test (undistort_roundtrip) was never compiled by the author and I could not complete it within my time cap either, so the MCP proof stands in; black-edge loss not measured separately; radial only (no tangential/p1p2).

- 3D camera tracker (K5, AE 3D Camera Tracker class): solve_camera `tangential` (opt-in Brown-Conrady p1/p2 in the bundle adjustment, OpenCV order; default off, radial-only results bit-identical), set_ground_plane (plane fit through >=3 solved points, normal to up = -y, origin, optional scale from two points + distance; re-applies cam_* keys and moves place_at_track layers), place_at_track (null/solid/text 3D layer at a solved point in the cam_* key mapping), export_st_map (16-bit PNG, Nuke STMap convention), effects lens_undistort / lens_redistort (exact per-pixel inverse mapping, bilinear, take the lens from strings.solve_id or explicit k1,k2,p1,p2,focal_px). Gates in tests/camera_tracker.rs (synthetic plate from known geometry: 0.3 px vs noise-free truth, k1 within 0.005, p1/p2 within 0.0005, camera path within 0.5% of scene scale, ground normal within 0.5 deg, placed layer within 0.5 px on every frame, nodal rotations within 0.1 deg). Written without running cargo in this lane: the gates have not been run yet. Solves live in session memory, so an effect with strings.solve_id errors after a reload until the solve is redone.

- refine_matte chroma-weighted background distance (SOLVE; chroma 2.5x luminance, in the auto_bg/bg_bounds tool path only): cuff streaks reduced, hair halo cleaner; right sleeve/jacket edge still ragged, a partial fix. Synthetic matting test re-run by me: passes.

- Contract fixes: add_text `x` now applies without `anchor` (it was silently ignored unless anchor was also given); add_text color and add_shape fill/stroke accept [r,g,b,a]. Fill/text alpha multiplies the layer opacity; stroke alpha is not supported and add_shape returns a warning. Proof: tests/contract_fixes_proof.py (x=150 centres at 471 px, half-alpha red fill reads 128).

- track_planar_anchored (BETA): keyframe-anchored planar tracking. Verified quads at >=2 frames; each gap is tracked forward from the left anchor and backward from the right, blended with smoothstep, so the result passes exactly through every anchor and drift cannot exceed one gap. Reports per-gap one-sided closure error and flags gaps that need another anchor. Measured on the synthetic plate with known corners (tests/track_anchored_proof.py): unanchored max error 33.85 px / mean 14.19; 2 anchors 3.61 / 1.87; 3 anchors 2.91 / 1.22; 4 anchors 2.51 / 0.94. Limits: anchors are trusted as given, so it does not pick the right window on a repeated facade, the anchors do; not yet checked against real-plate truth; slower than a plain track (about 11 s for 40 frames at 640x400 with 2 anchors, because the backward leg often runs the coarse re-acquisition). Line-convergence constraints and constrained feature identities are NOT built.
- fracture_glass `realism` (0..1, default 0 = legacy pixel-identical): scales cell count up (x1+12r, cap 2500) and debris, per-shard drag/terminal-velocity gravity/flutter, size-dependent push, crack-arrival release timing with heavy tail, refractive see-through back faces, per-edge lit rims, soft contact shadows, tapered finer cracks. Set via add_effect/set_effect_param.
- fracture_glass `realism` honest note: published from the SHAPE track; realism=0 gated to the legacy brute-force path (default and 0 render identically on the city plate). Author caveats: a few large foreground slabs stay near camera in late frames; no hairline stress cracks. Fuzz 255 tools / 9359 calls / 0 failures.
- refine_matte (fix): bg_bounds and the thin-streak alpha closing now self-gate per frame on background roughness (bg_rms < 0.10, reported per frame with bounds_used). The synthetic hair proof regressed from 16.57 to 30.40 MAE x1000 with bounds on a textured background and is now 6.70 vs 10.40 for the rough seed. Real wind-in-hair plate (bg_rms 0.067) keeps bounds on and the cleaner wall-halo result. Limit: the 0.10 threshold is calibrated on two clips only; use bg_bounds=false on busy backgrounds. Sleeve/jacket errors from cream-on-pink shadow remain; use include/exclude strokes.
- fracture_glass `core_damp` (0..1, default 0 = pixel-identical, checked by md5 on 3 frames against the previous build) and `core_radius` (fraction of frame height, default 0.16): shards near the impact get lower launch speed and extra angular scatter, fading out by core_radius. Effect is modest: at 0.8 the centre keeps denser debris at 0.7-0.9 s after impact, but the large dark hole is not reduced because it is the backdrop revealed once shards leave, not shard density. The funnel/starburst look is only partly addressed; shard collisions and per-shard thickness are still not done.
- Particle emitter motion blur (BETA): add_emitter config `shutter_deg` (0..720, default 0 = off) and `blur_samples` (0 = auto). The system is integrated at several instants across the shutter interval centred on the frame time and the premultiplied buffers are averaged, i.e. the temporal integral of every sprite, force and speed, not a fake streak. Auto samples = fastest-particle travel over the shutter / (0.6 x smallest sprite), clamped 4..32 so there is no visible stepping. Measured at 1280x720, 300 particles/s, speed 1400: render 0.11 s off, 0.18 s at 180 degrees, 0.20 s at 360. Default off; the off path runs the same code as before but I did not md5-compare it against the previous binary. Limits: samples cost linearly; blur is per emitter, not per particle; fracture_glass and shard bursts already had their own shutter, scene3d still has none.
- 3D scene motion blur (BETA): add_scene3d config `shutter_deg` (0..720, default 0 = off, identical code path to before) and `blur_samples` (0 = 8, max 32). The whole scene is re-posed and re-rendered at several instants across the shutter and averaged in premultiplied space, so object animation, parenting and the project camera move all blur. Measured on a 1280x720 rbox scene: 0.19 s/frame off, 0.87 s at 8 samples (cost is linear in samples). Limits: it is sampling, not analytic, so a very fast move at 8 samples shows stepped copies at the shutter ends (raise blur_samples); no per-object shutter, one shutter for the whole layer; no rolling shutter.
- frosted_glass (BETA, new): general backdrop effect. The layer's alpha is the pane; inside it the composite below is box-blurred (`blur` px), saturated, tinted (`tint_amount`, `tint_r/g/b`), refracted along a bevel gradient of the smoothed alpha (`refraction`, `bevel`, per-channel `dispersion`) and rim-lit (`edge_light`); `layer_mix` keeps some of the layer's own colour. It runs after the layer transform (position, scale, rotation), so moving panels frost what is under them. Proof tests/frosted_proof.py: outside the pane the render is bit-identical to effect-off; fine detail inside a pane drops 26x on a mandelbrot test plate (sum of second differences 114444 to 4473); a panel moved 140 px frosts the plate under it (colour distance 70.9 vs 123.6 for the unmoved spot; the first proof used testsrc2, 54 vs 502). Cost: 0.08 s per 640x360 frame, 0.11 s at 1280x720 render_frame. Limits: 3D-tilted layers skip it; no ray-traced lens; box blur not Gaussian; needs a layer below.
- fracture_glass `hole_fill` (0..1, default 0 = bit-identical to before, checked by md5 in tests/fracture_hole_proof.py) fills what shows where glass has left. `hole_source` 0 (default): a defocused (`hole_blur` px, default 40) and dimmed (`hole_dim_color`, default 0.45) copy of the intact pane; 1: the layers below the clip. Measured on a test plate: hole centre mean RGB 2 (black) to 57 with the fill; with a bars layer below and hole_source=1 the hole centre matches the layer below (colour distance 5 of 765). Limits: the fill is a blur of the pane, not a reconstructed scene; the backdrop counts as black where no layer is below, so hole_source=1 only makes sense over footage.
- Particle colliders and vortex (BETA, defaults off = bit-identical, checked in tests/particle_collide_proof.py): add_emitter config `colliders` [{kind circle (x,y,r) | rect (x,y,w,h) | plane (x,y,angle of outward normal)}] with `bounce` (restitution) and `friction` (tangent loss, default 0.15), and `vortex` + `vortex_x/y` swirl force. Pushed out along the surface normal, velocity reflected. Measured: sparks falling on a circle, red pixels deep inside it 426 without the collider, 0 with it. Layer silhouette collider (kind `layer`, `clip_id`, `threshold` default 0.5): the clip's alpha rendered at the current frame; particles inside it are pushed out along the alpha-gradient normal and reflected. Measured (tests/particle_layer_collide_proof.py): red particles on a blue ellipse layer, 1085 interior pixels without the collider, 0 with it. Limits: the silhouette is held static over the particle's history (a moving obstacle is approximate), no particle-particle collisions, thin rects can be tunnelled by fast particles (fixed 60 steps/s).
- Particles from a layer (BETA, new shape, default shapes unchanged): add_emitter config `shape: "layer"`, `emit_clip`, `emit_edge`. Spawn points are the pixels of the clip's alpha >= 0.5 (or only outline pixels), subsampled to at most 60k, chosen per particle by seeded hash, so it is deterministic and scrub-safe. Measured (tests/particle_emit_layer_proof.py): on a blue ellipse layer, area mode puts 19708 red pixels inside and 0 outside; edge mode 2273 pixels all on the outline (area mode only 19% near the edge). Limits: silhouette read at the current frame; no per-particle colour from the layer.
- Particles `emit_follow` (BETA, default off = unchanged): with shape `layer`, each particle spawns on the clip's silhouette at its own birth time (0.1 s bins, one clip render per bin). Measured (tests/particle_emit_follow_proof.py): ellipse sliding at 100 px/s, static particles with life 1 s: centroid x 319.0 without follow (on the ellipse, at 319.6) and 269.3 with follow, a 50 px lag = speed x half the life, as expected. Limits: 0.1 s bin width; cost grows with life/0.1 clip renders per frame.
- frosted_glass `magnify` (0.25..8, default 1 = unchanged): magnifies (or shrinks) what is under the pane about the pane's alpha centroid. With blur 0, tint_amount 0, saturation 1, edge_light 0, refraction 0 it is a clear magnifier lens. Measured (tests/frosted_proof.py, mandelbrot plate): at magnify 2 the pane region differs from the plate zoomed 2x by mean abs error 16.1 (0..255 per channel sum of 3) versus 84.5 against the unzoomed plate. Limits: centroid of the alpha is the zoom centre (no separate centre control yet), bilinear sampling of a frame that is already composited, so no extra detail appears.
- Particle collider `follow` (BETA, default off): a `layer` collider with `follow: true` uses the clip's silhouette at each integration step's own time (0.1 s bins, one clip render per bin). Measured (tests/particle_collide_follow_proof.py): an ellipse sliding at 167 px/s through resting dots; red pixels in the corridor behind it 163 with the collider frozen at the current frame, 88 with follow (dots born before the ellipse reached them are plowed ahead; the rest were born after it passed). Bug fixed in the same change, disclosed: a particle with zero velocity deep inside a flat-alpha silhouette (no gradient) was never pushed out, because the fallback direction was the negated velocity; it now exits by the nearest of 16 directions. The earlier layer-collider proof did not catch it because its particles were moving. Limits: 0.1 s bins (the silhouette can sit about 8 px off the exact frame at 167 px/s, so dots can rest up to that deep inside the exact outline); cost one clip render per bin per follow collider; push-out path capped at 400 px.
- Silhouette cache for follow/emit_follow (internal speed-up, output bit-identical, md5-checked on a 1280x720 title frame): clip-alone alpha planes are cached across frames, keyed by a hash of the clip's full JSON + time + canvas size (any clip edit changes the key), at most 96 planes. Measured on the title test with a follow collider and an emit_follow emitter: 1.72 s per frame uncached, 1.17 s for the next frames. Derived forms (u8 plane, spawn points) are now cached per plane too, with no per-frame clones: 1.17 s to 1.04 s per frame, md5-identical. Profiling the same shot: base video + text 0.27 s, embers (emit_follow) +0.45 s, rain with a frozen layer collider +0.10 s, rain with a follow collider +0.80 s. The follow cost is in the collision walk (particles inside the moving title use the 16-direction exit search per sub-step), not in the cache. Correction after profiling inside the engine: the caches live for one tool call, so a lone render_frame pays the full silhouette cost (about 0.85 s at 720p, of which about 0.5 s is silhouette rendering and 0.05 s is the particle simulation), while an export reuses them across frames. Measured: the full moving-title shot (video, text, follow collider rain, emit_follow embers), 5 s at 1280x720, 125 frames, exports in 37 s (0.30 s per frame), ffprobe and a full decode clean. The earlier 'about 150 s' estimate was wrong (it extrapolated from single-frame calls). The 16-direction push-out search is not the cost (disabling it changed nothing).
- Silhouette cache is now shared across tool calls (it was per call, so a lone render_frame always missed). The key also hashes the project parts the clip-alone render reads (grade scopes, editorial, lights, materials, occluders, colour pipeline, camera, assets, fps) so an edit never serves a stale plane. Measured on the moving-title shot with follow collider and emit_follow embers: repeat render_frame 1.04 s to 0.42 s, output md5-identical; an edit after a cached render is identical to a fresh process. Limit: the cache does not watch media files on disk, so replacing a source file in place under the same path can serve a stale plane until 96 other planes push it out or the server restarts.
- frosted_glass `magnify_cx` / `magnify_cy` (canvas fractions of width / height, default -1 = the pane's alpha centroid): zoom the lens about any point, for example a cursor or a feature under a moving pane. Both are keyframable like the other parameters. Proof tests/frosted_center_proof.py on the mandelbrot plate with a clear lens (no blur, tint, refraction, edge light): with magnify 3 about (0.25, 0.4) the pane matches an independent bilinear resample of the plate about that point to a mean error of 1.05 (0-255), against 96.77 for the centroid resample; the default stays a zoom about the pane centre (error 9.93 to a canvas-centre resample, the pane centroid is not exactly the centre) and setting -1 restores it byte-identically.
- frosted_glass on 3D-tilted layers (rot_x / rot_y / pos_z): fixed. Before, the effect was silently skipped on any tilted layer (the 3D path never ran the post-transform frosting; found by tests/frosted_tilt_proof.py failing on the v1.0.0 binary with "frosted_glass did nothing on a tilted layer"). The pane is now the projected silhouette and frosts what is really under it. Proof on the mandelbrot plate, rot_y 42: footprint is a trapezoid (changed-pixel column height 142 px near the left edge, 260 near the right, against the layer's own 138 and 258), pixels outside the pane (3 px margin) are bit-identical to the effect-off render, detail in the pane core drops from 3266 to 688. Limit: refraction and magnify still work in screen space on the projected pane; they do not follow the 3D surface normal.
- frosted_glass `tilt_refraction` (BETA, px per unit of surface normal, default 0 = off, output unchanged): on a 3D-tilted layer the whole pane refracts the backdrop along its 3D normal (normal xy = sin(rot_y), -sin(rot_x)), like a tilted slab, instead of only at the bevel. Proof tests/frosted_normal_proof.py (testsrc2 plate, rot_y +40 / -40): shift of the backdrop is +13 / -13 px at 20 and +26 / -26 px at 40, opposite for opposite tilt and linear in the value; the default render is bit-identical to the param-less render. Limit: first-order (a constant offset from the layer's rotation, not a per-pixel ray through the slab thickness); rot_z and pos_z do not change the normal.
- set_property / set_keyframe reject unknown clip property names (fix, found by an audit). In v1.0.0 any name returned `{ok: true}` and was stored but never read, for example `crop_left`, `flip_h`, `anchor_x` or a typo, so an AI could believe it had cropped or flipped a clip when nothing changed. Now they return an error that lists the valid names (opacity, x, y, scale, rotation, rot_x, rot_y, pos_z, backface); effect params still go through `effect_id` / set_effect_param. Proof tests/unknown_property_proof.py fails on v1.0.0 ("set_property accepted banana"). Not yet done: set_effect_param with a misspelled param name is still accepted silently. The same audit compared flat and 0.05 degree tilted renders for shape and text layers with 8 effects, 8 transform props and an emitter: no skipped feature (differences stay near the 0.1 resampling floor; fracture_glass is higher at 0.6 to 0.9 because its shard edges resample).
- set_effect_param warns on unlisted param names (non-breaking: the value is still stored, the reply adds `warning` with the effect's listed params). Reason: a typo like `blurr` returned a bare `{ok: true}` and did nothing. It does not reject, because some effects read params that list_effects does not show. Found by the same audit: list_effects for frosted_glass was missing magnify_cx, magnify_cy and tilt_refraction and still said "not for 3D-tilted layers"; both are corrected. Proof tests/effect_param_warning_proof.py. Open: other effects may read params their catalog entry does not list; a full catalog-vs-code audit is not done.
- History and named checkpoints (BETA, new tools `history_list`, `checkpoint`, `restore`, `checkpoint_delete`; `undo` / `redo` gain `steps`). v1.0.0 already had a bounded linear undo/redo (full-project snapshot before each mutating call, 100 steps, about 256 MB). Added: `history_list` shows every undo step by tool name and clip id, the redo stack and the checkpoints; `checkpoint name` saves the whole project under a name (same name overwrites, newest 24 kept); `restore name` returns the project to it and is itself one undo step, so a bad restore is undoable; `undo steps=N` / `redo steps=N` move several steps. Proof tests/undo_checkpoint_proof.py: restore gives a project byte-identical to the checkpoint (get_project string equal), undo of the restore brings the edits back, undo 3 / redo 3 round-trip exactly, a new edit clears redo, errors on unknown names, cap of 24. Fuzz: 259 tools / 9416 calls / 0 failures. Limits: history and checkpoints live in server memory only (lost on restart, not saved in the project file); every step and checkpoint is a whole-project clone (no diffs); files written to disk (renders, exports) are not undone. The tool count is now 259 (README still says 255 on main; update it when this branch is merged).
- History survives restarts (opt-in): `save_project with_history=true` also writes `<path>.history` (undo steps, their labels, checkpoints); `load_project with_history=true` restores them, and the load is one more undo step on top. Plain save/load are unchanged (no .history file, no history loaded). Proof tests/history_persist_proof.py runs two separate server processes: after the restart history_list shows the old steps, undo 3 reaches the original project exactly, and restore of a pre-restart checkpoint equals the original. Limit: the sidecar holds whole-project snapshots, so it can be large on big projects (100 steps max); redo is not saved.
- Dead-parameter audit (measurement, not a feature): 597 numeric params over 111 effects were each changed and the render compared. 93 showed no change on a plain video plate; retested with a layer that has real alpha, several times and by-hand sweeps, none is dead. What remains has a precondition: impact_shake needs `strings.hits`, spill_suppress needs spill, fracture_glass light_gain needs scene_lights=1 plus a light, hole_blur / hole_dim_color / hole_source need hole_fill > 0. The qualifier, power_window, primaries, log_wheels, hue_curves, color_transfer, color_managed and lab_match params (colour modules) were not retested. Those preconditions are now written into the list_effects descriptions. Limit: 4 sample times and 2 test values per param can miss a param that only acts in a narrow range.
- Undo/redo coverage audit (tests/undo_coverage_proof.py). Every tool is called with trivial arguments on a fresh project. A call that changes the project must add exactly one undo step; undo must give back a byte-identical project, redo must restore it byte-identical, and a fresh edit must clear redo. Result on 259 tools: 114 tools accepted the trivial arguments, 83 of them are mutating and passed undo, redo and redo-clear; the rest are read-only and changed nothing. Found and fixed one real hole: `list_modifiers` with an `effect_id` created the property it was asked to list, so a read tool changed the project with no undo step (now a read-only lookup). Not covered: 126 tools rejected the trivial arguments (they need a real clip type, a file, a colour pipeline or a camera solve), and slow tools (exports, motion_extract) were skipped. This is a sample of the surface, not a proof for every tool.
- frosted_glass slab refraction (BETA; `slab_thickness` world px, `ior`, default off, default render bit-identical): on a 3D-tilted pane the backdrop is refracted per pixel as through a real glass slab. The eye ray's incidence angle at each pixel (perspective, so it differs across the pane) goes through Snell's law, and the backdrop point seen is the front-face hit moved in the plane by T*(tan ti - tan tt). It replaces the constant first-order `tilt_refraction` for glass that should look like glass; dispersion splits the ior per channel. Proof tests/frosted_slab_proof.py (testsrc2 plate, white 420x240 pane, blur 0): zero shift at normal incidence and at ior 1; the shift at the pane centre at rot_y 50, T 100, ior 1.5 is 47.5 px against 43.6 px from an independent perspective closed form (9% off; the measurement averages a 120 px window across a shift that varies in x), and 60.0 px against 63.0 for ior 2.2; shift 22 px at 30 degrees; half the thickness gives 21 px (0.44, not exactly 0.5 because the window mixes shifts); left and right of the pane differ (47.5 vs 37.5), which a constant offset cannot do. Limits: single front-to-back slab, no internal reflections or total internal reflection, no thickness variation or bevel in 3D, the backdrop is the 2D composite below (not a 3D scene), box-blur frost as before.
- frosted_glass `fresnel` reflection (BETA, default 0 = off, default render unchanged; params fresnel, f0, env_gain, box_gain, box_x, box_y, box_size). Schlick reflectance per pixel from the view ray and the pane normal (flat panes use the default camera, tilted panes the layer's projector), reflecting a procedural environment (sky gradient plus one Gaussian softbox) over the frosted pane. Proof tests/frosted_fresnel_proof.py on a black backdrop (pixel = F * env exactly): centre pixel of a 300x180 pane vs an independent closed form, rot_y 0: F 0.040, expected rgb 3.4/4.2/5.6, measured 3/4/6; rot_y 45: 3.5/4.4/5.9 vs 4/4/6; rot_y 75: F 0.255, expected 21.3/26.9/35.6, measured 21/27/35. Reflectance rises toward grazing, fresnel 0 is bit-identical to off, the softbox shows only when the reflected ray points at it (207 vs 3). Limits: the environment is a fixed procedural sky plus one softbox, not an HDRI or the scene; no reflection of other layers (no screen-space or ray-traced reflection yet); one reflection lobe, no roughness blur of the environment.
- frosted_glass `reflect_scene` (BETA, default 0 = off; with fresnel > 0; params reflect_scene, reflect_dist, env_blur): the Fresnel reflection also shows the backdrop, looked up in screen space along the reflected ray (x + r.x*reflect_dist, y + r.y*reflect_dist) and blurred by env_blur px, so tilting the pane slides the reflection. Proof tests/frosted_reflect_scene_proof.py (white stripe on black, 300 px pane, reflect_dist 120): the ghost of the stripe appears where the closed form x0 + sin(2*rot_y)*dist says: rot_y 35, stripe at 150: ghost at x=266 vs 263; rot_y -35, stripe at 490: ghost at 373 vs 377; reflect_scene 0 shows no ghost. Limits: this is a screen-space approximation. The reflected ray is looked up in the same 2D backdrop (a studio set that also exists in front of the pane), not in a 3D scene, so there is no occlusion or depth, the lookup distance is a user constant, and nothing outside the frame can be reflected.
- light_wrap effect (BETA, new; params width, intensity, blur, desaturate): the backdrop's light bleeds over the layer's own edge, wrap = blur(backdrop) * blur(1 - alpha, width), added where the layer is opaque. It runs after the layer transform (same post path as frosted_glass, so it also works on moved and tilted layers) and needs a layer below. Proof tests/light_wrap_proof.py: a dark grey disc over a backdrop that is red on the left and blue on the right picks up +106 red 4 px inside the left edge (0 blue) and +106 blue at the right edge; the wrap fades inward (red +106, +28, 0 at 4, 20 and 70 px inside); the disc centre and every pixel outside the layer are bit-identical to effect-off; intensity 0 is a no-op; desaturate 1 gives a grey wrap (+22 each channel). Looked at on the city plate (dark disc, width 40, intensity 1.6): the plate's warm and cool lights tint the disc rim. Limits: wrap uses a blurred 2D backdrop colour (no depth, no light direction), the edge falloff is a box blur, and it is not driven by scene lights.
- cast_shadow effect (BETA, new; params use_scene_light, light_x/y/z, height, opacity, light_size, color): the layer's alpha is projected onto the plane below it from a scene light (or light_x/y/z) by similar triangles, shift = (C - L).xy * H / (Lz - H), then blurred by a finite-light penumbra radius = light_size * H / (Lz - H). It runs in the post path after the layer transform (works on moved and tilted layers), needs a layer below, and only writes outside the caster. Proof tests/cast_shadow_proof.py: the shadow far edge matches the closed form within 2.5 px (bbox to x 397, y 250 vs predicted 27.4, 20.6 px shift); the caster is bit-identical to effect-off; the opposite light flips the shadow side; the 10-90% penumbra width is 7 px at H 0.1 and 14 px at H 0.2 (ratio 2.00 vs formula 2.33, within 25%); light_size 0 is a hard edge; opacity 0 is a no-op. Limits: one constant height for the whole layer, so there is no per-point contact hardening; the receiver is a flat plane (shadows do not climb over objects below); not ray traced.
- bloom effect (BETA, new; params amount, threshold, knee, radius, levels, conserve, tint_r/g/b): light-intensity-responsive bloom. Soft-knee bright-pass that keeps the pixel's colour and scales with its excess over the threshold (so a light twice as bright blooms more than twice as much, including values above 1.0), then a multi-scale blur (radius, 2r, 4r ... up to 7 levels, equal weights). conserve 1 (default) removes from the bright pixels what the bloom adds, so the frame energy is preserved; conserve 0 is plain additive. The glow spills past the layer's alpha. Proof tests/bloom_proof.py: raising a light from 0.9 to 1.8 gives a tail ratio of 3.25 vs 3.25 predicted from the excess (1.3/0.4); conserve keeps the frame sum within 0.31% of the input; additive adds 6351 vs 6400 predicted (0.8% off); a tint of (1, 0.3, 0.3) makes a red tail (0.0213 vs 0.0064 green); amount 0 and a threshold above the light level are bit-identical to effect-off. Limits: box-blur approximation of a Gaussian (3 passes per level), not a measured lens point-spread function or diffraction glare; the spill uses normal alpha compositing over the layer below, not an add blend, so it slightly darkens a bright backdrop under the halo; no star/streak glare.
- glow `mode` 1 deep glow (BETA, default 0 = classic glow, bit-identical; params falloff, iterations, exposure, knee, color_a_r/g/b, color_b_r/g/b, linear): bloom's soft-knee bright pass (shared code), then an exact sum of separable sampled Gaussians (taps -r..r, r = ceil(3 sigma)+1, normalised, clamped edges). falloff 0: iterations n Gaussians, sigma_k = radius * 2^k / 2^(n-1), weights 1/n; falloff 1: fixed 5-term mixture approximating exp(-r/radius), sigma = radius * {1/4, 1/2, 1, 2, 4}, weights proportional to s^3 exp(-s^2/2) (~0.0082, 0.0601, 0.3303, 0.5897, 0.0117). Kernel weights sum to 1, so with white tints the glow adds amount * 2^exposure * bright-pass energy (interior). Tint lerps color_a (tightest) to color_b (widest) per component. linear 1 (default) decodes sRGB, glows, re-encodes; untouched pixels keep their bits; the glow spills past alpha like bloom. pub API in glow.rs: gaussian_kernel_1d, deep_glow_components, deep_glow_response, deep_glow_support, knee_excess. Limits: no proof script yet; the exponential is an octave quadrature (truncation/discretisation error about 1.5% before normalisation); the kernel window is square (3 sigma + 1).
- lens_flare `mode` 1 v2 (BETA, default 0 = classic flare, bit-identical): halo + core, ring, ghosts (count 0..8, spacing, size) on the source -> centre axis, anamorphic streak (length, width), starburst (rays, length, angle); each element has its own intensity (0 = off). Source: strings.source_clip (displacement of a track_point-keyed clip's x/y from their base, added to x*w, y*h), strings.source_light (project light pos x*w, y*h at t), else x/y; resolved exactly at the frame time. strings.occluder: visible = 1 - mean alpha of that clip rendered alone over a 49-point disc of radius source_r px (flare::flare_visibility), every element scales by it. The renderer injects the resolved values as _src_x/_src_y/_visible params on a copy of the effect (render::resolve_flares, no statics). Limits: no proof script yet; positions are canvas px, so on a moved layer the flare is drawn in the layer's own (pre-transform) pixels; inside a precomp only that precomp's clips can be referenced; no 3D camera projection of lights.
- light_wrap `linear` (BETA, default 0 = as before, bit-identical): 1 decodes the layer and the backdrop sRGB -> linear, adds blur(backdrop) * clamp(box_blur3(1 - alpha, width)) * intensity * alpha, re-encodes; the edge falloff is the same alpha-only box^3 blur, untouched pixels keep their bits.
- light_rig tool (BETA, new, MCP): one call sets up a key/fill/rim rig from a named recipe (three_point, rembrandt, butterfly, split, golden_hour, neon_night) as three project point lights named <prefix>_key/_fill/_rim, replacing any with that prefix. Params: ratio (key:fill, 1..32, recipe default), intensity, warmth, subject [x,y] (moves the rig to follow the subject). The lights feed set_relight, cast_shadow and light_wrap. Proof tests/light_rig_proof.py: fill = key / ratio to 1e-9, rim = key * recipe gain, a repeat call replaces instead of adding, warmth shifts the key colour, subject moves the rig by the exact offset, bad input and a 17th light are rejected; on a relit disc under rembrandt, the key side is brighter and the key-to-far-side contrast rises from 1.06 at ratio 2 to 1.45 at ratio 16. Limits: the recipes are positions and colours chosen by hand from common cinematography setups, not derived from a measured scene; all lights are point lights without shadows of their own (shadows come from cast_shadow and the relight height field); the effect on a layer is only as good as relight's height-map normals.
- Dangling clip references (BEHAVIOUR CHANGE, branch arch-u9i): when a tool removes a clip (delete_clip, ripple_delete, lift_range, remove_track, any mutating tool that drops clips), every reference to it is cleared and listed in the result as `cleared_refs: [{clip_id, field, removed_id}]`. Kinds cleared: mask.matte.source (the clip then renders exactly like an unmatted clip, pixel-exact, tested), mask layer ref_clip, ShardBurst plane.occlusion_target, Emitter emit_clip and layer colliders, Project.occluders entry keyed by the clip, caption-group membership, planar anchor/sequence globals for that id; precomps are searched recursively. Undo restores the whole pre-delete state. Before this, a deleted matte source left a dangling id that rendered without error. Not covered yet: grade_scopes clip_bindings/groups (COLOR owns that module), unknown effect/mask `strings` keys (reported by the test-only validator `Project::dangling_refs()`, never cleared).
- Grade scopes and delete (fix, branch arch-u9j): delete_clip on a clip with a shared-graph binding used to fail ("rejected by sandbox: missing bound clip") and now succeeds, clearing clip_bindings and group membership (reported in cleared_refs; the shared graph stays). duplicate_clip now copies the shared-graph binding and group membership, like split already did (before, a duplicate of a graded clip rendered ungraded). Known limit: clip lookups by id (find_clip, find_clip_mut, most tools) do not reach clips nested inside a precomp, while render and grade-scope validation do; pinned by a test.
- Track lock is now enforced centrally (BEHAVIOUR CHANGE, branch arch-u9l): any mutating tool whose result would change a locked track's clips (count, order or any clip, including adding a clip onto it or removing the track) is rolled back with "track 'X' is locked: tool 'T' would modify it; unlock with set_track locked=false". Before, only the 9 edit.rs tools honoured the lock; move_clip, delete_clip, trim_clip, set_property, add_effect and the rest ignored it. Exempt: set_track, undo, redo, restore, load_project, checkpoint, project_set, history_*. A blocked call leaves the project byte-identical and the undo stack unchanged; the lock check runs before dangling-reference clearing. Reads and renders are unaffected. Cross-track side effects (ripple across tracks) are blocked too, by outcome. Tested for 10 tools plus unlock-then-succeed; mutation-checked.
- loop_clip tool (BETA, new, MCP; branch arch-u10): repeat a clip `times` (1..100) extra copies back to back with optional `gap`; copies keep effects, keyframes, masks and the grade binding, drop transition_in. Does not ripple other clips. Contract rows: spacing, validation errors, binding copy, undo/redo byte-exact.
- distribute_clips tool (BETA, new, MCP; branch arch-u10): lay a track's clips end to end in start order with a `gap`, optional new `start` for the first; only starts change, clips already in place stay shared with the undo snapshot (moved count returned). Rows: ordering, gap, start, no-op, errors.
- swap_clips tool (BETA, new, MCP; branch arch-u11): swap two clips on the same track; the earlier one's slot start goes to the other, which then follows after the same gap, so the pair keeps its span. Refuses overlap, other tracks, locked tracks, or another clip inside the span. loop_clip/duplicate note: copies reuse the same source range (media clips share the asset, in_point and speed, so no new decode source) and get new clip ids, but effect ids are copied unchanged (effect ids are scoped per clip).

- Preview proxy media: six lifecycle tools, CFR timestamp/frame validation, intra-frame H.264 generation, preview switching, original-only exports and analysis, deep SHA256 checks, fallback and byte-identical relinking. Bindings and preference persist and support undo/redo. Synchronous, opaque SDR only; see [workflow and limits](docs/PROXY_WORKFLOW.md).

## v7 additions

Audited against the registry (495 tools at the start of this pass) and FEATURES.md before adding anything, specifically to avoid duplicating cut_to_speech, captions, proxies, multicam, auto_reframe, beat_cut/detect_beats/cut_to_beat, remove_silence_cuts/trim_filler_words, scene_split, auto_duck, measure_loudness/normalize_loudness, export_preset/join_export, project_diff, timeline_summary/transcript_search and `find` (clip/track query by name/glob/role/color/tag/track/source_kind/time_range/text_contains) -- all of which already existed and cover most of the obvious "new editor feature" ideas before any code was written. What follows are the real gaps found.

- [x] `find`: `has_effect` filter + `effects` field on every result. New param `has_effect` (an effect kind, e.g. `glow`) restricts matches to clips carrying that effect; every clip match now also lists its effect kinds in order (`effects`), so a query answers "what effects does this clip have" without a second round trip. Clip-only (errors with `kind: track`). 1 new TDD test (`find_has_effect_filters_by_effect_kind_and_lists_effects_in_results`, tests/names.rs): filters correctly, multiple clips with the same kind, unknown kind returns empty, effects ride the result in add order, track-kind + has_effect is a typed error, and `find` is confirmed still read-only (undo stack unchanged, project byte-identical before/after). golden_registry.txt hand-updated for find's grown schema+description hash (first sentence re-shortened to stay under the 150-char contract tests/description_first_sentence.rs enforces, caught by the full-suite run at the wrap).
- [x] `project_merge`: a genuinely new tool, the write-side of `project_diff` (which is read-only; nothing in the engine applied a diff before this). Two-way clip-level merge: base -> theirs revisions (same `current | undo:<n> | checkpoint:<name> | file:<path.kimo>` forms as project_diff), applied onto the live project. Tracks/clips matched by id. Detects four conflict shapes (add_add, edit_edit, edit_removed, remove_modify) by comparing each side's content against the common base, not just against each other, so an edit that happens to produce the same end state on both sides is never a false conflict. `on_conflict`: `fail` (default, applies nothing, lists every conflict), `theirs`, or `base`. `dry_run` reports without mutating. Destination tracks are created on demand. Deliberately bounded to tracks/clips, not whole-project settings. 5 TDD tests (tests/project_merge.rs): clean fast-forward (add + move), edit_edit_conflict under all three policies (including the base-case regression this surfaced: a no-op merge must not look like something to undo), remove_modify_conflict + a clean removal in the same call, dry_run + a no-op self-merge, and typed errors + confirming the merge is itself one ordinary undo step. golden_registry.txt hand-updated (496 tools), toolgroups.rs updated (project group), all green including toolgroups::tests::every_tool_has_a_real_group (first sentence also re-shortened for the 150-char rule, same wrap-time catch as find's).
- [x] `mark_scene_cuts`: new tool, automates the existing two-call workflow (`detect_cuts` then `add_marker` by hand per cut) into one call. Not a duplicate of `scene_split` (splits a clip into separate timeline clips) or `detect_cuts` (read-only, never touches the project) -- this only places markers. Same detection options and fps-snap technique as `scene_split` (reused via `dispatch_nested("detect_cuts")`). `skip_existing` (default true, 2-frame tolerance) means calling it twice does not pile up duplicate markers; `name_prefix`/`color` are configurable; `dry_run` reports without creating anything. 4 new TDD tests on a generated red/green/blue/white shot video (same fixture style as scene_split.rs): one marker per cut named in order, dry_run + custom prefix/color, skip_existing across repeated calls (and the escape hatch to force duplicates), single-shot/no-cut and typed-error cases. golden_registry.txt hand-updated (497 tools), toolgroups.rs updated (timeline group).
- [x] `export_variants`: new tool, batch-exports several size/aspect-ratio deliverables from one project in a single call. `batch` explicitly forbids `export*` tools, and no existing tool rendered more than one output size per call. Each variant: name + width/height, or one of 4 named aspect presets (16:9/9:16/1:1/4:5), or neither (current canvas size) -- useful for batching named deliverables even at one resolution. `path_template` fills in `{name}` per variant, or each variant can give its own path. `auto_reframe` (opt-in): when a variant's aspect differs from the project's own, runs `auto_reframe` on every media clip for that export, then removes the window again right after -- nothing is left on the clips. The project's width/height is restored to exactly what it was before the call, whether every variant succeeds or not; variants are independent (one bad size doesn't stop the rest, each result carries ok + path/width/height/duration/bytes or error). 5 new TDD tests: per-variant sizing + canvas restoration, a bad variant failing without blocking a good one in the same call, path_template substitution + duplicate-name/missing-placeholder/empty-variants/width-without-height errors, an unknown aspect preset, and auto_reframe tracking a real media clip's subject then leaving no reframe props behind. golden_registry.txt hand-updated (498 tools); auto-classified into the output group (starts with `export`).
- [x] `kimo gui`: clip detail endpoint + effect-params panel. New `GET /api/clip?id=<clip_id>` (`clip_detail` in gui.rs) returns a clip's name/timing, every property's base value, and every effect with its id/kind/params -- read through the same `Session` the rest of the GUI already shares, no second dispatch path. The web UI: click a clip bar to select it (highlighted), which loads the panel -- editable number inputs per property and per effect param (onchange -> `set_effect_param`/`set_property` through the existing `/api/call`), a remove button per effect, and an "add effect" picker (populated from `list_effects`, itself called through `/api/call`) that adds a bare new effect via `add_effect`. No new dispatch path anywhere; the panel only ever drives tools that already existed. 3 new headless TDD tests (clip_detail's shape incl. an unknown id, the new route's 200/404/400/405, and the exact fetch-detail -> call set_effect_param -> refetch sequence the panel's JS drives, confirmed to actually move the param) -- 14/14 gui tests green. Hand smoke-tested against a live server (curl through project_set/add_track/add_shape/add_effect/set_effect_param/preview.png, same as the original GUI's own smoke test). Honest gap: no keyframe editing (base values only, same scope set_property already has without a time), and the add-effect picker has no param inputs at add-time (set them after, in the panel).
- [x] `kimo gui`: editable timeline -- drag to move or trim a clip. Clip bars are now absolutely positioned within their track lane (start * px/sec), which the previous flex layout needed for a gapless sequence but a movable clip does not. Dragging the body of a bar moves it (`move_clip`, new start); dragging its right-edge handle trims it (`trim_clip`, new duration); both resolve on mouseup through the same `/api/call` the tool panel already drives -- no new dispatch path. A sub-3px drag is treated as a click (selects the clip for the effect-params panel) rather than a move, so clicking still works. 1 new headless TDD test driving the exact move-then-trim sequence the JS performs (through handle_request, confirming position survives the trim and a stale clip id reports a tool error not an HTTP one) -- 15/15 gui tests green. Hand smoke-tested against a live server (move_clip + trim_clip through /api/call, confirmed on /api/timeline). Honest gap: no snapping to other clips/markers, no multi-select drag, no keyboard nudge.
- [x] `kimo gui`: scrub and play. No new backend route -- this is JS driving the existing `/preview.png?t=` endpoint repeatedly, honestly: a play control steps `t` forward on a 150 ms tick (real-elapsed-time based, so it tracks wall-clock speed rather than drifting), each tick re-rendering a still frame through the real engine; clicking or dragging anywhere on the timeline (not on a clip) scrubs the playhead to that time and renders it immediately; a red playhead line tracks the current time, kept in sync whether it moved by scrub, play, or typing into the time field directly. Explicitly a frame-stepped preview, not a video element -- the same honesty the original GUI commit held for the single preview frame. 1 new headless test confirming the page actually carries the controls it claims (play-btn, togglePlay, scrubTo, the playhead element) -- 16/16 gui tests green. Hand smoke-tested against a live server: repeated /preview.png ticks at the play-loop's own cadence all render real frames.
- [x] `kimo gui`: undo/redo + history panel. Undo/Redo buttons call the existing `undo`/`redo` tools through `/api/call`, refreshing the timeline and (if one is open) the effect-params panel afterward -- the engine's bounded linear undo/redo (100 steps, pre-v7) was already there; the GUI just didn't expose it. A History panel lists every undo step by tool name (oldest first, the one `undo` reverts next highlighted) and every redo step, both from `history_list`, disabling each button when its stack is empty. 2 new tests: a headless undo -> history_list -> redo round trip through the exact sequence the panel drives (confirming undo/redo actually move the live project, not just the history bookkeeping), and the page-carries-its-controls check extended to the new buttons/functions -- 17/17 gui tests green. Hand smoke-tested against a live server (project_set, add_track, history_list before/after, undo, undo reverted the track, redo, redo brought it back).

## v7.2 additions

PRIORITY CHANGE (Naman, 8:00-8:01 AM IST Oct 30): the north star is "an AI agent driving kimo over MCP
alone, with no human and no GUI, can do a complete professional edit end to end" and "editor that is
better than any editor in the world" -- favour features where an agent-driven editor beats Premiere/
Resolve/Final Cut, not features that copy them. GUI work (direction a from the original brief) is
deprioritized below the agent-completeness and agent-superiority gaps.

Step 1 audit: walked the full workflow (ingest -> rough cut -> fine cut -> transitions -> titles/graphics
-> color -> audio mix -> captions -> review -> export/deliver) against the registry (498 tools at the
start of this pass) and FEATURES.md, grep-verifying every claim below before writing it.

Already covered (checked, NOT gaps -- listed so the next pass doesn't re-discover them):
- Textual/structured timeline summary: `timeline_summary`. Whole-project read: `get_project`.
- "What changed" readback after an edit: `project_diff` (read) / `project_merge` (write), v7.1.
- Render a frame or a range as a contact sheet for visual review: `render_frame`, `contact_sheet`
  (times[], start/end/n, at_cuts, keyframes_of -- already takes a time or a range, 16-tile grid).
- Searchable transcript / finding a moment by spoken words: `transcript_search` (exact normalized
  text search over transcript takes, returns timeline ranges).
- Atomic edit-plan / EDL-apply tool with dry-run, diff and rollback: `batch` already does exactly this
  -- ops list, atomic (all-or-nothing + restores ids), dry_run (rehearses + returns a revision hash),
  expect_revision drift rejection, foreach with `{"find":{...filters...}}` as the source. This is also
  already "apply an effect/preset to a filtered set of clips in one call": `batch` with
  `foreach:{var,in:{find:{...}},ops:[...]}` substitutes `{{c}}`/`{{c.id}}` per match -- no new tool
  needed, confirmed by hand-running it against a live project (foreach + find + add_effect in one batch
  call, 3 matching clips all got the effect, non-matching clips untouched).
- Missing/broken media references: `list_dependencies` (status ok|missing|changed per external file,
  read-only) -- so `validate_timeline` below is scoped to pure timeline structure, not file presence.
- Per-clip and per-mix loudness measurement: `measure_loudness` already takes `clip_id` (not just the
  whole mix) -- the gap is QUERYING across every clip at once (see `find_by_loudness` below), not
  measuring one.
- Auto chapter generation from markers: `mux_chapters` already writes the timeline's own markers into
  an exported file as real container chapters.
- Ripple-close every gap on a track: `distribute_clips` with gap=0 already re-lays every clip butt-
  joined in start order (a dedicated `close_gaps` would be a thin alias of this, not a new capability).
- Multi-variant output from one timeline: `export_variants` (v7.1).
- Dry-run/preview of any op: covered generically by `batch`'s `dry_run` (works for any tool reachable
  from batch, which is everything except the short deny-list).

Real gaps found (perception/feedback and agent-superiority), built below, cheapest first:
- [x] `validate_timeline`: new tool, structural health check independent of `lint_edit` (platform/
  export-readiness lint: dark frames, text under UI, b-roll-over-silence, loudness/captions/fonts --
  confirmed by reading its full source, it never looks at raw clip start/duration structure) and
  `list_dependencies` (missing/changed media files -- not re-checked here). Per track: gaps (empty time
  between consecutive clips, with duration), overlaps (two clips sharing time on the same track);
  anywhere: zero/negative-duration clips, negative clip starts. `min_gap_seconds` ignores gaps under a
  threshold (e.g. sub-frame rounding from other tools); `track_id` (id or @name, same resolution as
  `find`) scopes to one track. Read-only. 10 new TDD tests (tests/validate_timeline.rs): a clean butt-
  joined timeline is ok, a gap is reported with both clips and its duration, an overlap is reported with
  its duration, min_gap_seconds filters small gaps, tracks are independent (same-span clips on different
  tracks never conflict), track_id scoping, negative-start/zero-duration clips reached by constructing
  the project directly (add_clip itself refuses these -- proves the check also catches a state that
  arrived some other way, e.g. a hand-edited .kimo loaded with load_project), empty project/track, and
  the two track_id edge cases (a plain unmatched id scopes to nothing silently; an unknown @name errors,
  matching find's own behaviour). golden_registry.txt hand-updated (499 tools; hash computed with a
  throwaway test against the live registry, same technique as every prior v7/v7.1 addition, then
  removed), toolgroups.rs updated (output group, next to lint_edit). All green including
  golden_registry_contract, description_first_sentence, external_refs and toolgroups::tests::*.
- [x] Keyframe CRUD, corrected scope after digging in: `get_curve`/`move_key`/`remove_key` (the
  existing graph-editor tools) already read/move/delete a clip's own transform-property keys -- I
  initially planned new `list_keyframes`/`remove_keyframe` tools before finding this and would have
  duplicated them. The REAL gap: all three only ever reached `c.props`, never an effect's own param,
  even though `set_keyframe` has always been able to target one (`effect_id` + `property`). Added an
  optional `effect_id` to all three (backward compatible: omitted behaves exactly as before, every
  existing call site is clip-props-only and untouched) so an effect param's curve can be read, moved and
  deleted the same way a transform prop's can. Separately, nothing listed what IS animated on a clip
  without already knowing a property name -> new `list_animated_properties` (read-only: every clip prop
  and effect param carrying >=1 key, with scope/effect_id/effect_kind/base/key_count/expression count).
  11 new TDD tests: tests/graph_editor_effect_params_v72.rs (6: get_curve/move_key/remove_key against an
  effect param, confirming effect_id omitted still only reaches clip props with no cross-talk, unknown
  effect_id errors on all three) and tests/list_animated_properties.rs (5: empty clip, a static prop is
  not listed but a keyframed one is, effect params carry their effect_id/kind, clip+effect mixed, unknown
  clip_id errors). golden_registry.txt hand-updated for the 3 changed tools' grown schema+description
  hashes plus the 1 new tool (500 tools), toolgroups.rs updated (animate group). All green including
  golden_frames_registry_and_projects (rendered frames and existing curves.rs's 22 tests unaffected).
- [x] `find_by_loudness`: new tool, whole-project loudness query in one call ("all clips under -20
  LUFS" is the north star's own example of agent-superior "whole-project reasoning in one call"). Reuses
  `measure_loudness` itself per clip through `dispatch_nested` (no second loudness implementation --
  identical results to calling it one clip at a time, just scoped to every media clip and range-filtered
  for you). min_lufs/max_lufs bound the range, both optional -- omit both for a full per-clip loudness
  readout of the project (also closes the "audio level readout per clip" perception gap in one tool);
  track_id scopes to one track; true_peak also measures true-peak dBFS. A clip too short/silent for an
  integrated reading (under measure_loudness's own 0.4s minimum), or one measure_loudness itself fails
  on (e.g. a corrupt file), is reported in skipped_clips with why -- never silently dropped and never
  treated as failing the whole call. Read-only. 8 new TDD tests (tests/find_by_loudness.rs, synthetic
  tone WAVs at known dBFS levels, same technique as the existing loudness_ext.rs): max_lufs finds the
  quiet clip and excludes the loud one, min+max together bound a range, no range given returns every
  clip unfiltered, non-media clips are never candidates, a too-short clip is skipped not matched or
  failed, track_id scoping, min>max errors, read-only (byte-identical project, no undo entry).
  golden_registry.txt hand-updated (501 tools), toolgroups.rs updated (scopes group, next to
  measure_loudness). All green including golden_frames_registry_and_projects.
- [x] `find_assets` grows into the shot list: checked first and found `find_assets` already returns
  asset_id/path/bin/width/height/duration/fps (would have duplicated a new `asset_info` tool). The real
  gaps: `kind`/`hash`/`origin` already sit on the `Asset` model but were never surfaced; nothing counts
  how many timeline clips use an asset (a reverse lookup, new `used_by_clips`, scanning every track);
  `has_audio` isn't a stored fact at all (an opt-in `probe_audio` flag now reports it via one ffprobe per
  matched asset, reusing `crate::audio::source_info` rather than a second ffprobe call site -- default
  false so an ordinary find_assets call spends nothing extra). Backward compatible: every existing field
  and caller (bins_subclips_colors.rs) is untouched, new fields are additive. 5 new TDD tests
  (tests/find_assets_shot_list_v72.rs): kind/hash/origin ride along, used_by_clips counts correctly
  across two tracks (0 before placement), has_audio is null by default and populated true/false per
  asset with probe_audio (silent vs sine-tone fixtures, same ffmpeg-lavfi technique as
  bins_subclips_colors.rs's own fixture()), existing bin/name_glob filters unaffected, read-only.
  golden_registry.txt hand-updated (find_assets' grown schema+description hash, still 501 tools -- no
  new tool name this time). All green including golden_frames_registry_and_projects and the existing
  bins_subclips_colors.rs (8 tests, unaffected).
- [x] `self_review`: new tool, one-call orchestration of `lint_edit` + `validate_timeline` +
  `list_dependencies` (missing files only), correlating every source's flagged moments into one merged
  `flagged_times` list instead of calling all three and matching times up by hand. `path` (optional) also
  renders a `contact_sheet` of every flagged moment (capped at 16, contact_sheet's own limit) so the
  report comes with evidence, not just numbers. Pure composition through `dispatch_nested` -- no new
  checking logic, no new dispatch path; `lint_edit` erroring on an empty timeline is reported, not
  treated as a self_review failure. Deliberately NOT auto-fix: `lint_edit`'s own problems already carry
  the exact fix call each needs, and applying them unasked is a mutation this tool does not take -- the
  north star's "self-review loops: render a check, lint it, auto-fix" stops at render+lint here, on
  purpose; auto-fix stays a deliberate separate step. 8 new TDD tests (tests/self_review.rs): a gap from
  validate_timeline shows up in the merged report and flagged_times, an empty-timeline lint_edit error is
  reported not thrown, missing media from list_dependencies flips ok false, no path skips rendering, path
  given renders a sheet when something is flagged and skips it when nothing is, `ok` is exactly the AND
  of the three sources (not asserting lint_edit finds zero problems on a real fixture -- its loudness/
  dark-frame thresholds are lint_edit's own business), read-only. golden_registry.txt hand-updated (502
  tools), toolgroups.rs updated (output group). All green including golden_frames_registry_and_projects;
  director_small's lint_edit timing test hit its 9.0s ceiling once under concurrent load and passed clean
  alone (12.49s single-threaded) -- the pre-existing documented flake, not a regression from this tool.
- [x] `explain_cut`: new BETA, best-effort tool -- why a clip's incoming cut might exist. No new
  detection logic: reads the clip's own `transition_in`, correlates with timeline markers within
  `window_seconds` (default 0.5s) of the boundary (`add_marker`/`mark_scene_cuts`), whether it's the
  same source asset as the clip before it (a split within one shot vs. a cut to new footage), and asks
  `detect_cuts` (via `dispatch_nested`, a small window around the exact boundary) whether the footage
  itself shows a real visual change there -- a signal the composition can't get any other way without
  running its own scoring. Built for an agent with no eye on the monitor: it can't glance at a cut and
  judge it the way a human editor would, so this hands it the same evidence a human would have used,
  phrased as a plain-English `explanation`, and says so explicitly when the evidence runs out (no
  transition, no marker, no conclusive detect_cuts read) rather than inventing a reason. Explicitly not
  a real decision ledger (north star: "explainable edits") -- building one would mean threading
  provenance through every mutating tool, out of scope for this pass; this reads what's already on the
  timeline instead. The first clip on a track reports `is_first_on_track` rather than guessing at a
  nonexistent predecessor. 11 new TDD tests (tests/explain_cut.rs): first-clip case, unknown clip_id
  errors plainly, an explicit transition is named in the explanation, same-asset-as-previous true/false,
  a marker inside the window is reported and named in the explanation (and excluded when the window is
  tightened past it), a real colour change across the cut IS detected by detect_cuts, an identical-
  colour cut (zero visual change) is correctly reported as NOT detected, gap_before_s is reported when
  the previous clip doesn't touch, read-only. golden_registry.txt hand-updated (503 tools), toolgroups.rs
  updated (output group). All green including golden_frames_registry_and_projects.
- [x] `review_export` grows QC, corrected scope after checking first: `review_export` already exists
  and already reports duration/size/width/height/LUFS/peak plus an inline contact sheet of an exported
  file (a planned new `export_qc` tool would have duplicated nearly all of it). The real gaps: no codec
  names (added `video_codec`/`audio_codec`, the same ffprobe call, two more fields) and nothing compared
  the file against what the LIVE PROJECT should have produced -- `review_export` told you everything
  ABOUT a file, nothing told you whether it was the RIGHT file. New opt-in `expect_project` (default
  false) computes the project's own expected duration (same max-clip-end technique `lint_edit` uses) and
  canvas size, returning `qc.duration_match`/`qc.resolution_match` with a small tolerance (0.2s, not
  exact-equality brittle); an empty project reports `checked: false` rather than a false mismatch.
  Backward compatible: existing fields and the existing caller (director_small.rs's
  `review_export_shows_the_exported_file`) are untouched. 7 new TDD tests
  (tests/review_export_qc_v72.rs): codec names ride along, expect_project off by default, a real
  export matches its own project, extending the project after export is caught as a duration mismatch,
  changing canvas size after export is caught as a resolution mismatch (duration unaffected), an empty
  project reports no expectation rather than a false failure, the existing missing-file error is
  unchanged. golden_registry.txt hand-updated (review_export's grown hash, still 502 tools). All green
  including golden_frames_registry_and_projects and the existing director_small.rs (29 tests, including
  the lint_edit timing test clean this run with no concurrent load).

Not built this pass (noted honestly): a true cut-provenance ledger (every mutating tool tagging its own
clips with why), and EDL/OTIO import (out-of-kimo edit decision lists) -- both real but bigger lifts than
the time box here; `project_merge`/`batch`/`project_diff` already cover kimo's own interchange needs.


## v7.2 feature 8/8: check_color_consistency

- [x] `check_color_consistency`: new tool, the read-only diagnosis `match_color` (one clip) and
  `match_shots` (every clip, mutating, uniform) have no counterpart for -- which clips in a group
  actually disagree in brightness/color BEFORE an agent reaches for either fix, and whether a fix
  afterward actually closed the gap (re-run the same call). No new pixel analysis: samples each clip's
  own midpoint timeline time through the existing `get_scopes` (`dispatch_nested`) and compares
  `luma_mean`/`avg_rgb` against either a named `ref_clip_id` (the hero shot `match_shots` targets) or,
  by default, the checked group's own median -- so a single genuinely odd clip gets flagged without
  needing a hero named first. `clip_ids` (optional) scopes the check to an explicit list of any clip
  type; the default set is every `Source::Media` clip on the timeline (a solid/title card isn't a "shot"
  with continuity to check). `luma_tolerance` (default 0.08) / `rgb_tolerance` (default 0.06, max per
  channel) are adjustable. An empty check reports `ok: true, checked: 0` rather than erroring; an unknown
  `ref_clip_id` errors plainly. Read-only (`get_scopes` is read-only; this adds no mutation). 9 new TDD
  tests (tests/check_color_consistency.rs): no clips is ok-empty not an error, three matching clips
  agree by default, one odd clip out of four is flagged and the other three are not, `ref_clip_id`
  compares everyone against the named hero shot, an unknown ref_clip_id errors plainly, explicit
  `clip_ids` scopes the check, solid-color generator clips are excluded from the default set, both
  tolerances are adjustable (loose finds nothing, tight finds the real gap), read-only. golden_registry.txt
  hand-updated (504 tools), toolgroups.rs updated (`color_grade`, beside `match_shots`). Full `--lib`
  suite green (372 passed, 12 ignored, pre-existing) plus the targeted suite (golden_frames_registry_and_
  projects, description_first_sentence, external_refs) all green.


## v7.3 additions

Naman (12:43 PM IST Oct 30): "make ai agent a complete editor", "better than any editor in the world".
Audit before building (nothing below duplicates an existing tool; v7.2's audit habit continued):
batch_normalize_loudness is new (normalize_loudness existed but only one clip at a time -- find_by_loudness's
own v7.2 precedent, applied to the fix side); apply_review_fixes is new (self_review documented but
deliberately did not build "apply the fix" -- this is that step, built directly against lint_edit, the
tool whose problems actually carry `fix` payloads); auto_chapters is new (mux_chapters existed but required
markers to already exist -- this composes mark_scene_cuts + mux_chapters so "no markers yet" isn't a
precondition the agent has to notice and clear itself); the GUI's keyboard shortcuts, split-at-playhead,
track add/remove, marker display, snapping, zoom, project open/save and export-with-progress are new (the
v7.1/v7.2 `kimo gui` had none of them -- FEATURES.md's own honest gaps list named several). import_edl is
new (the v7.2 audit explicitly flagged EDL/OTIO import as a real gap, bigger than that pass's time box).

## v7.3 feature 1/N: apply_review_fixes

- [x] `apply_review_fixes`: new tool, the "apply the fix" step self_review's own v7.2 writeup deliberately
  left undone ("lint_edit's problems already carry the exact fix call each needs; applying them is a
  deliberate separate step, not this tool's"). Runs `lint_edit` itself, and for every problem whose `fix`
  is a concrete, fully-specified tool call -- no human-fill-in placeholder value (lint_edit's own
  convention: angle-bracketed prose like `"<the text without those characters>"`) -- dispatches it,
  including a chained `then` step (e.g. `move_clip` then `trim_clip` for `broll_over_silence`) when the
  primary call succeeds. A problem whose fix needs a human judgment call (a placeholder, an `"or"`
  alternative, or no fix at all) is reported under `skipped_manual` with why, never guessed at. `codes`
  (optional) scopes which problem codes to act on; everything else is left alone entirely, not even listed.
  `dry_run` reports exactly what would be applied/skipped (`would_call`) without mutating anything -- the
  documented way to check first. One fix failing (e.g. a clip an earlier fix in the same batch already
  removed) is recorded in `failed` and does not stop the rest. Read path is `lint_edit` (unchanged);
  nothing about `lint_edit` or `self_review` was touched -- pure composition through `dispatch_nested`,
  the same pattern `mark_scene_cuts`/`self_review` already established. 5 new TDD tests
  (tests/apply_review_fixes.rs): a concrete fix (`transition_from_black` -> `remove_transition`) is
  actually applied and the transition is verified gone from the project, not just reported; `dry_run`
  reports the exact call and mutates nothing; a placeholder fix (`missing_glyph` -> `set_text` with a
  fill-in-the-blank value) is skipped as manual and the clip's text is verified untouched (no placeholder
  string ever dispatched); `codes` scopes which problems are even considered; an empty timeline is `ok`
  with `problems_seen: 0`, not an error. golden_registry.txt hand-updated (504 -> 505 tools), toolgroups.rs
  updated (`output` group, beside `self_review`).


## v7.3 feature 2/N: auto_chapters

- [x] `auto_chapters`: new tool, closes the precondition gap the v7.2 audit named: `mux_chapters` needs at
  least 2 timeline markers to already exist, so an agent starting from a bare timeline had to notice that
  itself, call `mark_scene_cuts`, then call `mux_chapters` -- three separate decisions to do one obvious
  thing ("chapter this export"). `auto_chapters` composes them: if the timeline already has `min_markers`
  (default 2, mux_chapters' own minimum) markers, it chapters straight from them, no detection run, nothing
  added; otherwise it runs `mark_scene_cuts` (source timeline) first to create them, then chapters from the
  result. If even scene-cut detection does not reach `min_markers` (a single continuous shot with no
  detectable cuts), it returns `ok:false, chaptered:false` with why -- never guesses a chapter boundary or
  fails loudly from inside `mux_chapters`. `sensitivity` passes through to `mark_scene_cuts` when it runs;
  `dry_run` reports whether it would use existing markers or run detection, and whether chaptering would
  succeed, without creating a marker or touching the exported file. Pure composition through
  `dispatch_nested` (same pattern `mark_scene_cuts`/`self_review` already established) -- no new marker or
  chapter logic. 4 new TDD tests (tests/auto_chapters.rs): no markers yet -> scene-cut detection runs and
  the file gets chaptered, markers verified to actually exist afterward; markers already present ->
  detection verified NOT to run (`scene_cut_detection.ran: false`) and marker count verified unchanged;
  `dry_run` verified to create no markers and leave the exported file's mtime untouched; a single
  continuous shot (no detectable cuts) reports `ok:false` plainly rather than erroring. golden_registry.txt
  hand-updated (505 -> 506 tools), toolgroups.rs updated (`output` group, beside `mux_chapters`).


## v7.3 feature 3/N: batch_normalize_loudness

- [x] `batch_normalize_loudness`: new tool, the write-side counterpart to `find_by_loudness` (v7.2):
  `normalize_loudness` (pre-v7.3) only ever took one `clip_id` -- hitting a target LUFS across many/all
  clips meant calling it once per clip and correlating results by hand, the exact gap `find_by_loudness`
  closed on the read side ("all clips under -20 LUFS" was the north star's own example; this is "normalize
  every clip to -16 LUFS"). Normalizes every media clip (or an explicit `clip_ids` list, or one `track_id`)
  to `target_lufs` (default -14) in one call. `tolerance_db` (default 0.5) leaves a clip already close
  enough to target untouched (`already_on_target`, no redundant gain change) -- the same "don't pile up
  no-op writes" instinct `mark_scene_cuts`' `skip_existing` already established. Reuses `measure_loudness`
  and `normalize_loudness` per clip through `dispatch_nested` (no second gain-staging implementation), so
  results match calling `normalize_loudness` directly, clip by clip. A clip too short/silent for a reading,
  or one `normalize_loudness` itself refuses (over 40dB gain, or would clip above -1 dBFS), is reported in
  `skipped`/`failed` with why and does not stop the rest. `dry_run` reports `measured_lufs`/`would_gain_db`
  per clip without mutating anything. 6 new TDD tests (tests/batch_normalize_loudness.rs, synthetic tone
  WAVs, same technique `find_by_loudness.rs` uses): a loud and a quiet clip both land on target, verified
  by the actual linear gain written onto each clip's `audio.volume` (re-measuring doesn't reflect it --
  `measure_loudness` reads raw decoded PCM, `normalize_loudness`'s own documented "remeasure timeline"
  caveat -- so the test checks the real mechanism, not a measurement that can't see it); a clip already
  within tolerance is left alone and its volume prop verified unchanged; `dry_run` verified to mutate
  nothing; `clip_ids` scopes to an explicit list and an unknown id errors; a too-short clip is skipped, not
  a failure; `track_id` scopes the batch. golden_registry.txt hand-updated (506 -> 507 tools), toolgroups.rs
  updated (`audio` group, beside `normalize_loudness`).


## v7.3 feature 4/N: kimo gui keyboard shortcuts

- [x] `kimo gui` keyboard shortcuts: space play/pause, J/K/L (step back one frame / pause / step forward
  one frame -- this is a frame-stepped preview, not continuous video playback, so J/L step rather than
  shuttle at speed), Delete/Backspace removes the selected clip, S splits it at the playhead, Ctrl/Cmd+Z
  undoes, Ctrl/Cmd+Shift+Z redoes. Was a real, named gap: v7.1/v7.2's "kimo gui" had no keyboard handling
  at all (FEATURES.md's own honest-gaps list for the GUI never even got that far). The key-to-action
  mapping is its own pure function, `gui_keymap.js` (`resolveKeyAction(event) -> action name | null`, no
  DOM) -- served as a real static file at `GET /gui_keymap.js` so the browser's
  `<script src="/gui_keymap.js">` and a headless Node test run the exact same bytes; there is one mapping,
  not two to keep in sync, and typing in an input/textarea/select never fires a shortcut (space in the
  tool-filter box types a space, not play/pause). `gui_index.html`'s keydown listener is thin DOM glue:
  it calls `resolveKeyAction` and dispatches to the existing `togglePlay`/`doUndo`/`doRedo` and two new
  small functions, `deleteSelectedClip()` and `splitAtPlayhead()`, both plain `/api/call` round trips
  through `delete_clip`/`split_clip` (already-registered engine tools; nothing new in kimo-core for this
  feature) followed by a timeline/inspector refresh, the same shape the move/trim drag and undo/redo
  buttons already use. 18 new headless Node tests (tests/js/keymap_test.js, no browser, no DOM -- every
  key combo this feature claims, plus the "ignored while typing in a field" cases) + 6 new Rust tests
  (crates/kimo-cli/src/gui.rs): `/gui_keymap.js` is served with the right content-type and really is the
  requireable module (405 on non-GET); the index page is checked to actually wire up the listener and
  reference every new function by name; delete's own API sequence actually removes the clip from the
  project (and a stale id reports a tool error, not a crash); split's own API sequence actually produces
  two clips at the right boundary (and an out-of-range playhead errors plainly, the path `splitAtPlayhead`'s
  `alert()` surfaces); undo/redo's shortcuts are confirmed to land on the same history tools the existing
  buttons already use and actually move the project back and forth. All 24 pre-existing kimo-cli tests
  stayed green.


## v7.3 feature 5/N: kimo gui add/remove tracks

- [x] `kimo gui`: a "+ Track" button beside the Timeline heading adds an empty track (`add_track`, no new
  args, same tool the registry already has) and refreshes; each track's label row grows a small "×"
  button (with a confirm() prompt, since it deletes the track's clips too) that calls the already-registered
  `delete_track`, clears the clip selection if it pointed into that track, and refreshes the timeline and
  inspector. Both are plain `/api/call` round trips through existing engine tools -- nothing new in
  kimo-core for this feature, the same "GUI is a thin client over the registry" shape every prior `kimo gui`
  feature has used. 3 new Rust tests (crates/kimo-cli/src/gui.rs): the index page is checked to actually
  carry the button, the onclick wiring and both new JS functions by name; add_track's own API sequence is
  confirmed to grow `track_count` and the new track starts with zero clips; delete_track's own API sequence
  is confirmed to actually remove both the track AND its clip (not just the track row), and a stale track id
  (a double-click after it's already gone) reports a tool error rather than crashing. All 24 pre-existing
  kimo-cli tests stayed green (27 total now).


## v7.3 feature 6/N: kimo gui marker display + add

- [x] `kimo gui`: `timeline_view_model` now also returns `markers` (id, time, duration, name, color, sorted
  by time -- the same order `list_markers` already returns, not a second source of truth) so the timeline
  can actually show them: each marker renders as a thin coloured tick across the full track height at its
  time, titled with its name and time, clicking it removes it (confirm()-guarded) through the
  already-registered `remove_marker`. A "+ Marker" button beside "+ Track" adds one at the current playhead
  time via the already-registered `add_marker` (optionally prompting for a name). Every mutation is a plain
  `/api/call` round trip through existing engine tools, same shape as every prior `kimo gui` feature; nothing
  new in kimo-core. 5 new Rust tests (crates/kimo-cli/src/gui.rs): `timeline_view_model` lists markers sorted
  by time (added out of order, read back in order) and returns an empty array rather than omitting the key
  when there are none; the index page is checked to actually carry the button, the tick rendering and both
  new JS functions by name; add_marker's own API sequence is confirmed to land the marker at the given time
  with its name; remove_marker's own API sequence is confirmed to actually remove it (and a stale marker id
  reports a tool error, not a crash). All 27 pre-existing kimo-cli tests stayed green (32 total now).


## v7.3 feature 7/N: kimo gui timeline snapping

- [x] `kimo gui`: drag-to-move and drag-to-trim now snap to nearby edit points -- the exact honest gap
  v7.1's own FEATURES.md entry named ("no snapping to other clips/markers"). The snap decision is its own
  pure function, `gui_snap.js` (`snapTime(proposed, candidates, toleranceSec)` -- nearest candidate within
  tolerance, else the proposed value unchanged; `candidatesFromTimeline(tl, excludeClipId)` gathers every
  clip edge on every track, every marker time, and 0 from the already-loaded `/api/timeline` view-model,
  the same candidate set `snap_time`'s own description already names -- "clip edges, markers, playhead 0"),
  served as a real static file at `GET /gui_snap.js` so the browser and a headless Node test run the exact
  same bytes. Candidates are gathered once per drag from the view-model already in memory (`lastTimeline`,
  cached by `loadTimeline()`), not fetched mid-drag, so snapping costs no extra API round trip and updates
  live as the mouse moves. A "snap" checkbox beside "+ Track"/"+ Marker" toggles it (default on); tolerance
  is 8px converted to seconds via the current zoom (`pxPerSec`), so it stays a constant screen distance at
  any zoom level. 10 new headless Node tests (tests/js/snap_test.js): nearest-candidate selection within and
  outside tolerance, an empty candidate list, tie-breaking, exact matches, snapping to 0, and
  `candidatesFromTimeline` gathering every edge/marker/0 while excluding the dragged clip's own edges (so a
  clip never snaps to itself) -- plus 5 new Rust tests (crates/kimo-cli/src/gui.rs): the JS file is served
  correctly (405 on non-GET); the index page is checked to wire up the checkbox and both snap functions by
  name; and the actual payoff -- a clip moved with an already-snapped value lands its edge EXACTLY flush
  against its neighbour (not a near-miss), confirmed through the real `move_clip` call and a re-read of the
  timeline. All 31 pre-existing kimo-cli tests stayed green (36 total now).


## v7.3 feature 8/N: kimo gui timeline zoom

- [x] `kimo gui`: timeline zoom in/out/reset, bounds-clamped 4-400px/sec (default 40). The clamp and step
  math is a pure function, `gui_zoom.js` (`clampZoom(v)`, `zoomBy(current, factor)` -- 2x for "+", 0.5x for
  "−", rounded to a whole pixel and bounded), served as a real static file at `GET /gui_zoom.js` so the
  browser and a headless Node test run the exact same bytes; `pxPerSec` (previously a hardcoded const used
  everywhere clip bars, marker ticks, the playhead and the scrub-to-pixel conversion are positioned) is now
  a mutable module variable driven entirely by this helper, so zooming re-renders the whole timeline at the
  new scale with no special-casing anywhere else. "−" / a live "NNpx/s" label / "+" / "reset" sit beside
  the snap checkbox. 10 new headless Node tests (tests/js/zoom_test.js): default value, doubling/halving,
  clamping at both ends, clamped-and-stays-clamped under repeated zoom, and rounding -- plus 3 new Rust
  tests (crates/kimo-cli/src/gui.rs): the JS file is served correctly (405 on non-GET) and the index page is
  checked to wire up all four controls and the shared helper by name. All 36 pre-existing kimo-cli tests
  stayed green (39 total now).


## v7.3 feature 9: kimo gui export with progress

- [x] `kimo gui`: an Export panel -- output path field, Export button, a live progress bar, status line
  (state, chunks done/total, ETA with its uncertainty band, any error), and a Cancel button that appears
  only while a job is active. Pure wiring onto the chunked-export system that already existed
  (`export_job_start`/`job_status`/`cancel_job`/`resume_job` -- incremental reuse, crash-safe chunking, ETA
  from measured chunk times): `startExport()` calls `export_job_start` with `wait:false` and starts polling
  `job_status` every 500ms; `renderExportStatus()` turns each poll into the bar's width (chunks done/total,
  falling back to frames done/total before any chunk has measured) and the status line's words; polling
  stops itself once the job reaches done/failed/cancelled; `cancelExport()` calls the already-registered
  `cancel_job`. Nothing new in kimo-core -- every piece this wires onto was already tested exhaustively
  there (tests/export_job*.rs). 3 new Rust tests (crates/kimo-cli/src/gui.rs): the index page is checked to
  wire up the panel and all three functions by name; a real export started through `/api/call` is polled
  through real `job_status` calls to a real "done" state with the file actually on disk; a real running job
  is actually cancelled through `/api/call` and reaches cancelled-or-done. All 39 pre-existing kimo-cli
  tests stayed green (42 total now).


## v7.3 feature 10: kimo gui project open/save

- [x] `kimo gui`: a path field in the header with Save/Open buttons and a status line. Pure wiring onto
  `save_project`/`load_project`, which already existed (atomic temp-file-then-rename writes, JSON schema
  migration on load, optional history persistence) -- `saveProject()` and `openProject()` are plain
  `/api/call` round trips, same shape as every other button; a successful open clears the clip selection and
  re-renders the timeline and history list so the GUI actually reflects the newly loaded project, not a
  stale view of the old one. Nothing new in kimo-core. 4 new Rust tests (crates/kimo-cli/src/gui.rs): the
  index page is checked to wire up the field, both buttons and both functions by name; Save actually writes
  a real project JSON file to disk (read back and checked for a `tracks` key); Open actually replaces the
  live session's project with a previously-saved one (save 1 track, add a 2nd live, open, confirm back to 1
  -- not just "no error"); opening a path that does not exist comes back as a clean tool error, not a crash.
  All 42 pre-existing kimo-cli tests stayed green (46 total now).


## v7.3 feature 11: import_edl -- CMX3600 EDL / OTIO timeline import

- [x] `import_edl` -- the real editorial-interchange gap flagged after the v7.3 audit: kimo could export
  EDL-adjacent things but had no way to bring a cut list IN. Takes a standard CMX3600 EDL (the
  lowest-common-denominator format every NLE speaks) or a simple single-video-track OTIO JSON timeline and
  creates real clips on the project's timeline. Format auto-detects from content (valid JSON with an
  OTIO_SCHEMA/tracks field -> otio, else cmx3600) or is set explicitly. Each event's record in/out places
  and sizes the resulting clip; its source in point offsets into the media. Source resolution: an EDL's `*
  FROM CLIP NAME:` comment (CMX3600 convention: it follows the event it names, not precedes it) names the
  source when present, else the reel field; an OTIO clip uses its own name or media_reference.target_url.
  Each name is resolved to a real file by `media_map` (exact name -> path) first, then a `media_dir` scan
  (exact filename, else case-insensitive stem match), then the name itself if it is already a real path --
  anything that resolves to nothing is reported under `unresolved` with its record range rather than
  silently dropped or failing the whole import (the same honest-gap reporting collect_project/relink_media
  already use for missing media). CMX3600: only V/B (video) events become clips; dissolve/wipe transitions
  import as a cut at the same record range (no transition-duration concept on this timeline, stated as a
  limitation). OTIO: the first Video track under tracks.children; a Gap advances position without a clip,
  anything else (nested stacks, transitions) is skipped, a real stated limitation for a first pass.
  `track_id` places onto an existing track; omitted creates a new one. `dry_run` reports
  events_total/imported/unresolved and the exact would-be clips without creating anything. Composes
  `import_media` (deduped by resolved path, reusing an already-imported asset) + `add_clip` through
  dispatch_nested -- no new placement logic, the same primitives every other add-clip path uses. 9 TDD tests
  in `tests/import_edl.rs`: two-event CMX3600 import at correct record positions, source in point from the
  EDL's own timecode (not 0), an unresolvable name reported not dropped/failed, media_map taking priority
  over media_dir, dry_run touching nothing, an explicit track_id reused instead of a new track, an OTIO
  timeline with a Gap landing the second clip at the right position, format auto-detection from content with
  a non-.otio extension, and a missing file as a clean error. Registered in the `project` toolgroup. Tools
  507->508.

## v7.4 additions

Naman (5:26 PM IST Oct 30): "keep working, and keep sending updates". Audit before building, same habit as
v7.2/v7.3 (nothing below duplicates an existing tool):

Already covered, NOT duplicated: close_gaps/ripple delete (ripple_delete already existed, pre-v7);
batch caption styling (set_caption_style already applies a style preset to every cue in a group in one call,
pre-v7); timeline diff between two project versions (project_diff already exists, pre-v7, revision-to-revision);
a one-call rough cut already exists in two forms (auto_rough_cut for talking-head assets, transcript_edit's
machinery for transcript-driven cuts) so a third generic version would be a fourth way to do the same job,
not a gap.

Real gaps found and built this pass:
- [x] export_edl -- feature 2: EDL/OTIO/FCPXML EXPORT. v7.3 built import_edl (bring a cut list in) and
  explicitly left export for later; this is that other half, so an edit can leave kimo for another NLE
  (or come back from one).
- [x] track_mute_solo -- feature 1: not from the audit's own direction list, but a real pre-existing bug
  found while reading the audio path for this pass -- Track.muted has existed since before v7 and every
  audio-render/analysis path already honoured it, but no tool could ever set it. Fixed, plus the standard
  NLE companion (solo).
- [x] reframe_timeline -- feature 3: auto-reframe (v7.2) is a single-clip, export-time-only operation
  (export_variants applies and then removes it per variant); this is the persistent, undoable
  whole-timeline aspect conversion that actually changes the canvas and keys every clip.
- [x] project_template -- feature 4: starter project templates (canvas size + fps + track layout
  conventions for vertical/square social, 16:9 widescreen, podcast interview, multicam panel) so a new
  project doesn't start from a blank canvas every time. Not template_define/apply (clip/op templates).
- [x] kimo gui: multi-select + group move/delete -- feature 6
- [ ] kimo gui: copy/paste clips
- [ ] kimo gui: ripple-delete in GUI (wire the existing ripple_delete tool to a shortcut/button distinct from
  plain delete)
- [x] kimo gui: track mute/solo/lock controls -- feature 5, now that track_mute_solo exists to wire to
- [x] kimo gui: audio waveforms on clips -- feature 7
- [x] kimo gui: a visible effects/transitions browser applying to the selected clip -- feature 8

## v7.4 feature 1/8: track_mute_solo

- [x] `set_track` can now actually set `muted` (it never could before -- schema only took locked/name/index,
  even though Track.muted has existed since before v7 and every audio-render and analysis path already
  honoured it; a track could never be muted by any tool). Plus the standard NLE companion, `solo`
  (Premiere/Resolve/Reaper semantics: when any track is soloed, every non-soloed track is silent regardless
  of its own muted flag; with nothing soloed, a track's own muted flag is the whole answer; soloing a track
  that is also marked muted still plays -- solo wins). `effective_mute(tracks, track)` in model.rs centralises
  the rule; `audio::mix`/`mix_strict` (the real rendered-audio paths) now use it. 9 TDD tests including a
  real ffmpeg-generated tone that actually disappears from the mix once its track is muted, not just a flag
  check, and an old project JSON with neither key still loading with both false. No new tool; set_track
  extended (its description's first sentence kept under the 150-char contract). Tools: 508 (unchanged).

## v7.4 feature 2/8: export_edl -- CMX3600 EDL / OTIO / FCPXML timeline EXPORT

- [x] The write-side counterpart to v7.3's `import_edl`. Exports the current timeline (or a named track) as
  a CMX3600 EDL, a simple single-video-track OTIO JSON timeline (the same shape import_edl's own parser
  reads, so export then import_edl round-trips), or a minimal but real FCPXML project (resources + a
  sequence/spine of frame-exact asset-clips) -- the three interchange formats a v7.2/v7.3-generation NLE
  audit keeps naming. Only media clips are representable on a cut list; a non-media clip on the exported
  track is reported under `skipped_non_media`, never silently dropped. A clip at a non-1.0 speed has no
  universal cross-format representation kimo's model carries, so its source range covers the actual footage
  consumed and it is listed under `approximated_speed` -- a real, stated limitation rather than a silently
  wrong number. `track_id` is required only when more than one track actually has media clips (the error
  names every candidate and its clip count). 10 TDD tests: CMX3600 timecodes and FROM CLIP NAME comments,
  a full export-then-reimport round trip for both CMX3600 and OTIO (via import_edl's own dry_run), OTIO Gap
  preservation between clips, FCPXML parsed back with roxmltree and checked for frame-exact rational times,
  writing straight to a file, non-media skip reporting, and the ambiguous-multi-track error naming both
  tracks by id. Registered in the `project` toolgroup (beside import_edl). Tools 508->509.

## v7.4 feature 3/8: reframe_timeline -- real, persistent whole-timeline aspect conversion

- [x] `auto_reframe` (v7.2) keys one clip. `export_variants` (v7.3) touches every clip but restores the
  canvas and every reframe window right after that one export -- by its own doc comment, "net effect on the
  project is nothing". Neither actually changes the project as a real edit. `reframe_timeline` does: it sets
  `project.width`/`height` for real and keys `auto_reframe` on every media clip in scope (`track_id` restricts
  to one track; default every track), as one ordinary mutating tool call -- one undo step reverses the whole
  conversion, same as any other tool. `aspect` takes the same small named-preset table export_variants uses
  (16:9/9:16/1:1/4:5), or give `width`+`height` directly. `auto_reframe: false` resizes the canvas only and
  leaves every clip's existing fit (contain/cover/stretch) alone. A clip that already has an active reframe
  window (set by hand, or by an earlier call) is left alone and listed under `skipped_already_framed` unless
  `overwrite_existing` -- a manually-framed shot is never silently clobbered. `dry_run` reports
  `would_reframe`/`skipped_already_framed` without touching anything. 11 TDD tests: the canvas change sticks
  (not restored like export_variants), undo reverses the whole conversion including every clip's reframe
  window in one step, `auto_reframe: false` leaves clip fit alone, a manually-framed clip is protected unless
  `overwrite_existing`, dry_run touches nothing, an already-matching aspect reframes nothing, explicit
  width/height without a named aspect, non-media clips are never touched, `track_id` restricts scope, and two
  clean-error cases (unknown aspect name, no size given at all). Registered in the `timeline` toolgroup
  (beside `ripple_delete`/`auto_reframe`). Tools 509->510.

## v7.4 feature 4/8: project_template -- starter project templates

- [x] `list_project_templates` / `create_project_from_template`. Every project before this started from a
  blank canvas: `project_set` takes width/height/fps/name but creates no tracks, so "set up a vertical
  short" or "set up a two-guest podcast" meant the agent had to already know (or guess) the right canvas
  size and then call `add_track` by hand, once per track, in the right order, every time. Five templates,
  each with a stated rationale for its track layout: `vertical_social` (1080x1920, Video/Captions/Music),
  `square_social` (1080x1080, same convention), `widescreen_16_9` (1920x1080, A-Roll/B-Roll/Music/SFX --
  the A/B split matches `place_broll`'s own expectation of a dedicated b-roll track), `podcast_interview`
  (1920x1080, Host/Guest/Screen Share/Music -- separate speaker tracks for `auto_rough_cut`'s per-speaker
  dead-air detection), `multicam_panel` (1920x1080, Cam A/B/C/Screen Share/Music -- a ready-made track set
  for `add_multicam`/`switch_angle`). `create_project_from_template` refuses to touch a project that already
  has tracks unless `overwrite: true`, so it can never silently wipe real work; applying a template is one
  ordinary mutating call (one undo step). Not to be confused with the pre-existing `template_define`/
  `template_apply` (a named, parameterised op-list applied to a clip -- a completely different thing this
  audit made sure not to duplicate). 7 TDD tests: every template is listed with canvas/fps/tracks/rationale,
  applying one sets canvas+fps and creates named empty tracks in the right order, overwrite protection
  refuses without the flag and works with it, an unknown template name is a clean error naming the valid
  ones, the whole application is one undo step, and every template in the catalog is internally consistent
  (its own reported track count matches what it actually created). Registered in the `project` toolgroup.
  Tools 510->512.

## v7.4 feature 5/8: kimo gui track mute/solo/lock controls

- [x] `kimo gui`: each track row gets three small toggle buttons -- M (mute), S (solo), L (lock) -- styled
  distinctly when active (mute tints red-brown, solo amber, lock red), wired straight onto `set_track`
  (feature 1). A locked track's clip bars stop accepting drag/trim (dimmed, "track is locked" in their
  title) rather than letting a drag run and failing only on mouseup. The button/interaction logic is pure
  JS, `gui_trackstate.js` (`buttonState(track)` -> each button's label/active/title, `clipInteractionAllowed
  (track)` -> whether drag/trim should start, `toggleArgs(track, which)` -> the exact one-key set_track
  args to send), served at `GET /gui_trackstate.js` so the browser and `tests/js/trackstate_test.js` run the
  exact same bytes (v7.3's pattern: pure logic as a real file, not duplicated in two places). 13 new headless
  Node tests plus 4 new Rust tests (`crates/kimo-cli/src/gui.rs`): the JS file is served byte-identical to
  disk, the index page wires up the file and all three functions by name, `timeline_view_model` carries
  locked/muted/solo per track, and a real `set_track` call through `/api/call` is reflected in the very next
  `/api/timeline` fetch. All 46 pre-existing kimo-cli tests stayed green (50 total now).

## v7.4 feature 6/8: kimo gui multi-select and group move/delete

- [x] `kimo gui`: shift/cmd/ctrl-click adds or removes a clip from a multi-selection (a plain click still
  replaces the selection with just that one clip, or deselects it if it was already the lone selection);
  every selected clip gets the existing `.selected` highlight. Dragging any one clip that is part of a
  2+ selection moves the whole group together, un-snapped, clamped only so the earliest clip in the group
  can never be dragged to a negative start -- sent on mouseup as ONE atomic `batch` call (one undo step for
  the whole group, not one per clip). Delete with 2+ selected removes every selected clip as one atomic
  `batch` call too. Trim stays single-clip only -- there is no settled meaning yet for "trim a group" the
  way there is for "move a group", and this is stated rather than guessed at.
- `gui_multiselect.js`: pure JS, no DOM -- `toggleSelect(selected, clipId, additive)`, `isSelected(selected,
  clipId)`, `clampGroupDelta(starts, deltaSec)` (bounds a leftward drag to the earliest clip's own start),
  `buildGroupMoveOps`/`buildGroupDeleteOps` (the exact `batch` op lists). Served at `GET
  /gui_multiselect.js`, same file the browser and the Node test run.

19 new headless Node tests (`tests/js/multiselect_test.js`) plus 5 new Rust tests: the JS file served
byte-identical to disk, the index page wires up the file and its functions by name, a real group-move batch
through `/api/call` moves exactly the selected clips and leaves the rest, that move is one undo step for the
whole group, and a group-delete batch removes exactly the selected clips. All 50 pre-existing kimo-cli tests
stayed green (55 total now).

## v7.4 feature 7/8: kimo gui clip waveforms

- [x] `kimo gui`: every media clip bar now shows its real audio waveform, filling the bar behind its name.
  No new peak-computation code -- `audio_transients` already had an `image: "waveform"` option (a
  deterministic, compact SVG built from the real decoded samples) before this feature; this wires the
  timeline renderer onto it rather than building a second copy of the same decode. After the timeline draws,
  one `audio_transients` call per media clip fires in parallel (`target: {clip_id}, image: "waveform"`), and
  each bar's waveform fills in as its own decode finishes. A clip with no audio track (or any other failure)
  just keeps a plain bar -- `audio_transients`' own `no_audio` case, not an error worth surfacing in the GUI.
  Only `source_kind === "media"` clips are asked at all; every other kind (solid, text, shape, ...) is silent
  by construction and would just cost a wasted decode attempt.
- `gui_waveform.js`: pure JS, no DOM, no audio decode -- `shouldFetchWaveform(clip)`, `waveformToolArgs(clip)`
  (the exact `audio_transients` call), `scaleSvgToFill(svgMarkup)` (rewrites the tool's fixed-pixel-width SVG
  to carry a `viewBox` + `preserveAspectRatio="none"` + 100% width/height, so the same markup stretches to
  fill whatever pixel width the clip is drawn at, at whatever timeline zoom). Served at `GET
  /gui_waveform.js`, same bytes the browser and the Node test run.

15 new headless Node tests (`tests/js/waveform_test.js`) plus 5 new Rust tests: the JS file served
byte-identical to disk, the index page wires up the file and its functions by name, a real media clip (an
ffmpeg-generated tone) gets back a real `<svg>...<path>...</svg>` waveform through `/api/call`, a silent
shape clip gets a clean tool error rather than a crash, and `timeline_view_model` reports `media` as the
source kind the GUI keys its fetch decision on. All 55 pre-existing kimo-cli tests stayed green (60 total
now).

## v7.4 feature 8/8: kimo gui effects/transitions browser

- [x] `kimo gui`: the inspector's bare effect-kind `<select>` is now a real searchable browser combining
  BOTH catalogs that exist on the engine side -- `list_effects` (object entries with description/params) and
  `list_transitions` (a bare array of kind strings, a genuinely different shape, not papered over). A filter
  box narrows the combined list by kind or description (so "screen" finds `chroma_key` without knowing its
  exact name); clicking an effect calls `add_effect` with empty params (the existing per-param inspector
  controls set real values afterward, unchanged); clicking a transition calls `add_transition` with a 1s
  default duration. Honest, stated gap: duration is fixed at add time -- there is no `set_transition_param`
  cousin of `set_effect_param` yet to edit it afterward from the GUI, so this is not pretended away.
- `gui_fxbrowser.js`: pure JS, no DOM -- `buildEntries(effectsCatalog, transitionKinds)` (flattens both
  catalogs into one `{type, kind, description}` list), `filterEntries(entries, query)`, `addArgsFor(entry,
  clipId)` (the exact `{tool, args}` call). Served at `GET /gui_fxbrowser.js`, same bytes the browser and the
  Node test run.

12 new headless Node tests (`tests/js/fxbrowser_test.js`) plus 4 new Rust tests: the JS file served
byte-identical to disk, the index page wires up the file and its functions by name, `list_effects` and
`list_transitions` are both reachable through `/api/call` in their two real (different) shapes, and a real
`add_transition` call through `/api/call` actually sets `transition_in` on the clip. All 60 pre-existing
kimo-cli tests stayed green (64 total now). Fixed a filesystem-fixture race along the way: two waveform tests
(feature 7) shared one PID-keyed temp dir name and could stomp each other's in-flight ffmpeg write when run
on parallel test threads -- now carries a per-call counter too.


## v7.5 additions

Audited the registry (512 tools) and FEATURES.md before adding anything. Already covered by existing tools,
not duplicated here: ripple_delete + drop_short_segments (single-clip and short-clip gap closing),
set_caption_style (per-caption styling), auto_rough_cut + transcript_edit (one-call rough cut from
transcript/markers), project_diff (timeline diff between two project revisions), place_broll (b-roll
placement on a dialogue track), auto_duck (ducking one music track under one dialogue track). The real
gaps, candidates for this pass:

- [ ] close_gaps: a general-purpose ripple-close for ANY gap on a track (or every unlocked track at once),
  independent of why the gap is there -- today only a single just-deleted clip's gap (ripple_delete) or a
  short-clip removal's gaps (drop_short_segments) ripple-close; nothing closes gaps already scattered on a
  timeline by manual moves/trims/imports.
- [ ] duck_timeline: one-call ducking across the WHOLE timeline (every music track under every dialogue
  track it overlaps) -- auto_duck today takes exactly one `track`/`under` pair per call.
- [ ] style_captions_batch: apply one style to every caption on a track (or a given list of caption ids) in
  one call -- set_caption_style today takes exactly one caption_id per call.
- [ ] import_clips: copy specific clips (with their full effect/keyframe/mask state, since clips serialize
  everything) from ANOTHER project file into the live project at a chosen track/time -- project_merge
  reconciles revisions of the same lineage by clip id; nothing today does a direct "grab these clips from
  that other project" copy/paste.
- [ ] GUI copy/paste: duplicate the selected clip(s) in place via keyboard (duplicate_clip already does
  this server-side; the GUI has no shortcut or clipboard state for it).
- [ ] GUI ripple delete: a second delete shortcut that closes the gap (ripple_delete) instead of leaving one
  (todays Delete key calls plain delete_clip).
- [ ] GUI marker/chapter list panel: a sidebar listing list_markers with click-to-seek (markers already
  render on the timeline ruler; there is no list view).
- [ ] GUI render queue panel: a panel listing in-flight/finished export jobs (export already reports
  progress and supports cancel; there is no panel showing the queue).
- [~] -> [x] sweep: FEATURES.md [~] rows re-checked against the live registry for anything honestly
  completable on this CPU-only box (no ML runtime, no mic, no GPU); flipped only where a real, tested gap
  closed this pass (see each feature's own entry below for what moved and why).

## v7.5 feature 1/8+: close_gaps

- [x] A general-purpose ripple-close for any gap on a track, or every unlocked track in the project at
  once. `ripple_delete` closes the single gap a just-deleted clip leaves; `drop_short_segments` closes gaps
  left by removing clips under a duration threshold; neither helps a timeline with gaps already scattered
  by manual deletes, trims, moves, or a clip-by-clip import. `close_gaps` walks each named track's clips in
  timeline order and snaps every clip to touch the one before it. `track_ids` omitted sweeps every unlocked
  track in one call ("the whole timeline"); naming a locked track explicitly is an error (nothing silently
  skipped on an explicit ask), while the default sweep instead skips a locked track and reports it in
  `tracks_skipped_locked`, so one protected track does not fail a whole-project sweep. Each track's own
  first clip (by timeline order) is left in place -- a real opening shot, not a stray gap -- unless
  `include_leading_gap` asks to snap it to 0 too. `min_gap` lets a deliberate small pause survive the sweep.
  `dry_run` reports `gaps_closed`/`shifted_seconds`/`moved_clip_ids` per track and changes nothing.
  Deterministic and undoable like any other mutating tool.
- New MCP tool `close_gaps` (`crates/kimo-core/src/ext/close_gaps.rs`). 11 new TDD tests
  (`tests/close_gaps.rs`): single-track gap closing, leading-gap default vs `include_leading_gap`,
  multi-track sweep in one call, locked-track skip-and-report vs locked-track-named-explicitly error,
  `dry_run`, `min_gap` threshold, undo, a no-op on a gap-free track, and an unknown-track-id error.
  `golden_registry.txt` hand-updated for the new tool (512 -> 513 tools). Full targeted pass green:
  `close_gaps`, `golden_frames` (registry+frames+projects, confirms no golden frame/project moved),
  `description_first_sentence`, `external_refs`, `registry_lint`, `registry_unique`.

## v7.5 feature 2/8+: duck_timeline

- [x] One-call ducking across the whole timeline: names every dialogue track and every music track once,
  runs kimo's own speech detector (the same VAD `auto_duck`'s own `detect: "speech"` mode uses) once per
  dialogue track, combines the speech windows across ALL of them, and hands that combined list to EVERY
  named music track through `auto_duck`'s existing `speech_windows` argument -- one `dispatch_nested` call
  to `auto_duck` per music track, reusing its ducking math rather than reimplementing it. A line said on
  either of two dialogue tracks now ducks both music beds in one call, where `auto_duck` alone needs one
  call per (music, dialogue) pair and cannot union two dialogue tracks' speech into one duck signal at all.
  A track named in both lists is an error. `amount_db`/`attack`/`release`/`hold` match `auto_duck`'s own
  speech-mode defaults.
- New MCP tool `duck_timeline` (`crates/kimo-core/src/ext/duck_timeline.rs`). 6 new TDD tests
  (`tests/duck_timeline.rs`) with synthetic voice-burst audio (same technique as `tests/duck_speech.rs`):
  one music track ducks under speech from either of two separate dialogue tracks, two music tracks both duck
  in one call, a track named as both dialogue and music errors, empty lists error, an unknown track id
  errors, and undo clears the ducking. `golden_registry.txt` hand-updated (513 -> 514 tools). Full targeted
  pass green including `golden_frames_registry_and_projects` (no frame/project golden moved) and
  `registry_lint`'s `via` advice check.

## v7.5 feature 3/8+: kimo gui copy/paste and ripple delete

- [x] `kimo gui`: Ctrl/Cmd+C copies the current selection (one or more clips), Ctrl/Cmd+V pastes it at the
  playhead, keeping the copied clips' relative arrangement (the earliest-starting one lands exactly on the
  playhead; the rest keep their original offset from it) -- a multi-clip paste is one atomic `batch` call of
  `duplicate_clip` ops (one undo step for the whole paste), and `duplicate_clip` already carries every
  effect/keyframe/mask, so nothing about a clip's content needs to pass through the clipboard itself, only
  `{id, start}`. Shift+Delete/Backspace ripple-deletes the selected clip (closes the gap via `ripple_delete`)
  instead of the plain Delete/Backspace, which still only removes it (leaves a gap) -- a deliberately
  different key so an accidental plain Delete never ripples clips the user did not mean to shift. Ripple
  delete is single-clip only (same honest limit this codebase already states for group trim: there is no
  settled meaning yet for "ripple a group").
- `gui_clipboard.js`: pure JS, no DOM -- `copySelection(clips)` (snapshots `{id, start}` at copy time,
  independent of later selection changes), `buildPasteOps(clipboard, pasteAtTime)` (the exact `batch` op
  list, relative arrangement preserved, never pastes negative, rounds to 1e-4s), `buildRippleDeleteOp(clipId)`.
  Served at `GET /gui_clipboard.js`, same bytes the browser and the Node test run. `gui_keymap.js` extended
  (Ctrl/Cmd+C/V, Shift+Delete/Backspace) with 10 new Node keymap tests alongside the existing ones.
- 10 new headless Node tests (`tests/js/clipboard_test.js`) plus 10 new Node keymap tests
  (`tests/js/keymap_test.js` additions) plus 5 new Rust tests: the JS file served byte-identical to disk,
  the index page wires up every new function and action by name, a real `duplicate_clip` through `/api/call`
  copies a clip WITH its effect, a two-clip batch paste is one atomic undo step, and `ripple_delete` through
  `/api/call` actually closes the gap. All 64 pre-existing kimo-cli tests stayed green (69 total now).

## v7.5 feature 4/8+: kimo gui marker/chapter list panel

- [x] `kimo gui`: markers already render as ticks on the timeline ruler; this adds a scrollable LIST panel
  beside the timeline (same markers `timeline_view_model` already returns, sorted by time) -- each row shows
  a color swatch, the marker's name (or a generic placeholder so an unnamed marker is never a blank row),
  and a formatted timestamp. Clicking a row seeks the playhead there (`scrubTo`); the row's own `x` removes
  the marker through the existing `removeMarkerAt` (reused, not reimplemented -- the ruler's ticks and the
  panel's rows both end up calling the same remove path).
- `gui_markerpanel.js`: pure JS, no DOM -- `formatTime(seconds)` (a timeline-duration label: `M:SS.ss`, or
  `H:MM:SS.ss` past an hour), `buildMarkerRows(markers)` (the exact display rows: id, label, time,
  timeLabel, color). Served at `GET /gui_markerpanel.js`, same bytes the browser and the Node test run.
- 13 new headless Node tests (`tests/js/markerpanel_test.js`) plus 3 new Rust tests: the JS file served
  byte-identical to disk, the index page wires up the panel and its function by name, and
  `timeline_view_model` (through `/api/call` + `/api/timeline`) feeds the panel real, sorted markers. All 69
  pre-existing kimo-cli tests stayed green (72 total now).

## v7.5 feature 5/8+: kimo gui render queue panel

- [x] `kimo gui`: the Export section already polls ONE job's progress inline; this adds a panel listing
  every render job started this session -- path, live state, percent, and a short eta/error note -- so a
  finished job's result stays visible instead of being overwritten the moment a second export starts. Each
  queued/running/cancelling job is polled independently (`job_status`, same tool the single-export bar
  already calls); a finished row (done/failed/cancelled) gets a dismiss `x` to clear it from the list. No new
  engine calls -- this only reshapes `job_status`'s own responses into display rows and tracks them as a
  list instead of one variable.
- `gui_renderqueue.js`: pure JS, no DOM -- `addJob(jobs, id, path)`, `updateJob(jobs, id, status)` (reshapes
  one `job_status` result into `{state, pct, note}`, same chunks/frames-fallback math the single-export bar
  already uses), `removeJob`, `activeJobIds` (still-unfinished jobs to keep polling), `isFinished`. Served at
  `GET /gui_renderqueue.js`, same bytes the browser and the Node test run.
- 15 new headless Node tests (`tests/js/renderqueue_test.js`) plus 3 new Rust tests: the JS file served
  byte-identical to disk, the index page wires up the panel and its functions by name, and
  `export_job_start`/`job_status` through `/api/call` prove the exact shape the panel polls. All 72
  pre-existing kimo-cli tests stayed green (75 total now).

## v7.5 feature 6/8+: import_clips

- [x] Copy specific clips into the live project, from another project entirely if you like -- with
  everything a clip itself holds (effects, keyframes, masks, its own embedded grade graph, speed), since a
  clip serializes all of it directly. `source` is a revision string in `project_diff`'s own forms (`current |
  undo:<n> | checkpoint:<name> | file:<path.kimo>`), reused rather than re-implementing project loading, so
  this can pull clips out of a wholly separate project file, not just an earlier state of this one.
  `project_merge` already reconciles two revisions of the SAME lineage by matching clip id (add/remove/
  change); a straight copy must not assume shared ids, so every copied clip gets a FRESH id in the live
  project, and a destination clip that happens to already use the source clip's id is left untouched (two
  independently-built project files can easily reuse the same ids). A media clip's asset is imported into
  the destination's own asset pool too -- reused by file path if the destination already has that exact
  file, created fresh otherwise -- so the copy never carries a dangling asset reference. `track_id` is the
  destination track, or omit it to create one new track for the call; `time_offset` shifts every copied
  clip's original start by the same amount. Honest gap: a grade bound through the project-level
  shared-grade-scopes feature does not travel (that binding lives on the source project itself, not the
  clip); a clip's own embedded grade graph does.
- New MCP tool `import_clips` (`crates/kimo-core/src/ext/import_clips.rs`). 13 new TDD tests
  (`tests/import_clips.rs`): a clip imports with its effect and keyframe onto a new track, `time_offset`
  shifts it, importing onto a named track creates no new track while omitting it does, a media clip's asset
  imports alongside it, two clips sharing one source asset only import that asset once, an asset already
  present in the destination by file path is reused rather than duplicated, a forced id collision with an
  existing destination clip does not overwrite it, unknown clip id / missing source file / unknown
  destination track all error cleanly, undo removes the whole import in one step, and multiple clips import
  onto one destination track in one call. `golden_registry.txt` hand-updated (514 -> 515 tools); added to
  the `project` toolgroup (`toolgroups.rs`) up front this time, caught nothing missing on the full suite.
  Full targeted pass green including `golden_frames_registry_and_projects` and `registry_lint`.

## v7.5 feature 7/8+: style_captions_batch

- [x] Apply one style to every caption GROUP in the project, or a named list of them, in one call.
  `set_caption_style` already restyles every CUE inside ONE group (its own description: "Apply caption
  preset to every cue") -- the real gap is that a project can hold several groups (one per
  `add_captions`/`add_caption_cues` call: different speakers, different tracks, an SRT import plus a later
  `caption_from_words` pass), and restyling all of them today is one `set_caption_style` call per group.
  `style_captions_batch` reuses `set_caption_style` itself (`dispatch_nested`, one call per named group)
  rather than reimplementing the per-cue styling math a second time. `caption_ids` omitted means every
  caption group in the project; an EXPLICIT empty array styles nothing (never silently falls back to "all").
  `size`/`y` pass straight through as the same optional overrides `set_caption_style` already takes.
- New MCP tool `style_captions_batch` (`crates/kimo-core/src/ext/style_captions_batch.rs`). 8 new TDD tests
  (`tests/style_captions_batch.rs`): styles every group when none are named, styles only the named ones and
  leaves the rest alone, size/y overrides pass through to every styled group's cues, a project with no
  captions is a harmless no-op, an explicit empty list styles nothing, an unknown named group id errors, an
  unknown style name errors, and undo restores every styled group in one step. `golden_registry.txt`
  hand-updated (515 -> 516 tools); added to the `text` toolgroup up front. Full targeted pass green.

## v7.5 feature 8/8: kimo gui keyframe lane

- [x] A "keyframes" section in the inspector: a dropdown of every animated property on the selected clip
  (clip transform props and effect params, each entry labelled with its key count -- an effect param is
  prefixed with the effect's kind so two effects animating the same param name don't collide), and below it
  a lane showing that property's keyframes laid out across the clip's own duration. Click an existing
  diamond to remove that keyframe (`remove_key`); click empty lane space to add one at that clip-local time
  seeded with whatever value is already playing there (`sample_property`, so adding a keyframe never
  silently resets the value), then `set_keyframe`. A click is told "this is an existing key" from "this is
  empty lane" by mirroring the server's own `move_key`/`remove_key` rule -- nearest key within half a frame
  -- so the lane never guesses differently than the tools it is calling. Honest scope limit: no dragging a
  key in time yet; that is `move_key` through the generic tool invoker already in the GUI, not this lane.
- `gui_keyframes.js`: `propertyKey`/`parsePropertyKey` (a composite key so an effect param can't collide with
  a same-named clip property or another effect's param of the same name), `buildPropertyOptions` (labels,
  singular/plural key count), `buildKeyMarkers` (clip-local time -> lane pixel x, divide-by-zero guarded on
  a zero-duration clip), `timeForLaneX` (the inverse, for a click), `nearestKeyWithinHalfFrame` (mirrors
  `move_key`/`remove_key`'s own matching rule). Served at `GET /gui_keyframes.js`, wired into
  `gui_index.html` (dropdown + lane markup, `loadKeyframePanel`/`loadKeyframeLane`, called from the existing
  `loadInspector()`) and `gui.rs` (route, 405 list). 21 new JS tests (`tests/js/keyframes_test.js`) + 5 new
  Rust tests (served bytes match disk, index page wires up the lane, `list_animated_properties`/`get_curve`
  feed it real keys through `/api/call`, `remove_key` removes exactly the clicked keyframe,
  `sample_property` reads back the value a new keyframe would be seeded with); all 80 kimo-cli tests green
  (75 prior + 5 new), no registry change (GUI-only, no new MCP tool).

## v7.6 additions

- [x] project_stats: one-call aggregate project summary for an agent (duration, track/clip counts by
  kind, unused media, caption/marker counts) instead of reading the whole project or paging find_assets.
- [x] organize_bins: file every asset into bins automatically by kind/extension or by usage (used/
  unused), creating the bins as needed, in one call.
- [x] batch_set_meta: apply a role/color/tags to many clips at once, by explicit ids or by a
  find-style filter, in one call.
- [x] kimo gui: clip context menu (right-click: split/duplicate/delete/properties).
- [x] kimo gui: media bin panel with search and a thumbnail list.
- [ ] kimo gui: timeline minimap/overview.
- [ ] kimo gui: inline caption text editing directly on the timeline.
- [ ] kimo gui: transitions drag-drop between clips.

## v7.6 feature 1/8: project_stats

- [x] One-call aggregate project summary for an agent: canvas, total timeline duration, track count with
  muted/solo/locked breakdowns, total clip count with a breakdown by source kind (media/text/generated),
  how many clips carry an effect or a keyframed property, total asset count against how many are actually
  used by a clip somewhere on the timeline (unused_assets lists the rest by id and path), bin count,
  marker count, and caption group/cue counts. `find_assets` already reports `used_by_clips` per asset and
  `timeline_summary` already reports full per-clip detail; neither gives the agent a fast aggregate read
  without paging results and counting by hand. Computed directly off the live project on every call, so it
  is always exact, never a cached guess.
- New MCP tool `project_stats` (`crates/kimo-core/src/ext/project_stats.rs`), read-only (`mutates: false`).
  7 new TDD tests (`tests/project_stats.rs`): zeroed counts on an empty project, clip counts by kind with
  duration as the furthest clip end, an imported-but-unplaced asset reported unused, deleting the clip
  that used an asset makes it unused again, markers/captions/bins/track-mute counts, clips-with-effects
  and clips-with-keyframes counts, and that the call adds no history entry. `golden_registry.txt`
  hand-updated (516 -> 517 tools); added to the `project` toolgroup up front. Full targeted pass green
  including `golden_frames_registry_and_projects`, `golden_registry_contract`, `description_first_sentence`
  and `external_refs`.

## v7.6 feature 2/8: organize_bins

- [x] File every asset in the media pool into bins automatically, by kind (Video/Image/Audio from the
  file extension via the project's existing `deps::asset_kind` classifier, already used for dependency
  reporting) or by usage (Used/Unused, by whether a clip on any track references the asset right now).
  `create_bin`/`move_asset_to_bin` already do the mechanics one asset at a time; a library of any size
  makes that a manual "create a bin per kind, then move each asset in" pass. `only_unfiled` (default
  true) leaves an asset alone if it is already filed in some bin, so a sweep never undoes a person's own
  manual organisation; an asset already sitting in exactly the bin this call would choose is left
  untouched either way. `dry_run` reports the plan and changes nothing. The whole sweep is one atomic
  undo step however many assets move (dispatch_nested per move inside the one outer mutating call, the
  same shape `close_gaps` and `style_captions_batch` already use for their own per-item loops).
- New MCP tool `organize_bins` (`crates/kimo-core/src/ext/organize_bins.rs`). 7 new TDD tests
  (`tests/organize_bins.rs`): by:"kind" sorts video/image/audio assets into their own bins, by:"usage"
  sorts used vs. unused, the default `only_unfiled` leaves a manually-filed asset alone,
  `only_unfiled:false` re-files everything, `dry_run` reports the plan without moving anything, the whole
  sweep over 3 assets lands as exactly one undo step, and an unknown `by` value errors. `golden_registry.txt`
  hand-updated (517 -> 518 tools); added to the `project` toolgroup up front. Full targeted pass green
  including `golden_frames_registry_and_projects`, `golden_registry_contract`, `description_first_sentence`
  and `external_refs`.

## v7.6 feature 3/8: batch_set_meta

- [x] Apply a role, colour label and/or tags to many clips at once, in one call: by explicit `clip_ids`,
  or by a `filter` object taking the same fields `find` already does (name, name_glob, role, color, tag,
  track, source_kind, has_effect, time_range, text_contains). The filter lives under its own nested
  object rather than spread at the top level, so filtering by role/color is never confused with the
  role/color being SET -- the same field name would otherwise mean two different things depending on how
  a clip was selected (tested explicitly: selecting by `has_effect` and setting a `color` that differs
  from any filter value). Literal `name` is deliberately not exposed as a value to set: `set_meta`
  requires names to be unique among clips, so one batch value could never legally apply to more than one
  clip -- naming stays its own per-clip `set_meta` call. A filter matching more than 500 clips (`find`'s
  own cap) errors asking the caller to narrow it, rather than silently only touching the first 500.
- New MCP tool `batch_set_meta` (`crates/kimo-core/src/ext/batch_set_meta.rs`), reusing `find` and
  `set_meta` via `dispatch_nested`. 9 new TDD tests (`tests/batch_set_meta.rs`): `clip_ids` sets role and
  colour on exactly those clips and leaves others untouched, a `has_effect` filter selects clips and
  applies a colour independent of the filter's own fields, `add_tags`/`remove_tags` pass through to every
  selected clip, an empty `clip_ids` updates nothing, and five error cases (no meta change requested,
  neither selector given, both selectors given, an unknown filter field, an unknown clip id). Undoable as
  one atomic step like the lane's other batch tools. `golden_registry.txt` hand-updated (518 -> 519
  tools); added to the `project` toolgroup up front. Full targeted pass green including
  `golden_frames_registry_and_projects`, `golden_registry_contract`, `description_first_sentence` and
  `external_refs`.

## v7.6 feature 4/8: kimo gui clip context menu

- [x] Right-click a clip bar for a real visible menu: Split at Playhead, Duplicate, Delete, Properties --
  the same tools (`split_clip`, `duplicate_clip`, `delete_clip`) and inspector the keyboard shortcuts and
  click-to-inspect already use, surfaced for discovery instead of needing to be known by heart. Split
  mirrors the server's own "split point outside clip" rule client-side, so the item shows disabled (with
  a reason on hover) exactly when the server would refuse it, rather than enabled and failing on click.
  Duplicate is a genuinely new direct path: before this, duplicating a clip only existed indirectly via
  copy-then-paste (v7.5's clipboard); the menu places the copy immediately after the original on the same
  track, one click. The menu is viewport-clamped so a right-click near the window edge never renders
  off-screen, and a click anywhere else on the page closes it.
- `gui_contextmenu.js`: pure JS, no DOM -- `buildMenuItems(clip, playheadTime)` (which of the 4 items are
  enabled, with a disabled reason for Split), `actionFor(itemId, clip, playheadTime)` (the exact
  `{tool, args}` to call, or `{open: "properties"}` to open the inspector instead), `menuPosition(x, y,
  viewportW, viewportH, menuW, menuH)` (clamped placement). Served at `GET /gui_contextmenu.js`, wired
  into `gui_index.html` (the clip bar's new `oncontextmenu` handler, a `#clip-context-menu` div, CSS, and
  `showContextMenu`/`hideContextMenu`/`runContextMenuAction`) and `gui.rs` (route, 405 list).
- 17 new headless Node tests (`tests/js/contextmenu_test.js`) plus 4 new Rust tests: the JS file served
  byte-identical to disk, the index page wires up the menu by name, a real `duplicate_clip` through
  `/api/call` actually creates a second clip at the computed start, and `split_clip` at the exact boundary
  the JS marks disabled fails server-side too (so the menu's enabled state never promises something the
  API refuses) while splitting inside the clip succeeds. No registry change (GUI-only, no new MCP tool,
  same shape as v7.5's keyframe lane). All 80 prior kimo-cli tests stayed green (84 total).

## v7.6 feature 5/8: kimo gui media bin panel

- [x] A searchable, thumbnailed list of every asset in the media pool -- before this, the GUI had no view
  at all of imported-but-not-yet-placed media; only clips already on the timeline showed up anywhere.
  Reshapes `find_assets`' own result (name, bin, duration, whether anything on the timeline currently uses
  it) for display, filters by file name as you type, and renders a real thumbnail per asset. A plain path
  input lets media actually get into the bin from the GUI (`import_media`), which the GUI had no control
  for before either. Honest scope: the thumbnail is the asset's own midpoint frame, not a scrubbable
  filmstrip, and the panel refreshes on load, after an import, and after any call through the generic
  Tools invoke panel -- not on every mutation made through a dedicated button elsewhere in the GUI.
- New server route `GET /asset_thumb.png?asset_id=<id>` (`gui.rs`'s `asset_thumbnail_png`): builds a
  throwaway one-clip project holding just that asset through `add_track`/`add_clip` (the same tools any
  caller uses, so no new clip-construction code) and renders its midpoint with the same `render_png`
  `/preview.png` already uses. The real session and its undo history are never touched -- this is a read,
  not an edit. No new MCP tool (an HTTP-only route, like `/preview.png` itself).
- `gui_mediabin.js`: pure JS, no DOM -- `basename`, `formatDuration`, `matchesQuery` (case-insensitive
  substring match on the file name), `buildAssetRows` (filters and reshapes `find_assets`' matches into
  display rows, sorted by name so the list does not reshuffle while typing). Served at `GET
  /gui_mediabin.js`, wired into `gui_index.html` (a Media bin section with an import path input, a search
  box, and the thumbnailed list) and `gui.rs` (route, 405 list).
- 24 new headless Node tests (`tests/js/mediabin_test.js`) plus 6 new Rust tests: the JS file served
  byte-identical to disk, the index page wires up the panel by name, a real `import_media` through
  `/api/call` feeds `find_assets` for the panel, `/asset_thumb.png` renders a real PNG (checked against
  the actual PNG magic bytes) for an imported asset without adding a clip to the live project, an unknown
  asset id is a clean 404, and a missing `asset_id` query param is a clean 400. All 84 prior kimo-cli tests
  stayed green (90 total).
