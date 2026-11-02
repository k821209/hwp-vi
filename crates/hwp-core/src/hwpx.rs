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
pub fn set_para_align(pkg: &mut Package, section: u32, para: u32, horizontal: &str) -> Result<()> {
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

    let (mut fresh, hits) = set_attr(&base, "<hh:align", "horizontal", horizontal, false);
    if hits == 0 {
        bail!("`paraPr` {old_id} 에 `hh:align/@horizontal` 이 없습니다");
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
    let pic = pic_xml(&id, width, height, inline);
    let mut sec2 = sec.clone();
    sec2.insert_str(p_start + run_end_rel, &pic);
    pkg.set_text(&sec_name, sec2);
    Ok(part.to_string())
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
fn pic_xml(id: &str, w: u32, h: u32, inline: bool) -> String {
    let as_char = if inline { "1" } else { "0" };
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
            r#" horzAlign="LEFT" vertOffset="0" horzOffset="0"/>"#,
            r#"<hp:outMargin left="0" right="0" top="0" bottom="0"/>"#,
            r#"</hp:pic>"#,
        ),
        w = w,
        h = h,
        id = id,
        c = as_char,
        rel = rel
    )
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
        let xml = pic_xml("image7", 15600, 2400, true);
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
        let inline = pic_xml("i", 100, 100, true);
        assert!(inline.contains(r#"treatAsChar="1""#), "{inline}");
        assert!(inline.contains(r#"vertRelTo="PARA""#), "{inline}");
        let float = pic_xml("i", 100, 100, false);
        assert!(float.contains(r#"treatAsChar="0""#), "{float}");
        assert!(float.contains(r#"vertRelTo="PAPER""#), "{float}");
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
        let e = insert_image(&mut pkg, 0, 0, "BinData/image1.png", vec![1, 2, 3], 100, 100, true)
            .expect_err("있는 이름이면 거부해야 한다");
        assert!(format!("{e}").contains("이미 있습니다"), "{e}");
        // 다음 빈 이름은 그 둘을 피한다
        let next = next_bin_part(&pkg, "png").expect("빈 이름");
        assert_eq!(next, "BinData/image3.png");
    }
}
