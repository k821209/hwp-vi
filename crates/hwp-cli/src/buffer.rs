//! 편집 버퍼 — 커서, 쌓인 편집, 되돌리기.
//!
//! 타이핑은 여기 쌓이고 디스크로 가지 않는다. `:w` 에서 [`hwp_core::edit::save`]
//! 가 rhwp 에 일괄 재생하고 독립 증인으로 검증한다(PLAN.md §8-3).
//!
//! 커서는 `(구역, 단락, 오프셋)` 이다 — rhwp 편집 좌표 그대로. 화면 자리에서
//! 좌표를 계산하지 않고, 좌표에서 화면 자리를 계산한다. 반대로 하면 접힘 위치가
//! 바뀔 때마다 커서가 흔들린다.

use hwp_core::ScreenDoc;
use hwp_core::edit::{CellAt, Coord, EditLog, Op};
use hwp_core::jamo::Automaton;
use hwp_core::offset;
use hwp_core::wrap::wrap;
use std::collections::{BTreeMap, BTreeSet};

/// 커서 — rhwp 좌표계.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub at: Coord,
    /// 보이는 글자 기준 오프셋 (0 = 맨 앞, 길이 = 맨 끝)
    pub offset: usize,
}

/// 낱말 이동의 방향.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    /// 다음 낱말의 첫 글자 (`w` · `Ctrl-F`)
    Forward,
    /// 앞 낱말의 첫 글자 (`b` · `Ctrl-A`)
    Back,
    /// 지금 낱말의 끝 다음 (`e`)
    End,
}

/// 한글 입력 방식.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ime {
    /// 키를 그대로 넣는다 — 터미널 IME 가 있을 때
    Off,
    /// 두벌식으로 직접 조합한다 — IME 없는 원격 세션에서 한글을 치는 길
    Dubeolsik,
}

/// 아직 하나의 연산으로 접히지 않은 입력.
#[derive(Debug, Clone)]
struct Pending {
    at: Coord,
    /// 입력이 시작된 오프셋
    start: usize,
    text: String,
}

/// 되돌리기 한 단계 — **버퍼를 이전 상태로 돌리는 데 필요한 것.**
///
/// [`EditLog`] 와 **일부러 따로 둔다.** 로그는 `:w` 에서 rhwp 에 재생할
/// 명령이고, 거기에는 "무엇을 지웠는지" 가 없다(`delete-text --count 3`).
/// 둘을 한 자료로 쓰려다 지운 것을 되돌릴 수 없게 됐었다 — `dd` 뒤 `u` 가
/// "되돌릴 수 없다" 고 답했다. 쓰는 쪽과 되돌리는 쪽이 필요한 것이 다르다.
#[derive(Debug, Clone)]
enum Back {
    /// 넣은 글자를 뺀다
    RemoveText { at: Coord, offset: usize, count: usize },
    /// 지운 글자를 되돌려 넣는다
    RestoreText { at: Coord, offset: usize, text: String },
    /// 넣은 단락을 뺀다
    RemovePara { at: Coord },
    /// 지운 단락을 되돌려 넣는다
    RestorePara { at: Coord, text: String },
    /// 표 칸을 전 글자로 되돌린다
    RestoreCell { at: CellAt, text: String },
}

pub struct Buffer {
    /// 표 칸의 **보이는** 글자. 편집이 여기 반영된다.
    ///
    /// 단락과 같은 자리에 둔다 — 칸 편집만 바깥(`ScreenDoc`)에 두면 되돌리기가
    /// 칸을 못 되돌린다. 화면은 이 값을 `ScreenDoc` 으로 흘려 그린다.
    cells: BTreeMap<CellAt, String>,
    /// **책갈피 열 개** (`Ctrl-K 1~0` 넣기 · `Ctrl-Q 1~0` 이동).
    ///
    /// 워드스타의 마크이고 HWP 가 지금도 그 키를 쓴다. 긴 문서에서 "앞의 그
    /// 표" 로 돌아가는 일이 잦은데, 쪽 번호를 외우게 하는 것보다 자리를
    /// 적어 두는 쪽이 맞다.
    marks: [Option<(Coord, usize)>; 10],
    /// **지금 고치고 있는 칸.** `Some` 이면 글자 넣기·지우기·커서가 단락이
    /// 아니라 이 칸에 걸린다.
    ///
    /// 커서 자리(`cursor.offset`)를 단락과 **같이 쓴다**. 그래야 `type_key`·
    /// `backspace`·두벌식 조합을 베끼지 않고 그대로 쓸 수 있다 — 베끼면
    /// 반드시 한쪽만 고쳐지는 날이 온다.
    editing_cell: Option<CellAt>,
    /// 칸을 고치기 시작할 때의 글자. 한 번의 고치기가 **되돌리기 한 칸**이다.
    pending_cell: Option<(CellAt, String)>,
    /// 좌표별 **보이는** 본문 — 인라인 컨트롤을 뺀 글자들. 편집이 여기 반영된다.
    ///
    /// 컨트롤을 담으면 안 된다. rhwp 의 `--offset` 은 컨트롤을 세지 않으므로
    /// (`hwp_core::offset` 의 실측), 버퍼가 컨트롤을 들고 있으면 커서 오프셋이
    /// rhwp 가 보는 자리와 어긋난다. 실제로 그렇게 짰다가 종단 시험에서 걸렸다.
    texts: BTreeMap<Coord, String>,
    /// 단락 순서 (구역, 단락) — 커서 이동에 쓴다
    order: Vec<Coord>,
    pub cursor: Cursor,
    log: EditLog,
    pending: Option<Pending>,
    pub jamo: Automaton,
    pub ime: Ime,
    /// 편집된 단락 — 조판이 더 이상 맞지 않는 곳
    dirty: BTreeSet<Coord>,
    /// `log` 와 **같은 길이로** 쌓인다. 한 칸씩 같이 꺼낸다.
    back: Vec<Back>,
    /// 되돌린 것들 — `Ctrl-r` 로 다시 한다. 새로 고치면 버린다.
    redo: Vec<(Op, Back)>,
    /// `yy`·`dd` 가 담는 자리. vi 처럼 **지우기도 담는다.**
    register: Option<String>,
    /// **눈에 보이는 상태가 몇 번 바뀌었나.**
    ///
    /// 화면을 다시 그릴지 판단하는 신호다. 전에는 *연산 개수*(`pending_edits`)로
    /// 판단했는데, 같은 단락에서 이어 치는 글자는 **한 연산으로 모이므로**
    /// 개수가 안 바뀐다 — 그래서 첫 글자만 보이고 그 뒤는 Enter 를 눌러야
    /// 나타났다(2026-10-10 보고, 한글·영문 둘 다). 조합 중인 자모도 같다:
    /// 커밋되기 전에는 연산이 생기지 않는데 화면에는 보여야 한다.
    revision: u64,
    /// **칸 글자가 몇 번 바뀌었나.** 따로 세는 이유: 칸이 바뀌면 격자를 전부
    /// 다시 그려야 하는데(표 서른 개·셀 2천 개인 문서가 있다), 본문 글자 하나
    /// 칠 때마다 그러면 낭비다.
    cell_revision: u64,
}

