//! Pictures in and out: favicons decoded for the toolkit to draw, and the
//! window's pixels encoded as a PNG (`ShellView::screenshot`).
//!
//! The toolkit draws premultiplied BGRA (`Ui::set_image`); favicons arrive
//! as PNG (the engine's `Image`, the shell's icon cache on disk). Anything
//! bigger than [`MAX_ICON`] is boxed down once here, so a 512-pixel touch
//! icon costs what a favicon does.

use std::io::Cursor;

/// The largest side kept for a tab's mark: 16–18 DIPs at up to 300 %.
pub const MAX_ICON: u32 = 64;

/// A decoded picture: premultiplied BGRA, `w`×`h`.
#[derive(Clone, PartialEq, Eq)]
pub struct Picture {
    pub w: u32,
    pub h: u32,
    pub bgra: Vec<u8>,
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Picture({}x{})", self.w, self.h)
    }
}

/// Decode a PNG (any colour type and depth) into premultiplied BGRA.
/// `None` for anything that is not a sound PNG of a sane size.
pub fn decode_png(bytes: &[u8]) -> Option<Picture> {
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: 16 * 1024 * 1024,
        },
    );
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    if w == 0 || h == 0 {
        return None;
    }
    let px = (w * h) as usize;
    let mut bgra = Vec::with_capacity(px * 4);
    let src = &buf[..info.buffer_size()];
    let mut push = |r: u8, g: u8, b: u8, a: u8| {
        let pm = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
        bgra.extend_from_slice(&[pm(b), pm(g), pm(r), a]);
    };
    match info.color_type {
        png::ColorType::Rgba => src
            .as_chunks::<4>()
            .0
            .iter()
            .for_each(|p| push(p[0], p[1], p[2], p[3])),
        png::ColorType::Rgb => src
            .as_chunks::<3>()
            .0
            .iter()
            .for_each(|p| push(p[0], p[1], p[2], 255)),
        png::ColorType::GrayscaleAlpha => src
            .as_chunks::<2>()
            .0
            .iter()
            .for_each(|p| push(p[0], p[0], p[0], p[1])),
        png::ColorType::Grayscale => src.iter().for_each(|&v| push(v, v, v, 255)),
        png::ColorType::Indexed => return None,
    }
    (bgra.len() == px * 4).then_some(Picture { w, h, bgra })
}

/// `p` boxed down so neither side exceeds `max` (aspect kept); smaller
/// pictures come back as they are.
pub fn fit(p: Picture, max: u32) -> Picture {
    if p.w <= max && p.h <= max {
        return p;
    }
    let scale = f64::from(max) / f64::from(p.w.max(p.h));
    let w = ((f64::from(p.w) * scale).round() as u32).max(1);
    let h = ((f64::from(p.h) * scale).round() as u32).max(1);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let y0 = y * p.h / h;
        let y1 = ((y + 1) * p.h / h).max(y0 + 1);
        for x in 0..w {
            let x0 = x * p.w / w;
            let x1 = ((x + 1) * p.w / w).max(x0 + 1);
            let mut sum = [0u32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = ((sy * p.w + sx) * 4) as usize;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += u32::from(p.bgra[i + c]);
                    }
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            let o = ((y * w + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = ((sum[c] + n / 2) / n) as u8;
            }
        }
    }
    Picture { w, h, bgra: out }
}

/// A favicon for the toolkit: decoded and fitted.
pub fn icon(png: &[u8]) -> Option<Picture> {
    decode_png(png).map(|p| fit(p, MAX_ICON))
}

/// Encode top-down BGRA rows (`stride` bytes apart, alpha ignored: a
/// window's pixels are opaque) as an RGB PNG.
pub fn encode_png(bgra: &[u8], w: u32, h: u32, stride: usize) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || stride < w as usize * 4 || bgra.len() < stride * h as usize {
        return None;
    }
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for row in bgra.chunks(stride).take(h as usize) {
        for p in row[..w as usize * 4].as_chunks::<4>().0 {
            rgb.extend_from_slice(&[p[2], p[1], p[0]]);
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(&rgb).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_of(w: u32, h: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(color);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(data).unwrap();
        }
        out
    }

    #[test]
    fn rgba_is_premultiplied_and_swizzled() {
        let png = png_of(1, 1, png::ColorType::Rgba, &[255, 0, 0, 128]);
        let p = decode_png(&png).unwrap();
        assert_eq!((p.w, p.h), (1, 1));
        assert_eq!(p.bgra, vec![0, 0, 128, 128]);
    }

    #[test]
    fn rgb_and_grey_are_opaque() {
        let p = decode_png(&png_of(1, 1, png::ColorType::Rgb, &[1, 2, 3])).unwrap();
        assert_eq!(p.bgra, vec![3, 2, 1, 255]);
        let p = decode_png(&png_of(1, 1, png::ColorType::Grayscale, &[9])).unwrap();
        assert_eq!(p.bgra, vec![9, 9, 9, 255]);
    }

    #[test]
    fn garbage_is_refused() {
        assert!(decode_png(b"not a png").is_none());
        assert!(decode_png(&[]).is_none());
    }

    #[test]
    fn big_icons_are_boxed_down() {
        let data = vec![200u8; 256 * 128 * 4];
        let p = icon(&png_of(256, 128, png::ColorType::Rgba, &data)).unwrap();
        assert_eq!((p.w, p.h), (64, 32));
        assert_eq!(p.bgra.len(), 64 * 32 * 4);
        // 200 premultiplied by 200/255, averaged over identical pixels.
        assert_eq!(p.bgra[3], 200);
    }

    #[test]
    fn screenshots_round_trip() {
        // Two rows of 2 pixels with 4 bytes of row padding.
        let stride = 12;
        let mut bgra = vec![0u8; stride * 2];
        bgra[..8].copy_from_slice(&[10, 20, 30, 0, 40, 50, 60, 0]);
        bgra[12..20].copy_from_slice(&[70, 80, 90, 0, 1, 2, 3, 0]);
        let png = encode_png(&bgra, 2, 2, stride).unwrap();
        let p = decode_png(&png).unwrap();
        assert_eq!(&p.bgra[..4], &[10, 20, 30, 255]);
        assert_eq!(&p.bgra[12..16], &[1, 2, 3, 255]);
        assert!(
            encode_png(&bgra, 4, 2, stride).is_none(),
            "stride too short"
        );
    }
}
