//! 표시 폭 기준 줄바꿈과, **문서에서 측정한** 단 폭.
//!
//! PLAN.md §2-3 초안은 "페이지는 추정밖에 못 한다"고 했다. rhwp 가 조판값을 주니
//! 추정하지 않는다 — 다만 터미널은 고정폭이고 한글은 2칸이라, 한 줄에 몇 칸이
//! 들어가는지는 글꼴 메트릭 없이 알아내야 한다.
//!
//! 글꼴을 읽지 않고 알아내는 길이 있다. 단락이 `n` 줄로 조판됐고 표시 폭이 `w`
//! 칸이면 한 줄 용량 `c` 는 `c ≥ ceil(w / n)` 이다 — `n` 줄에 담겼으니 그보다
//! 좁을 수 없다. 마지막 줄은 보통 덜 차 있으므로 이 값은 **하한**이다.
//!
//! 처음에는 상한(`c < w/(n-1)`)까지 써서 구간을 교집합했다. 실측에서 그 교집합이
//! 14개 중 9개 문서에서 비었다 — 제목과 본문의 **글꼴 크기가 다르면 단일 폭이
//! 애초에 존재하지 않기** 때문이다. 버그가 아니라 문서의 사실이다.
//!
//! 그래서 하한만 모으고 그 **최댓값**을 쓴다. 글꼴이 작을수록 한 줄에 더 들어가
//! `w/n` 이 커지므로, 최댓값은 가장 작은 글꼴 — 곧 본문 — 의 단 폭에 수렴한다.
//! 제목처럼 큰 글꼴 단락은 우리 화면에서 문서보다 늦게 접힌다. 화면에 그 사실을
//! 적는다.

use std::borrow::Cow;
use unicode_width::UnicodeWidthChar;

/// 터미널 표시 폭(칸). 한글·한자·일본어는 2칸.
/// **터미널이 East Asian Ambiguous 글자를 몇 칸으로 그리는가.**
///
/// 이것이 표가 어긋나는 가장 큰 원인이다. `─ │ ┼ ├ ┤ ┬ ┴ ┌ ┐ └ ┘` 같은
/// **표를 그리는 선 문자가 전부 Ambiguous** 다 — 코퍼스를 찍은 줄의 76% 에
/// 들어 있고, 54,754개 중 53,000개가 선 문자다.
///
/// 터미널이 2칸으로 그리면 선은 두 배가 되는데 한글은 그대로여서 끝선이
/// 어긋난다. 실측:
///
/// ```text
/// 1칸으로 볼 때   ┌────┬────┐ 11칸 · │가나│다라│ 11칸   → 맞는다
/// 2칸으로 볼 때   ┌────┬────┐ 22칸 · │가나│다라│ 14칸   → 8칸 어긋난다
/// ```
///
/// 글자 쪽도 같다: `Ⅰ. 사업추진 방향` 16↔17칸, `① 첫째 · 둘째` 13↔15칸.
///
/// 유니코드가 정하지 않고 **터미널이 정하는** 값이라 코드로 단정할 수 없다.
/// 그래서 (1) 띄울 때 터미널에 직접 물어보고, (2) 사람이 `:set amb=` 로
/// 덮을 수 있게 하고, (3) 못 물어보면 좁은 쪽으로 둔다 — 요즘 터미널 대부분이
/// 그쪽이고, 틀렸을 때 표가 넓어지는 쪽보다 좁아지는 쪽이 읽기 쉽다.
static AMBIGUOUS_WIDE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Ambiguous 를 2칸으로 센다 (`:set amb=wide`).
pub fn set_ambiguous_wide(wide: bool) {
    AMBIGUOUS_WIDE.store(wide, std::sync::atomic::Ordering::Relaxed);
}

pub fn ambiguous_wide() -> bool {
    AMBIGUOUS_WIDE.load(std::sync::atomic::Ordering::Relaxed)
}

/// 표시 폭 `cells` 가 되도록 공백을 채운다. 넘치면 자른다.
///
/// **`format!("{s:<10}")` 를 쓰면 안 된다** — 그쪽은 **글자 수**로 채우므로
/// 한글(한 글자 두 칸)이 섞이면 두 배로 넓어진다. 메뉴 항목을 그렇게 채웠다가
/// 글자가 테두리를 넘어 테두리를 지웠다.
pub fn pad_to(s: &str, cells: usize) -> String {
    let w = display_width(s);
    if w >= cells {
        // 자를 때도 2칸 글자를 쪼개지 않는다
        let mut out = String::new();
        let mut used = 0usize;
        for c in s.chars() {
            let cw = char_width(c);
            if used + cw > cells {
                break;
            }
            out.push(c);
            used += cw;
        }
        out.push_str(&" ".repeat(cells - used));
        out
    } else {
        format!("{s}{}", " ".repeat(cells - w))
    }
}

