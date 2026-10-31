"""auto_edit_benchmark: raw clips folder -> one finished edit, via Kimo's own auto_edit tool (MCP).

Built to make the quality benchmark reproducible instead of a one-off shell session: point it at a folder of
raw clips and it imports them, calls auto_edit with sensible defaults, exports an mp4, and prints the numbers
(duration, mix_lufs, lint_clean, warnings) auto_edit itself already hands back -- the same honest read a human
watching the render would want before trusting it.

Clip discovery in <clips_dir> (by filename, case-insensitive, sorted within each group so multi-take talking
heads land in story order):
  talkinghead* / talking_head* / head*   -> talking_head_asset_ids (required: at least one)
  broll* / b_roll* / cutaway*            -> broll_asset_ids (optional)
  music* / bed*                          -> music_asset_id (optional, first match only)
Anything else in the folder is ignored (so a project file or a readme sitting alongside the clips is fine).

Usage:
  python3 examples/auto_edit_benchmark.py <clips_dir> <out.mp4> [--hook "Cold-open line"] [--no-music]
                                           [--caption-style pop|karaoke|clean|boxed|impact] [--platform none|tiktok|reels|shorts]
                                           [--exe path/to/kimo-mcp] [--project /tmp/scratch.kimo]

Exit: prints one JSON line (the fields that matter for a before/after comparison) then auto_edit's own full
result on stderr for anyone who wants the rest. Non-zero exit on any tool error (KimoError), its text on stderr.
"""
import argparse, json, os, re, subprocess, sys, tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mcp_client import Kimo, KimoError

GROUPS = {
    "talking_head": re.compile(r"^(talking[_-]?head|head)", re.I),
    "broll": re.compile(r"^(b[_-]?roll|cutaway)", re.I),
    "music": re.compile(r"^(music|bed)", re.I),
}


def discover(clips_dir):
    groups = {"talking_head": [], "broll": [], "music": []}
    for name in sorted(os.listdir(clips_dir)):
        path = os.path.join(clips_dir, name)
        if not os.path.isfile(path):
            continue
        for g, pat in GROUPS.items():
            if pat.match(name):
                groups[g].append(path)
                break
    return groups


def default_exe():
    for cand in (
        os.environ.get("KIMO_MCP_EXE"),
        "target/release/kimo-mcp",
        "target/gate/kimo-mcp",
        os.path.join(os.environ.get("CARGO_TARGET_DIR", ""), "gate", "kimo-mcp") if os.environ.get("CARGO_TARGET_DIR") else None,
        os.path.join(os.environ.get("CARGO_TARGET_DIR", ""), "release", "kimo-mcp") if os.environ.get("CARGO_TARGET_DIR") else None,
    ):
        if cand and os.path.isfile(cand):
            return cand
    return "target/release/kimo-mcp"  # let Kimo() raise a clear FileNotFoundError if truly absent


WHISPER_BIN = os.path.expanduser("~/.cache/kimo_models/bin/whisper-cli")
WHISPER_MODEL = os.path.expanduser("~/.cache/kimo_models/ggml-small-q5_1.bin")


def _transcribe(media_path):
    """16kHz mono WAV + whisper-cli (both already on this box for the real-footage quality gate) -> plain text."""
    with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tmp:
        wav_path = tmp.name
    try:
        subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", media_path, "-ar", "16000", "-ac", "1", "-f", "wav", wav_path],
                        check=True, timeout=30)
        out = subprocess.run([WHISPER_BIN, "-m", WHISPER_MODEL, "-f", wav_path, "-l", "en", "-np", "-nt", "-t", "4"],
                              check=True, timeout=120, capture_output=True, text=True)
        return out.stdout
    finally:
        os.unlink(wav_path)


def _norm_words(text):
    return [w for w in re.sub(r"[^a-z0-9'\s]", " ", text.lower()).split() if w]


def word_recall(source_media, rendered_media):
    """Whisper-transcribes both and returns (matched, total, ratio): a bag-of-words recall of the source
    dialogue's own words found in the finished render -- the regression gate for finish_audio's dialogue-vs-
    ambience classifier (V7.8Q: real dialogue was getting silently muted as 'ambience' while every loudness
    and silence check still passed, because the final mix only ever levels whatever survived the mute)."""
    import collections
    src_words = collections.Counter(_norm_words(_transcribe(source_media)))
    out_words = collections.Counter(_norm_words(_transcribe(rendered_media)))
    matched = sum(min(src_words[w], out_words[w]) for w in src_words)
    total = sum(src_words.values()) or 1
    return matched, total, matched / total


