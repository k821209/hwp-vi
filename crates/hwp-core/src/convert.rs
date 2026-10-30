//! 포맷 변환 — **증인이 양쪽을 대조한 뒤에만** 파일을 남긴다.
//!
//! 백엔드에 변환이 두 방향 다 있다(`export-hwpx` → HWPX, `convert` → HWP5).
//! 둘 다 `--verify` 를 받는데, 그건 **자기 산출물을 자기가 다시 파싱하는 것**
//! 이다 — `scaffold` 때와 같은 이유로 그것만으로는 믿지 않는다. 우리 독립
//! 판독기로 원본과 산출물을 읽어 대조하고, 같을 때만 자리에 놓는다.
//!
//! ## 무엇을 대조하는가 — 재서 고른 축 다섯 개
//!
//! 실무 문서 10개(hwp 5 + hwpx 5)를 양방향으로 돌려 **포맷 간에 비교 가능한
//! 것만** 골랐다. 아무거나 비교하면 손실이 아닌 것이 손실로 잡힌다.
//!
//! 1. 본문 단락 글자 — **보이는 글자로**(`offset::visible`)
//! 2. 글자별 꾸밈(진하게·기울임·밑줄·취소선) — **구간이 아니라 글자별로**
//! 3. 표 칸 글자 — `(표, 행, 열)` 마다
//! 4. 표의 꼴 — 번호마다의 행·열 (앵커는 증인이 모른다. 아래 참고)
//! 5. 그림이 올라앉은 단락과 그 수 — **자리까지** (수만 보면 옮겨 가도 통과한다)
//!
//! ## 비교하지 않는 것, 그리고 그 이유
//!
//! - **날 글자**(컨트롤 포함). 두 포맷은 **개체 자리 표시의 수가 다르다** —
//!   p.hwpx 단락0 은 HWP5 쪽에 U+FFFC 가 3개 더 있다(구역 정의·쪽번호 같은
//!   확장 컨트롤이고 OWPML 은 본문 글자로 내지 않는다). 손실이 아니다.
//! - **꾸밈 구간의 경계.** 같은 27구간인데 경계가 갈리는 경우가 있고, 글자
//!   마다의 속성은 같다. 경계로 비교하면 손실이 아닌 것을 손실로 읽는다.
//! - **컨테이너 부품 수.** ZIP 부품과 CFB 스트림은 애초에 다른 것이다
//!   (실측: ZIP 12개 ↔ CFB 11개).

use crate::offset;
use crate::witness::Testimony;
use crate::Rhwp;
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 변환 결과.
#[derive(Debug, Clone)]
pub struct ConvertReport {
    pub path: PathBuf,
    /// `hwp` 또는 `hwpx`
    pub to: String,
    /// 대조한 본문 단락 수
    pub paragraphs: usize,
    /// 대조한 표 칸 수
    pub cells: usize,
    /// 대조한 그림 수
    pub images: usize,
    /// **막지는 않지만 말해 둘 것.** 문서 내용은 같은데 달라진 것들.
    pub warnings: Vec<String>,
}

/// 비교 단위 — 포맷 간에 뜻이 같은 것만 담는다.
#[derive(Debug, PartialEq, Eq)]
struct Facts {
    /// `(구역, 단락)` → 보이는 글자
    paras: BTreeMap<(u32, u32), String>,
    /// `(구역, 단락)` → 글자마다의 꾸밈 표(`B`·`I`·`U`·`S` 조합)
    deco: BTreeMap<(u32, u32), Vec<String>>,
    /// `(표, 행, 열)` → 칸 글자
    cells: BTreeMap<(u32, usize, usize), String>,
    /// 표 번호 → `(행, 열)`.
    ///
    /// **표의 앵커(어느 단락에 올라앉았는지)는 넣지 않는다.** 증인이 그걸 모르고
    /// (백엔드가 말해 준다) 백엔드에게 물으면 **백엔드가 자기 산출물을 자기
    /// 값으로 채점하는** 꼴이 된다. 이 모듈이 피하려는 바로 그것이다. 대신
    /// 표의 꼴과 모든 칸 글자를 보므로, 표가 사라지거나 늘거나 모양이 바뀌면
    /// 잡힌다.
    tables: BTreeMap<u32, (usize, usize)>,
    /// `(구역, 단락)` → 그 단락에 올라앉은 그림의 수.
    ///
    /// **수만이 아니라 자리까지** 비교한다. 실측으로 문서 10개 양방향에서
    /// 자리·수가 전부 같았으므로 비교 가능한 축이다. 수만 보면 그림이 다른
    /// 단락으로 옮겨 가도 통과한다.
    images_at: BTreeMap<(u32, u32), usize>,
}

