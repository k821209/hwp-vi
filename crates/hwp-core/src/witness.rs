//! 독립 증인 — rhwp 가 쓴 결과를 rhwp 아닌 코드로 센다.
//!
//! PLAN.md §2-6: 저장은 자가검증을 통과해야 한다. 그런데 쓰기를 한 쪽이 그
//! 쓰기를 검증하면 같은 버그를 두 번 통과한다. 그래서 여기서 컨테이너를 직접
//! 열어 **구조만** 센다 — 스트림 목록, 단락 텍스트, 바이너리 blob.
//!
//! 이것은 파서가 아니다. 서식도 조판도 해석하지 않는다. "저장 전과 후에 무엇이
//! 몇 개이고 어디가 달라졌나"에만 답한다. 그 질문에 답하는 데 필요한 만큼만 읽는다.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

/// 증인이 읽은 표 하나. 좌표가 rhwp `export-tables` 와 같음을 실측으로 확인했다
/// (s16.hwp 표0 의 (1,1) 이 양쪽에서 같은 글자).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitnessTable {
    /// 문서 안의 표 번호 (0부터) — rhwp `--table` 과 같은 번호
    pub index: u32,
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<WitnessCell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitnessCell {
    pub row: usize,
    pub col: usize,
    pub row_span: usize,
    pub col_span: usize,
    pub text: String,
    /// **셀 폭·높이 (HWPUNIT).** 문서가 적어 둔 값이다 — 내용 길이가 아니다.
    ///
    /// 화면의 표 폭을 내용에서 짐작하면 문서와 다른 표가 나온다("실제 문서의
    /// 표 크기와 상동하지 않는다" 는 보고, 2026-10-12). 조판값은 표의 **전체**
    /// 폭만 주고(`dump-pages` 의 `table.widthPx`) 열별 폭은 주지 않으므로,
    /// 증인이 `LIST_HEADER` payload 에서 직접 읽는다.
    pub width: u32,
    pub height: u32,
    /// 셀 **첫 단락**의 정렬. 표 안에서는 이것이 다수의 모양이다 — 코퍼스의
    /// 표·글상자 단락 2357개 중 **64%(1518개)가 가운데**이고 양쪽맞춤은 25%뿐이다.
    /// 본문과 반대다(본문은 양쪽맞춤 76%).
    ///
    /// 셀 안 단락이 여럿이면 첫 단락만 본다. 줄마다 정렬이 다른 셀은 드물고,
    /// 셀 하나에 정렬 하나를 두는 쪽이 격자를 그리는 쪽과 맞는다.
    pub align: ParaAlign,
}

impl WitnessTable {
    pub fn cell(&self, row: usize, col: usize) -> Option<&WitnessCell> {
        self.cells.iter().find(|c| c.row == row && c.col == col)
    }
}

/// 한 문서의 구조 지문.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Testimony {
    pub container: Container,
    /// 컨테이너 안의 부품 이름 (CFB 스트림 / ZIP 엔트리), 정렬됨
    pub parts: Vec<String>,
    /// 모든 단락, 본문 순서. 표 셀 안의 단락도 들어 있다 (`depth > 0`).
    pub paragraphs: Vec<Para>,
    /// 표 — 셀 좌표까지. 셀 편집을 독립 검증하려면 좌표가 있어야 한다.
    pub tables: Vec<WitnessTable>,
}

/// 단락 하나.
///
/// `depth == 0` 이 본문 단락이고, **구역 안에서의** 순번이 `index` 다. rhwp 의
/// 편집 좌표가 `(section, para)` 쌍이므로 둘을 같이 들고 다닌다 — 구역이 여러
/// 개인 문서에서 전역 순번을 쓰면 어긋난다(s14.hwpx 로 실측했다).
///
/// 단락 수는 레코드 레벨 0 의 `PARA_HEADER` 개수로 맞췄다 (s13.hwp 65=65,
/// s16.hwp 13=13). 레벨 2 는 표 셀 안의 단락이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Para {
    pub section: u32,
    /// 구역 안에서의 본문 단락 순번. 표 셀 안 단락(`depth > 0`)에는 `None`.
    pub index: Option<u32>,
    pub text: String,
    pub depth: u16,
    /// 이 단락에 든 확장 제어문자의 **종류**.
    ///
    /// 개수만 세면 쓸모가 없다. 구역 정의(2)·쪽번호 위치(21)·머리말(16)은
    /// 본문에 보일 내용이 아니고, **그리기개체/표(11)** 만 사람이 볼 것이다.
    /// s00.hwp 단락 0 은 구역정의 2개 + 쪽번호 1개뿐인데 "개체 있음" 으로
    /// 표시되어 "그림이 있는데 안 보인다" 는 오해를 불렀다.
    pub objects: Vec<u16>,
    /// 이 단락의 **문단서식** — 정렬과 들여쓰기. 읽지 못하면 기본값.
    pub style: ParaStyle,
    /// 글자 꾸밈 구간. 꾸밈이 없는 단락은 빈 목록이다.
    pub runs: Vec<Run>,
}

/// 글자 꾸밈 — **터미널이 그릴 수 있는 넷만.**
///
/// 글자 크기·글꼴·색·외곽선·그림자는 담지 않는다. 터미널에는 한 가지 크기와
/// 한 가지 글꼴만 있고, 색은 문서의 의도를 터미널 팔레트로 옮기는 별개의
/// 문제다. 넷은 ANSI SGR 1·3·4·9 로 그대로 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Deco {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

impl Deco {
    /// 한 바이트로 접는다. 글자마다 하나씩 들고 다니므로 작아야 한다.
    pub fn bits(self) -> u8 {
        (self.bold as u8) | (self.italic as u8) << 1 | (self.underline as u8) << 2
            | (self.strike as u8) << 3
    }

    pub fn from_u8(b: u8) -> Self {
        Self {
            bold: b & 1 != 0,
            italic: b & 2 != 0,
            underline: b & 4 != 0,
            strike: b & 8 != 0,
        }
    }

    pub fn any(self) -> bool {
        self.bold || self.italic || self.underline || self.strike
    }

    /// HWP5 `CHAR_SHAPE` 의 **속성 `UINT32`** 에서 읽는다.
    ///
    /// payload 74바이트 중 **오프셋 46**. 비트는 실측으로 확정했다:
    ///
    /// | 비트 | 뜻 | 확인 |
    /// |---|---|---|
    /// | 0 | 기울임 | 코퍼스 6문서에서 변환본 HWPX 의 `hh:italic` 과 전수 일치 |
    /// | 1 | 진하게 | 같음 (`hh:bold`) |
    /// | 2 | 밑줄 | 같음 (`hh:underline type != NONE`) |
    /// | 19 | 취소선 | **만들어서** 확인 — 그 속성만 켠 HWPX 를 `rhwp convert` 로 되돌렸다 |
    ///
    /// 취소선은 코퍼스에 0개여서 코퍼스로는 찾을 수 없었다. (덤으로 8 외곽선,
    /// 11 그림자 — 담지 않는다.)
    fn from_bits(attr: u32) -> Self {
        Self {
            italic: attr & (1 << 0) != 0,
            bold: attr & (1 << 1) != 0,
            underline: attr & (1 << 2) != 0,
            strike: attr & (1 << 19) != 0,
        }
    }

    /// OWPML `hh:charPr` 에서.
    ///
    /// **`hh:underline`·`hh:strikeout`·`hh:shadow` 는 값이 `NONE` 이어도
    /// 엘리먼트가 늘 있다.** 있는지만 보면 전부 켜진 것으로 읽힌다 — 처음에
    /// 그렇게 재서 그림자가 100% 로 나왔다. `hh:bold`·`hh:italic` 은 반대로
    /// **참일 때만** 나온다.
    fn from_owpml(bold: bool, italic: bool, underline: &str, strikeout: &str) -> Self {
        Self {
            bold,
            italic,
            underline: !underline.is_empty() && underline != "NONE",
            strike: !strikeout.is_empty() && strikeout != "NONE",
        }
    }
}

/// 붙어 있고 꾸밈이 같은 구간을 합치고, **글자 없는 구간을 버린다.**
///
/// 둘 다 두 포맷을 맞추는 데 필요하다. 같은 문서를 HWP5 와 (변환된) HWPX 로
/// 읽어 비교하면서 드러났다:
///
/// - HWPX 는 **글꼴이 달라지면 `hp:run` 을 쪼갠다.** 꾸밈이 같아도 구간이
///   둘이 된다(`B 0+4`, `B 4+30` vs HWP5 의 `B 0+34`). 화면에는 같은 뜻이다.
/// - 그림·필드 같은 **컨트롤 한 글자에 붙은 꾸밈**이 있다. 글자가 아니므로
///   진하게도 뜻이 없다(s00.hwp 단락5 가 `￼` 하나에 `B`).
fn tidy_runs(mut runs: Vec<Run>, text: &str) -> Vec<Run> {
    let chars: Vec<char> = text.chars().collect();
    runs.retain(|r| {
        let end = (r.start + r.len).min(chars.len());
        r.deco.any()
            && r.len > 0
            && r.start < chars.len()
            // 보이는 글자가 하나라도 있어야 한다 (컨트롤만이면 버린다)
            && chars[r.start..end].iter().any(|c| *c != crate::offset::CONTROL)
    });
    runs.sort_by_key(|r| r.start);
    let mut out: Vec<Run> = Vec::new();
    for r in runs {
        match out.last_mut() {
            Some(prev) if prev.deco == r.deco && prev.start + prev.len >= r.start => {
                let end = (r.start + r.len).max(prev.start + prev.len);
                prev.len = end - prev.start;
            }
            _ => out.push(r),
        }
    }
    out
}

