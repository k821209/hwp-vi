//! 화면 글자 자리 ↔ rhwp 편집 오프셋.
//!
//! **rhwp 의 `--offset` 은 컨트롤을 세지 않는다.** 실측으로 확정했다
//! (s13.hwp 단락 0, s00.hwp 단락 31 — 둘 다 컨트롤이 섞인 단락):
//!
//! ```text
//!   원문   ○○대학￼ ○○￼학과        (￼ = 인라인 컨트롤, 증인이 U+FFFC 로 표시)
//!   offset 3 → ○○대◆학￼ ○○￼학과     3번째 글자 뒤
//!   offset 4 → ○○대학￼◆ ○○￼학과     4번째 글자 뒤 — 컨트롤을 건너뛴다
//! ```
//!
//! 그래서 화면에서 n번째 글자가 오프셋 n이 아니다. 이 모듈이 그 변환을 맡고,
//! 편집 경로는 반드시 여기를 지나간다 — 한 군데서만 틀리게 하려고.

/// 증인이 인라인 컨트롤을 표시하는 글자.
pub const CONTROL: char = '\u{FFFC}';

/// 화면 글자 자리를 rhwp 오프셋으로. 앞쪽의 컨트롤은 세지 않는다.
pub fn to_rhwp(text: &str, screen_index: usize) -> usize {
    text.chars()
        .take(screen_index)
        .filter(|c| *c != CONTROL)
        .count()
}

/// rhwp 오프셋을 화면 글자 자리로.
///
/// 오프셋 `k` 는 "컨트롤 아닌 글자 `k` 개를 지난 자리" 이고, rhwp 는 거기서
/// 뒤따르는 컨트롤을 **건너뛴 뒤** 넣는다. 그 동작을 그대로 돌려준다.
pub fn to_screen(text: &str, offset: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut seen = 0usize;
    let mut i = 0usize;
    while i < chars.len() && seen < offset {
        if chars[i] != CONTROL {
            seen += 1;
        }
        i += 1;
    }
    // rhwp 가 하는 대로 뒤따르는 컨트롤을 건너뛴다
    while i < chars.len() && chars[i] == CONTROL {
        i += 1;
    }
    i
}

/// 이 단락에서 rhwp 가 받아들이는 가장 큰 오프셋 (= 컨트롤 아닌 글자 수).
/// `--offset` 이 이 값을 넘으면 rhwp 는 인자 오류를 낸다.
pub fn max_offset(text: &str) -> usize {
    text.chars().filter(|c| *c != CONTROL).count()
}

/// 컨트롤을 뗀 본문 — 사람이 읽고 세는 글자들.
pub fn visible(text: &str) -> String {
    text.chars().filter(|c| *c != CONTROL).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 실측값 그대로. s00.hwp 단락 31 — 컨트롤이 중간에 있다.
    const MIXED: &str = "○○대학\u{FFFC} ○○\u{FFFC}학과";
    /// s13.hwp 단락 0 — 컨트롤이 앞에 있다.
    const LEADING: &str = "\u{FFFC}\u{FFFC}과 업 지 시 서";

    #[test]
    fn measured_offsets_land_where_rhwp_put_them() {
        // offset → 화면 자리. 실측표(모듈 주석)와 같아야 한다.
        for (offset, screen) in [(0, 0), (1, 1), (2, 2), (3, 3), (4, 5), (5, 6), (6, 7)] {
            assert_eq!(
                to_screen(MIXED, offset),
                screen,
                "offset {offset} 은 화면 {screen} 번째여야 한다"
            );
        }
        // 선두 컨트롤 2개 — offset 0 이 화면 2번째다
        for (offset, screen) in [(0, 2), (1, 3), (2, 4), (3, 5)] {
            assert_eq!(to_screen(LEADING, offset), screen, "offset {offset}");
        }
    }

    #[test]
    fn screen_to_offset_is_the_inverse_where_it_can_be() {
        // 컨트롤 자리가 아닌 모든 화면 자리는 왕복한다
        for (i, c) in MIXED.chars().enumerate() {
            if c == CONTROL {
                continue;
            }
            let off = to_rhwp(MIXED, i);
            assert_eq!(to_screen(MIXED, off), i, "화면 {i} 번째 글자 {c:?}");
        }
    }

    #[test]
    fn max_offset_is_the_visible_length() {
        assert_eq!(max_offset(MIXED), 9);
        assert_eq!(max_offset(LEADING), 9);
        assert_eq!(visible(LEADING), "과 업 지 시 서");
        // 컨트롤만 있는 단락은 오프셋 0 밖에 못 받는다 — rhwp 가 1 이상에서
        // 인자 오류를 냈다(s00.hwp 단락 0 으로 실측).
        assert_eq!(max_offset("\u{FFFC}\u{FFFC}\u{FFFC}"), 0);
    }

    #[test]
    fn end_of_paragraph_is_addressable() {
        // "--offset 가 문단 길이와 같으면 끝에 붙인다" — rhwp 도움말
        let n = max_offset(MIXED);
        assert_eq!(to_screen(MIXED, n), MIXED.chars().count());
    }
}