/// 칸 글자를 **문단 목록**으로 나눈다.
///
/// 증인이 칸 안 문단을 `\n` 으로 이어 읽으므로, 거꾸로 `\n` 으로 나누면 원래
/// 문단 경계가 돌아온다. 사람 쪽에서 여러 문단 칸을 고칠 때 이걸 쓰지 않으면
/// 저장하면서 문단이 하나로 접힌다 — 문서를 조용히 고치는 일이다.
fn split_cell_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    text.split('\n').map(String::from).collect()
}

impl Buffer {
    pub fn from_doc(doc: &ScreenDoc) -> Self {
        // **원문은 `doc.paras` 에서 온다.** 블록의 접힌 줄을 되붙여 쓰면 안
        // 된다 — `wrap` 은 줄을 끊은 자리의 공백을 버리므로(표시에는 그게
        // 맞다) 되붙이면 접힌 자리마다 한 칸이 사라진다. 실측: 코퍼스 단락
        // 200개. 그러면 커서 오프셋이 그만큼 앞으로 밀려 **엉뚱한 자리에**
        // 글자가 들어가고, 자가검증은 기대값과 재생이 같은 틀린 오프셋을
        // 쓰므로 둘이 일치해 통과해 버린다.
        //
        // 컨트롤은 떼어 낸다 — rhwp 의 `--offset` 이 컨트롤을 세지
        // 않으므로(`hwp_core::offset`), 버퍼가 들고 있으면 커서 오프셋이
        // 어긋난다. 어떤 개체가 있었는지는 화면 모델의 `objects` 가 안다.
        let mut texts: BTreeMap<Coord, String> = doc
            .paras
            .iter()
            .map(|(k, v)| (*k, offset::visible(v)))
            .collect();
        // 쪽에 놓이지 않은 좌표는 그릴 수 없으므로 버퍼에도 두지 않는다.
        let drawn: std::collections::BTreeSet<Coord> = doc
            .pages
            .iter()
            .flat_map(|p| p.blocks.iter())
            .filter_map(|b| b.para_index.map(|i| (b.section, i)))
            .collect();
        texts.retain(|k, _| drawn.contains(k));
        let mut order: Vec<Coord> = texts.keys().copied().collect();
        order.sort();
        // 표 칸도 담는다 — 편집과 되돌리기가 단락과 같은 길을 쓰게
        let cells: BTreeMap<CellAt, String> = doc
            .tables
            .iter()
            .flat_map(|t| {
                let idx = t.index;
                t.table
                    .cells
                    .iter()
                    .map(move |c| ((idx, c.row, c.col), c.text.clone()))
            })
            .collect();
        let cursor = Cursor {
            at: order.first().copied().unwrap_or((0, 0)),
            offset: 0,
        };
        Self {
            texts,
            order,
            cells,
            cursor,
            log: EditLog::new(),
            pending: None,
            jamo: Automaton::new(),
            ime: Ime::Off,
            dirty: BTreeSet::new(),
            back: Vec::new(),
            redo: Vec::new(),
            register: None,
            marks: [None; 10],
            editing_cell: None,
            pending_cell: None,
            revision: 0,
            cell_revision: 0,
        }
    }

    /// 지금 고치고 있는 칸 (없으면 본문 단락을 고치는 중이다).
    pub fn editing_cell(&self) -> Option<CellAt> {
        self.editing_cell
    }

    /// 책갈피를 넣는다 (`Ctrl-K <숫자>`). 0 은 열 번째 자리다.
    pub fn set_mark(&mut self, slot: usize) -> Option<Coord> {
        // 표 칸을 고치는 중에는 자리를 적지 않는다 — 돌아갈 좌표가 단락이
        // 아니라서 되돌아가도 엉뚱한 곳이다.
        if self.editing_cell.is_some() {
            return None;
        }
        self.marks[slot % 10] = Some((self.cursor.at, self.cursor.offset));
        Some(self.cursor.at)
    }

    /// 책갈피로 간다 (`Ctrl-Q <숫자>`). 그 단락이 사라졌으면 `None`.
    pub fn goto_mark(&mut self, slot: usize) -> Option<Coord> {
        let (at, off) = self.marks[slot % 10]?;
        // **사라진 자리로 뛰지 않는다.** 단락을 지웠으면 그 좌표에는 다른
        // 글이 있다 — 뛰어 놓고 "여기였나" 하게 만드는 쪽이 더 나쁘다.
        if !self.texts.contains_key(&at) {
            self.marks[slot % 10] = None;
            return None;
        }
        self.commit_jamo();
        self.cursor.at = at;
        self.cursor.offset = off.min(self.text_at(at).chars().count());
        self.revision += 1;
        Some(at)
    }

    /// 적어 둔 책갈피 자리들 — `(숫자, 좌표)`.
    pub fn mark_list(&self) -> Vec<(usize, Coord)> {
        self.marks
            .iter()
            .enumerate()
            .filter_map(|(i, m)| m.map(|(at, _)| (i, at)))
            .collect()
    }

    /// **커서에서 낱말 하나를 지운다** (`Ctrl-T`, 워드스타).
    ///
    /// vi 의 `dw` 자리를 이 키가 채운다. 한국어는 띄어쓰기가 경계라 `w` 와
    /// 같은 셈을 쓴다 — 두 군데서 따로 세면 `w` 가 가는 자리와 `Ctrl-T` 가
    /// 지우는 자리가 달라진다.
    pub fn delete_word(&mut self) -> usize {
        self.commit_jamo();
        let at = self.cursor.at;
        let start = self.cursor.offset;
        let len = self.here().chars().count();
        if start >= len {
            return 0;
        }
        // **`w` 가 가는 자리까지 지운다 — 그 셈을 다시 쓰지 않는다.**
        //
        // 처음엔 여기서 따로 셌고, 낱말만 지우고 뒤 공백을 남겨 `w` 와 다른
        // 자리가 되었다(`첫째 낱말` 에서 `Ctrl-T` 가 ` 낱말` 을 남겼다).
        // 같은 일을 두 군데서 세면 반드시 어긋난다.
        let keep = self.cursor;
        self.move_word(Word::Forward);
        let target = if self.cursor.at == at { self.cursor.offset } else { len };
        self.cursor = keep;
        let n = target.saturating_sub(start);
        if n > 0 {
            self.delete_chars(n);
        }
        n
    }

    /// **커서에서 단락 끝까지 지운다** (`Ctrl-Q Y`, 워드스타).
    pub fn delete_to_end(&mut self) -> usize {
        self.commit_jamo();
        let n = self.here().chars().count().saturating_sub(self.cursor.offset);
        if n > 0 {
            self.delete_chars(n);
        }
        n
    }

    /// 지금 커서가 올라가 있는 글자 — 단락이든 칸이든.
    fn here(&self) -> &str {
        match self.editing_cell {
            Some(at) => self.cells.get(&at).map(|s| s.as_str()).unwrap_or(""),
            None => self.text_at(self.cursor.at),
        }
    }


