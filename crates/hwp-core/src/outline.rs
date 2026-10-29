//! 공문서 글머리 개요 트리.
//!
//! rhwp 의 `export-structure` 는 이걸 못 만든다 — `--mode auto|outline|clause`
//! 세 모드가 모두 `roots=0` 이고 내용이 평평한 배열로 떨어진다(샘플 6건 전수).
//! 법령 조문(제N조)이나 스타일 기반 개요만 보는 듯하다. 그래서 여기서 만든다.
//!
//! 레벨을 어떻게 정하는가. 코퍼스 14개를 전수 조사해 보니 **기호 하나로도,
//! 들여쓰기 하나로도 정해지지 않는다**:
//!
//! - 같은 기호가 들여쓰기 0·1·2 에 다 나온다 (`○` 가 85/15/3 회, `□` 가 41/16/5 회)
//! - `※` 와 `*` 는 들여쓰기와 무관하게 **주석**이다 — 앞 항목에 붙고 형제가 아니다
//! - 서로 다른 `(들여쓰기, 기호)` 조합이 42 가지였다
//!
//! 그래서 `(들여쓰기, 기호분류)` 를 키로 삼아 **문서가 실제로 쓴 키들만** 모아
//! 정렬하고 1부터 레벨을 매긴다. `○` 와 `-` 만 쓰는 문서는 1·2 레벨이 되고,
//! `□ ○ -` 를 쓰는 문서는 1·2·3 이 된다 — 문서마다 자기 체계로 읽힌다.

use std::collections::BTreeSet;

/// 글머리 기호의 분류. 안의 순서가 공문서의 관행적 깊이 순서다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MarkerClass {
    /// `□ ■ ◇ ◆ ▶ ▷` — 큰 항목
    Box,
    /// `1. 2) Ⅰ.` — 번호
    Number,
    /// `가. 나) A.` — 가나다
    Letter,
    /// `① ② ⑴` — 동그라미 숫자
    Circled,
    /// `○ ㅇ ◦ ● ❍` — 중간 항목
    Circle,
    /// `- – — ·` — 작은 항목
    Dash,
    /// `▪ ▫ ◾` — 더 작은 항목
    Square,
    /// `※ * ‣ →` — 주석. **레벨을 갖지 않고 앞 항목에 붙는다.**
    Note,
}

impl MarkerClass {
    pub fn is_note(self) -> bool {
        self == Self::Note
    }

    /// **값이 세어지는 글머리인가** (`1.` `2.` / `가.` `나.` / `①` `②`).
    ///
    /// 단계를 옮길 때 중요하다. 고정 기호(`□` `○` `-`)는 그 단계의 것으로
    /// 갈아 끼우면 되지만, 세어지는 글머리는 **그 자리에서 몇 번째인지**에
    /// 달려 있어서 다른 단락에서 본 표본을 붙이면 틀린 번호가 들어간다.
    /// 코퍼스에서 실제로 `8.` `6.` `나.` 가 대표값으로 뽑혔다.
    pub fn counts(self) -> bool {
        matches!(self, Self::Number | Self::Letter | Self::Circled)
    }
}

/// 한 단락에서 읽어낸 글머리.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// 글머리 기호 그대로 (`□`, `1.`, `가.`)
    pub text: String,
    pub class: MarkerClass,
    /// 기호 앞 공백 칸 수
    pub indent: usize,
}

/// 개요 트리의 한 마디.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// 편집 좌표
    pub section: u32,
    pub para: u32,
    /// 1부터. 주석은 부모와 같은 레벨을 쓰지 않고 `None` 이다.
    pub level: Option<usize>,
    /// 글머리 기호 (없으면 `None` — 글머리 없는 단락)
    pub marker: Option<String>,
    /// 글머리와 뒤따르는 공백을 뗀 본문
    pub text: String,
    /// 주석인가
    pub is_note: bool,
    pub children: Vec<Node>,
}

/// 트리로 만들 입력 — 본문 단락 하나.
#[derive(Debug, Clone)]
pub struct Item {
    pub section: u32,
    pub para: u32,
    pub text: String,
}

