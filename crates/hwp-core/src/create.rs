//! 문서를 **새로 만든다** — 명세에서, 그리고 만든 것을 증인으로 대조한다.
//!
//! 사용자 요청(2026-10-29): "AI 가 HWP 파일을 만들고 수정하고 저장하기 편하게."
//! 고치기와 저장은 이미 있었고 **만들기가 비어 있었다** — 기계 쪽 입구에
//! `create` 가 없어서 AI 는 이미 있는 문서만 손댈 수 있었다.
//!
//! ## 왜 백엔드를 그냥 부르지 않는가
//!
//! 백엔드에 `scaffold` 가 있다(구조화된 명세 → HWPX). 그걸 그대로 통과시키면
//! 이 프로젝트의 중심 장치가 만들기 경로에만 빠진다 — **백엔드가 말한 것을
//! 독립적으로 다시 세는** 그 장치다. `scaffold` 는 자기가 만든 것을 자기가
//! 세어 `blockCount`·`paragraphCount`·`tableCount` 로 보고하는데, 그건
//! 작성자가 자기 답을 채점하는 것이다.
//!
//! 그래서 저장과 같은 모양으로 간다:
//!
//! 1. 명세에서 **"이렇게 되어야 한다"** 를 뽑는다 — 글줄마다의 글자, 표마다의 칸.
//! 2. 백엔드로 임시본을 만든다.
//! 3. 그 임시본을 **증인으로 읽어** 1과 대조한다.
//! 4. 같으면 목적지로 `rename`. 다르면 **아무것도 남기지 않고** 무엇이 달랐는지 말한다.
//!
//! `--force` 는 여기에도 없다.
//!
//! ## 명세 (백엔드가 받는 그대로)
//!
//! ```json
//! {"version":"1","title":"과업지시서","font":"함초롬바탕",
//!  "page_size":{"width_mm":210,"height_mm":297},
//!  "blocks":[
//!    {"type":"heading","level":1,"text":"1. 과업명"},
//!    {"type":"paragraph","text":"○ 기후변화 대응 기술개발"},
//!    {"type":"table","rows":[["항목","값"],["예산","1,000"]],"cell_align":"center"}
//!  ]}
//! ```
//!
//! 블록은 `heading`·`paragraph`·`table` 셋이고 `cell_align` 은
//! `justify|left|center|right` 다. 실측으로 확인한 것이며, 우리가 다시 적지
//! 않고 **그대로 넘긴다** — 베껴 적으면 백엔드가 늘 때 어긋난다.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::rhwp::Rhwp;
use crate::witness::Testimony;

/// 만든 결과. 수는 **증인이 센 것**이다 — 백엔드가 보고한 수가 아니다.
#[derive(Debug, Clone)]
pub struct CreateReport {
    pub path: PathBuf,
    /// 명세의 블록 수
    pub blocks: usize,
    /// 증인이 센 본문 단락 수
    pub paragraphs: usize,
    /// 증인이 센 표 수
    pub tables: usize,
    /// 증인이 센 표 칸 수
    pub cells: usize,
}

/// 명세에서 뽑은 "이렇게 되어야 한다".
#[derive(Debug)]
struct Wanted {
    /// 글줄마다 들어가야 할 글자 (heading·paragraph 의 `text`)
    lines: Vec<String>,
    /// 표마다의 격자
    tables: Vec<Vec<Vec<String>>>,
}