    /// **칸 고치기를 시작한다.** 커서를 칸 글자 끝에 둔다.
    ///
    /// 여러 단락이 든 칸은 거절한다 — 한 줄로 접어 넣으면 조용히 문서를
    /// 망친다. 그 경우는 `hwp set-cell` 이 할 일이다.
    pub fn begin_cell_edit(&mut self, at: CellAt) -> Result<(), String> {
        self.commit_jamo();
        self.flush_pending();
        let Some(cur) = self.cells.get(&at) else {
            return Err(format!("없는 칸입니다: 표{} ({},{})", at.0, at.1, at.2));
        };
        if cur.contains('\n') {
            return Err(format!(
                "칸에 단락이 여럿이라 화면에서 고칠 수 없습니다 — `hwp set-cell {}:{}:{}=…`",
                at.0, at.1, at.2
            ));
        }
        self.pending_cell = Some((at, cur.clone()));
        self.editing_cell = Some(at);
        self.cursor.offset = cur.chars().count();
        self.revision += 1;
        Ok(())
    }

    /// 칸 고치기를 끝낸다 (`Esc`). 바뀐 것이 있으면 연산 하나로 쌓는다.
    pub fn end_cell_edit(&mut self) {
        self.commit_jamo();
        self.flush_pending();
        self.editing_cell = None;
        self.revision += 1;
    }

    /// **칸을 그릴 글자** — 조합 중인 자모를 커서 자리에 끼워 넣은 것.
    ///
    /// 담아 두는 값에는 조합 중인 자모가 들어가면 안 된다(확정되지 않았다).
    /// 그런데 화면에는 보여야 한다 — 안 보이면 한글을 칠 때 손이 허공을
    /// 짚는다. 그래서 그리는 글자만 따로 만든다.
    pub fn cell_display(&self, at: CellAt) -> String {
        let text = self.cells.get(&at).cloned().unwrap_or_default();
        if self.editing_cell != Some(at) {
            return text;
        }
        let pre = self.jamo.preedit();
        if pre.is_empty() {
            return text;
        }
        let chars: Vec<char> = text.chars().collect();
        let cut = self.cursor.offset.min(chars.len());
        let mut out: String = chars[..cut].iter().collect();
        out.push_str(&pre);
        out.extend(&chars[cut..]);
        out
    }

    /// 고치는 중인 칸에서 커서가 **몇 번째 글자**에 있나 (조합 중인 자모 포함).
    pub fn cell_cursor_offset(&self) -> usize {
        self.cursor.offset + self.jamo.preedit().chars().count()
    }

    /// 표 칸의 지금 글자.
    pub fn cell_text(&self, at: CellAt) -> Option<&str> {
        self.cells.get(&at).map(|s| s.as_str())
    }

    /// 바뀐 칸들 — 화면이 이걸 `ScreenDoc` 으로 흘려 그린다.
    pub fn cells(&self) -> &BTreeMap<CellAt, String> {
        &self.cells
    }

    /// **표 칸에 쓴다.** 되돌리기 기록을 같이 쌓는다.
    ///
    /// 셀 편집을 바깥에만 두면 `u` 가 칸을 못 되돌린다 — 그래서 단락과 같은
    /// 자리에 둔다. 저장은 `Op::SetCell` 로 평소 경로를 탄다(자가검증 포함).
    pub fn set_cell(&mut self, at: CellAt, text: &str) -> Result<(), String> {
        self.commit_jamo();
        self.flush_pending();
        let Some(cur) = self.cells.get(&at) else {
            return Err(format!("없는 칸입니다: 표{} ({},{})", at.0, at.1, at.2));
        };
        if cur == text {
            return Err("바뀐 것이 없습니다".into());
        }
        let was = cur.clone();
        self.revision += 1;
        self.cell_revision += 1;
        self.cells.insert(at, text.to_string());
        self.record(
            Op::SetCell { table: at.0, row: at.1, col: at.2, lines: split_cell_lines(text) },
            Back::RestoreCell { at, text: was },
        );
        Ok(())
    }

    /// 재생 로그와 되돌리기 기록을 **같이** 쌓는다. 둘이 어긋나면 되돌리기가
    /// 엉뚱한 것을 되돌린다 — 그래서 한 자리에서만 쌓는다.
    fn record(&mut self, op: Op, back: Back) {
        self.log.push(op);
        self.back.push(back);
        // 새로 고치면 다시하기 줄은 버린다 (vi 와 같다)
        self.redo.clear();
    }

    pub fn text_at(&self, at: Coord) -> &str {
        self.texts.get(&at).map(|s| s.as_str()).unwrap_or("")
    }

    pub fn is_dirty(&self) -> bool {
        !self.log.is_empty() || self.pending.is_some()
    }

    pub fn dirty_paragraphs(&self) -> &BTreeSet<Coord> {
        &self.dirty
    }

    /// 눈에 보이는 상태의 판 번호. 이 값이 바뀌면 화면을 다시 그려야 한다.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 칸 글자의 판 번호. 바뀌면 격자를 다시 그려야 한다.
    pub fn cell_revision(&self) -> u64 {
        self.cell_revision
    }

    pub fn pending_edits(&self) -> usize {
        self.log.len() + usize::from(self.pending.is_some())
    }

    /// 접은 줄들 — 화면에 그릴 것.
    pub fn wrapped(&self, at: Coord, width: usize) -> Vec<String> {
        wrap(self.text_at(at), width.max(1))
    }

    /// 커서 오프셋이 접힌 줄의 몇 번째 줄, 몇 칸인지.
    ///
    /// **접은 줄의 자리를 받아 쓴다**(`wrap_spans`). 전에는 줄들의 글자 수를
    /// 더해 가며 찾았는데, 접는 쪽이 끊은 자리의 공백을 **버리므로** 그 합이
    /// 원문보다 짧다(실측: 4줄 77자 vs 원문 80자). 그래서 커서가 지나온 접힘
    /// 수만큼 오른쪽으로 밀렸고, 단락 끝 쪽 오프셋은 아예 못 찾아 마지막 줄의
    /// 0열로 떨어졌다.
    pub fn cursor_screen(&self, width: usize) -> (usize, usize) {
        let spans = hwp_core::wrap::wrap_spans(self.text_at(self.cursor.at), width.max(1));
        let off = self.cursor.offset;
        let mut last = (0usize, 0usize);
        for (row, sp) in spans.iter().enumerate() {
            // **버린 글자 자리**(끊긴 공백이나 강제 줄바꿈)에 커서가 있으면
            // 그 글자는 어느 줄에도 그려지지 않는다 — 앞 줄의 끝에 둔다.
            if off < sp.start {
                return last;
            }
            if off <= sp.start + sp.len {
                let cells: usize = sp
                    .text
                    .chars()
                    .take(off - sp.start)
                    .map(|c| hwp_core::wrap::display_width(&c.to_string()))
                    .sum();
                return (row, cells);
            }
            last = (row, hwp_core::wrap::display_width(&sp.text));
        }
        last
    }

    // ---- 이동 ----

    pub fn max_offset(&self) -> usize {
        self.here().chars().count()
    }

    pub fn move_horizontal(&mut self, delta: isize) {
        self.commit_jamo();
        let max = self.max_offset() as isize;
        self.cursor.offset = (self.cursor.offset as isize + delta).clamp(0, max) as usize;
    }

    pub fn move_paragraph(&mut self, delta: isize) {
        self.commit_jamo();
        let Some(i) = self.order.iter().position(|c| *c == self.cursor.at) else {
            return;
        };
        let next = (i as isize + delta).clamp(0, self.order.len() as isize - 1) as usize;
        self.cursor.at = self.order[next];
        self.cursor.offset = self.cursor.offset.min(self.max_offset());
    }

