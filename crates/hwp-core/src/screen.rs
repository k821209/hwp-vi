//! 화면 문서 — 페이저가 그릴 것.
//!
//! 두 재료를 합친다:
//!
//! - **조판**은 rhwp `dump-pages` 에서 온다. 어느 단락이 몇 쪽에 있고 몇 줄을
//!   차지하는지. 추정이 아니라 조판 엔진이 계산한 값이다.
//! - **본문**은 독립 증인에서 온다. `dump-pages` 의 `textPreview` 는 40자에서
//!   잘려 전문이 아니다(실측).
//!
//! 두 좌표계가 맞물린다는 것은 R0 에서 샘플 14/14 로 확인했다 — 깊이 0 단락의
//! 순번이 rhwp 의 `paraIndex` 다.
//!
//! 접히는 위치는 우리 것이고 **줄 수는 rhwp 것이다.** 그래서 쪽 경계는 정확한
//! 줄에 떨어지고, 한 줄 안에서 어디서 접혔는지만 우리 계산이다. 페이저는 그
//! 구분을 화면에 표시한다 — 거짓말하지 않는 것이 §2-3 부터의 원칙이다.

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

/// 편집 좌표.
pub type Coord = (u32, u32);

use crate::rhwp::Rhwp;
use crate::witness::Testimony;
use crate::wrap::{ColumnWidth, display_width, infer_column_width, wrap};

/// 단 폭을 못 재었을 때 쓰는 값. A4 본문에 11pt 한글이면 대략 이 정도다.
pub const FALLBACK_COLUMN_CELLS: usize = 80;

#[derive(Debug, Clone)]
pub struct ScreenDoc {
    pub path: String,
    pub format: String,
    pub page_count: u32,
    /// 본문 단락 수 (= rhwp paraCount)
    pub para_count: usize,
    /// 좌표별 **단락 원문.** 편집 버퍼가 쓰는 본문은 이것이어야 한다.
    ///
    /// 접힌 줄(`Block::lines`)을 되붙여 만들면 **안 된다.** `wrap` 은 줄을
    /// 끊은 자리의 공백을 버리므로(그게 표시에 맞다) 되붙이면 접힌 자리마다
    /// 한 칸이 사라지고, 커서 오프셋이 그만큼 앞으로 밀린다 — 코퍼스
    /// 단락 200개가 그랬다. 자가검증도 이걸 잡지 못한다: 기대값과 재생이
    /// 같은 틀린 오프셋을 쓰므로 둘이 일치해 버린다.
    pub paras: std::collections::HashMap<Coord, String>,
    /// 구역마다 본문 단락이 몇 개인가
    pub section_counts: Vec<usize>,
    /// 공문서 글머리로 만든 개요 트리 — 페이저의 `:toc` 와 `hwp json` 이 같이 쓴다
    pub outline: Vec<crate::outline::Node>,
    /// 본문 영역 폭 (HWPUNIT). 그림이 본문 폭의 몇 분의 몇인지 재는 데 쓴다.
    pub body_width: u32,
    /// 좌표별 문단 들여쓰기. 0 인 문단은 넣지 않는다.
    pub indents: std::collections::HashMap<Coord, Indent>,
    /// 좌표별 **글자 꾸밈 구간.** 꾸밈이 없는 단락은 넣지 않는다 — 코퍼스
    /// 단락의 77% 가 그렇다.
    pub runs: std::collections::HashMap<Coord, Vec<crate::witness::Run>>,
    /// 좌표별 문단 정렬. 왼쪽으로 그릴 것(`Justify`·`Left` 등)은 넣지 않는다 —
    /// 코퍼스 본문에서 가운데·오른쪽은 12%(96/795)뿐이다.
    pub aligns: std::collections::HashMap<Coord, crate::witness::ParaAlign>,
    /// 좌표별 **그리기개체/표** 개수. 구역 정의·쪽번호 같은 것은 세지 않는다.
    pub objects: std::collections::HashMap<Coord, usize>,
    /// 표 — `(구역, 단락)` 좌표와 함께. 한국 공문서는 대개 표라서, 그리지
    /// 않으면 서식 문서가 빈 쪽으로 보인다.
    pub tables: Vec<PlacedTable>,
    pub column: Column,
    pub pages: Vec<Page>,
}

