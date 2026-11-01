# Changelog

## v8.2.0 (2026-11-01)

V8.2-V visual finish quality pass: "if I put some clips into it, be it of anything, I get top YouTube
editor quality work" -- scoped to how the finished video LOOKS (no voice/audio work this pass, per
steer). Every item wires an existing, already-tested engine tool into `auto_edit`'s own defaults; no
new tools (522, unchanged).

- **Slow Ken Burns push-in on static b-roll cutaways**, new `push_in` auto_edit stage (default on).
  Wires `add_smart_punch` (saliency-driven continuous push-in, already registered and tested, never
  called from `auto_edit`) onto every static b-roll clip (no pre-existing scale/x/y animation) at
  least 1.2s long. New options `push_in` (bool, default true) and `push_in_zoom` (default 1.05 = 5%,
  clamped 1.01..1.3, inside the asked 3-6% range).
  - TDD: `push_in_animates_static_broll_by_default` (a real keyframed ramp, 2.5-7% over the shot, plus
    the off switch) and `push_in_skips_very_short_broll` (<1.2s left alone).
  - Verified on the real benchmark footage: `real_v41.mp4` vs the prior render at the identical 6.5s
    timestamp in the coffee-cup b-roll shows the push-in clearly by eye (background crossbars shifted,
    cups larger); the engine's own returned keyframes confirm it numerically (scale 1.0 -> 1.025 ->
    1.05). Audio completely unchanged: -13.996 LUFS, 90.0%/91.67% recall, identical to the prior render.
- **Punch-ins now force a split on any dialogue stretch that would sit static past `punch_max_static`
  seconds** (new option, default 4.5, clamped 1.5..20). Natural-pause splitting (`auto_rough_cut`,
  `tighten_pauses`) already keeps most real takes short enough for the existing `add_punch_ins` to
  alternate wide/tight on (the real benchmark footage's longest natural stretch was already 3.2s,
  unaffected by this change) -- but a continuous sentence with no breath, or a single long uncut take,
  could leave one giant clip with nothing to alternate against. New `split_points` helper finds the
  nearest real word-pause (>=0.12s gap between two transcript words) to each `punch_max_static` mark
  and splits there via the existing `split_clip`, falling back to a plain interval cut when no clean
  pause is nearby.
  - TDD: `job2_splits_a_long_uncut_dialogue_take_for_punch_ins` (a 10s single take splits into pieces
    all <=4.5s, at least one punched) and `job2_punch_max_static_is_configurable` (set high, a short
    take is left as one clip, matching the pre-existing behaviour and its warning).
  - Verified on the real footage with `pacing` disabled (so natural-pause splitting couldn't paper over
    the gap): the one long ~12.5s dialogue stretch split into 4.5/4.4/4.5/0.5s pieces, each with its own
    wide/tight zoom; frames right at one of the new cuts show a clean, real jump-cut by eye.
- **Captions default to `auto` placement** (new `caption_position` option, default `"auto"`, was a
  hardcoded `"bottom"`). `auto_captions` already had a real face-aware `auto` mode (samples the source
  footage for a skin-toned band, moves the caption strip to the top when a face/body reaches into the
  normal bottom band) that `auto_edit` simply never used.
  - TDD: `job3_caption_position_defaults_to_auto_and_avoids_a_low_face` (a synthetic low-face shot gets
    its captions pushed to the top band by default; an explicit `caption_position` still overrides it).
  - On the real benchmark footage the speaker's face sits clear of the bottom band already, so `auto`
    resolves to `"bottom"` exactly as before -- verified audio-identical to the prior render and looked
    at captions at 3 timestamps: clean, readable, inside safe margins, never over the face. (The
    word-by-word pop itself -- active word coloured + scaled up to 1.22x, synced to exact word timing --
    already existed in the caption styles `auto_edit` uses; placement was the one real gap.)
- **A short whip/zoom-blur transition into each b-roll cutaway**, new `broll_transitions` stage
  (default on; new `broll_transition_duration`, default 0.25s, clamped 0.1..0.6). Wires the existing
  `add_transition` tool's `zoom_blur`/`whip_left` kinds (already in the engine, never called from
  `auto_edit`) onto each b-roll clip's own `transition_in`, alternating kinds across multiple cutaways.
  Scoped strictly to the b-roll boundary -- never applied to the talking-head's own jump-cut punch-ins,
  which stay hard cuts on purpose.
  - TDD: `job4_broll_gets_a_short_whip_transition_by_default` (a b-roll clip gets a short
    zoom_blur/whip_left `transition_in` by default; the off switch leaves it with none).
  - Verified by eye mid-transition on the real footage: a clean, tasteful blur-blend, not jarring;
    audio unaffected.