/// 단락 앞머리에서 글머리를 읽는다. 없으면 `None`.
pub fn parse_marker(text: &str) -> Option<Marker> {
    // **글자 수와 바이트 수를 섞지 않는다.** `indent` 는 레벨을 가르는 키이고
    // 뜻이 "앞 공백 칸 수"(글자 수)다. 자르는 쪽은 **바이트**여야 한다.
    //
    // 지금은 둘이 같다 — 세는 글자가 `' '` 와 `'\t'` 뿐이고 둘 다 1바이트라서.
    // 그래서 전에는 글자 수를 그대로 바이트 자리에 썼고 **우연히** 돌았다.
    // 세는 목록에 전각 공백(U+3000, 3바이트)이나 U+2007 을 한 줄 더하는 순간
    // 바이트 자리가 어긋나 한글 문서에서 터진다 — 이 코퍼스에 U+2007 이
    // 74회 있으므로 가정이 아니다. 그래서 둘을 따로 센다.
    let indent_bytes = text.len() - text.trim_start_matches([' ', '\t']).len();
    let indent = text[..indent_bytes].chars().count();
    let rest: &str = &text[indent_bytes..];
    let mut chars = rest.chars();
    let first = chars.next()?;

    // 한 글자 기호 — 뒤에 공백이나 끝이 와야 한다 ('-' 가 '-5℃' 를 먹지 않게)
    if let Some(class) = class_of_symbol(first) {
        let after = chars.clone().next();
        if after.is_none() || after == Some(' ') || after == Some('\u{00A0}') {
            return Some(Marker { text: first.to_string(), class, indent });
        }
    }

    // 동그라미 숫자는 뒤에 공백이 없어도 글머리다
    if ('\u{2460}'..='\u{2473}').contains(&first) {
        return Some(Marker {
            text: first.to_string(),
            class: MarkerClass::Circled,
            indent,
        });
    }

    // `1.` `12)` `가.` `나)` — 숫자나 가나다 뒤에 마침표/닫는 괄호, 그리고 공백
    let mut head = String::new();
    let mut it = rest.chars();
    while let Some(c) = it.next() {
        if c.is_ascii_digit() && head.len() < 2 {
            head.push(c);
            continue;
        }
        if (c == '.' || c == ')') && !head.is_empty() {
            let next = it.next();
            if next.is_none() || next == Some(' ') {
                head.push(c);
                return Some(Marker {
                    text: head,
                    class: MarkerClass::Number,
                    indent,
                });
            }
        }
        break;
    }

    let mut it = rest.chars();
    if let Some(h) = it.next() {
        if is_hangul_order_letter(h) {
            if let Some(c) = it.next() {
                if c == '.' || c == ')' {
                    let next = it.next();
                    if next.is_none() || next == Some(' ') {
                        return Some(Marker {
                            text: format!("{h}{c}"),
                            class: MarkerClass::Letter,
                            indent,
                        });
                    }
                }
            }
        }
    }

    None
}

fn class_of_symbol(c: char) -> Option<MarkerClass> {
    match c {
        '□' | '■' | '◇' | '◆' | '▶' | '▷' | '❑' => Some(MarkerClass::Box),
        '○' | 'ㅇ' | '◦' | '●' | '❍' | '㉮' => Some(MarkerClass::Circle),
        '-' | '–' | '—' | '·' | '‧' => Some(MarkerClass::Dash),
        '▪' | '▫' | '◾' | '▣' => Some(MarkerClass::Square),
        '※' | '*' | '‣' | '→' | '⇒' => Some(MarkerClass::Note),
        _ => None,
    }
}

/// 가나다 순서 글자 — 첫 14 자만 (받침 없는 음절).
fn is_hangul_order_letter(c: char) -> bool {
    matches!(c, '가' | '나' | '다' | '라' | '마' | '바' | '사' | '아' | '자' | '차' | '카' | '타' | '파' | '하')
}

