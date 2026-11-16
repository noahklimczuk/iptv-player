//! A still of the picture, for the seek preview (E31).

/// An RGB picture, tightly packed: `width * height * 3` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// The largest picture mpv could plausibly hand back. Anything bigger is a corrupt
/// reply, and refusing it is cheaper than allocating for it.
const MAX_SIDE: u32 = 16_384;

impl Frame {
    /// From `screenshot-raw`'s `bgr0` (four bytes a pixel, blue first, rows `stride`
    /// bytes apart), shrunk to at most `max_width` wide and never enlarged.
    ///
    /// `None` for a reply that does not describe a picture: a zero or absurd size, or
    /// fewer bytes than the size and stride promise.
    pub fn from_bgr0(
        width: u32,
        height: u32,
        stride: usize,
        data: &[u8],
        max_width: u32,
    ) -> Option<Self> {
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE || max_width == 0 {
            return None;
        }
        let row_bytes = width as usize * 4;
        if stride < row_bytes {
            return None;
        }
        let needed = stride
            .checked_mul(height as usize - 1)?
            .checked_add(row_bytes)?;
        if data.len() < needed {
            return None;
        }
        let out_w = width.min(max_width);
        // Rounded, and at least one row, so a very wide strip still yields a picture.
        let out_h = ((u64::from(height) * u64::from(out_w) + u64::from(width) / 2)
            / u64::from(width))
        .max(1) as u32;
        let mut rgb = Vec::with_capacity(out_w as usize * out_h as usize * 3);
        for y in 0..out_h {
            let sy = (u64::from(y) * u64::from(height) / u64::from(out_h)) as usize;
            let row = &data[sy * stride..sy * stride + row_bytes];
            for x in 0..out_w {
                let sx = (u64::from(x) * u64::from(width) / u64::from(out_w)) as usize * 4;
                rgb.extend_from_slice(&[row[sx + 2], row[sx + 1], row[sx]]);
            }
        }
        Some(Self {
            width: out_w,
            height: out_h,
            rgb,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture whose every pixel says where it is, so scaling can be checked.
    fn picture(w: u32, h: u32, stride: usize) -> Vec<u8> {
        let mut d = vec![0xEE; stride * h as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let i = y * stride + x * 4;
                d[i] = 1; // blue
                d[i + 1] = x as u8; // green
                d[i + 2] = y as u8; // red
            }
        }
        d
    }

    #[test]
    fn blue_first_becomes_red_first() {
        let f = Frame::from_bgr0(2, 2, 8, &picture(2, 2, 8), 100).unwrap();
        assert_eq!(&f.rgb[..3], &[0, 0, 1]);
        assert_eq!(&f.rgb[3..6], &[0, 1, 1]);
    }

    #[test]
    fn row_padding_is_skipped() {
        let f = Frame::from_bgr0(3, 2, 16, &picture(3, 2, 16), 100).unwrap();
        assert_eq!(f.rgb.len(), 3 * 2 * 3);
        assert!(!f.rgb.contains(&0xEE));
    }

    #[test]
    fn a_large_picture_is_shrunk_keeping_its_shape() {
        let f = Frame::from_bgr0(200, 100, 800, &picture(200, 100, 800), 50).unwrap();
        assert_eq!((f.width, f.height), (50, 25));
        assert_eq!(f.rgb.len(), 50 * 25 * 3);
    }

    #[test]
    fn a_small_picture_is_not_enlarged() {
        let f = Frame::from_bgr0(20, 10, 80, &picture(20, 10, 80), 160).unwrap();
        assert_eq!((f.width, f.height), (20, 10));
    }

    #[test]
    fn nonsense_is_refused() {
        assert!(Frame::from_bgr0(0, 10, 0, &[], 10).is_none());
        assert!(Frame::from_bgr0(10, 10, 40, &[0; 39], 10).is_none());
        assert!(Frame::from_bgr0(10, 10, 20, &[0; 400], 10).is_none());
        assert!(Frame::from_bgr0(100_000, 1, 400_000, &[], 10).is_none());
        assert!(Frame::from_bgr0(10, 10, 40, &[0; 400], 0).is_none());
    }
}
