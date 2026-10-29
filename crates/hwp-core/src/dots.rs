//! 그림을 터미널 글자로 — 점자와 placeholder.
//!
//! PLAN.md §2-4 의 세 모드 중 둘을 여기서 만든다.
//!
//! **placeholder** 는 그림의 자리와 정체만 네모로 알린다. 늘 동작하고 거짓말을
//! 하지 않는다. 그림이 있는 자리에 `￼` 를 날것으로 찍고 있었는데, 그건 터미널에서
//! 빈 네모로 보여서 "그림이 안 보인다" 가 된다.
//!
//! **점자**(U+2800 블록)는 한 칸에 **2×4 점**을 담는다. 같은 칸 수에 블록 문자의
//! 8배, 반블록의 4배 해상도가 나오므로 터미널에서 그림을 보는 가장 정보량 많은
//! 방법이다. 1비트로 떨구므로 Atkinson 디더를 쓴다 — Floyd–Steinberg 보다 오차를
//! 덜 퍼뜨려 선과 글자가 뭉개지지 않고, 1984년 매킨토시가 흑백 화면에 쓰던 그
//! 알고리즘이라 레트로 도트의 결이 그대로 나온다.


/// 점자 한 칸이 담는 점의 수.
pub const DOT_COLS: u32 = 2;
pub const DOT_ROWS: u32 = 4;

/// 점자 칸 안에서 (x, y) → 비트 번호.
///
/// U+2800 의 비트 배치는 **열 우선이 아니다.** 왼쪽 열이 위에서 아래로 1·2·3,
/// 오른쪽 열이 4·5·6, 그리고 네 번째 행이 뒤늦게 7·8 로 붙었다(6점 점자에 2점을
/// 더해 8점이 된 역사 때문이다). 순서대로 채우면 그림이 세로로 찢어진다.
fn dot_bit(x: u32, y: u32) -> u8 {
    const BITS: [[u8; 2]; 4] = [
        [0x01, 0x08], // 1행: 왼쪽 점1, 오른쪽 점4
        [0x02, 0x10], // 2행: 점2, 점5
        [0x04, 0x20], // 3행: 점3, 점6
        [0x40, 0x80], // 4행: 점7, 점8  ← 뒤에 붙은 두 점
    ];
    BITS[y as usize % 4][x as usize % 2]
}

/// 점자로 그린 그림.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotArt {
    /// 글자 줄들. 각 줄의 표시 폭은 `cols` 다.
    pub lines: Vec<String>,
    pub cols: usize,
    pub rows: usize,
    /// **실제로 쓴** 글리프. 요청한 것과 다를 수 있다 — 넓게 그리는 터미널에서는
    /// 점자·반블록이 ASCII 로 바뀐다([`Glyphs::narrow`]). 쓴 것을 말하지 않으면
    /// 사용자는 왜 그림이 거칠어졌는지 알 수 없다.
    pub glyphs: Glyphs,
}

/// 그리는 방식.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// 밝기 범위를 펼친다. 문서 그림은 어두운 배너나 흐린 스캔이 많고, 그대로
    /// 1비트로 떨구면 **통째로 칠해져** 아무것도 안 보인다. 실측: 코퍼스의 첫
    /// 그림은 픽셀의 75%가 128 미만이고 중앙값이 0 이라 전부 켜졌다.
    pub normalize: bool,
    /// 밝고 어두움을 뒤집는다. 어두운 배경에 밝은 글씨인 그림에서 쓴다.
    pub invert: bool,
    /// 어떤 글자로 그릴지.
    pub glyphs: Glyphs,
    /// 켜질 점이 과반이면 **스스로 뒤집는다.**
    ///
    /// 점자는 켜진 점이 잉크다. 잉크가 다수면 칸이 통째로 `⣿` 가 되어 정보가
    /// 0 이 된다. 실측: 코퍼스의 첫 그림은 어두운 배너에 흰 글씨인데 픽셀의
    /// 75%가 어두워 전부 칠해졌고, 대비 확장도 듣지 않았다(이미 0~255 를 다
    /// 쓰는 그림이라 펼칠 여지가 없었다). 뒤집으면 글씨가 잉크가 된다.
    pub auto_invert: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { normalize: true, invert: false, auto_invert: true, glyphs: Glyphs::Braille }
    }
}