/// 들여쓰기 값들을 **뭉친다.** 사람이 손으로 띄운 공백은 한 칸씩 흔들린다 —
/// 실제 공고문에서 `- (IRIS)` 와 `- (이메일)` 의 들여쓰기가 한 칸 달라서, 뭉치지
/// 않으면 형제가 부모·자식이 된다.
///
/// 같은 기호 분류 안에서 차이가 `TOLERANCE` 이하인 들여쓰기는 작은 쪽으로 모은다.
const TOLERANCE: usize = 1;

fn snap_indents(mut observed: Vec<usize>) -> Vec<usize> {
    observed.sort_unstable();
    observed.dedup();
    let mut canon: Vec<usize> = Vec::new();
    for v in observed {
        match canon.last() {
            Some(&last) if v.saturating_sub(last) <= TOLERANCE => {}
            _ => canon.push(v),
        }
    }
    canon
}

/// 들여쓰기를 뭉친 대표값으로 바꾼다.
fn canonical_indent(canon: &[usize], indent: usize) -> usize {
    canon
        .iter()
        .rev()
        .find(|&&c| c <= indent)
        .copied()
        .unwrap_or_else(|| canon.first().copied().unwrap_or(indent))
}

/// 한 구역이 실제로 쓴 `(들여쓰기, 분류)` 키들로 레벨 표를 만든다.
///
/// **구역마다 따로 만든다.** 공고문은 본문과 붙임 양식이 서로 다른 구역에
/// 있고 글머리 체계도 다르다 — 한 표로 묶으면 붙임의 `1.` 이 본문 계층
/// 아래로 끌려 들어간다(s14.hwpx 로 실측했다).
#[derive(Debug, Default)]
struct LevelTable {
    /// 분류마다 뭉친 들여쓰기 대표값
    canon: std::collections::BTreeMap<MarkerClass, Vec<usize>>,
    keys: Vec<(usize, MarkerClass)>,
    /// 단계마다 **문서가 실제로 쓴** 글머리 하나 (`keys` 와 같은 순서).
    /// 단계를 옮길 때 기호를 지어내지 않으려고 들고 있다.
    samples: Vec<String>,
}

impl LevelTable {
    fn build(items: &[Item]) -> Self {
        let mut raw: std::collections::BTreeMap<MarkerClass, Vec<usize>> = Default::default();
        for it in items {
            if let Some(m) = parse_marker(&it.text) {
                if !m.class.is_note() {
                    raw.entry(m.class).or_default().push(m.indent);
                }
            }
        }
        let canon: std::collections::BTreeMap<MarkerClass, Vec<usize>> = raw
            .into_iter()
            .map(|(c, v)| (c, snap_indents(v)))
            .collect();

        let mut keys: BTreeSet<(usize, MarkerClass)> = BTreeSet::new();
        for it in items {
            if let Some(m) = parse_marker(&it.text) {
                if m.class.is_note() {
                    continue;
                }
                if let Some(cs) = canon.get(&m.class) {
                    keys.insert((canonical_indent(cs, m.indent), m.class));
                }
            }
        }
        let keys: Vec<(usize, MarkerClass)> = keys.into_iter().collect();

        // 단계마다 그 단계에서 가장 많이 쓰인 기호를 고른다. 번호 글머리는
        // 값이 단락마다 달라지므로(`1.` `2.` `3.`) 최빈값이 대표로 맞다.
        let mut counts: Vec<std::collections::BTreeMap<String, usize>> =
            vec![Default::default(); keys.len()];
        for it in items {
            if let Some(m) = parse_marker(&it.text) {
                if m.class.is_note() {
                    continue;
                }
                let Some(cs) = canon.get(&m.class) else { continue };
                let ind = canonical_indent(cs, m.indent);
                if let Some(i) = keys.iter().position(|k| *k == (ind, m.class)) {
                    *counts[i].entry(m.text.clone()).or_insert(0) += 1;
                }
            }
        }
        let samples = counts
            .iter()
            .map(|c| {
                c.iter()
                    .max_by_key(|(t, n)| (**n, std::cmp::Reverse(t.len())))
                    .map(|(t, _)| t.clone())
                    .unwrap_or_default()
            })
            .collect();

        Self { canon, keys, samples }
    }