/// 문단 안에서 꾸밈이 같은 구간. `start`·`len` 은 **보이는 글자** 기준이다.
///
/// 파일에는 원시 `PARA_TEXT` 의 wchar 위치로 적혀 있고, 컨트롤이 8 wchar 를
/// 차지한다. 실측으로 확정했다: s00.hwp 의 한 단락이 보이는 글자 13개인데
/// 구간 위치가 **20** 이다 — 보이는 글자 기준으로는 나올 수 없는 수다.
/// 읽을 때 보이는 글자 기준으로 옮겨 둔다. 화면·커서가 그 기준이기 때문이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub start: usize,
    pub len: usize,
    pub deco: Deco,
}

/// 문단서식에서 화면에 옮길 수 있는 것만.
///
/// HWP 문단서식에는 왼쪽·오른쪽 여백, 문단 위·아래 간격, 줄 간격, 탭 설정이
/// 더 있다. **일부러 담지 않는다.** 코퍼스 본문 795단락에서 실측한 쓰임:
///
/// | 속성 | 0 아닌 단락 | 값 | 판정 |
/// |---|---|---|---|
/// | 들여쓰기(`intent`) | **63%** | 전부 음수(내어쓰기) | 담는다 |
/// | 문단 위 간격 | 42% | 500·1000·300 | 터미널 줄 모델과 충돌 |
/// | 문단 아래 간격 | 28% | 500·100·300 | 같음 |
/// | 왼쪽 여백 | 4% | 831·600·1330 | 담는다 (들여쓰기 계산에 필요) |
/// | 오른쪽 여백 | 0.6% | 775 뿐 | 버린다 |
///
/// 왼쪽·오른쪽 여백은 쓰는 문단이 거의 없고 값도 잡값이다. 간격은 빈 줄로
/// 그리면 조판기가 준 쪽·줄 수와 어긋난다. 들여쓰기는 **쓰는 쪽이 다수이고**
/// 글머리 아래로 글을 맞추는 바로 그 장치다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParaStyle {
    pub align: ParaAlign,
    /// 첫 줄이 들어가는 양 (HWPUNIT). `left + max(intent, 0)`.
    pub first_indent: i64,
    /// 이어지는 줄이 들어가는 양 (HWPUNIT). `left + max(-intent, 0)` — 내어쓰기.
    pub rest_indent: i64,
}

/// 문단 정렬. 터미널은 양쪽맞춤을 할 수 없어 `Justify`·`Distribute`·`Division`
/// 은 왼쪽으로 그린다 — 버리지 않고 들고 있는 것은 `hwp json` 이 쓰기 때문이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParaAlign {
    #[default]
    Justify,
    Left,
    Right,
    Center,
    Distribute,
    Division,
}

impl ParaAlign {
    /// HWP5 문단서식 속성1 의 **비트 2~4**. s00.hwp 본문 174단락에서 변환본
    /// HWPX 의 `hh:align horizontal` 과 174/174 일치했다.
    fn from_bits(attr: u32) -> Self {
        match (attr >> 2) & 0b111 {
            1 => Self::Left,
            2 => Self::Right,
            3 => Self::Center,
            4 => Self::Distribute,
            5 => Self::Division,
            _ => Self::Justify,
        }
    }

    fn from_owpml(v: &str) -> Self {
        match v {
            "LEFT" => Self::Left,
            "RIGHT" => Self::Right,
            "CENTER" => Self::Center,
            "DISTRIBUTE" => Self::Distribute,
            "DIVISION" => Self::Division,
            _ => Self::Justify,
        }
    }
}

/// CFB `.hwp` 의 `FileHeader` 속성 비트. 열기 전에 **읽을 수 있는 문서인지**
/// 가리는 데 쓴다.
///
/// bit 0 압축 · bit 1 암호 · bit 2 **배포용**. 배포용은 본문이 `ViewText/` 에
/// 구역별로 암호화되어 있고 `BodyText/Section0` 에는 껍데기만 남는다 — 읽으면
/// 오류 없이 1단락이 나온다. 실측: 공개된 한글문서파일형식 5.0 스펙 문서가
/// `flags = 0x00020005`(압축+배포용)이고 백엔드는 70쪽 619단락이라 하는데
/// 우리 증인은 1단락이었다.
pub fn cfb_header_flags(path: &Path) -> Option<u32> {
    let mut comp = cfb::open(path).ok()?;
    let mut buf = Vec::new();
    comp.open_stream("/FileHeader").ok()?.read_to_end(&mut buf).ok()?;
    (buf.len() >= 40).then(|| u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]))
}

/// 그리기개체·표를 담는 확장 제어문자. 사람이 볼 내용은 이것이다.
pub const CTRL_DRAWING_OR_TABLE: u16 = 11;

/// OWPML 에서 **그리기개체**인 요소 이름.
///
/// HWP5 쪽은 `CTRL_HEADER` 의 id 11 하나로 끝나지만, OWPML 은 종류마다 요소
/// 이름이 다르다. 코퍼스에 실제로 나오는 것은 `hp:tbl`·`hp:pic`·`hp:rect`·
/// `hp:container` 이고, 나머지는 같은 자리에 올 수 있는 것들이다.
///
/// 이것이 비어 있던 동안 HWPX 문서는 개체를 **하나도** 세지 않았다. 그래서
/// 여백의 `◻`(못 그린 개체) 표시가 HWPX 에서는 뜨지 않았고, 개체가 달린
/// 단락을 지우는 것을 막는 관문도 HWP5 에서만 걸렸다 (2026-10-10 실측).
const OWPML_DRAWINGS: &[&str] = &[
    "hp:tbl",
    "hp:pic",
    "hp:rect",
    "hp:container",
    "hp:line",
    "hp:ellipse",
    "hp:arc",
    "hp:polygon",
    "hp:curve",
    "hp:connectLine",
    "hp:textart",
    "hp:ole",
    "hp:equation",
    "hp:chart",
    "hp:video",
];

fn is_owpml_drawing(name: &str) -> bool {
    OWPML_DRAWINGS.contains(&name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// HWP 5.x — CFB(OLE2) + 스트림별 raw deflate + 태그 레코드
    Cfb,
    /// HWPX — ZIP(OPC) + OWPML XML
    Zip,
}

/// 두 증언의 차이.
#[derive(Debug, Clone)]
pub struct Difference {
    pub parts_added: Vec<String>,
    pub parts_removed: Vec<String>,
    /// (단락 번호, 이전, 이후) — 길이가 같을 때만 채운다
    pub paragraphs_changed: Vec<(usize, String, String)>,
    pub paragraph_count_before: usize,
    pub paragraph_count_after: usize,
}

impl Difference {
    /// 구조가 보존되었는가 — 부품이 드나들지 않고 단락 수가 같은가.
    pub fn structure_preserved(&self) -> bool {
        self.parts_added.is_empty()
            && self.parts_removed.is_empty()
            && self.paragraph_count_before == self.paragraph_count_after
    }

    pub fn is_identical(&self) -> bool {
        self.structure_preserved() && self.paragraphs_changed.is_empty()
    }
}

/// 컨테이너 종류를 **내용으로** 알아낸다.
///
/// 확장자로 판별하면 `doc.hwp.bak` 같은 파일을 못 읽는다. 백업·임시본은 편집
/// 경로가 늘 만드는 것이므로 거기서 막히면 안 된다.
pub fn sniff_container(path: &Path) -> Result<Container> {
    use std::io::Read;
    let mut head = [0u8; 8];
    let mut f = std::fs::File::open(path)
        .with_context(|| format!("{} 열기 실패", path.display()))?;
    let n = f.read(&mut head)?;
    // CFB(OLE2) 서명
    if n >= 8 && head == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1] {
        return Ok(Container::Cfb);
    }
    // ZIP 서명 (빈 zip·스팬 zip 변종까지)
    if n >= 4 && head[0] == 0x50 && head[1] == 0x4B {
        return Ok(Container::Zip);
    }
    bail!(
        "{} 는 HWP5(CFB) 도 HWPX(ZIP) 도 아니다 — 앞 8바이트가 {:02X?}",
        path.display(),
        &head[..n.min(8)]
    )
}

impl Testimony {
    /// 본문 단락만 (표 셀 제외), 문서 순서.
    pub fn body_paragraphs(&self) -> Vec<&Para> {
        self.paragraphs.iter().filter(|p| p.depth == 0).collect()
    }

    /// rhwp 좌표 `(section, para)` 로 본문 단락을 찾는다.
    pub fn at(&self, section: u32, index: u32) -> Option<&Para> {
        self.paragraphs
            .iter()
            .find(|p| p.depth == 0 && p.section == section && p.index == Some(index))
    }

    /// 표를 번호로 찾는다.
    pub fn table(&self, index: u32) -> Option<&WitnessTable> {
        self.tables.iter().find(|t| t.index == index)
    }

    /// 구역마다 본문 단락이 몇 개인가.
    pub fn section_counts(&self) -> Vec<usize> {
        let mut v = Vec::new();
        for p in self.paragraphs.iter().filter(|p| p.depth == 0) {
            let s = p.section as usize;
            if v.len() <= s {
                v.resize(s + 1, 0);
            }
            v[s] += 1;
        }
        v
    }

    pub fn body_count(&self) -> usize {
        self.paragraphs.iter().filter(|p| p.depth == 0).count()
    }

    pub fn read(path: &Path) -> Result<Self> {
        match sniff_container(path)? {
            Container::Cfb => Self::read_cfb(path),
            Container::Zip => Self::read_zip(path),
        }
    }