impl ScreenDoc {
    /// 이 문단 글자마다의 꾸밈을 한 바이트씩. 꾸밈이 없으면 빈 벡터다.
    ///
    /// 그리는 쪽이 글자별로 묻지 않도록 한 번에 만들어 준다.
    pub fn deco_bytes(&self, at: Coord, chars: usize) -> Vec<u8> {
        let Some(runs) = self.runs.get(&at) else { return Vec::new() };
        let mut out = vec![0u8; chars];
        for r in runs {
            let end = (r.start + r.len).min(chars);
            for b in out[r.start.min(chars)..end].iter_mut() {
                *b = r.deco.bits();
            }
        }
        out
    }

    /// 이 문단의 한 줄을 **정렬 때문에 얼마나 밀어야 하는가.**
    ///
    /// `drawn` 은 그 줄이 차지하는 표시 폭(들여쓰기 포함), `column` 은 단 폭.
    /// 가운데·오른쪽 정렬만 밀고 나머지는 0 이다 — 터미널은 양쪽맞춤을 할 수
    /// 없으므로 `Justify` 는 왼쪽과 같게 그린다.
    ///
    /// 화면을 그리는 쪽과 **커서 자리를 계산하는 쪽이 같은 값을 써야 한다.**
    /// 한쪽만 밀면 커서가 글자에서 떨어진다.
    pub fn align_pad(&self, at: Coord, drawn: usize, column: usize) -> usize {
        use crate::witness::ParaAlign;
        match self.aligns.get(&at) {
            Some(ParaAlign::Center) => column.saturating_sub(drawn) / 2,
            Some(ParaAlign::Right) => column.saturating_sub(drawn),
            _ => 0,
        }
    }

    /// 선언한 블록에서 `paras` 를 채운다 — **시험 도우미 전용.**
    ///
    /// 실제 문서는 `build()` 가 증인의 원문으로 채운다. 손으로 만든 화면
    /// 모델은 접기 전 원문이 따로 없으므로 블록의 줄을 이어 쓴다. 시험에
    /// 쓰는 본문은 접히지 않을 만큼 짧아서 이어 붙이면 원문과 같다 — 길어
    /// 접히는 본문으로 시험을 쓸 때는 `paras` 를 직접 주어야 한다.
    pub fn with_paras_from_blocks(mut self) -> Self {
        for page in &self.pages {
            for b in &page.blocks {
                if let Some(i) = b.para_index {
                    self.paras
                        .entry((b.section, i))
                        .or_default()
                        .push_str(&b.lines.concat());
                }
            }
        }
        self
    }

    /// 이 문단의 들여쓰기를 **칸**으로. `(첫 줄, 이어지는 줄)`.
    ///
    /// 들여쓰기가 없는 문단은 `(0, 0)` 이다 — 거의 모든 문단이 그렇다.
    pub fn indent_cells(&self, at: Coord, column: usize) -> (usize, usize) {
        self.indents
            .get(&at)
            .map(|i| i.cells(self.body_width, column))
            .unwrap_or((0, 0))
    }
}

/// 본문의 어느 자리에 놓인 표.
#[derive(Debug, Clone)]
pub struct PlacedTable {
    pub at: (u32, u32),
    /// 한 단락에 표가 여럿일 때의 순번
    pub control: u32,
    /// **문서 안의 표 번호** (0부터). `doc.tables` 는 좌표로 정렬되므로 배열의
    /// 자리와 다르다 — 칸 안의 그림이 어느 표인지 `images::ImageCell::table`
    /// 과 맞추려면 이 번호를 봐야 한다.
    pub index: u32,
    pub table: crate::grid::Table,
}

/// 단 폭과, 그것을 어떻게 얻었는지.
#[derive(Debug, Clone, Copy)]
pub struct Column {
    pub cells: usize,
    pub measured: Option<ColumnWidth>,
}

