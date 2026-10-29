//! 문서 안의 그림 — 어느 단락에 있고, 어떤 바이트인가.
//!
//! 페이저가 그림을 보여주려면(placeholder 든 점자 든) 두 가지가 필요하다:
//! 그림이 **어느 단락에** 있는지, 그리고 **바이트**가 무엇인지.
//!
//! 백엔드에는 그림을 나열하는 명령이 없다(`capabilities` 의 98개를 훑어 확인했고,
//! `export-svg` 도 `<image>` 를 넣지 않는다). 그래서 컨테이너에서 직접 읽는다.
//!
//! ## HWP5 — 그림 레코드에서 BinData 번호를 읽는다
//!
//! `HWPTAG_SHAPE_COMPONENT_PICTURE`(= `HWPTAG_BEGIN + 69` = 85) 의 payload
//! **71번째 바이트에 BinData 번호(u16)** 가 있다. 스펙을 짐작하지 않고 실측으로
//! 찾았다 — 모든 그림 레코드에서 1~N 범위의 u16 이 나오는 공통 위치를 교집합하면
//! 71 만 남는다. 샘플 3개·그림 20개에서 전부 실재하는 스트림을 가리켰고,
//! s01.hwp 는 번호가 `[1,2,3,4,6,5,7,8,9,10]` 로 뒤섞여 있어 문서 순서가 아닌
//! **진짜 매핑**임을 보여준다.
//!
//! **스트림 이름의 번호는 16진수다** — `BinData/BIN000A` 는 10번이다. 10진수로
//! 읽으면 10개 넘는 그림이 든 문서에서 깨진다.
//!
//! ## HWPX — 참조를 두 번 따라간다
//!
//! `hc:img@binaryItemIDRef` → `Contents/content.hpf` 의 `opf:item@id` →
//! `@href`(= `BinData/…` 경로).
//!
//! ## BinData 스트림은 **개별로 압축되어 있을 수 있다**
//!
//! `BIN0001.bmp` 라는 이름이 붙어 있어도 내용이 BMP 가 아니다 — raw deflate 로
//! 눌려 있다. 실측: s01.hwp 의 그림 10개 중 7개가 BMP 이고 전부 압축돼 있었다
//! (139,551 → 1,950,118 바이트, 풀면 `BM` 서명과 1058×614 24bpp 가 나온다).
//! PNG·JPEG 는 이미 압축된 형식이라 그냥 저장되어 있었고, 그래서 **BMP 만
//! "읽을 수 없는 형식" 으로 보이는** 모양으로 드러났다.
//!
//! DocInfo 의 `BIN_DATA` 레코드에 항목별 압축 플래그가 있지만 그걸 읽지 않는다.
//! 알려진 그림 서명이 아니면 풀어 보고, 풀어서 서명이 맞으면 그걸 쓴다 —
//! 플래그를 잘못 읽어 조용히 틀리는 것보다 결과로 판단하는 쪽이 안전하다.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use crate::witness::Container;

/// 문서에서 찾은 그림 하나.
#[derive(Debug, Clone)]
pub struct DocImage {
    /// 몇 번째 그림인가 (문서 순서, 0부터)
    pub index: usize,
    /// 이 그림이 놓인 본문 단락 — 없으면 표 셀·글상자 안이다
    pub at: Option<(u32, u32)>,
    /// 컨테이너 안의 이름 (`BinData/BIN0002.png`, `BinData/image1.png`)
    pub name: String,
    pub bytes: Vec<u8>,
    /// **문서에 놓인 크기** (HWPUNIT, 1pt = 100). 픽셀 크기가 아니다 — 문서는
    /// 그림을 늘리거나 줄여서 놓는다. 화면에 몇 칸으로 그릴지는 이것으로 정한다.
    pub placed: Option<(u32, u32)>,
    /// 글자처럼 취급되는가. 참이면 본문 줄에 섞인 작은 아이콘이다 —
    /// 블록으로 크게 그리면 문서 모양이 완전히 달라진다.
    pub inline: bool,
    /// 가로 정렬.
    pub align: Align,
    /// **표 칸 안의 그림이면 그 칸.** 코퍼스 그림 43개 중 23개가 여기 해당한다.
    ///
    /// 표 아래에 쌓아 그리면 자리가 완전히 틀린다 — s00.hwp 2쪽의 로고 두 개는
    /// 1행 2열 표의 두 칸에 들어 있어서 **문서에서는 같은 줄 양끝**이다.
    pub cell: Option<ImageCell>,
}

/// 그림이 들어 있는 표 칸.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageCell {
    /// 문서 안의 표 번호 (0부터). 백엔드 `export-tables` 의 `index` 와 같은
    /// 번호다 — 레코드 순서로 세고, 백엔드도 그 순서로 센다.
    pub table: u32,
    pub row: usize,
    pub col: usize,
}