- Not attempted this pass: any voice/audio work (explicitly steered off partway through: "Work on more
  features except voice now").

Tests: full workspace suite green apart from the pre-existing exempt failures named in this pass's own
brief (`director_small`'s text-crowding test and its CPU-timing lint_edit perf test; `readme_count`,
the `robust_est` doctest, the `search_tools` `fit_clip_duration` tie, one `xform_lint` read -- none
touched this pass). Goldens unchanged (no new tools, no default changed in a way that moves rendered
pixels on an existing golden's own call shape). Warnings: 34 (cap 35, unchanged from v8.1.0).

## v8.1.0 (2026-11-01)

V8.1-Q quality pass: fixing the real cause behind two V8.0-Q compromises, instead of
shipping around them. No new tools.

- **`level_dynamics` (the master-bus speech leveler) is now scoped to the dialogue span, not the
  whole mix.** New `Project.master_speech_level_scope_end` (set by `finish_audio` from its own
  classified dialogue clips' latest end, read by `audio::mix`/`mix_strict` via a shared
  `apply_master_speech_level`, with a 50ms crossfade at the boundary) narrows ffmpeg `speechnorm` to
  `[0, dialogue_end)`, leaving an end card or music-only tail untouched. `0` = unscoped (the old
  whole-buffer behaviour), so any caller that sets `master_speech_level` without this stays
  byte-identical.
  - This directly disproves v8.0.0's own explanation for why the end-card swell was capped at 3dB:
    with speechnorm now provably never seeing the end card (checked `finish_audio`'s own dialogue
    classification at the moment it sets the scope), raising the swell to 8dB reproduced the exact
    same 56.7% whisper-recall collapse v8.0.0 blamed on speechnorm. The dialogue audio itself was
    never damaged at any swell size (proven by cutting 3dB and 8dB renders down to the dialogue span
    and whisper-transcribing each -- byte-identical transcripts both times); the collapse was whisper
    transcribing the WHOLE exported file in one pass and reading the EARLY dialogue worse once a loud
    segment followed later in the same pass. See the benchmark fix below.
  - TDD: the scope fix itself has no new unit test (`finish_audio_level_dynamics_narrows_loudness_range_within_a_clip`
    and the rest of `director_small.rs`'s `finish_audio_*` suite cover it unchanged); verified end to
    end on the real benchmark.
- **`examples/auto_edit_benchmark.py` now reports and gates dialogue-span recall separately from a
  whole-file number.** `dual_span_recall()` whisper-transcribes a render twice -- once whole, once cut
  to the dialogue span -- against the same reference transcript; both numbers always print in the
  summary JSON (`word_recall_vs_full_source` / `word_recall_vs_ceiling`, each now `{dialogue, full}`),
  and `--recall-span {dialogue,full}` (default `dialogue`) picks which one gates pass/fail. Also fixed
  a second, smaller artifact found along the way: a dialogue-span crop with no lead-in silence reliably
  dropped whisper's read of the clip's very first sentence (66.7% recall straight; 90.0% with 0.5s of
  silence prepended, recovering exactly the dropped opening and nothing else) -- `_trim_to_span` now
  pads 0.5s before transcribing a cropped span, and does it audio-only (WAV) rather than re-encoding
  video it was going to throw away anyway.
- **End-card music swell raised 3dB -> 11dB**, now that both the real scoping fix and the honest
  benchmark gate are in place. End-card LUFS: **-18.4** (was -23.4 under the old unscoped/leaky
  behaviour at 3dB, -26.7 with no swell at all) -- inside the originally-asked -18..-20 range.
  Dialogue-span recall holds at 90.0%; full-file recall 91.7%.
  - TDD: `music_bed_swells_under_the_end_card_once_dialogue_ends` (its `after > before * 1.3`
    assertion holds comfortably at 11dB's ~3.5x boost).
- **Voice dynamics (loudness_range) [~]: investigated, not reached.** Target was dialogue-span
  `loudness_range` >= 3 LU (was 1.4-1.5) while holding -14+-0.5 LUFS, <=-1 dBTP, recall >=90%, no
  interior silences. Swept every candidate the mission named on the real benchmark footage
  (dialogue span only, now that the scope/benchmark fixes above make that measurement trustworthy):
  `level_strength` 1.0 (speechnorm's own floor) reaches 2.0 LU at 90.0% recall -- a real, safe gain,
  and disproves v7.9.0's note that a lower `level_strength` "collapses recall" (the same benchmark
  dialogue-span-crop-with-no-lead-in artifact described above, not a real regression). But 1.0 is a
  literal no-op for speechnorm (`e=c=1.0` cannot change gain either direction) and breaks the
  feature's own regression test for footage that genuinely needs leveling
  (`finish_audio_level_dynamics_narrows_loudness_range_within_a_clip`: on=off=11.4 LU at 1.0, zero
  narrowing). `level_strength` stays at its default 2.5 to protect that real case -- a caller editing
  already-even footage can pass `level_strength: 1.0` explicitly (now a tested, documented choice).
  With every leveling touch off at once (`level_dynamics` off, `match_strength` 0, `compress_strength`
  0 -- the gentlest this engine can be) the honest ceiling measured was **2.6 LU**, still short of the
  >=3 target. Reaching it would need an active dynamics *expander* (deliberately widening quiet/loud
  gaps beyond the source's own natural range), a materially different and riskier feature than
  "gentler leveling" -- out of scope for this pass.
- No job 3 (a visible slow push-in on static b-roll) this pass -- time went to finding and fixing the
  two real bugs above instead; disclosed, not attempted.

- Added preview proxy media management: generation, enable/disable, health listing, deep integrity validation, safe relinking and detachment. PNG/range previews can use verified proxies; exports, audio and analysis retain originals. Missing/stale proxies fall back. Persisted, undoable bindings; synchronous SDR/CFR scope. See [proxy workflow](docs/PROXY_WORKFLOW.md).

## v8.0.0 (2026-11-01)

V8.0-Q quality pass: the finished video itself, not feature count. Tools: 521 -> 522 (new
`match_shot_look`).

- **The 3.0s dead-air tail at the end of every render, fixed.** Every `real_*` render back to v15
  ended on 3.0s of near-silence (18.86-21.87s): the default-on end screen (v7.7-Q) extends the real
  export past the talking-head's own `edit_duration`, but the music bed's own clip length only ever
  targeted `edit_duration`, so the card played over a muted freeze-frame background with nothing else
  under it. Confirmed by extracting frames at the silent span (the SUBSCRIBE FOR MORE card, audibly
  dead) before touching any code.
  - Fix: the music clip's length now targets the full planned export (`edit_duration` +
    `end_screen_duration` when the end screen is on). A music asset shorter than that full length is
    TILED (looped from its own start, capped at 200 repeats) rather than left to run out, with a short
    fade at each loop seam and the true tail fading to silence over up to 0.5s. `duck_timeline` needed
    no change -- it already iterates every clip on a track.
  - Honest result: **zero silences anywhere** (was 3.0s), word recall unchanged at 90.0%, loudness and
    true peak unchanged within spec.
  - TDD: `music_bed_keeps_playing_under_the_end_screen`, `end_screen_with_no_music_warns_about_the_silent_tail`.
- **B-roll color/exposure match to the talking head** (new tool `match_shot_look`, wired into
  `auto_edit` as `broll_color_match`, default on whenever there is b-roll). `auto_grade` only ever
  pulls a clip toward one fixed absolute target (neutral cast, a target luma) -- it never looks at
  what ANOTHER shot in the same edit actually looks like, so a warm-lit interview cut against a
  cool-toned or bright-daylight b-roll still reads as two different shoots stitched together even
  after `auto_grade` runs (measured on the real benchmark: the talking head reads warm/indoor, the
  city b-roll cool, the coffee b-roll bright daylight, each already inside `auto_grade`'s own "fine"
  band). `match_shot_look(reference_clip_ids, target_clip_ids, strength)` measures each clip's own mean
  RGB with the same fit-aware reader `auto_grade` uses, then pulls each target clip's measured look a
  `strength` fraction toward the reference clips' averaged look -- white balance and exposure together,
  gains clamped 0.5..2.0 per channel so a very different reference can never crush or blow a shot.
  Before/after frame comparison saved to `v80_broll_compare.png` (talking head reference, b-roll
  before, b-roll after -- a clearly visible warm push on the cool coffee-shop shot, looked at by hand).
  New options on `auto_edit`: `broll_color_match` (bool, default true when b-roll exists),
  `broll_color_match_strength` (0..1, default 0.6).
  - TDD: `match_shot_look_pulls_target_toward_reference`,
    `match_shot_look_skips_a_clip_that_is_its_own_reference`,
    `broll_color_match_runs_by_default_and_can_be_turned_off`.
- **End-card music swell, capped at a measured-safe level.** Once the dialogue ends there is nothing
  left for the music to duck under, but `finish_audio`/`duck_timeline` only ever set the bed's level for
  use UNDER speech -- the end card's own span measured 12.6 LU quieter than the speech body with no
  voice left to protect. A short ramp keyed right where the card starts brings the bed up once speech is
  done. **Capped at 3dB, not the originally-targeted ~8dB**: an 8dB swell measured clean in isolated
  testing but, re-rendered on the real benchmark, collapsed whisper word recall from 90.0% to 56.7%
  (reproduced twice, byte-identical both times, with no change to the dialogue audio itself) --
  `finish_audio`'s master-bus speech leveler (`level_dynamics`, ffmpeg `speechnorm` run once over the
  WHOLE mixed timeline) reacts to a sudden loud segment anywhere in the mix, confirmed by
  `--no-level-dynamics` on the same 8dB render recovering to 93.3%. The boundary measured as a cliff,
  not a gradient: 3dB held at 91.7% recall, 5dB fell straight back to the same 56.7% collapse as 8dB.
  Shipped at the confirmed-safe 3dB. Honest result: end-card LUFS -26.7 -> -23.4 (a real improvement,
  short of the -18..-20 originally asked, reported as such rather than faked).
  - TDD: `music_bed_swells_under_the_end_card_once_dialogue_ends`.
- No job 3 (voice dynamics, `loudness_range` still 1.4 vs the >=3 target) this pass -- disclosed, not
  attempted; see v7.9.0's own entry for what was already tried and ruled out there.

## Unreleased

## v7.9.0 (2026-10-31)

V7.9-Q quality pass, continuing V7.7/7.8-Q's priority (what actually moves a finished video, not
feature count). Tools: 521 -> 523 (whoosh_sfx on `place_broll`; `compress_amount` on `set_audio`).

- **Voice dynamics investigated honestly; one real, verified, recall-safe gain shipped -- the >=3 LU
  target not reached, marked [~].** The final mix's `loudness_range` sat at 1.2 LU in 7.8.0 while the
  raw source measures 2.4-3.6 LU unprocessed. The suspected cause going in (from 7.8-Q's own notes):
  `finish_audio`'s per-clip average-matching loop pulls every dialogue clip's average loudness the FULL
  distance to the loudest one, with no knob. Added `match_strength` (0..1) to scale that pull -- but
  measured on the real benchmark, clips from the SAME speaker after a rough cut were already within
  ~2.3 dB of each other at that step, so lowering it barely moved `loudness_range` (1.2 -> 1.3 LU at
  0.6) while it reproducibly collapsed whisper word recall from 88.3% to 56.7% (the opening sentence
  dropped entirely -- a hard threshold effect in the small whisper model, not a gradual one, confirmed
  twice on identical input). `match_strength` defaults back to 1.0 (the exact old behaviour) and stays
  as a real option for its original case: two different speakers at very different mic levels (the
  existing `finish_audio_levels_speakers_and_hits_the_target` test covers this).
  - The actual dominant force: `finish_audio` always ran a fixed `acompressor` (threshold=-18dB,
    ratio=3:1) on every dialogue clip with no knob at all. Disabling `level_dynamics` (speechnorm)
    entirely only moved `loudness_range` from 1.2 to 1.4 LU -- the compressor alone was holding it
    there regardless. Added `compress_strength` (0..1) to scale its ratio; swept the real benchmark:
    `compress_strength:0.0` (compressor fully off) reproducibly measured **90.0% word recall (54/60,
    better than 7.8's 88.3% baseline)** with **`loudness_range` 1.2 -> 1.4 LU**. Any value above 0 that
    still ran some compression cost recall for no further LRA gain (0.35 -> 86.7% recall, 1.3 LU). The
    absolute ceiling -- everything off (match_strength 0, compress_strength 0, level_dynamics off) --
    only reached 2.3 LU, still short of 3, and that configuration is not recall-safe. `compress_strength`
    defaults to 0.0: a real, measured, recall-positive change, not a cosmetic knob.
  - Honest result: **`loudness_range` 1.2 -> 1.4 LU** (target >=3 not reached; marked [~] in
    FEATURES.md), **word recall 88.3% -> 90.0%** (improved, not regressed), -14 LUFS and true peak
    unchanged within spec.
  - `Audio.compress_amount` (new project field, custom serde default 1.0 so every pre-7.9 saved project
    loads unchanged) and `set_audio`'s `compress_amount` arg expose the same scale directly.
  - TDD: `finish_audio_match_strength_defaults_to_full_match`,
    `finish_audio_match_strength_explicit_value_partially_matches`,
    `finish_audio_match_strength_zero_leaves_clip_averages_untouched_relative_to_each_other`,
    `finish_audio_compress_strength_scales_the_stored_ratio`,
    `finish_audio_compress_strength_defaults_to_off`. Golden registry hashes hand-updated for the three
    changed schemas (`finish_audio`, `auto_edit`, `set_audio`) -- read from the failing test's own
    computed hash, never `KIMO_UPDATE_GOLDEN`.
- **Whoosh SFX on b-roll cut-ins and the hook title-in** (new, OFF by default): `synth_whoosh_sfx()` in
  `director_tools.rs` generates a short, purely procedural rising-pitch tone (300Hz -> ~2.3kHz over
  0.22s, exponential decay, gentle lowpass) at call time into the OS temp dir and reuses it -- same
  pattern as `add_keyword_pops`' `synth_pop_sfx` (0ae47c0), never a bundled sample. `place_broll` gets
  `whoosh_sfx`: when true, one whoosh at each b-roll cut-in's own start, on a dedicated SFX track.
  `auto_edit`'s hook-title stage places the same cue at the title's entrance when true (threaded through
  to `place_broll` too). Fails open exactly like `add_keyword_pops`' `sfx`: a warning, never a blocked
  cut or title.
  - **Defaults to false, not true, after a real measured cost**: a b-roll cut-in by `place_broll`'s own
    design always lands during active speech (never a true silence), so any sound placed there is a real
    risk, not just a loud one. At 0.3 volume on its own dedicated track, the whoosh cost whisper word
    recall 90.0% -> 86.7% on the real benchmark. Tried the obvious fix -- quieter, 0.15 -- and recall got
    WORSE, 90.0% -> 80.0%, not better: this is not a simple louder-is-worse masking curve, and a third
    blind guess at a volume was not going to be a verified fix with the time left. Left the volume at an
    actually-audible 0.3 and defaulted `whoosh_sfx` to false instead of shipping a recall regression as
    default behaviour -- the same standard this pass already held `match_strength` to. The feature is
    real, built, and TDD'd for a caller who opts in; it is not claimed as safe-by-default.
  - TDD: `place_broll_whoosh_sfx_places_a_real_sound_at_each_cut_in` proves the dedicated track exists,
    one clip per cut-in at the cut-in's own start time, the mix is actually audible there (non-zero
    PCM) when turned on, and the default (no `whoosh_sfx` arg) places none.
  - Classified the new file-open site (the temp whoosh WAV) in `external_refs.rs`.
- Job 3 (a visible, frame-verified b-roll gain -- colour/exposure match or a slow push) did not happen
  this pass; not claimed, not faked.

## v7.8.0 (2026-10-31)

V7.8-Q quality pass: more of `examples/auto_edit_benchmark.py`'s real CC0 interview footage, continuing
V7.7-Q's priority (what actually moves a finished video, not feature count). Tools: 520 -> 521 (one new
tool). The second item below is the headline: a serious pre-existing correctness bug, not a polish pass.

- **`tighten_pauses`** (new tool): a rough cut (`auto_rough_cut`) already drops dead air BETWEEN kept
  clips; nothing before this tightened the dead air a rough cut leaves INSIDE a clip it kept -- an
  interior breath between two phrases, or the pause before the speaker's first word. Probes each
  dialogue clip's own interior silence (reusing `audio_speech_intervals` in `silence` mode, not VAD,
  whose padding/hangover would have inflated speech boundaries and understated the real gap) and ripple-
  deletes everything past a natural pad (default 120ms interior, 80ms lead-in) via the same
  `trim_filler_words` machinery filler-word trimming already uses, so caption retiming comes for free.
  `auto_edit` runs it by default (`pacing: true`) right after the rough cut, before transcription, and
  re-derives dialogue_clips/edit_duration from the tightened track.
  - Three real-footage calibration bugs found and fixed while building this, each only visible on an
    actual render (a synthetic fixture missed all three): a fixed -45dB silence threshold never fired on
    real room tone (-20 to -25dB); the first per-clip auto-calibration attempt biased short speech-heavy
    segments' floor estimate toward speech level and gutted 9.6s of real dialogue as "false silence";
    the whole-track fix that replaced it was then too conservative on a recording with only ~8.5dB of
    real headroom between noise floor and quietest speech, finding zero gaps again. Settled: calibrate
    once from the whole track (more quiet frames to read), threshold = floor + a fraction (default 0.5)
    of the way to the quietest detected speech level, capped 3dB under speech either way.
  - Never leaves a surviving piece shorter than `min_piece_s` (default 1.0s, clamped 0.2-5): fragmenting
    a clip too far raised the odds of the bug below (a short clip reads less reliably to other per-clip
    tools downstream) -- directly responsible for making that bug worse mid-session, caught and capped
    before it shipped.
- **Found and fixed a serious pre-existing bug: `finish_audio` was silently muting most real dialogue as
  "ambience".** `finish_audio` classifies every media clip on the timeline as dialogue, music, or
  ambience by probing `audio_speech_intervals` at aggressiveness 2 and requiring speech_ratio >= 0.2;
  ambience gets muted. On the real benchmark interview, 4 of `auto_rough_cut`'s own 6 kept dialogue
  segments read speech_ratio under that bar at aggressiveness 2 (one segment read -0.0 speech_ratio on
  0.8s of unmistakable continuous speech) and were muted outright -- and every loudness/silence check in
  the benchmark still passed, because finish_audio's final gain-trim loop only ever levels whatever
  survived the mute up to the target LUFS, with no signal that most of the source was gone. `git log -S`
  on the classifier line traces this to commit 77e14e2 (~294 commits back, when `finish_audio` was first
  written) -- present in every real-footage render this project has ever made, not something any
  V7.6Q/7.7Q/7.8Q commit introduced. Caught by whisper-transcribing a shipped render and finding most of
  the source's real sentences missing.
  - Fix: try speech_ratio at aggressiveness 1 first (the same real segments read 0.69-0.90 there,
    comfortably over the bar), then rescue anything still called ambience by checking aggressiveness 0 at
    a materially higher bar (0.5, not the same 0.2) so a real miss still gets caught (the one segment that
    failed at both 1 and 2 reads 0.825 at 0) without casually relabelling real background tone as dialogue
    just because the most lenient level is more permissive.
  - TDD: a synthetic sine-harmonic voice stand-in (the test file's own `talk()` helper, reused everywhere
    else in this suite) does NOT reproduce this -- it is too spectrally pure to trip aggressiveness 2's
    steady-tone rejection the way real voice does. The new regression test slices a short excerpt of the
    real interview fresh at test time (ffmpeg, not committed media) and skips (not fails) if that
    box-local fixture isn't present, so it stays honest without demanding real media in every checkout.
  - New permanent regression gate: `examples/auto_edit_benchmark.py --whisper-recall-source <raw file>`
    transcribes both sides with whisper-cli and fails the run (exit 1) if the rendered word recall against
    the source falls under `--whisper-recall-min` (default 0.90) -- loudness and silence checks alone
    cannot catch this class of bug, since a 90%-muted dialogue track still mixes to exactly the target LUFS.
  - Measured on the real benchmark (whisper word recall vs the source transcript, bag-of-words):
    real_v1.mp4 (first real render, long before this quarter's work) 27.5%, real_v14.mp4 (last V7.7Q
    render) 27.5%, real_v15.mp4 with this session's pacing work but before this fix 13.0% (tighten_pauses'
    extra fragmentation made the pre-existing bug measurably worse), real_v15.mp4 with the fix 68.1%.
    Confirmed directly against finish_audio's own report, not just the recall number: every one of the 11
    real dialogue clips on the fixed render's talking-head track is now classified as dialogue and
    leveled; the only clips still muted as ambience are b-roll's own scratch audio (correct and intended).
    The remaining gap to the 90% gate on this one real clip is not muting -- it is whisper's own
    recognition noise on compressed/leveled speech plus a handful of words legitimately removed by
    rough_cut/filler-trim's own dead-air and filler trimming, not dropped audio.