fn facts(t: &Testimony, path: &Path) -> Result<Facts> {
    let mut paras = BTreeMap::new();
    let mut deco = BTreeMap::new();
    for p in t.body_paragraphs() {
        let Some(i) = p.index else { continue };
        let text = offset::visible(&p.text);
        // **글자별 꾸밈.** 구간 경계는 포맷에 따라 갈릴 수 있으나 글자마다의
        // 속성은 같아야 한다. 구간은 날 글자 기준이고 여기 글자는 보이는
        // 글자이므로, 날 글자를 훑으며 보이는 자리만 적는다.
        let raw: Vec<char> = p.text.chars().collect();
        let mut marks: Vec<String> = Vec::with_capacity(text.chars().count());
        let mut at: Vec<Option<usize>> = Vec::with_capacity(raw.len());
        for (n, c) in raw.iter().enumerate() {
            if *c == '\u{FFFC}' {
                at.push(None);
            } else {
                at.push(Some(n));
                marks.push(String::new());
            }
        }
        // 보이는 글자의 순번 ← 날 글자의 순번
        let visible_index: BTreeMap<usize, usize> = at
            .iter()
            .enumerate()
            .filter_map(|(n, keep)| keep.map(|_| n))
            .enumerate()
            .map(|(v, n)| (n, v))
            .collect();
        for r in &p.runs {
            for n in r.start..(r.start + r.len) {
                let Some(&v) = visible_index.get(&n) else { continue };
                if let Some(m) = marks.get_mut(v) {
                    let d = &r.deco;
                    *m = format!(
                        "{}{}{}{}",
                        if d.bold { "B" } else { "" },
                        if d.italic { "I" } else { "" },
                        if d.underline { "U" } else { "" },
                        if d.strike { "S" } else { "" }
                    );
                }
            }
        }
        paras.insert((p.section, i), text);
        deco.insert((p.section, i), marks);
    }
    let cells = t
        .tables
        .iter()
        .flat_map(|tb| {
            tb.cells
                .iter()
                .map(move |c| ((tb.index, c.row, c.col), c.text.clone()))
        })
        .collect();
    let tables: BTreeMap<u32, (usize, usize)> =
        t.tables.iter().map(|tb| (tb.index, (tb.rows, tb.cols))).collect();
    let mut images_at: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for (coord, idx) in crate::images::by_paragraph(&crate::images::read(path)?) {
        images_at.insert(coord, idx.len());
    }
    Ok(Facts { paras, deco, cells, tables, images_at })
}

/// 두 쪽의 차이. `hard` 가 비어 있지 않으면 변환을 멈춘다. `soft` 는 말만 한다.
///
/// **가르는 기준은 "보이는 글자가 달라졌는가" 다.** 거르는 것이 아니라 등급을
/// 나누는 것이다 — 이 저장소의 1번 재발 패턴이 "시험을 깔끔하게 하려고 걸러낸
/// 것이 결함이 사는 자리" 이므로, 어느 쪽도 조용히 사라지지 않게 한다.
#[derive(Default)]
struct Diff {
    hard: Vec<String>,
    soft: Vec<String>,
}