/// 그림의 가로 정렬.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl DocImage {
    /// 문서에 놓인 폭이 본문 폭의 몇 분의 몇인가. 모르면 `None`.
    ///
    /// `body_width` 는 같은 단위(HWPUNIT)여야 한다.
    pub fn width_ratio(&self, body_width: u32) -> Option<f64> {
        let (w, _) = self.placed?;
        (body_width > 0 && w > 0).then(|| (w as f64 / body_width as f64).min(1.0))
    }

    /// 주어진 단 폭에서 몇 칸으로 그릴지.
    pub fn cells_wide(&self, body_width: u32, column_cells: usize) -> usize {
        match self.width_ratio(body_width) {
            Some(r) => ((r * column_cells as f64).round() as usize).clamp(1, column_cells),
            // 모르면 단 폭 전체 — 지금까지 늘 그랬다
            None => column_cells,
        }
    }
}

impl DocImage {
    /// 바이트에서 읽은 형식과 픽셀 크기. 읽지 못하면 `None`.
    ///
    /// 화면에 뭘 적을지는 **파일이 말하는 것**으로 정한다. 문서가 적어 둔 표시
    /// 크기(HWPUNIT)보다 이게 언제나 있고 언제나 맞다.
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        image::load_from_memory(&self.bytes)
            .ok()
            .map(|i| (i.width(), i.height()))
    }

    /// `PNG` · `JPEG` 같은 짧은 이름.
    pub fn format(&self) -> String {
        image::guess_format(&self.bytes)
            .map(|f| format!("{f:?}").to_uppercase())
            .unwrap_or_else(|_| {
                Path::new(&self.name)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .to_uppercase()
            })
    }

    /// 문서에 놓인 크기를 pt 로.
    pub fn placed_pt(&self) -> Option<(f64, f64)> {
        self.placed.map(|(w, h)| (w as f64 / 100.0, h as f64 / 100.0))
    }

    /// 사람에게 보여 줄 한 줄 — placeholder 에 쓴다.
    pub fn label(&self) -> String {
        let placed = match self.placed_pt() {
            Some((w, h)) => format!(" · 문서 {w:.0}×{h:.0}pt"),
            None => String::new(),
        };
        match self.dimensions() {
            Some((w, h)) => format!(
                "그림 {} · {} {}×{}{placed} · {}",
                self.index + 1,
                self.format(),
                w,
                h,
                human_bytes(self.bytes.len())
            ),
            None => format!(
                "그림 {} · {} · {} (읽을 수 없는 형식)",
                self.index + 1,
                self.format(),
                human_bytes(self.bytes.len())
            ),
        }
    }
}

