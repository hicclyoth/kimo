//! V8.11.0 TDD: HDR (HLG/PQ) phone footage corrected into the SDR bt709 pipeline (quality audit #7).
//!
//! Fixtures: an SDR reference clip (testsrc2, tagged bt709) and the SAME visual content re-encoded as HLG
//! (arib-std-b67) and PQ (smpte2084), bt2020 primaries, 10-bit -- built with ffmpeg's own zscale filter
//! (forward: bt709 -> scene-linear at npl=100 -> bt2020 primaries -> HLG/PQ transfer), so any decoded RGB
//! difference against the SDR reference is caused by Kimo's own decode path, not by a different scene.
use kimo_core::{commands::{dispatch, Session}, render};
use serde_json::json;
use std::process::{Command, Stdio};

fn fixture_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("kimo_q8110_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn ffmpeg(args: &[&str]) {
    let st = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).stdin(Stdio::null()).status().expect("ffmpeg");
    assert!(st.success(), "ffmpeg {args:?} failed");
}

const W: u32 = 640;
const H: u32 = 360;
const DUR: f64 = 1.0;

/// The SDR reference: testsrc2, correctly tagged bt709 throughout (decode, scale and output tags all
/// agree), built via the same explicit zscale round-trip the HDR fixtures below use, so all three fixtures
/// share one decode path and only the transfer/primaries tag (and the resulting byte values) differ.
fn sdr_fixture() -> String {
    let p = fixture_dir().join("sdr.mp4");
    if !p.exists() {
        let vf = "zscale=t=bt709:m=bt709:p=bt709:tin=bt709:min=bt709:pin=bt709,format=yuv420p";
        ffmpeg(&["-f", "lavfi", "-i", &format!("testsrc2=size={W}x{H}:rate=25:duration={DUR}"),
            "-vf", vf, "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709", "-color_range", "tv",
            p.to_str().unwrap()]);
    }
    p.to_string_lossy().into_owned()
}

/// Builds an HDR-tagged fixture by converting the SDR reference up to bt2020 scene-linear (npl=100) and
/// back down through the given transfer curve ("arib-std-b67" for HLG, "smpte2084" for PQ) -- the inverse
/// of the correction this version adds, so a correct decode should (ignoring the lossy 10-bit h264
/// round-trip) land close to the SDR reference again.
fn hdr_fixture(name: &str, transfer: &str) -> String {
    let p = fixture_dir().join(name);
    if !p.exists() {
        let sdr = sdr_fixture();
        let vf = format!(
            "zscale=t=linear:m=bt709:p=bt709:tin=bt709:min=bt709:pin=bt709,format=gbrpf32le,\
             zscale=p=bt2020:t=linear:m=bt709:tin=linear:min=bt709:pin=bt709,\
             zscale=t={transfer}:m=bt2020nc:p=bt2020:r=tv:tin=linear:min=bt709:pin=bt2020:npl=100,\
             format=yuv420p10le"
        );
        ffmpeg(&["-i", &sdr, "-vf", &vf, "-c:v", "libx264", "-profile:v", "high10", "-pix_fmt", "yuv420p10le",
            "-color_primaries", "bt2020", "-color_trc", transfer, "-colorspace", "bt2020nc", "-color_range", "tv",
            p.to_str().unwrap()]);
    }
    p.to_string_lossy().into_owned()
}

/// A skin-tone-ish solid swatch (no resampling/compression artifacts to confound a hue check), SDR and HLG
/// versions of the exact same colour -- item 5's "skin and colour hue don't shift wildly" check.
fn skin_fixture(name: &str, transfer: Option<&str>) -> String {
    let p = fixture_dir().join(name);
    if !p.exists() {
        // A representative mid-tone skin colour, roughly sRGB (221, 178, 153).
        let base = format!("color=c=0xddb299:s={W}x{H}:rate=25:duration={DUR}");
        match transfer {
            None => ffmpeg(&["-f", "lavfi", "-i", &base,
                "-vf", "zscale=t=bt709:m=bt709:p=bt709:tin=bt709:min=bt709:pin=bt709,format=yuv420p",
                "-c:v", "libx264", "-pix_fmt", "yuv420p",
                "-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709", "-color_range", "tv",
                p.to_str().unwrap()]),
            Some(t) => {
                let vf = format!(
                    "zscale=t=linear:m=bt709:p=bt709:tin=bt709:min=bt709:pin=bt709,format=gbrpf32le,\
                     zscale=p=bt2020:t=linear:m=bt709:tin=linear:min=bt709:pin=bt709,\
                     zscale=t={t}:m=bt2020nc:p=bt2020:r=tv:tin=linear:min=bt709:pin=bt2020:npl=100,\
                     format=yuv420p10le"
                );
                ffmpeg(&["-f", "lavfi", "-i", &base, "-vf", &vf, "-c:v", "libx264", "-profile:v", "high10",
                    "-pix_fmt", "yuv420p10le", "-color_primaries", "bt2020", "-color_trc", t, "-colorspace", "bt2020nc",
                    "-color_range", "tv", p.to_str().unwrap()]);
            }
        }
    }
    p.to_string_lossy().into_owned()
}

