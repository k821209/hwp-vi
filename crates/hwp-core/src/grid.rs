//! 표를 터미널 글자로 그린다.
//!
//! 한국 공문서는 대개 표다 — 이 코퍼스만 해도 표 126개, 누름틀 7개다. 표를
//! 그리지 않으면 서식 문서는 **빈 쪽**으로 보인다(s16.hwp 1쪽이 실제로 그랬다).
//!
//! ## 병합을 다루는 방법
//!
//! 병합된 표를 글자로 그리는 어려움은 전부 **선을 그을지 말지**에 있다. 칸마다
//! 어느 셀의 땅인지 적어 둔 지도(`owner`)를 만들면 규칙이 하나로 줄어든다:
//!
//! - 두 칸 사이에 선이 있다 ⟺ **주인이 다르다**
//!
//! 그러면 교차점의 모양도 따라 나온다 — 그 점에서 뻗는 네 방향 선분이 있는지만
//! 보고 `┼ ├ ┤ ┬ ┴ ─ │` 중에 고른다. 특수 경우를 손으로 나열하지 않는다.

use crate::wrap::{display_width, wrap};

/// 그릴 셀 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
    pub row_span: usize,
    pub col_span: usize,
    pub text: String,
    /// 이 칸 안에 **그대로** 놓을 줄들 (그림의 점 그림). 글자처럼 접지 않는다
    /// — 이미 칸 폭에 맞춰 그려져 있어서 접으면 그림이 부서진다.
    pub art: Vec<String>,
    /// 칸 안 글자의 정렬. **표 안에서는 가운데가 다수다** — 코퍼스의
    /// 표·글상자 단락 2357개 중 64%가 가운데이고 양쪽맞춤은 25%뿐이다.
    /// 전부 왼쪽으로 그리면 공문서 표가 서식처럼 보이지 않는다.
    pub align: crate::witness::ParaAlign,
    /// 이 칸이 **적어도 이만큼은** 넓어야 한다고 미리 말해 두는 값.
    ///
    /// 그림이 든 칸에 필요하다. 그림은 칸 폭에 맞춰 그리고 칸 폭은 내용이
    /// 필요한 만큼을 보고 정하므로, 그림의 폭을 글자처럼 재면 서로를 기다린다.
    /// 그래서 **원하는 폭만 먼저 말하고**(문서가 놓은 크기에서 나온 값) 실제
    /// 폭은 표가 정하게 한다. 이게 없으면 글자 없는 그림 칸이 `MIN_CELL`(4칸)
    /// 로 쪼그라들어 `[그` 까지만 보인다.
    pub min_width: usize,
    /// **문서가 적어 둔 칸 폭** (HWPUNIT). 0 이면 모른다.
    ///
    /// 화면의 열 폭을 **내용 길이로 짐작하면** 문서와 다른 표가 나온다 —
    /// "실제 문서의 표 크기와 상동하지 않는다" 는 보고(2026-10-12)의 원인이
    /// 그것이었다. 조판값은 표의 전체 폭만 주므로 열별 폭은 증인이 셀
    /// 레코드에서 읽는다(HWP5 `LIST_HEADER`, OWPML `hp:cellSz`).
    pub doc_width: u32,
}

/// 그릴 표.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<Cell>,
}

/// 칸이 적어도 이만큼은 넓어야 글자가 들어간다 (한글 한 자 = 2칸).
const MIN_CELL: usize = 4;

/// 표시 폭 `max` 칸까지 자른다. 폭 2 글자가 경계에 걸리면 넣지 않는다 —
/// 한 칸 넘치면 표의 그 줄이 전부 밀린다.
fn clip(line: &str, max: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for ch in line.chars() {
        let w = display_width(&ch.to_string());
        if used + w > max {
            break;
        }
        out.push(ch);
        used += w;
    }
    out
}