/// 알려진 그림 서명인가.
fn looks_like_image(b: &[u8]) -> bool {
    b.starts_with(b"\x89PNG")            // PNG
        || b.starts_with(&[0xFF, 0xD8])   // JPEG
        || b.starts_with(b"BM")           // BMP
        || b.starts_with(b"GIF8")         // GIF
        || (b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP")
        || b.starts_with(b"II*\0")        // TIFF (little endian)
        || b.starts_with(b"MM\0*")        // TIFF (big endian)
}

/// 컨테이너에서 꺼낸 바이트를 쓸 수 있는 그림 바이트로.
///
/// 서명이 맞으면 그대로, 아니면 raw deflate 로 풀어 본다. 풀어도 서명이 아니면
/// 원래 바이트를 돌려준다 — 우리가 모르는 형식일 수 있고, 그때는 placeholder 가
/// "읽을 수 없는 형식" 이라고 정직하게 말한다.
fn decode_bindata(raw: Vec<u8>) -> Vec<u8> {
    if looks_like_image(&raw) {
        return raw;
    }
    match inflate(&raw) {
        Ok(out) if looks_like_image(&out) => out,
        _ => raw,
    }
}

fn human_bytes(n: usize) -> String {
    if n >= 1 << 20 {
        format!("{:.1}MB", n as f64 / (1 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{}KB", n / (1 << 10))
    } else {
        format!("{n}B")
    }
}

/// 문서의 그림을 전부 읽는다.
pub fn read(path: &Path) -> Result<Vec<DocImage>> {
    match crate::witness::sniff_container(path)? {
        Container::Cfb => read_cfb(path),
        Container::Zip => read_zip(path),
    }
}

/// 좌표별로 몇 개가 있는지 — 페이저가 그릴 자리를 찾는 데 쓴다.
pub fn by_paragraph(images: &[DocImage]) -> HashMap<(u32, u32), Vec<usize>> {
    let mut map: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, img) in images.iter().enumerate() {
        if let Some(at) = img.at {
            map.entry(at).or_default().push(i);
        }
    }
    map
}

// ---- HWP5 ----

const HWPTAG_BEGIN: u32 = 0x10;
const HWPTAG_PARA_HEADER: u32 = HWPTAG_BEGIN + 50;
/// 실측으로 확인: `HWPTAG_BEGIN + 69`. 처음에 +75 로 짐작했다가 레코드가
/// 하나도 안 잡혀서 태그 분포를 세어 바로잡았다.
const HWPTAG_SHAPE_COMPONENT_PICTURE: u32 = HWPTAG_BEGIN + 69;
const HWPTAG_CTRL_HEADER: u32 = HWPTAG_BEGIN + 55;
const HWPTAG_LIST_HEADER: u32 = HWPTAG_BEGIN + 56;
const HWPTAG_TABLE: u32 = HWPTAG_BEGIN + 61;
/// payload 안에서 BinData 번호(u16)가 있는 자리.
const BIN_ID_OFFSET: usize = 71;

/// `CTRL_HEADER` 의 **개체 공통 속성**에서 놓인 크기와 자리를 읽는다.
///
/// payload: `[0..4]` ctrl id(리틀엔디언이라 `gso ` 가 `" osg"` 로 보인다),
/// `[4..8]` **속성**, `[8..12]` 세로 오프셋, `[12..16]` 가로 오프셋,
/// `[16..20]` **너비**, `[20..24]` **높이** (HWPUNIT).
///
/// 속성 비트는 **지어내지 않고 만들어서 확인했다.** s03.hwpx 의 `hp:pos` 를
/// 한 속성만 바꿔 네 벌로 만들고(`treatAsChar` 0/1, `horzAlign`
/// LEFT/CENTER/RIGHT) `rhwp convert` 로 HWP5 로 되돌린 뒤 비트를 읽었다.
/// 바꾼 속성 하나당 비트밭 하나만 움직였다:
///
/// | 만든 것 | attr | bit 0 | bits 10–12 |
/// |---|---|---|---|
/// | treatAsChar=0 LEFT | `0x042a2210` | 0 | 0 |
/// | treatAsChar=0 CENTER | `0x042a2610` | 0 | 1 |
/// | treatAsChar=0 RIGHT | `0x042a2a10` | 0 | 2 |
/// | treatAsChar=1 LEFT | `0x042a2211` | 1 | 0 |
///
/// 그래서 **bit 0 = 글자처럼 취급**, **bits 10–12 = 가로 정렬**(0 왼쪽,
/// 1 가운데, 2 오른쪽)이다. 이 코퍼스의 HWP5 그림 20개는 전부 비트가
/// `글자처럼=1, 가로정렬=0` 이어서 코퍼스만으로는 이 해석을 시험할 수 없었다.
///
/// 크기 쪽은 두 번 확인했다. 비율: s00.hwp 그림 2 의 `10536×2655` 는 비율
/// 3.97 로 픽셀 크기 `12564×3166` 의 3.97 과 같다. 그리고 rhwp 의 조판기가
/// 내놓는 `export-render-tree` 의 bbox(96dpi px)와 맞춰 봤다 — s00 의 그림
/// 95×20pt → 127.0×27.0px(계산 126.7×26.7), 105×27pt → 140.5×35.4px,
/// 415×32pt → 553.8×42.9px. 우리가 읽은 값과 조판 결과가 1% 안에서 같다.
struct Placement {
    size: Option<(u32, u32)>,
    inline: bool,
    align: Align,
}

fn placement(payload: &[u8]) -> Option<Placement> {
    if payload.len() < 24 || &payload[0..4] != b" osg" {
        return None;
    }
    let attr = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let w = u32::from_le_bytes([payload[16], payload[17], payload[18], payload[19]]);
    let h = u32::from_le_bytes([payload[20], payload[21], payload[22], payload[23]]);
    Some(Placement {
        size: (w > 0 && h > 0).then_some((w, h)),
        inline: attr & 1 != 0,
        align: match (attr >> 10) & 0b111 {
            1 => Align::Center,
            2 => Align::Right,
            _ => Align::Left,
        },
    })
}

fn read_cfb(path: &Path) -> Result<Vec<DocImage>> {
    let mut comp = cfb::open(path).with_context(|| format!("{} 열기 실패", path.display()))?;

    // BinData 스트림 목록. 번호는 **16진수**다.
    let mut bins: HashMap<u16, String> = HashMap::new();
    let names: Vec<String> = comp
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_string_lossy().trim_start_matches('/').to_string())
        .collect();
    for name in &names {
        if let Some(rest) = name.strip_prefix("BinData/BIN") {
            // **바이트로 자르지 않는다.** `rest.len() >= 4` 는 바이트 수이고
            // `&rest[..4]` 도 바이트라서, 스트림 이름에 ASCII 가 아닌 글자가
            // 들어오면 글자 중간을 잘라 **터진다.** 문서는 신뢰할 수 없는
            // 입력이므로 이름이 멀쩡하다고 가정하지 않는다. 16진수 네 자리는
            // 어차피 ASCII 이므로 바이트로 보고 검사한다.
            let hex = rest.as_bytes();
            if hex.len() >= 4 && hex[..4].iter().all(|b| b.is_ascii_hexdigit()) {
                if let Ok(id) = u16::from_str_radix(&rest[..4], 16) {
                    bins.insert(id, name.clone());
                }
            }
        }
    }

    let compressed = {
        let mut buf = Vec::new();
        comp.open_stream("/FileHeader")
            .context("FileHeader 가 없다")?
            .read_to_end(&mut buf)?;
        buf.len() >= 40 && (u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]) & 1) != 0
    };

    let mut sections: Vec<String> = names
        .iter()
        .filter(|p| p.starts_with("BodyText/Section"))
        .cloned()
        .collect();
    sections.sort_by_key(|p| {
        p.trim_start_matches("BodyText/Section")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });

    // 그림 레코드를 훑으며 지금 열려 있는 본문 단락에 붙인다.
    struct Found {
        at: Option<(u32, u32)>,
        id: u16,
        placed: Option<(u32, u32)>,
        inline: bool,
        align: Align,
        cell: Option<ImageCell>,
    }

    /// 열려 있는 표 하나. `level` 은 `TABLE`·`LIST_HEADER` 가 놓인 레코드
    /// 레벨이고, 셀 안의 단락은 그보다 깊다.
    struct TableCtx {
        level: u16,
        table: u32,
        rows: usize,
        cols: usize,
        cell: Option<(usize, usize)>,
    }

    let mut found: Vec<Found> = Vec::new();
    // 표 번호는 구역을 넘어 이어진다 — 백엔드가 그렇게 센다
    let mut next_table: u32 = 0;
    for (sec_no, s) in sections.iter().enumerate() {
        let mut raw = Vec::new();
        comp.open_stream(format!("/{s}"))?.read_to_end(&mut raw)?;
        let plain = if compressed { inflate(&raw)? } else { raw };

        let records = walk_records(&plain);
        let mut body_index: i64 = -1;
        let mut open: Vec<TableCtx> = Vec::new();
        for (i, (tag, level, payload)) in records.iter().enumerate() {
            // 레벨이 얕아졌으면 표에서 빠져나온 것이다
            while open.last().is_some_and(|c| *level < c.level) {
                open.pop();
            }
            if *tag == HWPTAG_TABLE && payload.len() >= 8 {
                while open.last().is_some_and(|c| c.level >= *level) {
                    open.pop();
                }
                open.push(TableCtx {
                    level: *level,
                    table: next_table,
                    rows: u16::from_le_bytes([payload[4], payload[5]]) as usize,
                    cols: u16::from_le_bytes([payload[6], payload[7]]) as usize,
                    cell: None,
                });
                next_table += 1;
            } else if *tag == HWPTAG_LIST_HEADER && payload.len() >= 16 {
                if let Some(c) = open.last_mut() {
                    if c.level == *level {
                        let col = u16::from_le_bytes([payload[8], payload[9]]) as usize;
                        let row = u16::from_le_bytes([payload[10], payload[11]]) as usize;
                        // 표 밖의 리스트(글상자 등)도 LIST_HEADER 다 — 좌표가
                        // 표 범위를 벗어나면 그 표의 셀이 아니다 (증인과 같은 규칙)
                        c.cell = (row < c.rows && col < c.cols).then_some((row, col));
                    }
                }
            }
            if *tag == HWPTAG_PARA_HEADER && *level == 0 {
                body_index += 1;
            } else if *tag == HWPTAG_SHAPE_COMPONENT_PICTURE
                && payload.len() >= BIN_ID_OFFSET + 2
            {
                let id = u16::from_le_bytes([payload[BIN_ID_OFFSET], payload[BIN_ID_OFFSET + 1]]);
                let at = (body_index >= 0).then(|| (sec_no as u32, body_index as u32));
                // 이 그림을 감싸는 개체의 머리 — **레벨이 더 낮은** 가장 가까운
                // 앞 CTRL_HEADER 다. 레벨을 안 보면 묶음 개체의 머리를 잡는다.
                let head = records[..i]
                    .iter()
                    .rev()
                    .find(|(t, l, _)| *t == HWPTAG_CTRL_HEADER && l < level)
                    .and_then(|(_, _, p)| placement(p));
                let cell = open.last().and_then(|c| {
                    c.cell.map(|(row, col)| ImageCell { table: c.table, row, col })
                });
                found.push(Found {
                    at,
                    id,
                    placed: head.as_ref().and_then(|h| h.size),
                    inline: head.as_ref().is_some_and(|h| h.inline),
                    align: head.as_ref().map_or(Align::Left, |h| h.align),
                    cell,
                });
            }
        }
    }

    let mut out = Vec::new();
    for (index, f) in found.into_iter().enumerate() {
        let Some(name) = bins.get(&f.id) else { continue };
        let mut raw = Vec::new();
        comp.open_stream(format!("/{name}"))
            .with_context(|| format!("{name} 열기 실패"))?
            .read_to_end(&mut raw)?;
        out.push(DocImage {
            index,
            at: f.at,
            name: name.clone(),
            bytes: decode_bindata(raw),
            placed: f.placed,
            inline: f.inline,
            align: f.align,
            cell: f.cell,
        });
    }
    Ok(out)
}