    pub fn diff(&self, after: &Testimony) -> Difference {
        let a: BTreeSet<_> = self.parts.iter().cloned().collect();
        let b: BTreeSet<_> = after.parts.iter().cloned().collect();

        let mut changed = Vec::new();
        if self.paragraphs.len() == after.paragraphs.len() {
            for (i, (x, y)) in self.paragraphs.iter().zip(&after.paragraphs).enumerate() {
                if x.text != y.text {
                    changed.push((i, x.text.clone(), y.text.clone()));
                }
            }
        }

        Difference {
            parts_added: b.difference(&a).cloned().collect(),
            parts_removed: a.difference(&b).cloned().collect(),
            paragraphs_changed: changed,
            paragraph_count_before: self.paragraphs.len(),
            paragraph_count_after: after.paragraphs.len(),
        }
    }

    // ---- HWP 5.x (CFB) ----

    fn read_cfb(path: &Path) -> Result<Self> {
        let mut comp = cfb::open(path)
            .with_context(|| format!("{} 를 CFB 로 열 수 없다", path.display()))?;

        let mut parts: Vec<String> = comp
            .walk()
            .filter(|e| e.is_stream())
            .map(|e| e.path().to_string_lossy().trim_start_matches('/').to_string())
            .collect();
        parts.sort();

        let compressed = read_file_header_compressed(&mut comp)?;

        // 문단서식 표 — 본문을 읽기 전에 DocInfo 에서 가져온다
        let shapes: Vec<ParaStyle> = {
            let mut raw = Vec::new();
            comp.open_stream("/DocInfo")
                .context("DocInfo 스트림이 없다")?
                .read_to_end(&mut raw)?;
            let plain = if compressed { inflate_raw(&raw)? } else { raw };
            para_shapes(&plain)
        };
        // 글자서식 표도 같은 스트림에서
        let char_decos: Vec<Deco> = {
            let mut raw = Vec::new();
            comp.open_stream("/DocInfo")
                .context("DocInfo 스트림이 없다")?
                .read_to_end(&mut raw)?;
            let plain = if compressed { inflate_raw(&raw)? } else { raw };
            char_shapes(&plain)
        };

        // BodyText/SectionN 을 번호 순서대로
        let mut sections: Vec<String> = parts
            .iter()
            .filter(|p| p.starts_with("BodyText/Section"))
            .cloned()
            .collect();
        sections.sort_by_key(|p| {
            p.trim_start_matches("BodyText/Section")
                .parse::<u32>()
                .unwrap_or(u32::MAX)
        });

        let mut paragraphs = Vec::new();
        for (sec_no, s) in sections.iter().enumerate() {
            let mut raw = Vec::new();
            comp.open_stream(format!("/{s}"))
                .with_context(|| format!("{s} 스트림 열기 실패"))?
                .read_to_end(&mut raw)?;
            let plain = if compressed { inflate_raw(&raw)? } else { raw };
            paragraphs.extend(number_body(
                paras_from_records(&plain, &shapes, &char_decos),
                sec_no as u32,
            ));
        }

        let mut tables = Vec::new();
        let mut next_index = 0u32;
        for s in &sections {
            let mut raw = Vec::new();
            comp.open_stream(format!("/{s}"))?.read_to_end(&mut raw)?;
            let plain = if compressed { inflate_raw(&raw)? } else { raw };
            tables.extend(tables_from_records(&plain, &mut next_index, &shapes));
        }

        Ok(Self { container: Container::Cfb, parts, paragraphs, tables })
    }

    // ---- HWPX (ZIP + OWPML) ----

    fn read_zip(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("{} 열기 실패", path.display()))?;
        let mut zip = zip::ZipArchive::new(file)
            .with_context(|| format!("{} 를 ZIP 으로 열 수 없다", path.display()))?;

        let mut parts: Vec<String> = (0..zip.len())
            .filter_map(|i| zip.by_index(i).ok().and_then(|f| f.name().ok().map(|n| n.to_string())))
            .collect();
        parts.sort();

        let mut sections: Vec<String> = parts
            .iter()
            .filter(|p| p.starts_with("Contents/section") && p.ends_with(".xml"))
            .cloned()
            .collect();
        sections.sort();

        // 문단서식 표 — 본문을 읽기 전에 header.xml 에서 가져온다
        let styles = {
            let mut header = String::new();
            if parts.iter().any(|p| p == "Contents/header.xml") {
                zip.by_name("Contents/header.xml")?.read_to_string(&mut header)?;
            }
            para_styles_from_header(&header)
        };
        let char_decos = {
            let mut header = String::new();
            if parts.iter().any(|p| p == "Contents/header.xml") {
                zip.by_name("Contents/header.xml")?.read_to_string(&mut header)?;
            }
            char_decos_from_header(&header)
        };

        let mut paragraphs = Vec::new();
        for (sec_no, s) in sections.iter().enumerate() {
            let mut xml = String::new();
            zip.by_name(s)
                .with_context(|| format!("{s} 읽기 실패"))?
                .read_to_string(&mut xml)?;
            paragraphs.extend(number_body(
                paras_from_owpml(&xml, &styles, &char_decos)?,
                sec_no as u32,
            ));
        }

        let mut tables = Vec::new();
        let mut next_index = 0u32;
        for s in &sections {
            let mut xml = String::new();
            zip.by_name(s)?.read_to_string(&mut xml)?;
            tables.extend(tables_from_owpml(&xml, &mut next_index, &styles)?);
        }

        Ok(Self { container: Container::Zip, parts, paragraphs, tables })
    }
}

/// 한 구역의 단락들에 구역 번호와 본문 순번을 매긴다.
fn number_body(mut paras: Vec<Para>, section: u32) -> Vec<Para> {
    let mut next = 0u32;
    for p in &mut paras {
        p.section = section;
        if p.depth == 0 {
            p.index = Some(next);
            next += 1;
        }
    }
    paras
}

/// `&ref;` 를 글자로 되돌린다.
///
/// quick-xml 0.42 는 엔티티를 `Event::Text` 가 아니라 **별도 이벤트**
/// `Event::GeneralRef` 로 내보낸다. 그걸 무시하면 글자가 조용히 사라진다 —
/// `R&D` 가 `RD` 가 됐고, 백엔드와 셀 내용을 대조하다가 비로소 드러났다.
/// 독립 대조가 우리 쪽 버그를 잡은 사례다.
/// 글자를 들고 있는 빈 엘리먼트 → 그 글자.
///
/// OWPML 은 공백·탭·줄바꿈을 텍스트가 아니라 엘리먼트로 적는다. 무시하면 글자가
/// 사라진다 — `소속 〔…〕` 이 `소속〔…〕` 이 됐다. 대응은 백엔드를 기준으로
/// 맞췄다(`fwSpace` → U+2007 FIGURE SPACE 는 rhwp 가 쓰는 값이다).
///
/// **읽는 쪽에서는 모르는 엘리먼트를 거부하지 않고 글자만 챙긴다.** 화이트리스트로
/// 받은 것만 통과시키면, 평범한 엘리먼트 하나(`fwSpace`·`tab`·`lineBreak`·
/// `titleMark` 같은 것)에 걸려 문서 전체를 못 읽거나 — 더 나쁘게 — 그 부분만
/// 조용히 빠뜨린다. 실제로 그렇게 막히는 판독기를 보고 정한 방침이다.
fn inline_char(tag: &str) -> Option<char> {
    match tag {
        "hp:fwSpace" => Some('\u{2007}'),
        "hp:tab" => Some('\t'),
        "hp:lineBreak" => Some('\n'),
        "hp:nbSpace" => Some('\u{00A0}'),
        _ => None,
    }
}

fn resolve_entity(name: &str) -> Option<String> {
    // 숫자 참조
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse::<u32>().ok()?,
        };
        return char::from_u32(code).map(String::from);
    }
    quick_xml::escape::resolve_predefined_entity(name).map(String::from)
}

/// `FileHeader` 의 flags 비트 0 = 압축 여부.
fn read_file_header_compressed<F: Read + std::io::Seek>(
    comp: &mut cfb::CompoundFile<F>,
) -> Result<bool> {
    let mut buf = Vec::new();
    comp.open_stream("/FileHeader")
        .context("FileHeader 스트림이 없다 — HWP 5.x 가 아니다")?
        .read_to_end(&mut buf)?;
    if buf.len() < 40 {
        bail!("FileHeader 가 너무 짧다 ({} 바이트)", buf.len());
    }
    if &buf[..17] != b"HWP Document File" {
        bail!("FileHeader 서명이 'HWP Document File' 이 아니다");
    }
    let flags = u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]);
    if flags & 0x2 != 0 {
        bail!("암호 문서다 — 증인은 열지 않는다");
    }
    Ok(flags & 0x1 != 0)
}

fn inflate_raw(data: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::DeflateDecoder;
    let mut out = Vec::new();
    DeflateDecoder::new(data)
        .read_to_end(&mut out)
        .context("스트림 raw deflate 해제 실패")?;
    Ok(out)
}

