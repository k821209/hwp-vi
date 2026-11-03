//! HWPX(ZIP + OWPML)를 **우리 손으로 고친다.**
//!
//! 왜 여기까지 왔나. 백엔드(`rhwp`)의 `scaffold` 가 받는 칸은 제목·글꼴 이름·
//! 용지 크기·블록뿐이어서 **글자 크기와 여백을 지정할 수 없다**. 그런데 국내
//! 제출 문서는 그것을 본문에 못 박는다 — 사용자 보고(hwp-vi#4):
//!
//! > A4규격: 글자 13포인트, 줄 간격 160%, 상하여백 15, 좌우여백 25
//!
//! 그리고 그 서식은 **원고 A4 매당 정산**이다. 10pt·여백 30mm 로 나오면 같은
//! 글이 더 적은 쪽에 담기므로 서식 위반이자 정산 근거의 소실이다.
//!
//! HWPX 는 ZIP + XML 이라 우리가 고칠 수 있다. 실측으로 확인했다 — 생성물의
//! `charPr/@height` 와 `hp:margin` 을 고쳐 다시 묶으면 **백엔드와 우리 증인이
//! 둘 다 받고 글자도 그대로**다.
//!
//! ## 지키는 두 가지
//!
//! 1. **`mimetype` 은 맨 앞에 무압축으로.** OPC 규칙이고, 어기면 한글이 못
//!    연다. 이 모듈이 다시 묶을 때 그 순서를 강제한다.
//! 2. **읽지 못하는 것은 쓰지 않는다.** 여기서 고치는 것은 증인이 되읽을 수
//!    있는 것들뿐이다(글자 크기·여백). 되읽어 대조하는 것이 이 도구의 전부
//!    이므로, 확인할 수 없는 값을 쓰면 장치가 비는 것과 같다.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

/// 풀어 놓은 HWPX — 부품 이름 → 바이트.
pub struct Package {
    /// 원래 순서를 지킨다. `mimetype` 은 쓸 때 맨 앞으로 간다.
    pub(crate) order: Vec<String>,
    pub(crate) parts: BTreeMap<String, Vec<u8>>,
}

impl Package {
    pub fn open(path: &Path) -> Result<Self> {
        let f = std::fs::File::open(path)
            .with_context(|| format!("{} 를 열 수 없습니다", path.display()))?;
        let mut zip = zip::ZipArchive::new(f)
            .with_context(|| format!("{} 가 ZIP 이 아닙니다", path.display()))?;
        let mut order = Vec::with_capacity(zip.len());
        let mut parts = BTreeMap::new();
        for i in 0..zip.len() {
            let mut e = zip.by_index(i)?;
            let name = e.name()?.to_string();
            if name.ends_with('/') {
                continue;
            }
            let mut buf = Vec::new();
            e.read_to_end(&mut buf)?;
            order.push(name.clone());
            parts.insert(name, buf);
        }
        Ok(Self { order, parts })
    }

    /// 글자로 읽을 부품. XML 이 아니면 `None`.
    pub fn text(&self, name: &str) -> Option<String> {
        self.parts
            .get(name)
            .and_then(|b| String::from_utf8(b.clone()).ok())
    }

    pub fn set_text(&mut self, name: &str, body: String) {
        self.parts.insert(name.to_string(), body.into_bytes());
    }

    /// 이름이 이 접두사로 시작하는 부품들, 이름 순서대로.
    pub fn names_starting(&self, prefix: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .parts
            .keys()
            .filter(|n| n.starts_with(prefix))
            .cloned()
            .collect();
        v.sort();
        v
    }

    /// 다시 묶는다. **`mimetype` 을 맨 앞에 무압축으로** 둔다.
    pub fn write(&self, dest: &Path) -> Result<()> {
        let f = std::fs::File::create(dest)
            .with_context(|| format!("{} 를 만들 수 없습니다", dest.display()))?;
        let mut zip = zip::ZipWriter::new(f);

        // OPC 규칙: 첫 엔트리가 `mimetype`, 압축하지 않는다. 어기면 한글이
        // 못 연다 — 우리가 지켜야 하는 쪽이다.
        if let Some(mime) = self.parts.get("mimetype") {
            zip.start_file(
                "mimetype",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )?;
            zip.write_all(mime)?;
        } else {
            bail!("HWPX 에 mimetype 부품이 없습니다 — 다시 묶으면 한글이 못 엽니다");
        }

        for name in &self.order {
            if name == "mimetype" {
                continue;
            }
            let Some(body) = self.parts.get(name) else { continue };
            zip.start_file(name.as_str(), zip::write::SimpleFileOptions::default())?;
            zip.write_all(body)?;
        }
        zip.finish()?;
        Ok(())
    }
}

// ---------------------------------------------------------------- XML 고치기

/// 엘리먼트 하나의 속성 값을 바꾼다. 없는 속성은 **더하지 않는다** —
/// 모르는 자리에 넣으면 스키마를 어긴다.
///
/// 여는 태그 하나만 본다(`<hp:margin … />`). 같은 태그가 여럿이면 `all` 로.
fn set_attr(xml: &str, tag: &str, key: &str, value: &str, all: bool) -> (String, usize) {
    let mut out = String::with_capacity(xml.len() + 16);
    let mut rest = xml;
    let mut hits = 0usize;
    while let Some(at) = rest.find(tag) {
        let Some(close) = rest[at..].find('>').map(|i| i + at) else { break };
        out.push_str(&rest[..at]);
        let seg = &rest[at..close];
        let needle = format!("{key}=\"");
        match seg.find(&needle) {
            Some(s) => {
                let s = s + needle.len();
                match seg[s..].find('"') {
                    Some(e) => {
                        out.push_str(&seg[..s]);
                        out.push_str(value);
                        out.push_str(&seg[s + e..]);
                        hits += 1;
                    }
                    None => out.push_str(seg),
                }
            }
            None => out.push_str(seg),
        }
        rest = &rest[close..];
        if !all {
            break;
        }
    }
    out.push_str(rest);
    (out, hits)
}

/// 여백을 HWPUNIT 으로 설정한다. 구역마다 `hp:margin` 이 하나씩 있다.
pub fn set_margins(
    pkg: &mut Package,
    left: Option<u32>,
    right: Option<u32>,
    top: Option<u32>,
    bottom: Option<u32>,
) -> Result<usize> {
    let mut changed = 0usize;
    for name in pkg.names_starting("Contents/section") {
        let Some(mut xml) = pkg.text(&name) else { continue };
        for (key, v) in [
            ("left", left),
            ("right", right),
            ("top", top),
            ("bottom", bottom),
        ] {
            let Some(v) = v else { continue };
            let (next, hits) = set_attr(&xml, "<hp:margin", key, &v.to_string(), true);
            xml = next;
            changed += hits;
        }
        pkg.set_text(&name, xml);
    }
    Ok(changed)
}