fn inflate(data: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::DeflateDecoder;
    let mut out = Vec::new();
    DeflateDecoder::new(data)
        .read_to_end(&mut out)
        .context("스트림 해제 실패")?;
    Ok(out)
}

fn walk_records(buf: &[u8]) -> Vec<(u32, u16, Vec<u8>)> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 4 <= buf.len() {
        let header = u32::from_le_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]);
        p += 4;
        let tag = header & 0x3FF;
        let level = ((header >> 10) & 0x3FF) as u16;
        let mut size = ((header >> 20) & 0xFFF) as usize;
        if size == 0xFFF {
            if p + 4 > buf.len() {
                break;
            }
            size = u32::from_le_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]) as usize;
            p += 4;
        }
        if p + size > buf.len() {
            break;
        }
        out.push((tag, level, buf[p..p + size].to_vec()));
        p += size;
    }
    out
}

// ---- HWPX ----

fn read_zip(path: &Path) -> Result<Vec<DocImage>> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)
        .with_context(|| format!("{} 를 ZIP 으로 열 수 없다", path.display()))?;

    let entries: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().and_then(|f| f.name().ok().map(|n| n.to_string())))
        .collect();

    // content.hpf: id → BinData 경로
    let mut manifest: HashMap<String, String> = HashMap::new();
    if entries.iter().any(|e| e == "Contents/content.hpf") {
        let mut hpf = String::new();
        zip.by_name("Contents/content.hpf")?.read_to_string(&mut hpf)?;
        manifest = parse_manifest(&hpf);
    }

    let mut sections: Vec<String> = entries
        .iter()
        .filter(|e| e.starts_with("Contents/section") && e.ends_with(".xml"))
        .cloned()
        .collect();
    sections.sort();

    let mut refs: Vec<PictureRef> = Vec::new();
    // 표 번호는 구역을 넘어 이어진다 — 백엔드가 그렇게 센다
    let mut next_table: u32 = 0;
    for (sec_no, s) in sections.iter().enumerate() {
        let mut xml = String::new();
        zip.by_name(s)?.read_to_string(&mut xml)?;
        refs.extend(picture_refs(&xml, sec_no as u32, &mut next_table)?);
    }

    let mut out = Vec::new();
    for (index, r) in refs.into_iter().enumerate() {
        // 매니페스트에 없으면 이름으로 짐작하지 않고 건너뛴다 — 틀린 그림을
        // 보여주는 것이 안 보여주는 것보다 나쁘다.
        let Some(name) = manifest.get(&r.id) else { continue };
        let Ok(mut f) = zip.by_name(name) else { continue };
        let mut raw = Vec::new();
        f.read_to_end(&mut raw)?;
        // HWPX 는 ZIP 이 이미 풀어 주지만, 안쪽이 또 눌려 있는 문서도 있을 수
        // 있으니 같은 판단을 거친다.
        out.push(DocImage {
            index,
            at: r.at,
            name: name.clone(),
            bytes: decode_bindata(raw),
            placed: r.placed,
            inline: r.inline,
            align: r.align,
            cell: r.cell,
        });
    }
    Ok(out)
}