/// ffprobe sanity: the HDR fixtures really do carry the tags this test suite assumes.
#[test]
fn fixtures_carry_the_expected_tags() {
    let probe = |p: &str, field: &str| -> String {
        let out = Command::new("ffprobe").args(["-v", "error", "-select_streams", "v:0", "-show_entries", &format!("stream={field}"), "-of", "default=nw=1:nk=1", p]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let hlg = hdr_fixture("q8110_hlg.mp4", "arib-std-b67");
    let pq = hdr_fixture("q8110_pq.mp4", "smpte2084");
    assert_eq!(probe(&hlg, "color_transfer"), "arib-std-b67");
    assert_eq!(probe(&hlg, "color_primaries"), "bt2020");
    assert_eq!(probe(&pq, "color_transfer"), "smpte2084");
    assert_eq!(probe(&pq, "color_primaries"), "bt2020");
    assert_eq!(probe(&sdr_fixture(), "color_transfer"), "bt709");
}

fn decode_frame(path: &str, hdr_tonemap: bool) -> Vec<u8> {
    let mut s = Session::new();
    dispatch(&mut s, "project_set", &json!({"width": W, "height": H, "fps": 25, "hdr_tonemap": hdr_tonemap})).unwrap();
    let t = dispatch(&mut s, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
    let a = dispatch(&mut s, "import_media", &json!({"path": path})).unwrap()["asset_id"].as_str().unwrap().to_string();
    dispatch(&mut s, "add_clip", &json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0.0, "duration": DUR / 2.0, "in_point": 0.0, "fit": "stretch"})).unwrap();
    let mut cache = render::Cache::new();
    let lt = DUR / 4.0;
    render::prepare_at(&s.project, &mut cache, Some(lt)).unwrap();
    render::export_frame_rgba8(&s.project, lt, &cache).unwrap()
}

fn mean_std_luma(rgba: &[u8]) -> (f64, f64) {
    let luma: Vec<f64> = rgba.chunks_exact(4).map(|p| 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64).collect();
    let mean = luma.iter().sum::<f64>() / luma.len() as f64;
    let var = luma.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / luma.len() as f64;
    (mean, var.sqrt())
}

/// quality audit #7's own TDD list, item 1: an SDR clip renders unchanged whether hdr_tonemap is on or
/// off -- the gate only ever touches a clip hdr_source() actually identifies as HDR-tagged.
#[test]
fn sdr_clip_is_untouched_by_hdr_tonemap() {
    let sdr = sdr_fixture();
    let off = decode_frame(&sdr, false);
    let on = decode_frame(&sdr, true);
    assert_eq!(off, on, "a correctly-tagged bt709 source must decode identically whether hdr_tonemap is on or off");
}

/// Item 1's other half: hdr_tonemap defaults to false and is omitted from saved projects (the same
/// skip_serializing_if convention as every other quality-audit gate), so golden_frames.txt/golden_project.txt
/// and every existing project stay byte-identical with no code-path possible to reach the new branch.
#[test]
fn hdr_tonemap_defaults_to_false_and_is_omitted_from_saved_projects() {
    let mut s = Session::new();
    dispatch(&mut s, "project_set", &json!({"width": 320, "height": 180, "fps": 30})).unwrap();
    assert!(!s.project.hdr_tonemap);
    let saved = serde_json::to_string(&s.project).unwrap();
    assert!(!saved.contains("hdr_tonemap"), "false hdr_tonemap must be omitted from the saved project (skip_serializing_if)");
}

/// Item 2: an HDR clip is detected -- turning hdr_tonemap on for an HLG-tagged clip measurably changes its
/// decoded pixels (there is something for the gate to do), for BOTH HLG and PQ.
#[test]
fn hdr_clip_is_detected_both_hlg_and_pq() {
    for (name, transfer) in [("q8110_det_hlg.mp4", "arib-std-b67"), ("q8110_det_pq.mp4", "smpte2084")] {
        let hdr = hdr_fixture(name, transfer);
        let off = decode_frame(&hdr, false);
        let on = decode_frame(&hdr, true);
        assert_ne!(off, on, "{transfer}: hdr_tonemap on must change the decoded pixels of an HDR-tagged clip");
    }
}

/// Item in the WORK list: "HDR output mean luma and contrast land near the SDR reference, not grey and
/// washed out (set a numeric threshold)". Measures all three: the SDR reference, the UNCORRECTED decode
/// (hdr_tonemap off -- today's/every-pre-V8.11.0 behaviour, HDR bytes read as if ordinary bt709), and the
/// CORRECTED decode (hdr_tonemap on). The uncorrected decode must show the audit's predicted defect (markedly
/// lower contrast than the reference); the corrected decode must land close to the reference on both mean
/// luma and contrast (std), substantially better than the uncorrected one.
#[test]
fn hdr_tonemap_on_lands_near_sdr_reference_not_grey_and_washed_out() {
    let sdr = sdr_fixture();
    let hlg = hdr_fixture("q8110_quality_hlg.mp4", "arib-std-b67");
    let (ref_mean, ref_std) = mean_std_luma(&decode_frame(&sdr, false));
    let (before_mean, before_std) = mean_std_luma(&decode_frame(&hlg, false));
    let (after_mean, after_std) = mean_std_luma(&decode_frame(&hlg, true));
    eprintln!("hdr_tonemap: sdr_ref mean={ref_mean:.1} std={ref_std:.1}; hlg_uncorrected mean={before_mean:.1} std={before_std:.1}; hlg_corrected mean={after_mean:.1} std={after_std:.1}");

    // The audit's predicted defect: decoding HLG bytes as if ordinary bt709 washes out contrast.
    assert!(before_std < ref_std * 0.75, "sanity: the uncorrected decode should show the audit's predicted washed-out low-contrast defect: ref_std={ref_std:.1} before_std={before_std:.1}");

    // The fix: within 30 luma units of mean and at least 70% of the reference's contrast -- the numeric
    // threshold this test's own brief asked for.
    assert!((after_mean - ref_mean).abs() < 30.0, "corrected mean luma should land near the SDR reference: ref={ref_mean:.1} after={after_mean:.1}");
    assert!(after_std > ref_std * 0.70, "corrected contrast should land near the SDR reference, not washed out: ref_std={ref_std:.1} after_std={after_std:.1}");
    assert!(after_std > before_std * 1.3, "corrected decode should have measurably more contrast than the uncorrected one: before={before_std:.1} after={after_std:.1}");
}

/// Item in the WORK list: "skin and colour hue don't shift wildly" -- a representative mid-tone skin colour
/// (a flat swatch, no resampling to confound the measurement) must keep roughly the same RGB ratios (hue)
/// after the HDR correction, compared to the same colour's SDR encode.
#[test]
fn skin_hue_does_not_shift_wildly_after_tonemap() {
    let sdr = skin_fixture("q8110_skin_sdr.mp4", None);
    let hlg = skin_fixture("q8110_skin_hlg.mp4", Some("arib-std-b67"));
    let ref_rgba = decode_frame(&sdr, false);
    let after_rgba = decode_frame(&hlg, true);
    // Average RGB over the whole (flat) frame.
    let avg = |rgba: &[u8]| -> (f64, f64, f64) {
        let n = rgba.len() / 4;
        let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);
        for p in rgba.chunks_exact(4) { r += p[0] as f64; g += p[1] as f64; b += p[2] as f64; }
        (r / n as f64, g / n as f64, b / n as f64)
    };
    let (rr, rg, rb) = avg(&ref_rgba);
    let (ar, ag, ab) = avg(&after_rgba);
    // Hue as normalized chroma ratios (R/G and B/G), robust to an overall brightness/contrast shift that
    // a tonemap curve is expected to apply -- what must NOT shift wildly is the ratio BETWEEN channels.
    let (ref_rg_ratio, ref_bg_ratio) = (rr / rg, rb / rg);
    let (after_rg_ratio, after_bg_ratio) = (ar / ag, ab / ag);
    eprintln!("skin hue: ref R/G={ref_rg_ratio:.3} B/G={ref_bg_ratio:.3}; after R/G={after_rg_ratio:.3} B/G={after_bg_ratio:.3}");
    assert!((after_rg_ratio - ref_rg_ratio).abs() < 0.08, "skin R/G ratio shifted too far: ref={ref_rg_ratio:.3} after={after_rg_ratio:.3}");
    assert!((after_bg_ratio - ref_bg_ratio).abs() < 0.08, "skin B/G ratio shifted too far: ref={ref_bg_ratio:.3} after={after_bg_ratio:.3}");
}