/// 네모를 그리는 글자. **Ambiguous 를 2칸으로 그리는 터미널에서는 ASCII** 다 —
/// `─ │ ┌ ┐ └ ┘` 가 전부 Ambiguous 라서 그런 터미널에서는 선이 2칸을 먹고
/// 격자가 어긋난다. 유니코드에 좁다고 보장된 선 문자가 없으므로 `+ - |` 로
/// 간다. 모양을 잃는 대신 맞는다.
pub fn box_h() -> &'static str {
    if ambiguous_wide() { "-" } else { "─" }
}

pub fn box_v() -> &'static str {
    if ambiguous_wide() { "|" } else { "│" }
}

/// 네모의 네 귀퉁이 — 좁은 쪽에서는 모양을 가리고, 넓은 쪽에서는 전부 `+`.
pub fn box_corner(top: bool) -> &'static str {
    if ambiguous_wide() {
        "+"
    } else if top {
        "┌"
    } else {
        "┐"
    }
}

/// **꾸밈틀의 글자를 넓은 터미널에서도 1칸인 것으로 바꾼다.**
///
/// 이 프로그램은 폭을 `char_width` 로 재지만 ratatui 는 언제나
/// `unicode-width` 의 좁은 폭(Ambiguous = 1)으로 잰다. 둘이 어긋나는 글자를
/// 화면에 내면 넓게 그리는 터미널에서 줄이 밀린다 — 실측(2026-10-10): 그런
/// 줄이 23,040줄 중 1,070줄이었고 대부분이 덮개(도움·메뉴) 테두리와
/// 안내줄이었다.
///
/// 그래서 **우리가 고른 글자**는 ASCII 로 바꾼다. 바꿀 수 있는 것만 바꾼다 —
/// 폭이 같은(1칸) 대체 글자가 있는 것들이다.
///
/// **본문 글자에는 쓰지 않는다.** 공문서는 `AI·디지털` 처럼 가운뎃점을
/// 구분자로 쓰는데 `.` 으로 바꾸면 `AI.디지털` 이 되어 문서를 잘못 보여 주는
/// 셈이다. 문서를 고쳐 보여 주는 것과 우리 장식을 고르는 것은 다르다.
pub fn narrow(s: &str) -> Cow<'_, str> {
    if !ambiguous_wide() || !s.chars().any(needs_narrowing) {
        return Cow::Borrowed(s);
    }
    Cow::Owned(s.chars().map(narrow_char).collect())
}

fn needs_narrowing(c: char) -> bool {
    narrow_char(c) != c
}

/// 꾸밈틀에서 쓰는 Ambiguous 글자 → 폭 1칸이 보장된 ASCII.
fn narrow_char(c: char) -> char {
    match c {
        '·' => '.',
        '→' => '>',
        '←' => '<',
        '↑' => '^',
        '↓' => 'v',
        // 줄표 — 이 프로그램의 글이 자주 쓴다. 덮개 제목과 상태줄에 들어 있었고,
        // 넓은 터미널에서 뒤의 공백 한 칸을 먹었다(`도움 —? 또는 …`).
        '—' | '–' => '-',
        '…' => '.',
        '‘' | '’' => '\'',
        '“' | '”' => '"',
        '×' => 'x',
        '│' => '|',
        '─' => '-',
        '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' => '+',
        other => other,
    }
}

/// 글자 하나의 표시 폭. **폭을 재는 곳은 전부 이 함수를 지나야 한다** —
/// `unicode-width` 를 직접 부르면 Ambiguous 설정이 그 자리만 비껴간다.
pub fn char_width(c: char) -> usize {
    char_width_with(c, ambiguous_wide())
}

/// 정책을 **인자로** 받는 판. 시험이 전역을 건드리지 않고 두 정책을 다 볼 수
/// 있어야 한다 — 전역을 뒤집으면 같이 돌는 다른 시험이 영향을 받는다.
pub fn char_width_with(c: char, wide: bool) -> usize {
    match c {
        '\t' => 1,
        _ if wide => c.width_cjk().unwrap_or(0),
        _ => c.width().unwrap_or(0),
    }
}

pub fn display_width_with(s: &str, wide: bool) -> usize {
    s.chars().map(|c| char_width_with(c, wide)).sum()
}