/// 이 글자들을 담은 문단이 가리키는 `charPrIDRef` 를 모은다.
///
/// **"id 0 이 본문" 을 코드에 박지 않는다.** 실측으로는 그렇지만(명세를 바꿔도
/// `charPr 0` 이 본문이었다) 그건 백엔드 생성물의 성질이고 판이 바뀌면 조용히
/// 틀린다. 글자로 찾으면 그 가정이 필요 없다.
pub fn char_pr_ids_for(pkg: &Package, texts: &[String]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for name in pkg.names_starting("Contents/section") {
        let Some(xml) = pkg.text(&name) else { continue };
        let mut rest = xml.as_str();
        while let Some(at) = rest.find("<hp:p ") {
            let Some(end) = rest[at..].find("</hp:p>").map(|i| i + at) else { break };
            let para = &rest[at..end];
            // 이 문단의 글자
            let mut body = String::new();
            let mut seg = para;
            while let Some(s) = seg.find("<hp:t>") {
                let Some(e) = seg[s..].find("</hp:t>").map(|i| i + s) else { break };
                body.push_str(&seg[s + 6..e]);
                seg = &seg[e + 7..];
            }
            if texts.iter().any(|t| t.trim() == body.trim() && !body.trim().is_empty()) {
                if let Some(s) = para.find("charPrIDRef=\"") {
                    let s = s + "charPrIDRef=\"".len();
                    if let Some(e) = para[s..].find('"') {
                        let id = para[s..s + e].to_string();
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                }
            }
            rest = &rest[end + 7..];
        }
    }
    ids
}

/// 그 `charPr` 들의 글자 크기를 설정한다. 단위는 **1/100 pt**.
pub fn set_char_height(pkg: &mut Package, ids: &[String], height_100pt: u32) -> Result<usize> {
    let name = "Contents/header.xml";
    let Some(mut xml) = pkg.text(name) else {
        bail!("HWPX 에 {name} 이 없습니다");
    };
    let mut changed = 0usize;
    for id in ids {
        let tag = format!("<hh:charPr id=\"{id}\"");
        let (next, hits) = set_attr(&xml, &tag, "height", &height_100pt.to_string(), false);
        xml = next;
        changed += hits;
    }
    pkg.set_text(name, xml);
    Ok(changed)
}

/// **본문 단락의 정렬을 바꾼다 — 그 단락만.**
///
/// `paraPr` 은 **심하게 공유된다.** 실측: s03.hwpx 는 문단 317개가 55종을 쓰고
/// 그중 한 종을 **79 문단**이 쓴다. 그래서 제자리에서 고치면 79개가 같이
/// 바뀐다 — 한 단락을 고치려면 **`paraPr` 을 새로 만들어** 그 단락만 가리키게
/// 해야 한다.
///
/// 하는 일: (1) 그 단락의 `paraPrIDRef` 를 읽고 (2) 그 `paraPr` 을 베껴
/// `hh:align/@horizontal` 만 바꾸고 (3) 새 번호로 `hh:paraProperties` 에 넣고
/// `itemCnt` 를 올리고 (4) 그 단락이 새 번호를 가리키게 한다.
///
/// 단락 번호는 **본문 단락 순번**이다 — 표 셀 안 단락(`hp:subList` 안)은 세지
/// 않는다. 증인의 `index` 와 같은 기준이어야 좌표가 통한다.
/// **`hh:margin` 의 들여쓰기를 고친다 — `hp:switch` 두 가지에 각각.**
///
/// 실측(2026-11-03, HWPX 8개 문서의 `paraPr` 615개): 0 이 아닌 544건 중
/// **540건에서 `hp:default` = `hp:case` × 2** 가 정확히 성립한다. 어긋난 4건은
/// 반올림이다(default -361 → case -181, 1201 → 600, 1 → 0). 즉 `hp:default`
/// 가 저장값이고 `hp:case`(HwpUnitChar)는 그 절반이다.
///
/// **두 가지 다 `unit="HWPUNIT"` 이라고 적혀 있다.** 값이 2배 차이인데
/// 단위 속성은 같은 말을 한다 — 속성을 믿으면 한쪽이 2배로 틀린다. 그리고
/// **2배로 틀린 들여쓰기는 "좀 이상한 문서" 로 보여 조용히 통과한다.**
///
/// 증인은 `hp:case` 쪽을 HWPUNIT 으로 읽고, 그 값이 조판기와 맞는 것은 이미
/// 시험이 본다(`witness_first_indent_agrees_with_the_typesetter`). 그래서
/// 여기 받는 `left`·`intent` 도 그 단위이고, `hp:default` 에는 2배로 적는다.
/// 우리가 쓸 때는 `V` 와 `2V` 를 함께 적으므로 반올림 오차가 없다.
///
/// ## `hp:default` 는 되읽어 확인할 수 없다 — 그래도 쓴다
///
/// 실측(2026-11-03): `hp:default` 에 일부러 1배로 적어 보고 `.hwp` 로 변환해
/// 양쪽을 읽어 봤는데 **결과가 같았다**. 즉 우리 증인도, 백엔드 `rhwp` 도
/// `hp:case` 만 읽는다. 이 묶음을 검증할 도구가 우리에게 없다.
///
/// 이 저장소의 규칙은 "읽지 못하는 것은 쓰지 않는다" 인데 여기서는 **안 쓰는
/// 것이 더 나쁘다.** 베껴 온 `paraPr` 의 그 묶음에는 **옛 값이 남아 있고**,
/// 그러면 두 벌이 서로 다른 들여쓰기를 말한다 — `hp:default` 를 읽는 쪽(옛
/// 한글, 다른 도구)이 옛 값을 본다.
///
/// 그래서 쓰고, 보호는 되읽기 대신 **생성된 XML 을 보는 단위 시험**에 맡긴다
/// (`들여쓰기는_두_묶음에_1배와_2배로_적힌다`). `hp:pic` 의 크기 일곱 자리와
/// 같은 꼴이다 — 두 벌이 어긋나는 것은 문서의 사고가 아니라 코드 결함이므로,
/// 막는 자리도 문서가 아니라 코드다.
fn set_margin_indent(
    para_pr: &str,
    left: i64,
    intent: i64,
    // 문단 위·아래 간격 (HWPUNIT). `None` 이면 건드리지 않는다.
    space: Option<(i64, i64)>,
) -> Result<String> {
    let mut out = String::with_capacity(para_pr.len() + 32);
    let mut rest = para_pr;
    let mut branches = 0usize;
    // `hp:case` / `hp:default` 묶음마다 배수를 달리 적용한다. 바이트로 찾는다 —
    // 태그 이름은 ASCII 이고, 사이에 한글이 있어도 자르는 자리는 안전하다.
    while let Some(at) = rest.find("<hp:case").or_else(|| rest.find("<hp:default")) {
        let is_case = rest[at..].starts_with("<hp:case");
        let close = if is_case { "</hp:case>" } else { "</hp:default>" };
        let Some(end) = rest[at..].find(close).map(|k| at + k + close.len()) else {
            break;
        };
        let body = &rest[at..end];
        // **HwpUnitChar 가 아닌 `hp:case` 는 재 보지 않았다.** 조용히 1배로
        // 쓰면 그 묶음이 2배로 틀린다 — 멈추고 말한다.
        let scale = if is_case {
            let head_end = body.find('>').unwrap_or(0);
            if !body[..head_end].contains("HwpUnitChar") {
                bail!(
                    "모르는 `hp:case` 묶음입니다 — `HwpUnitChar` 가 아닙니다. 단위를 재지 않았으므로 쓰지 않습니다: {}",
                    &body[..head_end.min(160)]
                );
            }
            1
        } else {
            2
        };
        out.push_str(&rest[..at]);
        let (b1, h1) = set_attr(body, "<hc:left", "value", &(left * scale).to_string(), false);
        let (mut b2, h2) = set_attr(&b1, "<hc:intent", "value", &(intent * scale).to_string(), false);
        if h1 == 0 || h2 == 0 {
            bail!("`hh:margin` 에 `hc:left`·`hc:intent` 가 없습니다 (left {h1}건, intent {h2}건)");
        }
        // 간격도 **같은 배수 규칙**을 탄다 — 실측에서 `prev` 도
        // `default = case × 2` 였다(130건).
        if let Some((before, after)) = space {
            let (n1, p1) = set_attr(&b2, "<hc:prev", "value", &(before * scale).to_string(), false);
            let (n2, p2) = set_attr(&n1, "<hc:next", "value", &(after * scale).to_string(), false);
            if p1 == 0 || p2 == 0 {
                bail!(
                    "`hh:margin` 에 `hc:prev`·`hc:next` 가 없습니다 (prev {p1}건, next {p2}건)"
                );
            }
            b2 = n2;
        }
        out.push_str(&b2);
        rest = &rest[end..];
        branches += 1;
    }
    out.push_str(rest);
    // **두 벌이 다 있어야 한다.** 한쪽만 고치면 다른 쪽이 옛 값으로 남고,
    // 한글이 어느 쪽을 읽느냐에 따라 결과가 갈린다.
    if branches < 2 {
        bail!(
            "`hp:switch` 의 묶음을 {branches}개만 찾았습니다 — `hp:case`(HwpUnitChar)와 `hp:default` 두 벌이 다 있어야 합니다. 한쪽만 고치면 한글이 읽는 쪽에 따라 들여쓰기가 2배로 틀립니다."
        );
    }
    Ok(out)
}

/// 베낀 `paraPr` 의 `hp:case` 쪽 `left`·`intent` 를 읽는다.
///
/// 간격만 바꾸려는 요청에서 들여쓰기를 0 으로 덮지 않으려고 있다 — 말하지
/// 않은 것을 바꾸는 것이 이 저장소가 가장 경계하는 꼴이다.
fn read_margin(para_pr: &str) -> (i64, i64) {
    let Some(at) = para_pr.find("<hp:case") else { return (0, 0) };
    let Some(end) = para_pr[at..].find("</hp:case>").map(|k| at + k) else { return (0, 0) };
    let seg = &para_pr[at..end];
    let num = |tag: &str| -> i64 {
        seg.find(tag)
            .and_then(|k| {
                let k = k + tag.len();
                seg[k..].find("value=\"").map(|v| k + v + "value=\"".len())
            })
            .and_then(|k| seg[k..].find('"').and_then(|e| seg[k..k + e].parse().ok()))
            .unwrap_or(0)
    };
    (num("<hc:left"), num("<hc:intent"))
}

/// 문단 서식을 고친다 — 정렬과 들여쓰기. **`paraPr` 을 한 번만 베낀다.**
///
/// 정렬과 들여쓰기를 따로 부르면 `paraPr` 이 두 벌 복제되고, 그래도 결과는
/// 맞지만 서식 표가 쓸데없이 불어난다. 한 연산에서 같이 받는다.
pub fn set_para_style(
    pkg: &mut Package,
    section: u32,
    para: u32,
    horizontal: Option<&str>,
    // `(first_indent, rest_indent)` — HWPUNIT, 증인이 읽는 그 단위.
    indent: Option<(i64, i64)>,
    // `(위 간격, 아래 간격)` — HWPUNIT. 1 pt = 100.
    space: Option<(i64, i64)>,
    // 자동 개요 번호를 켤지 끌지. `None` 이면 건드리지 않는다.
    numbered: Option<bool>,
) -> Result<()> {
    let sec_name = format!("Contents/section{section}.xml");
    let Some(sec) = pkg.text(&sec_name) else {
        bail!("구역 {section} 이 없습니다 ({sec_name})");
    };

    // ---- 1) 그 단락을 찾는다 (subList 밖의 `hp:p` 만 센다)
    // **바이트로 훑는다.** 글자열을 아무 자리에서 자르면 한글 한가운데에서
    // 터진다(실제로 터졌다: "start byte index 5815 is not a char boundary").
    // `<` 와 `>` 는 ASCII 라 그 자리에서 자르는 것은 안전하다.
    let mut depth = 0i32;
    let mut seen = 0u32;
    let mut target: Option<(usize, usize)> = None; // 여는 태그의 (시작, 끝)
    let b = sec.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'<' {
            if b[i..].starts_with(b"<hp:subList") {
                depth += 1;
            } else if b[i..].starts_with(b"</hp:subList") {
                depth -= 1;
            } else if b[i..].starts_with(b"<hp:p ") || b[i..].starts_with(b"<hp:p>") {
                if depth == 0 {
                    if seen == para {
                        let end = b[i..]
                            .iter()
                            .position(|c| *c == b'>')
                            .map(|k| i + k)
                            .unwrap_or(i);
                        target = Some((i, end));
                        break;
                    }
                    seen += 1;
                }
            }
        }
        i += 1;
    }
    let Some((tag_start, tag_end)) = target else {
        bail!("구역 {section} 에 본문 단락 {para} 이 없습니다 (본문 단락 {seen}개)");
    };

    // ---- 2) 그 단락이 가리키는 paraPr
    let tag = &sec[tag_start..tag_end];
    let key = "paraPrIDRef=\"";
    let Some(s) = tag.find(key) else {
        bail!("구역 {section} 단락 {para} 에 `paraPrIDRef` 가 없습니다");
    };
    let s = s + key.len();
    let Some(e) = tag[s..].find('"') else {
        bail!("구역 {section} 단락 {para} 의 `paraPrIDRef` 를 읽을 수 없습니다");
    };
    let old_id = tag[s..s + e].to_string();

    // ---- 3) 그 paraPr 을 베껴 정렬만 바꾼 새 것을 만든다
    let hdr_name = "Contents/header.xml";
    let Some(mut hdr) = pkg.text(hdr_name) else {
        bail!("{hdr_name} 이 없습니다");
    };
    let open = format!("<hh:paraPr id=\"{old_id}\"");
    let Some(ps) = hdr.find(&open) else {
        bail!("`paraPr` {old_id} 를 {hdr_name} 에서 찾지 못했습니다");
    };
    let Some(pe) = hdr[ps..].find("</hh:paraPr>").map(|k| ps + k + "</hh:paraPr>".len()) else {
        bail!("`paraPr` {old_id} 가 닫히지 않았습니다");
    };
    let base = hdr[ps..pe].to_string();

    // 새 번호는 **지금 쓰이는 번호 중 가장 큰 것 + 1**
    let mut max_id = 0u32;
    let mut rest = hdr.as_str();
    while let Some(at) = rest.find("<hh:paraPr id=\"") {
        let s2 = at + "<hh:paraPr id=\"".len();
        if let Some(e2) = rest[s2..].find('"') {
            if let Ok(v) = rest[s2..s2 + e2].parse::<u32>() {
                max_id = max_id.max(v);
            }
        }
        rest = &rest[s2..];
    }
    let new_id = max_id + 1;

    let mut fresh = base.clone();
    if let Some(h) = horizontal {
        let (next, hits) = set_attr(&fresh, "<hh:align", "horizontal", h, false);
        if hits == 0 {
            bail!("`paraPr` {old_id} 에 `hh:align/@horizontal` 이 없습니다");
        }
        fresh = next;
    }
    // 들여쓰기와 간격은 같은 `hh:margin` 안에 있으므로 한 번에 적는다 —
    // 두 번 훑으면 두 번째가 첫 번째의 결과 위에서 자리를 다시 찾아야 한다.
    if indent.is_some() || space.is_some() {
        let (first, rest_i) = indent.unwrap_or((0, 0));
        // 증인의 공식을 되돌린다: `first = left + max(intent,0)`,
        // `rest = left + max(-intent,0)` 이므로 `left = min(first, rest)` 이고
        // `intent = first - rest` 다. (s03 paraPr 69 로 확인: left 1330 ·
        // intent -2040 → first 1330 · rest 3370.)
        if first < 0 || rest_i < 0 {
            bail!("들여쓰기는 음수일 수 없습니다 (first {first}, rest {rest_i}) — 내어쓰기는 `firstIndent < restIndent` 로 적습니다");
        }
        // 들여쓰기를 안 줬으면 **지금 값을 그대로** 둔다 — 0 으로 덮으면
        // 간격만 바꾸려다 들여쓰기를 날린다.
        let (l, i) = match indent {
            Some(_) => (first.min(rest_i), first - rest_i),
            None => {
                let cur = read_margin(&fresh);
                (cur.0, cur.1)
            }
        };
        fresh = set_margin_indent(&fresh, l, i, space)?;
    }
    if let Some(on) = numbered {
        // **자동 번호는 `hh:heading/@type` 하나다.** `OUTLINE` 이면 조판기가
        // 제목 앞에 번호를 붙이고, `NONE` 이면 안 붙인다.
        //
        // 실측(2026-11-03): `NONE` 으로 바꾸면 번호만 사라지고 제목 서식
        // (굵기·크기·간격)은 그대로다. `convert --verify` 도 IR 차이 없음.
        //
        // `level`·`idRef` 는 건드리지 않는다 — 다시 켤 때 쓰는 값이고,
        // 우리가 지어낼 값이 아니다.
        let (next, hits) = set_attr(
            &fresh,
            "<hh:heading",
            "type",
            if on { "OUTLINE" } else { "NONE" },
            false,
        );
        if hits == 0 {
            bail!("`paraPr` {old_id} 에 `hh:heading/@type` 이 없습니다");
        }
        fresh = next;
    }
    let (f2, idh) = set_attr(&fresh, "<hh:paraPr", "id", &new_id.to_string(), false);
    if idh == 0 {
        bail!("새 `paraPr` 의 번호를 적지 못했습니다");
    }
    fresh = f2;

    // 넣고 `itemCnt` 를 올린다 — 수가 안 맞으면 한글이 거부할 수 있다
    hdr.insert_str(pe, &fresh);
    let cnt_key = "<hh:paraProperties itemCnt=\"";
    let Some(cs) = hdr.find(cnt_key) else {
        bail!("`hh:paraProperties` 를 찾지 못했습니다");
    };
    let cs = cs + cnt_key.len();
    let Some(ce) = hdr[cs..].find('"').map(|k| cs + k) else {
        bail!("`itemCnt` 를 읽을 수 없습니다");
    };
    let cnt: u32 = hdr[cs..ce].parse().unwrap_or(0);
    hdr.replace_range(cs..ce, &(cnt + 1).to_string());
    pkg.set_text(hdr_name, hdr);

    // ---- 4) 그 단락이 새 번호를 가리키게
    let mut sec2 = sec.clone();
    let abs = tag_start + s;
    sec2.replace_range(abs..abs + e, &new_id.to_string());
    pkg.set_text(&sec_name, sec2);
    Ok(())
}