/// hdr_source()'s fallback branch: bt2020 primaries with no recognizable SDR transfer tag (simulated the
/// same way quality_v881 simulates a mistagged source -- remux the HLG fixture's own bytes with the
/// transfer tag stripped via h264_metadata, a copy, no re-encode) is still treated as HDR (assumed HLG) and
/// corrected, not left to decode as if it were ordinary bt709.
#[test]
fn bt2020_primaries_with_no_transfer_tag_is_still_treated_as_hdr() {
    let hlg = hdr_fixture("q8110_fallback_src.mp4", "arib-std-b67");
    let untagged = fixture_dir().join("q8110_fallback_untagged.mp4");
    if !untagged.exists() {
        ffmpeg(&["-i", &hlg, "-c:v", "copy", "-bsf:v", "h264_metadata=transfer_characteristics=2", untagged.to_str().unwrap()]);
    }
    let probe = |p: &str, field: &str| -> String {
        let out = Command::new("ffprobe").args(["-v", "error", "-select_streams", "v:0", "-show_entries", &format!("stream={field}"), "-of", "default=nw=1:nk=1", p]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let untagged = untagged.to_str().unwrap();
    assert_eq!(probe(untagged, "color_transfer"), "unknown", "sanity: the fallback fixture's transfer tag must actually be stripped");
    assert_eq!(probe(untagged, "color_primaries"), "bt2020", "sanity: bt2020 primaries must still be tagged");

    let off = decode_frame(untagged, false);
    let on = decode_frame(untagged, true);
    assert_ne!(off, on, "a bt2020-primaries clip with no transfer tag should still be treated as HDR and corrected");
}

// ---------------------------------------------------------------------------------------------
// V8.11.1 regression test (fixes the V8.11.0 brightness-shift bug): auto_edit turns BOTH
// color_tag_sanity and hdr_tonemap on by default, so the real-world gate combination to test is
// both flags on together, not hdr_tonemap in isolation against a Session that leaves
// color_tag_sanity at its own raw-Session default of false (which is what every test above this
// point does, and is exactly why none of them caught the bug: with color_tag_sanity off there was
// never a mistag-correction for hdr_tonemap's old buggy priority to clobber). The V8.11.0 bug:
// decode()'s color_override picked the HDR branch whenever the PROJECT flag `p.hdr_tonemap` was
// on, for every clip, regardless of whether that specific clip was actually HDR -- so a non-HDR
// clip's hdr_source() correctly returning None fell to an empty override instead of ever reaching
// the color_tag_sanity mistag check below it, silently losing the bt709 retag a mistagged/
// untagged HD source (e.g. bt470bg with no primaries/transfer, the real talkinghead_real_final.mp4
// shape) depends on.
fn mistagged_720p_fixture() -> String {
    let dir = fixture_dir();
    let truth = dir.join("q8110_regress_truth.mp4");
    let mistagged = dir.join("q8110_regress_mistagged.mp4");
    if !truth.exists() {
        ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=1280x720:d=0.2:r=25",
            "-vf", "scale=out_color_matrix=bt709:out_range=tv", "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-color_range", "tv", "-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709",
            truth.to_str().unwrap()]);
    }
    if !mistagged.exists() {
        ffmpeg(&["-i", truth.to_str().unwrap(), "-c:v", "copy",
            "-bsf:v", "h264_metadata=colour_primaries=2:transfer_characteristics=2:matrix_coefficients=5",
            mistagged.to_str().unwrap()]);
    }
    mistagged.to_string_lossy().into_owned()
}