def _concat_kept_spans(source_path, spans, out_path):
    """Trims and concatenates the given (start,end) source-second spans out of source_path's own audio into
    one file at out_path -- the exact audio auto_rough_cut decided to KEEP, before any further trimming."""
    filters = [f"[0:a]atrim=start={s}:end={e},asetpts=PTS-STARTPTS[a{i}]" for i, (s, e) in enumerate(spans)]
    concat_in = "".join(f"[a{i}]" for i in range(len(spans)))
    filters.append(f"{concat_in}concat=n={len(spans)}:v=0:a=1[out]")
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", source_path, "-filter_complex", ";".join(filters), "-map", "[out]", out_path],
                   check=True, timeout=60)


def ceiling_word_recall(kimo, th_ids, th_paths, rendered_media):
    """The HONEST recall gate: auto_rough_cut's own dead-air/filler trimming removes real source content ON
    PURPOSE (that's its whole job), so gating a finished render against the FULL raw source's word count
    punishes correct editing, not a bug. This re-runs auto_rough_cut (read-derived; called again here purely
    to recover the exact source spans it kept, after the real edit already exported) on the same talking-
    head asset(s), builds a reference clip of just that kept audio, transcribes it as the ceiling (the most
    any finished edit built from these same cuts could possibly recall), and returns the render's recall
    against THAT ceiling instead of the full source. Single or multiple talking-head assets both work;
    multiple assets' kept spans are each concatenated on their own asset, then joined.
    Returns (matched, ceiling_total, ratio, ceiling_text)."""
    import collections
    part_paths = []
    try:
        for aid, path in zip(th_ids, th_paths):
            rc = kimo.call("auto_rough_cut", asset_ids=[aid])
            spans = [tuple(seg["source"]) for seg in rc.get("segments", []) if not seg.get("dropped")]
            if not spans:
                continue
            part = tempfile.NamedTemporaryFile(suffix=".wav", delete=False).name
            _concat_kept_spans(path, spans, part)
            part_paths.append(part)
        if not part_paths:
            return 0, 1, 0.0, ""
        if len(part_paths) == 1:
            ceiling_path = part_paths[0]
        else:
            list_file = tempfile.NamedTemporaryFile(suffix=".txt", delete=False, mode="w")
            for p in part_paths:
                list_file.write(f"file '{p}'\n")
            list_file.close()
            ceiling_path = tempfile.NamedTemporaryFile(suffix=".wav", delete=False).name
            subprocess.run(["ffmpeg", "-y", "-v", "error", "-f", "concat", "-safe", "0", "-i", list_file.name, ceiling_path], check=True, timeout=60)
            os.unlink(list_file.name)
        ceiling_text = _transcribe(ceiling_path)
        render_text = _transcribe(rendered_media)
        ceiling_words = collections.Counter(_norm_words(ceiling_text))
        render_words = collections.Counter(_norm_words(render_text))
        matched = sum(min(ceiling_words[w], render_words[w]) for w in ceiling_words)
        total = sum(ceiling_words.values()) or 1
        return matched, total, matched / total, ceiling_text
    finally:
        for p in part_paths:
            try: os.unlink(p)
            except OSError: pass


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("clips_dir")
    ap.add_argument("out")
    ap.add_argument("--hook", default=None, help="cold-open hook line; omitted = auto_edit skips the hook honestly")
    ap.add_argument("--no-music", action="store_true", help="skip even an auto-discovered music* file")
    ap.add_argument("--caption-style", default="boxed", help="auto_captions style (default boxed, YouTube-norm pill)")
    ap.add_argument("--platform", default="none")
    ap.add_argument("--target-lufs", type=float, default=-14.0)
    ap.add_argument("--level-strength", type=float, default=None, help="passed to finish_audio: speechnorm expansion/compression factor, 1..4")
    ap.add_argument("--no-level-dynamics", action="store_true", help="passed to finish_audio: disable gentle speech leveling")
    ap.add_argument("--match-strength", type=float, default=None, help="passed to finish_audio: how fully per-clip loudness is matched to the loudest clip, 0..1")
    ap.add_argument("--compress-strength", type=float, default=None, help="passed to finish_audio: dialogue compressor ratio scale, 0..1 (0=off, 1=old fixed 3:1)")
    ap.add_argument("--no-whoosh-sfx", action="store_true", help="passed to auto_edit: disable the whoosh SFX on b-roll cut-ins / hook title")
    ap.add_argument("--exe", default=None)
    ap.add_argument("--project", default=None, help="scratch .kimo project path (default: alongside --out)")
    ap.add_argument("--whisper-recall-source", default=None, help="path to the raw talking-head source; when given, transcribes it and the final render with whisper-cli and reports/gates word_recall (fails if under --whisper-recall-min)")
    ap.add_argument("--whisper-recall-min", type=float, default=0.90, help="minimum acceptable word_recall when --whisper-recall-source is given (default 0.90)")
    args = ap.parse_args()

    groups = discover(args.clips_dir)
    if not groups["talking_head"]:
        sys.exit(f"no talking-head clip found in {args.clips_dir} (name it talkinghead*.mp4 or head*.mp4)")

    project = args.project or (os.path.splitext(args.out)[0] + ".kimo")
    exe = args.exe or default_exe()
    k = Kimo(exe=exe)
    try:
        k.call("project_set", width=1280, height=720, fps=30)

        def import_all(paths):
            return [k.call("import_media", path=os.path.abspath(p))["asset_id"] for p in paths]

        th_ids = import_all(groups["talking_head"])
        broll_ids = import_all(groups["broll"])
        music_id = None
        if groups["music"] and not args.no_music:
            music_id = import_all(groups["music"][:1])[0]

        auto_edit_args = dict(
            talking_head_asset_ids=th_ids,
            broll_asset_ids=broll_ids,
            caption_style=args.caption_style,
            platform=args.platform,
            target_lufs=args.target_lufs,
        )
        if music_id:
            auto_edit_args["music_asset_id"] = music_id
        if args.hook:
            auto_edit_args["hook_text"] = args.hook
        if args.level_strength is not None:
            auto_edit_args["level_strength"] = args.level_strength
        if args.no_level_dynamics:
            auto_edit_args["level_dynamics"] = False
        if args.match_strength is not None:
            auto_edit_args["match_strength"] = args.match_strength
        if args.compress_strength is not None:
            auto_edit_args["compress_strength"] = args.compress_strength
        if args.no_whoosh_sfx:
            auto_edit_args["whoosh_sfx"] = False

        result = k.call("auto_edit", **auto_edit_args)
        k.call("save_project", path=os.path.abspath(project))
        k.call("export", path=os.path.abspath(args.out))

        summary = {
            "out": args.out,
            "duration": result.get("duration"),
            "mix_lufs": result.get("mix_lufs"),
            "lint_clean": result.get("lint_clean"),
            "warnings": result.get("warnings", []),
            "remaining_problems": [p.get("code") for p in result.get("remaining_problems", [])],
            "talking_head_clips": len(th_ids),
            "broll_clips": len(broll_ids),
            "music": bool(music_id),
        }
        recall_failed = False
        if args.whisper_recall_source:
            matched, total, ratio = word_recall(args.whisper_recall_source, args.out)
            summary["word_recall_vs_full_source"] = {"matched": matched, "total": total, "ratio": round(ratio, 4)}
            # The HONEST gate: against the ceiling (what auto_rough_cut itself kept), not the full raw
            # source -- auto_rough_cut's own dead-air/filler trimming removes real words ON PURPOSE, so an
            # absolute floor against the untrimmed source punishes correct editing along with real bugs.
            c_matched, c_total, c_ratio, ceiling_text = ceiling_word_recall(k, th_ids, groups["talking_head"], args.out)
            summary["word_recall_vs_ceiling"] = {"matched": c_matched, "total": c_total, "ratio": round(c_ratio, 4), "min": args.whisper_recall_min, "ceiling_transcript": ceiling_text.strip()}
            if c_ratio < args.whisper_recall_min:
                recall_failed = True
                summary["word_recall_vs_ceiling"]["FAILED"] = f"{c_ratio:.1%} of the kept-span ceiling is under the {args.whisper_recall_min:.0%} floor -- the render is dropping real dialogue that auto_rough_cut itself decided to keep (check finish_audio's dialogue/ambience split, not just loudness)"
        print(json.dumps(summary, indent=2))
        print(json.dumps(result, indent=2), file=sys.stderr)
        if recall_failed:
            sys.exit(1)
    except KimoError as e:
        sys.exit(f"{e.tool} failed: {e.text}" + (f" (hint: {e.hint})" if e.hint else ""))
    finally:
        k.close()


if __name__ == "__main__":
    main()
