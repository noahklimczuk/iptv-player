//! Still frames for the seek preview.
//!
//! Hovering the scrub bar shows the picture at that point, where one can be had without
//! opening another connection to the provider: frames kept from what has already
//! played (the player grabs one every few seconds), and any frame at all of a local
//! recording, read by a second, silent player that only ever touches the file.
//!
//! This half is platform-neutral -- the type, and the downscale from what mpv hands
//! over -- so it can be tested on any host. The grabbing is in `mpv.rs`.

/// A small RGB picture, rows top to bottom, three bytes a pixel, no padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// Something that can produce the frame at a given point of one file.
pub trait FrameSource: Send {
    /// The frame at `secs`, no wider than `max_width`. `None` when it cannot be had.
    fn frame_at(&mut self, secs: f64, max_width: u32) -> Option<Frame>;
}

/// Shrink a `bgr0` image (what `screenshot-raw` returns) to at most `max_width` wide,
/// keeping its shape, averaging each source block so a thumbnail is not a shimmer of
/// sampled pixels.
///
/// `stride` is bytes per source row, which mpv may pad beyond `width * 4`.
pub fn downscale_bgr0(
    src: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    max_width: u32,
) -> Option<Frame> {
    if width == 0 || height == 0 || max_width == 0 || stride < width as usize * 4 {
        return None;
    }
    if src.len() < stride * (height as usize - 1) + width as usize * 4 {
        return None;
    }
    let out_w = width.min(max_width).max(1);
    let out_h = ((height as u64 * out_w as u64 + width as u64 / 2) / width as u64).max(1) as u32;
    let mut rgb = Vec::with_capacity(out_w as usize * out_h as usize * 3);
    for oy in 0..out_h {
        let y0 = (oy as u64 * height as u64 / out_h as u64) as usize;
        let y1 = (((oy + 1) as u64 * height as u64 / out_h as u64) as usize).max(y0 + 1);
        for ox in 0..out_w {
            let x0 = (ox as u64 * width as u64 / out_w as u64) as usize;
            let x1 = (((ox + 1) as u64 * width as u64 / out_w as u64) as usize).max(x0 + 1);
            let (mut b, mut g, mut r, mut n) = (0u32, 0u32, 0u32, 0u32);
            for y in y0..y1.min(height as usize) {
                let row = &src[y * stride..];
                for x in x0..x1.min(width as usize) {
                    let p = &row[x * 4..x * 4 + 3];
                    b += p[0] as u32;
                    g += p[1] as u32;
                    r += p[2] as u32;
                    n += 1;
                }
            }
            let n = n.max(1);
            rgb.extend_from_slice(&[(r / n) as u8, (g / n) as u8, (b / n) as u8]);
        }
    }
    Some(Frame {
        width: out_w,
        height: out_h,
        rgb,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, stride: usize, bgr: [u8; 3]) -> Vec<u8> {
        let mut v = vec![0u8; stride * h as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                v[y * stride + x * 4..y * stride + x * 4 + 4]
                    .copy_from_slice(&[bgr[0], bgr[1], bgr[2], 0]);
            }
        }
        v
    }

    #[test]
    fn a_frame_keeps_its_shape_and_turns_bgr_into_rgb() {
        let src = solid(1920, 1080, 1920 * 4, [10, 20, 30]);
        let f = downscale_bgr0(&src, 1920, 1080, 1920 * 4, 160).unwrap();
        assert_eq!((f.width, f.height), (160, 90));
        assert_eq!(f.rgb.len(), 160 * 90 * 3);
        assert_eq!(&f.rgb[..3], &[30, 20, 10]);
    }

    #[test]
    fn padding_at_the_end_of_each_row_is_skipped() {
        let mut src = solid(4, 2, 32, [0, 0, 255]);
        // Garbage in the padding must not leak into the picture.
        for y in 0..2 {
            for b in &mut src[y * 32 + 16..y * 32 + 32] {
                *b = 77;
            }
        }
        let f = downscale_bgr0(&src, 4, 2, 32, 2).unwrap();
        assert!(f.rgb.chunks(3).all(|p| p == [255, 0, 0]));
    }

    #[test]
    fn a_small_picture_is_not_enlarged_and_nonsense_is_refused() {
        let src = solid(100, 50, 400, [1, 2, 3]);
        assert_eq!(downscale_bgr0(&src, 100, 50, 400, 160).unwrap().width, 100);
        assert!(
            downscale_bgr0(&src, 100, 50, 300, 160).is_none(),
            "stride too short"
        );
        assert!(
            downscale_bgr0(&src[..10], 100, 50, 400, 160).is_none(),
            "buffer too short"
        );
        assert!(downscale_bgr0(&src, 0, 50, 400, 160).is_none());
    }
}
