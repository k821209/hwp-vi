//! 두벌식 한글 오토마타 — 터미널 안에서 조합한다.
//!
//! 왜 필요한가. 터미널 IME(fcitx/ibus)는 조합 중 글자를 **우리 화면 위에** 그린다
//! — 우리 버퍼는 커밋된 글자만 받으므로 조합 중인 음절을 보여줄 수 없고, 원격
//! 접속으로 들어간 세션에는 IME 가 아예 없다. 도스 한글이 하던 대로 직접 조합한다.
//!
//! 상태는 초성·중성·종성 세 칸뿐이고, 입력마다 "지금 글자를 확정하고 새로
//! 시작할지" 를 정한다. 확정된 글자는 [`Step::commit`] 으로 나가고 조합 중인
//! 글자는 [`Automaton::preedit`] 가 보여준다.

/// 초성 19 — 유니코드 음절 계산에 쓰는 순서 그대로.
const CHO: [char; 19] = [
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ',
    'ㅌ', 'ㅍ', 'ㅎ',
];
/// 중성 21
const JUNG: [char; 21] = [
    'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ', 'ㅙ', 'ㅚ', 'ㅛ', 'ㅜ', 'ㅝ', 'ㅞ',
    'ㅟ', 'ㅠ', 'ㅡ', 'ㅢ', 'ㅣ',
];
/// 종성 27 (없음은 0 이라 따로 둔다)
const JONG: [char; 27] = [
    'ㄱ', 'ㄲ', 'ㄳ', 'ㄴ', 'ㄵ', 'ㄶ', 'ㄷ', 'ㄹ', 'ㄺ', 'ㄻ', 'ㄼ', 'ㄽ', 'ㄾ', 'ㄿ', 'ㅀ', 'ㅁ',
    'ㅂ', 'ㅄ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];

/// 겹모음: (앞, 뒤) → 합친 것
const VOWEL_PAIRS: [(char, char, char); 7] = [
    ('ㅗ', 'ㅏ', 'ㅘ'),
    ('ㅗ', 'ㅐ', 'ㅙ'),
    ('ㅗ', 'ㅣ', 'ㅚ'),
    ('ㅜ', 'ㅓ', 'ㅝ'),
    ('ㅜ', 'ㅔ', 'ㅞ'),
    ('ㅜ', 'ㅣ', 'ㅟ'),
    ('ㅡ', 'ㅣ', 'ㅢ'),
];

/// 겹받침: (앞, 뒤) → 합친 것
const FINAL_PAIRS: [(char, char, char); 11] = [
    ('ㄱ', 'ㅅ', 'ㄳ'),
    ('ㄴ', 'ㅈ', 'ㄵ'),
    ('ㄴ', 'ㅎ', 'ㄶ'),
    ('ㄹ', 'ㄱ', 'ㄺ'),
    ('ㄹ', 'ㅁ', 'ㄻ'),
    ('ㄹ', 'ㅂ', 'ㄼ'),
    ('ㄹ', 'ㅅ', 'ㄽ'),
    ('ㄹ', 'ㅌ', 'ㄾ'),
    ('ㄹ', 'ㅍ', 'ㄿ'),
    ('ㄹ', 'ㅎ', 'ㅀ'),
    ('ㅂ', 'ㅅ', 'ㅄ'),
];

/// 두벌식 자판: 키 → 자모.
pub fn key_to_jamo(key: char) -> Option<char> {
    Some(match key {
        'q' => 'ㅂ', 'w' => 'ㅈ', 'e' => 'ㄷ', 'r' => 'ㄱ', 't' => 'ㅅ',
        'y' => 'ㅛ', 'u' => 'ㅕ', 'i' => 'ㅑ', 'o' => 'ㅐ', 'p' => 'ㅔ',
        'a' => 'ㅁ', 's' => 'ㄴ', 'd' => 'ㅇ', 'f' => 'ㄹ', 'g' => 'ㅎ',
        'h' => 'ㅗ', 'j' => 'ㅓ', 'k' => 'ㅏ', 'l' => 'ㅣ',
        'z' => 'ㅋ', 'x' => 'ㅌ', 'c' => 'ㅊ', 'v' => 'ㅍ',
        'b' => 'ㅠ', 'n' => 'ㅜ', 'm' => 'ㅡ',
        'Q' => 'ㅃ', 'W' => 'ㅉ', 'E' => 'ㄸ', 'R' => 'ㄲ', 'T' => 'ㅆ',
        'O' => 'ㅒ', 'P' => 'ㅖ',
        // 나머지 대문자는 소문자와 같다 (자판에 겹자모가 없는 자리)
        'A' => 'ㅁ', 'S' => 'ㄴ', 'D' => 'ㅇ', 'F' => 'ㄹ', 'G' => 'ㅎ',
        'H' => 'ㅗ', 'J' => 'ㅓ', 'K' => 'ㅏ', 'L' => 'ㅣ',
        'Y' => 'ㅛ', 'U' => 'ㅕ', 'I' => 'ㅑ',
        'Z' => 'ㅋ', 'X' => 'ㅌ', 'C' => 'ㅊ', 'V' => 'ㅍ',
        'B' => 'ㅠ', 'N' => 'ㅜ', 'M' => 'ㅡ',
        _ => return None,
    })
}

fn is_vowel(c: char) -> bool {
    JUNG.contains(&c)
}

fn cho_index(c: char) -> Option<usize> {
    CHO.iter().position(|x| *x == c)
}
fn jung_index(c: char) -> Option<usize> {
    JUNG.iter().position(|x| *x == c)
}
fn jong_index(c: char) -> Option<usize> {
    JONG.iter().position(|x| *x == c)
}

/// 초성이 될 수 있는 자음인가. `ㄳ` 같은 겹받침은 초성이 못 된다.
fn can_be_cho(c: char) -> bool {
    cho_index(c).is_some()
}

/// 겹받침을 쪼갠다. `ㄺ` → `(ㄹ, ㄱ)`.
fn split_final(c: char) -> Option<(char, char)> {
    FINAL_PAIRS
        .iter()
        .find(|(_, _, both)| *both == c)
        .map(|(a, b, _)| (*a, *b))
}

/// 겹모음을 쪼갠다.
fn split_vowel(c: char) -> Option<(char, char)> {
    VOWEL_PAIRS
        .iter()
        .find(|(_, _, both)| *both == c)
        .map(|(a, b, _)| (*a, *b))
}

/// 한 번의 입력이 낳은 결과.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Step {
    /// 확정되어 버퍼로 나가는 글자들
    pub commit: String,
    /// 이 입력을 자모로 처리하지 못했다 — 부른 쪽이 그대로 넣어야 한다
    pub passthrough: Option<char>,
}