/// 표 레코드에서 표와 셀을 읽는다.
///
/// 구조: `TABLE` 레코드가 표의 시작이고 행·열 수를 들고 있다. 그 뒤로 셀마다
/// `LIST_HEADER` 가 오고 그 payload 에 `(col, row, colSpan, rowSpan)` 이 있으며,
/// 다음 `LIST_HEADER`/`TABLE` 까지의 `PARA_TEXT` 가 그 셀의 본문이다.
///
/// rhwp `export-tables` 와 좌표가 같음을 실측으로 확인했다.
fn tables_from_records(
    buf: &[u8],
    next_index: &mut u32,
    shapes: &[ParaStyle],
) -> Vec<WitnessTable> {
    let records = collect_records(buf);
    let mut out: Vec<WitnessTable> = Vec::new();

    // 표마다 그 `TABLE` 레코드의 중첩 수준. **칸은 표보다 정확히 한 단계
    // 아래의 `LIST_HEADER` 다.**
    //
    // 수준을 안 보면 **칸 안 글상자의 글자 목록이 칸으로 잡힌다.** 실측
    // (s14 를 변환한 .hwp 표17): 진짜 칸은 수준 2 의 `LIST_HEADER` 인데,
    // 글상자 안의 목록이 수준 6 에 또 있고 거기도 `(행 0, 열 0)` 이라 1×1
    // 표의 좌표 검사를 통과해 **칸을 `③` 으로 덮었다.**
    // `(out 안의 색인, 그 TABLE 레코드의 수준)` 을 **스택**으로 든다.
    //
    // 실측(s00.hwp): `TABLE` 과 그 칸들의 `LIST_HEADER` 는 **같은 수준**이다
    // (둘 다 2). 한 단계 아래일 거라 짐작했다가 시험 넷을 깼다 — `PARA_HEADER`
    // 가 `LIST_HEADER` 와 형제인 것과 같은 꼴이다. 이 포맷은 "담는 것" 과
    // "담기는 것" 을 수준으로 가르지 않는 자리가 있다.
    //
    // `last()` 만으로는 안 된다 — 중첩 표가 끝난 뒤 **바깥 표의 칸이 또
    // 나온다.** 그때 마지막 표는 중첩된 쪽이라 그 칸들이 버려진다.
    let mut stack: Vec<(usize, u16)> = Vec::new();

    for (i, (tag, lvl, payload)) in records.iter().enumerate() {
        match *tag {
            HWPTAG_TABLE => {
                if payload.len() < 8 {
                    continue;
                }
                let rows = u16::from_le_bytes([payload[4], payload[5]]) as usize;
                let cols = u16::from_le_bytes([payload[6], payload[7]]) as usize;
                out.push(WitnessTable {
                    index: *next_index,
                    rows,
                    cols,
                    cells: Vec::new(),
                });
                // 이 수준 이상으로 열려 있던 표는 끝났다
                while stack.last().is_some_and(|(_, l)| *l >= *lvl) {
                    stack.pop();
                }
                stack.push((out.len() - 1, *lvl));
                *next_index += 1;
            }
            HWPTAG_LIST_HEADER => {
                // **표보다 한 단계 아래가 아니면 이 표의 칸이 아니다.**
                // 같은 수준에 열려 있는 가장 안쪽 표의 칸이다. 그런 표가
                // 없으면 이건 표의 칸이 아니라 **다른 것**(글상자 안의 글자
                // 목록 등)이다 — 실측: s14 의 글상자 목록이 수준 6 에 있는데
                // 그 수준에 열린 표가 없다.
                let Some(parent) = stack
                    .iter()
                    .rev()
                    .find(|(_, l)| l == lvl)
                    .map(|(idx, _)| *idx)
                else {
                    continue;
                };
                let Some(table) = out.get_mut(parent) else { continue };
                if payload.len() < 16 {
                    continue;
                }
                let col = u16::from_le_bytes([payload[8], payload[9]]) as usize;
                let row = u16::from_le_bytes([payload[10], payload[11]]) as usize;
                let col_span = u16::from_le_bytes([payload[12], payload[13]]) as usize;
                let row_span = u16::from_le_bytes([payload[14], payload[15]]) as usize;
                // 명세상 그 뒤가 셀 폭·높이(HWPUNIT). 없으면 0 — 쓰는 쪽이
                // 0 을 "모른다" 로 받는다.
                let u32at = |i: usize| -> u32 {
                    if payload.len() >= i + 4 {
                        u32::from_le_bytes([
                            payload[i],
                            payload[i + 1],
                            payload[i + 2],
                            payload[i + 3],
                        ])
                    } else {
                        0
                    }
                };
                let width = u32at(16);
                let height = u32at(20);
                // 표 밖의 리스트(글상자 등)도 LIST_HEADER 다 — 좌표가 표 범위를
                // 벗어나면 이 표의 셀이 아니다.
                if row >= table.rows || col >= table.cols {
                    continue;
                }
                // 셀 안에 단락이 여럿이면 줄바꿈으로 잇는다 — rhwp
                // `export-tables` 가 그렇게 주고, 전수 대조로 맞췄다.
                // 셀이 몇 단락인지는 `LIST_HEADER` 앞 4바이트가 말한다. 그 수를
                // 지켜야 한다 — 다음 `LIST_HEADER` 까지 긁으면 **표 밖의 본문
                // 단락까지 셀 안으로 끌려 들어온다**(s16.hwp 에서 실제로 그랬다).
                //
                // 단락 경계는 `PARA_HEADER` 다. `PARA_TEXT` 만 세면 빈 단락을
                // 놓쳐 줄바꿈이 모자라게 된다 — 본문에서 이미 당한 현상이다(§8-2).
                let n_para = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]])
                    .max(0) as usize;
                let mut parts: Vec<String> = Vec::new();
                let mut align = ParaAlign::default();
                // **칸보다 깊은 레코드는 칸의 글자가 아니다.**
                //
                // 레코드 헤더에 중첩 수준이 있는데 이 루프가 `_` 로 버리고
                // 있었다. 그래서 칸 안에 **글상자**가 있으면 그 안의 단락
                // 글자가 칸으로 끌려 들어왔다 — 실측: s14 를 변환한 .hwp 의
                // 표17 (0,0) 이 `③` 으로 읽혔다(백엔드와 우리 OWPML 판독기는
                // 둘 다 빈 칸이라고 한다).
                //
                // 보기가 이상한 데서 끝나지 않는다. `edit::snapshot` 이 이
                // 값을 쓰므로 그런 칸은 `set-cell` 의 기대값이 영원히 어긋나
                // **저장이 되지 않는다.**
                let cell_level = records[i].1;
                // 실측한 수준(s00.hwp 표0): `LIST_HEADER` 와 칸의 `PARA_HEADER`
                // 가 **같은 수준**이고 `PARA_TEXT` 가 한 단계 아래다.
                //
                //     LIST_HEADER   level 2   ← 칸
                //     PARA_HEADER   level 2   ← 칸의 단락 (형제다!)
                //     PARA_TEXT     level 3
                //
                // 칸 안 글상자는 그 아래로 한 벌 더 들어간다(CTRL_HEADER →
                // SHAPE_COMPONENT → LIST_HEADER level 4 → PARA_HEADER level 4 →
                // PARA_TEXT level 5). 그래서 `cell_level + 1` 보다 깊은 것을
                // 건너뛰면 개체 안의 글자가 칸으로 들어오지 않는다.
                for (tag2, lvl2, pl2) in &records[i + 1..] {
                    if *lvl2 > cell_level + 1 {
                        continue;
                    }
                    match *tag2 {
                        HWPTAG_PARA_HEADER if *lvl2 == cell_level => {
                            if parts.len() >= n_para {
                                break;
                            }
                            // 첫 단락의 정렬만 쓴다
                            if parts.is_empty() && pl2.len() >= 10 {
                                let id = u16::from_le_bytes([pl2[8], pl2[9]]) as usize;
                                if let Some(st) = shapes.get(id) {
                                    align = st.align;
                                }
                            }
                            parts.push(String::new());
                        }
                        HWPTAG_PARA_TEXT if *lvl2 == cell_level + 1 => {
                            let text = decode_para_text(pl2);
                            if let Some(last) = parts.last_mut() {
                                *last = text;
                            }
                        }
                        HWPTAG_LIST_HEADER | HWPTAG_TABLE => break,
                        _ => {}
                    }
                }
                // 떼고 잇는 규칙은 `join_cell_paragraphs` 하나에 있다 — OWPML
                // 쪽도 같은 함수를 쓴다. 인라인 컨트롤은 빼는데, 그건 글자가
                // 아니고 `--offset` 도 세지 않기 때문이다(`crate::offset`).
                let text: String = join_cell_paragraphs(
                    parts
                        .into_iter()
                        .map(|s| s.chars().filter(|c| *c != crate::offset::CONTROL).collect())
                        .collect(),
                );
                table.cells.push(WitnessCell {
                    row,
                    col,
                    width,
                    height,
                    row_span: row_span.max(1),
                    col_span: col_span.max(1),
                    text,
                    align,
                });
            }
            _ => {}
        }
    }
    out
}