/// 어떤 글자로 그릴지.
///
/// 점자는 해상도가 가장 높지만 **터미널 글꼴에 점자가 없으면 네모로 보인다.**
/// 블록 문자는 해상도가 1/4 이지만 어느 글꼴에나 있다 — 보이지 않는 고해상도보다
/// 보이는 저해상도가 낫다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyphs {
    /// U+2800 점자 — 한 칸에 2×4 점
    Braille,
    /// 반블록(`▀▄█`) — 한 칸에 1×2 점. 어느 글꼴에나 있다
    Blocks,
    /// ASCII 만 (`@%+-. `) — 전신 환경까지
    Ascii,
}

impl Glyphs {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "braille" | "dots" => Some(Self::Braille),
            "blocks" | "block" => Some(Self::Blocks),
            "ascii" => Some(Self::Ascii),
            _ => None,
        }
    }

    /// **터미널이 넓게 그리는 폭 정책에서 안전한 글리프.**
    ///
    /// 점자(U+28xx)와 반블록(`▀▄█`)은 선 문자와 똑같이 East Asian
    /// **Ambiguous** 다. 넓게 그리는 터미널은 한 글리프를 두 칸으로 내는데,
    /// 그림을 만들 때는 한 칸으로 셈했으므로 줄이 두 배가 되어 표 안의
    /// 그림이 **표를 부순다.** 실측(2026-10-10, s00.hwp 2쪽): 바깥 테두리는
    /// 40칸인데 그림이 든 줄은 26~35칸으로 잘려 오른쪽 세로줄이 사라졌다.
    ///
    /// ratatui 는 폭을 `width()`(Ambiguous = 1)로 재므로 우리가 폭을 두 칸으로
    /// 셈해도 배치가 어긋난다 — 그래서 **아예 안 쓴다.** 표를 ASCII(`+ - |`)로
    /// 그리기로 한 것과 같은 판단이다: 유니코드에 좁다고 보장된 그림 글자가
    /// 없다.
    pub fn narrow(self) -> Self {
        match self {
            Self::Braille | Self::Blocks if crate::wrap::ambiguous_wide() => Self::Ascii,
            other => other,
        }
    }

    /// 한 칸이 담는 점의 수 (가로, 세로).
    fn cell(self) -> (u32, u32) {
        match self {
            Self::Braille => (2, 4),
            Self::Blocks => (1, 2),
            Self::Ascii => (1, 1),
        }
    }
}

/// 그림 바이트를 `max_cols` 칸 폭에 맞춰 점자로 그린다.
///
/// 세로는 가로비를 지켜 정한다 — 점자 칸은 2×4 점이라 **세로로 긴 칸**이므로,
/// 칸 수를 그대로 비율에 맞추면 그림이 납작해진다.
pub fn render(bytes: &[u8], max_cols: usize, max_rows: usize) -> Option<DotArt> {
    render_with(bytes, max_cols, max_rows, Options::default())
}

pub fn render_with(
    bytes: &[u8],
    max_cols: usize,
    max_rows: usize,
    opts: Options,
) -> Option<DotArt> {
    let img = image::load_from_memory(bytes).ok()?;
    Some(render_image(&img, max_cols, max_rows, opts))
}