    fn level_of(&self, m: &Marker) -> usize {
        let indent = self
            .canon
            .get(&m.class)
            .map(|cs| canonical_indent(cs, m.indent))
            .unwrap_or(m.indent);
        self.keys
            .iter()
            .position(|k| *k == (indent, m.class))
            .map(|i| i + 1)
            .unwrap_or(1)
    }
}

/// 본문 단락들을 개요 트리로 만든다.
///
/// 글머리가 없는 단락은 가장 가까운 앞 마디의 본문 뒤에 이어 붙지 않고,
/// 그 마디의 자식으로 들어간다 — 트리를 보고 원문을 되살릴 수 있어야 한다.
pub fn build(items: &[Item]) -> Vec<Node> {
    // 구역마다 레벨 표를 따로 만들고, 구역 경계에서 스택을 비운다.
    let mut roots: Vec<Node> = Vec::new();
    let mut sections: Vec<u32> = items.iter().map(|i| i.section).collect();
    sections.dedup();
    for sec in sections {
        let slice: Vec<Item> = items.iter().filter(|i| i.section == sec).cloned().collect();
        roots.extend(build_one_section(&slice));
    }
    roots
}

fn build_one_section(items: &[Item]) -> Vec<Node> {
    let table = LevelTable::build(items);
    let mut roots: Vec<Node> = Vec::new();
    // (레벨, roots 로부터의 경로)
    let mut stack: Vec<usize> = Vec::new();

    for it in items {
        if it.text.trim().is_empty() {
            continue;
        }
        let marker = parse_marker(&it.text);
        let (level, marker_text, is_note, body) = match &marker {
            Some(m) if m.class.is_note() => {
                let body = it.text[m.indent + m.text.len()..].trim_start().to_string();
                (None, Some(m.text.clone()), true, body)
            }
            Some(m) => {
                let lv = table.level_of(m);
                let body = it.text[m.indent + m.text.len()..].trim_start().to_string();
                (Some(lv), Some(m.text.clone()), false, body)
            }
            None => (None, None, false, it.text.trim().to_string()),
        };

        let node = Node {
            section: it.section,
            para: it.para,
            level,
            marker: marker_text,
            text: body,
            is_note,
            children: Vec::new(),
        };

        match level {
            Some(lv) => {
                // 이 레벨 이상으로 열려 있는 마디를 닫는다
                while stack.len() >= lv {
                    stack.pop();
                }
                push_at(&mut roots, &stack, node);
                // 방금 넣은 마디를 스택에 올린다
                let idx = child_count(&roots, &stack) - 1;
                stack.push(idx);
            }
            None => {
                // 주석·글머리 없는 단락은 지금 열린 마디의 자식
                push_at(&mut roots, &stack, node);
            }
        }
    }
    roots
}

fn child_count(roots: &[Node], path: &[usize]) -> usize {
    match path.split_first() {
        None => roots.len(),
        Some((&i, rest)) => match roots.get(i) {
            Some(n) => child_count(&n.children, rest),
            None => 0,
        },
    }
}

fn push_at(roots: &mut Vec<Node>, path: &[usize], node: Node) {
    match path.split_first() {
        None => roots.push(node),
        Some((&i, rest)) => match roots.get_mut(i) {
            Some(n) => push_at(&mut n.children, rest, node),
            // 경로가 끊겼으면 — 있을 수 없지만 — 글자를 잃지 않게 여기 붙인다
            None => roots.push(node),
        },
    }
}