fn correctly_tagged_720p_fixture() -> String {
    let dir = fixture_dir();
    let p = dir.join("q8110_regress_truth.mp4"); // built as a side effect of mistagged_720p_fixture, but
    if !p.exists() {                              // build it directly too so this fn works standalone.
        ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=1280x720:d=0.2:r=25",
            "-vf", "scale=out_color_matrix=bt709:out_range=tv", "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-color_range", "tv", "-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709",
            p.to_str().unwrap()]);
    }
    p.to_string_lossy().into_owned()
}

/// decode_frame above hardcodes color_tag_sanity off (the Session default); this variant sets both flags,
/// matching how auto_edit actually configures a project (both default true).
fn decode_frame_auto_edit_style(path: &str, w: u32, h: u32, hdr_tonemap: bool) -> Vec<u8> {
    let mut s = Session::new();
    dispatch(&mut s, "project_set", &json!({"width": w, "height": h, "fps": 25, "color_tag_sanity": true, "hdr_tonemap": hdr_tonemap})).unwrap();
    let t = dispatch(&mut s, "add_track", &json!({})).unwrap()["track_id"].as_str().unwrap().to_string();
    let a = dispatch(&mut s, "import_media", &json!({"path": path})).unwrap()["asset_id"].as_str().unwrap().to_string();
    dispatch(&mut s, "add_clip", &json!({"track_id": t, "source_type": "media", "asset_id": a, "start": 0.0, "duration": 0.16, "in_point": 0.0, "fit": "stretch"})).unwrap();
    let mut cache = render::Cache::new();
    render::prepare_at(&s.project, &mut cache, Some(0.08)).unwrap();
    render::export_frame_rgba8(&s.project, 0.08, &cache).unwrap()
}