/// **한 본문 단락의 글자 크기를 바꾼다 — 그 단락만.**
///
/// 사용자 보고(hwp-vi#5)의 `set-char` 중 크기. 글꼴 이름은 **쓰지 않는다** —
/// 증인이 구간의 글꼴 이름을 읽지 않으므로 되읽어 대조할 수 없고, 이 저장소는
/// 확인할 수 없는 것을 쓰지 않는다. 크기는 증인이 구간마다 읽는다(`sizePt`).
///
/// `charPr` 도 `paraPr` 처럼 **심하게 공유된다** — 실측(c01.hwpx): `charPr 0`
/// 하나를 구간 24곳이 쓴다. 그래서 제자리에서 고치면 문서 전체의 글자 크기가
/// 바뀐다. `paraPr` 과 같은 복제 장치를 쓴다: 그 단락이 쓰는 `charPr` 마다
/// 베껴 `height` 만 바꾸고, 새 번호로 넣고, **그 단락의 구간들만** 새 번호를
/// 가리키게 한다.
///
/// **문단 자기 수준의 구간만** 센다. 표 칸 안(`hp:subList` → `hp:p` →
/// `hp:run`)까지 바꾸면 말하지 않은 것을 바꾼다 — 그림을 넣을 때 같은 자리에서
/// 한 번 당했다.
/// **그 글꼴을 `hh:fontfaces` 의 언어마다 확보한다** — 있으면 그 번호,
/// 없으면 더하고 그 번호.
///
/// 언어마다 번호가 따로다(실측 s03: HANGUL 23종, 언어 7개). 같은 이름이라도
/// 번호가 다르므로 언어마다 찾아야 한다.
///
/// 일곱 언어를 다 맞추는 이유: `font: "D2Coding"` 은 "이 구간은 이 글꼴" 이라는
/// 말이다. 한글만 바꾸면 코드(라틴)가 안 바뀌고, 라틴만 바꾸면 한글 주석이
/// 안 바뀐다 — 실무 문서 구간 752개 중 **233개(31%)가 한글≠라틴** 이므로
/// 한쪽만 건드리면 반쪽만 바뀐 상태가 흔하게 남는다.
fn ensure_fonts(pkg: &mut Package, face: &str) -> Result<BTreeMap<String, String>> {
    const LANGS: &[&str] = &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"];
    let name = "Contents/header.xml";
    let Some(mut hdr) = pkg.text(name) else {
        bail!("{name} 이 없습니다");
    };
    let mut ids: BTreeMap<String, String> = BTreeMap::new();

    for lang in LANGS {
        // 그 언어 묶음의 범위를 잡는다
        let open_pat = format!("<hh:fontface lang=\"{lang}\"");
        let Some(fs) = hdr.find(&open_pat) else {
            // 그 언어 묶음이 없는 문서도 있다 — 지어내지 않고 건너뛴다.
            continue;
        };
        let Some(fe) = hdr[fs..].find("</hh:fontface>").map(|k| fs + k) else {
            bail!("`hh:fontface lang=\"{lang}\"` 가 닫히지 않았습니다");
        };
        let block = &hdr[fs..fe];
        // 이미 있나 — `face="이름"` 으로 찾는다
        let want = format!("face=\"{face}\"");
        if let Some(at) = block.find(&want) {
            // 그 `hh:font` 의 id 를 뒤로 찾아 읽는다
            let head = &block[..at];
            if let Some(ts) = head.rfind("<hh:font id=\"") {
                let ts = ts + "<hh:font id=\"".len();
                if let Some(te) = head[ts..].find('"') {
                    ids.insert(lang.to_string(), head[ts..ts + te].to_string());
                    continue;
                }
            }
        }
        // 없으면 더한다. 번호는 그 묶음에서 가장 큰 것 + 1.
        let mut max_id = -1i64;
        let mut rest = block;
        while let Some(at) = rest.find("<hh:font id=\"") {
            let s2 = at + "<hh:font id=\"".len();
            if let Some(e2) = rest[s2..].find('"') {
                if let Ok(v) = rest[s2..s2 + e2].parse::<i64>() {
                    max_id = max_id.max(v);
                }
            }
            rest = &rest[s2..];
        }
        let new_id = (max_id + 1).to_string();
        // `typeInfo` 는 넣지 않는다 — 실물에는 있지만 **우리가 모르는 값**이다
        // (familyType·weight·proportion …). 모르는 값을 지어내는 대신 빼고,
        // 한글이 받는지는 `convert --verify` 가 본다.
        let item = format!(
            "<hh:font id=\"{new_id}\" face=\"{face}\" type=\"TTF\" isEmbedded=\"0\"/>"
        );
        hdr.insert_str(fe, &item);
        // `fontCnt` 를 올린다 — 수가 안 맞으면 한글이 거부할 수 있다
        let cnt_key = "fontCnt=\"";
        let seg_end = fs + open_pat.len() + 64;
        if let Some(cs) = hdr[fs..seg_end.min(hdr.len())].find(cnt_key).map(|k| fs + k + cnt_key.len()) {
            if let Some(ce) = hdr[cs..].find('"').map(|k| cs + k) {
                let n: u32 = hdr[cs..ce].parse().unwrap_or(0);
                hdr.replace_range(cs..ce, &(n + 1).to_string());
            }
        }
        ids.insert(lang.to_string(), new_id);
    }
    if ids.is_empty() {
        bail!("`hh:fontfaces` 에서 어느 언어 묶음도 찾지 못했습니다 — HWPX 가 아닌 듯합니다");
    }
    pkg.set_text(name, hdr);
    Ok(ids)
}