    /// 낱말 단위 이동 — `w` `b` `e` 와 도스 `Ctrl-F` `Ctrl-A`.
    ///
    /// **한국어는 띄어쓰기가 낱말 경계다.** vi 는 낱말을 "글자 묶음 / 기호
    /// 묶음 / 공백" 으로 가르는데, 한글은 조사가 붙어 한 덩이로 움직이는 편이
    /// 자연스럽다. 그래서 **공백으로만** 가른다 — `과업지시서를` 은 한 낱말이다.
    ///
    /// `Forward` 는 다음 낱말의 **첫 글자**, `Back` 은 앞 낱말의 첫 글자,
    /// `End` 는 지금 낱말의 **끝 글자 다음**이다 (vi 와 같은 자리).
    pub fn move_word(&mut self, how: Word) {
        self.commit_jamo();
        let chars: Vec<char> = self.text_at(self.cursor.at).chars().collect();
        let n = chars.len();
        let at = self.cursor.offset.min(n);
        let space = |i: usize| chars.get(i).is_some_and(|c| c.is_whitespace());

        self.cursor.offset = match how {
            Word::Forward => {
                let mut i = at;
                // 지금 낱말을 지나고
                while i < n && !space(i) {
                    i += 1;
                }
                // 공백을 지나면 다음 낱말의 첫 글자
                while i < n && space(i) {
                    i += 1;
                }
                i
            }
            Word::Back => {
                let mut i = at;
                // 바로 앞의 공백을 지나고
                while i > 0 && space(i - 1) {
                    i -= 1;
                }
                // 그 낱말의 첫 글자까지
                while i > 0 && !space(i - 1) {
                    i -= 1;
                }
                i
            }
            Word::End => {
                let mut i = at;
                // 공백에 서 있으면 다음 낱말로
                while i < n && space(i) {
                    i += 1;
                }
                // 그 낱말 끝 다음
                while i < n && !space(i) {
                    i += 1;
                }
                i
            }
        };
    }

    pub fn move_line_start(&mut self) {
        self.commit_jamo();
        self.cursor.offset = 0;
    }

    pub fn move_line_end(&mut self) {
        self.commit_jamo();
        self.cursor.offset = self.max_offset();
    }

    pub fn goto(&mut self, at: Coord) {
        self.commit_jamo();
        if self.texts.contains_key(&at) {
            self.cursor.at = at;
            self.cursor.offset = 0;
        }
    }

    // ---- 입력 ----

    /// 조합 중인 글자를 확정해 버퍼에 넣는다. 커서를 옮기기 전에 반드시 부른다.
    pub fn commit_jamo(&mut self) {
        self.revision += 1;
        let text = self.jamo.flush();
        if !text.is_empty() {
            self.insert_str(&text);
        }
    }

    /// 글자를 커서 자리에 넣는다.
    pub fn insert_str(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.revision += 1;
        let at = self.cursor.at;
        let offset = self.cursor.offset;
        // **칸을 고치는 중이면 칸에 넣는다.** 연산은 `flush_pending` 이
        // `SetCell` 하나로 만든다 — 글자마다 쌓지 않는다.
        if let Some(cell) = self.editing_cell {
            let Some(cur) = self.cells.get_mut(&cell) else { return };
            let chars: Vec<char> = cur.chars().collect();
            let cut = offset.min(chars.len());
            let mut next: String = chars[..cut].iter().collect();
            next.push_str(s);
            next.extend(&chars[cut..]);
            *cur = next;
            self.cursor.offset = cut + s.chars().count();
            self.cell_revision += 1;
            return;
        }
        let Some(cur) = self.texts.get_mut(&at) else { return };
        let chars: Vec<char> = cur.chars().collect();
        let cut = offset.min(chars.len());
        let mut next: String = chars[..cut].iter().collect();
        next.push_str(s);
        next.extend(&chars[cut..]);
        *cur = next;

        match self.pending.as_mut() {
            // 같은 단락에서 이어 치는 중이면 한 연산으로 모은다
            Some(p) if p.at == at && p.start + p.text.chars().count() == offset => {
                p.text.push_str(s);
            }
            _ => {
                self.flush_pending();
                self.pending = Some(Pending { at, start: cut, text: s.to_string() });
            }
        }
        self.cursor.offset = cut + s.chars().count();
        self.dirty.insert(at);
    }

    /// 키 하나를 입력 모드에서 처리한다.
    pub fn type_key(&mut self, key: char) {
        // 조합 중인 자모도 화면에 보여야 한다 — 커밋 전에는 연산이 안 생긴다
        self.revision += 1;
        match self.ime {
            Ime::Off => self.insert_str(&key.to_string()),
            Ime::Dubeolsik => {
                let step = self.jamo.push_key(key);
                if !step.commit.is_empty() {
                    self.insert_str(&step.commit);
                }
                if let Some(c) = step.passthrough {
                    self.insert_str(&c.to_string());
                }
            }
        }
    }

    /// 입력 모드의 지우기. 조합 중이면 자모를 되돌리고, 아니면 글자를 지운다.
    pub fn backspace(&mut self) {
        self.revision += 1;
        if self.ime == Ime::Dubeolsik && self.jamo.backspace() {
            return;
        }
        if self.cursor.offset == 0 {
            return;
        }
        self.cursor.offset -= 1;
        self.delete_chars(1);
    }

    /// 커서 자리에서 `count` 글자를 지운다.
    pub fn delete_chars(&mut self, count: usize) {
        self.revision += 1;
        self.commit_jamo();
        let at = self.cursor.at;
        let offset = self.cursor.offset;
        if let Some(cell) = self.editing_cell {
            let Some(cur) = self.cells.get_mut(&cell) else { return };
            let chars: Vec<char> = cur.chars().collect();
            if offset >= chars.len() {
                return;
            }
            let n = count.min(chars.len() - offset);
            let mut next: String = chars[..offset].iter().collect();
            next.extend(&chars[offset + n..]);
            *cur = next;
            self.register = Some(chars[offset..offset + n].iter().collect());
            self.cell_revision += 1;
            return;
        }
        let Some(cur) = self.texts.get_mut(&at) else { return };
        let chars: Vec<char> = cur.chars().collect();
        if offset >= chars.len() {
            return;
        }
        let n = count.min(chars.len() - offset);
        let mut next: String = chars[..offset].iter().collect();
        next.extend(&chars[offset + n..]);
        *cur = next;

        self.flush_pending();
        // 지운 글자를 들고 있어야 되돌릴 수 있다
        let gone: String = chars[offset..offset + n].iter().collect();
        self.register = Some(gone.clone());
        self.record(
            Op::DeleteText { at, offset, count: n },
            Back::RestoreText { at, offset, text: gone },
        );
        self.dirty.insert(at);
    }

    /// 커서가 있는 단락의 좌표들 — 사다리를 문서에서 읽을 때 쓴다.
    pub fn coords(&self) -> &[Coord] {
        &self.order
    }