fn render_image(
    img: &image::DynamicImage,
    max_cols: usize,
    max_rows: usize,
    opts: Options,
) -> DotArt {
    use image::imageops::FilterType;

    let max_cols = max_cols.max(1);
    let max_rows = max_rows.max(1);
    // **폭 정책을 여기서 한 번 본다.** 호출부마다 보게 하면 한 곳이 빠지고,
    // 빠진 곳은 "그림이 표를 부순다" 로만 드러난다.
    let opts = Options { glyphs: opts.glyphs.narrow(), ..opts };
    let (cell_w, cell_h) = opts.glyphs.cell();
    let (iw, ih) = (img.width().max(1), img.height().max(1));

    // 점 단위 목표 크기. 칸이 2×4 점이므로 세로 점 하나가 가로 점 하나의 절반
    // 높이를 차지한다 — 가로비를 지키려면 세로 점을 두 배로 잡아야 한다.
    let dot_w = (max_cols as u32) * cell_w;
    let dot_h_cap = (max_rows as u32) * cell_h;
    // 터미널 칸은 가로보다 세로가 길다(대략 1:2). 글자 하나가 담는 점이
    // (cell_w, cell_h) 이므로 점 하나의 가로세로비는 (2/cell_w) : (1/cell_h) 이고,
    // 가로비를 지키려면 세로 점을 그 비로 늘려야 한다.
    let dot_aspect = (2.0 / cell_w as f64) * (cell_h as f64);
    let wanted_h = (dot_w as f64 * ih as f64 / iw as f64 * dot_aspect / 2.0)
        .round()
        .max(1.0) as u32;
    let dot_h = wanted_h.min(dot_h_cap).max(1);
    // 칸 경계에 맞춰 올린다
    let dot_h = dot_h.div_ceil(cell_h) * cell_h;
    let dot_w = dot_w.div_ceil(cell_w) * cell_w;

    let mut gray = to_gray_on_white(&img.resize_exact(dot_w, dot_h, FilterType::Triangle));

    if opts.normalize {
        stretch(&mut gray);
    }
    let mut flip = opts.invert;
    if opts.auto_invert {
        // 디더 전에 임계값만으로 미리 세어 본다 — 디더를 두 번 돌리지 않으려고.
        let dark = gray.pixels().filter(|p| p.0[0] < 128).count();
        let ratio = dark as f64 / (gray.pixels().len().max(1)) as f64;
        if ratio > 0.6 {
            flip = !flip;
        }
    }
    if flip {
        for p in gray.pixels_mut() {
            p.0[0] = 255 - p.0[0];
        }
    }

    let mask = atkinson(&gray);

    let cols = (dot_w / cell_w) as usize;
    let rows = (dot_h / cell_h) as usize;
    let mut lines = Vec::with_capacity(rows);
    for cy in 0..rows as u32 {
        let mut line = String::with_capacity(cols * 3);
        for cx in 0..cols as u32 {
            let on = |x: u32, y: u32| -> bool {
                let px = cx * cell_w + x;
                let py = cy * cell_h + y;
                mask[(py * dot_w + px) as usize]
            };
            line.push(match opts.glyphs {
                Glyphs::Braille => {
                    let mut bits = 0u8;
                    for y in 0..cell_h {
                        for x in 0..cell_w {
                            if on(x, y) {
                                bits |= dot_bit(x, y);
                            }
                        }
                    }
                    // U+2800(점 없음)은 공백처럼 보이지만 폭이 1칸이라 줄이
                    // 어긋나지 않는다. 그래서 빈 칸도 점자 공백으로 둔다.
                    char::from_u32(0x2800 + bits as u32).unwrap_or(' ')
                }
                Glyphs::Blocks => match (on(0, 0), on(0, 1)) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                },
                Glyphs::Ascii => {
                    if on(0, 0) {
                        '#'
                    } else {
                        ' '
                    }
                }
            });
        }
        lines.push(line);
    }

    DotArt { lines, cols, rows, glyphs: opts.glyphs }
}

/// 회색조로 바꾸되 **투명한 자리는 흰 종이로 친다.**
///
/// `to_luma8` 은 알파를 무시한다. 투명 픽셀의 RGB 는 보통 (0,0,0) 이므로 투명한
/// 배경이 **새까맣게** 읽힌다. 실측: 코퍼스 첫 그림은 84%가 완전 투명한 로고
/// 띠인데 알파를 무시하면 픽셀의 75%가 어두워 보이고(흰 배경에 합성하면 14.5%),
/// 그 때문에 자동 반전까지 걸려 사람이 보는 것과 정반대가 나왔다.
///
/// 문서에 놓인 그림은 흰 종이 위에 있다. 그대로 친다.
fn to_gray_on_white(img: &image::DynamicImage) -> image::GrayImage {
    use image::GenericImageView;
    if !img.color().has_alpha() {
        return img.to_luma8();
    }
    let (w, h) = img.dimensions();
    let rgba = img.to_rgba8();
    let mut out = image::GrayImage::new(w, h);
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = p.0[3] as f32 / 255.0;
        // 흰 종이 위에 알파로 얹는다
        let mix = |c: u8| c as f32 * a + 255.0 * (1.0 - a);
        let (r, g, b) = (mix(p.0[0]), mix(p.0[1]), mix(p.0[2]));
        // ITU-R BT.601 — image 크레이트의 to_luma8 과 같은 가중치
        let luma = 0.299 * r + 0.587 * g + 0.114 * b;
        out.put_pixel(x, y, image::Luma([luma.round().clamp(0.0, 255.0) as u8]));
    }
    out
}