/// **칸 글자를 칸 폭에 맞춰 접고 정렬한 줄들.**
///
/// 그리는 쪽과 **커서를 놓는 쪽**이 같은 함수를 써야 커서가 글자 위에 선다.
/// 따로 적으면 접는 자리나 정렬 여백이 한 칸 어긋나고, 그러면 커서가 글자
/// 사이에 떠서 "어디를 고치는지" 를 알 수 없다.
pub fn lay_cell(
    text: &str,
    width: usize,
    align: crate::witness::ParaAlign,
) -> Vec<String> {
    let mut lines: Vec<String> = if text.trim().is_empty() {
        Vec::new()
    } else {
        // 칸 안의 줄바꿈을 지키고, 각 줄을 폭에 맞춰 또 접는다
        text.lines()
            .flat_map(|l| wrap(&crate::wrap::for_screen(l), width))
            .collect()
    };
    // 정렬 — 줄마다 그 줄의 폭으로 민다. 빈 줄은 밀지 않는다
    // (꼬리 공백이 되고, 칸을 지나 복사하면 따라간다).
    lines = lines
        .into_iter()
        .map(|l| {
            if l.trim().is_empty() {
                return l;
            }
            let pad = match align {
                crate::witness::ParaAlign::Center => {
                    width.saturating_sub(display_width(&l)) / 2
                }
                crate::witness::ParaAlign::Right => width.saturating_sub(display_width(&l)),
                _ => 0,
            };
            if pad > 0 { format!("{}{l}", " ".repeat(pad)) } else { l }
        })
        .collect();
    lines
}

impl Table {
    /// 칸마다 주인을 적은 지도. 병합된 자리는 좌상단 셀이 주인이다.
    fn owners(&self) -> Vec<Vec<Option<usize>>> {
        let mut map = vec![vec![None; self.cols]; self.rows];
        for (i, c) in self.cells.iter().enumerate() {
            for r in c.row..(c.row + c.row_span.max(1)).min(self.rows) {
                for col in c.col..(c.col + c.col_span.max(1)).min(self.cols) {
                    // 겹쳐 적힌 셀이 있으면 먼저 온 쪽이 주인 — 깨진 표에서도
                    // 그리기는 끝나야 한다.
                    if map[r][col].is_none() {
                        map[r][col] = Some(i);
                    }
                }
            }
        }
        map
    }

    /// 열 폭을 정한다. 내용이 필요한 만큼을 보고 전체 폭에 맞춰 줄인다.
    /// **문서가 적어 둔 열 폭** (HWPUNIT). 모르면 `None`.
    ///
    /// 한 열에 걸치지 않은 칸(`col_span == 1`)이 그 열의 폭을 말한다. 걸친
    /// 칸만 있는 열은 그 칸의 폭을 걸친 수로 나눠 받는다 — 가진 정보로는
    /// 그보다 더 알 수 없다.
    pub fn doc_column_widths(&self) -> Option<Vec<u32>> {
        if self.cols == 0 {
            return None;
        }
        let mut out = vec![0u32; self.cols];
        // 걸치지 않은 칸이 먼저 — 그게 정확한 값이다
        for c in &self.cells {
            if c.col_span.max(1) == 1 && c.doc_width > 0 && c.col < self.cols {
                out[c.col] = out[c.col].max(c.doc_width);
            }
        }
        // 남은 열은 걸친 칸에서 나눠 받는다
        for c in &self.cells {
            let span = c.col_span.max(1);
            if span == 1 || c.doc_width == 0 {
                continue;
            }
            let each = c.doc_width / span as u32;
            for col in c.col..(c.col + span).min(self.cols) {
                if out[col] == 0 {
                    out[col] = each;
                }
            }
        }
        if out.iter().all(|w| *w > 0) { Some(out) } else { None }
    }

    /// 시험·측정용 — `column_widths` 를 바깥에서 부른다.
    pub fn cell_widths_for_test(&self, total: usize) -> Vec<usize> {
        self.column_widths(total)
    }