fn differences(a: &Facts, b: &Facts) -> Diff {
    let mut d = Diff::default();
    let lines = &mut d.hard;
    if a.paras.len() != b.paras.len() {
        lines.push(format!(
            "본문 단락 수: 원본 {} / 산출물 {}",
            a.paras.len(),
            b.paras.len()
        ));
    }
    for (k, want) in &a.paras {
        match b.paras.get(k) {
            Some(got) if got == want => {}
            Some(got) => lines.push(format!(
                "구역{} 단락{} 의 글자가 다릅니다\n    원본  : {want:?}\n    산출물: {got:?}",
                k.0, k.1
            )),
            None => lines.push(format!("구역{} 단락{} 이 산출물에 없습니다", k.0, k.1)),
        }
        if lines.len() >= 6 {
            break;
        }
    }
    // **꾸밈 차이가 강제 줄바꿈 자리에만 있으면 멈추지 않는다.**
    //
    // 실측(s11.hwpx → .hwp): HWPX 는 그 줄바꿈이 진한 구간 안인데
    // (`start=71 len=1 bold`) HWP5 쪽에는 그 구간이 없다. 보이는 글자 160자는
    // 같다. 줄바꿈은 글자를 그리지 않으므로 **문서가 달라 보이지 않는다.**
    //
    // 그렇다고 없는 일로 두지는 않는다. 백엔드의 `insert-text` 는 **왼쪽 구간**
    // 의 속성을 물려받으므로(이 저장소의 체크박스 작업에서 실측), 그 줄바꿈
    // 바로 뒤에 글자를 넣으면 속성이 달라진다. 보이지 않지만 **고칠 때 보인다.**
    let mut lb_only = 0usize;
    for (k, want) in &a.deco {
        let Some(got) = b.deco.get(k) else { continue };
        if got == want {
            continue;
        }
        let chars: Vec<char> = a.paras.get(k).map(|s| s.chars().collect()).unwrap_or_default();
        let visible_differs = want.len() != got.len()
            || want
                .iter()
                .zip(got)
                .enumerate()
                .any(|(i, (x, y))| x != y && chars.get(i) != Some(&'\n'));
        if visible_differs {
            if lines.len() < 6 {
                lines.push(format!("구역{} 단락{} 의 글자 꾸밈이 다릅니다", k.0, k.1));
            }
        } else {
            lb_only += 1;
        }
    }
    if lb_only > 0 {
        d.soft.push(format!(
            "강제 줄바꿈 {lb_only}곳의 글자 속성이 바뀌었습니다 — 보이는 글자는 그대로입니다. \
             다만 그 줄바꿈 **바로 뒤**에 글자를 넣으면 물려받는 속성이 달라집니다 \
             (백엔드는 왼쪽 구간의 속성을 물려줍니다)"
        ));
    }
    let lines = &mut d.hard;
    if lines.len() < 6 {
        if a.cells.len() != b.cells.len() {
            lines.push(format!(
                "표 칸 수: 원본 {} / 산출물 {}",
                a.cells.len(),
                b.cells.len()
            ));
        }
        for (k, want) in &a.cells {
            if b.cells.get(k) != Some(want) {
                lines.push(format!(
                    "표{} ({},{}) 의 글자가 다릅니다\n    원본  : {want:?}\n    산출물: {:?}",
                    k.0,
                    k.1,
                    k.2,
                    b.cells.get(k)
                ));
                if lines.len() >= 6 {
                    break;
                }
            }
        }
    }
    if lines.len() < 6 && a.tables != b.tables {
        lines.push(format!(
            "표의 꼴이 다릅니다 (번호 → 행×열) — 원본 {:?} / 산출물 {:?}",
            a.tables, b.tables
        ));
    }
    if lines.len() < 6 && a.images_at != b.images_at {
        lines.push(format!(
            "그림이 올라앉은 단락이 다릅니다 — 원본 {:?} / 산출물 {:?}",
            a.images_at, b.images_at
        ));
    }
    d
}

/// 목적지 확장자로 정해지는 백엔드 명령.
fn command_for(dest: &Path) -> Result<&'static str> {
    match dest.extension().and_then(|e| e.to_str()) {
        Some("hwpx") => Ok("export-hwpx"),
        Some("hwp") => Ok("convert"),
        Some(other) => bail!(
            "`.{other}` 로는 변환할 수 없습니다 — `.hwp` 또는 `.hwpx` 만 됩니다"
        ),
        None => bail!("목적지에 확장자가 없습니다 — `.hwp` 또는 `.hwpx` 를 주세요"),
    }
}