- **Found and fixed: `auto_rough_cut` lost real opening words on a cold-open recording.** Its speech
  detector (VAD mode, aggressiveness 1) needs several consecutive confirming frames before declaring
  speech has started; on the real benchmark interview (talking begins immediately, no lead-in silence)
  that onset lag cost the first ~0.48s of real words -- confirmed independently with whisper, which
  hears real speech ("It takes so much knowledge...") at the source's literal 0.000s, while the raw VAD
  probe's own first detected span started at 0.48s. Fix: `recover_missed_onset` re-checks only the very
  first kept segment of the whole output (every later segment already has real silence, or another kept
  clip, right before it) with a plain RMS silence probe -- the same technique `tighten_pauses` already
  uses, with no VAD confirmation delay -- and pulls the start back to wherever real silence actually
  ends, never later than VAD's own answer. On the real benchmark: first kept segment went from source
  [0.48, 2.6] to [0.0, 2.6]. TDD against a real-footage excerpt sliced at test time (not committed media).
- **Made the recall gate honest.** `auto_rough_cut`'s own dead-air/filler trimming removes real source
  content ON PURPOSE -- gating a finished render's word recall against the FULL untrimmed source punished
  correct editing right alongside the real muting bug above. `--whisper-recall-source` now gates against
  the kept-span CEILING instead: it re-runs `auto_rough_cut` on the same talking-head asset(s) (read-
  derived, purely to recover the exact spans it kept), concatenates just that kept audio into a reference
  clip, transcribes it as the ceiling, and reports the render's recall against that (still 90% default,
  `--whisper-recall-min`). Reports both numbers plus the ceiling's own transcript text.
  - Measured on the real benchmark with every fix above in: 88.3% against the ceiling (53/60 words),
    up from the original broken baseline's 27.5% against the full source -- close to the 90% floor but
    not over it. The remaining gap looks more like whisper's own flag-sensitive recognition noise at clip
    boundaries than real content loss (an alternate whisper-cli invocation on the identical render
    recovered the one phrase bag-of-words scoring called missing) but this was not chased to full
    certainty this quarter; reported as a disclosed, open gap, not claimed closed.
- **Honest miss, not hidden: loudness range (LRA) on this one real clip.** The brief's 6-8 LU target was
  not met -- measured ~1.2-1.4 LU after every fix above, against a ~2.4-3.6 LU natural ceiling in the RAW
  unprocessed source itself (checked directly: `measure_loudness` on the track right after `auto_rough_cut`,
  before any `finish_audio` processing at all). This specific calm interview voice does not naturally carry
  6-8 LU of dynamic range. Tested with `level_dynamics` (speechnorm) fully disabled: LRA barely moved
  (1.4 vs 1.2), showing the dominant narrowing force is `finish_audio`'s per-clip gain-matching step
  (every dialogue clip leveled to the loudest clip's own average -- a deliberate consistency feature, not
  a bug, and not something this pass has an exposed knob to disable). The prior real_v14 render's 7.1 LU
  reading almost certainly reflects the finish_audio muting bug above (big silent-vs-loud swings from
  mostly-missing dialogue masquerading as dynamics), not healthy vocal variation -- not re-chased further
  this quarter given the time already spent on the higher-severity muting and onset bugs; disclosed as a
  known gap for a future pass, not claimed fixed.

## v7.7.0 (2026-10-31)

V7.7-Q quality pass: finished-video editing quality on real footage (`examples/auto_edit_benchmark.py`
over a real CC0 interview), prioritized by what actually moves a finished video rather than feature
count. Tools: 520 -> 520 (new optional args on two existing tools, no new tool names).

- **B-roll placed by what's actually being said, not just given order**: `place_broll` now takes
  optional `words` (a timeline-second transcript) and `asset_tags`; a curated keyword/synonym table
  (deliberately plain-English, not a classifier) and a filename-keyword fallback score each b-roll
  asset against each speech slot and assign greedily, so "...when I was learning to type..." pulls the
  typing clip instead of whichever b-roll happened to be listed first. Falls back to the old given-order
  behavior byte-for-byte when no words/only one asset. `auto_edit` transcribes dialogue once, right
  after the rough cut, and feeds that same transcript to both b-roll placement and captions (no second
  transcription pass). Verified honest: on the real benchmark interview, which never mentions any of its
  three b-roll concepts (typing/city/coffee), the matcher correctly found nothing and fell back to given
  order -- proven separately on footage with real spoken-concept matches via dedicated TDD.