/// HWPX 에서 읽은 그림 참조 하나.
struct PictureRef {
    at: Option<(u32, u32)>,
    id: String,
    placed: Option<(u32, u32)>,
    inline: bool,
    align: Align,
    cell: Option<ImageCell>,
}

/// `<opf:item id="image1" href="BinData/image1.png" …/>` 들을 모은다.
fn parse_manifest(hpf: &str) -> HashMap<String, String> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(hpf);
    let mut out = HashMap::new();
    loop {
        let ev = match reader.read_event() {
            Ok(e) => e,
            Err(_) => break,
        };
        let tag = match &ev {
            Event::Empty(e) | Event::Start(e) => e.name().as_ref().to_string(),
            Event::Eof => break,
            _ => continue,
        };
        if !tag.ends_with("item") {
            continue;
        }
        if let Event::Empty(e) | Event::Start(e) = &ev {
            let mut id = None;
            let mut href = None;
            for a in e.attributes().flatten() {
                match a.key.as_ref() {
                    "id" => id = Some(a.value.to_string()),
                    "href" => href = Some(a.value.to_string()),
                    _ => {}
                }
            }
            if let (Some(id), Some(href)) = (id, href) {
                if href.starts_with("BinData/") {
                    out.insert(id, href);
                }
            }
        }
    }
    out
}