fn wanted_from(spec: &Value) -> Result<Wanted> {
    let blocks = spec
        .get("blocks")
        .and_then(|b| b.as_array())
        .ok_or_else(|| anyhow::anyhow!("명세에 `blocks` 배열이 없습니다"))?;
    if blocks.is_empty() {
        bail!("명세의 `blocks` 가 비었습니다 — 빈 문서는 만들 것이 없습니다");
    }
    let mut lines = Vec::new();
    let mut tables = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        let kind = b.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match kind {
            "heading" | "paragraph" => {
                let t = b.get("text").and_then(|t| t.as_str()).ok_or_else(|| {
                    anyhow::anyhow!("blocks[{i}] ({kind}) 에 `text` 가 없습니다")
                })?;
                lines.push(t.to_string());
            }
            "table" => {
                let rows = b.get("rows").and_then(|r| r.as_array()).ok_or_else(|| {
                    anyhow::anyhow!("blocks[{i}] (table) 에 `rows` 배열이 없습니다")
                })?;
                let grid: Vec<Vec<String>> = rows
                    .iter()
                    .map(|r| {
                        r.as_array()
                            .map(|cs| {
                                cs.iter()
                                    .map(|c| c.as_str().unwrap_or("").to_string())
                                    .collect()
                            })
                            .unwrap_or_default()
                    })
                    .collect();
                if grid.is_empty() || grid[0].is_empty() {
                    bail!("blocks[{i}] (table) 의 `rows` 가 비었습니다");
                }
                // 행마다 열 수가 다르면 격자가 아니다. 백엔드가 어떻게
                // 메우든 우리가 대조할 기준이 사라지므로 먼저 거부한다.
                let w = grid[0].len();
                if let Some(bad) = grid.iter().position(|r| r.len() != w) {
                    bail!(
                        "blocks[{i}] (table) 의 행마다 열 수가 다릅니다 — 0행 {w}개, {bad}행 {}개",
                        grid[bad].len()
                    );
                }
                tables.push(grid);
            }
            other => bail!(
                "blocks[{i}] 의 `type` 이 `{other}` 입니다 (받는 것: heading|paragraph|table)"
            ),
        }
    }
    Ok(Wanted { lines, tables })
}