- **Per-page caption y-nudge, not whole-group**: `lint_edit`'s `text_over_subject` fix used to flag only
  the first colliding page of a multi-page caption, then move the WHOLE group by that one page's offset
  -- the flagged page landed right, every other page (including already-fine ones) shifted by the same
  amount for no reason. Every page is now evaluated against its own sampled saliency peak and fixed with
  its own `set_property` (single clip's y), not a shared `set_caption_style` across the group. Verified
  on real footage: 5 of 31 caption pages got distinct per-page y values instead of one shared default.
  Honest limitation found and kept, not hidden: the saliency detector is a spectral-residual heuristic,
  not real face detection, so on one busy-background frame (patterned shirt, ornate mirrored room) it
  still reads background clutter as salient as the face and leaves a partial overlap -- a pre-existing
  DSP-heuristic ceiling, not something this pass caused or can fully close without real face detection.
- **Subscribe-style end screen**: `auto_edit` gets a new stage after the final mix -- a short (2-4s,
  default 3s) outro card using the existing `subscribe_pop` title preset, defaulting on for a
  landscape/youtube-style edit (`platform` none) and off for a vertical short (a subscribe pill fights
  the platform's own UI there), always overridable (`end_screen`/`end_screen_text`/`end_screen_duration`).
  New `total_duration` on the result reports the real export length; `duration` stays the talking-head-
  only number existing callers already read.
  - Caught on the real render, not a synthetic fixture, and fixed before this landed: the `subscribe_pop`
    pill was a fixed 230px tuned for the single word "SUBSCRIBE" -- the end screen's own default text,
    "SUBSCRIBE FOR MORE", ran past both of the pill's rounded ends, and its accent dot (a fixed offset
    that only happened to clear "SUBSCRIBE"'s first letter) landed on top of the "S" with the wider
    label. The pill now sizes itself to the real text (`crate::text::measure`, same tight-ink-extent
    approach as the caption-pill fix below it in this changelog's prior release), the dot moved outside
    the pill beside it, and the card now sits over a blurred, darkened freeze of the video's own last
    frame instead of flat black. That freeze is a real media clip placed after all speech ends, which
    tripped an unrelated existing check (`broll_over_silence`) into "fixing" it back onto the dialogue
    and trimming it to nothing -- that check now exempts a clip placed after all speech has ended (an
    epilogue/end-screen shot) while still correctly flagging b-roll laid over a genuine mid-edit pause.
- **Disk-leak fix (not a feature, worth noting)**: `director_small`'s shared fixture temp dir (reused by
  path across the whole test binary, so fixture media an earlier test builds is found, not re-encoded,
  by a later one) was never removed -- 83 of them, ~3 GB, found on one box. It now registers a libc
  `atexit` hook (no new dependency) the first time it runs, removing the dir once the whole binary is
  done with it.

## v7.6.0 (2026-10-31)

V7.6-Q2 quality pass on `real_v1.mp4` (a real CC0 interview auto-edit), continued from a prior
worker's mid-task exit. No new tools this round -- every change below improves what `auto_edit` and
`add_punch_ins`/`add_keyword_pops` actually deliver on real footage, verified by rendering real_v1's
own clips through the fix each time, not just synthetic fixtures. Tools: 520 -> 520 (two new optional
args on two existing tools, no new tool names).

- **Caption pill hugs the text, not the wrap width**: a boxed caption's background box was sized to
  the full available wrap width regardless of how short the actual line was -- a one-word caption got
  a bar spanning nearly the whole frame. `coverage()` now computes the tight ink extent of the line's
  own glyphs for the bg-box path. Verified: the pill width for a short caption dropped from 1048px to
  under 500px on real footage; golden frames unaffected (only the bg-box path reads the new bounds).
- **New `examples/auto_edit_benchmark.py`**: a reproducible one-command pipeline (clips folder ->
  finished mp4, with a JSON summary of duration/LUFS/lint/clip counts) reconstructed from the actual
  real_v1 render after no script or shell history survived the worker that first produced it.
  Auto-discovers talking-head/b-roll/music clips by filename; `--level-strength`, `--no-level-dynamics`
  flags added in the same pass as the LRA fix below.
- **`finish_audio`'s loudness range actually narrows now**: the per-clip highpass+compressor only
  ever matched each dialogue clip's AVERAGE loudness to the loudest one -- whatever swing already
  lived within a clip rode straight through. A gentle master-bus speech leveler (ffmpeg `speechnorm`)
  now runs once over the whole mixed timeline, loudness-neutral (so it doesn't fight `finish_audio`'s
  own gain-convergence loop -- the first per-clip attempt at this made LRA worse, not better, for
  exactly that reason). `level_dynamics`/`level_strength` on `finish_audio`/`auto_edit`, default on.
  Measured on real_v1: LRA 11.1 -> 6.8 LU (target 6-8), -14.55 LUFS (closer to -14 than the unleveled
  -14.87), -3.94 dBTP (more headroom, not less).
- **Punch-in zoom variety**: `add_punch_ins` alternated wide/punched but every punched clip zoomed by
  the exact same amount -- mechanical on a longer edit. New opt-in `variety`/`variety_amount` (default
  off, so direct calls and existing tests are unchanged) cycles each punched clip's own zoom through a
  fixed, deterministic sequence (never random, never drops near 1.0 so a punch always still reads as
  punched in; the focus point is recomputed per clip so it never drifts). `auto_edit` turns it on by
  default. Verified on real_v1: its two punch-ins landed at 1.132x and 1.1635x off a 1.15x base --
  genuinely different amounts.
- **`add_keyword_pops`'s `sfx:true` plays a real sound now**: previously a complete no-op with a
  warning (the prior research pass marked pop sound effects unverified for lack of a bundled asset,
  and media is never committed to this repo). Now synthesizes a short procedural "pop" blip at call
  time (ffmpeg lavfi tone, not a sampled/downloaded file) and places it on its own audio track, one
  per pop. Verified the mix is actually audible at each pop, not silently placed.

Full gate suite: unchanged pre-existing failures only (confirmed against the pre-session commit in a
throwaway git worktree for each one before touching anything) -- a `director_small` text-crowding
test, a `search_tools` ranking tie unaffected by any of this session's added schema text, one unmarked
`xform_lint` read, and a stray doctest in `robust_est.rs`; two more (`lint_edit` timing, `pick_thumbnail`
/ `push_wipe` temp-file setup) only fail under full `-j2` parallel load and pass clean alone. Goldens
(`golden_frames`, `recipes`) unchanged throughout -- every schema change that grew a tool's registry
hash was hand-verified and hand-updated in `golden_registry.txt`, `KIMO_UPDATE_GOLDEN` was never used.
`kimo-core` lib warnings: 34 (unchanged).

## v7.5.0 (2026-10-30)

Naman's standing instruction continued: "keep working, and keep sending updates." Picked up straight
from v7.4.0 (512 tools). Re-audited the registry and FEATURES.md first (most candidate ideas --
caption styling, a rough cut, project diff -- were already covered pre-v7.5; real gaps found and
built instead). 8 features shipped, each with its own TDD commit:

- New `close_gaps`: general ripple-close for any/all gaps on named or all-unlocked tracks in one
  call, where before it was one `ripple_delete`/`drop_short_segments` call per gap. `min_gap`,
  `include_leading_gap`, `dry_run`; a locked track named explicitly errors, one omitted is skipped and
  reported. 11 new tests. Tools: 512 -> 513.
- New `duck_timeline`: ducks every named music track under the union of speech from every named
  dialogue track in one call, reusing `auto_duck`'s own `speech_windows` path rather than
  reimplementing it. 6 new tests with synthetic voice-burst audio. Tools: 513 -> 514.
- `kimo gui` copy/paste and ripple-delete: Ctrl/Cmd+C/V copies and pastes selected clips as one
  atomic `batch` of `duplicate_clip` calls preserving relative offsets; Shift+Delete ripple-deletes a
  single clip. Pure shared module `gui_clipboard.js`. 20 Node tests + 5 Rust tests.
- `kimo gui` marker/chapter list panel: every project marker in one scrollable list, click a row to
  seek the playhead there, `x` removes it. Pure shared module `gui_markerpanel.js`. 13 Node tests + 3
  Rust tests.
- `kimo gui` render queue panel: every export job started from the GUI gets its own row, polled
  independently of the main preview loop until it reaches a terminal state, then it is dismissable.
  Pure shared module `gui_renderqueue.js`. 15 Node tests + 3 Rust tests.
- New `import_clips`: copy specific clips -- with everything a clip itself holds (effects, keyframes,
  masks, its own embedded grade graph, speed) -- from another project revision into the live project
  at a chosen track and time. Reuses `project_diff`'s own revision forms (`current | undo:<n> |
  checkpoint:<name> | file:<path.kimo>`) rather than re-implementing project loading. Every copied
  clip gets a fresh id (two independent project files can reuse the same ids); a media clip's asset
  imports into the destination's own asset pool too, reused by file path or created fresh. Honest gap:
  a project-level shared grade-scope binding does not travel, only a clip's own embedded grade. 13 new
  tests. Tools: 514 -> 515.
- New `style_captions_batch`: apply one style to every caption group in the project, or a named list
  of them, in one call -- `set_caption_style` already restyles every cue inside ONE group, but a
  project can hold several groups (different speakers, different tracks, an SRT import plus a later
  `caption_from_words` pass), and restyling all of them before this was one call per group. Reuses
  `set_caption_style` itself rather than reimplementing the per-cue styling math. 8 new tests. Tools:
  515 -> 516.
- `kimo gui` keyframe lane: a dropdown of every animated property on the selected clip (clip
  transform props and effect params, labelled with key count), and a lane showing that property's
  keyframes across the clip's own duration. Click a diamond to remove it, click empty lane space to
  add one seeded with the value already playing there (so adding a keyframe never silently resets the
  value). A click is classified existing-key vs empty-lane by mirroring `move_key`/`remove_key`'s own
  "nearest key within half a frame" rule. No dragging yet (left to `move_key` through the generic tool
  invoker). Pure shared module `gui_keyframes.js`. 21 Node tests + 5 Rust tests.

Also fixed along the way: `close_gaps` had no toolgroup heuristic match (`every_tool_has_a_real_group`
caught it at the mid-lane checkpoint); added to the `timeline` exact-match list.

Full workspace suite: 2590 passed, 2 failed -- both pre-existing and untouched by this release:
`readme_v3_tool_count_matches_the_registry` (the README's tool count is stale; fixing it is out of
scope for this lane) and a doctest in `robust_est.rs` (a vendored OpenCV license notice inside a `//!`
doc comment that rustdoc tries to compile as code; the file is byte-identical to v7.4.0). `kimo-core`
lib warnings held at 34 (budget 35), no new warnings introduced. Tools: 516.

## v7.4.0 (2026-10-30)

Naman's standing instruction continued: "keep working, and keep sending updates." Picked up straight
from v7.3.0 (508 tools). Re-audited the registry and FEATURES.md first (close_gaps/ripple delete,
batch caption styling, timeline diff and a generic rough-cut were all already covered pre-v7 --
confirmed and NOT duplicated); real gaps found and built instead. 8 features shipped, each with its
own TDD commit:

- New `track_mute_solo`: not from the audit's suggestion list, but a real pre-existing bug found while
  reading the audio path -- `Track.muted` has existed since before v7 and every audio-render/analysis
  path already honoured it, but no tool could ever set it. `set_track` now accepts `muted` and the
  standard NLE companion `solo` (solo-overrides-mute semantics, centralised in
  `model::effective_mute`). 9 new tests including a real ffmpeg-tone-silenced-by-mute test. Tools: 508
  (unchanged).
- New `export_edl`: the write-side counterpart to v7.3's `import_edl`. Exports the timeline as
  CMX3600 EDL, OTIO JSON (round-trips through `import_edl`'s own parser) or a minimal FCPXML. Non-media
  clips and non-1.0-speed clips are reported, never silently dropped or wrong. 10 new tests. Tools:
  508 -> 509.
- New `reframe_timeline`: v7.2's `auto_reframe` keys one clip; v7.3's `export_variants` touches every
  clip but restores everything after one export. This is the persistent, undoable whole-timeline
  aspect conversion that actually changes the canvas for real -- one undo step reverses the whole
  thing. Protects manually-framed clips unless `overwrite_existing`. 11 new tests. Tools: 509 -> 510.
- New `list_project_templates`/`create_project_from_template`: 5 starter templates (vertical/square
  social, 16:9 widescreen, podcast interview, multicam panel) with canvas/fps/named-track-layout, one
  call instead of a blank canvas plus manual `add_track` calls every time. 7 new tests. Tools:
  510 -> 512.
- `kimo gui` track mute/solo/lock controls: M/S/L buttons per track wired onto `set_track`; a locked
  track's clips stop accepting drag/trim in the GUI. Pure shared module `gui_trackstate.js`. 13 Node
  tests + 4 Rust tests.
- `kimo gui` multi-select and group move/delete: shift/cmd/ctrl-click selects several clips; dragging
  any one moves the whole group as one atomic `batch` call (one undo step); Delete removes them all
  the same way. Pure shared module `gui_multiselect.js`. 19 Node tests + 5 Rust tests.
- `kimo gui` clip waveforms: every media clip bar shows its real audio waveform, wired onto
  `audio_transients`' existing `image: "waveform"` SVG option (no new peak-computation code). Pure
  shared module `gui_waveform.js`. 15 Node tests + 5 Rust tests.
- `kimo gui` effects/transitions browser: the bare effect-kind `<select>` is now a real searchable
  list combining `list_effects` and `list_transitions` (two genuinely different catalog shapes),
  click to add to the selected clip. Pure shared module `gui_fxbrowser.js`. 12 Node tests + 4 Rust
  tests.

Also fixed along the way: `reframe_timeline`'s hand-rolled aspect-enum error message collided with
`dispatch_ex`'s own `invalid_value` short-circuit and tripped `registry_lint`'s
`every_schema_enum_rejection_lists_allowed_values` check (caught by the full-suite run, not by its own
feature commit); rephrased to the same "X must be ..., got 'Y'" shape `export_edl`'s own enum error
already used successfully. A filesystem-fixture race in the waveform tests (two tests sharing one
PID-keyed temp dir name, racing on parallel test threads) was found and fixed with a per-call counter.
Two trivial unused-`mut` warnings removed (zero behaviour change), taking kimo-core's own warning
count from 36 to 34.

`kimo-gui` (the separate Tauri spectator shell under `crates/kimo-gui`) is deliberately its own
workspace (see its Cargo.toml comment) and carries its own `6.0.0` version, unrelated to and not
tracking this workspace's version -- not touched by this release, by design, not oversight.

README.md's own tool count is out of date as of this release (not updated by this pass; a prior
release noted the same `readme_count` test as a known, deliberately-left failure).

## v7.3.0 (2026-10-30)

Naman's standing instruction continued: "make ai agent a complete editor," "better than any editor
in the world," "keep working, and keep sending updates." Picked up straight from v7.2.0 (504 tools).
Re-audited the registry and FEATURES.md first against the two stated directions -- GUI usability and
agent-superiority engine tools -- before writing anything: several GUI gaps were real (zero keyboard
shortcuts, no track add/remove, no marker display, no snapping, no zoom bounds, no project open/save
button, no export progress bar) even though the underlying engine tools (`add_marker`/`list_markers`,
`add_track`/`delete_track`, `snap_time`, `export_job_start`/`job_status`/`cancel_job`) already
existed; on the engine side, `apply_review_fixes`, `auto_chapters` and `batch_normalize_loudness`
were genuine gaps, and EDL/OTIO import was a real missing capability with nothing close to it in the
registry. 11 features shipped, each with its own TDD commit:

- New `apply_review_fixes`: batch-applies `lint_edit`'s (and so `self_review`'s) machine-actionable
  fixes in one call instead of reading each problem and dispatching its fix by hand. Skips anything
  needing a human decision (a placeholder value, an "or" choice) under `skipped_manual`, never
  guesses. `dry_run` supported. 5 new tests. Tools: 504 -> 505.
- New `auto_chapters`: composes `mark_scene_cuts` + `mux_chapters` -- `mux_chapters` alone needs
  markers to already exist; this detects them first if there aren't enough, and reports plainly
  (not a failure) when even detection can't reach the minimum. 4 new tests. Tools: 505 -> 506.
- New `batch_normalize_loudness`: the write-side counterpart to v7.2's `find_by_loudness` --
  normalizes every/filtered media clip to a target LUFS in one call instead of calling
  `normalize_loudness` once per clip by hand. `tolerance_db` skips near-target clips. 6 new tests.
  Tools: 506 -> 507.
- `kimo gui` keyboard shortcuts: space/J/K/L/Delete/S/ctrl+z/ctrl+shift+z, served as a real static
  module (`gui_keymap.js`) so the browser and a headless Node test run the exact same bytes. 18 Node
  tests + 6 Rust tests.
- `kimo gui` add/remove tracks: a "+ Track" button and a per-track remove button, wired onto the
  existing `add_track`/`delete_track`. 3 new Rust tests.
- `kimo gui` marker display + add: markers render as ticks on the timeline (click to remove), a
  "+ Marker" button adds one at the playhead, wired onto the existing `add_marker`/`list_markers`/
  `remove_marker`. 5 new Rust tests.
- `kimo gui` timeline snapping: drag-to-move/trim snaps to clip edges, markers and 0 within an 8px
  tolerance (toggle checkbox, default on) -- the exact gap v7.1's own FEATURES.md entry named. The
  snap decision is a pure module (`gui_snap.js`) shared by the browser and a headless Node test. 10
  Node tests + 5 Rust tests.
- `kimo gui` timeline zoom: in/out/reset, bounds-clamped 4-400px/sec, also a pure shared module
  (`gui_zoom.js`). 10 Node tests + 3 Rust tests.
- `kimo gui` export with progress: a path field, Export button, live progress bar, ETA with
  uncertainty band, and Cancel, wired onto the chunked export system that already existed
  (`export_job_start`/`job_status`/`cancel_job`). 3 new Rust tests, including a real export reaching
  "done" with the file actually on disk.
- `kimo gui` project open/save: a path field with Save/Open buttons wired onto `save_project`/
  `load_project`; a successful open actually re-renders the timeline and clears the stale selection.
  4 new Rust tests, including a real save-then-reopen that replaces the live project.
- New `import_edl`: the editorial-interchange gap -- kimo could export EDL-adjacent things but had no
  way to bring a cut list IN. Imports a standard CMX3600 EDL or a simple single-video-track OTIO JSON
  timeline as real clips, resolving each event's source name via an explicit `media_map`, a
  `media_dir` scan, or a path the file already names, reporting anything unresolved rather than
  dropping it or failing the whole import. 9 new tests. Tools: 507 -> 508.

Also fixed along the way: `batch_normalize_loudness`'s own description ran 256 characters before its
first ". " (limit 149, caught by the full-suite run, not by its own feature commit -- a contract bug
of this release's own making, fixed in its own commit); `import_edl.rs`'s one `read_to_string` call
needed registering in `external_refs.rs`'s open-sites classification table (a caller-supplied tool
argument, not project-derived, not stored in the project).

golden_registry.txt hand-updated at every new-tool commit (504 -> 508 registered tools); goldens
(golden_frames, golden_registry) green throughout. Full workspace test suite green apart from the two
pre-existing issues already on record (README tool-count phrasing, out of scope; the `robust_est`
doctest, a pre-existing doc-comment artifact unrelated to this release) -- `pick_thumbnail` and
`external_refs` each showed one transient/real issue during this session's own full-suite runs and
were confirmed fixed (the latter) or confirmed a CPU-load flake, clean alone (the former), same as
`director_small`'s long-documented sensitivity. Warning count unchanged (35). No push anywhere --
all 11 commits plus this release are local to the `v7` branch.