/// `hp:pic` 하나하나에서 참조·놓인 크기·자리를 읽는다.
///
/// **놓인 크기 = `hp:orgSz` × 그 그림의 `hp:renderingInfo` 안 `hc:scaMatrix`
/// 전부의 곱**(가로 `e1`, 세로 `e5`). 형제 `hp:sz` 를 읽는 쪽이 간단해 보이지만
/// `hp:sz` 는 **맨 바깥 개체에만** 있고, 그림이 묶음(`hp:container`) 안에 있으면
/// 그림 자신에는 없다 — s14.hwpx 의 그림 15개 중 8개가 그렇게 빈칸으로 나왔다.
/// 그리고 묶음에 둘 이상이 들어 있으면 `hp:sz` 는 묶음 전체의 크기여서 각 그림의
/// 크기가 아니다.
///
/// 배율 쪽은 겹친 묶음 수만큼 행렬이 쌓이고, 그 곱이 바깥 크기를 정확히 되돌린다.
/// s14.hwpx 로 실측:
///
/// | 겹 | orgSz 가로 | scaMatrix e1 곱 | 곱한 값 | 바깥 `hp:sz` |
/// |---|---|---|---|---|
/// | 0 | 92580 | 0.503392 | 46604 | 46604 |
/// | 1 | 134160 | 1 × 0.347376 | 46606 | 46604 |
/// | 2 | 96960 | 0.975269 × 1.025337 × 0.497009 | 48189 | 48189 |
///
/// `hp:curSz` 는 쓰지 않는다. 겹친 경우 0 으로 비어 있는 문서가 실제로 있고
/// (s14.hwpx 뒤쪽 4개), 그때도 orgSz × 배율은 맞는 값을 돌려준다.
///
/// `hp:pos` 는 맨 바깥 개체에만 있으므로 그 개체 안에서 만든 그림 전부에
/// 되돌려 적용한다. `treatAsChar="1"` 이면 글자처럼 줄에 섞이는 아이콘이다 —
/// 실측: p.hwpx 의 그림은 본문 폭의 **2%**(11×33pt)이고 전부 `treatAsChar=1`
/// 이다. 그런 것을 블록으로 크게 그리면 문서가 전혀 달라 보인다.
fn picture_refs(xml: &str, section: u32, next_table: &mut u32) -> Result<Vec<PictureRef>> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    fn attr(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
        e.attributes()
            .flatten()
            .find(|a| a.key.as_ref() == name)
            .map(|a| a.value.to_string())
    }
    fn num(e: &quick_xml::events::BytesStart, name: &str) -> Option<f64> {
        attr(e, name).and_then(|v| v.parse().ok())
    }

    /// 조립 중인 그림 하나. 크기는 마지막에 orgSz × 배율로 접는다.
    struct Building {
        r: PictureRef,
        org: Option<(f64, f64)>,
        scale: (f64, f64),
        /// 맨 바깥 그림이면 자기 `hp:sz` 를 들고 있다. 배율 계산이 비면 쓴다.
        sz: Option<(u32, u32)>,
    }

    /// 열려 있는 표 하나 (중첩될 수 있다).
    ///
    /// `hp:cellAddr` 는 칸의 **내용 뒤에** 온다(`hp:tc` → `hp:subList` →
    /// 단락들 → `hp:cellAddr`). 그래서 그림을 만들 때는 좌표를 아직 모르고,
    /// 좌표가 오면 그 칸이 열린 뒤 만들어진 그림들에 되돌려 찍는다. 앞에서
    /// 찍어 두려다 s14.hwpx 의 칸 안 그림 9개를 전부 놓쳤다.
    struct TableCtx {
        table: u32,
        /// 지금 열린 칸이 시작될 때의 `out` 길이
        cell_start: usize,
    }

    let mut reader = Reader::from_str(xml);
    let mut elements: Vec<String> = Vec::new();
    let mut body_index: i64 = -1;
    let mut open: Vec<TableCtx> = Vec::new();
    let mut out: Vec<Building> = Vec::new();
    // 지금 열려 있는 hp:pic 이 out 의 몇 번째인지
    let mut open_pic: Option<usize> = None;
    // 그리기개체 겹 수, 그리고 맨 바깥 개체가 시작될 때의 out 길이
    let mut draw_depth: usize = 0;
    let mut obj_start: usize = 0;

    loop {
        let ev = reader.read_event().context("OWPML 파싱 실패")?;
        let (name, is_empty) = match &ev {
            Event::Start(e) => (e.name().as_ref().to_string(), false),
            Event::Empty(e) => (e.name().as_ref().to_string(), true),
            Event::End(_) => {
                let closed = elements.pop();
                if closed.as_deref() == Some("hp:tbl") {
                    open.pop();
                }
                match closed.as_deref() {
                    Some("hp:pic") => {
                        open_pic = None;
                        draw_depth = draw_depth.saturating_sub(1);
                    }
                    Some("hp:container") => draw_depth = draw_depth.saturating_sub(1),
                    _ => {}
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };

        // 본문 단락은 직계 부모가 hs:sec 인 hp:p 다 (witness 와 같은 규칙)
        if name == "hp:p" && elements.last().map(|s| s.as_str()) == Some("hs:sec") {
            body_index += 1;
        }

        if let Event::Start(e) | Event::Empty(e) = &ev {
            match name.as_str() {
                "hp:tbl" => {
                    if !is_empty {
                        open.push(TableCtx { table: *next_table, cell_start: out.len() });
                    }
                    *next_table += 1;
                }
                "hp:tc" => {
                    if let Some(c) = open.last_mut() {
                        c.cell_start = out.len();
                    }
                }
                "hp:cellAddr" => {
                    if let Some(c) = open.last() {
                        let n = |k: &str| attr(e, k).and_then(|v| v.parse().ok()).unwrap_or(0);
                        let at = ImageCell { table: c.table, row: n("rowAddr"), col: n("colAddr") };
                        let from = c.cell_start.min(out.len());
                        for b in out[from..].iter_mut() {
                            // 이미 찍혀 있으면 더 안쪽 표의 칸이다 — 그쪽이 맞다
                            if b.r.cell.is_none() {
                                b.r.cell = Some(at);
                            }
                        }
                    }
                }
                "hp:container" | "hp:pic" => {
                    if draw_depth == 0 {
                        obj_start = out.len();
                    }
                    if name == "hp:pic" {
                        out.push(Building {
                            r: PictureRef {
                                at: (body_index >= 0).then(|| (section, body_index as u32)),
                                id: String::new(),
                                placed: None,
                                inline: false,
                                align: Align::Left,
                                cell: None,
                            },
                            org: None,
                            scale: (1.0, 1.0),
                            sz: None,
                        });
                        if !is_empty {
                            open_pic = Some(out.len() - 1);
                        }

                    }
                    if !is_empty {
                        draw_depth += 1;
                    }
                }
                "hc:img" => {
                    if let (Some(i), Some(v)) = (open_pic, attr(e, "binaryItemIDRef")) {
                        out[i].r.id = v;
                    }
                }
                "hp:sz" => {
                    if let Some(i) = open_pic {
                        if let (Some(w), Some(h)) = (num(e, "width"), num(e, "height")) {
                            if w >= 1.0 && h >= 1.0 {
                                out[i].sz = Some((w.round() as u32, h.round() as u32));
                            }
                        }
                    }
                }
                "hp:orgSz" => {
                    if let Some(i) = open_pic {
                        if let (Some(w), Some(h)) = (num(e, "width"), num(e, "height")) {
                            out[i].org = Some((w, h));
                        }
                    }
                }
                "hc:scaMatrix" => {
                    if let Some(i) = open_pic {
                        // e1 가로, e5 세로. 겹친 묶음마다 하나씩 쌓인다.
                        // **부호는 뒤집기(flip)다 — 크기가 아니다.** p.hwpx 는
                        // e1 = -0.172642 이고 바깥 hp:sz 는 1098 로 양수다.
                        let sx = num(e, "e1").unwrap_or(1.0).abs();
                        let sy = num(e, "e5").unwrap_or(1.0).abs();
                        out[i].scale.0 *= sx;
                        out[i].scale.1 *= sy;
                    }
                }
                "hp:pos" => {
                    // 맨 바깥 개체에만 있다 — 그 안에서 만든 그림 전부에 적용한다
                    let inline = attr(e, "treatAsChar").as_deref() == Some("1");
                    let align = match attr(e, "horzAlign").as_deref() {
                        Some("CENTER") => Align::Center,
                        Some("RIGHT") => Align::Right,
                        _ => Align::Left,
                    };
                    let from = obj_start.min(out.len());
                    for b in out[from..].iter_mut() {
                        b.r.inline = inline;
                        b.r.align = align;
                    }
                }
                _ => {}
            }
        }

        if !is_empty {
            elements.push(name);
        }
    }

    let mut refs: Vec<PictureRef> = Vec::new();
    for b in out {
        // 그림을 못 찾은 참조는 버린다
        if b.r.id.is_empty() {
            continue;
        }
        let mut r = b.r;
        if let Some((w, h)) = b.org {
            let (pw, ph) = (w * b.scale.0, h * b.scale.1);
            if pw >= 1.0 && ph >= 1.0 {
                r.placed = Some((pw.round() as u32, ph.round() as u32));
            }
        }
        // 배율로 못 구했으면 맨 바깥 그림의 hp:sz 라도 쓴다
        r.placed = r.placed.or(b.sz);
        refs.push(r);
    }
    Ok(refs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `placement` 의 비트 해석을 **실측값으로** 못 박는다.
    ///
    /// 이 네 attr 값은 짐작이 아니다. s03.hwpx 의 `hp:pos` 를 한 속성만 바꿔
    /// 네 벌로 만들고 `rhwp convert` 로 HWP5 로 되돌려 읽은 값이다. 바꾼
    /// 속성 하나당 비트밭 하나만 움직였고, 그래서 bit 0 = 글자처럼 취급,
    /// bits 10–12 = 가로 정렬이라고 말할 수 있다.
    #[test]
    fn gso_attribute_bits_match_what_hancom_wrote() {
        fn head(attr: u32) -> Placement {
            let mut p = Vec::new();
            p.extend_from_slice(b" osg");
            p.extend_from_slice(&attr.to_le_bytes());
            p.extend_from_slice(&0i32.to_le_bytes()); // 세로 오프셋
            p.extend_from_slice(&0i32.to_le_bytes()); // 가로 오프셋
            p.extend_from_slice(&8281u32.to_le_bytes()); // 너비
            p.extend_from_slice(&1885u32.to_le_bytes()); // 높이
            placement(&p).expect("gso 머리")
        }

        let left = head(0x042a_2210);
        assert!(!left.inline);
        assert_eq!(left.align, Align::Left);
        assert_eq!(left.size, Some((8281, 1885)));

        let center = head(0x042a_2610);
        assert_eq!(center.align, Align::Center);
        assert!(!center.inline);

        let right = head(0x042a_2a10);
        assert_eq!(right.align, Align::Right);
        assert!(!right.inline);

        // 같은 문서를 treatAsChar=1 로만 바꾼 것 — 정렬 비트는 그대로다
        let as_char = head(0x042a_2211);
        assert!(as_char.inline);
        assert_eq!(as_char.align, Align::Left);
    }

    /// `gso ` 가 아닌 컨트롤 머리는 크기를 말하지 않는다 — 조용히 0 을 쓰지 않고 거절한다.
    #[test]
    fn placement_rejects_other_controls() {
        let mut p = Vec::new();
        p.extend_from_slice(b" lbt"); // "tbl " — 표
        p.extend_from_slice(&[0u8; 20]);
        assert!(placement(&p).is_none());
        // 너무 짧은 payload 도 거절한다
        assert!(placement(b" osg").is_none());
    }

    #[test]
    fn manifest_maps_ids_to_bindata_paths() {
        let hpf = r#"<opf:package><opf:manifest>
            <opf:item id="header" href="Contents/header.xml" media-type="application/xml"/>
            <opf:item id="image1" href="BinData/image1.png" media-type="image/png"/>
            <opf:item id="image2" href="BinData/image2.bmp" media-type="image/bmp"/>
        </opf:manifest></opf:package>"#;
        let m = parse_manifest(hpf);
        assert_eq!(m.get("image1").map(String::as_str), Some("BinData/image1.png"));
        assert_eq!(m.get("image2").map(String::as_str), Some("BinData/image2.bmp"));
        // BinData 가 아닌 항목은 담지 않는다
        assert!(!m.contains_key("header"));
    }

    #[test]
    fn a_compressed_bindata_stream_is_inflated() {
        use std::io::Write;
        // BMP 서명을 가진 바이트를 raw deflate 로 눌러 둔다 — HWP5 가 실제로
        // 하는 그대로.
        let plain = b"BM\x00\x01\x02\x03 pretend this is a bitmap".to_vec();
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(&plain).unwrap();
        let squeezed = enc.finish().unwrap();

        assert!(!looks_like_image(&squeezed), "눌린 바이트는 서명이 아니다");
        assert_eq!(decode_bindata(squeezed), plain, "풀어서 써야 한다");
    }

    #[test]
    fn already_usable_bytes_are_left_alone() {
        let png = b"\x89PNG\r\n\x1a\n and then some".to_vec();
        assert_eq!(decode_bindata(png.clone()), png);
    }

    #[test]
    fn an_unknown_format_is_returned_as_is_not_mangled() {
        // 풀 수도 없고 서명도 아니면 원래 바이트를 그대로 — label 이
        // "읽을 수 없는 형식" 이라고 말하게 둔다.
        let junk = vec![0x11, 0x22, 0x33, 0x44, 0x55];
        assert_eq!(decode_bindata(junk.clone()), junk);
    }

    #[test]
    fn bytes_are_small_so_say_so_readably() {
        assert_eq!(human_bytes(164), "164B");
        assert_eq!(human_bytes(23_078), "22KB");
        assert_eq!(human_bytes(914_369), "892KB"); // 1MB 를 넘지 않는다
        assert_eq!(human_bytes(1_950_118), "1.9MB"); // 압축 풀린 BMP 쯤 되면 MB
    }

    #[test]
    fn a_label_says_what_it_could_read_and_admits_what_it_could_not() {
        let broken = DocImage {
            index: 0,
            at: Some((0, 3)),
            name: "BinData/BIN0001.png".into(),
            bytes: vec![0, 1, 2, 3],
            placed: None,
            inline: false,
            align: Align::Left,
            cell: None,
        };
        let label = broken.label();
        assert!(label.contains("그림 1"), "{label}");
        assert!(label.contains("읽을 수 없는 형식"), "{label}");
    }
}