    fn column_widths(&self, total: usize) -> Vec<usize> {
        // 선이 cols+1 개 들어간다
        let borders = self.cols + 1;
        let usable = total.saturating_sub(borders).max(self.cols * MIN_CELL);

        // **문서가 적어 둔 폭을 알면 그 비율로 간다.**
        //
        // 내용 길이로 짐작하면 문서와 다른 표가 나온다. 바닥(`MIN_CELL`)은
        // 지킨다 — 문서의 2% 짜리 열을 비율대로 주면 한 칸이 되어 글자를 읽을
        // 수 없다. 그래서 **비율이 바닥에서 깨지는 것은 어쩔 수 없다**고 적어
        // 둔다(터미널의 한계이고, 어긋난 표보다 읽히는 표가 낫다).
        if let Some(doc) = self.doc_column_widths() {
            let sum: u64 = doc.iter().map(|w| *w as u64).sum();
            if sum > 0 {
                let mut out: Vec<usize> = doc
                    .iter()
                    .map(|w| {
                        ((*w as u64 * usable as u64 + sum / 2) / sum) as usize
                    })
                    .map(|w| w.max(MIN_CELL))
                    .collect();
                // 바닥을 지키다 넘쳤으면 넓은 칸부터 한 칸씩 깎는다
                let mut over = out.iter().sum::<usize>() as isize - usable as isize;
                while over > 0 {
                    let Some(widest) = (0..self.cols).max_by_key(|&i| out[i]) else { break };
                    if out[widest] <= MIN_CELL {
                        break;
                    }
                    out[widest] -= 1;
                    over -= 1;
                }
                // 남으면 넓은 칸에 보탠다 — 반올림으로 한두 칸이 뜬다
                let mut left = usable as isize - out.iter().sum::<usize>() as isize;
                while left > 0 {
                    let Some(widest) = (0..self.cols).max_by_key(|&i| out[i]) else { break };
                    out[widest] += 1;
                    left -= 1;
                }
                return out;
            }
        }

        // 폭을 모르면 내용으로 짐작한다 — 글상자나 폭이 없는 문서다.
        // 셀 하나가 차지하고 싶은 폭. 병합된 셀은 걸친 열들에 나눠 준다.
        let mut want = vec![MIN_CELL; self.cols];
        for c in &self.cells {
            let span = c.col_span.max(1);
            let need = c
                .text
                .lines()
                .map(display_width)
                .max()
                .unwrap_or(0)
                // 한 줄에 다 넣으려 들지 않는다 — 긴 셀은 접히는 게 정상이다
                .min(40 * span);
            let each = need.div_ceil(span);
            // 그림이 말한 최소 폭도 같이 본다
            let each = each.max(c.min_width.min(40 * span).div_ceil(span));
            for col in c.col..(c.col + span).min(self.cols) {
                want[col] = want[col].max(each.max(MIN_CELL));
            }
        }

        let sum: usize = want.iter().sum();
        if sum <= usable {
            return want;
        }

        // 넘치면 비례해서 줄이되 바닥은 지킨다.
        let mut out: Vec<usize> = want
            .iter()
            .map(|w| ((*w as f64) * (usable as f64) / (sum as f64)).floor() as usize)
            .map(|w| w.max(MIN_CELL))
            .collect();

        // 바닥을 지키다 다시 넘쳤으면 넓은 칸부터 한 칸씩 깎는다.
        let mut over: isize = out.iter().sum::<usize>() as isize - usable as isize;
        while over > 0 {
            let Some(widest) = (0..self.cols).max_by_key(|&i| out[i]) else { break };
            if out[widest] <= MIN_CELL {
                break; // 더 깎을 수 없다 — 표가 터미널보다 넓다
            }
            out[widest] -= 1;
            over -= 1;
        }
        // 남으면 좁은 칸에 돌려준다
        let mut spare: isize = usable as isize - out.iter().sum::<usize>() as isize;
        while spare > 0 {
            let Some(narrowest) = (0..self.cols).min_by_key(|&i| out[i]) else { break };
            out[narrowest] += 1;
            spare -= 1;
        }
        out
    }

    /// 셀이 걸친 폭 (사이의 선까지 합친다).
    fn span_width(widths: &[usize], col: usize, span: usize) -> usize {
        let end = (col + span).min(widths.len());
        let inner: usize = widths[col..end].iter().sum();
        inner + end.saturating_sub(col + 1) // 안쪽 선
    }

    /// 칸마다 **글자가 들어갈 안쪽 폭**. `cells` 와 같은 순서다.
    ///
    /// `render` 안에서 폭을 정하므로, 칸에 넣을 그림을 그 폭으로 그리려면
    /// 미리 물어볼 길이 필요하다. 같은 `total_width` 로 부르면 `render` 가
    /// 쓰는 값과 같다.
    pub fn cell_widths(&self, total_width: usize) -> Vec<usize> {
        if self.rows == 0 || self.cols == 0 {
            return vec![0; self.cells.len()];
        }
        let widths = self.column_widths(total_width);
        self.cells
            .iter()
            .map(|c| Self::span_width(&widths, c.col, c.col_span.max(1)))
            .collect()
    }