## v7.2.0 (2026-10-30)

Naman asked to keep shipping new editor features; partway through, steered toward favoring
"agent-superior" features -- things that make an AI driving kimo through MCP alone, with no GUI and no
human eye on the monitor, better able to do a full professional edit, over GUI-parity features. Audited
the registry (501 tools before this session's growth) and FEATURES.md first: several planned candidates
turned out to already exist under other names once checked (`review_export` already did most of what a
planned `export_qc` would have; `get_curve`/`move_key`/`remove_key` already existed before
`list_keyframes`/`remove_keyframe` would have duplicated them; `find_assets` already had duration/fps/
size before `asset_info` would have duplicated it) -- extended instead of duplicated in each case. The 8
features shipped, each with its own commit:

- New `validate_timeline`: a structural health check (gaps, overlaps, zero/negative-duration clips,
  negative starts) independent of `lint_edit` (platform/export-readiness) and `list_dependencies`
  (missing media) -- the one structural check neither of those already does. Read-only. 10 new tests.
  Tools: 498 -> 499.
- `get_curve`/`move_key`/`remove_key` grow an optional `effect_id` to reach keyframes on an effect's own
  params, not just a clip's transform properties (backward compatible); new `list_animated_properties`
  discovers what's keyframed on a clip without already knowing the property name. 11 new tests. Tools:
  499 -> 500.
- New `find_by_loudness`: whole-project loudness query in one call (Naman's own example: "all clips under
  -20 LUFS"), reusing the existing `measure_loudness` per clip via `dispatch_nested` rather than
  reimplementing loudness measurement. 8 new tests. Tools: 500 -> 501.
- `find_assets` grows into a shot list: `kind`/`hash`/`origin` (already on the `Asset` model, just
  unsurfaced), a new `used_by_clips` reverse-lookup count, and an opt-in `probe_audio` flag for
  `has_audio` (reuses `audio::source_info`, default off so the common call stays cheap). 5 new tests.
  Tool count unchanged (501) -- an extension, not a new tool.
- New `self_review`: one-call orchestration of `lint_edit` + `validate_timeline` + `list_dependencies`,
  correlating every source's flagged moments into one merged report instead of calling three tools and
  matching times up by hand; an optional `path` also renders a `contact_sheet` of what it found, so the
  report comes with evidence. Pure composition (`dispatch_nested`, no new checking logic); deliberately
  NOT auto-fix -- `lint_edit`'s own problems already carry the exact fix call each needs, and applying
  them unasked is outside this tool's scope. 8 new tests. Tools: 501 -> 502.
- `review_export` grows QC: checked for duplication first and found `review_export` already reports
  duration/size/width/height/LUFS/peak plus a contact sheet, so a separately-planned `export_qc` would
  have nearly duplicated it. Added `video_codec`/`audio_codec` (same ffprobe call) and an opt-in
  `expect_project` that compares the exported file's duration and resolution against what the LIVE
  project should currently produce (the same max-clip-end technique `lint_edit` uses), catching a stale
  or wrong-size export; an empty project reports no expectation rather than a false mismatch. Backward
  compatible. 7 new tests. Tool count unchanged (502).
- New `explain_cut` (BETA, best-effort): why a clip's incoming cut might exist. Built for an agent with
  no eye on the monitor -- it can't glance at a cut and judge it the way a human editor would, so this
  hands it the same evidence instead: the clip's own `transition_in`, nearby timeline markers
  (`add_marker`/`mark_scene_cuts`) within a configurable window, whether it's the same source asset as
  the clip before it (a split within one shot vs. a cut to new footage), and whether `detect_cuts` finds
  a real visual change right at the boundary. Says plainly when the evidence runs out rather than
  inventing a reason; not a real decision ledger (that would mean threading provenance through every
  mutating tool). 11 new tests. Tools: 502 -> 503.
- New `check_color_consistency`: the read-only diagnosis `match_color` (one clip) and `match_shots`
  (every clip, mutating, uniform) had no counterpart for -- which clips actually disagree in brightness/
  color before reaching for either fix, or whether a fix afterward actually closed the gap. No new pixel
  analysis: samples each clip's own midpoint through the existing `get_scopes`, compared against a named
  `ref_clip_id` or, by default, the checked group's own median. 9 new tests. Tools: 503 -> 504.

69 new TDD tests across the 8 feature commits, plus one small test-fixture fixup caught by this
release's own full-suite run: `self_review.rs`'s `clean_media_scene` helper built its fixture path from
a fixed tag, and two different tests sharing it wrote to the exact same path under the full suite's real
concurrency -- a test-fixture bug, not a `kimo-core` bug; fixed by giving each caller its own tag.
golden_registry.txt hand-updated at every tool change (498 -> 504 registered tools); goldens
(golden_frames, golden_registry) green throughout. Full workspace test suite green apart from the two
pre-existing issues already on record (README tool-count phrasing, out of scope; `director_small`'s
`lint_edit` timing test, which is sensitive to concurrent CPU load on this machine and passes clean
every time it's run alone -- confirmed repeatedly during this session). Warning count unchanged (35).
No push anywhere -- all 8 commits plus this release are local to the `v7` branch.


## v7.1.0 (2026-10-30)

Naman asked for new features added to the local v7 branch. Audited the registry (495 tools) and
FEATURES.md first: most of the obvious directions (beat-detection auto-cut, auto-reframe, silence/filler
trim, scene-change detection, loudness-normalized ducking, project diff, timeline search) already shipped
in v7.0.0 under different names (`beat_cut`/`detect_beats`, `auto_reframe`, `remove_silence_cuts`/
`trim_filler_words`, `detect_cuts`, `auto_duck`/`measure_loudness`, `project_diff`, `find`). The 8 real
gaps found, each with its own commit:

- `find` gains a `has_effect` filter (which clips carry a given effect kind) and lists every clip's
  effect kinds on every result -- the one real gap in the existing clip/track query tool. 1 new test.
- New `project_merge`: the write-side of `project_diff`, which only ever reported. Two-way clip-level
  merge (base -> theirs, same revision forms as `project_diff`) onto the live project, matched by id,
  with four conflict shapes detected against the common base (add_add, edit_edit, edit_removed,
  remove_modify) rather than just against each other, so an edit that happens to land on the same end
  state on both sides is never a false conflict. `on_conflict`: fail (default)/theirs/base, plus
  `dry_run`. 5 new tests. Tools: 495 -> 496.
- New `mark_scene_cuts`: automates the existing `detect_cuts`-then-`add_marker`-per-cut workflow into
  one call. `skip_existing` (2-frame tolerance) means calling it twice does not pile up duplicate
  markers. Not a duplicate of `scene_split` (splits clips) or `detect_cuts` (read-only). 4 new tests.
  Tools: 496 -> 497.
- New `export_variants`: batch-exports several size/aspect-ratio deliverables (named presets 16:9/9:16/
  1:1/4:5, or explicit width/height) from one project in a single call -- `batch` explicitly forbids
  `export*` tools, so this previously needed several manual exports with the project's canvas hand-edited
  in between. Optional `auto_reframe` per differing-aspect variant (window removed again right after);
  the project's own canvas is always restored, success or failure; variants are independent. 5 new tests.
  Tools: 497 -> 498.
- `kimo gui` deepened into an actually-editable local editor, in 4 commits:
  - Clip detail endpoint (`GET /api/clip?id=`) + an effect-params panel: select a clip, see and edit its
    properties and every effect's params live, add/remove effects -- all through tools that already
    existed (`set_property`, `set_effect_param`, `add_effect`, `remove_effect`, `list_effects`), no new
    dispatch path. 3 new tests.
  - Editable timeline: drag a clip to move it (`move_clip`) or drag its right edge to trim it
    (`trim_clip`); clip bars switched to absolute positioning (start * px/sec) so a moved clip isn't
    forced back into a gapless sequence. A sub-3px drag is treated as a click (selection), not a move.
    1 new test.
  - Scrub and play: no new backend route -- JS drives the existing `/preview.png?t=` repeatedly. Play
    steps forward on a wall-clock-paced tick (real elapsed time, so it tracks actual speed rather than
    drifting); clicking/dragging the timeline background scrubs; a playhead line stays in sync. Honestly
    a frame-stepped preview, not a video element. 1 new test.
  - Undo/redo + a history panel: buttons wired to the pre-existing (pre-v7) bounded undo/redo tools,
    which the GUI simply hadn't exposed yet; a history panel lists every undo/redo step by tool name.
    2 new tests.
  - 17/17 gui tests green; every GUI change was also hand smoke-tested against a live server (curl
    through the exact call sequences the JS drives), the way the original v7.0.0 GUI commit was.

22 new TDD tests across the 8 commits. golden_registry.txt hand-updated at every tool change (495 -> 498
registered tools); goldens (golden_frames, golden_registry, golden_project) unchanged throughout. No
push anywhere -- all 8 commits are local to the `v7` branch.

## v7.0.0 "studio" (2026-10-29)

- Particles (FEATURES.md L483): new `bubbles` and `foam` emitter presets, and a new `card` sprite kind (a flat
  rectangle whose half-width scales by `|cos(phase)|`, phase a pure function of time+seed -- a 2D stand-in for a
  3D card flip) with a `card_dance` preset built on it. Credited, not duplicated: `add_emitter`'s existing
  forces/gravity/wind/drag/turbulence/attractor/vortex/trails/colliders (analytic + layer-silhouette),
  `simulate_physics` (deterministic circle-proxy rigid bodies) and `shatter`/`shatter_cells` (glass-shard
  transitions) already covered most of this line pre-v7 and were uncredited; fixed in passing, a stale baseline
  in error_codes.txt (claimed `list_effects` returns 182 kinds; it returns 185 since an earlier lens-blur commit
  added 3 without updating it). 7 new TDD tests. Honest gaps: no real fluid/smoke grid solver, no cloth sim, no
  polygon/joint rigid bodies, no true cannon-driven Card Dance, no lens-droplet refraction.
- GPU pipeline (FEATURES.md L557): new optional `gpu2d` build feature (default off, separate from the existing
  `gpu3d`/wgpu mesh path so this doesn't pull in its dependencies) -- real WGSL compute shaders (`gpu_gaussian_blur`,
  separable 2-pass; `gpu_color_matrix`, an AE-style 4x5 matrix, single-pass) through wgpu's adapter/device/
  compute-pipeline API, run and proven on this box's software Vulkan (Mesa lavapipe): GPU-vs-CPU max_abs_diff
  measured at exactly 0.0 on both. Exposed as 3 read-only diagnostic MCP tools (`gpu2d_status`,
  `gpu_gaussian_blur_frame`, `gpu_color_matrix_frame`) -- standalone, not wired into `add_effect` or the default
  render path, so every golden reference frame is unchanged. Tools: 490 -> 493 registered. Honest gaps: no GPU
  compositing of the main pipeline, no persistent VRAM frame textures, no async readback, no NVENC/hardware GPU
  (lavapipe is software-only).
- MOGRT substitute (FEATURES.md, title/lower-third line): new `export_template_package` / `import_template_package`
  (ext/templates.rs) bundle a named template (the pre-existing `template_define`/`apply`/`list`/`describe`/
  `delete` parametric batch-template system) together with its clips' `essential_properties` marks (also
  pre-existing, the MOGRT-style "which knobs to expose" tags) into one JSON file -- Kimo's own open one-file
  substitute for Adobe's closed/proprietary `.mogrt` container. Round-trips within or across projects: import
  matches essential marks back onto clips by role first, falls back to the original clip_id, and reports (never
  silently drops) anything unmatched. 5 new TDD tests. Tools: 493 -> 495 registered. Honest gap: no Adobe
  `.mogrt` IMPORT (closed format, not attempted) and no thumbnail/preview asset in the package.
- GUI (FEATURES.md L562, the one item this project had marked `[ ]`): new `kimo gui [--project p.kimo]
  [--port 4567] [--bind 127.0.0.1]` -- a minimal local web UI (crates/kimo-cli/src/gui.rs + gui_index.html,
  std::net only, no new dependency) over the exact same tool registry/dispatch the CLI and MCP server already
  use. A timeline view (GET /api/timeline), a preview frame rendered by the real engine (GET
  /preview.png?t=<seconds>, the same render_png export/render_frame use), and a tool-invocation panel (GET
  /api/registry, POST /api/call running through the same `run_tool` path as `kimo call`). Headless-testable: its
  whole routing table is a pure `handle_request(session, method, path, query, body)` function with no socket in
  it (12 new tests), smoke-tested by hand against a live server. Credited alongside the pre-existing (and
  previously uncredited) **Kimo Spectator** desktop app (crates/kimo-gui, its own Tauri workspace): Spectator is
  a read-only live view of an agent's own edits (no IPC commands exist, by construction); `kimo gui` is the new,
  complementary piece -- something a human can actually drive. Honest gaps: no JS framework or build step
  (one small inline page, on purpose), no scrubbing/playback, no drag-to-trim/drag-to-move on the timeline
  (read-only, not yet editable), no auth (loopback tool), no multi-client session sharing, no desktop packaging.
- FEATURES.md: every remaining `[ ]` item is now `[x]` or honestly `[~]` (57 `[x]`, 48 `[~]`, 0 `[ ]`).
- Known, unchanged in this release: `readme_count` fails because README.md's tool-count badge text does not
  contain the literal phrase "registered tools" the test looks for (not a wrong number -- the phrase is simply
  absent); the fix is a one-line README wording change, out of scope for this pass (README.md was intentionally
  left untouched). `robust_est`'s doctest fails to compile on an unrelated pre-existing doc-comment issue
  (a "License Agreement" line inside a doctest code block that rustdoc tries to parse as Rust). Both predate this
  release and are unrelated to anything shipped in it.

## v6.0.0 "eyes" (2026-10-27)

- Added `transcript_search`: normalized phrase lookup with script indexes, timeline ranges and ready-to-use delete operations.
- Added `transcript_edit_apply`: revision-checked, atomic all-track ripple deletion with transcript/caption/marker updates, source/keyframe continuity, dry-run validation and undo/redo. The first version supports explicitly bound normal-speed media and rejects ambiguous edits. See [transcript editing](docs/TRANSCRIPT_EDITING.md).
- `transcript_edit_preview` now returns a project revision and rejects invalid `to_index` types. Added Windows guards around Unix-only code-clip process-group calls.

- `vignette` is shaped and tracked: new params `center_x`/`center_y`, `roundness` (0 frame-shaped .. 1 true circle), `squareness` (ellipse .. rounded rectangle), `size`, `aspect`, `feather`, `color_r/g/b` (tint the edge instead of darkening), and the centre can follow a clip moved by `track_point` (`strings.source_clip`) or a mask layer such as one `track_mask` keyed (`strings.source_mask`, its centre / mean point). A call with only `amount`/`softness` renders exactly as before.
- New transition kind `film_burn`: a seeded hot burn blooms from a point with a noise-ragged edge, blows the frame out orange/white at the cut, flickers, then burns off into the incoming shot (`intensity`, `seed`).
- New tool `flash_series` (group `scopes`, read-only): per-frame linear relative luminance (sRGB decode, Rec.709 Y), red saturation R/(R+G+B) and red quantity, luminance and red transitions, changed-area fractions, and per-window transition counts with window start times. Window length and thresholds are arguments; raw numbers only, no pass/fail. Harding / ITU-R BT.1702 / WCAG reference values are in the tool description only. Tools: 376 → 377 registered, flat `tools/list` 378 → 379.
  Next (documented, not implemented): the WCAG 2.2 red-flash rule (a 0.2 change in CIE 1976 u′v′); only the WCAG 2.0 (R−G−B)×320 red measure is computed today.
  Per-window flash counts come two ways: `lum_flashes` / `red_flashes` count a flash in every window it touches, `lum_flashes_fully_inside` / `red_flashes_fully_inside` only where both of its transitions fall inside the window. `params_used.max_resolvable_hz` reports fps / step_frames / 2. Next (not in this release): an optional `min_area_fraction` argument so a flash over a small part of the frame can be left out of the counts; today `changed_area_fraction` is reported but does not feed the counts.
  Transition stamps: a luminance or red transition is stamped on the first frame that crosses `lum_delta` (or `red_delta`) inside the monotonic run that reaches the swing's extreme (strict-run guard), so the stamp marks where the change starts, not where a slow drift after it peaks; no tolerance argument. On the jaja ad the six luminance stamps move from 4.958 … 34.708 s to 4.750, 9.667, 14.750, 24.750, 29.708, 30.542 s; counts unchanged. The spectator's info panel lists the stamps on a Stamps row.
- `measure_loudness` (extended, no new tool): new optional arguments only. `curve` (momentary / short_term / both) adds ungated EBU Tech 3341 momentary (400 ms) and short-term (3 s) LUFS series every `hop_s` (0.1–3 s in 0.01 s steps, default 0.1; a value outside that range is an error, never clamped) with each series' max, and `max_points` (10–100000) raises the hop so each series holds at most that many readings; a curve over 100000 readings is refused with an error naming the fix; `lra` adds the EBU Tech 3342 loudness range (−70 LUFS absolute and −20 LU relative gates, 95th − 10th percentile) with its low/high levels; `clip_threshold_dbfs` adds clipped sample and frame counts. All three reuse the engine's own BS.1770 K-weighting (no new dependency). The existing `true_peak` stays: 4x oversampling with a 64-tap-per-phase windowed sinc, the BS.1770-4 Annex 2 method, now pinned by an fs/4 intersample test. Raw numbers only. With none of the new arguments the output is byte-identical to v5.0.0 (pinned by a test). Tools unchanged: 377 registered, flat 379. The audio panel shows LRA when it is returned.
- New tool `audio_spectrum` (group `audio`, read-only): averaged magnitude spectrum and band energies in dBFS of the final timeline mix or one clip (post filters, before volume/pan, as `measure_loudness`), optional `start`/`end`. Periodic Hann window, `fft_size` a power of 2 from 256 to 65536 (default 4096), frames every `fft_size`/2, L and R power averaged. `peak_hz`/`peak_dbfs` and `spectrum` are amplitude dBFS (a full-scale sine centred on a bin reads 0); `total_dbfs` and band `dbfs` are power dBFS (RMS level, a full-scale sine reads -3.01, as ffmpeg astats). `bands`: octave (default, IEC 61260 base-ten, 31.5 Hz-16 kHz), third_octave (25 Hz-20 kHz) or custom edges in Hz. `spectrum_points` (default 256, 0 omits) caps the spectrum as log-spaced points; `hop_s` adds `band_series` (band levels per hop), `max_points` raises that hop the way `measure_loudness`'s curve does and both `requested_hop_s` and `hop_s` are returned; a series over 200000 values is refused. Out-of-range arguments are errors, never clamped; silence reads null. New dependency: rustfft 6.4.1 (MIT OR Apache-2.0; pulls in primal-check 0.3.4, strength_reduce 0.2.4, transpose 0.2.3, all MIT OR Apache-2.0). Spectator: a qa rail row and info panel rows (Range, Peak, Loudest band, Total) read only the result's top-level numbers, so the bands and spectrum arrays the event cuts to 8 entries are never read.
- `diff_frames` (extended, new optional arguments only) and new tool `compare_ranges` (group `output`, read-only): full-reference picture metrics on luma, our own code from the published formulas. Luma is Y' = 0.2126 R' + 0.7152 G' + 0.0722 B' (BT.709) on the display-referred R'G'B' kimo renders, data range 255. PSNR = 10 log10(255²/MSE); identical frames return `"psnr_db": null` with `"identical": true` (JSON has no infinity), or `psnr_cap_db` when given (which also caps the rest). SSIM per Wang et al. 2004 (11×11 Gaussian, σ 1.5, K1 0.01, K2 0.03, valid windows, mean of the map); MS-SSIM per Wang et al. 2003 (5 scales, weights 0.0448/0.2856/0.3001/0.2363/0.1333, needs 176 px on the short side; a negative term returns null, never NaN). `diff_frames` takes `quality: ["psnr","ssim","ms_ssim"]` and adds a `quality` object; the existing v5 `metrics` (PSNR 99 on identical, 8×8 block SSIM) are untouched and without `quality` the output is byte-identical to v5 (pinned by a test against bytes captured from the 5ad355c binary). `compare_ranges` compares two sources or time ranges (`a`/`b` = `{source, start}`, shared `duration`) every `hop_s` (whole ms, default 1) or `step_frames`, returning `t_s` with `psnr_db` / `ssim` / `ms_ssim` curves, `identical_count` and a summary per metric (mean, min with its time, p5, max); `max_points` (2–2000) raises the hop the way `measure_loudness`'s curve does and returns both hops, and more than 2000 samples without it is an error. Out-of-range arguments are errors, never clamped. Spectator: `diff_frames` gains a Quality row when `quality` is passed, and `compare_ranges` shows Range / PSNR / SSIM / MS-SSIM rows read from the summary's scalars only; the `diff_frames` Compared row now wraps instead of being cut by an ellipsis.
- `get_scopes` (extended, new optional arguments only; without them the output is byte-identical to v5): `percentiles` (1..32 values in 0..100, strictly ascending) adds `luma_percentiles`, `chroma_percentiles` (the BT.709 vectorscope radius sqrt(Cb²+Cr²), Cb = (B′−Y′)/1.8556, Cr = (R′−Y′)/1.5748), per-column `waveform_percentiles` (`waveform_columns` 1..256, default 16 or the frame width when narrower), `cb_mean`, `cr_mean` and `chroma_mean`; `zones` [cols, rows] (1..32 each) adds per-zone `zone_crushed_blacks_pct` / `zone_clipped_whites_pct` (row-major) and their maxima at `crush_below` / `clip_above` (0..1, defaults 0.02 / 0.98, the v5 constants). Luma is the v5 luma; percentiles are numpy's linear interpolation. Raw numbers, no verdict; bad arguments are errors, never clamped. The spectator's get_scopes card shows the luma spread, chroma mean and worst zone when they are present.
- New tool `contrast_ratio` (group `scopes`, read-only): the WCAG 2.x contrast ratio (L1 + 0.05) / (L2 + 0.05) between two colours (`"#rrggbb"`, `"#rgb"`, [r,g,b] 0..1) or two pixel rects `{"rect": [x,y,w,h]}` of the frame at `time`, or one of each, with each side's relative luminance (sRGB EOTF with the 0.04045 knee, Rec.709 weights; a rect's luminance is the mean of its per-pixel luminance, with p5 / p95 / min / max beside it). Optional `cvd` (protan, deutan, tritan) adds `ratio_protan` / `ratio_deutan` / `ratio_tritan` under the Machado, Oliveira & Fernandes 2009 colour-vision-deficiency matrices at `severity` 0..1 (default 1; the published 0.1 steps, linearly interpolated), applied to linear RGB and clamped to 0..1. Raw numbers, no pass/fail; bad arguments are errors, never clamped. Spectator: Ratio, Luminance a / b and Under CVD rows.
- `query_safe_margins` (extended, new optional argument only; without it the output is byte-identical to v5): `rects` (1..64 of `{name?, rect: [x,y,w,h]}` canvas pixels, may reach past the canvas, each coordinate within ±65536, w and h > 0, unknown keys refused) adds `overlaps`: per rect `area_px`, `outside_canvas_pct` and per zone `overlap_px` / `overlap_pct` (exact rectangle intersection with the zone rects as returned); for safe zones also `outside_pct`, `crossed_edges` (`[{edge: top|bottom|left|right, px}]`, how far the rect's edge lies past the zone's edge) and `suggested_shift` (`{dx, dy}`, the smallest move that puts the rect fully inside, rounded to 0.01 px; `null` when the rect is wider or taller than the zone). Top level: `rects_outside_safe_max_pct`, `rects_in_avoid_max_pct`, `worst_edge`, `worst_edge_px` (0 when nothing crosses), `worst_edge_rect`, `worst_edge_zone` (`null` when the profile has no zone of that kind). Raw numbers, no verdict. Spectator: with `rects` the read card shows Profile, Max outside safe, Max in avoid and Worst edge (`left 52 px · rect1`); without them Profile, Zones, Estimated and Source, with Source wrapped (the generic row clipped `documented-constant`).
- `get_scopes` description: the first sentence (145 characters) now names the numeric arguments, "Video scopes at a time: histogram, luma stats, clipping, plus optional brightness/colourfulness percentiles and per-zone blown-out/crushed share." The v5 sentence (155 characters) did not mention them. Output unchanged. New test `tests/description_first_sentence.rs` checks every registered tool's first sentence is under 150 characters; the 77 v5 tools already over it sit on a frozen, sorted allowlist that can only shrink (the test also fails when an allowlisted tool comes in under 150).
- `flash_series` (extended, new optional arguments only; without them the output is byte-identical to the previous build): `flash_limit` (> 0, <= 1000; 3 for the 3-flashes-per-second rule) adds the flash rule numbers as top-level scalars: `worst_window_start` / `_end` / `_flashes` (the earliest window with the most flashes, a flash counted in every window its span touches, as `lum_flashes`), `windows_over_limit` (windows with more than `flash_limit` flashes), `flash_area_pct_max` (a flash's area is the larger `changed_area_fraction` x 100 at its two transitions), `flashes_counted`, and the same six for red (`worst_red_window_*`, `red_windows_over_limit`, `red_flash_area_pct_max`, `red_flashes_counted`), plus `flash_rule_method`. `area_min_pct` (0..100, needs `flash_limit`) leaves out flashes whose area is smaller. Raw numbers, no verdict. Spectator: the flash_series card gains a Worst 1 s flashes row when the call gave `flash_limit` (`5 @ 5.92s, 27 > 3, area 98% · red 5 @ 7.67s, 15 > 3, area 25%`).
- `flash_series` pinned to the WCAG 2.x flash definitions (new optional `area_basis`, needs `flash_limit`; without it the output is byte-identical to the previous commit, pinned by tests/flash_pin.rs against three calls captured from that build). Definitions used: 1 s windows start at every sample (window_hop_s defaults to one frame), so a burst across a second boundary counts together; a flash is one pair of opposing transitions (rise then fall or fall then rise), a transition being a swing of >= 0.1 relative luminance (10 % of the maximum, 1.0) whose darker state is < 0.80, and a red flash the same pairing on saturated-red transitions (R/(R+G+B) >= 0.8, a (R-G-B) x 320 change >= 20); a flash counts in every window its span touches and a window with more than `flash_limit` (3) flashes counts in `windows_over_limit`. `area_basis: "field"` applies this per 10 degree visual field: 341 x 256 px at 1024 x 768 scaled per axis to the analysed frame (639 x 360 at 1920 x 1080; `field_px`), fields on a half-field grid (`fields`); each field's own mean luminance (red: mean red quantity) is paired into flashes, a flash counts only when MORE than `area_min_pct` (default 25) of that field changed, and the rule scalars are the worst field's, at `worst_field_rect` / `worst_red_field_rect`. `area_basis: "frame"` is the whole-frame reading. Fixtures: full-frame 4 Hz strobe fails, full-frame 2 Hz strobe passes, a 4 Hz strobe on 2.6 % of the frame (23.8 % of its field) passes; a compact box on 9.1 % of the frame (82 % of its field) fails on the field basis, which the whole-frame reading misses. GUI: on the field basis the flash_series card's Worst 1 s flashes row ends with the worst field (" · field x,y w×h").
- `flash_series` area_basis field: the 10 degree field now slides in steps of 1/8 field per axis (was a half-field grid), so any flashing patch up to 7/8 of a field per axis lies wholly inside one field wherever it sits; a patch straddling the old grid lines is no longer split. Field sums come from box sums (O(pixels) per sample). Fixtures in tests/flash_pin.rs: a 2.6 % strobe centred on a corner of the old grid passes like an uncentred one, a field-size strobe there fails; a 72x18 / 72x19 box (23.8 % / 25.1 % of the 85x64 field) passes / fails at five positions, where the old grid saw at most 20.3 % of the 72x19 box and passed it; a compact 4 Hz strobe on 9 % of the frame fails at 4:3 and 16:9. Default and flash_limit-only output unchanged; jaja + strobe counts unchanged (lum 12 counted, worst 5 @ 5.92 s, 57 windows over 3; red 5, worst 5 @ 7.67 s, 15 over), only the worst field rect moves (fields 30 -> 306).
- New `text_contrast_series` (scopes group): worst-frame WCAG 2.x contrast of text over moving video. Per frame of a range (step_frames, default every frame; with step_frames > 1 the frames between two samples around a cut are measured too, so every 2nd frame plus every cut is one call): the text colour or a text rect against the background luminance read as bg_stat (default worst = whichever of p5 / p95 gives the lower ratio; p95 = the brightest 5 %) of the ring around text_box (burned-in text), the pixels under text_box (bg_region box) or a background rect. Reports the WORST frame with its time, frame and ratio (no averages), best ratio, frames_sampled, cut_frames_added. text_px (+ bold, true only for weight 700+; semibold counts as normal) gives size_class at 1080 p and the bar: 3:1 only at 24 px+ or bold 18.66 px+, else 4.5:1; threshold overrides. Static images (contrast_ratio) use the worst pixel; moving video uses p95, the brightest 5 % of the background. A bar adds frames_below, pct_below, first_below_time, longest_below_s and, when frames fall below, suggested_fix scrim with scrim_alpha_min (the smallest opacity of scrim_color, default black, that brings the worst frame to the bar, linear-light blend): the fix is a scrim, never a recolour. Cuts inside the range (detect_cuts' score >= cut_threshold and, unless cut_guard is false, detect_cuts' motion guard: above 3x the mean of the 5 scores before it + 0.02) give cut_count, cut_times and crosses_cut, which flags text on screen across a cut. captions (name, text_box, start, end each) measures every caption over its own time on screen in one call: per caption the worst frame, time and ratio; the worst caption; captions_below / below_names; crossing_cut lists each caption on screen across a cut by name with its cut times. text_min_px (needs text_box and a text colour) counts a frame only when at least that many pixels inside text_box read as text (closer in contrast to the text colour than to the background), so a word-by-word reveal or an empty scrim before the first word is not measured; frames_without_text says how many were left out (absent unless text_min_px is given). Reads the output pixels (what render_frame writes, 8-bit). Raw numbers.
- New tool `edit_check` (group `output`, read-only): did an edit stay inside its allowed rects. Two frames (`a` / `b`, diff_frames side objects) compared at full resolution with no downscale; a pixel is changed when any RGB channel (0..1) differs by more than `threshold` (default 0.04). `allowed` (up to 64 `[x,y,w,h]` rects; absent = nothing allowed) splits the changed pixels into `inside` (count, % of frame, % of the allowed area) and `outside` (count, % of frame, % of the rest, `max_delta` with its pixel over every outside pixel changed or not, `bbox`, regions touching it). Changed pixels form 8-connected `regions` (bbox, pixels, outside_pixels, max_delta), largest first; `max_regions` (default 32, up to 256) are listed, `region_count` counts all, `min_region_px` leaves smaller ones out of the list. Raw numbers, no verdict; no existing tool's output changes. Spectator: an info panel read card whose header carries the threshold ("edit_check · threshold 0.2", from the result) and 4 rows: Changed outside band "n px (max Δ there d)", Changed inside band "n px", Max Δ overall, Regions.
- Descriptions: the 77 tools whose first sentence was 150 characters or longer in v5.0.0 each get a new plain first sentence (all under 150; the longest is 128) in front of their unchanged old text, so a client that shows only the first sentence reads what the tool does and no detail is lost. The first-sentence test has no allowlist any more: every tool's first sentence must be under 150 characters. Schemas and behaviour are unchanged.
- `validate_captions` gains an optional `legibility` (boolean, default false; absent or false = the v5 output, byte for byte): per caption group a `legibility` block with, per cue at its middle frame, the capital-H ink height of its style (`cap_px`, and `cap_frac` of the canvas height, measured as lint_frame's min_text_size), the ink box from compute_text_bounds, `chars` (newlines left out) and `cps` per second on screen, and the WCAG contrast of the caption colour at its opacity against the frame rendered without that cue, every pixel of the ink box (`min_ratio`, `p10_ratio`, `mean_ratio`, `worst_px`); the group's `min_cap_frac`, `max_cps` and `min_contrast` with their cue ids; and a top-level `legibility` summary over all groups. Raw numbers only, no verdict; at most 500 cues per group. Spectator: with legibility the Inspector rows read Captions, Min caps, Lowest contrast, Max reading.
- `lint_frame` (extended, new optional argument only; absent or false = the v5 output, byte for byte, timings aside): `quality` (true, or an object with `crush_below` and `clip_above` in 0..1, defaults 0.02 / 0.98, and `band_step` 1..32 8-bit luma codes, default 2) adds per sampled frame a `quality` block on Rec.709 luma of the output values: `mean_luma`, `crushed_frac` (at or under crush_below), `clipped_frac` (at or over clip_above), `channel_clipped_frac` (any RGB channel at or over clip_above), `luma_levels` (distinct 8-bit luma codes), banding on 8-bit luma (`band_steps_per_row`: horizontal neighbour steps of 1..band_step codes per row; `band_plateaus` and `band_plateau_px`: the flat runs those steps bound and their mean length) and the change from the frame one project frame earlier (`prev_mean_delta`, `prev_max_delta`, `prev_identical`; null at t = 0). `summary.quality` carries the extremes with their times (`max_crushed_frac`, `max_clipped_frac`, `max_channel_clipped_frac`, `min_luma_levels`, `max_band_plateau_px`, `min_mean_luma`, `min_prev_mean_delta`), `identical_to_prev` with its times, and the params used. Black frames read crushed 1 / mean 0, frozen or duplicate frames read identical to the frame before. Raw numbers, no verdict. Spectator: with quality the lint_frame card adds Crushed / clipped and Banding / frozen rows.
- `lint_frame` (extended, new optional argument only; absent or false = the v5 output, byte for byte, timings aside): `decode_media` (boolean, default false). The text_contrast check renders the frame without the text layer as its background; in v5 that render used an empty render cache, so media layers drew black and white text over footage read 21:1. With `decode_media: true` the media frames are decoded into the session cache first (as `validate_captions` legibility does), so text_contrast measures against the real footage. On white captions over files/jaja/jaja-ad.mp4 the three cues read min 1.02 / 1.00 / 1.27 : 1 with it (the same numbers and worst pixel as `validate_captions` legibility) and 21.0 without it.
- `render_flow` (extended, new optional argument only; absent or false = the v5 output, byte for byte, timings aside): `stats` (true, or an object with `grid` 1..16, default 4, and `moving_px` 0..1000, default 0.5) adds `flow_stats`, the same dense flow field the picture is drawn from (between the clip's displayed frame at `time` and the next one) as raw numbers in canvas pixels per frame, +u right and +v down: `mean_u` / `mean_v`, `median_u` / `median_v`, `mean_dir_deg` (0 = right, 90 = down; null when the mean vector is zero), `mean_speed`, `speed_p50` / `speed_p90` / `speed_p99` / `speed_max` with `speed_max_at`, `moving_frac` (share of the field faster than moving_px), a grid x grid table of mean vectors and speeds, the field size and the params used. A content moved 4 px right reads a median vector of (4.0004, 0) in tests/flow_stats.rs. Raw numbers, no verdict. Spectator: with stats a read card shows Motion (mean vector length and direction), Speed (p50 / p90 / max) and Moving rows; without stats the screen is the v5 screen.
- New tool `saliency_map` (group `output`, read-only): where the eye is likely to go in one output frame, as raw numbers. Spectral residual saliency (Hou & Zhang 2007) on Rec.709 luma, area-averaged to `work_px` on the long side (default 64, 16..512), log amplitude minus its 3x3 average (edges replicated), original phase, squared magnitude, Gaussian `blur_px` (default 2.5 grid cells, 0..32), min-max scaled to 0..1 (a constant frame gives an all-zero map and `empty: true`). Optional `motion` (0..1, default 0) mixes in the luma change to the next project frame. Returns the peak and the mass centroid / spread in canvas px, `half_mass_area_frac`, a `grid` x `grid` mass table (default 3, 1..8), and per rect `[x,y,w,h]` (up to 16) `area_frac`, `mass_frac`, `mass_per_area`, `max` and `top10_frac`; `path` writes the map as a png. No verdict. Spectator: read card Peak / Half the mass in / In rect 1 / Motion.
- Spectator: a window too small for a screen's fixed panels plus a minimal viewer (160 x 260 px at the design scale), e.g. 640 x 360, now lays the app out at the smallest size that fits and scales it down to the window, so no panel (and no Inspector card) runs past the window edge. At every size that already fits (1100 x 768 and up) nothing changes: a 1920 x 1080 screenshot is byte-identical before and after. Timeline wheel-zoom divides the pointer offset by that scale.
- Spectator: the `flash_series` READ card's Transitions row reads "max 8 luminance · 8 red per 1s · area 98%" stacked under its label (full card width) instead of the old wording wrapped in the value column: 3 lines (64.8 px at 1920 x 1080) instead of 6 (129.6 px). Same three numbers; the timeline rail and the toast keep their wording.
- Spectator: the `flash_series` READ card's "Worst 1 s flashes" row (only drawn with `flash_limit`) reads "5 @ 5.92s, 27>3, 98% · red 5 @ 7.67s, 15>3, 25%" stacked under its label: 3 lines (label + 2, 64.8 px at 1920 x 1080) instead of 8 beside the label (172.8 px). Same numbers (flashes in the worst 1 s window @ its start, windows over the limit, largest flash area); the word "area" and the spaces around ">" are dropped to fit two lines. At 640 x 360 the card's top moves from -62.3 px to 2.2 px in the window; the Inspector panel (top 32.8 px there) still clips the card's header and Range row. With `area_basis: "field"` the row needs 4 lines and the card still starts at -23.6 px (was -101).
- Spectator, small windows only (the app scaled down, i.e. under 1100 x 768): a screen whose Inspector shares the viewer's rows gives it at least 370 design px, taken from the tallest row of the band under it, which keeps at least 120 (edit / graphics: the timeline; graph, modelling, particles: their bottom panel). At 640 x 360 the Inspector grows from 155.3 to 220.9 px and the timeline shrinks from 150.5 to 84.8 px (window px). The flash_limit READ card (top 67.9) and the flash pin card (top 42.1) are fully visible under the top bar, header and Range row included. 640 x 640 is unchanged (the Inspector is already taller there), and every size from 1100 x 768 up is untouched (1920 x 1080 byte-identical).
- Spectator, small windows only (under 1100 x 768): the Inspector's least height beside the viewer goes from 370 to 402 design px and the band under it keeps at least 110 (was 120), so the tallest READ card (the flash pin one, 341.5 design px) sits under the panel's own "Inspector" title instead of over it. When that takes rows from the timeline, its plain track rows draw compact (top 34, height 30, gap 6 design px instead of the mode's 38 / 35 / 9.5; text sizes unchanged, track label and clip name 16 px = 9.55 px in the window), so the shorter band still holds 2 whole tracks. At 640 x 360 (window px) the Inspector grows from 220.9 to 240 px and the timeline shrinks from 84.8 to 65.7 px; the flash pin card moves from 42.1..246 (over the title, 33.4..59.7) to 61.2..265.1 and the flash_limit card from 67.9..246 to 87..265.1, header and Range row visible on both; tracks V1 299.1..317 and V2 320.6..338.5 are whole (were 282.4..303.3 and 309..329.9). 640 x 640 is unchanged, and every size from 1100 x 768 up is untouched (1920 x 1080 byte-identical).
- Spectator: the Loudness panel no longer puts a bare "target" under a measured value. After `measure_loudness` the big number is the measured integrated loudness (EBU R128), and the line under it read "target" with no value, so it looked like "-15.8 LUFS target". That line now reads "Integrated" (16 px, 9.55 px in the window at 640 x 360) whenever no target exists, and keeps "target <n>" only when `normalize_loudness` set one. 1920 x 1080: only the label's pixels change, on screens that show the Loudness panel. GUI only; new test `tests/loudness_label_row_test.py`.
- Spectator: a flash that runs out no longer stays in the page. A call's flash is a stamp read for 700 ms (FLASH_MS); nothing repainted an idle screen when it ran out, so the last frame's `.flash` class (a History row, a READ card) stayed until the next event, and a re-read 720 ms later (cov_c's "panels byte-identical after the failure") could see it. The app now holds one timeout at the earliest stamp + 700 ms whose callback repaints directly, with no animation frame (router_test's idle transform check still reads 0 rAF), re-arms for any later stamp still drawn, then goes idle; hiding or leaving the page cancels it. 1920 x 1080: on the six standard screens the READ card loses its stale `flash` class (and with it the `will-change: transform` layer), so its pixels change slightly in text anti-aliasing. GUI only; new test `tests/flash_expiry_row_test.py`.
- New tool `av_offset` (group `audio`, read-only): how far the sound sits from the picture. Audio onsets (positive jumps of short-time RMS over `onset_window_ms`, default 10, on a 1 ms grid) are cross-correlated (Pearson) with the picture's frame-to-frame change (mean absolute Rec.709 luma difference on a copy scaled to `scale` px, default 160) for every lag in ±`max_lag_ms` (default 500, up to 2000). Returns `best_lag_ms` / `best_r` (positive = the sound comes after the picture, audio late), `r_at_zero`, the runner-up peak more than a frame away and `peak_margin`, the curve every `curve_step_ms` (default 10) and the 8 biggest picture changes. `source` = timeline (default) or clip:<id>; `audio` = mix, a clip id or {clip_id|track_id}; `start` / `end` (4..6000 frames). Raw numbers only, no verdict; bad arguments are errors, never clamped; silent audio gives null, never 0. Existing sync tools (sync_clips, audio_transients) are unchanged. Spectator: a read card (Best lag, r there vs at 0, Runner-up, Biggest picture change). Tests plant known shifts and read them back within one frame (they read exact): +120 / -80 ms and an in-sync control reading 0 on the timeline, with a 2 s quiet head, and on files rendered by the engine's own export, including a 44.1 kHz source rendered at 48 kHz.
- Spectator: the `flash_series` info panel's Stamps row now reads a compact `lum_stamps` / `red_stamps` list (`[{t, dir}]`, up to 64, plus `*_stamps_total`) that kimo-mcp adds to the complete event from the FULL result. Before, it read the result's `frames` arrays, which the spectator feed cuts to 8 entries plus a `…+N` marker, so a live session showed `Stamps ↓0s` instead of the real stamps. A `…+N` string or a non-finite time is skipped, never read as 0. Telemetry only: the tool's MCP output is unchanged.
- New tool `frozen_frames` (group `output`, read-only): runs of frames where the picture stands still, each with its start frame, end frame, length and the largest pixel difference inside it (`max_diff` 0..1 and `max_diff_255`). Every project frame in `start`..`end` (default the source span, 1..6000 frames) of `source` = timeline or `clip:<id>` is rendered at full resolution (or `scale` px on the long side) and compared with the first frame of its run; `tolerance` (default 0 = identical) and `min_run` (default 2) set what counts. Comparing with the run's first frame keeps a slow drift from chaining into a hold; black frames get no special case. Rendered fixtures in `crates/kimo-core/tests/frozen_frames.rs`: a known 12-frame hold reported exactly, a slow pan that reports nothing (close to the line: passes via edge rows; edge-free pan at about 1.1 counts per frame reads as hold for frames 0-5 at 0.0255), a grainy still caught only at the stated tolerance (close to the line: 5/255 measured and 6/255 worst case by construction against 6.5/255), and a labelled known-limitation test that pins that edge-free pan as current behaviour. Limit, also in the tool description: each frame is compared with its run's first frame, so a total drift from that frame within tolerance still reads as a hold; a very slow pan reads as a hold until its total drift crosses the line, and nothing under the line is detected. Spectator GUI: a read card (Held, Longest run, Tolerance, Frames read).
- New tool `black_frames` (group `output`, read-only): runs of near-black frames, each with its start frame, end frame, length and the brightest pixel inside it (`max_luma` 0..1 and `max_luma_255`; 0 for pure black). A frame counts only when the Rec.709 luma of EVERY pixel is at or under `luma_max` (default 0.02 = 5.1/255, the pre-registered fixture line; 0 is strict mode), so letterbox bars, a black corner, a dark scene with a brighter object or a black frame with a logo are not runs. Every project frame in `start`..`end` (default the source span, 1..6000 frames) of `source` = timeline or `clip:<id>` is rendered at full resolution; `min_run` (default 2) drops a lone black frame. Limited-range video is expanded at decode (video black 16 reads as 0); a limited-range file wrongly tagged full range is the stated limit, made visible: the report carries each media clip's declared `range_tag` (tv, pc or unknown) and lists stretches that sit flat at about 16/255 in `limited_range_suspect` with a `note`. A black run that is also still is reported by `frozen_frames` too, with the same frame numbers (tested; nothing dedupes them). Rules and predictions were committed before the tool existed (tests/data/black_frames/THRESHOLD*.md). Spectator READ card (5 rows: Black, Longest run, Line, Range, Frames read).
- `black_frames`: the units sentence (luma_max on a 0..1 scale; max_luma 0..1 and max_luma_255 on the 0..255 code scale; luma_max is compared to max_luma, never to max_luma_255) is now also in the `luma_max` field of the MCP input schema, verbatim, so a client that never reads the tool description still sees it. The tool description is unchanged. New test `luma_max_schema_field_text_carries_the_units_sentence_verbatim` in `crates/kimo-core/tests/black_frames.rs`.
- New tool `focus_series` (group `output`, read-only): sharpness per frame, raw numbers. Every `step`-th project frame (default 1) in `start`..`end` of `source` = timeline or `clip:<id>` is rendered at full resolution; on its Rec.709 luma (0..255 code scale, interior pixels only) `lap_var` is the population variance of the 4-neighbour Laplacian (code units squared) and `tenengrad` the mean 3x3 Sobel gradient magnitude (code units), for the whole frame and up to 16 `rects` [x, y, w, h] (a rect with no interior pixel reads null, never 0). A flat frame reads exactly 0. Returns the series, the sharpest and softest sampled frame by `lap_var` (ties go to the earliest), and min / median / max of each score; at most 1200 sampled frames per call. No blurry or sharp verdict: the only line is the optional `soft_max`, which lists runs of at least `min_run` (default 2) sampled frames at or under it. Definitions and predictions (an 8 px checker under a keyed blur ramp, flat and split-frame rects, and the real 1080p clip with its freeze and black gap) were committed before the tool existed (tests/data/focus_series/PREREG.md); tests in `crates/kimo-core/tests/focus_series.rs` (the real-clip test is ignored by default: it needs the local file). Spectator READ card (4 rows: Sharpest and Softest with frame, time, lap_var and tenengrad; then Per rect when rects were given, else Soft when soft_max was given, else the lap_var spread; then Frames read), updated on every call from the call's own result, no render needed. All 15 pre-registered predictions scored in tests/data/focus_series/SCORE.tsv (15 HIT, 0 MISS).

- New tool `add_pip` (group `graphics`): one-call picture-in-picture inset of a second video -- circle or rounded-rect crop, border ring, drop shadow, and pop-in scale animation, built entirely on existing primitives (add_track/add_clip/add_mask/add_shape/set_keyframe/set_audio) through dispatch_nested, no new compositing code. Not on the director-mode tool list.
- New tool `add_keyword_pops` (group `text`): one emphasised word or phrase pops onto screen as spoken, either at explicit `words=[{text, at_s, duration_s}]` or `auto:true` picking emphasis words from the loaded transcript (the same scorer `auto_captions` uses), capped at `max_per_minute` (default 4) and spaced `min_gap_s` (default 3) apart. Styles: `bold_caps` (default), `boxed`, `typewriter`; `position` defaults to `upper_third`. Built on add_title/add_text/add_text_animator, no new text rendering. Does not touch captions or caption-ripple behaviour.
- `add_keyword_pops` look polish: text now sizes to ~60% of frame width (measured from the actual word, not a fixed fraction of frame height), stroke width and drop shadow increased so pops read as creator-style keyword pops rather than subtitles, `boxed` style's box now hugs the measured text instead of a fixed-width bar, and `accent_color` can now tint `bold_caps`/`typewriter` text directly (default off; byte-identical output when omitted).
- New tool `add_arrow` (group `graphics`): an animated "look here" arrow/callout pointing at a normalized `{x,y}` target on screen, with an optional auto-computed `from` point, draw/pop/fade animation styles, and an optional label at the tail styled like `add_keyword_pops`'s `bold_caps`. Built on add_shape (line + triangle) and add_text through dispatch_nested, no new rasterizer. Not on the director-mode tool list.
- `add_arrow` fix: the label's placement (continue past the tail, then clamp into frame) could land the label directly on top of the arrowhead when the auto `from` default placed it near a frame edge. The label now falls back to a perpendicular offset from the tail, picking whichever side needs the least clamping, only when the straight placement would actually overlap the arrow's own shaft/head bounding footprint; the common case is unchanged (all original tests still pass unmodified).
- Docs: release docs updated.