impl Column {
    /// 화면에 띄울 한 마디. 재었는지 아닌지를 숨기지 않는다.
    pub fn provenance(&self) -> String {
        match self.measured {
            Some(m) if m.is_measured() && m.is_mixed() => format!(
                "{}칸 (조판 {}단락에서 측정 — 글꼴이 섞여 좁은 쪽은 {}칸, \
                 큰 글꼴 단락은 문서보다 늦게 접힌다)",
                self.cells, m.samples, m.narrowest
            ),
            Some(m) if m.is_measured() => {
                format!("{}칸 (조판 {}단락에서 측정)", self.cells, m.samples)
            }
            _ => format!("{}칸 (측정 못 함, 기본값)", self.cells),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Page {
    /// 문서 순서 (1부터). 쪽을 가리키는 주소.
    pub number: u32,
    /// 문서가 찍는 쪽번호. `number` 와 다르면 문서가 쪽번호를 다시 시작한 것이다.
    pub printed: Option<u32>,
    pub section: u32,
    pub blocks: Vec<Block>,
}

/// 문단의 들여쓰기 (HWPUNIT).
///
/// `first` 는 첫 줄, `rest` 는 이어지는 줄. 문단서식의 `왼쪽 여백`·`들여쓰기`
/// 에서 나온다 — `first = left + max(intent,0)`, `rest = left + max(-intent,0)`.
/// 들여쓰기가 음수면 **내어쓰기**이고 그때 둘이 달라진다.
///
/// **내어쓰기가 다수다.** 코퍼스 본문 795단락 중 63%가 음수 들여쓰기를 쓴다 —
/// 글머리를 왼쪽에 두고 글을 그 아래로 맞추는 바로 그 장치다.
///
/// 조판기의 `lineSegs.columnStart` 로는 이걸 알 수 없다. 그 값은 **첫 줄만**
/// 말하고 내어쓰기 문단에서 `(0, 0)` 을 돌려준다 — s03.hwpx 단락3 은
/// `intent=-3114` 로 둘째 줄이 31.1pt 들어가는데 columnStart 는 0 이다.
/// 그래서 문단서식을 증인이 직접 읽는다. 첫 줄 값은 columnStart 와
/// 코퍼스 전수(670/670) 일치하므로 그쪽이 이 판독의 시험이 된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Indent {
    pub first: i64,
    pub rest: i64,
}

impl Indent {
    /// 본문 폭에 대한 비율로 칸 수를 낸다. 글자가 들어갈 자리는 남긴다.
    pub fn cells(&self, body_width: u32, column: usize) -> (usize, usize) {
        if body_width == 0 || column == 0 {
            return (0, 0);
        }
        let to_cells = |v: i64| -> usize {
            if v <= 0 {
                return 0;
            }
            let c = (v as f64 / body_width as f64 * column as f64).round() as usize;
            // 들여쓰기가 단을 다 먹으면 글자를 읽을 수 없다 — 바닥을 남긴다
            c.min(column.saturating_sub(8))
        };
        (to_cells(self.first), to_cells(self.rest))
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    pub section: u32,
    /// 구역 안에서의 단락 순번. `(section, para_index)` 가 편집 좌표다.
    pub para_index: Option<u32>,
    /// 이 쪽에 놓인 줄들 (이미 접힌 상태)
    pub lines: Vec<String>,
    /// 이 단락이 쪽을 넘어 이어지는가
    pub continues: bool,
    /// 이전 쪽에서 이어진 것인가
    pub continued: bool,
}

impl ScreenDoc {
    pub fn build(rhwp: &Rhwp, path: &Path) -> Result<Self> {
        let info = rhwp.info(path)?;
        let layout = rhwp.dump_pages(path)?;
        let witness = Testimony::read(path)?;

        // 본문을 rhwp 좌표 (구역, 단락) 로 색인한다. 구역이 여러 개인 문서에서
        // 전역 순번을 쓰면 어긋난다 — s14.hwpx 로 실측했다.
        let mut texts: HashMap<(u32, u32), String> = HashMap::new();
        for p in witness.body_paragraphs() {
            if let Some(i) = p.index {
                texts.insert((p.section, i), p.text.clone());
            }
        }
        let para_count = texts.len();

        // 1) 단락마다 전체 줄 수와 등장 횟수.
        //    쪽을 넘는 단락은 닿는 쪽마다 같은 전체 줄 수를 되풀이한다 — 더하면 안 된다.
        let mut total_lines: HashMap<(u32, u32), u32> = HashMap::new();
        let mut appearances: HashMap<(u32, u32), u32> = HashMap::new();
        for page in &layout.pages {
            for col in &page.columns {
                for item in &col.items {
                    if let (Some(pi), Some(n)) = (item.para_index, item.total_lines()) {
                        let key = (page.section, pi);
                        let e = total_lines.entry(key).or_insert(0);
                        *e = (*e).max(n);
                        *appearances.entry(key).or_insert(0) += 1;
                    }
                }
            }
        }

        // 2) 단 폭을 잰다. 쪽을 넘는 단락은 빼고 — 폭을 좁히는 근거로 약하다.
        let observations: Vec<(usize, usize)> = total_lines
            .iter()
            .filter(|(k, n)| **n >= 2 && appearances.get(*k).copied().unwrap_or(0) <= 1)
            .filter_map(|(k, n)| texts.get(k).map(|t| (display_width(t), *n as usize)))
            .collect();
        let measured = infer_column_width(&observations);
        let cells = measured
            .filter(|m| m.is_measured())
            .map(|m| m.chosen())
            .unwrap_or(FALLBACK_COLUMN_CELLS);
        let column = Column { cells, measured };
        // 원문을 그대로 들고 나간다 — 접기는 표시일 뿐이다 (`paras` 주석 참고).
        let paras: HashMap<Coord, String> = texts.clone();

        // 3) 접고, rhwp 가 말한 쪽별 줄 수대로 나눠 담는다.
        let wrapped: HashMap<(u32, u32), Vec<String>> = texts
            .iter()
            .map(|(k, t)| (*k, wrap(t, cells)))
            .collect();
        let mut consumed: HashMap<(u32, u32), usize> = HashMap::new();

        let mut pages = Vec::with_capacity(layout.pages.len());
        for (pi, page) in layout.pages.iter().enumerate() {
            let mut blocks = Vec::new();
            for col in &page.columns {
                for item in &col.items {
                    let Some(para) = item.para_index else {
                        blocks.push(Block {
                            section: page.section,
                            para_index: None,
                            lines: item
                                .text_preview
                                .clone()
                                .filter(|t| !t.is_empty())
                                .map(|t| vec![t])
                                .unwrap_or_default(),
                            continues: false,
                            continued: false,
                        });
                        continue;
                    };
                    let key = (page.section, para);
                    let Some(all) = wrapped.get(&key) else { continue };
                    let start = *consumed.get(&key).unwrap_or(&0);
                    let remaining = all.len().saturating_sub(start);
                    let want = item.lines_on_page() as usize;
                    // 마지막 조각이면 남은 줄 전부 — 우리 접힘이 rhwp 와 한두 줄
                    // 달라도 글자를 잃지 않게 한다.
                    let once = appearances.get(&key).copied().unwrap_or(1) <= 1;
                    let take = if once || start + want >= all.len() {
                        remaining
                    } else {
                        want.min(remaining)
                    };
                    let lines = all[start..(start + take).min(all.len())].to_vec();
                    let now = (start + take).min(all.len());
                    consumed.insert(key, now);
                    blocks.push(Block {
                        section: page.section,
                        para_index: Some(para),
                        lines,
                        continues: now < all.len(),
                        continued: start > 0,
                    });
                }
            }
            pages.push(Page {
                number: page.display_page.unwrap_or((pi + 1) as u32),
                printed: page.page_number,
                section: page.section,
                blocks,
            });
        }

        // 개요 트리 — 빈 단락은 넣지 않는다.
        let mut items: Vec<crate::outline::Item> = texts
            .iter()
            .filter(|(_, t)| !t.trim().is_empty())
            .map(|((sec, idx), t)| crate::outline::Item {
                section: *sec,
                para: *idx,
                text: crate::offset::visible(t),
            })
            .collect();
        items.sort_by_key(|i| (i.section, i.para));
        let outline = crate::outline::build(&items);

        let objects: HashMap<Coord, usize> = witness
            .body_paragraphs()
            .into_iter()
            .filter_map(|p| {
                let index = p.index?;
                let n = p
                    .objects
                    .iter()
                    .filter(|c| **c == crate::witness::CTRL_DRAWING_OR_TABLE)
                    .count();
                (n > 0).then_some(((p.section, index), n))
            })
            .collect();

        // 표 — 좌표와 셀을 백엔드에서 받는다(증인과 셀 단위로 일치함을
        // 코퍼스 전수 대조로 확인했다).
        let mut tables: Vec<PlacedTable> = rhwp
            .export_tables(path)
            .map(|e| {
                e.tables
                    .iter()
                    .map(|t| PlacedTable {
                        at: (t.section, t.paragraph),
                        control: t.control,
                        index: t.index,
                        table: crate::grid::Table {
                            rows: t.rows.max(
                                t.cells.iter().map(|c| c.row + c.row_span).max().unwrap_or(0),
                            ),
                            cols: t.cols.max(
                                t.cells.iter().map(|c| c.col + c.col_span).max().unwrap_or(0),
                            ),
                            cells: t
                                .cells
                                .iter()
                                .map(|c| crate::grid::Cell {
                                    row: c.row,
                                    col: c.col,
                                    row_span: c.row_span.max(1),
                                    col_span: c.col_span.max(1),
                                    text: c.text.clone(),
                                    art: Vec::new(),
                                    min_width: 0,
                                    // 폭은 백엔드가 주지 않는다 — 아래에서
                                    // 증인이 읽은 것을 좌표로 맞춰 채운다
                                    doc_width: 0,
                                    // 정렬은 백엔드가 주지 않는다 — 아래에서
                                    // 증인이 읽은 것을 좌표로 맞춰 채운다
                                    align: Default::default(),
                                })
                                .collect(),
                        },
                    })
                    .collect()
            })
            .unwrap_or_default();
        // 칸 정렬을 증인에게서 받아 채운다. 백엔드 `export-tables` 는 글자만
        // 주고 정렬을 주지 않는다. 표 번호·행·열로 맞추는데, 그 좌표가 두
        // 판독에서 같다는 것은 코퍼스 전수 대조로 확인했다(그림 23/23).
        for t in tables.iter_mut() {
            let Some(wt) = witness.tables.iter().find(|w| w.index == t.index) else { continue };
            for cell in t.table.cells.iter_mut() {
                if let Some(wc) = wt.cell(cell.row, cell.col) {
                    cell.align = wc.align;
                    // **문서가 적어 둔 칸 폭.** 이것으로 열 비율을 정한다 —
                    // 내용 길이로 짐작하면 문서와 다른 표가 나온다.
                    cell.doc_width = wc.width;
                }
            }
        }
        tables.sort_by_key(|t| (t.at, t.control));

        Ok(Self {
            path: path.display().to_string(),
            format: info.format,
            page_count: layout.page_count,
            para_count,
            paras,
            section_counts: witness.section_counts(),
            // `bodyArea.width` 는 **96dpi px** 다 — pt 가 아니다. 백엔드가
            // 같은 줄에 `used=970.5px` 로 단위를 적어 둔다. 1px = 7200/96 =
            // 75 HWPUNIT. 맞춰 봤다: 642.5px × 75 = 48188 이고, 같은 문서의
            // `lineSegs.sw`(단락 줄 폭, HWPUNIT)가 정확히 48188 이다.
            // 100 을 곱하면 본문이 4/3 배로 넓어 보여서 그림이 그만큼 작게
            // 그려진다 — s00 의 배너가 64칸 대신 48칸으로 나왔다.
            body_width: layout
                .pages
                .first()
                .map(|p| (p.body_area.width * 75.0).round().max(0.0) as u32)
                .unwrap_or(0),
            // 문단 들여쓰기·정렬 — **증인이 문단서식에서 직접 읽은 것**.
            //
            // 조판기의 `columnStart` 는 첫 줄만 말하고 내어쓰기를 알려주지
            // 않는다(내어쓰기 문단에서 0 을 돌려준다). 코퍼스 본문의 63%가
            // 내어쓰기이므로 그쪽만 보면 다수를 놓친다. 첫 줄 값이
            // columnStart 와 전수 일치하는 것은 코퍼스 시험이 지킨다.
            indents: witness
                .body_paragraphs()
                .iter()
                .filter_map(|p| {
                    let at = (p.section, p.index?);
                    let ind = Indent { first: p.style.first_indent, rest: p.style.rest_indent };
                    (ind.first > 0 || ind.rest > 0).then_some((at, ind))
                })
                .collect(),
            runs: witness
                .body_paragraphs()
                .iter()
                .filter_map(|p| {
                    let at = (p.section, p.index?);
                    (!p.runs.is_empty()).then(|| (at, p.runs.clone()))
                })
                .collect(),
            aligns: witness
                .body_paragraphs()
                .iter()
                .filter_map(|p| {
                    use crate::witness::ParaAlign;
                    let at = (p.section, p.index?);
                    matches!(p.style.align, ParaAlign::Center | ParaAlign::Right)
                        .then_some((at, p.style.align))
                })
                .collect(),
            outline,
            objects,
            tables,
            column,
            pages,
        })
    }

    /// 모든 쪽의 모든 줄 수 — 세로 스크롤 길이.
    pub fn total_lines(&self) -> usize {
        self.pages
            .iter()
            .map(|p| p.blocks.iter().map(|b| b.lines.len()).sum::<usize>())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_provenance_says_when_it_guessed() {
        let guessed = Column { cells: 80, measured: None };
        assert!(guessed.provenance().contains("측정 못 함"));

        let measured = Column {
            cells: 40,
            measured: Some(ColumnWidth { cells: 40, narrowest: 38, samples: 7 }),
        };
        assert!(measured.provenance().contains("7단락에서 측정"));
    }
}
