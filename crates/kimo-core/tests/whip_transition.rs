//! V8.5.7: the whip_left/whip_right transition must read as a crisp whip-pan -- a short,
//! strong directional blur concentrated on the middle frames -- not a long smear that leaves
//! the outgoing and incoming shots mushy. See crates/kimo-core/src/transitions.rs (whip_left
//! / whip_right in `plan()`) and the montage/benchmark review that flagged it at ~4.05s.
use kimo_core::effects::Frame;
use kimo_core::model::Transition;
use kimo_core::transitions::plan;

const W: usize = 640;
const H: usize = 360;
const FPS: f64 = 30.0;

/// A single hard vertical edge (black left half, white right half), fully opaque. Blurring
/// this directionally turns the hard edge into a ramp; the width of that ramp IS the blur's
/// width in pixels, read straight off the frame -- a direct, pattern-independent way to
/// measure how wide (and therefore how visible) the whip's smear really is at any instant.
fn edge_frame(w: usize, h: usize) -> Frame {
    let mut f = Frame::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = if x < w / 2 { 0.0 } else { 1.0 };
            let i = (y * w + x) * 4;
            f.d[i] = v;
            f.d[i + 1] = v;
            f.d[i + 2] = v;
            f.d[i + 3] = 1.0;
        }
    }
    f
}

/// Width in pixels of the blurred transition zone around the centre edge: the span between
/// where the red channel first rises above 0.1 and where it last sits below 0.9. Zero (or
/// near it) means the edge is still sharp; a large number means a wide, visible smear.
fn edge_blur_width(f: &Frame) -> usize {
    let y = f.h / 2;
    let row: Vec<f32> = (0..f.w).map(|x| f.d[(y * f.w + x) * 4]).collect();
    let lo = row.iter().position(|&v| v > 0.1).unwrap_or(0);
    let hi = row.iter().rposition(|&v| v < 0.9).unwrap_or(f.w - 1);
    hi.saturating_sub(lo)
}

fn whip(kind: &str, duration: f64, intensity: f64) -> Transition {
    Transition {
        kind: kind.into(),
        duration,
        ease: "in_out".into(),
        softness: 0.08,
        angle: 0.0,
        color: [0.0, 0.0, 0.0],
        intensity,
        count: 10.0,
        seed: 0,
        spec: None,
    }
}

/// Run only the outgoing-clip blur (`post`) a whip transition plans for one instant, the way
/// render.rs applies it to the clip being wiped away before compositing -- and measure the
/// resulting blur width.
fn blur_width_at(tr: &Transition, lt: f64) -> usize {
    let pl = plan(tr, lt, W, H);
    let mut f = edge_frame(W, H);
    if let Some(post) = &pl.post {
        post(&mut f);
    }
    edge_blur_width(&f)
}

#[test]
fn whip_start_and_end_frames_stay_sharp() {
    let tr = whip("whip_left", 0.25, 1.0);
    for &lt in &[0.0, 0.25] {
        let width = blur_width_at(&tr, lt);
        assert!(
            width <= 2,
            "the whip's own start/end (lt={lt}) must read sharp, got a {width}px blur"
        );
    }
}

#[test]
fn whip_smear_spans_at_most_a_handful_of_frames() {
    let tr = whip("whip_left", 0.25, 1.0);
    let n_frames = (tr.duration * FPS).round() as usize;
    const VISIBLE_SMEAR_PX: usize = 20;
    let mut smeared = 0usize;
    let mut widths = vec![];
    for i in 0..=n_frames {
        let lt = (i as f64 / FPS).min(tr.duration);
        let width = blur_width_at(&tr, lt);
        widths.push(width);
        if width > VISIBLE_SMEAR_PX {
            smeared += 1;
        }
    }
    assert!(
        smeared <= 5,
        "a pro whip pan holds its heavy blur for only a few frames, not the whole transition: {smeared} of {n_frames} frames exceeded a {VISIBLE_SMEAR_PX}px blur width ({widths:?})"
    );
}

#[test]
fn whip_actually_blurs_hard_at_its_own_peak() {
    // The fix must not just chase the "smeared" test down to nothing -- a whip pan is
    // supposed to read as a strong, deliberate blur at its peak, not a timid wobble.
    let tr = whip("whip_left", 0.25, 1.0);
    let peak_width = blur_width_at(&tr, 0.125);
    assert!(
        peak_width > 60,
        "the whip's peak frame should carry a strong, clearly-visible blur, got only {peak_width}px"
    );
}

#[test]
fn whip_right_mirrors_whip_left() {
    for kind in ["whip_left", "whip_right"] {
        let tr = whip(kind, 0.25, 1.0);
        assert!(blur_width_at(&tr, 0.0) <= 2, "{kind} must start sharp");
        assert!(blur_width_at(&tr, 0.25) <= 2, "{kind} must end sharp");
        assert!(blur_width_at(&tr, 0.125) > 60, "{kind} must actually blur hard at its own peak");
    }
}

