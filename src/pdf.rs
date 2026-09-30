//! A minimal PDF writer: one full-page RGB image per page.
//!
//! This is all a slide export needs, and small enough to write directly
//! rather than pull in a PDF library. The file is PDF 1.4: a catalog, a page
//! tree, and for each page a content stream that paints one Flate-compressed
//! image across the whole page.

use std::fmt::Write as _;

use miniz_oxide::deflate::compress_to_vec_zlib;

/// Pixels per PDF point. Four gives a page of about 13.3 x 7.5 inches for
/// a 3840-pixel-wide image, the usual size of a 16:9 slide.
const PIXELS_PER_POINT: f64 = 4.0;

/// zlib compression level; 6 is zlib's own default trade-off.
const COMPRESSION: u8 = 6;

pub struct Pdf {
    title: String,
    /// Each page's image: width, height, and zlib-compressed RGB rows.
    pages: Vec<(u32, u32, Vec<u8>)>,
}

impl Pdf {
    pub fn new(title: &str) -> Pdf {
        Pdf {
            title: title.to_string(),
            pages: Vec::new(),
        }
    }

    /// Adds a page showing `pixels`, `0x00RRGGBB` values in rows of `width`.
    /// Pages are compressed as they are added, so a long deck does not hold
    /// every uncompressed frame in memory.
    pub fn add_page(&mut self, width: u32, height: u32, pixels: &[u32]) {
        assert_eq!(
            pixels.len(),
            width as usize * height as usize,
            "pixel count"
        );
        let mut rgb = Vec::with_capacity(pixels.len() * 3);
        for p in pixels {
            rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
        }
        self.pages
            .push((width, height, compress_to_vec_zlib(&rgb, COMPRESSION)));
    }

    /// The finished file.
    pub fn finish(self) -> Vec<u8> {
        let mut out = Writer::default();
        out.raw(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

        // Objects: 1 catalog, 2 page tree, 3 info, then three per page
        // (page, content stream, image).
        let n = self.pages.len();
        let page_id = |i: usize| 4 + 3 * i;
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", page_id(i))).collect();

        out.object(1, "<< /Type /Catalog /Pages 2 0 R >>");
        out.object(
            2,
            &format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
        );
        out.object(
            3,
            &format!(
                "<< /Title {} /Producer (tui-slides) >>",
                text_string(&self.title)
            ),
        );
        for (i, (w, h, data)) in self.pages.iter().enumerate() {
            let (page, content, image) = (page_id(i), page_id(i) + 1, page_id(i) + 2);
            let (pw, ph) = (points(*w), points(*h));
            out.object(
                page,
                &format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw} {ph}] \
                     /Resources << /XObject << /Im0 {image} 0 R >> >> /Contents {content} 0 R >>"
                ),
            );
            // Scale the unit-square image to cover the page.
            out.stream(
                content,
                "",
                format!("q {pw} 0 0 {ph} 0 0 cm /Im0 Do Q").as_bytes(),
            );
            out.stream(
                image,
                &format!(
                    "/Type /XObject /Subtype /Image /Width {w} /Height {h} \
                     /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode"
                ),
                data,
            );
        }
        out.trailer(3 + 3 * n, 1, 3)
    }
}