/// **한 본문 단락의 글자 크기와 글꼴을 바꾼다 — 그 단락만.**
///
/// 둘을 한 함수에서 받는 이유: `charPr` 을 한 번만 베낀다. 따로 부르면 서식
/// 표가 쓸데없이 불어난다.
pub fn set_para_char(
    pkg: &mut Package,
    section: u32,
    para: u32,
    height_100pt: Option<u32>,
    face: Option<&str>,
) -> Result<usize> {
    if height_100pt.is_none() && face.is_none() {
        bail!("바꿀 것이 없습니다 — 크기나 글꼴 중 하나는 주세요");
    }
    if height_100pt == Some(0) {
        bail!("글자 크기가 0 입니다");
    }
    // 글꼴을 먼저 확보한다 — `header.xml` 을 두 번 고치지 않게
    let font_ids = match face {
        Some(f) if !f.trim().is_empty() => Some(ensure_fonts(pkg, f.trim())?),
        Some(_) => bail!("글꼴 이름이 비었습니다"),
        None => None,
    };
    let sec_name = format!("Contents/section{section}.xml");
    let Some(sec) = pkg.text(&sec_name) else {
        bail!("구역 {section} 이 없습니다 ({sec_name})");
    };
    let (p_start, p_end) = find_body_para(&sec, para)?;

    // ---- 1) 그 단락 자기 수준의 `hp:run` 들과 그 charPrIDRef 자리
    //
    // `(절대 시작, 절대 끝)` 을 모은다 — 나중에 뒤에서부터 갈아 끼우려면
    // 자리를 알아야 하고, 앞에서부터 바꾸면 뒤 자리가 밀린다.
    let mut spots: Vec<(usize, usize, String)> = Vec::new();
    {
        let body = &sec[p_start..p_end];
        let b = body.as_bytes();
        let mut depth = 0i32;
        let mut i = 0usize;
        while i < b.len() {
            if b[i] == b'<' {
                if b[i..].starts_with(b"<hp:subList") {
                    depth += 1;
                } else if b[i..].starts_with(b"</hp:subList") {
                    depth -= 1;
                } else if depth == 0 && b[i..].starts_with(b"<hp:run") {
                    let end = b[i..].iter().position(|c| *c == b'>').map(|k| i + k).unwrap_or(i);
                    let tag = &body[i..end];
                    let key = "charPrIDRef=\"";
                    if let Some(s0) = tag.find(key) {
                        let s0 = s0 + key.len();
                        if let Some(e0) = tag[s0..].find('"') {
                            spots.push((
                                p_start + i + s0,
                                p_start + i + s0 + e0,
                                tag[s0..s0 + e0].to_string(),
                            ));
                        }
                    }
                }
            }
            i += 1;
        }
    }
    if spots.is_empty() {
        bail!(
            "구역 {section} 단락 {para} 에 (표 밖의) `hp:run` 이 없습니다 — \
             글자가 없는 단락인 듯합니다"
        );
    }

    // ---- 2) 쓰이는 `charPr` 마다 베껴 `height` 를 바꾼다
    let hdr_name = "Contents/header.xml";
    let Some(mut hdr) = pkg.text(hdr_name) else {
        bail!("{hdr_name} 이 없습니다");
    };
    let mut max_id = 0u32;
    {
        let mut rest = hdr.as_str();
        while let Some(at) = rest.find("<hh:charPr id=\"") {
            let s2 = at + "<hh:charPr id=\"".len();
            if let Some(e2) = rest[s2..].find('"') {
                if let Ok(v) = rest[s2..s2 + e2].parse::<u32>() {
                    max_id = max_id.max(v);
                }
            }
            rest = &rest[s2..];
        }
    }

    let mut mapping: BTreeMap<String, String> = BTreeMap::new();
    let mut added = 0u32;
    let distinct: Vec<String> = {
        let mut v: Vec<String> = spots.iter().map(|(_, _, id)| id.clone()).collect();
        v.sort();
        v.dedup();
        v
    };
    for old_id in distinct {
        let open = format!("<hh:charPr id=\"{old_id}\"");
        let Some(cs) = hdr.find(&open) else {
            bail!("`charPr` {old_id} 를 {hdr_name} 에서 찾지 못했습니다");
        };
        let Some(ce) = hdr[cs..].find("</hh:charPr>").map(|k| cs + k + "</hh:charPr>".len()) else {
            bail!("`charPr` {old_id} 가 닫히지 않았습니다");
        };
        let base = hdr[cs..ce].to_string();
        added += 1;
        let new_id = max_id + added;
        let mut f1 = base.clone();
        if let Some(h) = height_100pt {
            let (next, hits) = set_attr(&f1, "<hh:charPr", "height", &h.to_string(), false);
            if hits == 0 {
                bail!("`charPr` {old_id} 에 `height` 속성이 없습니다");
            }
            f1 = next;
        }
        if let Some(ids) = &font_ids {
            // `hh:fontRef` 는 언어마다 속성이 하나다 — 확보한 번호로 전부 맞춘다
            let mut hit_any = false;
            for (lang, id) in ids {
                let key = lang.to_lowercase();
                let (next, hits) = set_attr(&f1, "<hh:fontRef", &key, id, false);
                f1 = next;
                hit_any |= hits > 0;
            }
            if !hit_any {
                bail!("`charPr` {old_id} 에 `hh:fontRef` 가 없습니다");
            }
        }
        let (f2, h2) = set_attr(&f1, "<hh:charPr", "id", &new_id.to_string(), false);
        if h2 == 0 {
            bail!("새 `charPr` 의 번호를 적지 못했습니다");
        }
        hdr.insert_str(ce, &f2);
        mapping.insert(old_id, new_id.to_string());
    }

    // `itemCnt` 를 올린다 — 수가 안 맞으면 한글이 거부할 수 있다
    let cnt_key = "<hh:charProperties itemCnt=\"";
    let Some(qs) = hdr.find(cnt_key) else {
        bail!("`hh:charProperties` 를 찾지 못했습니다");
    };
    let qs = qs + cnt_key.len();
    let Some(qe) = hdr[qs..].find('"').map(|k| qs + k) else {
        bail!("`itemCnt` 를 읽을 수 없습니다");
    };
    let cnt: u32 = hdr[qs..qe].parse().unwrap_or(0);
    hdr.replace_range(qs..qe, &(cnt + added).to_string());
    pkg.set_text(hdr_name, hdr);

    // ---- 3) 그 단락의 구간들이 새 번호를 가리키게 — **뒤에서부터**
    let mut sec2 = sec.clone();
    for (s0, e0, old_id) in spots.iter().rev() {
        let Some(new_id) = mapping.get(old_id) else { continue };
        sec2.replace_range(*s0..*e0, new_id);
    }
    pkg.set_text(&sec_name, sec2);
    Ok(spots.len())
}