/// 레코드를 한 번에 모은다 — 표를 읽을 때 앞뒤를 봐야 하기 때문이다.
fn collect_records(buf: &[u8]) -> Vec<(u32, u16, Vec<u8>)> {
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

const HWPTAG_BEGIN: u32 = 0x10;
const HWPTAG_PARA_HEADER: u32 = HWPTAG_BEGIN + 50;
const HWPTAG_PARA_SHAPE: u32 = HWPTAG_BEGIN + 9;
const HWPTAG_CHAR_SHAPE: u32 = HWPTAG_BEGIN + 5;
const HWPTAG_PARA_CHAR_SHAPE: u32 = HWPTAG_BEGIN + 52;

/// `DocInfo` 의 글자서식 표 — `PARA_CHAR_SHAPE` 의 번호가 이 배열을 가리킨다.
fn char_shapes(doc_info: &[u8]) -> Vec<Deco> {
    collect_records(doc_info)
        .into_iter()
        .filter(|(tag, _, payload)| *tag == HWPTAG_CHAR_SHAPE && payload.len() >= 50)
        .map(|(_, _, payload)| {
            let attr =
                u32::from_le_bytes([payload[46], payload[47], payload[48], payload[49]]);
            Deco::from_bits(attr)
        })
        .collect()
}

/// `DocInfo` 의 문단서식 표. `PARA_HEADER` 의 `paraShapeId` 가 이 배열을 가리킨다.
///
/// payload: `[0..4]` 속성1, `[4..8]` 왼쪽 여백, `[8..12]` 오른쪽 여백,
/// `[12..16]` **들여쓰기**, `[16..20]` 문단 위 간격, `[20..24]` 아래 간격,
/// `[24..28]` 줄 간격. 전부 `INT32` 이고 음수가 온다.
///
/// **단위는 HWPUNIT 의 절반이다.** 실측으로 확정했다: s00.hwp 의
/// `PARA_SHAPE[245]` 가 들여쓰기 `-3506` 을 들고 있고, 그 서식을 쓰는 단락
/// 105·107 의 둘째 줄이 조판기 렌더트리에서 본문+**17.6pt** 에 그려진다
/// (`3506/200 = 17.5`, `3506/100 = 35.1` 이 아니다). 변환된 HWPX 의
/// `hh:margin` 값도 정확히 절반(`-1753`)이다 — 두 출력이 같은 말을 한다.
/// 그래서 읽는 즉시 2로 나눠 HWPUNIT 으로 맞춘다.
fn para_shapes(doc_info: &[u8]) -> Vec<ParaStyle> {
    let mut out = Vec::new();
    for (tag, _lvl, payload) in collect_records(doc_info) {
        if tag != HWPTAG_PARA_SHAPE || payload.len() < 16 {
            continue;
        }
        let i32_at = |o: usize| -> i64 {
            i32::from_le_bytes([payload[o], payload[o + 1], payload[o + 2], payload[o + 3]]) as i64
        };
        let attr = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let left = i32_at(4);
        let intent = i32_at(12);
        out.push(ParaStyle {
            align: ParaAlign::from_bits(attr),
            // 저장 단위가 HWPUNIT 의 절반이다 (위 문서 주석의 실측)
            first_indent: (left + intent.max(0)) / 2,
            rest_indent: (left + (-intent).max(0)) / 2,
        });
    }
    out
}
const HWPTAG_PARA_TEXT: u32 = HWPTAG_BEGIN + 51;
const HWPTAG_LIST_HEADER: u32 = HWPTAG_BEGIN + 56;
const HWPTAG_TABLE: u32 = HWPTAG_BEGIN + 61;

/// 태그 레코드 스트림을 훑어 단락을 센다.
///
/// `PARA_HEADER` 가 단락의 시작이고 레코드 헤더의 `level` 이 중첩 깊이다.
/// `PARA_TEXT` 는 바로 앞 단락의 본문이며, 빈 단락에는 아예 없다 — 그래서
/// `PARA_TEXT` 를 세면 빈 단락을 놓친다. rhwp 좌표와 어긋나는 원인이 이것이었다.
fn paras_from_records(buf: &[u8], shapes: &[ParaStyle], chars: &[Deco]) -> Vec<Para> {
    let mut out: Vec<Para> = Vec::new();
    // 방금 읽은 `PARA_TEXT` 의 보이는 글자별 원시 wchar 위치. 바로 뒤에 오는
    // `PARA_CHAR_SHAPE` 가 이걸로 구간을 옮긴다.
    let mut raw_pos: Vec<usize> = Vec::new();
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
        match tag {
            HWPTAG_PARA_HEADER => {
                // **단락마다 비운다.** 빈 단락은 `PARA_TEXT` 레코드를 아예
                // 내지 않으면서 `PARA_CHAR_SHAPE` 는 낸다(s13.hwp 단락1).
                // 안 비우면 앞 단락의 위치표를 물려받아, 빈 단락에 11자
                // 구간이 붙는다 — 실제로 그렇게 나왔다.
                raw_pos.clear();
                // paraShapeId 는 payload `[8..10]`. 서식 표를 못 읽었거나
                // 범위를 벗어나면 기본값 — 지어내지 않는다.
                let style = (size >= 10)
                    .then(|| u16::from_le_bytes([buf[p + 8], buf[p + 9]]) as usize)
                    .and_then(|id| shapes.get(id).copied())
                    .unwrap_or_default();
                out.push(Para {
                    section: 0,
                    index: None,
                    text: String::new(),
                    depth: level,
                    objects: Vec::new(),
                    style,
                    runs: Vec::new(),
                })
            }
            HWPTAG_PARA_TEXT => {
                let codes = extended_codes(&buf[p..p + size]);
                let text = decode_para_text(&buf[p..p + size]);
                raw_pos = raw_positions(&buf[p..p + size]);
                match out.last_mut() {
                    Some(last) => {
                        last.objects = codes;
                        last.text = text;
                    }
                    // PARA_HEADER 없이 PARA_TEXT 가 먼저 나오는 일은 없어야 하지만,
                    // 깨진 문서를 만나도 증인은 멈추지 않는다.
                    None => out.push(Para {
                        section: 0,
                        index: None,
                        text,
                        depth: level,
                        objects: Vec::new(),
                        style: ParaStyle::default(),
                        runs: Vec::new(),
                    }),
                }
            }
            HWPTAG_PARA_CHAR_SHAPE => {
                if let Some(last) = out.last_mut() {
                    let runs = runs_from_char_shape(&buf[p..p + size], &raw_pos, chars);
                    last.runs = tidy_runs(runs, &last.text);
                }
            }
            _ => {}
        }
        p += size;
    }
    out
}

/// `PARA_TEXT` 에 든 확장 제어문자의 종류를 순서대로.
///
/// 확장 제어문자는 8 wchar 를 차지하고 **코드가 앞뒤에 한 번씩** 적혀 있다.
/// 선두 코드만 읽고 16바이트를 건너뛰면 하나로 센다.
fn extended_codes(payload: &[u8]) -> Vec<u16> {
    const EXTENDED: [u16; 13] = [1, 2, 3, 11, 12, 14, 15, 16, 17, 18, 21, 22, 23];
    const INLINE: [u16; 8] = [4, 5, 6, 7, 8, 9, 19, 20];
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 2 <= payload.len() {
        let c = u16::from_le_bytes([payload[i], payload[i + 1]]);
        if EXTENDED.contains(&c) {
            out.push(c);
            i += 16;
        } else if INLINE.contains(&c) {
            i += 16;
        } else {
            i += 2;
        }
    }
    out
}

/// `PARA_TEXT` 는 UTF-16LE 에 인라인 제어문자가 섞여 있고, 확장 제어문자는
/// 8 wchar(16바이트)를 차지한다. 텍스트만 뽑고 컨트롤은 자리표시자로 둔다.
///
/// **서러게이트 쌍을 합쳐야 한다.** BMP 밖 문자(기호 글꼴의 사용자 영역, 이모지,
/// 확장 한자)는 UTF-16 에서 두 칸을 쓴다. 따로 읽으면 `char::from_u32` 가 짝
/// 없는 서러게이트를 거부해 **글자가 조용히 사라진다** — 백엔드가 `󰁾` 를 내는데
/// 우리는 빈 문자열을 내던 것이 그 때문이었다.
fn decode_para_text(payload: &[u8]) -> String {
    // HWP5 의 제어문자는 세 종류이고 **차지하는 길이가 다르다**.
    //   확장(1,2,3,11,12,14~18,21~23) 8 wchar — 그림·표 같은 개체
    //   인라인(4~9,19,20)          8 wchar — 탭, 필드 시작/끝 등
    //   단독(0,10,13,24~31)        1 wchar — 10 강제 줄바꿈(글자다), 13 단락 끝
    // 인라인을 1 wchar 로 건너뛰면 그 payload 를 글자로 읽는다. 실제로
    // `1,200,000,000원` 뒤에 `浵ࡦ` 같은 쓰레기가 붙었고, 백엔드와 대조해서
    // 비로소 드러났다.
    const EXTENDED: [u16; 13] = [1, 2, 3, 11, 12, 14, 15, 16, 17, 18, 21, 22, 23];
    const INLINE: [u16; 8] = [4, 5, 6, 7, 8, 9, 19, 20];
    let mut s = String::new();
    let mut i = 0usize;
    while i + 2 <= payload.len() {
        let c = u16::from_le_bytes([payload[i], payload[i + 1]]);
        if EXTENDED.contains(&c) {
            s.push('\u{FFFC}'); // OBJECT REPLACEMENT CHARACTER
            i += 16;
            continue;
        }
        if INLINE.contains(&c) {
            if c == 9 {
                s.push('\t'); // 탭은 글자로 보인다
            }
            i += 16;
            continue;
        }
        // 30·31 은 고정폭 빈칸·묶음 빈칸 — 글자다. 버리면 `※ 본` 이 `※본` 이
        // 된다. 백엔드는 이것을 U+2007 FIGURE SPACE 로 내고, HWPX 의
        // `hp:fwSpace` 와 같은 글자를 쓴다 — 두 포맷의 같은 개념이므로 맞춘다.
        // 나머지 단독 컨트롤(0,13,24~29)은 버린다 — 10 은 아래에서 줄바꿈으로 챈다.
        if c == 30 || c == 31 {
            s.push('\u{2007}');
            i += 2;
            continue;
        }
        // **단독 컨트롤 10 은 강제 줄바꿈이고 글자다.** 버리면 안 된다.
        //
        // OWPML 쪽은 `hp:lineBreak` 를 `'\n'` 으로 읽는데(아래 `inline_char`)
        // HWP5 쪽에서 버리고 있었다 — **같은 개념을 두 판독기가 다르게 읽은 것**
        // 이고, U+2007(컨트롤 30·31 ↔ `hp:fwSpace`) 때와 같은 모양이다.
        //
        // 글자 하나가 빠지는 데서 끝나지 않는다. `--offset` 은 이 글자를
        // **센다**(실측: 줄바꿈이 64번째인 단락의 65번째에 넣으면 줄바꿈 바로
        // 뒤에 들어간다). 그래서 버리는 동안 강제 줄바꿈이 있는 `.hwp` 의
        // 모든 오프셋이 **하나씩 짧았다** — 편집이 엉뚱한 자리에 들어간다.
        //
        // **자가검증이 이걸 못 잡는다.** 기대값과 재생이 같은 틀린 오프셋을
        // 쓰므로 둘이 일치해 통과한다. 접힌 줄 결함과 같은 종류다.
        //
        // 드러난 경로: 사용자가 "hwp→hwpx 변환 기능이 있나" 고 물어서 변환
        // 충실도를 재다가, HWPX→HWP 왕복에서 본문이 다르게 읽혀 발견했다.
        // 변환은 무손실이었고 **우리 판독기가 손실원**이었다.
        if c == 10 {
            s.push('\n');
            i += 2;
            continue;
        }
        if c < 32 {
            i += 2;
            continue;
        }
        // 상위 서러게이트면 다음 칸과 짝을 맞춘다
        if (0xD800..=0xDBFF).contains(&c) && i + 4 <= payload.len() {
            let low = u16::from_le_bytes([payload[i + 2], payload[i + 3]]);
            if (0xDC00..=0xDFFF).contains(&low) {
                let code = 0x1_0000 + (((c as u32) - 0xD800) << 10) + ((low as u32) - 0xDC00);
                if let Some(ch) = char::from_u32(code) {
                    s.push(ch);
                }
                i += 4;
                continue;
            }
        }
        if let Some(ch) = char::from_u32(c as u32) {
            s.push(ch);
        }
        i += 2;
    }
    s
}