/// 트리를 평평한 `(레벨, 줄)` 목록으로 — 목차 표시나 시험에 쓴다.
pub fn flatten(nodes: &[Node], depth: usize, out: &mut Vec<(usize, String)>) {
    for n in nodes {
        let head = match &n.marker {
            Some(m) => format!("{m} {}", n.text),
            None => n.text.clone(),
        };
        out.push((depth, head));
        flatten(&n.children, depth + 1, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(lines: &[&str]) -> Vec<Item> {
        lines
            .iter()
            .enumerate()
            .map(|(i, t)| Item { section: 0, para: i as u32, text: t.to_string() })
            .collect()
    }

    #[test]
    fn reads_the_markers_the_corpus_actually_uses() {
        for (src, want_class, want_text) in [
            ("□ 목 적", MarkerClass::Box, "□"),
            (" ○ 극한기후 대응", MarkerClass::Circle, "○"),
            ("   - 내용", MarkerClass::Dash, "-"),
            ("  ◦ 전 학과", MarkerClass::Circle, "◦"),
            ("※ 본사업은", MarkerClass::Note, "※"),
            ("  * 하이플렉스", MarkerClass::Note, "*"),
            ("▶ 시설관리", MarkerClass::Box, "▶"),
            ("1. 신청 개요", MarkerClass::Number, "1."),
            ("  가. 활용 예정 수업", MarkerClass::Letter, "가."),
            ("① 첫째", MarkerClass::Circled, "①"),
            ("12) 열두째", MarkerClass::Number, "12)"),
        ] {
            let m = parse_marker(src).unwrap_or_else(|| panic!("{src:?} 에서 글머리를 못 읽었다"));
            assert_eq!(m.class, want_class, "{src:?}");
            assert_eq!(m.text, want_text, "{src:?}");
        }
    }

    #[test]
    fn a_dash_in_a_number_is_not_a_marker() {
        // '-5℃' 나 '2026-443호' 를 글머리로 먹으면 안 된다
        assert!(parse_marker("-5℃ 까지").is_none());
        assert!(parse_marker("제2026-443호").is_none());
        assert!(parse_marker("과업명").is_none());
    }

    #[test]
    fn levels_come_from_what_the_document_uses() {
        // □ ○ - 를 쓰는 문서 → 1, 2, 3
        let three = build(&items(&["□ 가", " ○ 나", "   - 다"]));
        let mut flat = Vec::new();
        flatten(&three, 0, &mut flat);
        assert_eq!(flat.iter().map(|(d, _)| *d).collect::<Vec<_>>(), vec![0, 1, 2]);

        // ○ 와 - 만 쓰는 문서 → 그 둘이 1, 2 가 된다 (2, 3 이 아니다)
        let two = build(&items(&["○ 가", "  - 나", "○ 다"]));
        assert_eq!(two.len(), 2, "뿌리가 2개여야 한다");
        assert_eq!(two[0].level, Some(1));
        assert_eq!(two[0].children[0].level, Some(2));
    }

    #[test]
    fn notes_attach_to_the_open_node_and_have_no_level() {
        let tree = build(&items(&["□ 조사 분야", " ○ 사업", "※ 제출기관 참고", "□ 조사 기간"]));
        assert_eq!(tree.len(), 2);
        let circle = &tree[0].children[0];
        assert_eq!(circle.children.len(), 1, "주석이 ○ 아래 붙어야 한다");
        let note = &circle.children[0];
        assert!(note.is_note);
        assert_eq!(note.level, None, "주석은 레벨을 갖지 않는다");
        assert_eq!(note.text, "제출기관 참고");
    }

    #[test]
    fn markerless_paragraphs_become_children_not_lost_text() {
        let tree = build(&items(&["□ 목적", "이어지는 설명 문장", "□ 기간"]));
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].text, "이어지는 설명 문장");
        assert_eq!(tree[0].children[0].marker, None);
    }

    #[test]
    fn one_space_of_sloppy_indent_does_not_create_a_level() {
        // 실제 공고문에서 겪은 것: '- (IRIS)' 와 '- (이메일)' 의 들여쓰기가 한 칸
        // 다르다. 뭉치지 않으면 형제가 부모·자식이 된다.
        let tree = build(&items(&[
            " ○ 범부처통합연구지원시스템",
            "    - (IRIS) 홈페이지",
            "     - (이메일) 첨부된 양식",
        ]));
        let circle = &tree[0];
        assert_eq!(
            circle.children.len(),
            2,
            "두 '-' 는 형제여야 한다. 실제 트리: {:#?}",
            circle.children
        );
    }

    #[test]
    fn each_section_gets_its_own_level_table() {
        // 구역 0 은 □ ○, 구역 1 은 1. 만 쓴다 — 붙임의 '1.' 이 본문 아래로
        // 끌려 들어가면 안 된다.
        let mut v = items(&["□ 본문", " ○ 세부"]);
        v.push(Item { section: 1, para: 0, text: "1. 붙임 개요".into() });
        v.push(Item { section: 1, para: 1, text: "2. 인적사항".into() });
        let tree = build(&v);
        let roots: Vec<_> = tree.iter().map(|n| (n.section, n.level)).collect();
        assert_eq!(
            roots,
            vec![(0, Some(1)), (1, Some(1)), (1, Some(1))],
            "구역 1 의 '1.' 과 '2.' 는 각자 뿌리여야 한다"
        );
    }

    #[test]
    fn every_item_keeps_its_edit_coordinate() {
        let tree = build(&items(&["□ 가", " ○ 나"]));
        assert_eq!((tree[0].section, tree[0].para), (0, 0));
        assert_eq!((tree[0].children[0].section, tree[0].children[0].para), (0, 1));
    }
}