/// 패키지에 부품을 **새로** 넣는다. 다시 묶을 때 순서에 들어간다.
pub fn add_part(pkg: &mut Package, name: &str, body: Vec<u8>) {
    if !pkg.order.iter().any(|n| n == name) {
        pkg.order.push(name.to_string());
    }
    pkg.parts.insert(name.to_string(), body);
}

/// **그림을 본문 단락에 넣는다.** HWPX 만.
///
/// 사용자 보고(hwp-vi#6): 그림을 **읽고 지울 수는 있는데 넣을 수 없었다**.
/// "교재·보고서·논문에서 그림 없는 원고는 드물다 … 그림이 하나라도 있으면
/// 문서를 hwp-vi 로 만들 수 없다".
///
/// 백엔드의 `insert-image` 로는 안 되는 이유가 있다 — 도움말이 말하는 대로
/// **"용지 기준 떠 있는 그림"**(도장·서명용)만 만든다. 실무 문서의 그림은
/// 글자처럼이고 표 칸 안에도 있다(실측: s00.hwp 의 그림 6개 전부 `inline`,
/// 그중 2개는 칸 안). 그래서 상류에 요청(rhwp#8370)해 두고, 우리는 HWPX 를
/// 직접 쓴다.
///
/// 네 곳을 건드린다 — 실물을 뜯어 확인했다:
///
/// 1. `BinData/<이름>` 에 바이트
/// 2. `Contents/content.hpf` 의 `<opf:manifest>` 에 `<opf:item … isEmbeded="1"/>`
///    — 이게 없으면 `binaryItemIDRef` 가 가리킬 데가 없다
/// 3. 대상 문단 안에 `hp:pic`
/// 4. 크기는 `hp:pic` 안 **여섯 곳**(`orgSz`·`curSz`·`imgRect`·`imgClip`·
///    `imgDim`·`sz`)에 같은 수로 — 한 곳만 고치면 어긋난다
///
/// `treatAsChar="1"` 이 글자처럼이다. 증인이 그걸 `inline` 으로 읽으므로
/// **쓴 것을 되읽어 대조할 수 있다**.
#[allow(clippy::too_many_arguments)]
/// **다음에 빌 BinData 부품 이름.** 호출하는 쪽이 미리 세어 연산에 적는다.
///
/// `insert_image` 가 스스로 고르게 하지 않는 이유: 그러면 "무슨 부품이 늘
/// 것인가" 를 저장 전에 말할 수 없고, 자가검증이 **부품 목록의 변화를
/// 통째로 묵인**해야 한다. 표에 `index` 를 미리 세어 적는 것과 같은 이유다.
pub fn next_bin_part(pkg: &Package, ext: &str) -> Result<String> {
    let hpf_name = "Contents/content.hpf";
    let Some(hpf) = pkg.text(hpf_name) else {
        bail!("{hpf_name} 이 없습니다 — HWPX 가 아닌 듯합니다");
    };
    let mut n = 1u32;
    // 등록부의 id 와 부품 이름 **양쪽**이 비어야 쓸 수 있다. 한쪽만 보면
    // 등록부에 없지만 파일은 있는(또는 그 반대) 꼴에서 부딪힌다.
    while hpf.contains(&format!("id=\"image{n}\"")) || pkg.text(&format!("BinData/image{n}.{ext}")).is_some() {
        n += 1;
    }
    Ok(format!("BinData/image{n}.{ext}"))
}