/// [`decode_para_text`] 와 **같은 걸음**으로 걸으며 보이는 글자마다 원시 wchar
/// 위치를 적는다. 꾸밈 구간을 보이는 글자 기준으로 옮기는 데 쓴다.
///
/// 두 함수가 갈라지면 구간이 엉뚱한 글자에 붙는다. **실제로 갈라졌다** —
/// 컨트롤 10 을 `decode_para_text` 에만 글자로 더했더니 줄바꿈 뒤의 구간이 한
/// 칸씩 밀렸다. 글자 수를 보는 코퍼스 시험이 있었지만 **코퍼스의 `.hwp` 에
/// 강제 줄바꿈이 없어서** 못 봤고, 포맷 간 대조(`convert`)가 잡았다.
///
/// 글자로 세는 컨트롤을 더할 때는 **두 함수를 같이** 고칠 것.
fn raw_positions(payload: &[u8]) -> Vec<usize> {
    const EXTENDED: [u16; 13] = [1, 2, 3, 11, 12, 14, 15, 16, 17, 18, 21, 22, 23];
    const INLINE: [u16; 8] = [4, 5, 6, 7, 8, 9, 19, 20];
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut raw = 0usize;
    while i + 2 <= payload.len() {
        let c = u16::from_le_bytes([payload[i], payload[i + 1]]);
        if EXTENDED.contains(&c) {
            out.push(raw);
            raw += 8;
            i += 16;
        } else if INLINE.contains(&c) {
            if c == 9 {
                out.push(raw);
            }
            raw += 8;
            i += 16;
        } else if c == 10 || c == 30 || c == 31 {
            // **10(강제 줄바꿈)·30·31(고정폭·묶음 빈칸)은 글자다.**
            //
            // 10 을 여기 넣지 않아 실제로 갈라졌다: `decode_para_text` 에만
            // 넣었더니 줄바꿈 뒤의 꾸밈 구간이 **한 칸씩 밀렸다**(s04: HWPX 가
            // 119번째 `‘` 에 주는 꾸밈을 HWP5 는 118번째 공백에 줬다). 위
            // 주석이 경고하는 바로 그것이고, 포맷 간 대조가 잡았다.
            out.push(raw);
            raw += 1;
            i += 2;
        } else if c < 32 {
            raw += 1;
            i += 2;
        } else if (0xD800..=0xDBFF).contains(&c)
            && i + 4 <= payload.len()
            && (0xDC00..=0xDFFF).contains(&u16::from_le_bytes([payload[i + 2], payload[i + 3]]))
        {
            out.push(raw);
            raw += 2;
            i += 4;
        } else {
            out.push(raw);
            raw += 1;
            i += 2;
        }
    }
    out
}

/// `PARA_CHAR_SHAPE` 의 `(원시위치, 글자서식번호)` 쌍을 **보이는 글자 기준**
/// 구간으로 옮긴다.
///
/// 꾸밈이 없는 구간만 나오면 빈 목록을 돌려준다 — 23%만 꾸밈이 있으므로
/// 대부분의 단락에서 아무것도 들고 있지 않는 것이 맞다.
fn runs_from_char_shape(payload: &[u8], raw_pos: &[usize], shapes: &[Deco]) -> Vec<Run> {
    let n = payload.len() / 8;
    let visible = raw_pos.len();
    let mut marks: Vec<(usize, Deco)> = Vec::with_capacity(n);
    for k in 0..n {
        let at = k * 8;
        let pos = u32::from_le_bytes([
            payload[at],
            payload[at + 1],
            payload[at + 2],
            payload[at + 3],
        ]) as usize;
        let id = u32::from_le_bytes([
            payload[at + 4],
            payload[at + 5],
            payload[at + 6],
            payload[at + 7],
        ]) as usize;
        // 원시 위치 → 보이는 글자 번째. 그 자리 이전의 보이는 글자 수다.
        let start = raw_pos.partition_point(|r| *r < pos);
        let deco = shapes.get(id).copied().unwrap_or_default();
        marks.push((start.min(visible), deco));
    }
    marks.dedup_by(|a, b| a.1 == b.1);

    let mut out = Vec::new();
    for (i, (start, deco)) in marks.iter().enumerate() {
        let end = marks.get(i + 1).map(|(s, _)| *s).unwrap_or(visible);
        if !deco.any() || end <= *start {
            continue;
        }
        out.push(Run { start: *start, len: end - *start, deco: *deco });
    }
    out
}