/// The actual regression: a correctly-tagged plain bt709 clip, decoded auto_edit-style (color_tag_sanity
/// AND hdr_tonemap both on, as auto_edit sets by default), must be pixel-identical whether hdr_tonemap is
/// on or off -- nothing HDR here, so the flag must be a no-op for this clip either way.
#[test]
fn auto_edit_style_bt709_clip_is_pixel_identical_regardless_of_hdr_tonemap() {
    let clip = correctly_tagged_720p_fixture();
    let off = decode_frame_auto_edit_style(&clip, 1280, 720, false);
    let on = decode_frame_auto_edit_style(&clip, 1280, 720, true);
    assert_eq!(off, on, "a plain bt709 clip must decode identically under auto_edit's settings whether hdr_tonemap is on or off");
}

/// The actual regression, the shape that broke: an untagged/mistagged (bt470bg, no primaries/transfer)
/// HD clip -- not HDR, but exactly the real-world shape color_tag_sanity exists to correct -- decoded
/// auto_edit-style, must be pixel-identical whether hdr_tonemap is on or off. Before the V8.11.1 fix this
/// failed: hdr_tonemap=true silently suppressed color_tag_sanity's bt709 retag for this clip, producing a
/// measurably different (brighter / wrong-matrix) decode than hdr_tonemap=false, even though the clip is
/// not HDR at all.
#[test]
fn auto_edit_style_mistagged_clip_is_pixel_identical_regardless_of_hdr_tonemap() {
    let clip = mistagged_720p_fixture();
    let off = decode_frame_auto_edit_style(&clip, 1280, 720, false);
    let on = decode_frame_auto_edit_style(&clip, 1280, 720, true);
    assert_eq!(off, on, "an untagged/mistagged bt470bg clip (not HDR) must decode identically under auto_edit's settings whether hdr_tonemap is on or off -- hdr_tonemap must never suppress color_tag_sanity's correction for a clip that isn't actually HDR");

    // And both must actually show the color_tag_sanity correction (i.e. not accidentally equal to the
    // untreated bt470bg-trusting decode) -- proves the fix didn't just disable color_tag_sanity instead.
    let truth = correctly_tagged_720p_fixture();
    let truth_rgba = decode_frame_auto_edit_style(&truth, 1280, 720, true);
    let mean_abs_diff = |a: &[u8], b: &[u8]| -> f64 {
        let n = a.len().min(b.len());
        let sum: f64 = (0..n).step_by(4).flat_map(|i| (0..3).map(move |j| i + j)).map(|i| (a[i] as f64 - b[i] as f64).abs()).sum();
        sum / (n / 4 * 3) as f64
    };
    let shift = mean_abs_diff(&truth_rgba, &on);
    eprintln!("regression check: hdr_tonemap-on mistagged decode vs truth mean abs diff = {shift:.2}/255 (must be small -- color_tag_sanity still corrected it)");
    assert!(shift <= 2.0, "color_tag_sanity's correction must still apply when hdr_tonemap is also on: mean abs diff vs truth = {shift:.2}/255");
}