#[test]
fn whip_does_not_reblur_the_composited_incoming_shot() {
    // `overlay` (transitions.rs Plan) runs on the whole already-composited frame in render.rs
    // (both the outgoing and the incoming layer), not just the clip being wiped away. A whip
    // that sets one would smear the incoming shot too, which is exactly the "mush" the review
    // flagged -- the incoming shot must stay untouched by the whip's blur.
    for kind in ["whip_left", "whip_right"] {
        let tr = whip(kind, 0.25, 1.0);
        let pl = plan(&tr, 0.125, W, H);
        assert!(pl.overlay.is_none(), "{kind} must not blur the already-composited frame (it would smear the incoming shot too)");
    }
}

/// Apply the `under` hook the way render.rs applies it for an outgoing layer with no
/// abutting same-track clip (`prev = None`, the auto_edit b-roll-over-talking-head case:
/// the backdrop is whatever track is already under the incoming clip, not a clip that ends
/// where this one starts) -- and measure the resulting blur width on an edge frame.
fn outgoing_blur_width_at(tr: &Transition, lt: f64) -> usize {
    let pl = plan(tr, lt, W, H);
    let mut f = edge_frame(W, H);
    if let Some(under) = &pl.under {
        under(&mut f, None);
    }
    edge_blur_width(&f)
}

/// Largest jump between two horizontally adjacent samples in the blurred row -- a smooth
/// streak changes gradually; distinct echoes (too few taps for a wide blur) show up as a
/// big single step instead of a gradual ramp.
fn max_adjacent_step(f: &Frame) -> f32 {
    let y = f.h / 2;
    let row: Vec<f32> = (0..f.w).map(|x| f.d[(y * f.w + x) * 4]).collect();
    row.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max)
}

#[test]
fn whip_outgoing_layer_blurs_hard_at_the_midpoint() {
    // V8.5.7 follow-up: the old whip only ever touched the incoming clip's own frame
    // (`post`); the backdrop under it (`out`, rendered via the `under` hook render.rs
    // calls before blending this clip on top) was never touched, so the outgoing layer
    // read perfectly sharp and static the whole time -- a cheap slide/wipe, not a
    // whip-pan. A real whip blurs BOTH layers with the SAME envelope, peaking together
    // at the midpoint.
    let tr = whip("whip_left", 0.25, 1.0);
    let width = outgoing_blur_width_at(&tr, 0.125);
    assert!(
        width > 40,
        "the outgoing layer should carry a strong blur at the whip's midpoint, got only {width}px"
    );
}

#[test]
fn whip_outgoing_layer_stays_sharp_at_the_very_start() {
    let tr = whip("whip_left", 0.25, 1.0);
    let width = outgoing_blur_width_at(&tr, 0.0);
    assert!(width <= 2, "the outgoing layer must still read sharp at k=0, got a {width}px blur");
}

#[test]
fn whip_incoming_layer_is_sharp_again_by_k_0_9() {
    // The incoming layer should be streaked near the middle but have settled back to
    // sharp well before the transition ends -- by k=0.9 (the last 1-2 frames of a short
    // whip), not just exactly at k=1.0.
    let tr = whip("whip_left", 0.25, 1.0);
    let width = blur_width_at(&tr, 0.9 * tr.duration);
    assert!(width <= 4, "the incoming layer should be sharp again by k=0.9, got a {width}px blur");
}

#[test]
fn whip_outgoing_layer_is_also_sharp_again_by_k_0_9() {
    let tr = whip("whip_left", 0.25, 1.0);
    let width = outgoing_blur_width_at(&tr, 0.9 * tr.duration);
    assert!(width <= 4, "the outgoing layer should be sharp again by k=0.9, got a {width}px blur");
}

#[test]
fn whip_peak_blur_has_no_ghosting_on_either_layer() {
    // The old bug: 16 taps spread across a ~450px blur meant samples landed ~30px apart,
    // producing distinct echoes (a big single step in the row) rather than a smooth
    // streak. A dense enough tap count keeps every adjacent-pixel jump small.
    let tr = whip("whip_left", 0.25, 1.0);
    let pl = plan(&tr, 0.125, W, H);
    let mut inc = edge_frame(W, H);
    if let Some(post) = &pl.post {
        post(&mut inc);
    }
    let inc_step = max_adjacent_step(&inc);
    assert!(inc_step < 0.1, "incoming layer's peak blur shows a {inc_step} jump between adjacent pixels (ghosting)");

    let mut out = edge_frame(W, H);
    if let Some(under) = &pl.under {
        under(&mut out, None);
    }
    let out_step = max_adjacent_step(&out);
    assert!(out_step < 0.1, "outgoing layer's peak blur shows a {out_step} jump between adjacent pixels (ghosting)");
}