/// 그림 하나를 HWPX 의 한 본문 단락에 넣는다. **`part` 는 부르는 쪽이 정한다**
/// (`next_bin_part`) — 자가검증이 그 이름을 미리 알아야 하기 때문이다.
///
/// 네 군데를 만진다: `BinData/<이름>` 바이트 · `content.hpf` 등록부 ·
/// 그 단락 run 안의 `hp:pic` · 그 안에 **여섯 번 반복되는** 크기.
pub fn insert_image(
    pkg: &mut Package,
    section: u32,
    para: u32,
    part: &str,
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    inline: bool,
    align: crate::images::Align,
) -> Result<String> {
    let hpf_name = "Contents/content.hpf";
    let Some(mut hpf) = pkg.text(hpf_name) else {
        bail!("{hpf_name} 이 없습니다 — HWPX 가 아닌 듯합니다");
    };
    let stem = part
        .rsplit('/')
        .next()
        .and_then(|f| f.rsplit_once('.'))
        .map(|(a, _)| a)
        .unwrap_or_default();
    let ext = part.rsplit_once('.').map(|(_, e)| e).unwrap_or_default();
    if stem.is_empty() || ext.is_empty() {
        bail!("그림 부품 이름이 `BinData/이름.확장자` 꼴이 아닙니다: {part}");
    }
    let id = stem.to_string();
    // **이미 있는 이름이면 멈춘다.** 덮으면 남의 그림이 조용히 사라진다.
    if hpf.contains(&format!("id=\"{id}\"")) || pkg.text(part).is_some() {
        bail!("{part} 는 이미 있습니다 — 덮으면 그 그림이 사라집니다. `next_bin_part` 로 빈 이름을 받으세요.");
    }

    // ---- 1) 바이트
    add_part(pkg, part, bytes);

    // ---- 2) 등록부
    let media = match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "tif" | "tiff" => "image/tiff",
        other => bail!("그림 형식을 모릅니다: {other} (png·jpg·bmp·gif·tif)"),
    };
    let item = format!(
        "<opf:item id=\"{id}\" href=\"{part}\" media-type=\"{media}\" isEmbeded=\"1\"/>"
    );
    let Some(at) = hpf.find("</opf:manifest>") else {
        bail!("{hpf_name} 에 `</opf:manifest>` 가 없습니다");
    };
    hpf.insert_str(at, &item);
    pkg.set_text(hpf_name, hpf);

    // ---- 3·4) 문단 안에 `hp:pic`
    let sec_name = format!("Contents/section{section}.xml");
    let Some(sec) = pkg.text(&sec_name) else {
        bail!("구역 {section} 이 없습니다 ({sec_name})");
    };
    // 그 문단의 **첫 `hp:run` 이 닫히는 자리**에 넣는다. `hp:pic` 은 run 의
    // 자식이어야 한다 — 문단 바로 아래에 넣으면 스키마를 어긴다.
    let (p_start, p_end) = find_body_para(&sec, para)?;
    // **문단 자기 수준의** 첫 `run` 이 닫히는 자리다.
    //
    // 그냥 첫 `</hp:run>` 을 찾으면 **표 칸 안의 run** 을 집는다 — 문단이 표를
    // 들고 있으면 그 안에 `hp:subList` → `hp:p` → `hp:run` 이 있다. 실측으로
    // 그렇게 들어가서, 증인이 "표 칸 안의 그림" 으로 읽었다.
    let run_end_rel = {
        let pb = sec[p_start..p_end].as_bytes();
        let mut depth = 0i32;
        let mut found = None;
        let mut i = 0usize;
        while i < pb.len() {
            if pb[i] == b'<' {
                if pb[i..].starts_with(b"<hp:subList") {
                    depth += 1;
                } else if pb[i..].starts_with(b"</hp:subList") {
                    depth -= 1;
                } else if pb[i..].starts_with(b"</hp:run>") && depth == 0 {
                    found = Some(i);
                    break;
                }
            }
            i += 1;
        }
        match found {
            Some(k) => k,
            None => bail!(
                "구역 {section} 단락 {para} 에 (표 밖의) `hp:run` 이 없습니다 — \
                 글자가 없는 단락인 듯합니다"
            ),
        }
    };
    let pic = pic_xml(&id, width, height, inline, align);
    let mut sec2 = sec.clone();
    sec2.insert_str(p_start + run_end_rel, &pic);
    pkg.set_text(&sec_name, sec2);
    // **그 줄을 그림이 들어갈 만큼 키운다.** 안 하면 그림이 아래를 덮는다.
    if inline {
        grow_line_for(pkg, section, para, height)?;
    }
    Ok(part.to_string())
}

/// **글자처럼 취급하는 그림이 들어간 줄의 저장 높이를 그림에 맞춘다.**
///
/// 안 하면 **그림이 아래 내용을 덮는다.** 눈으로 보고 찾았다 — 11pt 본문에
/// 127pt 그림을 넣었더니 캡션과 표가 그림 뒤에 깔렸고, 저장된 줄은 여전히
/// `vertsize="1000"`(10pt)이었다. 조판기는 **저장된 줄을 재현**하므로 10pt
/// 자리에 127pt 그림을 그린다.
///
/// **`layout-anomaly` 는 이것을 못 본다.** 겹침 0, 글자겹침 0 을 보고했다 —
/// 그림은 글자 런이 아니라서 그 검사의 대상이 아니다. 기계 검사가 통과한
/// 자리를 눈이 잡은 경우다.
///
/// 값은 실측으로 정했다. 실무 문서에서 그림이 든 단락 8건 **전부**:
///
/// | 문서 | 그림 높이 | `vertsize` | `textheight` | `baseline` |
/// |---|---|---|---|---|
/// | s03 | 1885 | 1885 | 1885 | 1602 (0.85) |
/// | p   | 3291 | 3291 | 3291 | 2797 (0.85) |
///
/// 즉 `vertsize = textheight = 그림 높이`, `baseline = 높이 × 0.85`. 글자만
/// 있는 줄도 같은 0.85 비율이었다(2000 → 1700).
///
/// `spacing`·`horzsize` 는 건드리지 않는다 — 실측에서 그림 높이와 관계가
/// 보이지 않았고(같은 높이에 240 과 984 가 둘 다 있다), 재지 않은 값을
/// 지어내지 않는다.
fn grow_line_for(pkg: &mut Package, section: u32, para: u32, height: u32) -> Result<()> {
    let name = format!("Contents/section{section}.xml");
    let Some(xml) = pkg.text(&name) else {
        bail!("구역 {section} 이 없습니다 ({name})");
    };
    let (p_start, p_end) = find_body_para(&xml, para)?;
    let body = &xml[p_start..p_end];
    // 그 단락 **자기 수준**의 `hp:lineseg` 만 — 표 칸 안의 줄은 남의 것이다
    let mut depth = 0i32;
    let mut spots: Vec<(usize, usize)> = Vec::new();
    let b = body.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'<' {
            if b[i..].starts_with(b"<hp:subList") {
                depth += 1;
            } else if b[i..].starts_with(b"</hp:subList") {
                depth -= 1;
            } else if depth == 0 && b[i..].starts_with(b"<hp:lineseg") {
                if let Some(e) = b[i..].iter().position(|c| *c == b'>') {
                    spots.push((i, i + e + 1));
                }
            }
        }
        i += 1;
    }
    if spots.is_empty() {
        // 저장된 줄이 없는 문서도 있다(조판 부기를 안 들고 있는 생성물).
        // 그때는 조판기가 스스로 계산하므로 할 일이 없다.
        return Ok(());
    }
    let baseline = ((height as f64) * 0.85).round() as u32;
    let mut out = String::with_capacity(xml.len() + 32);
    out.push_str(&xml[..p_start]);
    let mut last = 0usize;
    for (s0, e0) in &spots {
        out.push_str(&body[last..*s0]);
        let tag = &body[*s0..*e0];
        // **이미 더 큰 줄은 줄이지 않는다.** 한 줄에 그림이 둘이면 큰 쪽이
        // 줄 높이를 정한다 — 뒤에 넣은 작은 그림이 앞의 큰 그림을 깎으면
        // 그 그림이 다시 아래를 덮는다.
        let now = tag
            .find("vertsize=\"")
            .and_then(|k| {
                let k = k + "vertsize=\"".len();
                tag[k..].find('"').and_then(|e| tag[k..k + e].parse::<u32>().ok())
            })
            .unwrap_or(0);
        if now >= height {
            out.push_str(tag);
        } else {
            let (t1, _) = set_attr(tag, "<hp:lineseg", "vertsize", &height.to_string(), false);
            let (t2, _) = set_attr(&t1, "<hp:lineseg", "textheight", &height.to_string(), false);
            let (t3, _) = set_attr(&t2, "<hp:lineseg", "baseline", &baseline.to_string(), false);
            out.push_str(&t3);
        }
        last = *e0;
    }
    out.push_str(&body[last..]);
    out.push_str(&xml[p_end..]);
    pkg.set_text(&name, out);
    Ok(())
}

/// 본문 단락 하나의 XML 범위 (`<hp:p …>` … `</hp:p>`).
///
/// 표 셀 안 단락(`hp:subList` 안)은 세지 않는다 — 증인의 `index` 와 같은
/// 기준이어야 좌표가 통한다. 바이트로 훑는다(글자열을 아무 자리에서 자르면
/// 한글 한가운데에서 터진다).
fn find_body_para(sec: &str, para: u32) -> Result<(usize, usize)> {
    let b = sec.as_bytes();
    let mut depth = 0i32;
    let mut seen = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'<' {
            if b[i..].starts_with(b"<hp:subList") {
                depth += 1;
            } else if b[i..].starts_with(b"</hp:subList") {
                depth -= 1;
            } else if (b[i..].starts_with(b"<hp:p ") || b[i..].starts_with(b"<hp:p>"))
                && depth == 0
            {
                if seen == para {
                    let end = sec[i..]
                        .find("</hp:p>")
                        .map(|k| i + k)
                        .unwrap_or(sec.len());
                    return Ok((i, end));
                }
                seen += 1;
            }
        }
        i += 1;
    }
    bail!("본문 단락 {para} 이 없습니다 (본문 단락 {seen}개)")
}