/// OWPML 의 `<hp:p>` 를 단락으로 센다.
///
/// 본문 단락의 기준은 **직계 부모가 `<hs:sec>` 인 `<hp:p>`** 다. 실측으로 맞췄다:
/// s14.hwpx 의 섹션 4개에서 그 수가 45+4+16+37 = 102 이고 rhwp 의 문단 수와 같다.
/// 표 셀·글상자(`subList`) 안의 단락은 깊이 1 이상이다.
///
/// `hp:p` 는 중첩된다 — 본문 단락이 표를 품고 그 셀 안에 또 단락이 있다. 그래서
/// 버퍼 하나로는 안 되고 스택이 필요하다(바깥 단락을 잃는다).
/// HWPX `header.xml` 의 문단서식 표 — `paraPrIDRef` → [`ParaStyle`].
///
/// 여백은 `hh:margin` 의 `hc:intent`·`hc:left` 다. **`hp:switch` 를 주의할 것**:
/// 같은 여백이 HWPUNIT 판과 글자 단위 판으로 두 벌 적혀 있고, 우리는
/// `hp:required-namespace` 가 `HwpUnitChar` 인 `hp:case` 쪽(HWPUNIT)을 쓴다.
/// 먼저 나오는 쪽을 그냥 읽으면 단위가 섞인다.
///
/// HWP5 와 달리 **여기 값은 이미 HWPUNIT 이다** — 나누지 않는다.
/// HWPX `header.xml` 의 글자서식 표 — `charPrIDRef` → [`Deco`].
fn char_decos_from_header(xml: &str) -> HashMap<String, Deco> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    let mut out: HashMap<String, Deco> = HashMap::new();
    let mut open: Option<String> = None;
    let (mut bold, mut italic) = (false, false);
    let (mut underline, mut strike) = (String::new(), String::new());

    let attr = |e: &quick_xml::events::BytesStart, name: &str| -> Option<String> {
        e.attributes()
            .flatten()
            .find(|a| a.key.as_ref() == name)
            .map(|a| a.value.to_string())
    };

    loop {
        let ev = match reader.read_event() {
            Ok(e) => e,
            Err(_) => break,
        };
        match &ev {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => match e.name().as_ref() {
                "hh:charPr" => {
                    open = attr(e, "id");
                    bold = false;
                    italic = false;
                    underline.clear();
                    strike.clear();
                }
                "hh:bold" if open.is_some() => bold = true,
                "hh:italic" if open.is_some() => italic = true,
                // **값이 NONE 이어도 엘리먼트는 늘 있다** — 종류를 봐야 한다
                "hh:underline" if open.is_some() => {
                    underline = attr(e, "type").unwrap_or_default();
                }
                "hh:strikeout" if open.is_some() => {
                    strike = attr(e, "shape").unwrap_or_default();
                }
                _ => {}
            },
            Event::End(e) => {
                if e.name().as_ref() == "hh:charPr" {
                    if let Some(id) = open.take() {
                        out.insert(id, Deco::from_owpml(bold, italic, &underline, &strike));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn para_styles_from_header(xml: &str) -> HashMap<String, ParaStyle> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    fn attr(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
        e.attributes()
            .flatten()
            .find(|a| a.key.as_ref() == name)
            .map(|a| a.value.to_string())
    }

    let mut reader = Reader::from_str(xml);
    let mut out: HashMap<String, ParaStyle> = HashMap::new();
    // 지금 읽고 있는 paraPr 의 id, 그리고 HWPUNIT case 안인지
    let mut open: Option<String> = None;
    let mut in_hwpunit = false;
    let mut margin: Option<(i64, i64)> = None; // (left, intent)
    let mut align = ParaAlign::default();
    let mut left = 0i64;
    let mut intent = 0i64;

    loop {
        let ev = match reader.read_event() {
            Ok(e) => e,
            Err(_) => break,
        };
        match &ev {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let name = e.name().as_ref().to_string();
                match name.as_str() {
                    "hh:paraPr" => {
                        open = attr(e, "id");
                        in_hwpunit = false;
                        margin = None;
                        align = ParaAlign::default();
                        left = 0;
                        intent = 0;
                    }
                    "hp:case" => {
                        in_hwpunit = attr(e, "hp:required-namespace")
                            .is_some_and(|v| v.contains("HwpUnitChar"));
                    }
                    "hh:align" => {
                        if open.is_some() {
                            if let Some(v) = attr(e, "horizontal") {
                                align = ParaAlign::from_owpml(&v);
                            }
                        }
                    }
                    "hc:left" if in_hwpunit => {
                        left = attr(e, "value").and_then(|v| v.parse().ok()).unwrap_or(0);
                    }
                    "hc:intent" if in_hwpunit => {
                        intent = attr(e, "value").and_then(|v| v.parse().ok()).unwrap_or(0);
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                let name = e.name().as_ref().to_string();
                match name.as_str() {
                    "hp:case" => {
                        if in_hwpunit {
                            margin = Some((left, intent));
                        }
                        in_hwpunit = false;
                    }
                    "hh:paraPr" => {
                        if let Some(id) = open.take() {
                            let (l, i) = margin.unwrap_or((0, 0));
                            out.insert(
                                id,
                                ParaStyle {
                                    align,
                                    first_indent: l + i.max(0),
                                    rest_indent: l + (-i).max(0),
                                },
                            );
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

fn paras_from_owpml(
    xml: &str,
    styles: &HashMap<String, ParaStyle>,
    decos: &HashMap<String, Deco>,
) -> Result<Vec<Para>> {
    use quick_xml::escape::unescape;
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    let mut elements: Vec<String> = Vec::new();
    let mut open_paras: Vec<usize> = Vec::new();
    // 열려 있는 단락마다 "그 단락 안에서 몇 겹의 개체 안에 있나".
    //
    // 겹을 세는 이유: `hp:container` 는 `hp:rect` 들을 묶고 있어서 전부 세면
    // 한 개체가 열두 개로 보인다. HWP5 는 묶음을 **하나**로 세므로(GSO 컨트롤
    // 하나) 같게 맞춘다. 단락마다 따로 세는 이유: 표 칸 안의 단락에 또 표가
    // 들어 있으면 그 표는 **칸의 단락** 것이다.
    let mut obj_depth: Vec<usize> = Vec::new();
    let mut paras: Vec<Para> = Vec::new();
    let mut text_depth: usize = 0;

    // 부모가 섹션이면 본문 단락(깊이 0), 아니면 중첩(깊이 1).
    let depth_for = |elements: &[String]| -> u16 {
        match elements.last().map(|s| s.as_str()) {
            Some("hs:sec") => 0,
            _ => 1,
        }
    };

    loop {
        match reader.read_event().context("OWPML 파싱 실패")? {
            Event::Start(e) => {
                let name = e.name().as_ref().to_string();
                match name.as_str() {
                    "hp:p" => {
                        let style = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == "paraPrIDRef")
                            .and_then(|a| styles.get(&a.value.to_string()).copied())
                            .unwrap_or_default();
                        paras.push(Para { section: 0, index: None, text: String::new(), depth: depth_for(&elements), objects: Vec::new(), style, runs: Vec::new() });
                        open_paras.push(paras.len() - 1);
                        obj_depth.push(0);
                    }
                    "hp:t" => text_depth += 1,
                    // 꾸밈 구간의 시작 — 지금까지 담긴 글자 수가 그 자리다
                    "hp:run" => {
                        if let Some(&i) = open_paras.last() {
                            let deco = e
                                .attributes()
                                .flatten()
                                .find(|a| a.key.as_ref() == "charPrIDRef")
                                .and_then(|a| decos.get(&a.value.to_string()).copied())
                                .unwrap_or_default();
                            let start = paras[i].text.chars().count();
                            // 길이는 다음 run 이 열릴 때(또는 단락이 닫힐 때)
                            // 채운다. 그때까지 0 으로 둔다.
                            if let Some(last) = paras[i].runs.last_mut() {
                                last.len = start.saturating_sub(last.start);
                            }
                            paras[i].runs.push(Run { start, len: 0, deco });
                        }
                    }
                    other if is_owpml_drawing(other) => {
                        if let (Some(&i), Some(d)) = (open_paras.last(), obj_depth.last_mut()) {
                            if *d == 0 {
                                paras[i].objects.push(CTRL_DRAWING_OR_TABLE);
                            }
                            *d += 1;
                        }
                    }
                    _ => {}
                }
                elements.push(name);
            }
            Event::End(e) => {
                let name = e.name().as_ref().to_string();
                elements.pop();
                if is_owpml_drawing(&name) {
                    if let Some(d) = obj_depth.last_mut() {
                        *d = d.saturating_sub(1);
                    }
                }
                match name.as_str() {
                    "hp:p" => {
                        if let Some(i) = open_paras.last().copied() {
                            let n = paras[i].text.chars().count();
                            if let Some(last) = paras[i].runs.last_mut() {
                                last.len = n.saturating_sub(last.start);
                            }
                            // 꾸밈이 없는 구간은 들고 있을 이유가 없다 —
                            // 코퍼스의 77% 가 그렇다
                            let runs = std::mem::take(&mut paras[i].runs);
                            let text = paras[i].text.clone();
                            paras[i].runs = tidy_runs(runs, &text);
                        }
                        open_paras.pop();
                        obj_depth.pop();
                    }
                    "hp:t" => text_depth = text_depth.saturating_sub(1),
                    _ => {}
                }
            }
            Event::Empty(e) => {
                let name = e.name().as_ref().to_string();
                // <hp:p/> 같은 빈 단락도 하나로 센다 — rhwp 의 paraIndex 가
                // 빈 단락을 세기 때문이다 (§8-2 에서 실측으로 확인했다).
                if name == "hp:p" {
                    let style = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == "paraPrIDRef")
                        .and_then(|a| styles.get(&a.value.to_string()).copied())
                        .unwrap_or_default();
                    paras.push(Para { section: 0, index: None, text: String::new(), depth: depth_for(&elements), objects: Vec::new(), style, runs: Vec::new() });
                } else if is_owpml_drawing(&name) {
                    if let (Some(&i), Some(&d)) = (open_paras.last(), obj_depth.last()) {
                        if d == 0 {
                            paras[i].objects.push(CTRL_DRAWING_OR_TABLE);
                        }
                    }
                } else if let Some(c) = inline_char(&name) {
                    if let Some(&i) = open_paras.last() {
                        paras[i].text.push(c);
                    }
                }
            }
            Event::Text(t) => {
                if text_depth > 0 {
                    if let Some(&i) = open_paras.last() {
                        let raw = t.xml10_content();
                        let text = unescape(&raw).context("텍스트 unescape 실패")?;
                        paras[i].text.push_str(&text);
                    }
                }
            }
            // 엔티티는 따로 온다 — 무시하면 `R&D` 가 `RD` 가 된다.
            Event::GeneralRef(r) => {
                if text_depth > 0 {
                    if let Some(&i) = open_paras.last() {
                        if let Some(c) = resolve_entity(&r) {
                            paras[i].text.push_str(&c);
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(paras)
}

/// OWPML 에서 표와 셀을 읽는다.
///
/// 칸 안 단락 목록 → 칸 글자. **두 판독기가 같은 이 함수를 쓴다.**
///
/// 규칙: **앞뒤의 빈 단락을 떼고**(백엔드가 그렇게 한다) 나머지를 `\n` 으로
/// 잇는다. 가운데 빈 단락은 남긴다 — 서식의 빈 줄이다.
///
/// **떼는 단위가 단락이지 글자가 아니다.** 전에는 HWP5 쪽이 이은 문자열에
/// `trim_matches('\n')` 를 걸었는데, 단독 컨트롤 10(강제 줄바꿈)을 `'\n'` 으로
/// 읽게 된 뒤로는 그 방식이 **칸 끝의 진짜 줄바꿈까지 먹는다.**
///
/// 그리고 OWPML 쪽에는 이 떼기가 **아예 없었다** — 그래서 `.hwp` 를 `.hwpx` 로
/// 바꾸면 같은 칸이 다르게 읽혔다(꼬리에 `\n` 하나). 백엔드는 양쪽을 531자로
/// 같게 보는데 우리만 달랐다. 두 판독기가 같은 개념을 다르게 읽은 네 번째
/// 자리다(U+2007, 강제 줄바꿈, 칸 떼기…).
/// 칸 안 단락 경계를 모으는 동안 쓰는 표식. 문서 글자에는 나올 수 없다
/// (HWP5 는 컨트롤 0 을 버리고 XML 에는 NUL 을 담을 수 없다).
const CELL_PARA_MARK: char = '\u{0}';

pub(crate) fn join_cell_paragraphs(mut parts: Vec<String>) -> String {
    while parts.first().is_some_and(|p| p.is_empty()) {
        parts.remove(0);
    }
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    parts.join("\n")
}

/// `hp:tbl` 에 `rowCnt`/`colCnt`, 셀마다 `hp:cellAddr`(colAddr/rowAddr)·
/// `hp:cellSpan`(colSpan/rowSpan)·`hp:subList`(본문)이 있다. 중첩 표도 있으므로
/// 스택으로 센다.
fn tables_from_owpml(
    xml: &str,
    next_index: &mut u32,
    styles: &HashMap<String, ParaStyle>,
) -> Result<Vec<WitnessTable>> {
    use quick_xml::escape::unescape;
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    fn attr(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
        e.attributes()
            .flatten()
            .find(|a| a.key.as_ref() == name)
            .map(|a| a.value.to_string())
    }
    fn num(e: &quick_xml::events::BytesStart, name: &str, fallback: usize) -> usize {
        attr(e, name).and_then(|v| v.parse().ok()).unwrap_or(fallback)
    }

    let mut reader = Reader::from_str(xml);
    let mut out: Vec<WitnessTable> = Vec::new();
    // 열려 있는 표들 (중첩) — out 안의 색인
    let mut open: Vec<usize> = Vec::new();
    // 열려 있는 셀 (표 색인, 셀 색인)
    let mut cell: Option<(usize, usize)> = None;
    let mut text_depth = 0usize;
    // **칸 안의 그리기개체 깊이.** 0 보다 크면 글자를 칸에 담지 않는다.
    //
    // `hp:tc` 안에 글상자가 있으면 그 안의 글자는 **칸의 글자가 아니다.**
    // 전에는 `hp:tc` 안의 모든 `hp:t` 를 담아서 글상자 글자를 칸으로 끌어왔다
    // (실측: s14.hwpx 표16 의 1×1 칸이 `①⏎②` 로 읽혔다. 백엔드와 우리 HWP5
    // 판독기는 둘 다 빈 칸이라고 한다 — OWPML 쪽만 틀렸다).
    //
    // 그냥 보기가 이상해지는 데서 끝나지 않는다. `edit::snapshot` 이 이 값을
    // 쓰므로, 그런 칸에 `set-cell` 을 하면 기대값과 되읽은 값이 영원히 어긋나
    // **저장이 되지 않는다.**
    let mut draw_depth = 0usize;

    loop {
        match reader.read_event().context("OWPML 표 파싱 실패")? {
            Event::Start(e) => match e.name().as_ref() {
                // 그리기개체가 열렸다 — 닫히기 전의 글자는 칸의 것이 아니다
                "hp:container" | "hp:drawText" | "hp:rect" | "hp:ellipse" | "hp:line"
                | "hp:polygon" | "hp:curve" | "hp:connectLine" | "hp:pic" => {
                    draw_depth += 1;
                }
                "hp:tbl" => {
                    out.push(WitnessTable {
                        index: *next_index,
                        rows: num(&e, "rowCnt", 0),
                        cols: num(&e, "colCnt", 0),
                        cells: Vec::new(),
                    });
                    *next_index += 1;
                    open.push(out.len() - 1);
                }
                "hp:tc" => {
                    if let Some(&t) = open.last() {
                        out[t].cells.push(WitnessCell {
                            row: 0,
                            col: 0,
                            row_span: 1,
                            col_span: 1,
                            text: String::new(),
                            align: ParaAlign::default(),
                            width: 0,
                            height: 0,
                        });
                        let last = out[t].cells.len() - 1;
                        cell = Some((t, last));
                    }
                }
                "hp:t" => text_depth += 1,
                // 셀 안의 둘째 단락부터는 줄바꿈으로 잇는다 (rhwp 와 같은 규칙)
                "hp:p" => {
                    // 그리기개체 안의 단락은 칸의 단락이 아니다 — 경계도 적지
                    // 않는다. 적으면 칸에 없는 빈 단락이 생긴다.
                    if let Some((t, c)) = cell.filter(|_| draw_depth == 0) {
                        // 셀 **첫 단락**의 정렬만 쓴다
                        if out[t].cells[c].text.is_empty() {
                            if let Some(a) = e
                                .attributes()
                                .flatten()
                                .find(|a| a.key.as_ref() == "paraPrIDRef")
                                .and_then(|a| styles.get(&a.value.to_string()))
                            {
                                out[t].cells[c].align = a.align;
                            }
                        } else {
                            // **단락 경계를 `\n` 이 아니라 표식으로 둔다.**
                            // `\n` 은 이제 글자이기도 하다(`hp:lineBreak`,
                            // 강제 줄바꿈). 같은 글자로 적으면 `hp:tc` 가
                            // 끝날 때 앞뒤 빈 단락을 뗄 수가 없다 — 칸 끝의
                            // 진짜 줄바꿈까지 먹는다. 표식은 `hp:tc` 끝에서
                            // `\n` 으로 바뀐다.
                            out[t].cells[c].text.push(CELL_PARA_MARK);
                        }
                    }
                }
                _ => {}
            },
            Event::Empty(e) => match e.name().as_ref() {
                "hp:cellAddr" => {
                    if let Some((t, c)) = cell {
                        out[t].cells[c].col = num(&e, "colAddr", 0);
                        out[t].cells[c].row = num(&e, "rowAddr", 0);
                    }
                }
                "hp:cellSpan" => {
                    if let Some((t, c)) = cell {
                        out[t].cells[c].col_span = num(&e, "colSpan", 1).max(1);
                        out[t].cells[c].row_span = num(&e, "rowSpan", 1).max(1);
                    }
                }
                // **셀 폭·높이 (HWPUNIT).** HWP5 쪽은 `LIST_HEADER` payload 에서
                // 읽고 여기서는 이 요소가 말한다 — 두 형식이 같은 값을 준다.
                "hp:cellSz" => {
                    if let Some((t, c)) = cell {
                        out[t].cells[c].width = num(&e, "width", 0) as u32;
                        out[t].cells[c].height = num(&e, "height", 0) as u32;
                    }
                }
                other => {
                    if let Some(ch) = inline_char(other) {
                        if let Some((t, c)) = cell {
                            out[t].cells[c].text.push(ch);
                        }
                    }
                }
            },
            Event::End(e) => match e.name().as_ref() {
                "hp:container" | "hp:drawText" | "hp:rect" | "hp:ellipse" | "hp:line"
                | "hp:polygon" | "hp:curve" | "hp:connectLine" | "hp:pic" => {
                    draw_depth = draw_depth.saturating_sub(1);
                }
                "hp:tbl" => {
                    open.pop();
                    cell = None;
                }
                "hp:tc" => {
                    // 칸이 끝났다 — 단락 표식을 풀어 **HWP5 쪽과 같은 규칙으로**
                    // 잇는다.
                    if let Some((ti, ci)) = cell {
                        let raw = std::mem::take(&mut out[ti].cells[ci].text);
                        out[ti].cells[ci].text = join_cell_paragraphs(
                            raw.split(CELL_PARA_MARK).map(String::from).collect(),
                        );
                    }
                    cell = None;
                }
                "hp:t" => text_depth = text_depth.saturating_sub(1),
                _ => {}
            },
            Event::Text(t) => {
                if text_depth > 0 && draw_depth == 0 {
                    if let Some((ti, ci)) = cell {
                        let raw = t.xml10_content();
                        let text = unescape(&raw).context("셀 텍스트 unescape 실패")?;
                        out[ti].cells[ci].text.push_str(&text);
                    }
                }
            }
            Event::GeneralRef(r) => {
                if text_depth > 0 && draw_depth == 0 {
                    if let Some((ti, ci)) = cell {
                        if let Some(c) = resolve_entity(&r) {
                            out[ti].cells[ci].text.push_str(&c);
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)

}

#[cfg(test)]
mod flag_tests {
    use super::*;

    /// **배포용 문서를 가려낼 수 있어야 한다.**
    ///
    /// 배포용은 본문이 `ViewText/` 에 구역별로 암호화되어 있고
    /// `BodyText/Section0` 에는 껍데기만 남는다. 그래서 읽으면 **오류 없이
    /// 1단락**이 나온다 — 공개된 한글문서파일형식 5.0 스펙 문서가
    /// `flags = 0x00020005` 이고 백엔드는 70쪽 619단락이라 하는데 우리 증인은
    /// 1단락이었다. 조용히 빈 문서를 보여주는 것이 가장 나쁜 결과다.
    ///
    /// 파일을 **만들어서** 시험한다 — 남의 시험 문서를 저장소에 들이지 않고,
    /// 비트만 바꿔 셋을 비교할 수 있다.
    #[test]
    fn the_file_header_flags_tell_a_distribution_document_apart() {
        use std::io::Write;

        fn make(flags: u32) -> std::path::PathBuf {
            let path = std::env::temp_dir().join(format!("hwp-flags-{flags:08x}.hwp"));
            let _ = std::fs::remove_file(&path);
            let mut comp = cfb::create(&path).expect("CFB 만들기");
            let mut header = vec![0u8; 256];
            header[..17].copy_from_slice(b"HWP Document File");
            header[36..40].copy_from_slice(&flags.to_le_bytes());
            {
                let mut st = comp.create_stream("/FileHeader").expect("FileHeader");
                st.write_all(&header).expect("쓰기");
            }
            comp.flush().expect("flush");
            path
        }

        // 압축만 — 보통 문서
        let plain = make(0x0000_0001);
        let f = cfb_header_flags(&plain).expect("플래그를 읽는다");
        assert_eq!(f & 0b001, 1, "압축 비트");
        assert_eq!(f & 0b100, 0, "보통 문서인데 배포용으로 읽혔다");

        // 압축 + 배포용 — 실제 스펙 문서와 같은 값
        let dist = make(0x0002_0005);
        let f = cfb_header_flags(&dist).expect("플래그를 읽는다");
        assert_ne!(f & 0b100, 0, "배포용 비트를 못 읽었다");

        // 암호
        let enc = make(0x0000_0003);
        let f = cfb_header_flags(&enc).expect("플래그를 읽는다");
        assert_ne!(f & 0b010, 0, "암호 비트를 못 읽었다");
        assert_eq!(f & 0b100, 0, "암호 문서를 배포용으로 읽었다");

        // CFB 가 아니면 플래그가 없다 — 지어내지 않는다
        let notcfb = std::env::temp_dir().join("hwp-flags-notcfb.bin");
        std::fs::write(&notcfb, b"PK\x03\x04 not a cfb").expect("쓰기");
        assert!(cfb_header_flags(&notcfb).is_none());

        for p in [plain, dist, enc, notcfb] {
            let _ = std::fs::remove_file(p);
        }
    }
}