/// 이 문서가 실제로 쓰는 **글머리 단계 사다리**.
///
/// 단축키로 단계를 올리고 내릴 때 들여쓰기 칸 수와 기호를 **지어내지 않기**
/// 위해 있다. 한국 공문서의 단계는 문서마다 다르다 — 실측: s00 은
/// `□(0칸) □(2칸) -(3칸)`, s13 은 `번호(0칸) ○(1칸) -(3칸)`, s02 는 5단계다.
/// 사다리를 하나 박아 두면 다른 문서에서 엉뚱한 기호를 넣는다.
///
/// 글머리가 문단 속성이 아니라 **실제 글자**라는 것은 전수로 확인했다 —
/// 코퍼스 664단락에서 조판기가 그리는 글자와 우리 글자가 한 군데도 다르지
/// 않다. 그래서 단계를 옮기는 일은 글자 편집이고, 검증된 저장 경로를 탄다.
pub struct Levels {
    table: LevelTable,
}

/// 단계 하나 — 들여쓰기 칸 수와 그 단계의 기호.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// 1부터
    pub level: usize,
    pub indent: usize,
    pub marker: String,
    pub class: MarkerClass,
}

impl Levels {
    pub fn build(items: &[Item]) -> Self {
        Self { table: LevelTable::build(items) }
    }

    pub fn len(&self) -> usize {
        self.table.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.keys.is_empty()
    }

    /// `level` (1부터) 의 들여쓰기와 기호.
    pub fn step(&self, level: usize) -> Option<Step> {
        if level == 0 {
            return None;
        }
        let (indent, class) = *self.table.keys.get(level - 1)?;
        Some(Step {
            level,
            indent,
            marker: self.table.samples.get(level - 1).cloned().unwrap_or_default(),
            class,
        })
    }

    /// 모든 단계. 사용자에게 사다리를 보여줄 때 쓴다.
    pub fn steps(&self) -> Vec<Step> {
        (1..=self.len()).filter_map(|l| self.step(l)).collect()
    }

    /// 이 글자가 지금 몇 단계인가. 글머리가 없거나 주석이면 `None`.
    pub fn level_of_text(&self, text: &str) -> Option<usize> {
        let m = parse_marker(text)?;
        if m.class.is_note() {
            return None;
        }
        Some(self.table.level_of(&m))
    }
}

/// 단계를 옮긴 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Releveled {
    pub text: String,
    /// 옮겨 간 단계 (1부터). 글머리 없는 단락을 밀었을 때는 `None`.
    pub level: Option<usize>,
    /// 사람에게 알려야 할 것. 번호를 손으로 고쳐야 하는 경우가 여기 담긴다.
    pub note: Option<String>,
}