/// 조합 상태.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Automaton {
    cho: Option<char>,
    jung: Option<char>,
    jong: Option<char>,
}

impl Automaton {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.cho.is_none() && self.jung.is_none() && self.jong.is_none()
    }

    /// 조합 중인 글자. 화면에 커서 자리에 보여 줄 것.
    pub fn preedit(&self) -> String {
        match (self.cho, self.jung, self.jong) {
            (Some(c), Some(v), jong) => {
                let (Some(ci), Some(vi)) = (cho_index(c), jung_index(v)) else {
                    return String::new();
                };
                let ji = jong.and_then(jong_index).map(|i| i + 1).unwrap_or(0);
                let code = 0xAC00 + (ci * 21 + vi) * 28 + ji;
                char::from_u32(code as u32).map(String::from).unwrap_or_default()
            }
            // 자음만, 모음만 — 자모 그대로 보여준다
            (Some(c), None, _) => c.to_string(),
            (None, Some(v), _) => v.to_string(),
            _ => String::new(),
        }
    }

    /// 조합을 끝내고 글자를 내놓는다. 모드를 나갈 때, 커서를 옮길 때 부른다.
    pub fn flush(&mut self) -> String {
        let out = self.preedit();
        *self = Self::default();
        out
    }

    /// 자모 하나를 넣는다.
    pub fn push_jamo(&mut self, j: char) -> Step {
        let mut step = Step::default();
        if is_vowel(j) {
            self.push_vowel(j, &mut step);
        } else {
            self.push_consonant(j, &mut step);
        }
        step
    }

    /// 키 하나를 넣는다. 자판에 없는 키는 조합을 끝내고 그대로 흘린다.
    pub fn push_key(&mut self, key: char) -> Step {
        match key_to_jamo(key) {
            Some(j) => self.push_jamo(j),
            None => Step {
                commit: self.flush(),
                passthrough: Some(key),
            },
        }
    }

    fn push_vowel(&mut self, v: char, step: &mut Step) {
        match (self.cho, self.jung, self.jong) {
            // 받침이 있는데 모음이 왔다 — 받침이 다음 글자의 초성으로 넘어간다
            // (간 + ㅏ → 가나). 겹받침이면 뒤쪽만 넘어간다 (갈ㄱ + ㅏ → 갈가).
            (Some(_), Some(_), Some(jong)) => {
                let (keep, moved) = match split_final(jong) {
                    Some((a, b)) => (Some(a), b),
                    None => (None, jong),
                };
                self.jong = keep;
                step.commit.push_str(&self.preedit());
                *self = Self {
                    cho: can_be_cho(moved).then_some(moved),
                    jung: Some(v),
                    jong: None,
                };
            }
            // 모음 + 모음 — 겹모음이 되면 합치고, 아니면 끊는다
            (_, Some(prev), None) => match VOWEL_PAIRS
                .iter()
                .find(|(a, b, _)| *a == prev && *b == v)
            {
                Some((_, _, both)) => self.jung = Some(*both),
                None => {
                    step.commit.push_str(&self.preedit());
                    *self = Self { cho: None, jung: Some(v), jong: None };
                }
            },
            // 초성만 있었다 — 중성을 채운다
            (Some(_), None, _) => self.jung = Some(v),
            // 빈 상태 — 모음 홀로
            (None, None, _) => self.jung = Some(v),
            // 초성 없이 받침이 있는 상태는 만들어지지 않는다. 그래도 조용히
            // 틀리는 것보다 글자를 지키는 쪽으로 처리한다.
            (None, Some(_), Some(_)) => {
                step.commit.push_str(&self.preedit());
                *self = Self { cho: None, jung: Some(v), jong: None };
            }
        }
    }

    fn push_consonant(&mut self, c: char, step: &mut Step) {
        match (self.cho, self.jung, self.jong) {
            // 초성+중성 뒤의 자음 — 받침이 될 수 있으면 받침으로
            (Some(_), Some(_), None) => {
                if jong_index(c).is_some() {
                    self.jong = Some(c);
                } else {
                    step.commit.push_str(&self.preedit());
                    *self = Self { cho: can_be_cho(c).then_some(c), jung: None, jong: None };
                    if self.cho.is_none() {
                        step.commit.push(c);
                    }
                }
            }
            // 받침이 있는데 자음이 또 왔다 — 겹받침이 되면 합치고, 아니면 끊는다
            (Some(_), Some(_), Some(prev)) => match FINAL_PAIRS
                .iter()
                .find(|(a, b, _)| *a == prev && *b == c)
            {
                Some((_, _, both)) => self.jong = Some(*both),
                None => {
                    step.commit.push_str(&self.preedit());
                    *self = Self { cho: can_be_cho(c).then_some(c), jung: None, jong: None };
                    if self.cho.is_none() {
                        step.commit.push(c);
                    }
                }
            },
            // 자음 뒤의 자음 — 앞 자음을 내보내고 새로 시작
            (Some(prev), None, _) => {
                step.commit.push(prev);
                *self = Self { cho: can_be_cho(c).then_some(c), jung: None, jong: None };
                if self.cho.is_none() {
                    step.commit.push(c);
                }
            }
            // 모음만 있었다 — 모음을 내보내고 자음으로 새로 시작
            (None, Some(v), _) => {
                step.commit.push(v);
                *self = Self { cho: can_be_cho(c).then_some(c), jung: None, jong: None };
                if self.cho.is_none() {
                    step.commit.push(c);
                }
            }
            (None, None, _) => {
                self.cho = can_be_cho(c).then_some(c);
                if self.cho.is_none() {
                    step.commit.push(c);
                }
            }
        }
    }

    /// 지우기. 조합 중이면 자모 하나를 되돌리고, 비어 있으면 `false` 를 내서
    /// 부른 쪽이 버퍼의 글자를 지우게 한다.
    pub fn backspace(&mut self) -> bool {
        if let Some(jong) = self.jong {
            self.jong = split_final(jong).map(|(a, _)| a);
            return true;
        }
        if let Some(jung) = self.jung {
            match split_vowel(jung) {
                Some((a, _)) => self.jung = Some(a),
                None => self.jung = None,
            }
            return true;
        }
        if self.cho.is_some() {
            self.cho = None;
            return true;
        }
        false
    }
}