/// A pixel length as PDF points, written without needless decimals.
fn points(px: u32) -> String {
    let pt = f64::from(px) / PIXELS_PER_POINT;
    let s = format!("{pt:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A PDF text string. UTF-16BE with a byte-order mark, in hex, handles any
/// title without escaping rules.
fn text_string(s: &str) -> String {
    let mut out = String::from("<FEFF");
    for unit in s.encode_utf16() {
        let _ = write!(out, "{unit:04X}");
    }
    out.push('>');
    out
}

/// Accumulates the file, remembering where each object starts for the
/// cross-reference table.
#[derive(Default)]
struct Writer {
    buf: Vec<u8>,
    offsets: Vec<(usize, usize)>,
}

impl Writer {
    fn raw(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    fn start(&mut self, id: usize) {
        self.offsets.push((id, self.buf.len()));
        self.raw(format!("{id} 0 obj\n").as_bytes());
    }

    fn object(&mut self, id: usize, body: &str) {
        self.start(id);
        self.raw(body.as_bytes());
        self.raw(b"\nendobj\n");
    }

    fn stream(&mut self, id: usize, dict: &str, data: &[u8]) {
        self.start(id);
        self.raw(format!("<< {dict} /Length {} >>\nstream\n", data.len()).as_bytes());
        self.raw(data);
        self.raw(b"\nendstream\nendobj\n");
    }

    fn trailer(mut self, last_id: usize, root: usize, info: usize) -> Vec<u8> {
        self.offsets.sort_unstable();
        debug_assert!(
            self.offsets.iter().map(|(id, _)| *id).eq(1..=last_id),
            "objects are numbered 1..=N with no gaps"
        );
        let xref = self.buf.len();
        let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", last_id + 1);
        for (_, offset) in &self.offsets {
            // Each entry is exactly 20 bytes, including the two-byte EOL.
            let _ = writeln!(table, "{offset:010} 00000 n ");
        }
        let _ = write!(
            table,
            "trailer\n<< /Size {} /Root {root} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            last_id + 1
        );
        self.raw(table.as_bytes());
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniz_oxide::inflate::decompress_to_vec_zlib;

    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    fn two_page_pdf() -> Vec<u8> {
        let mut pdf = Pdf::new("Dogwood (talk) · 2026");
        pdf.add_page(
            4,
            2,
            &[0xFF0000, 0x00FF00, 0x0000FF, 0xFFFFFF, 0, 0, 0, 0x123456],
        );
        pdf.add_page(8, 2, &[0x0000AA; 16]);
        pdf.finish()
    }

    #[test]
    fn structure_and_cross_references() {
        let pdf = two_page_pdf();
        assert!(pdf.starts_with(b"%PDF-1.4\n"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Count 2"));
        assert!(text.contains("/MediaBox [0 0 1 0.5]"));
        assert!(text.contains("/MediaBox [0 0 2 0.5]"));

        // startxref points at the table, and every entry at its object.
        // (Offsets are in bytes, so work on the raw file, not the lossy text:
        // the binary comment on line 2 is not UTF-8.)
        let tail_at = pdf.windows(10).rposition(|w| w == b"startxref\n").unwrap() + 10;
        let tail = std::str::from_utf8(&pdf[tail_at..]).unwrap();
        let xref: usize = tail.lines().next().unwrap().parse().unwrap();
        assert!(pdf[xref..].starts_with(b"xref\n0 10\n"));
        let table = std::str::from_utf8(&pdf[xref..]).unwrap();
        let entries = &table.lines().skip(3).take(9).collect::<Vec<_>>();
        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(entry.len(), 19, "{entry:?}");
            let offset: usize = entry[..10].parse().unwrap();
            let header = format!("{} 0 obj\n", i + 1);
            assert!(
                pdf[offset..].starts_with(header.as_bytes()),
                "object {}",
                i + 1
            );
        }
    }

    #[test]
    fn images_decompress_to_the_pixels() {
        let pdf = two_page_pdf();
        let at = find(&pdf, b"/Width 4 /Height 2").unwrap();
        let len_at = at + find(&pdf[at..], b"/Length ").unwrap() + 8;
        let len: usize = std::str::from_utf8(&pdf[len_at..len_at + 10])
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let data = len_at + find(&pdf[len_at..], b"stream\n").unwrap() + 7;
        let rgb = decompress_to_vec_zlib(&pdf[data..data + len]).unwrap();
        assert_eq!(&rgb[..6], &[0xFF, 0, 0, 0, 0xFF, 0]);
        assert_eq!(&rgb[21..], &[0x12, 0x34, 0x56]);
    }

    #[test]
    fn titles_survive_any_characters() {
        assert_eq!(text_string("A·"), "<FEFF004100B7>");
        let text = String::from_utf8_lossy(&two_page_pdf()).to_string();
        assert!(text.contains(&text_string("Dogwood (talk) · 2026")));
    }

    #[test]
    fn point_sizes() {
        assert_eq!(points(3840), "960");
        assert_eq!(points(2150), "537.5");
        assert_eq!(points(1), "0.25");
    }
}