/// 단락의 단계를 `delta` 만큼 옮긴 글자. 옮길 수 없으면 `None`.
///
/// 글머리가 **있고** 목표 단계의 기호가 고정 기호면 들여쓰기와 기호를 함께
/// 바꾼다. 공문서는 단계마다 기호가 정해져 있어서 들여쓰기만 밀면 문서 규칙이
/// 깨진다.
///
/// 목표 단계가 **세어지는 글머리**(`1.` `가.` `①`)면 들여쓰기만 옮기고 기호는
/// 원래 것을 그대로 둔 뒤 `note` 로 알린다. 번호는 그 자리에서 몇 번째인지에
/// 달려 있어서 다른 단락의 표본을 붙이면 틀린 번호가 된다.
///
/// 글머리가 **없으면** 들여쓰기만 사다리의 다음 칸으로 옮긴다. 기호를
/// 지어내지 않는다 — 사용자가 달라고 하지 않은 글머리를 붙이는 것은 글자를
/// 만들어 내는 일이다.
///
/// `None`: 사다리가 비었다(글머리를 쓰지 않는 문서), 주석 단락이다, 또는
/// 바뀐 것이 없다(사다리 끝).
pub fn relevel(text: &str, levels: &Levels, delta: isize) -> Option<Releveled> {
    if levels.is_empty() || delta == 0 {
        return None;
    }
    let marker = parse_marker(text);
    if marker.as_ref().is_some_and(|m| m.class.is_note()) {
        return None;
    }

    let out = match (&marker, levels.level_of_text(text)) {
        (Some(m), Some(cur)) => {
            let target = (cur as isize + delta).max(1) as usize;
            let step = levels.step(target)?;
            let body = text.get(m.indent + m.text.len()..).unwrap_or("").trim_start();
            if step.class.counts() {
                Releveled {
                    text: format!("{}{} {}", " ".repeat(step.indent), m.text, body),
                    level: Some(target),
                    note: Some(format!(
                        "{target}단계는 번호 글머리입니다 — 들여쓰기만 옮겼고 `{}` 는 \
                         그대로 두었습니다. 번호는 앞 항목을 보고 직접 고치세요.",
                        m.text
                    )),
                }
            } else {
                Releveled {
                    text: format!("{}{} {}", " ".repeat(step.indent), step.marker, body),
                    level: Some(target),
                    note: None,
                }
            }
        }
        _ => {
            let body = text.trim_start();
            let now = text.len() - body.len();
            let ladder: Vec<usize> = {
                let mut v: Vec<usize> = std::iter::once(0)
                    .chain(levels.steps().into_iter().map(|s| s.indent))
                    .collect();
                v.sort_unstable();
                v.dedup();
                v
            };
            // 지금 들여쓰기가 사다리의 어디쯤인지 — 같은 값이 없으면 가까운 칸
            let at = ladder
                .iter()
                .position(|i| *i >= now)
                .unwrap_or(ladder.len().saturating_sub(1));
            let target = (at as isize + delta).clamp(0, ladder.len() as isize - 1) as usize;
            Releveled {
                text: format!("{}{body}", " ".repeat(ladder[target])),
                level: None,
                note: None,
            }
        }
    };

    (out.text != text).then_some(out)
}

#[cfg(test)]
mod relevel_tests {
    use super::*;