/// 변환하지 않고 **무엇을 할지만** 말한다. 디스크를 건드리지 않는다.
pub fn plan(src: &Path, dest: &Path) -> Result<serde_json::Value> {
    let cmd = command_for(dest)?;
    let t = Testimony::read(src)?;
    let f = facts(&t, src)?;
    Ok(serde_json::json!({
        "from": src.extension().and_then(|e| e.to_str()),
        "to": dest.extension().and_then(|e| e.to_str()),
        "backend": cmd,
        "willCompare": {
            "paragraphs": f.paras.len(),
            "cells": f.cells.len(),
            "tables": f.tables.len(),
            "images": f.images_at.values().sum::<usize>(),
        },
        "verifiedBy": "witness",
    }))
}

/// 변환하고 **증인으로 대조한 뒤에만** 자리에 놓는다.
pub fn convert(rhwp: &Rhwp, src: &Path, dest: &Path, overwrite: bool) -> Result<ConvertReport> {
    let cmd = command_for(dest)?;
    if crate::edit::same_file(src, dest) {
        bail!("원본과 목적지가 같은 파일입니다 — 다른 이름을 주세요");
    }
    if dest.exists() && !overwrite {
        bail!(
            "{} 이 이미 있습니다 — 덮으려면 그렇게 말해 주세요",
            dest.display()
        );
    }

    let before = Testimony::read(src).with_context(|| format!("{} 를 읽을 수 없습니다", src.display()))?;
    let want = facts(&before, src)?;

    // **목적지 옆에 임시본을 만든다.** 다른 파일시스템에 두면 마지막 `rename`
    // 이 복사가 되고, 끊기면 반쪽 파일이 자리에 남는다.
    // **임시본도 목적지의 확장자를 가져야 한다.** 백엔드는 확장자로 방향을
    // 정하므로 `cv.hwp.hwpconv` 는 거부당한다(실측: "convert 출력 경로는
    // .hwp 확장자여야 합니다"). 그래서 표식을 확장자 **앞**에 넣는다.
    let ext = dest.extension().and_then(|e| e.to_str()).unwrap_or("tmp");
    let stem = dest.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    let tmp = dest
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".{stem}.hwpconv.{ext}"));
    let _guard = Cleanup(tmp.clone());
    let _ = std::fs::remove_file(&tmp);

    let (s, d) = (src.display().to_string(), tmp.display().to_string());
    // 백엔드의 `--verify` 도 켠다. 믿는 근거는 아니지만, 백엔드가 자기 쪽에서
    // 이미 아는 문제를 들고 있으면 우리 대조 전에 멈춰 주는 것이 낫다.
    rhwp.run(&[cmd, &s, &d, "--verify"])
        .with_context(|| format!("백엔드 {cmd} 가 실패했습니다"))?;
    if !tmp.is_file() {
        bail!("백엔드가 {} 를 만들지 않았습니다", tmp.display());
    }

    let after = Testimony::read(&tmp)
        .with_context(|| format!("{} 를 다시 읽을 수 없습니다", tmp.display()))?;
    let got = facts(&after, &tmp)?;

    let diff = differences(&want, &got);
    if !diff.hard.is_empty() {
        bail!(
            "변환을 멈췄습니다 — 산출물이 원본과 다릅니다. 파일을 남기지 않았습니다.\n  {}\n\n\
             `--force` 는 없습니다: 무엇이 달라졌는지 모르는 채로 바뀐 문서를 주지 않습니다.",
            diff.hard.join("\n  ")
        );
    }

    let images = want.images_at.values().sum::<usize>();

    std::fs::rename(&tmp, dest)
        .with_context(|| format!("{} 로 옮길 수 없습니다", dest.display()))?;

    Ok(ConvertReport {
        path: dest.to_path_buf(),
        to: dest
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string(),
        paragraphs: want.paras.len(),
        cells: want.cells.len(),
        images,
        warnings: diff.soft,
    })
}

/// 임시본은 어떤 길로 나가도 치운다.
struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
