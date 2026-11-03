//! Cheap per-clip noise-level estimate (quality audit #6): a fast, deterministic
//! single-frame noise sigma, so auto_edit can tell a genuinely noisy phone/webcam
//! clip from a clean one at analysis time and denoise only the former.
//!
//! Method: Immerkaer's Laplacian estimator (J. Immerkaer, "Fast Noise Variance
//! Estimation", CVIU 1996) with the kernel
//!   [[1,-2,1],[-2,4,-2],[1,-2,1]]
//! which has zero response to any locally linear or quadratic surface (a flat
//! region OR a smooth gradient/ramp), so in principle it isolates noise even
//! across a gentle exposure gradient. In practice a real edge or fine texture
//! also triggers a large response that is not noise, so this estimator gates
//! on a plain local-gradient threshold first and only accumulates the Laplacian
//! response at pixels that are locally flat (the audit's own "flat-region
//! variance" framing) -- real edges and detailed texture are excluded from the
//! estimate rather than counted as noise.
//!
//! Deterministic, single pass, no RNG, no time. Operates on grayscale f32
//! buffers (0..=1 nominal, as `tracking::clip_gray_pub` returns).

/// Local-gradient-gated Immerkaer noise sigma on one grayscale frame. Returns
/// 0.0 for a frame too small to have interior pixels (<3x3), or one with no
/// flat interior pixels at all (degenerate: fully textured or wrong-sized
/// buffer). `flat_grad_thresh` is the central-difference magnitude (0..1 luma
/// units) below which a pixel counts as "flat" and its Laplacian response is
/// counted; above it, the pixel is skipped (a real edge, not noise).
pub fn laplacian_noise_sigma(gray: &[f32], w: usize, h: usize, flat_grad_thresh: f32) -> f64 {
    if w < 3 || h < 3 || gray.len() < w * h {
        return 0.0;
    }
    let at = |x: usize, y: usize| gray[y * w + x] as f64;
    let mut sum = 0.0f64;
    let mut n = 0u64;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            // Flat-region gate: the largest absolute difference to any of the 4 immediate neighbours.
            // Deliberately an ADJACENT-pixel difference, not a skip-one central difference -- a skip-one
            // gradient is blind to period-2 (Nyquist) texture (a tight checkerboard, or the fine fractal
            // detail a mandelbrot test pattern carries even in its nominally smooth regions measured
            // 0.044 with the skip-one version, 55x the real-footage range, before this fix): x-1 and x+1
            // are the same parity and cancel exactly for a period-2 signal, so it reads as "flat" when it
            // is the busiest texture a frame can carry. Adjacent differences catch that directly.
            let c = at(x, y);
            let d_left = (c - at(x - 1, y)).abs();
            let d_right = (at(x + 1, y) - c).abs();
            let d_up = (c - at(x, y - 1)).abs();
            let d_down = (at(x, y + 1) - c).abs();
            let max_d = d_left.max(d_right).max(d_up).max(d_down);
            if max_d as f32 > flat_grad_thresh {
                continue;
            }
            let lap = at(x - 1, y - 1) - 2.0 * at(x, y - 1) + at(x + 1, y - 1) - 2.0 * at(x - 1, y)
                + 4.0 * at(x, y)
                - 2.0 * at(x + 1, y)
                + at(x - 1, y + 1)
                - 2.0 * at(x, y + 1)
                + at(x + 1, y + 1);
            if !lap.is_finite() {
                continue;
            }
            sum += lap.abs();
            n += 1;
        }
    }
    if n == 0 {
        return 0.0;
    }
    // Immerkaer's normalization: sigma = sqrt(pi/2) * sum(|L|) / (6 * n).
    (std::f64::consts::PI / 2.0).sqrt() * sum / (6.0 * n as f64)
}

/// The default flat-region gradient threshold, in 0..=1 luma units (about
/// 5/255): tight enough to exclude real edges and fine texture, loose enough
/// that ordinary smooth skin/sky/wall regions in real footage still qualify.
pub const DEFAULT_FLAT_GRAD_THRESH: f32 = 0.02;

/// Sample up to `max_samples` frames evenly spaced across a media clip
/// (including the endpoints) and return the MEDIAN per-frame flat-region noise
/// sigma -- robust to one atypical sampled frame (a flash frame, a hard cut
/// artifact at frame 0) skewing the estimate. Clips shorter than 2 frames are
/// measured on their single frame. Not a media clip, or any decode error on
/// the sampled frames, is returned as an `Err` for the caller to decide how to
/// treat (auto_edit treats it as "skip, don't fail the whole edit").
pub fn estimate_clip_noise(
    p: &crate::model::Project,
    cache: &mut crate::render::Cache,
    c: &crate::model::Clip,
    max_samples: usize,
) -> Result<f64, String> {
    if !matches!(c.source, crate::model::Source::Media { .. }) {
        return Err(format!("clip '{}' is not a media clip", c.id));
    }
    let n = crate::tracking::clip_frame_count(c.duration, p.fps);
    let k = max_samples.max(1).min(n.max(1));
    let mut sigmas = Vec::with_capacity(k);
    for j in 0..k {
        let i = if k <= 1 { n / 2 } else { j * (n - 1) / (k - 1) };
        // tracking::clip_gray_pub returns luma in the raw 0..=255 byte range (0.299R+0.587G+0.114B with no
        // /255 normalization -- confirmed against its own gray() helper), not 0..=1; normalize here so this
        // module's own 0..=1 convention (and DEFAULT_FLAT_GRAD_THRESH, calibrated in 0..=1 units) holds.
        let gray_255 = crate::tracking::clip_gray_pub(p, cache, c, i)?;
        let gray: Vec<f32> = gray_255.iter().map(|&v| v / 255.0).collect();
        sigmas.push(laplacian_noise_sigma(&gray, p.width as usize, p.height as usize, DEFAULT_FLAT_GRAD_THRESH));
    }
    sigmas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(sigmas[sigmas.len() / 2])
}