    /// 단락의 글자를 **통째로** 바꾼다. 단계 옮기기(`Tab`)가 쓴다.
    ///
    /// 지우고 넣는 두 연산으로 쌓는다 — `edit::Op` 에 "통째로 바꾸기" 가 없고,
    /// 만들지도 않았다. 지우기+넣기는 이미 코퍼스로 검증된 경로이고, 새 연산을
    /// 더하면 그 경로를 또 검증해야 한다. 되돌리기도 두 번이면 제자리로 온다.
    pub fn replace_paragraph(&mut self, at: Coord, text: &str) {
        self.revision += 1;
        self.commit_jamo();
        self.flush_pending();
        let Some(cur) = self.texts.get(&at) else { return };
        let old = cur.clone();
        if old == text {
            return;
        }
        let n = old.chars().count();
        if n > 0 {
            self.record(
                Op::DeleteText { at, offset: 0, count: n },
                Back::RestoreText { at, offset: 0, text: old.clone() },
            );
        }
        if !text.is_empty() {
            self.record(
                Op::InsertText { at, offset: 0, text: text.to_string() },
                Back::RemoveText { at, offset: 0, count: text.chars().count() },
            );
        }
        self.texts.insert(at, text.to_string());
        self.dirty.insert(at);
        // 커서는 글자 수가 줄었을 수 있으니 안으로 끌어온다
        if self.cursor.at == at {
            self.cursor.offset = self.cursor.offset.min(text.chars().count());
        }
    }

    /// 커서가 있는 단락을 담는다 (`yy`) — 지우지 않는다.
    pub fn yank_paragraph(&mut self) -> usize {
        self.commit_jamo();
        let text = self.text_at(self.cursor.at).to_string();
        let n = text.chars().count();
        self.register = Some(text);
        n
    }

    /// 담아 둔 글을 **새 단락으로** 붙인다 (`p` / `P`).
    ///
    /// 글자 단위가 아니라 단락 단위다. 이 편집기의 좌표가 단락이고, 빈 단락을
    /// 넣는 일은 백엔드에 이미 있는 연산(`insert-paragraph`)이라 검증된 길을
    /// 그대로 탄다 — 넣고 나서 글자를 채운다.
    pub fn paste_paragraph(&mut self, before: bool) -> Option<usize> {
        self.revision += 1;
        let text = self.register.clone()?;
        self.commit_jamo();
        self.flush_pending();
        self.open_paragraph(before);
        let at = self.cursor.at;
        if !text.is_empty() {
            self.record(
                Op::InsertText { at, offset: 0, text: text.clone() },
                Back::RemoveText { at, offset: 0, count: text.chars().count() },
            );
            self.texts.insert(at, text.clone());
            self.dirty.insert(at);
        }
        self.cursor = Cursor { at, offset: 0 };
        Some(text.chars().count())
    }

    /// 단락을 지운다.
    pub fn delete_paragraph(&mut self) {
        self.revision += 1;
        self.commit_jamo();
        self.flush_pending();
        let at = self.cursor.at;
        if self.order.len() <= 1 {
            return;
        }
        let gone = self.texts.remove(&at).unwrap_or_default();
        // vi 처럼 지운 단락도 담는다 — `dd` 뒤 `p` 로 옮겨 붙일 수 있다
        self.register = Some(gone.clone());
        self.record(Op::DeleteParagraph { at }, Back::RestorePara { at, text: gone });
        // 같은 구역의 뒤쪽 좌표를 당긴다
        self.reindex(at.0, at.1, -1);
        self.cursor.offset = 0;
        self.cursor.at = *self
            .order
            .iter()
            .find(|c| **c >= at)
            .or_else(|| self.order.last())
            .unwrap_or(&at);
        self.dirty.insert(self.cursor.at);
    }

    /// 커서 단락 앞(`before`)이나 뒤에 빈 단락을 넣는다.
    pub fn open_paragraph(&mut self, before: bool) {
        self.revision += 1;
        self.commit_jamo();
        self.flush_pending();
        let at = self.cursor.at;
        let target = if before { at } else { (at.0, at.1 + 1) };
        self.record(Op::InsertParagraph { at: target }, Back::RemovePara { at: target });
        self.reindex(target.0, target.1, 1);
        self.texts.insert(target, String::new());
        self.order = self.texts.keys().copied().collect();
        self.order.sort();
        self.cursor = Cursor { at: target, offset: 0 };
        self.dirty.insert(target);
    }

    /// 단락을 넣거나 뺀 뒤 좌표를 다시 매긴다.
    fn reindex(&mut self, section: u32, from: u32, delta: i64) {
        let moved: BTreeMap<Coord, String> = self
            .texts
            .iter()
            .map(|((s, i), v)| {
                let key = if *s == section && *i >= from {
                    (*s, (*i as i64 + delta).max(0) as u32)
                } else {
                    (*s, *i)
                };
                (key, v.clone())
            })
            .collect();
        self.texts = moved;
        self.order = self.texts.keys().copied().collect();
        self.order.sort();
        let shifted: BTreeSet<Coord> = self
            .dirty
            .iter()
            .map(|(s, i)| {
                if *s == section && *i >= from {
                    (*s, (*i as i64 + delta).max(0) as u32)
                } else {
                    (*s, *i)
                }
            })
            .collect();
        self.dirty = shifted;
    }

    /// 모아 둔 입력을 연산 하나로 접는다.
    fn flush_pending(&mut self) {
        // **칸 하나를 고친 것은 연산 하나다.** 글자마다 `SetCell` 을 쌓으면
        // 저장 때 같은 칸에 열 번 쓰고, 되돌리기도 글자 단위가 된다.
        if let Some((at, was)) = self.pending_cell.take() {
            let now = self.cells.get(&at).cloned().unwrap_or_default();
            if now != was {
                self.cell_revision += 1;
                self.record(
                    Op::SetCell { table: at.0, row: at.1, col: at.2, lines: split_cell_lines(&now) },
                    Back::RestoreCell { at, text: was },
                );
            }
        }
        if let Some(p) = self.pending.take() {
            let back = Back::RemoveText {
                at: p.at,
                offset: p.start,
                count: p.text.chars().count(),
            };
            self.record(
                Op::InsertText { at: p.at, offset: p.start, text: p.text },
                back,
            );
        }
    }

    /// 되돌리기 — 연산 하나를 물리고 본문을 되돌린다.
    /// 되돌린다 — **지운 것도 되돌아온다.**
    ///
    /// 재생 로그에서 한 칸을 빼서 `:w` 가 그 편집을 보내지 않게 하고, 같이
    /// 쌓아 둔 [`Back`] 으로 버퍼를 되돌린다. 전에는 로그만 보고 되돌렸는데
    /// 로그에는 지운 **내용**이 없어서(`delete-text --count 3`) `dd` 뒤 `u` 가
    /// "되돌릴 수 없다" 고 답했다. 일을 잃는 쪽이었다.
    pub fn undo(&mut self) -> Option<String> {
        self.revision += 1;
        self.commit_jamo();
        self.flush_pending();
        let op = self.log.undo()?;
        let Some(back) = self.back.pop() else {
            // 둘이 어긋났다 — 로그를 되돌려 놓고 멈춘다. 짐작으로 버퍼를
            // 건드리는 쪽이 훨씬 나쁘다.
            self.log.push(op.clone());
            return Some("되돌릴 수 없습니다 — 되돌리기 기록이 편집 기록과 어긋났다".into());
        };
        self.apply_back(&back);
        self.redo.push((op.clone(), back));
        Some(format!("되돌렸습니다 — {}", op.describe()))
    }