pub fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| match c {
            // **탭은 한 칸으로 센다.** `unicode-width` 는 탭에 `None` 을 주고
            // 우리는 그것을 0 으로 받았는데, 터미널은 탭을 다음 탭 스톱까지
            // 띄운다(보통 8칸). 0 으로 세고 1~8칸을 그리면 그 줄이 밀린다 —
            // 공개된 한글문서파일형식 5.0 스펙 문서(70쪽)에서 탭이 든 줄이
            // 109개이고 표 169개 중 10개의 테두리가 어긋났다.
            //
            // 세는 쪽과 그리는 쪽을 **1글자 = 1칸 = 공백 하나**로 맞춘다.
            // [`for_screen`] 가 그리기 직전에 공백으로 바꾼다.
            other => char_width(other),
        })
        .sum()
}

/// 화면에 내보낼 글자로 바꾼다 — **그리기 직전에만** 부른다.
///
/// 지금 하는 일은 탭을 공백 하나로 바꾸는 것뿐이다. HWP 의 탭은 탭 표에
/// HWPUNIT 으로 적힌 조판 지시이고 터미널은 그것을 지킬 수 없다. 글자 수를
/// 바꾸지 않는 것이 중요하다 — 커서 오프셋이 글자 수로 계산되므로 탭 하나를
/// 공백 여러 개로 펴면 커서가 어긋난다.
///
/// 글 모형에는 탭을 그대로 둔다. 백엔드의 `--offset` 도 탭을 한 글자로 세고,
/// 저장할 때 글자가 바뀌면 자가검증이 거부한다.
pub fn for_screen(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains('\t') {
        std::borrow::Cow::Owned(s.replace('\t', " "))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// 측정된 단 폭.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnWidth {
    /// 쓸 폭 — 관측된 하한들의 최댓값.
    pub cells: usize,
    /// 관측된 하한들의 최솟값. `cells` 와 많이 벌어지면 문서에 글꼴 크기가
    /// 여러 가지라는 뜻이다.
    pub narrowest: usize,
    /// 측정에 쓴 단락 수. 0 이면 근거가 없다.
    pub samples: usize,
}

impl ColumnWidth {
    pub fn chosen(&self) -> usize {
        self.cells
    }

    pub fn is_measured(&self) -> bool {
        self.samples > 0 && self.cells > 0
    }

    /// 문서 안에서 단락별 폭이 갈리는가 — 제목·본문 글꼴이 다르면 벌어진다.
    pub fn is_mixed(&self) -> bool {
        self.samples > 1 && self.narrowest * 4 < self.cells * 3
    }
}

/// `(표시폭, 조판된 줄 수)` 관측들로 단 폭을 잰다.
pub fn infer_column_width(observations: &[(usize, usize)]) -> Option<ColumnWidth> {
    let mut cells = 0usize;
    let mut narrowest = usize::MAX;
    let mut samples = 0usize;

    for &(w, n) in observations {
        if n < 2 || w == 0 {
            continue; // 한 줄 단락은 하한을 주지 않는다
        }
        samples += 1;
        let low = w.div_ceil(n);
        cells = cells.max(low);
        narrowest = narrowest.min(low);
    }

    if samples == 0 {
        return None;
    }
    Some(ColumnWidth { cells, narrowest, samples })
}

/// 표시 폭 `width` 칸에 맞춰 접는다. 공백에서 끊고, 끊을 곳이 없으면 글자에서 끊는다.
///
/// 한글은 단어 사이 공백이 드물어 글자 단위 접힘이 기본이 된다 — 한글 조판이
/// 원래 그렇게 동작한다.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_w = 0usize;
    // 마지막으로 본 공백의 (line 내 바이트 위치, 그 시점 폭)
    let mut last_space: Option<(usize, usize)> = None;

    for ch in text.chars() {
        // **정책을 따르는 함수로 잰다.** 여기서 `unicode-width` 를 직접 부르면
        // 접는 폭만 좁은 기준으로 계산되고, 그린 칸은 넓은 기준이라 글자가
        // 옆 칸을 먹는다 — s03.hwpx 에서 `• 1,000,000원×25명(…` 이 27칸인데
        // 20칸 칸에 그대로 들어가 `|` 를 지웠다.
        let cw = char_width(ch);
        if line_w + cw > width && !line.is_empty() {
            match last_space {
                // 공백에서 끊는다. 공백 자체는 버린다.
                Some((byte, _)) if byte > 0 => {
                    let rest = line[byte..].trim_start_matches(' ').to_string();
                    line.truncate(byte);
                    lines.push(std::mem::take(&mut line));
                    line = rest;
                    line_w = display_width(&line);
                }
                _ => {
                    lines.push(std::mem::take(&mut line));
                    line_w = 0;
                }
            }
            last_space = None;
        }
        if ch == ' ' {
            last_space = Some((line.len(), line_w));
        }
        line.push(ch);
        line_w += cw;
    }
    lines.push(line);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hangul_is_two_cells() {
        assert_eq!(display_width("과업"), 4);
        assert_eq!(display_width("ab"), 2);
        assert_eq!(display_width("과a"), 3);
    }

    #[test]
    fn width_is_the_largest_lower_bound() {
        // 폭 100칸이 3줄 → 한 줄에 최소 34칸
        let one = infer_column_width(&[(100, 3)]).unwrap();
        assert_eq!(one.cells, 34);

        // 폭 80칸이 2줄 → 최소 40칸. 둘 중 큰 쪽을 쓴다.
        let two = infer_column_width(&[(100, 3), (80, 2)]).unwrap();
        assert_eq!(two.cells, 40);
        assert_eq!(two.narrowest, 34);
        assert!(two.is_measured());
    }

    #[test]
    fn mixed_font_sizes_show_up_as_a_spread() {
        // 큰 글꼴 단락(한 줄 20칸)과 본문(한 줄 70칸)이 섞인 문서
        let mixed = infer_column_width(&[(40, 2), (140, 2)]).unwrap();
        assert_eq!((mixed.cells, mixed.narrowest), (70, 20));
        assert!(mixed.is_mixed(), "글꼴이 섞였음을 알려야 한다");

        let uniform = infer_column_width(&[(140, 2), (138, 2)]).unwrap();
        assert!(!uniform.is_mixed());
    }

    #[test]
    fn single_line_paragraphs_give_no_measurement() {
        assert!(infer_column_width(&[(30, 1), (12, 1)]).is_none());
    }

    #[test]
    fn wrap_respects_display_width() {
        let lines = wrap("가나다라마바사", 6);
        assert_eq!(lines, vec!["가나다", "라마바", "사"]);
        for l in &lines {
            assert!(display_width(l) <= 6);
        }
    }

    #[test]
    fn wrap_prefers_spaces() {
        assert_eq!(wrap("alpha beta gamma", 11), vec!["alpha beta", "gamma"]);
    }

    #[test]
    fn wrapped_lines_rejoin_to_the_source_ignoring_break_spaces() {
        let src = "○ 소요예산: 금19,800,000원(부가세 포함)";
        let joined: String = wrap(src, 20).join("");
        assert_eq!(joined.replace(' ', ""), src.replace(' ', ""));
    }

    /// **탭은 한 칸으로 센다.** 0 으로 세면 그린 줄이 밀린다.
    #[test]
    fn a_tab_counts_as_one_cell_and_becomes_a_space_on_screen() {
        assert_eq!(display_width("a\tb"), 3);
        assert_eq!(display_width("\t"), 1);
        // 글자 수는 바뀌지 않는다 — 커서 오프셋이 글자 수로 계산된다
        let screen = for_screen("가\t나");
        assert_eq!(screen, "가 나");
        assert_eq!(screen.chars().count(), "가\t나".chars().count());
        assert_eq!(display_width(&screen), display_width("가\t나"));
        // 탭이 없으면 복사하지 않는다
        assert!(matches!(for_screen("가나"), std::borrow::Cow::Borrowed(_)));
    }

    /// **좁힌 글자는 어느 폭 정책에서도 1칸이어야 한다.** 이게 틀리면 좁히는
    /// 일이 문제를 옮기기만 한다.
    #[test]
    fn narrowed_glyphs_are_one_cell_under_either_policy() {
        let decorations = "·→←↑↓│─┌┐└┘├┤┬┴┼—–…‘’“”×";
        let narrowed: String = {
            set_ambiguous_wide(true);
            let out = narrow(decorations).into_owned();
            set_ambiguous_wide(false);
            out
        };
        assert_ne!(narrowed, decorations, "넓은 정책에서 하나도 바뀌지 않았다");
        for c in narrowed.chars() {
            assert_eq!(char_width_with(c, true), 1, "{c:?} 가 넓은 쪽에서 1칸이 아니다");
            assert_eq!(char_width_with(c, false), 1, "{c:?} 가 좁은 쪽에서 1칸이 아니다");
        }
        // 글자 수가 같아야 한다 — 한 칸을 두 글자로 늘리면(`→` → `->`) 폭이 밀린다
        assert_eq!(
            narrowed.chars().count(),
            decorations.chars().count(),
            "좁히면서 글자 수가 달라졌다"
        );
    }

    /// **좁은 정책에서는 아무것도 바꾸지 않는다.** 보통의 터미널에서 모양이
    /// 나빠질 이유가 없다.
    #[test]
    fn a_narrow_terminal_keeps_the_pretty_glyphs() {
        set_ambiguous_wide(false);
        let s = " ↑↓ 고르기 · ←→ 묶음 — Esc 닫기";
        assert_eq!(narrow(s), s);
        assert!(matches!(narrow(s), std::borrow::Cow::Borrowed(_)), "쓸데없이 복사했다");
    }
}