/// 키 열을 한 번에 — 시험과 붙여넣기에 쓴다.
pub fn type_keys(keys: &str) -> String {
    let mut a = Automaton::new();
    let mut out = String::new();
    for k in keys.chars() {
        let step = a.push_key(k);
        out.push_str(&step.commit);
        if let Some(c) = step.passthrough {
            out.push(c);
        }
    }
    out.push_str(&a.flush());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_plain_syllables() {
        assert_eq!(type_keys("rk"), "가");
        assert_eq!(type_keys("gksrmf"), "한글");
        assert_eq!(type_keys("dkssud"), "안녕");
    }

    #[test]
    fn carries_a_final_into_the_next_syllable() {
        // 간 + ㅏ → 가나. 받침이 다음 초성으로 넘어간다.
        assert_eq!(type_keys("rksk"), "가나");
        // 먹어 — ㄱ 받침이 ㅇ 자리로
        assert_eq!(type_keys("ajrdj"), "먹어");
    }

    #[test]
    fn builds_and_splits_compound_finals() {
        assert_eq!(type_keys("dkfg"), "앓");
        // 갈ㄱ + ㅏ → 갈가 (겹받침의 뒤쪽만 넘어간다)
        assert_eq!(type_keys("rkfrk"), "갈가");
        // 없다
        assert_eq!(type_keys("djqtek"), "없다");
    }

    #[test]
    fn builds_compound_vowels() {
        assert_eq!(type_keys("rhk"), "과");
        assert_eq!(type_keys("dnjs"), "원");
    }

    #[test]
    fn real_words_from_the_corpus() {
        assert_eq!(type_keys("rhkdjqwltltj"), "과업지시서");
        assert_eq!(type_keys("thdydPtks"), "소요예산");
        assert_eq!(type_keys("rjatn"), "검수");
    }

    #[test]
    fn non_layout_keys_pass_through_and_end_composition() {
        // 숫자·기호는 조합을 끝내고 그대로 나간다
        assert_eq!(type_keys("rk1"), "가1");
        assert_eq!(type_keys("rk "), "가 ");
        assert_eq!(type_keys("rk, sk"), "가, 나");
    }

    #[test]
    fn preedit_shows_the_syllable_being_built() {
        let mut a = Automaton::new();
        a.push_key('r');
        assert_eq!(a.preedit(), "ㄱ", "자음만 있으면 자모를 보여준다");
        a.push_key('k');
        assert_eq!(a.preedit(), "가");
        a.push_key('s');
        assert_eq!(a.preedit(), "간");
        assert_eq!(a.flush(), "간");
        assert!(a.is_empty());
    }

    #[test]
    fn backspace_walks_back_through_the_jamo() {
        let mut a = Automaton::new();
        for k in "rks".chars() {
            a.push_key(k);
        }
        assert_eq!(a.preedit(), "간");
        assert!(a.backspace());
        assert_eq!(a.preedit(), "가");
        assert!(a.backspace());
        assert_eq!(a.preedit(), "ㄱ");
        assert!(a.backspace());
        assert_eq!(a.preedit(), "");
        // 조합이 비면 false — 버퍼의 글자를 지우라는 뜻
        assert!(!a.backspace());
    }

    #[test]
    fn backspace_splits_a_compound_before_removing_it() {
        let mut a = Automaton::new();
        for k in "rhk".chars() {
            a.push_key(k);
        }
        assert_eq!(a.preedit(), "과");
        a.backspace();
        assert_eq!(a.preedit(), "고", "겹모음은 쪼개진 뒤에 지워진다");

        let mut b = Automaton::new();
        for k in "dkfr".chars() {
            b.push_key(k);
        }
        assert_eq!(b.preedit(), "앍");
        b.backspace();
        assert_eq!(b.preedit(), "알", "겹받침도 쪼개진다");
    }

    #[test]
    fn shifted_keys_give_tense_consonants() {
        assert_eq!(type_keys("Rk"), "까");
        assert_eq!(type_keys("Tk"), "싸");
        assert_eq!(type_keys("rO"), "걔");
    }
}