    /// 표를 글자 줄들로. 각 줄의 표시 폭은 같다.
    pub fn render(&self, total_width: usize) -> Vec<String> {
        self.render_with_spots(total_width).0
    }

    /// 그린 줄들과 칸 자리를 같이 돌려준다.
    pub fn render_with_spots(&self, total_width: usize) -> (Vec<String>, Vec<CellSpot>) {
        if self.rows == 0 || self.cols == 0 {
            return (Vec::new(), Vec::new());
        }
        let widths = self.column_widths(total_width);
        let owners = self.owners();

        // 셀마다 접힌 글자
        let wrapped: Vec<Vec<String>> = self
            .cells
            .iter()
            .map(|c| {
                let w = Self::span_width(&widths, c.col, c.col_span.max(1));
                let mut lines = lay_cell(&c.text, w, c.align);
                // 그림은 접지 않고 그대로 — 넘치면 자른다. 칸이 그림보다
                // 좁아졌을 때 표가 어긋나는 쪽이 더 나쁘다.
                lines.extend(c.art.iter().map(|l| {
                    if display_width(l) > w { clip(l, w) } else { l.clone() }
                }));
                lines
            })
            .collect();

        // 행 높이: 그 행에서 시작하는 셀들이 필요한 줄 수.
        // 여러 행에 걸친 셀은 걸친 행들에 나눠 준다.
        let mut heights = vec![1usize; self.rows];
        for (i, c) in self.cells.iter().enumerate() {
            let span = c.row_span.max(1);
            let need = wrapped[i].len().max(1);
            let each = need.div_ceil(span);
            for r in c.row..(c.row + span).min(self.rows) {
                heights[r] = heights[r].max(each);
            }
        }

        let table_w = widths.iter().sum::<usize>() + self.cols + 1;
        let table_h = heights.iter().sum::<usize>() + self.rows + 1;
        // 칸 하나가 글자 하나라고 보면 안 된다. 조합용 한글 자모(`ᄒᆞᆫ글` =
        // U+1112 U+119E U+11AB …)는 뒤 자모가 **폭 0** 이라 앞 글자에 붙는다.
        // 한 칸씩 전진시키면 줄이 짧아져 표가 어긋난다 — 코퍼스가 잡아 줬다.
        // 그래서 칸마다 문자열을 둔다.
        let mut canvas: Vec<Vec<String>> = vec![vec![" ".to_string(); table_w]; table_h];

        // 칸의 왼쪽 선 x 좌표, 행의 위쪽 선 y 좌표
        let xs: Vec<usize> = (0..=self.cols)
            .map(|c| widths[..c].iter().sum::<usize>() + c)
            .collect();
        let ys: Vec<usize> = (0..=self.rows)
            .map(|r| heights[..r].iter().sum::<usize>() + r)
            .collect();

        // 선분이 있는가 — 주인이 다르면 있다.
        let vline = |r: usize, c: usize| -> bool {
            if c == 0 || c == self.cols {
                return true; // 표의 바깥 테두리
            }
            owners[r][c - 1] != owners[r][c]
        };
        let hline = |r: usize, c: usize| -> bool {
            if r == 0 || r == self.rows {
                return true;
            }
            owners[r - 1][c] != owners[r][c]
        };

        // 가로 선
        for r in 0..=self.rows {
            for c in 0..self.cols {
                if !hline(r, c) {
                    continue;
                }
                // 바깥 테두리와 안쪽 선을 구분한다. ASCII 로 그릴 때
                // 둘 다 `+---+` 면 표가 어디서 끝나는지 알 수 없다 — 사람도
                // 기계도. 유니코드 쪽은 교차 글자가 이미 구분된다.
                let outer = r == 0 || r == self.rows;
                for x in (xs[c] + 1)..xs[c + 1] {
                    canvas[ys[r]][x] = h_glyph(outer).into();
                }
            }
        }
        // 세로 선
        for c in 0..=self.cols {
            for r in 0..self.rows {
                if !vline(r, c) {
                    continue;
                }
                for y in (ys[r] + 1)..ys[r + 1] {
                    canvas[y][xs[c]] = v_glyph().into();
                }
            }
        }
        // 교차점 — 네 방향 선분이 있는지만 보고 고른다
        for (r, &y) in ys.iter().enumerate() {
            for (c, &x) in xs.iter().enumerate() {
                let up = r > 0 && vline(r - 1, c);
                let down = r < self.rows && vline(r, c);
                let left = c > 0 && hline(r, c - 1);
                let right = c < self.cols && hline(r, c);
                canvas[y][x] = junction(up, down, left, right).to_string();
            }
        }

        // 글자
        for (i, c) in self.cells.iter().enumerate() {
            let x0 = xs[c.col] + 1;
            let y0 = ys[c.row] + 1;
            for (dy, line) in wrapped[i].iter().enumerate() {
                let y = y0 + dy;
                if y >= table_h.saturating_sub(1) {
                    break;
                }
                let mut x = x0;
                for ch in line.chars() {
                    let w = display_width(&ch.to_string());
                    if w == 0 {
                        // 폭 0 — 앞 칸의 글자에 붙는다. 칸을 쓰지 않는다.
                        if x > x0 {
                            canvas[y][x - 1].push(ch);
                        }
                        continue;
                    }
                    if x + w > table_w {
                        break;
                    }
                    canvas[y][x] = ch.to_string();
                    // 2칸 글자는 다음 칸을 먹는다 — 비워 두지 않으면 줄이 밀린다
                    for k in 1..w {
                        canvas[y][x + k].clear();
                    }
                    x += w;
                }
            }
        }

        // 칸 자리 — 커서가 넘나들 때 쓴다. 테두리를 뺀 안쪽만 가리킨다.
        let spots: Vec<CellSpot> = self
            .cells
            .iter()
            .map(|c| {
                let span = c.col_span.max(1);
                let rspan = c.row_span.max(1);
                let x = xs[c.col.min(self.cols)] + 1;
                let y = ys[c.row.min(self.rows)] + 1;
                let x2 = xs[(c.col + span).min(self.cols)];
                let y2 = ys[(c.row + rspan).min(self.rows)];
                CellSpot {
                    row: c.row,
                    col: c.col,
                    row_span: rspan,
                    col_span: span,
                    x,
                    y,
                    w: x2.saturating_sub(x),
                    h: y2.saturating_sub(y),
                }
            })
            .collect();

        (canvas.into_iter().map(|row| row.concat()).collect(), spots)
    }
}

