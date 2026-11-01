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
    order: Vec<String>,
    parts: BTreeMap<String, Vec<u8>>,
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