/// 밝기 범위를 2~98 백분위 기준으로 0~255 로 펼친다.
///
/// 양 끝 2% 를 버리는 이유: 스캔 그림의 검은 테두리나 흰 여백 한 점이 범위를
/// 다 잡아먹어 펼침이 무력화된다. 범위가 너무 좁으면(거의 단색) 그냥 둔다 —
/// 단색을 늘리면 잡음만 증폭된다.
fn stretch(gray: &mut image::GrayImage) {
    let mut hist = [0u32; 256];
    for p in gray.pixels() {
        hist[p.0[0] as usize] += 1;
    }
    let total: u32 = hist.iter().sum();
    if total == 0 {
        return;
    }
    let cut = (total as f64 * 0.02) as u32;

    let mut acc = 0u32;
    let mut lo = 0u8;
    for (v, c) in hist.iter().enumerate() {
        acc += c;
        if acc > cut {
            lo = v as u8;
            break;
        }
    }
    acc = 0;
    let mut hi = 255u8;
    for (v, c) in hist.iter().enumerate().rev() {
        acc += c;
        if acc > cut {
            hi = v as u8;
            break;
        }
    }

    // 거의 단색이면 건드리지 않는다
    if hi <= lo || (hi - lo) < 8 {
        return;
    }
    let span = (hi - lo) as f32;
    for p in gray.pixels_mut() {
        let v = p.0[0].clamp(lo, hi);
        p.0[0] = (((v - lo) as f32 / span) * 255.0).round() as u8;
    }
}

/// Atkinson 디더 — 오차를 이웃 6칸에 1/8 씩 퍼뜨린다(합이 6/8 이라 일부를 버린다).
///
/// 버리는 것이 의도다. 오차를 전부 보존하는 Floyd–Steinberg 는 밝은 면에 점을
/// 흩뿌려 글자와 가는 선을 지저분하게 만든다. Atkinson 은 대비를 세우는 쪽이라
/// 글자가 든 문서 그림에서 읽히는 결과가 나온다.
fn atkinson(gray: &image::GrayImage) -> Vec<bool> {
    let (w, h) = (gray.width() as usize, gray.height() as usize);
    let mut buf: Vec<f32> = gray.pixels().map(|p| p.0[0] as f32).collect();
    let mut out = vec![false; w * h];

    // 오차를 뿌릴 상대 좌표 (오른쪽 둘, 아래 세, 그 아래 하나)
    const SPREAD: [(isize, isize); 6] = [(1, 0), (2, 0), (-1, 1), (0, 1), (1, 1), (0, 2)];

    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let old = buf[i];
            // 어두운 쪽을 점으로 켠다 (종이에 잉크를 얹는 감각)
            let on = old < 128.0;
            out[i] = on;
            let new = if on { 0.0 } else { 255.0 };
            let err = (old - new) / 8.0;
            for (dx, dy) in SPREAD {
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                    continue;
                }
                buf[(ny as usize) * w + nx as usize] += err;
            }
        }
    }
    out
}