    /// 되돌린 것을 다시 한다 (`Ctrl-r`).
    pub fn redo(&mut self) -> Option<String> {
        self.revision += 1;
        self.commit_jamo();
        self.flush_pending();
        let (op, back) = self.redo.pop()?;
        let Some(fwd) = Self::forward(&op) else {
            self.redo.push((op, back));
            return Some("다시 할 수 없습니다 — 버퍼가 만든 편집이 아닙니다".into());
        };
        self.apply_back(&fwd);
        self.log.push(op.clone());
        self.back.push(back);
        Some(format!("다시 했습니다 — {}", op.describe()))
    }

    /// 편집 하나를 **앞으로** 적용하는 데 필요한 것. 다시하기가 쓴다.
    ///
    /// `Back` 을 뒤집는 쪽으로 짜려다 막혔다 — "넣은 글자를 뺀다" 를 뒤집으면
    /// 그 **글자**가 필요한데 `Back` 에는 개수만 있다. 재생 로그의 `Op` 는
    /// 글자를 들고 있으므로(그게 rhwp 에 보낼 내용이니까) 그쪽에서 만든다.
    fn forward(op: &Op) -> Option<Back> {
        Some(match op {
            Op::InsertText { at, offset, text } => Back::RestoreText {
                at: *at,
                offset: *offset,
                text: text.clone(),
            },
            Op::DeleteText { at, offset, count } => Back::RemoveText {
                at: *at,
                offset: *offset,
                count: *count,
            },
            Op::InsertParagraph { at } => {
                Back::RestorePara { at: *at, text: String::new() }
            }
            Op::DeleteParagraph { at } => Back::RemovePara { at: *at },
            // **표 만들기는 버퍼가 다시하기로 재생하지 않는다.**
            //
            // 표가 들어가면 단락 좌표와 표 번호가 같이 밀린다. 그걸 버퍼 안에서
            // 되감는 것은 저장 경로(`edit::expected`)가 하는 셈을 한 번 더
            // 적는 일이고, 두 군데서 세면 어긋난다 — 되돌리기는 `:e!`(디스크에서
            // 다시 읽기)로 간다.
            Op::InsertTable { .. } => return None,
            // **칸 안 부분 편집은 `hwp api` 전용이다.** 화면 편집기는 칸을
            // 고칠 때 `SetCell`(칸 전체) 을 만들고 이 둘은 만들지 않으므로,
            // 다시하기 로그에 들어올 길이 없다. `Back` 에 "칸 문단 n 의
            // m번째부터 k자" 를 더해 두는 것은 쓰이지 않는 셈을 한 벌 더
            // 적는 일이고, 두 군데서 세면 어긋난다.
            Op::InsertTextInCell { .. } | Op::DeleteTextInCell { .. } => return None,
            // **지우기도 버퍼가 다시하기로 재생하지 않는다.** 표를 지우면 단락
            // 좌표와 표 번호가 같이 당겨지는데, 그걸 버퍼 안에서 되감는 것은
            // 저장 경로가 하는 셈을 한 번 더 적는 일이다 (`InsertTable` 과 같은
            // 이유). 되돌리기는 `:e!` 로 간다.
            Op::DeleteTable { .. } | Op::DeleteControl { .. } => return None,
            // **꾸밈도 버퍼가 다시하기로 재생하지 않는다.** 화면 편집기에는
            // 꾸밈을 거는 키가 없어서 이 연산을 만들지 않는다(`hwp api` 전용).
            // `Back` 에 "그 구간의 옛 꾸밈" 을 더해 두는 것은 쓰이지 않는 셈을
            // 한 벌 더 적는 일이다.
            Op::SetDeco { .. } => return None,
            // **칸은 연산 자체가 "이 글자로" 이므로 그대로 앞으로 적용된다.**
            // 전에는 `None` 을 돌려주고 "버퍼가 만든 편집이 아니다" 라고 했다 —
            // 셀 편집을 버퍼가 만들지 않던 때의 말이고, 이제는 만든다.
            Op::SetCell { table, row, col, lines } => Back::RestoreCell {
                at: (*table, *row, *col),
                text: lines.join("\n"),
            },
        })
    }

    fn apply_back(&mut self, b: &Back) {
        match b {
            Back::RestoreCell { at, text } => {
                self.cells.insert(*at, text.clone());
                self.cell_revision += 1;
            }
            Back::RemoveText { at, offset, count } => {
                if let Some(t) = self.texts.get_mut(at) {
                    let chars: Vec<char> = t.chars().collect();
                    let from = (*offset).min(chars.len());
                    let to = (offset + count).min(chars.len());
                    let mut next: String = chars[..from].iter().collect();
                    next.extend(&chars[to..]);
                    *t = next;
                }
                self.cursor = Cursor { at: *at, offset: *offset };
                self.dirty.insert(*at);
            }
            Back::RestoreText { at, offset, text } => {
                if let Some(t) = self.texts.get_mut(at) {
                    let chars: Vec<char> = t.chars().collect();
                    let cut = (*offset).min(chars.len());
                    let mut next: String = chars[..cut].iter().collect();
                    next.push_str(text);
                    next.extend(&chars[cut..]);
                    *t = next;
                }
                self.cursor = Cursor { at: *at, offset: offset + text.chars().count() };
                self.dirty.insert(*at);
            }
            Back::RemovePara { at } => {
                self.texts.remove(at);
                self.reindex(at.0, at.1, -1);
                self.cursor = Cursor { at: *self.order.first().unwrap_or(at), offset: 0 };
            }
            Back::RestorePara { at, text } => {
                // 뒤 좌표를 밀고 그 자리에 되돌려 넣는다
                self.reindex(at.0, at.1, 1);
                self.texts.insert(*at, text.clone());
                self.order = self.texts.keys().copied().collect();
                self.order.sort();
                self.cursor = Cursor { at: *at, offset: 0 };
                self.dirty.insert(*at);
            }
        }
    }

    pub fn log(&mut self) -> &EditLog {
        self.flush_pending();
        &self.log
    }