/// `hp:pic` 한 벌. 크기를 **여섯 곳**에 같은 수로 적는다.
fn pic_xml(id: &str, w: u32, h: u32, inline: bool, align: crate::images::Align) -> String {
    let as_char = if inline { "1" } else { "0" };
    // **개체 정렬.** 실측(실무 문서 14개, 그림 43개): 전부 `LEFT` 다 — 가운데
    // 맞춘 그림은 **문단**이 가운데다(center 3 · right 2). 그래서 글자처럼
    // 취급하는 그림의 정렬은 부르는 쪽에서 `set-para-align` 으로 가고, 이
    // 속성은 떠 있는 그림에만 쓴다.
    let horz = match align {
        crate::images::Align::Center => "CENTER",
        crate::images::Align::Right => "RIGHT",
        crate::images::Align::Left => "LEFT",
    };
    // 글자처럼이면 글자 흐름을 따르므로 기준은 단락(`PARA`)이고, 떠 있는
    // 그림이면 용지(`PAPER`)다 — 실물에서 본 조합이다.
    let rel = if inline { "PARA" } else { "PAPER" };
    format!(
        concat!(
            r#"<hp:pic reverse="0" id="0" zOrder="0" numberingType="PICTURE" textWrap="SQUARE""#,
            r#" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="0">"#,
            r#"<hp:offset x="0" y="0"/>"#,
            r#"<hp:orgSz width="{w}" height="{h}"/>"#,
            r#"<hp:curSz width="{w}" height="{h}"/>"#,
            r#"<hp:flip horizontal="0" vertical="0"/>"#,
            r#"<hp:rotationInfo angle="0" centerX="0" centerY="0" rotateimage="0"/>"#,
            r#"<hp:renderingInfo>"#,
            r#"<hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>"#,
            r#"<hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>"#,
            r#"<hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>"#,
            r#"</hp:renderingInfo>"#,
            r#"<hp:imgRect><hc:pt0 x="0" y="0"/><hc:pt1 x="{w}" y="0"/>"#,
            r#"<hc:pt2 x="{w}" y="{h}"/><hc:pt3 x="0" y="{h}"/></hp:imgRect>"#,
            r#"<hp:imgClip left="0" right="{w}" top="0" bottom="{h}"/>"#,
            r#"<hp:inMargin left="0" right="0" top="0" bottom="0"/>"#,
            r#"<hp:imgDim dimwidth="{w}" dimheight="{h}"/>"#,
            r#"<hc:img binaryItemIDRef="{id}" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/>"#,
            r#"<hp:effects></hp:effects>"#,
            r#"<hp:sz width="{w}" widthRelTo="ABSOLUTE" height="{h}" heightRelTo="ABSOLUTE" protect="0"/>"#,
            r#"<hp:pos treatAsChar="{c}" affectLSpacing="0" flowWithText="1" allowOverlap="0""#,
            r#" holdAnchorAndSO="0" vertRelTo="{rel}" horzRelTo="{rel}" vertAlign="TOP""#,
            r#" horzAlign="{horz}" vertOffset="0" horzOffset="0"/>"#,
            r#"<hp:outMargin left="0" right="0" top="0" bottom="0"/>"#,
            r#"</hp:pic>"#,
        ),
        w = w,
        h = h,
        id = id,
        c = as_char,
        rel = rel,
        horz = horz
    )
}

/// **그 단락의 글자를 비운다.** 구간과 `hp:pic` 은 그대로 둔다.
///
/// `create` 의 그림 블록이 쓴다: 백엔드의 `scaffold` 는 그림을 모르므로 그
/// 자리에 **표지 글자**가 든 단락을 하나 만들게 하고, 그 단락을 찾아 그림을
/// 넣은 뒤 표지를 지운다. 표지를 남기면 문서에 쓰레기 글자가 박힌다.
///
/// `<hp:t>…</hp:t>` 의 **내용만** 비우는 이유: 태그째 지우면 구간(`hp:run`)이
/// 글자 없는 꼴이 되는데 그게 스키마에 맞는지 재지 않았다. 빈 `hp:t` 는
/// 실물에 있다.
pub fn clear_para_text(pkg: &mut Package, section: u32, para: u32) -> Result<usize> {
    let name = format!("Contents/section{section}.xml");
    let Some(xml) = pkg.text(&name) else {
        bail!("구역 {section} 이 없습니다 ({name})");
    };
    let (p_start, p_end) = find_body_para(&xml, para)?;
    let mut out = String::with_capacity(xml.len());
    out.push_str(&xml[..p_start]);
    let mut rest = &xml[p_start..p_end];
    let mut cleared = 0usize;
    while let Some(s0) = rest.find("<hp:t>") {
        let body = s0 + "<hp:t>".len();
        let Some(e0) = rest[body..].find("</hp:t>").map(|i| i + body) else { break };
        out.push_str(&rest[..body]);
        if e0 > body {
            cleared += 1;
        }
        rest = &rest[e0..];
    }
    out.push_str(rest);
    out.push_str(&xml[p_end..]);
    pkg.set_text(&name, out);
    Ok(cleared)
}