/// 그림 자리를 알리는 네모. 그림을 그릴 수 없을 때도 **자리와 정체는 말한다.**
pub fn placeholder(label: &str, width: usize) -> Vec<String> {
    let inner = width.max(8).saturating_sub(2);
    let text_w = crate::wrap::display_width(label);
    let body = if text_w <= inner {
        let pad = inner - text_w;
        let left = pad / 2;
        format!("{}{}{}", " ".repeat(left), label, " ".repeat(pad - left))
    } else {
        // 넘치면 2칸 글자를 쪼개지 않게 잘라 붙인다
        let mut cut = String::new();
        let mut used = 0usize;
        for c in label.chars() {
            let cw = crate::wrap::char_width(c);
            if used + cw > inner.saturating_sub(1) {
                break;
            }
            cut.push(c);
            used += cw;
        }
        format!("{cut}…{}", " ".repeat(inner.saturating_sub(used + 1)))
    };
    use crate::wrap::{box_h, box_v};
    let rule = |top: bool| {
        if crate::wrap::ambiguous_wide() {
            format!("+{}+", "=".repeat(inner))
        } else if top {
            format!("┌{}┐", box_h().repeat(inner))
        } else {
            format!("└{}┘", box_h().repeat(inner))
        }
    };
    vec![rule(true), format!("{}{body}{}", box_v(), box_v()), rule(false)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    /// 가로 `w` 세로 `h` 의 단색 그림을 PNG 바이트로.
    fn solid(w: u32, h: u32, v: u8) -> Vec<u8> {
        let img = image::GrayImage::from_pixel(w, h, image::Luma([v]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn braille_bits_follow_the_historical_dot_order() {
        // 왼쪽 열 위에서 아래로 1·2·3, 오른쪽 4·5·6, 넷째 행이 7·8.
        assert_eq!(dot_bit(0, 0), 0x01);
        assert_eq!(dot_bit(0, 1), 0x02);
        assert_eq!(dot_bit(0, 2), 0x04);
        assert_eq!(dot_bit(1, 0), 0x08);
        assert_eq!(dot_bit(1, 2), 0x20);
        assert_eq!(dot_bit(0, 3), 0x40);
        assert_eq!(dot_bit(1, 3), 0x80);
        // 여덟 점이 모두 다른 비트여야 한다
        let mut seen = std::collections::BTreeSet::new();
        for y in 0..4 {
            for x in 0..2 {
                assert!(seen.insert(dot_bit(x, y)), "비트가 겹친다 ({x},{y})");
            }
        }
    }

    const PLAIN: Options = Options { normalize: false, invert: false, auto_invert: false, glyphs: Glyphs::Braille };

    #[test]
    fn a_black_image_fills_every_dot_and_white_fills_none() {
        let black = render_with(&solid(16, 16, 0), 4, 4, PLAIN).unwrap();
        assert!(
            black.lines.iter().all(|l| l.chars().all(|c| c == '\u{28FF}')),
            "검은 그림은 여덟 점이 다 켜져야 한다: {:?}",
            black.lines
        );

        let white = render_with(&solid(16, 16, 255), 4, 4, PLAIN).unwrap();
        assert!(
            white.lines.iter().all(|l| l.chars().all(|c| c == '\u{2800}')),
            "흰 그림은 점이 하나도 없어야 한다: {:?}",
            white.lines
        );
    }

    #[test]
    fn every_line_has_the_width_it_claims() {
        let art = render(&solid(100, 40, 90), 20, 10).unwrap();
        assert!(art.cols <= 20, "폭을 넘지 않는다: {}", art.cols);
        assert!(art.rows <= 10, "높이를 넘지 않는다: {}", art.rows);
        for l in &art.lines {
            assert_eq!(l.width(), art.cols, "줄 폭이 어긋나면 화면이 밀린다: {l:?}");
            assert_eq!(l.chars().count(), art.cols);
        }
    }

    #[test]
    fn aspect_ratio_survives_the_tall_braille_cell() {
        // 점자 칸은 2×4 점이라 세로로 길다. 2:1 그림(가로가 두 배)을
        // 칸 수로 2:1 로 그리면 납작해진다 — 보정이 들어가야 한다.
        let wide = render(&solid(200, 100, 90), 20, 50).unwrap();
        // 200×100 → 점 40×40 → 칸 20×10
        assert_eq!((wide.cols, wide.rows), (20, 10), "{wide:?}");

        let square = render(&solid(100, 100, 90), 20, 50).unwrap();
        // 100×100 → 점 40×80 → 칸 20×20
        assert_eq!((square.cols, square.rows), (20, 20), "{square:?}");
    }

    #[test]
    fn a_flat_image_is_left_alone_so_noise_is_not_amplified() {
        // 거의 단색이면 펼침이 잡음만 키운다. 검은 그림은 여전히 전부 켜져야 한다.
        let black = render_with(&solid(16, 16, 10), 4, 4, PLAIN).unwrap();
        assert!(black.lines.iter().all(|l| l.chars().all(|c| c == '\u{28FF}')), "{black:?}");
        let white = render_with(&solid(16, 16, 245), 4, 4, PLAIN).unwrap();
        assert!(white.lines.iter().all(|l| l.chars().all(|c| c == '\u{2800}')), "{white:?}");
    }

    #[test]
    fn stretching_reveals_structure_in_a_dark_image() {
        // 0~60 범위에만 있는 그림. 그대로면 전부 켜지고, 펼치면 반쯤 갈린다.
        let mut img = image::GrayImage::new(16, 16);
        for (x, _y, p) in img.enumerate_pixels_mut() {
            p.0[0] = if x < 8 { 0 } else { 60 };
        }
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        let bytes = buf.into_inner();

        let raw = render_with(&bytes, 4, 4, Options { normalize: false, invert: false, auto_invert: false, glyphs: Glyphs::Braille }).unwrap();
        let on_raw: usize = raw.lines.iter().flat_map(|l| l.chars()).filter(|c| *c == '\u{28FF}').count();

        let norm = render(&bytes, 4, 4).unwrap();
        let on_norm: usize = norm.lines.iter().flat_map(|l| l.chars()).filter(|c| *c == '\u{28FF}').count();

        assert!(on_raw > on_norm, "펼치면 꽉 찬 칸이 줄어야 한다 (그대로 {on_raw}, 펼침 {on_norm})");
        assert!(on_norm > 0, "완전히 비어서도 안 된다");
    }

    #[test]
    fn invert_flips_which_dots_are_on() {
        let bytes = solid(16, 16, 250);
        let normal = render_with(&bytes, 4, 4, Options { normalize: false, invert: false, auto_invert: false, glyphs: Glyphs::Braille }).unwrap();
        let flipped = render_with(&bytes, 4, 4, Options { normalize: false, invert: true, auto_invert: false, glyphs: Glyphs::Braille }).unwrap();
        assert!(normal.lines.iter().all(|l| l.chars().all(|c| c == '\u{2800}')));
        assert!(flipped.lines.iter().all(|l| l.chars().all(|c| c == '\u{28FF}')));
    }

    #[test]
    fn a_mostly_dark_image_inverts_itself_so_it_is_not_a_solid_block() {
        // 어두운 배경(값 20)에 밝은 무늬(값 230)가 조금. 그대로면 거의 다 켜져
        // 통째로 `⣿` 가 되고, 스스로 뒤집으면 무늬만 잉크가 된다.
        let mut img = image::GrayImage::from_pixel(32, 32, image::Luma([20]));
        for (x, y, p) in img.enumerate_pixels_mut() {
            if x < 6 && y < 6 {
                p.0[0] = 230;
            }
        }
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        let bytes = buf.into_inner();

        let full = |a: &DotArt| -> usize {
            a.lines.iter().flat_map(|l| l.chars()).filter(|c| *c == '\u{28FF}').count()
        };
        let plain = render_with(&bytes, 8, 20, PLAIN).unwrap();
        let auto = render(&bytes, 8, 20).unwrap();
        assert!(
            full(&plain) > full(&auto),
            "스스로 뒤집으면 꽉 찬 칸이 줄어야 한다 (그대로 {}, 자동 {})",
            full(&plain),
            full(&auto)
        );
    }

    #[test]
    fn blocks_and_ascii_render_with_glyphs_every_font_has() {
        // 점자가 없는 터미널에서도 보여야 한다. 블록은 1×2 점, ASCII 는 1×1.
        let bytes = solid(32, 32, 0);
        for (glyphs, allowed) in [
            (Glyphs::Blocks, ['\u{2588}', '\u{2580}', '\u{2584}', ' ']),
            (Glyphs::Ascii, ['#', ' ', ' ', ' ']),
        ] {
            let opts = Options { glyphs, ..Default::default() };
            let art = render_with(&bytes, 8, 8, opts).unwrap();
            for l in &art.lines {
                assert_eq!(l.width(), art.cols, "{glyphs:?}: 줄 폭이 어긋난다 {l:?}");
                for c in l.chars() {
                    assert!(allowed.contains(&c), "{glyphs:?} 가 {c:?} 를 썼다");
                }
            }
        }
    }

    #[test]
    fn glyph_modes_parse_by_the_names_the_cli_offers() {
        assert_eq!(Glyphs::parse("braille"), Some(Glyphs::Braille));
        assert_eq!(Glyphs::parse("dots"), Some(Glyphs::Braille));
        assert_eq!(Glyphs::parse("blocks"), Some(Glyphs::Blocks));
        assert_eq!(Glyphs::parse("ascii"), Some(Glyphs::Ascii));
        assert_eq!(Glyphs::parse("sixel"), None);
    }

    #[test]
    fn transparent_areas_are_paper_not_ink() {
        // 투명 배경 위에 검은 점 하나. 알파를 무시하면 배경이 새까매져
        // 전부 잉크가 되고, 자동 반전까지 걸려 정반대가 나온다.
        let mut img = image::RgbaImage::from_pixel(32, 32, image::Rgba([0, 0, 0, 0]));
        for (x, y, p) in img.enumerate_pixels_mut() {
            if x < 4 && y < 4 {
                *p = image::Rgba([0, 0, 0, 255]);
            }
        }
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        let art = render(&buf.into_inner(), 8, 20).unwrap();

        let ink: usize = art
            .lines
            .iter()
            .flat_map(|l| l.chars())
            .filter(|c| *c != '\u{2800}')
            .count();
        let total: usize = art.lines.iter().map(|l| l.chars().count()).sum();
        assert!(ink > 0, "검은 점이 잉크로 남아야 한다: {art:?}");
        assert!(
            ink * 4 < total,
            "투명한 자리가 잉크가 되면 안 된다 — {ink}/{total} 칸이 켜졌다: {art:?}"
        );
    }

    #[test]
    fn an_unreadable_image_is_not_drawn_at_all() {
        assert!(render(&[0u8, 1, 2, 3], 10, 10).is_none());
    }

    #[test]
    fn placeholder_is_a_box_of_the_width_asked_for() {
        let b = placeholder("그림 1 · PNG 1024×768 · 22KB", 40);
        assert_eq!(b.len(), 3);
        for l in &b {
            assert_eq!(l.width(), 40, "{l:?}");
        }
        assert!(b[1].contains("그림 1"), "{:?}", b[1]);
    }

    #[test]
    fn placeholder_truncates_without_splitting_a_wide_char() {
        let b = placeholder("아주아주아주아주 긴 그림 이름입니다", 16);
        for l in &b {
            assert_eq!(l.width(), 16, "{l:?}");
        }
        assert!(b[1].contains('…'), "넘치면 줄였음을 보여야 한다: {:?}", b[1]);
    }
}

// ─────────────────────────── 색으로 그리기 ───────────────────────────

/// 칸 하나에 담긴 **색 픽셀**.
///
/// 반블록 `▀` 에 글자색·바탕색을 따로 주면 한 칸이 위·아래 두 픽셀이 된다.
/// 터미널 이미지 뷰어(chafa·timg)가 쓰는 방식이고, 1비트 흑백보다 **훨씬**
/// 알아보기 쉽다 — 로고는 모양보다 색으로 가린다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorCell {
    pub upper: [u8; 3],
    pub lower: [u8; 3],
}

/// 색으로 그린 그림.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorArt {
    /// 줄마다 칸들.
    pub rows: Vec<Vec<ColorCell>>,
    pub cols: usize,
    /// 한 칸에 담긴 픽셀의 세로 수 — 2(반블록) 또는 1(공백+바탕색).
    pub per_cell: usize,
}

impl ColorArt {
    /// 담은 색 픽셀 수 — "얼마나 보이는가" 를 수로 말할 때.
    pub fn pixels(&self) -> usize {
        self.rows.len() * self.cols * self.per_cell
    }
}

/// **색으로 그린다.** `max_cols`×`max_rows` 칸 안에 가로비를 지켜 넣는다.
///
/// `half` 가 참이면 한 칸에 위·아래 두 픽셀(`▀`), 거짓이면 한 픽셀(공백 +
/// 바탕색)이다. 거짓으로 쓰는 경우: **넓게 그리는 터미널.** `▀`(U+2580)은
/// East Asian Ambiguous 라 두 칸으로 나오는데 공백은 폭이 보장된다 — 세로
/// 해상도를 절반 잃는 대신 줄이 밀리지 않는다. 선 문자를 ASCII 로 바꾼 것과
/// 같은 판단이다.
pub fn render_color(
    bytes: &[u8],
    max_cols: usize,
    max_rows: usize,
    half: bool,
) -> Option<ColorArt> {
    let img = image::load_from_memory(bytes).ok()?;
    Some(render_color_image(&img, max_cols, max_rows, half))
}

fn render_color_image(
    img: &image::DynamicImage,
    max_cols: usize,
    max_rows: usize,
    half: bool,
) -> ColorArt {
    use image::imageops::FilterType;

    let max_cols = max_cols.max(1);
    let max_rows = max_rows.max(1);
    let per_cell = if half { 2 } else { 1 };
    let (iw, ih) = (img.width().max(1), img.height().max(1));

    // 터미널 칸은 가로보다 세로가 길다(대략 1:2). 한 칸이 세로로 `per_cell`
    // 픽셀을 담으므로, 픽셀 하나의 가로세로비는 `2 / per_cell` 이다.
    let px_w = max_cols as u32;
    let aspect = 2.0 / per_cell as f64;
    let wanted_h = (px_w as f64 * ih as f64 / iw as f64 / aspect).round().max(1.0) as u32;
    let cap = (max_rows * per_cell) as u32;
    let px_h = wanted_h.min(cap).max(1);
    // 칸 경계에 맞춰 올린다 — 반쪽 칸은 그릴 수 없다
    let px_h = px_h.div_ceil(per_cell as u32) * per_cell as u32;

    let small = img.resize_exact(px_w, px_h, FilterType::Triangle);
    let rgba = small.to_rgba8();
    // **흰 종이 위에 얹는다.** 투명한 로고를 검게 깔면 글자가 사라진다 —
    // 1비트 쪽에서 이미 같은 판단을 했다(`to_gray_on_white`).
    let at = |x: u32, y: u32| -> [u8; 3] {
        let p = rgba.get_pixel(x.min(px_w - 1), y.min(px_h - 1));
        let a = p.0[3] as f32 / 255.0;
        let mix = |c: u8| (c as f32 * a + 255.0 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
        [mix(p.0[0]), mix(p.0[1]), mix(p.0[2])]
    };

    let rows_n = (px_h as usize) / per_cell;
    let mut rows = Vec::with_capacity(rows_n);
    for r in 0..rows_n as u32 {
        let mut line = Vec::with_capacity(max_cols);
        for c in 0..px_w {
            let upper = at(c, r * per_cell as u32);
            let lower = if half { at(c, r * 2 + 1) } else { upper };
            line.push(ColorCell { upper, lower });
        }
        rows.push(line);
    }
    ColorArt { rows, cols: px_w as usize, per_cell }
}

#[cfg(test)]
mod color_tests {
    use super::*;

    /// 네 칸을 또렷이 다른 색으로 칠한 그림을 만든다.
    fn quadrants() -> Vec<u8> {
        let mut img = image::RgbImage::new(40, 40);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb(match (x < 20, y < 20) {
                (true, true) => [220, 20, 20],
                (false, true) => [20, 200, 20],
                (true, false) => [20, 20, 220],
                (false, false) => [240, 230, 10],
            });
        }
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("png");
        out
    }

    /// **색이 실제로 서로 다르게 들어오는가.**
    ///
    /// 1비트로 떨구면 네 칸이 전부 켜지거나 전부 꺼진다 — 그게 "화면에서
    /// 거의 구분이 안 된다" 의 원인이었다.
    #[test]
    fn four_colored_quadrants_stay_four_different_colors() {
        let art = render_color(&quadrants(), 20, 20, true).expect("그려야 한다");
        assert!(art.cols > 4 && art.rows.len() > 2, "{}×{}", art.cols, art.rows.len());

        let left = art.rows[0][0].upper;
        let right = art.rows[0][art.cols - 1].upper;
        let bottom_left = art.rows[art.rows.len() - 1][0].lower;
        let bottom_right = art.rows[art.rows.len() - 1][art.cols - 1].lower;
        let four = [left, right, bottom_left, bottom_right];
        for (i, a) in four.iter().enumerate() {
            for b in four.iter().skip(i + 1) {
                let d: i32 = a
                    .iter()
                    .zip(b.iter())
                    .map(|(x, y)| (*x as i32 - *y as i32).abs())
                    .sum();
                assert!(d > 90, "두 구역의 색이 거의 같다: {a:?} vs {b:?} (차이 {d})");
            }
        }
    }

    /// **반쪽 칸은 세로 픽셀이 두 배다.** 그게 색을 쓰는 이유의 절반이다.
    #[test]
    fn half_blocks_carry_twice_the_vertical_pixels() {
        let bytes = quadrants();
        let half = render_color(&bytes, 20, 20, true).expect("반블록");
        let full = render_color(&bytes, 20, 20, false).expect("공백");
        assert_eq!(half.per_cell, 2);
        assert_eq!(full.per_cell, 1);
        assert!(
            half.pixels() > full.pixels(),
            "반블록이 더 많은 픽셀을 담아야 한다 ({} vs {})",
            half.pixels(),
            full.pixels()
        );
        // 같은 칸 수 안에 든다 — 넘치면 표가 어긋난다
        assert!(half.cols <= 20 && full.cols <= 20);
        assert!(half.rows.len() <= 20 && full.rows.len() <= 20);
    }

    /// 투명한 곳은 **흰 종이**다. 검게 깔면 투명 배경 로고의 글자가 사라진다.
    #[test]
    fn transparent_pixels_sit_on_white_paper() {
        let mut img = image::RgbaImage::new(8, 8);
        for p in img.pixels_mut() {
            *p = image::Rgba([0, 0, 0, 0]);
        }
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("png");
        let art = render_color(&bytes, 8, 8, true).expect("그려야 한다");
        assert_eq!(art.rows[0][0].upper, [255, 255, 255], "투명이 검게 깔렸다");
    }
}