/// 미리보기 — **실제로 만들어 보고 버린다.**
///
/// 사용자 보고(hwp-vi#3): `dryRun` 이 통과시킨 요청이 저장에서 깨졌다.
/// `{"version":"9.9"}` 이나 객체로 준 `font` 가 미리보기를 통과하고 백엔드의
/// `scaffold` 에서 거부됐다. **미리보기의 쓸모가 "쓰기 전에 확인" 하나인데
/// 그 확인을 지난 요청이 깨지면 장치가 비어 있는 것과 같다** — 부르는 쪽은
/// 미리보기를 믿고 다음 단계를 짠다.
///
/// 전에는 일부러 백엔드를 부르지 않았다. 이유는 "미리보기가 디스크를 건드리지
/// 않는다" 는 약속이었고(기계 쪽 `output` 에서 그 약속이 깨져 남의 문서를
/// 지운 적이 있다), 그 약속은 **사용자가 준 경로**에 대한 것이다. 임시
/// 디렉터리에 만들어 보고 지우는 것은 그 약속을 깨지 않으면서 명세를 실제로
/// 검증한다.
///
/// 그래서 `create` 와 **같은 길**을 간다 — 자가검증까지. 통과한 미리보기는
/// "이 명세로 저장하면 된다" 를 뜻하고, 그게 미리보기가 할 말이다.
pub fn plan(rhwp: &Rhwp, spec: &Value) -> Result<Value> {
    let w = wanted_from(spec)?;
    let dir = std::env::temp_dir().join(format!(
        "hwp-create-plan-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("{} 를 만들 수 없습니다", dir.display()))?;
    let probe = dir.join("probe.hwpx");
    let report = create(rhwp, spec, &probe);
    let _ = std::fs::remove_dir_all(&dir);
    let report = report?;

    Ok(serde_json::json!({
        "blocks": w.lines.len() + w.tables.len(),
        "lines": w.lines,
        "tables": w.tables.iter().map(|g| serde_json::json!({
            "rows": g.len(),
            "cols": g.first().map(|r| r.len()).unwrap_or(0),
        })).collect::<Vec<_>>(),
        "title": spec.get("title").and_then(|t| t.as_str()),
        // 미리보기가 **실제로 만들어 본** 결과다 — 숫자는 증인이 센 것이다.
        "paragraphs": report.paragraphs,
        "cells": report.cells,
        "verifiedBy": "witness (임시본을 만들어 대조한 뒤 지웠습니다)",
    }))
}

/// 명세에서 문서를 만든다. **목적지가 이미 있으면 만들지 않는다.**
///
/// 덮어도 된다는 판단은 부르는 쪽이 한다 — 이 함수는 없는 자리에만 쓴다.
pub fn create(rhwp: &Rhwp, spec: &Value, dest: &Path) -> Result<CreateReport> {
    if dest.exists() {
        bail!("{} 이 이미 있습니다 — 다른 이름을 주세요", dest.display());
    }
    // HWPX 만 만들 수 있다 — 백엔드의 `scaffold` 가 그것만 낸다. 확장자가
    // `.hwp` 면 사용자는 HWP5 를 기대하므로, 조용히 다른 것을 주지 않는다.
    match dest.extension().and_then(|e| e.to_str()) {
        Some("hwpx") => {}
        Some(other) => bail!(
            "`.{other}` 는 만들 수 없습니다 — 만들기는 `.hwpx` 만 됩니다. \
             HWP5 가 필요하면 만든 뒤 `rhwp convert` 로 바꾸세요"
        ),
        None => bail!("목적지에 확장자가 없습니다 — `.hwpx` 로 주세요"),
    }
    if let Some(dir) = dest.parent() {
        if !dir.as_os_str().is_empty() && !dir.is_dir() {
            bail!("{} 폴더가 없습니다", dir.display());
        }
    }

    let want = wanted_from(spec)?;
    let blocks = want.lines.len() + want.tables.len();

    // 명세와 산출물 **둘 다 목적지 옆에** 둔다. `rename` 이 원자적이려면
    // 같은 파일시스템이어야 한다(저장 경로에서 배운 것이다).
    let dir = dest.parent().unwrap_or(Path::new("."));
    let tag = std::process::id();
    let spec_path = dir.join(format!(".hwp-cmd-spec-{tag}.json"));
    let tmp = dir.join(format!(
        ".{}.hwp-cmd-new-{tag}.hwpx",
        dest.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    ));
    let _spec_guard = Cleanup(spec_path.clone());
    let _tmp_guard = Cleanup(tmp.clone());

    std::fs::write(
        &spec_path,
        serde_json::to_vec_pretty(spec).context("명세를 JSON 으로 쓸 수 없습니다")?,
    )
    .with_context(|| format!("{} 에 쓸 수 없습니다", spec_path.display()))?;

    rhwp.run(&[
        "scaffold",
        &spec_path.to_string_lossy(),
        "--format",
        "hwpx",
        "-o",
        &tmp.to_string_lossy(),
        "--json",
    ])
    .context("백엔드가 문서를 만들지 못했습니다")?;

    if !tmp.exists() {
        bail!("백엔드가 성공했다고 했는데 파일이 없습니다: {}", tmp.display());
    }

    // **독립 대조.** 백엔드가 센 수를 받아 적지 않고 증인이 직접 읽는다.
    let witness = Testimony::read(&tmp)
        .with_context(|| format!("만든 문서를 다시 읽을 수 없습니다: {}", tmp.display()))?;

    let mut diffs: Vec<String> = Vec::new();

    // 1) 글줄 — 명세의 글자가 본문에 **순서대로** 있어야 한다.
    //    백엔드가 제목 단락이나 표 앵커 단락을 더 넣으므로 "같은 수"는
    //    요구하지 않는다. 요구는 "명세의 글줄이 그 순서로 나타난다" 다.
    let body: Vec<String> = witness
        .body_paragraphs()
        .iter()
        .map(|p| crate::offset::visible(&p.text))
        .collect();
    let mut at = 0usize;
    for (i, line) in want.lines.iter().enumerate() {
        match body[at..].iter().position(|b| b.trim() == line.trim()) {
            Some(k) => at += k + 1,
            None => diffs.push(format!(
                "명세 {i}번째 글줄이 본문에 없습니다 (이 순서로): {line:?}"
            )),
        }
    }

    // 2) 표 — 격자가 글자까지 같아야 한다.
    // 증인의 표를 격자로 펼친다. 병합은 명세에 없으므로(`rows` 가 직사각형)
    // 좌표로 그대로 놓으면 된다 — 비어 있는 칸은 빈 글자로 남는다.
    let grids: Vec<Vec<Vec<String>>> = witness
        .tables
        .iter()
        .map(|t| {
            let mut g = vec![vec![String::new(); t.cols]; t.rows];
            for c in &t.cells {
                if c.row < t.rows && c.col < t.cols {
                    g[c.row][c.col] = crate::offset::visible(&c.text);
                }
            }
            g
        })
        .collect();
    if grids.len() != want.tables.len() {
        diffs.push(format!(
            "표 수가 다릅니다 — 명세 {}개 / 만들어진 것 {}개",
            want.tables.len(),
            grids.len()
        ));
    }
    for (i, (w, g)) in want.tables.iter().zip(grids.iter()).enumerate() {
        if w.len() != g.len() {
            diffs.push(format!(
                "표{i} 의 행 수가 다릅니다 — 명세 {} / 만들어진 것 {}",
                w.len(),
                g.len()
            ));
            continue;
        }
        for (r, (wr, gr)) in w.iter().zip(g.iter()).enumerate() {
            if wr.len() != gr.len() {
                diffs.push(format!(
                    "표{i} {r}행의 열 수가 다릅니다 — 명세 {} / 만들어진 것 {}",
                    wr.len(),
                    gr.len()
                ));
                continue;
            }
            for (c, (wc, gc)) in wr.iter().zip(gr.iter()).enumerate() {
                if wc.trim() != gc.trim() {
                    diffs.push(format!(
                        "표{i} ({r},{c}) 의 글자가 다릅니다 — 명세 {wc:?} / 만들어진 것 {gc:?}"
                    ));
                }
            }
        }
    }

    if !diffs.is_empty() {
        let shown: Vec<String> = diffs.iter().take(5).cloned().collect();
        bail!(
            "만들기를 멈췄습니다 — 만든 문서가 명세와 다릅니다. 파일을 남기지 않았습니다.\n  {}\n\n  \
             (다른 곳 {}군데 중 처음 {}개)\n  `--force` 는 없습니다: 명세와 다른 문서를 \
             명세대로라고 말하지 않습니다.",
            shown.join("\n  "),
            diffs.len(),
            shown.len()
        );
    }

    let cells: usize = grids.iter().map(|g| g.iter().map(|r| r.len()).sum::<usize>()).sum();
    std::fs::rename(&tmp, dest)
        .with_context(|| format!("{} 로 들여놓을 수 없습니다", dest.display()))?;
    std::mem::forget(_tmp_guard);

    Ok(CreateReport {
        path: dest.to_path_buf(),
        blocks,
        paragraphs: body.len(),
        tables: grids.len(),
        cells,
    })
}

/// 떨어질 때 치운다. 실패 경로마다 `remove_file` 을 적으면 하나를 빠뜨린다.
struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_spec_without_blocks_is_refused_before_anything_runs() {
        let err = wanted_from(&json!({"title": "x"})).unwrap_err();
        assert!(format!("{err}").contains("blocks"), "{err}");
    }

    #[test]
    fn an_empty_blocks_array_is_refused() {
        let err = wanted_from(&json!({"blocks": []})).unwrap_err();
        assert!(format!("{err}").contains("비었"), "{err}");
    }

    #[test]
    fn an_unknown_block_type_names_what_is_accepted() {
        let err = wanted_from(&json!({"blocks":[{"type":"chart","text":"x"}]})).unwrap_err();
        let m = format!("{err}");
        assert!(m.contains("chart"), "무엇이 틀렸는지 말해야 한다: {m}");
        assert!(m.contains("heading"), "받는 것을 말해야 한다: {m}");
    }

    /// **열 수가 들쭉날쭉한 표는 먼저 거부한다.** 백엔드가 어떻게 메우든
    /// 우리가 대조할 기준이 없어지기 때문이다 — 검증되지 않는 쓰기 경로를
    /// 내지 않는다는 이 프로젝트의 규칙이 만들기에도 걸린다.
    #[test]
    fn a_ragged_table_is_refused_with_the_row_that_differs() {
        let err = wanted_from(&json!({"blocks":[
            {"type":"table","rows":[["a","b"],["c"]]}
        ]}))
        .unwrap_err();
        let m = format!("{err}");
        assert!(m.contains("열 수가 다릅니다"), "{m}");
        assert!(m.contains("1행"), "어느 행인지 말해야 한다: {m}");
    }

    #[test]
    fn the_wanted_shape_counts_lines_and_tables_separately() {
        let w = wanted_from(&json!({"blocks":[
            {"type":"heading","level":1,"text":"머리"},
            {"type":"paragraph","text":"글"},
            {"type":"table","rows":[["a","b"],["c","d"]]}
        ]}))
        .unwrap();
        assert_eq!(w.lines, vec!["머리".to_string(), "글".to_string()]);
        assert_eq!(w.tables.len(), 1);
        assert_eq!(w.tables[0].len(), 2);
    }
}