    fn ladder(texts: &[&str]) -> Levels {
        let items: Vec<Item> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| Item { section: 0, para: i as u32, text: (*t).to_string() })
            .collect();
        Levels::build(&items)
    }

    /// 고정 기호 단계로 옮기면 **들여쓰기와 기호를 함께** 바꾼다.
    #[test]
    fn relevel_swaps_a_fixed_symbol_and_its_indent() {
        let lv = ladder(&["□ 큰 항목", "  ○ 중간", "    - 작은"]);
        let r = relevel("  ○ 중간", &lv, 1).expect("한 단계 더");
        assert_eq!(r.text, "    - 중간");
        assert_eq!(r.level, Some(3));
        assert!(r.note.is_none(), "고정 기호인데 알림이 붙었다: {:?}", r.note);

        let back = relevel("    - 작은", &lv, -1).expect("한 단계 밖");
        assert_eq!(back.text, "  ○ 작은");
    }

    /// 세어지는 글머리 단계로 옮기면 **들여쓰기만** 옮기고 번호는 그대로 둔다.
    #[test]
    fn relevel_keeps_a_counting_marker_and_says_so() {
        let lv = ladder(&["□ 큰 항목", "  1. 하나", "  2. 둘", "  3. 셋"]);
        let r = relevel("□ 큰 항목", &lv, 1).expect("한 단계 더");
        assert_eq!(r.text, "  □ 큰 항목", "번호를 지어내 붙였다: {:?}", r.text);
        let note = r.note.expect("번호를 손으로 고쳐야 한다고 알리지 않았다");
        assert!(note.contains("번호"), "알림이 무슨 말인지 모르겠다: {note}");
    }

    /// 글머리가 없는 단락은 **들여쓰기만** 옮긴다 — 기호를 지어내지 않는다.
    #[test]
    fn relevel_does_not_invent_a_bullet_for_a_plain_paragraph() {
        let lv = ladder(&["□ 큰 항목", "  ○ 중간", "    - 작은"]);
        let r = relevel("그냥 글", &lv, 1).expect("한 칸 들여");
        assert_eq!(r.text, "  그냥 글");
        assert_eq!(r.level, None);
        assert!(!r.text.contains('○'), "없던 글머리가 생겼다: {:?}", r.text);
    }

    /// 주석(`※`)은 단계를 갖지 않으므로 옮기지 않는다.
    #[test]
    fn relevel_leaves_a_note_paragraph_alone() {
        let lv = ladder(&["□ 큰 항목", "  ○ 중간", "    - 작은"]);
        assert!(relevel("※ 참고", &lv, 1).is_none());
    }

    /// 사다리 끝에서는 아무것도 하지 않는다 — 없는 단계를 만들지 않는다.
    #[test]
    fn relevel_stops_at_the_ends_of_the_ladder() {
        let lv = ladder(&["□ 큰 항목", "  ○ 중간", "    - 작은"]);
        assert!(relevel("    - 작은", &lv, 1).is_none(), "사다리 밖으로 나갔다");
        assert!(relevel("□ 큰 항목", &lv, -1).is_none());
    }

    /// 글머리를 쓰지 않는 문서에서는 사다리가 비고, 옮길 수 없다.
    #[test]
    fn an_empty_ladder_refuses_instead_of_guessing() {
        let lv = ladder(&["그냥 글", "또 글"]);
        assert!(lv.is_empty());
        assert!(relevel("그냥 글", &lv, 1).is_none());
    }
}

#[cfg(test)]
mod boundary_audit {
    use super::*;

    /// **글머리를 읽을 때 글자 수를 바이트 자리에 쓰면 안 된다.**
    ///
    /// 감사(2026-10-13)에서 나온 것: 앞 공백을 `chars().count()` 로 세어
    /// 그 수를 `&text[indent..]` 의 **바이트** 자리에 쓰고 있었다. 세는 글자가
    /// `' '`·`'\t'` 뿐이라 우연히 돌았을 뿐이다.
    ///
    /// 이 시험은 그 가정이 깨졌을 때를 흉내 낸다 — 앞에 **여러 바이트 글자**가
    /// 섞인 단락을 읽혀 본다. 글자 수를 바이트로 쓰면 글자 중간을 잘라 터진다.
    #[test]
    fn reading_a_marker_never_slices_a_character_in_half() {
        for text in [
            "　○ 전각 공백이 앞에 있다",          // U+3000 (3바이트)
            " \u{2007}○ 코퍼스에 74회 있는 공백", // U+2007 (3바이트)
            "한글로 시작하는 단락",
            "\t\t  ○ 탭과 공백이 섞였다",
            "",
            "　",
            "   ",
        ] {
            // 터지지 않는 것이 요구다. 무엇을 돌려주든 좋다.
            let _ = parse_marker(text);
        }
    }

    /// 앞 공백 칸 수(`indent`)는 **글자 수**라는 뜻을 유지해야 한다 —
    /// 레벨을 가르는 키이고, 바이트로 바뀌면 탭·공백 섞인 문서의 계층이 흔들린다.
    #[test]
    fn indent_stays_a_count_of_characters_not_bytes() {
        let m = parse_marker("   ○ 셋").expect("글머리");
        assert_eq!(m.indent, 3, "공백 3칸이어야 한다");
        let m = parse_marker("\t ○ 탭과 공백").expect("글머리");
        assert_eq!(m.indent, 2, "탭 1 + 공백 1 = 2칸");
    }
}
