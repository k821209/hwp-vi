//! 한글 음절 → 한자 후보.
//!
//! HWP 는 이 일을 `F9`(한자로 바꾸기)로 한다: 커서 앞의 한글 음절을 보고 같은
//! 음으로 읽는 한자를 늘어놓는다. 공문서에 `課業` `株` `外` `第` 같은 글자가
//! 섞이는데, 터미널에는 그걸 넣을 길이 없다.
//!
//! **표는 짓지 않았다 — 가져왔다.** 한글 음과 한자의 대응은 사전 데이터이고,
//! 지어내면 틀린 글자를 가르친다. 출처는 libhangul 의 `data/hanja/hanja.txt`
//! 이고 **BSD-3-Clause** 다(libhangul 본체는 LGPL 이지만 이 데이터 파일은 따로
//! BSD-3 이다 — 그래서 MIT 인 이 프로젝트에 담을 수 있다). 저작권 표시와
//! 라이선스 전문을 `crates/hwp-core/data/` 에 함께 둔다.
//!
//! **`vendor/` 에 두지 않는다.** 거기는 `scripts/fetch-rhwp.sh` 가 받아 오는
//! 자리이고 `.gitignore` 에 들어 있다 — 처음에 거기 뒀다가 `git add -A` 가
//! 조용히 빼놓아서 **새로 클론한 기계에서 빌드가 깨졌다**(2026-10-11, 맥에서
//! 보고받았다). 컴파일 시점에 박아 넣는 데이터는 받아 오는 것이 아니라 소스다.
//!
//! 담은 것: **키가 한글 음절 한 글자**이고 값이 한자 한 글자인 항목 — 555음절
//! 28,472자, 311 KB. 원본의 낱말 항목 22만여 개(`과업:課業`)는 6.3 MB 라 넣지
//! 않았다. 음절 단위 변환이 한자 키의 본래 동작이고, 낱말은 음절을 이어 바꾸면
//! 된다(`과`→課, `업`→業).
//!
//! 순서는 원본 그대로다 — 중요한 글자가 앞에 온다(`한` → 韓 漢 寒 限 閑 恨).
//! 가나다순으로 다시 늘어놓으면 매번 찾아 헤매게 된다.

/// 음절 표. 한 줄에 한 음절, 탭으로 `한자<공백>뜻` 들.
const TABLE: &str = include_str!("../data/hanja-syllables.txt");

/// 이 음절로 읽는 한자들 — `(한자, 뜻)`. 뜻이 빈 것은 사전에 새김이 없는
/// 드문 글자다(목록 뒤쪽에 몰려 있다).
///
/// 311 KB 를 줄마다 훑는다. 찾는 줄 하나만 보면 되므로 앞서 표를 지도로 펼칠
/// 이유가 없다 — 한 번 쓰고 끝나는 조회다.
pub fn candidates(syllable: char) -> Vec<(char, &'static str)> {
    if !is_hangul_syllable(syllable) {
        return Vec::new();
    }
    let mut key = String::with_capacity(4);
    key.push(syllable);
    for line in TABLE.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut fields = line.split('\t');
        let Some(head) = fields.next() else { continue };
        if head != key {
            continue;
        }
        return fields
            .filter_map(|f| {
                let mut it = f.chars();
                let c = it.next()?;
                let rest = f[c.len_utf8()..].trim();
                Some((c, rest))
            })
            .collect();
    }
    Vec::new()
}

/// 한글 음절(가~힣)인가. 자모(`ㄱ`)와 조합 중인 글자는 아니다.
pub fn is_hangul_syllable(c: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&c)
}

/// 표가 아는 음절 수. `hwp doctor` 가 말한다.
pub fn syllable_count() -> usize {
    TABLE.lines().filter(|l| !l.starts_with('#') && !l.is_empty()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **흔한 음절이 흔한 글자를 먼저 준다.** 순서가 원본과 달라지면 쓸 때마다
    /// 뒤에서 찾게 된다 — 가나다순으로 다시 늘어놓고 싶은 유혹의 자리다.
    #[test]
    fn a_common_syllable_offers_the_common_character_first() {
        for (syl, want) in [
            ('한', '韓'),
            ('과', '果'),
            ('업', '業'),
            ('주', '主'),
            ('외', '外'),
            ('제', '弟'),
            ('대', '大'),
        ] {
            let c = candidates(syl);
            assert!(!c.is_empty(), "{syl} 의 후보가 없다");
            assert_eq!(c[0].0, want, "{syl} 의 첫 후보");
        }
    }

    /// 후보는 **한자 한 글자**여야 한다. 두 글자가 들어오면 바꿀 때 길이가
    /// 어긋나고, 한글이 들어오면 바꾼 보람이 없다.
    #[test]
    fn every_candidate_is_a_single_han_character() {
        let mut n = 0usize;
        for line in TABLE.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let syl = line.chars().next().expect("음절");
            for (c, _) in candidates(syl) {
                n += 1;
                let o = c as u32;
                let han = (0x3400..=0x4DBF).contains(&o)
                    || (0x4E00..=0x9FFF).contains(&o)
                    || (0xF900..=0xFAFF).contains(&o)
                    || (0x20000..=0x2FA1F).contains(&o);
                assert!(han, "{syl} 의 후보 {c:?}(U+{o:04X}) 가 한자가 아니다");
            }
        }
        assert!(n > 20_000, "후보가 {n}개뿐이다 — 표가 깨졌다");
    }

    /// 음절이 아닌 것을 물으면 **빈 손**이다. 자모(`ㄱ`)나 한자를 물었을 때
    /// 엉뚱한 줄을 집으면 커서 앞 글자를 멋대로 바꾼다.
    #[test]
    fn asking_about_something_that_is_not_a_syllable_gives_nothing() {
        for c in ['ㄱ', 'A', '漢', '○', ' ', '1'] {
            assert!(candidates(c).is_empty(), "{c:?} 가 후보를 냈다");
        }
    }

    #[test]
    fn the_table_knows_every_syllable_the_source_had() {
        assert_eq!(syllable_count(), 555, "음절 수가 달라졌다");
    }
}