/// mm → HWPUNIT (1/7200 inch).
pub fn mm_to_hwpunit(mm: f64) -> u32 {
    (mm * 7200.0 / 25.4).round().max(0.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 속성을_바꾸되_없는_속성은_더하지_않는다() {
        let xml = r#"<hp:margin header="1" left="100" right="100"/>"#;
        let (out, n) = set_attr(xml, "<hp:margin", "left", "7087", false);
        assert_eq!(n, 1);
        assert!(out.contains(r#"left="7087""#), "{out}");
        assert!(out.contains(r#"header="1""#), "다른 속성을 건드렸다: {out}");
        // 없는 속성은 더하지 않는다 — 모르는 자리에 넣으면 스키마를 어긴다
        let (out2, n2) = set_attr(xml, "<hp:margin", "gutter", "0", false);
        assert_eq!(n2, 0);
        assert_eq!(out2, xml);
    }

    #[test]
    fn mm_환산() {
        assert_eq!(mm_to_hwpunit(25.0), 7087);
        assert_eq!(mm_to_hwpunit(15.0), 4252);
        assert_eq!(mm_to_hwpunit(210.0), 59528);
    }
}

#[cfg(test)]
mod indent_tests {
    use super::*;

    /// **들여쓰기는 `hp:switch` 두 묶음에 1배와 2배로 적힌다.**
    ///
    /// 실측(HWPX 8개 문서의 `paraPr` 615개): 0 이 아닌 544건 중 540건에서
    /// `hp:default` = `hp:case` × 2. 어긋난 4건은 반올림이다. 양쪽 다
    /// `unit="HWPUNIT"` 이라고 적혀 있으므로 **속성을 믿으면 한쪽이 2배로
    /// 틀린다.**
    ///
    /// 되읽기로는 못 잡는다 — 우리 증인도 `rhwp` 도 `hp:case` 만 읽는다
    /// (`hp:default` 에 1배로 적어 보고 `.hwp` 로 변환해 확인했다: 결과가
    /// 같았다). 그래서 여기서 센다.
    const SWITCHED: &str = concat!(
        r#"<hh:paraPr id="3" tabPrIDRef="0"><hh:align horizontal="JUSTIFY" vertical="BASELINE"/>"#,
        r#"<hp:switch>"#,
        r#"<hp:case hp:required-namespace="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar">"#,
        r#"<hh:margin><hc:intent value="0" unit="HWPUNIT"/><hc:left value="0" unit="HWPUNIT"/>"#,
        r#"<hc:right value="0" unit="HWPUNIT"/><hc:prev value="0" unit="HWPUNIT"/>"#,
        r#"<hc:next value="0" unit="HWPUNIT"/></hh:margin>"#,
        r#"<hh:lineSpacing type="PERCENT" value="160" unit="HWPUNIT"/></hp:case>"#,
        r#"<hp:default>"#,
        r#"<hh:margin><hc:intent value="0" unit="HWPUNIT"/><hc:left value="0" unit="HWPUNIT"/>"#,
        r#"<hc:right value="0" unit="HWPUNIT"/><hc:prev value="0" unit="HWPUNIT"/>"#,
        r#"<hc:next value="0" unit="HWPUNIT"/></hh:margin>"#,
        r#"<hh:lineSpacing type="PERCENT" value="160" unit="HWPUNIT"/></hp:default>"#,
        r#"</hp:switch></hh:paraPr>"#,
    );

    #[test]
    fn 들여쓰기는_두_묶음에_1배와_2배로_적힌다() {
        // 내어쓰기: 첫 줄 0, 나머지 2835 → left = 0, intent = -2835
        let out = set_margin_indent(SWITCHED, 0, -2835, None).expect("적기");
        let branches: Vec<&str> = out
            .split("<hp:")
            .filter(|b| b.starts_with("case") || b.starts_with("default"))
            .collect();
        assert_eq!(branches.len(), 2, "묶음이 둘이어야 한다\n{out}");
        assert!(
            branches[0].contains(r#"<hc:intent value="-2835""#),
            "hp:case 는 1배여야 한다\n{}", branches[0]
        );
        assert!(
            branches[1].contains(r#"<hc:intent value="-5670""#),
            "hp:default 는 2배여야 한다 — 같은 수를 쓰면 그쪽을 읽는 도구에서 \
             들여쓰기가 절반이 된다\n{}", branches[1]
        );
        // 손대지 않은 속성은 그대로
        assert_eq!(out.matches(r#"<hc:right value="0""#).count(), 2, "{out}");
        assert_eq!(out.matches(r#"value="160""#).count(), 2, "줄 간격을 건드렸다\n{out}");
    }

    /// **문단 간격도 같은 배수 규칙을 탄다.**
    ///
    /// 되읽기로는 못 잡는다 — 우리 증인도 `rhwp` 도 `hp:case` 만 읽는다.
    /// 실측으로 확인했다: `hp:default` 에 1배로 적고 `.hwp` 로 변환해 양쪽을
    /// 읽어 봤는데 **결과가 같았다**(10.0pt / 10.0pt). 그래서 여기서 센다.
    #[test]
    fn 문단_간격도_두_묶음에_1배와_2배로_적힌다() {
        // 위 10pt · 아래 3pt
        let out = set_margin_indent(SWITCHED, 0, 0, Some((1_000, 300))).expect("적기");
        let branches: Vec<&str> = out
            .split("<hp:")
            .filter(|b| b.starts_with("case") || b.starts_with("default"))
            .collect();
        assert_eq!(branches.len(), 2, "묶음이 둘이어야 한다\n{out}");
        assert!(
            branches[0].contains(r#"<hc:prev value="1000""#)
                && branches[0].contains(r#"<hc:next value="300""#),
            "hp:case 는 1배여야 한다\n{}", branches[0]
        );
        assert!(
            branches[1].contains(r#"<hc:prev value="2000""#)
                && branches[1].contains(r#"<hc:next value="600""#),
            "hp:default 는 2배여야 한다 — 같은 수를 쓰면 그쪽을 읽는 도구에서 \
             간격이 절반이 된다\n{}", branches[1]
        );
        // 간격만 달라고 했으면 줄 간격은 건드리지 않는다
        assert_eq!(out.matches(r#"value="160""#).count(), 2, "줄 간격을 건드렸다\n{out}");
    }

    /// **두 벌이 다 있어야 쓴다.** 한쪽만 고치면 다른 쪽에 옛 값이 남고,
    /// 한글이 어느 쪽을 읽느냐에 따라 결과가 갈린다 — 조용히.
    #[test]
    fn 묶음이_하나뿐이면_거부한다() {
        let one = SWITCHED
            .replace(r#"<hp:default>"#, "<!--")
            .replace("</hp:default>", "-->");
        let e = set_margin_indent(&one, 0, -100, None).expect_err("거부해야 한다");
        assert!(format!("{e}").contains("두 벌"), "{e}");
    }

    /// 모르는 `hp:case` 묶음은 **멈춘다.** 조용히 1배로 쓰면 그 묶음이
    /// 2배로 틀리는데, 단위를 재지 않은 묶음에 대고 추측할 수 없다.
    #[test]
    fn 모르는_case_묶음이면_멈춘다() {
        let odd = SWITCHED.replace(
            "http://www.hancom.co.kr/hwpml/2016/HwpUnitChar",
            "http://example.com/other",
        );
        let e = set_margin_indent(&odd, 0, -100, None).expect_err("거부해야 한다");
        let m = format!("{e}");
        assert!(m.contains("HwpUnitChar"), "{m}");
        assert!(m.contains("재지 않았"), "{m}");
    }
}

#[cfg(test)]
mod pic_tests {
    use super::*;

    /// **크기는 여섯 곳에 같은 수로 적힌다.**
    ///
    /// 저장 검증은 이것을 못 잡는다 — 증인은 `placed` 를 `hp:orgSz ×
    /// hc:scaMatrix` 로 구하고(`hp:sz` 는 그게 없을 때의 대안), 그러니 나머지
    /// 다섯 곳을 틀리게 써도 되읽기는 "맞다" 고 한다. 실측으로 확인했다:
    /// `hp:sz/@height` 만 999 로 바꿔 넣었는데 저장이 **성공했다**.
    ///
    /// 여섯이 어긋나는 것은 문서의 사고가 아니라 **이 함수의 코드 결함**이므로,
    /// 막는 자리도 되읽기가 아니라 생성된 XML 을 보는 여기다.
    ///
    /// 실물에서 여섯이 다를 수는 있다(`orgSz` 는 원래 크기, `sz` 는 놓인 크기,
    /// `scaMatrix` 가 그 사이를 잇는다). 그래서 증인에게 "여섯이 같아야 한다"
    /// 를 물리면 **남의 문서를 틀렸다고 한다.** 우리가 쓰는 것만 배율 1 로
    /// 맞춰 둔다 — "원래 크기 w×h, 놓인 크기 w×h, 늘이지 않음".
    #[test]
    fn 그림_크기는_일곱_자리에_같은_수로_적힌다() {
        let xml = pic_xml("image7", 15600, 2400, true, crate::images::Align::Left);
        for attr in [
            r#"<hp:orgSz width="15600" height="2400"/>"#,
            r#"<hp:curSz width="15600" height="2400"/>"#,
            r#"<hc:pt2 x="15600" y="2400"/>"#,
            r#"<hp:imgClip left="0" right="15600" top="0" bottom="2400"/>"#,
            r#"<hp:imgDim dimwidth="15600" dimheight="2400"/>"#,
            r#"<hp:sz width="15600" widthRelTo="ABSOLUTE" height="2400""#,
        ] {
            assert!(xml.contains(attr), "크기가 빠진 자리가 있다: {attr}\n{xml}");
        }
        // 배율은 1 이어야 한다 — 그렇지 않으면 `orgSz` 와 놓인 크기가 갈린다
        assert!(
            xml.contains(r#"<hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>"#),
            "배율이 1 이 아니면 적어 둔 크기와 놓인 크기가 갈린다\n{xml}"
        );
        // **몇 번 나오는가까지 센다.** 위 목록은 "있어야 할 것이 있나" 만 보고,
        // 한 곳을 다른 수로 바꿔 넣은 것은 그 수를 세야 걸린다.
        //
        // 7 이다 — `imgRect` 가 가로를 pt1·pt2 에, 세로를 pt2·pt3 에 각각 두 번
        // 적는다. 여섯 "곳" 이지만 적히는 자리는 일곱이다(세어 보고 바로잡았다).
        assert_eq!(xml.matches("15600").count(), 7, "가로가 적힌 자리 수가 다르다\n{xml}");
        assert_eq!(xml.matches("2400").count(), 7, "세로가 적힌 자리 수가 다르다\n{xml}");
    }

    /// 글자처럼취급은 `hp:pos/@treatAsChar` 한 곳이고, 기준 좌표가 함께 간다.
    #[test]
    fn 글자처럼_여부가_기준_좌표까지_바꾼다() {
        let inline = pic_xml("i", 100, 100, true, crate::images::Align::Left);
        assert!(inline.contains(r#"treatAsChar="1""#), "{inline}");
        assert!(inline.contains(r#"vertRelTo="PARA""#), "{inline}");
        let float = pic_xml("i", 100, 100, false, crate::images::Align::Center);
        assert!(float.contains(r#"treatAsChar="0""#), "{float}");
        assert!(float.contains(r#"vertRelTo="PAPER""#), "{float}");
        // 떠 있는 그림만 개체 정렬을 쓴다 — 글자처럼은 문단이 한다
        assert!(float.contains(r#"horzAlign="CENTER""#), "{float}");
        assert!(inline.contains(r#"horzAlign="LEFT""#), "{inline}");
    }

    /// **이미 있는 이름에는 넣지 않는다.** 덮으면 그 그림이 조용히 사라진다.
    #[test]
    fn 이미_있는_부품_이름이면_거부한다() {
        let src = std::path::Path::new("/home/k821209/works/hwp-vi/samples/c00.hwpx");
        if !src.is_file() {
            return;
        }
        let mut pkg = Package::open(src).expect("풀기");
        // c00 은 image1·image2 를 쓴다
        let e = insert_image(&mut pkg, 0, 0, "BinData/image1.png", vec![1, 2, 3], 100, 100, true, crate::images::Align::Left)
            .expect_err("있는 이름이면 거부해야 한다");
        assert!(format!("{e}").contains("이미 있습니다"), "{e}");
        // 다음 빈 이름은 그 둘을 피한다
        let next = next_bin_part(&pkg, "png").expect("빈 이름");
        assert_eq!(next, "BinData/image3.png");
    }
}