/// 그린 표에서 **한 칸이 차지한 자리.** 커서가 칸을 넘나들려면 필요하다.
///
/// 좌표는 그린 줄 안에서의 자리다 — `y` 는 몇 번째 줄, `x` 는 표시 폭 기준
/// 몇 칸째. **테두리는 빼고 글자가 들어가는 안쪽**만 가리킨다. 병합된 칸은
/// 걸친 만큼 넓고 높다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellSpot {
    pub row: usize,
    pub col: usize,
    pub row_span: usize,
    pub col_span: usize,
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

/// 네 방향 선분 유무 → 교차 글자.
/// 가로·세로 선분 글자. Ambiguous 를 넓게 그리는 터미널에서는 ASCII 다
/// (이유는 [`junction`] 의 설명).
fn h_glyph(outer: bool) -> &'static str {
    if crate::wrap::ambiguous_wide() {
        if outer { "=" } else { "-" }
    } else {
        "─"
    }
}

fn v_glyph() -> &'static str {
    if crate::wrap::ambiguous_wide() { "|" } else { "│" }
}

/// 네 방향에서 선 문자 하나.
///
/// **Ambiguous 를 2칸으로 그리는 터미널에서는 ASCII 로 그린다.**
/// `─ │ ┼ ├ ┤` 는 전부 East Asian Ambiguous 라서, 그런 터미널에서는 선 하나가
/// 2칸을 먹는다. 격자는 한 칸에 글자 하나를 두고 세므로 그 가정이 깨지고,
/// 선은 두 배로 늘어나는데 한글 줄은 그대로여서 끝선이 어긋난다 — 실측으로
/// 선 22칸 vs 글자 줄 14칸.
///
/// 유니코드에는 **좁다고 보장된 선 문자가 없다.** `+ - |` 만 ASCII 라서
/// 확실하다. 모양을 잃는 대신 격자가 맞는다 — 어긋난 예쁜 표보다 맞는 표가
/// 읽힌다.
fn junction(up: bool, down: bool, left: bool, right: bool) -> char {
    if crate::wrap::ambiguous_wide() {
        return match (up, down, left, right) {
            (false, false, false, false) => ' ',
            (false, false, _, _) => '-',
            (_, _, false, false) => '|',
            _ => '+',
        };
    }
    match (up, down, left, right) {
        (true, true, true, true) => '┼',
        (true, true, true, false) => '┤',
        (true, true, false, true) => '├',
        (true, true, false, false) => '│',
        (true, false, true, true) => '┴',
        (true, false, true, false) => '┘',
        (true, false, false, true) => '└',
        (true, false, false, false) => '│',
        (false, true, true, true) => '┬',
        (false, true, true, false) => '┐',
        (false, true, false, true) => '┌',
        (false, true, false, false) => '│',
        (false, false, true, true) => '─',
        (false, false, true, false) => '─',
        (false, false, false, true) => '─',
        (false, false, false, false) => ' ',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **셀 안의 탭이 표를 깨지 않는다.**
    ///
    /// 터미널은 탭을 다음 탭 스톱(보통 8칸)까지 띄운다. 폭 0 으로 세고 내보내면
    /// 그 줄과 아래 테두리가 전부 밀린다 — 공개된 한글문서파일형식 5.0 스펙
    /// 문서에서 탭이 든 줄이 109개이고 표가 어긋났다.
    #[test]
    fn a_tab_inside_a_cell_does_not_break_the_grid() {
        let t = Table {
            rows: 2,
            cols: 1,
            cells: vec![
                cell(0, 0, "구버전과\t호환 불가능."),
                cell(1, 0, "탭 없는 줄이 열을 넓힌다 — 비교할 기준"),
            ],
        };
        let lines = t.render(60);
        for l in &lines {
            assert!(!l.contains('\t'), "그린 줄에 탭이 남았다: {l:?}");
        }
        let w0 = display_width(&lines[0]);
        for (i, l) in lines.iter().enumerate() {
            assert_eq!(display_width(l), w0, "{i}번째 줄 폭이 다르다: {l:?}");
        }
    }

    /// 가운데 정렬 칸은 글자를 **그 칸 안에서** 가운데로, 빈 줄은 그대로.
    ///
    /// 열 폭은 내용에 맞춰 정해지므로(표가 들어가면 폭을 다 쓰지 않는다)
    /// 가운데로 밀 자리는 **다른 행이 그 열을 넓혔을 때** 생긴다. 공문서
    /// 표에서 머리 칸이 가운데로 보이는 것이 바로 이 모양이다.
    #[test]
    fn a_centered_cell_centers_each_line_but_leaves_blanks_alone() {
        use crate::witness::ParaAlign;
        let mut head = cell(0, 0, "기 관\n\n둘째 줄");
        head.align = ParaAlign::Center;
        // 둘째 행이 같은 열을 넓힌다 — 그래서 머리 칸에 여백이 생긴다
        let body = cell(1, 0, "농림축산식품부장관과 그 소속기관");
        let t = Table { rows: 2, cols: 1, cells: vec![head, body] };
        let lines = t.render(60);

        let inner: Vec<String> = lines
            .iter()
            .filter(|l| l.starts_with('│'))
            .map(|l| l.trim_matches('│').to_string())
            .collect();
        assert!(inner.len() >= 4, "줄이 모자라다: {inner:?}");
        let lead = |s: &str| s.len() - s.trim_start().len();
        assert!(lead(&inner[0]) > 0, "첫 글줄이 가운데로 가지 않았다: {:?}", inner[0]);
        assert_eq!(inner[1].trim(), "", "가운데 빈 줄이 아니다: {:?}", inner[1]);
        assert!(lead(&inner[2]) > 0, "둘째 글줄이 가운데로 가지 않았다: {:?}", inner[2]);
        // 정렬 없는 아래 칸은 왼쪽에 붙어 있다
        assert_eq!(lead(&inner[3]), 0, "정렬 없는 칸이 밀렸다: {:?}", inner[3]);
        // 모든 줄 폭이 같다 — 정렬을 넣어도 격자가 어긋나지 않는다
        let w0 = display_width(&lines[0]);
        for l in &lines {
            assert_eq!(display_width(l), w0, "줄 폭이 다르다: {l}");
        }
    }

    /// **그림이 든 칸은 쪼그라들지 않고, 그림은 접히지 않는다.**
    ///
    /// 글자가 없는 칸은 `MIN_CELL`(4칸)로 줄어드는 게 맞다 — 그런데 그림이
    /// 들어 있으면 `[그` 까지만 보인다. `min_width` 로 원하는 폭을 먼저
    /// 말해 두면 표가 그만큼 준다.
    #[test]
    fn a_cell_holding_art_keeps_its_width_and_the_art_is_not_wrapped() {
        let art: Vec<String> = vec!["⣿⣿⣿⣿⣿⣿⣿⣿".into(), "⠿⠿⠿⠿⠿⠿⠿⠿".into()];
        let t = Table {
            rows: 1,
            cols: 2,
            cells: vec![
                Cell {
                    row: 0,
                    col: 0,
                    row_span: 1,
                    col_span: 1,
                    text: String::new(),
                    art: art.clone(),
                    min_width: 16,
                    doc_width: 0,
                    align: Default::default(),
                },
                Cell {
                    row: 0,
                    col: 1,
                    row_span: 1,
                    col_span: 1,
                    text: String::new(),
                    art: Vec::new(),
                    min_width: 0,
                    doc_width: 0,
                    align: Default::default(),
                },
            ],
        };
        let widths = t.cell_widths(40);
        assert!(
            widths[0] >= 16,
            "그림 칸이 {}칸으로 줄었다 — 16칸을 달라고 했다",
            widths[0]
        );

        let lines = t.render(40);
        // 그림 두 줄이 그대로 들어가 있다 (접히지 않았다)
        assert!(
            lines.iter().any(|l| l.contains("⣿⣿⣿⣿⣿⣿⣿⣿")),
            "그림 줄이 접혔거나 사라졌다:\n{}",
            lines.join("\n")
        );
        // 표의 모든 줄 폭이 같다 — 그림을 넣어도 어긋나지 않는다
        let w0 = display_width(&lines[0]);
        for (i, l) in lines.iter().enumerate() {
            assert_eq!(display_width(l), w0, "{i}번째 줄 폭이 다르다: {l}");
        }
    }

    fn cell(row: usize, col: usize, text: &str) -> Cell {
        Cell { row, col, row_span: 1, col_span: 1, text: text.into() , art: Vec::new(), min_width: 0, doc_width: 0, align: Default::default() }
    }

    fn plain() -> Table {
        Table {
            rows: 2,
            cols: 2,
            cells: vec![
                cell(0, 0, "기관"),
                cell(0, 1, "금액"),
                cell(1, 0, "농진청"),
                cell(1, 1, "1,200"),
            ],
        }
    }

    /// 줄 폭이 하나라도 어긋나면 화면이 밀린다. 모든 시험이 이걸 본다.
    fn assert_even(lines: &[String]) {
        let first = display_width(&lines[0]);
        for (i, l) in lines.iter().enumerate() {
            assert_eq!(display_width(l), first, "{i}번째 줄 폭이 다르다: {l:?}");
        }
    }

    #[test]
    fn a_plain_grid_has_corners_and_a_cross() {
        let out = plain().render(40);
        assert_even(&out);
        assert!(out[0].starts_with('┌') && out[0].ends_with('┐'), "{:?}", out[0]);
        assert!(out.last().unwrap().starts_with('└'), "{:?}", out.last());
        // 가운데 가로선에 교차점이 있어야 한다
        assert!(out.iter().any(|l| l.contains('┼')), "{out:#?}");
        assert!(out.iter().any(|l| l.contains("기관")), "{out:#?}");
        assert!(out.iter().any(|l| l.contains("1,200")), "{out:#?}");
    }

    #[test]
    fn a_merged_cell_has_no_line_running_through_it() {
        // 첫 행을 두 열로 병합 — 그 행 가운데에 세로선이 없어야 한다
        let t = Table {
            rows: 2,
            cols: 2,
            cells: vec![
                Cell { row: 0, col: 0, row_span: 1, col_span: 2, text: "신청서".into() , art: Vec::new(), min_width: 0, doc_width: 0, align: Default::default() },
                cell(1, 0, "기관"),
                cell(1, 1, "금액"),
            ],
        };
        let out = t.render(40);
        assert_even(&out);
        // 제목 줄: 바깥 테두리만 있고 안쪽 세로선이 없다
        let title = out.iter().find(|l| l.contains("신청서")).expect("제목 줄");
        assert_eq!(
            title.chars().filter(|c| *c == '│').count(),
            2,
            "병합된 칸을 가로지르는 선이 있다: {title:?}"
        );
        // 병합이 끝나는 자리는 ┬ 가 된다 (위에서 내려오는 선이 없고 아래로 갈린다)
        assert!(out.iter().any(|l| l.contains('┬')), "{out:#?}");
    }

    #[test]
    fn a_row_span_keeps_the_left_column_whole() {
        let t = Table {
            rows: 2,
            cols: 2,
            cells: vec![
                Cell { row: 0, col: 0, row_span: 2, col_span: 1, text: "기관".into() , art: Vec::new(), min_width: 0, doc_width: 0, align: Default::default() },
                cell(0, 1, "가"),
                cell(1, 1, "나"),
            ],
        };
        let out = t.render(40);
        assert_even(&out);
        // 가운데 가로선은 오른쪽 열에만 — 왼쪽은 이어진다
        let mid = out
            .iter()
            .find(|l| l.contains('├') || l.contains('└') || l.contains('┼'))
            .expect("가운데 선");
        assert!(mid.contains('├'), "왼쪽 열이 이어져야 한다: {mid:?}");
    }

    #[test]
    fn long_text_wraps_inside_its_cell() {
        let t = Table {
            rows: 1,
            cols: 2,
            cells: vec![
                cell(0, 0, "사업명"),
                cell(0, 1, "차세대 디지털육종 기반 기술개발 및 콩 종자 형질 예측 모델 구축"),
            ],
        };
        let out = t.render(40);
        assert_even(&out);
        assert!(out.len() > 3, "접혀서 여러 줄이 되어야 한다: {out:#?}");
        let joined: String = out.join("");
        for word in ["차세대", "디지털육종", "구축"] {
            assert!(joined.contains(word), "{word} 가 사라졌다: {out:#?}");
        }
    }

    #[test]
    fn a_table_too_wide_for_the_terminal_overflows_instead_of_being_crushed() {
        // 열이 10개면 테두리만 11칸이고 칸마다 최소 폭이 필요하다. 30칸에
        // 넣을 방법이 없다 — 억지로 1칸씩 만들어 못 읽게 하는 쪽이 더 나쁘다.
        // 넘치는 것을 인정하고 페이저의 가로 스크롤(`<` `>`)에 맡긴다.
        let t = Table {
            rows: 1,
            cols: 10,
            cells: (0..10).map(|c| cell(0, c, "내용이 긴 칸")).collect(),
        };
        let out = t.render(30);
        assert_even(&out);
        let floor = 10 * MIN_CELL + 11;
        assert_eq!(
            display_width(&out[0]),
            floor,
            "들어갈 수 없으면 바닥 폭({floor})까지만 줄인다"
        );
    }

    #[test]
    fn a_table_that_fits_uses_the_width_given() {
        let out = plain().render(40);
        assert_even(&out);
        assert!(
            display_width(&out[0]) <= 40,
            "들어갈 수 있으면 주어진 폭을 넘지 않는다: {}",
            display_width(&out[0])
        );
    }

    #[test]
    fn an_empty_cell_is_drawn_as_an_empty_cell() {
        let t = Table {
            rows: 1,
            cols: 2,
            cells: vec![cell(0, 0, "항목"), cell(0, 1, "")],
        };
        let out = t.render(30);
        assert_even(&out);
        assert_eq!(out.len(), 3, "테두리 2줄 + 내용 1줄: {out:#?}");
    }

    #[test]
    fn a_table_with_no_cells_is_not_drawn() {
        let t = Table { rows: 0, cols: 0, cells: Vec::new() };
        assert!(t.render(40).is_empty());
    }

    #[test]
    fn overlapping_spans_do_not_panic() {
        // 깨진 표(두 셀이 같은 칸을 주장)에서도 그리기는 끝나야 한다
        let t = Table {
            rows: 2,
            cols: 2,
            cells: vec![
                Cell { row: 0, col: 0, row_span: 2, col_span: 2, text: "가".into() , art: Vec::new(), min_width: 0, doc_width: 0, align: Default::default() },
                Cell { row: 1, col: 1, row_span: 1, col_span: 1, text: "나".into() , art: Vec::new(), min_width: 0, doc_width: 0, align: Default::default() },
            ],
        };
        let out = t.render(30);
        assert_even(&out);
    }
}