    pub fn mark_saved(&mut self) {
        self.log.clear();
        self.pending = None;
        self.dirty.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwp_core::screen::{Block, Column, Page};

    fn doc() -> ScreenDoc {
        ScreenDoc {
            paras: Default::default(),
            path: "/tmp/t.hwp".into(),
            format: "hwp5".into(),
            page_count: 1,
            para_count: 2,
            section_counts: vec![2],
            outline: Vec::new(),
            tables: Vec::new(),
            body_width: 0,
            indents: Default::default(),
            aligns: Default::default(),
            runs: Default::default(),
            objects: Default::default(),
            column: Column { cells: 40, measured: None },
            pages: vec![Page {
                number: 1,
                printed: Some(1),
                section: 0,
                blocks: vec![
                    Block {
                        section: 0,
                        para_index: Some(0),
                        lines: vec!["과업명".into()],
                        continues: false,
                        continued: false,
                    },
                    Block {
                        section: 0,
                        para_index: Some(1),
                        lines: vec!["소요예산".into()],
                        continues: false,
                        continued: false,
                    },
                ],
            }],
        }.with_paras_from_blocks()
    }

    /// **접힌 단락에서 커서가 글자 위에 서야 한다.**
    ///
    /// 전에는 접은 줄들의 글자 수를 더해 가며 찾았는데, 접는 쪽이 끊은 자리의
    /// 공백을 **버리므로** 그 합이 원문보다 짧다(실측: 4줄 77자 vs 원문 80자).
    /// 그래서 커서가 지나온 접힘 수만큼 오른쪽으로 밀리고, 단락 끝 쪽 오프셋은
    /// 아예 못 찾아 **마지막 줄의 0열로 떨어졌다** — 뒤로 갈수록 커서가
    /// 앞으로 되돌아가는 꼴이다.
    #[test]
    fn 접힌_단락에서_커서가_글자_위에_선다() {
        let text = " ○ 여름철 고온 대응 관수 기술을 적용한 노지 재배 안정성 향상 융합 과제의 세부 추진 계획과 연차별 목표";
        let mut d = doc();
        d.paras.insert((0, 0), text.to_string());
        let mut b = Buffer::from_doc(&d);
        b.cursor.at = (0, 0);

        let width = 40usize;
        let spans = hwp_core::wrap::wrap_spans(text, width);
        assert!(spans.len() >= 3, "이 시험은 접히는 단락이어야 뜻이 있다");

        let n = text.chars().count();
        let mut prev: Option<(usize, usize)> = None;
        for off in 0..=n {
            b.cursor.offset = off;
            let (row, cells) = b.cursor_screen(width);
            assert!(row < spans.len(), "오프셋 {off}: 없는 줄 {row} 을 가리킨다");
            // **앞으로만 간다.** 오프셋이 커지면 (줄, 칸) 도 커져야 한다 —
            // 되돌아가는 것이 옛 결함의 증상이었다.
            if let Some(p) = prev {
                assert!(
                    (row, cells) >= p,
                    "오프셋 {off}: 커서가 되돌아갔다 {p:?} → {:?}",
                    (row, cells)
                );
            }
            prev = Some((row, cells));
            // **그 자리에 그려지는 글자가 그 오프셋의 글자여야 한다.**
            // (버린 글자 자리는 앞 줄 끝에 놓이므로 건너뛴다)
            let sp = &spans[row];
            if off >= sp.start && off < sp.start + sp.len {
                let drawn: usize = sp
                    .text
                    .chars()
                    .take(off - sp.start)
                    .map(|c| hwp_core::wrap::display_width(&c.to_string()))
                    .sum();
                assert_eq!(cells, drawn, "오프셋 {off}: 칸이 어긋난다");
            }
        }

        // **마지막 글자는 마지막 줄에 있다.** 전에는 여기서 0열로 떨어졌다.
        b.cursor.offset = n;
        let (row, cells) = b.cursor_screen(width);
        assert_eq!(row, spans.len() - 1, "단락 끝이 마지막 줄이 아니다");
        assert_eq!(
            cells,
            hwp_core::wrap::display_width(&spans[spans.len() - 1].text),
            "단락 끝이 마지막 줄의 끝이 아니다"
        );
    }

    #[test]
    fn typing_collapses_into_one_op() {
        let mut b = Buffer::from_doc(&doc());
        b.cursor.offset = 3;
        for c in "수정".chars() {
            b.insert_str(&c.to_string());
        }
        assert_eq!(b.text_at((0, 0)), "과업명수정");
        // 이어 친 글자는 연산 하나로 접힌다 — rhwp 호출을 글자마다 하지 않는다
        assert_eq!(b.log().ops().len(), 1);
        assert_eq!(
            b.log().ops()[0],
            Op::InsertText { at: (0, 0), offset: 3, text: "수정".into() }
        );
    }

    #[test]
    fn dubeolsik_composes_while_typing() {
        let mut b = Buffer::from_doc(&doc());
        b.ime = Ime::Dubeolsik;
        b.cursor.offset = 0;
        for k in "rhkdjq".chars() {
            b.type_key(k);
        }
        // 마지막 음절은 조합 중 — 아직 버퍼에 없다
        assert_eq!(b.text_at((0, 0)), "과과업명");
        assert_eq!(b.jamo.preedit(), "업");
        b.commit_jamo();
        assert_eq!(b.text_at((0, 0)), "과업과업명");
    }

    #[test]
    fn moving_the_cursor_commits_the_syllable_in_progress() {
        let mut b = Buffer::from_doc(&doc());
        b.ime = Ime::Dubeolsik;
        for k in "rk".chars() {
            b.type_key(k);
        }
        assert_eq!(b.text_at((0, 0)), "과업명", "조합 중에는 아직 안 들어간다");
        b.move_paragraph(1);
        assert_eq!(b.text_at((0, 0)), "가과업명", "커서를 옮기면 확정된다");
    }

    #[test]
    fn backspace_walks_back_through_jamo_then_characters() {
        let mut b = Buffer::from_doc(&doc());
        b.ime = Ime::Dubeolsik;
        b.cursor.offset = 1; // '과' 뒤 — 지울 글자가 앞에 있다
        for k in "rks".chars() {
            b.type_key(k);
        }
        assert_eq!(b.jamo.preedit(), "간");
        b.backspace();
        assert_eq!(b.jamo.preedit(), "가", "먼저 자모를 되돌린다");
        b.backspace();
        b.backspace();
        assert_eq!(b.jamo.preedit(), "");
        assert_eq!(b.text_at((0, 0)), "과업명", "조합만 지워졌고 본문은 그대로");
        // 조합이 비면 버퍼의 글자를 지운다
        b.backspace();
        assert_eq!(b.text_at((0, 0)), "업명");
    }

    #[test]
    fn backspace_at_the_start_of_a_paragraph_does_nothing() {
        // 지울 글자가 없으면 아무 일도 없어야 한다 — 앞 단락을 건드리면 안 된다
        let mut b = Buffer::from_doc(&doc());
        b.cursor.offset = 0;
        b.backspace();
        assert_eq!(b.text_at((0, 0)), "과업명");
        assert!(!b.is_dirty());
    }

    #[test]
    fn undo_puts_back_inserted_text() {
        let mut b = Buffer::from_doc(&doc());
        b.cursor.offset = 0;
        b.insert_str("□ ");
        assert_eq!(b.text_at((0, 0)), "□ 과업명");
        let msg = b.undo().unwrap();
        assert!(msg.contains("되돌렸"), "{msg}");
        assert_eq!(b.text_at((0, 0)), "과업명");
        assert_eq!(b.cursor.offset, 0);
    }

    #[test]
    fn undo_restores_what_was_deleted() {
        let mut b = Buffer::from_doc(&doc());
        assert_eq!(b.text_at((0, 0)), "과업명");
        b.delete_chars(2);
        assert_eq!(b.text_at((0, 0)), "명");

        let msg = b.undo().unwrap();
        assert!(msg.contains("되돌렸"), "{msg}");
        // **지운 글자가 그대로 돌아온다.** 전에는 "되돌릴 수 없다" 였다 —
        // 재생 로그에 지운 내용이 없다는 이유였는데, 그건 로그의 성질이지
        // 되돌리기의 한계가 아니었다. 되돌리기 기록을 따로 쌓아 해결했다.
        assert_eq!(b.text_at((0, 0)), "과업명");
        // 로그에서 빠졌다 — `:w` 가 그 삭제를 보내지 않는다
        assert!(b.log().ops().is_empty());

        // 다시하기로 되돌아간다
        let msg = b.redo().unwrap();
        assert!(msg.contains("다시 했"), "{msg}");
        assert_eq!(b.text_at((0, 0)), "명");
        assert_eq!(b.log().ops().len(), 1);
    }

    /// `dd` 가 담고 `p` 가 붙인다 — vi 처럼 지우기도 담는다.
    #[test]
    fn deleting_a_paragraph_puts_it_in_the_register_so_p_brings_it_back() {
        let mut b = Buffer::from_doc(&doc());
        let before: Vec<String> =
            b.coords().iter().map(|c| b.text_at(*c).to_string()).collect();
        assert!(before.len() >= 2, "단락이 둘 이상 필요하다: {before:?}");

        b.goto((0, 0));
        let gone = b.text_at((0, 0)).to_string();
        b.delete_paragraph();
        assert_ne!(b.text_at((0, 0)), gone, "지워지지 않았다");

        // 담겨 있으니 붙일 수 있다
        let n = b.paste_paragraph(true).expect("담아 둔 것이 있다");
        assert_eq!(n, gone.chars().count());
        assert_eq!(b.text_at(b.cursor.at), gone, "붙인 글이 다르다");
    }

    /// 담아 둔 것이 없으면 **아무것도 하지 않고 그렇다고 말한다.**
    #[test]
    fn pasting_with_an_empty_register_does_nothing() {
        let mut b = Buffer::from_doc(&doc());
        let n = b.coords().len();
        assert!(b.paste_paragraph(false).is_none());
        assert_eq!(b.coords().len(), n, "빈 단락이 생겼다");
        assert!(b.log().ops().is_empty(), "연산이 쌓였다");
    }

    /// `yy` 는 담기만 하고 글을 바꾸지 않는다.
    #[test]
    fn yank_does_not_change_the_text() {
        let mut b = Buffer::from_doc(&doc());
        let text = b.text_at((0, 0)).to_string();
        let n = b.yank_paragraph();
        assert_eq!(n, text.chars().count());
        assert_eq!(b.text_at((0, 0)), text);
        assert!(b.log().ops().is_empty(), "담기가 편집으로 쌓였다");
        // 담은 것을 붙이면 같은 글이 하나 더 생긴다
        b.paste_paragraph(false).expect("담아 뒀다");
        assert_eq!(b.text_at(b.cursor.at), text);
    }

    /// **새로 고치면 다시하기 줄을 버린다** — vi 와 같다. 안 버리면 갈라진
    /// 역사를 다시 하게 되어 글이 뒤섞인다.
    #[test]
    fn editing_after_undo_drops_the_redo_trail() {
        let mut b = Buffer::from_doc(&doc());
        b.insert_str("가");
        b.undo();
        b.insert_str("나");
        assert!(b.redo().is_none(), "버려야 할 다시하기가 남았다");
    }

    #[test]
    fn opening_a_paragraph_shifts_the_coordinates() {
        let mut b = Buffer::from_doc(&doc());
        b.open_paragraph(false);
        assert_eq!(b.cursor.at, (0, 1), "커서가 새 단락으로");
        assert_eq!(b.text_at((0, 1)), "");
        assert_eq!(b.text_at((0, 2)), "소요예산", "뒤로 밀렸다");
    }

    #[test]
    fn deleting_a_paragraph_pulls_the_rest_up() {
        let mut b = Buffer::from_doc(&doc());
        b.delete_paragraph();
        assert_eq!(b.text_at((0, 0)), "소요예산", "앞으로 당겨졌다");
        assert_eq!(b.log().ops().len(), 1);
    }

    #[test]
    fn cursor_screen_position_accounts_for_wide_chars() {
        let mut b = Buffer::from_doc(&doc());
        b.cursor.offset = 2;
        // '과업' = 4칸
        assert_eq!(b.cursor_screen(40), (0, 4));
        // 좁은 폭에서는 둘째 줄로 넘어간다
        b.cursor.offset = 3;
        assert_eq!(b.cursor_screen(4), (1, 2));
    }

    #[test]
    fn dirty_paragraphs_are_tracked_for_the_stale_layout_warning() {
        let mut b = Buffer::from_doc(&doc());
        assert!(!b.is_dirty());
        b.insert_str("x");
        assert!(b.is_dirty());
        assert!(b.dirty_paragraphs().contains(&(0, 0)));
        b.mark_saved();
        assert!(!b.is_dirty());
        assert!(b.dirty_paragraphs().is_empty());
    }

    /// **한국어 낱말은 띄어쓰기로 가른다.**
    ///
    /// vi 는 낱말을 "글자 묶음 / 기호 묶음 / 공백" 으로 가르는데, 한글은 조사가
    /// 붙어 한 덩이로 움직이는 편이 자연스럽다 — `과업지시서를` 은 한 낱말이다.
    #[test]
    fn word_motions_split_korean_on_spaces() {
        use super::Word;
        let mut b = Buffer::from_doc(&doc());
        b.goto((0, 1));
        b.replace_paragraph((0, 1), "과업지시서를 다시 읽는다");
        b.cursor.offset = 0;

        // w — 다음 낱말의 첫 글자
        b.move_word(Word::Forward);
        assert_eq!(b.cursor.offset, 7, "`다시` 앞이어야 한다");
        b.move_word(Word::Forward);
        assert_eq!(b.cursor.offset, 10, "`읽는다` 앞이어야 한다");
        // 끝에서 더 가지 않는다
        b.move_word(Word::Forward);
        let n = b.text_at((0, 1)).chars().count();
        assert_eq!(b.cursor.offset, n);
        b.move_word(Word::Forward);
        assert_eq!(b.cursor.offset, n, "끝을 넘어갔다");

        // b — 앞 낱말의 첫 글자
        b.move_word(Word::Back);
        assert_eq!(b.cursor.offset, 10);
        b.move_word(Word::Back);
        assert_eq!(b.cursor.offset, 7);
        b.move_word(Word::Back);
        assert_eq!(b.cursor.offset, 0);
        b.move_word(Word::Back);
        assert_eq!(b.cursor.offset, 0, "처음을 넘어갔다");

        // e — 지금 낱말의 끝 다음
        b.cursor.offset = 0;
        b.move_word(Word::End);
        assert_eq!(b.cursor.offset, 6, "`과업지시서를` 끝 다음");

        // 조사가 붙은 덩이를 쪼개지 않는다
        b.cursor.offset = 0;
        b.move_word(Word::Forward);
        let seen: String = b.text_at((0, 1)).chars().take(6).collect();
        assert_eq!(seen, "과업지시서를", "조사를 떼어 냈다");
    }

    /// 공백만 있거나 빈 단락에서도 **멈추고 끝난다** — 무한히 돌지 않는다.
    #[test]
    fn word_motions_terminate_on_blank_paragraphs() {
        use super::Word;
        let mut b = Buffer::from_doc(&doc());
        for text in ["", "   ", " 하나 "] {
            b.replace_paragraph((0, 1), text);
            b.goto((0, 1));
            for how in [Word::Forward, Word::Back, Word::End] {
                b.move_word(how);
                let n = b.text_at((0, 1)).chars().count();
                assert!(b.cursor.offset <= n, "{text:?} {how:?} 에서 범위를 넘었다");
            }
        }
    }
}
