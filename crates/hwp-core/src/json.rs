//! `hwp json` — LLM 이 읽기 쉬운 단일 문서 객체.
//!
//! rhwp 에도 JSON 출력이 여러 개 있다(`digest`, `export-structure`,
//! `export-tables`, `dump`). 쟤들이 못 주는 것만 여기서 채운다 — 실측으로
//! 확인한 빈 칸은 PLAN.md §9-2 에 적혀 있다:
//!
//! 1. **공문서 글머리 개요 트리** — `export-structure` 가 세 모드 전부
//!    `roots=0` 이다. [`crate::outline`] 이 만든다.
//! 2. **본문 순서 인터리빙** — 단락과 표가 한 배열에 본문 순서대로.
//! 3. **병합 해소된 2D 격자** — rhwp 는 평평한 cells + rowSpan/colSpan 을 준다.
//! 4. **토큰 프로파일** — 167 KB(dump) 와 28 KB 사이에 중간이 없었다.
//! 5. **단일 문서 객체** + stdout 파이프.
//!
//! 두 가지는 **승계하고 재발명하지 않는다**: 모든 항목이 rhwp 편집 좌표
//! `(section, para)` 를 들고 다니고, 문서 본문은 `untrusted` 로 표시된다.
//! 공문서를 LLM 에 넣을 때 문서 안의 문장이 모델에게 지시로 읽히면 그게 주입이다.

use anyhow::Result;
use serde::Serialize;
use std::path::Path;

use crate::outline;
use crate::rhwp::Rhwp;
use crate::witness::Testimony;

/// 얼마나 담을지. 토큰을 쓰는 쪽이 고른다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// 본문 텍스트만 — 제일 쌈
    Text,
    /// 개요 트리 + 표 제목 — 문서 파악
    Outline,
    /// 개요 + 본문 + 표 격자 (기본)
    Doc,
    /// + 구역별 단락 수 같은 구조 메타
    Full,
}

impl Profile {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "text" => Some(Self::Text),
            "outline" => Some(Self::Outline),
            "doc" => Some(Self::Doc),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    fn wants_body(self) -> bool {
        matches!(self, Self::Text | Self::Doc | Self::Full)
    }
    fn wants_outline(self) -> bool {
        matches!(self, Self::Outline | Self::Doc | Self::Full)
    }
    fn wants_tables(self) -> bool {
        matches!(self, Self::Doc | Self::Full)
    }
}

pub const SCHEMA_VERSION: &str = "hwpcmd/1";

#[derive(Debug, Serialize)]
pub struct DocJson {
    #[serde(rename = "schemaVersion")]
    pub schema_version: &'static str,
    pub source: Source,
    /// **본문은 신뢰할 수 없는 입력이다.** rhwp 의 관례를 그대로 쓴다.
    pub untrusted: bool,
    #[serde(rename = "untrustedFields")]
    pub untrusted_fields: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<Vec<OutlineNode>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Vec<BodyItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tables: Option<Vec<Table>>,
}

#[derive(Debug, Serialize)]
pub struct Source {
    pub path: String,
    pub format: String,
    pub pages: u32,
    /// 본문 단락 수
    pub paragraphs: usize,
    #[serde(rename = "sectionParagraphs", skip_serializing_if = "Option::is_none")]
    pub section_paragraphs: Option<Vec<usize>>,
    pub profile: &'static str,
}

#[derive(Debug, Serialize)]
pub struct OutlineNode {
    pub section: u32,
    pub para: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    pub text: String,
    /// 주석(`※`, `*`) — 형제가 아니라 앞 항목에 붙는 설명이다
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub note: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<OutlineNode>,
}

/// 본문 순서대로 놓인 항목. 단락과 표가 섞여 있다.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum BodyItem {
    Para {
        section: u32,
        para: u32,
        text: String,
    },
    /// 이 자리에 표가 있다. 내용은 `tables` 에서 `ref` 로 찾는다.
    Table {
        section: u32,
        para: u32,
        #[serde(rename = "ref")]
        reference: String,
    },
}

#[derive(Debug, Serialize)]
pub struct Table {
    pub id: String,
    pub section: u32,
    pub para: u32,
    /// 한 단락에 표가 여럿일 때의 순번
    #[serde(skip_serializing_if = "is_zero")]
    pub control: u32,
    pub rows: usize,
    pub cols: usize,
    /// 병합을 해소한 2D 격자. 병합된 자리는 빈 문자열이고, 값은 좌상단 칸에 있다.
    pub grid: Vec<Vec<String>>,
    /// 병합 목록 — 격자만 보면 왜 빈 칸인지 알 수 없다
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub merges: Vec<Merge>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub header_row: bool,
}

#[derive(Debug, Serialize)]
pub struct Merge {
    pub row: usize,
    pub col: usize,
    #[serde(rename = "rowSpan")]
    pub row_span: usize,
    #[serde(rename = "colSpan")]
    pub col_span: usize,
}

/// 문서를 JSON 객체로 만든다.
pub fn build(rhwp: &Rhwp, path: &Path, profile: Profile) -> Result<DocJson> {
    let info = rhwp.info(path)?;
    let witness = Testimony::read(path)?;

    // 본문 단락 — 편집 좌표를 달고 문서 순서대로.
    let paras: Vec<(u32, u32, String)> = witness
        .body_paragraphs()
        .into_iter()
        .filter_map(|p| {
            p.index
                .map(|i| (p.section, i, crate::offset::visible(&p.text)))
        })
        .collect();

    let tables = if profile.wants_tables() {
        let export = rhwp.export_tables(path)?;
        export.tables.iter().map(convert_table).collect()
    } else {
        Vec::new()
    };

    let outline_tree = if profile.wants_outline() {
        let items: Vec<outline::Item> = paras
            .iter()
            .map(|(s, i, t)| outline::Item {
                section: *s,
                para: *i,
                text: t.clone(),
            })
            .collect();
        Some(outline::build(&items).iter().map(convert_node).collect())
    } else {
        None
    };

    // 본문 순서 인터리빙 — 표가 놓인 단락 뒤에 표 참조를 끼운다.
    let body = if profile.wants_body() {
        let mut out: Vec<BodyItem> = Vec::new();
        for (section, para, text) in &paras {
            // **빈 단락도 넣는다 — 좌표를 숨기면 고칠 수가 없다.**
            //
            // 전에는 `text.trim()` 이 비면 건너뛰었다. 토큰을 아끼려는 것이었지만
            // 이 JSON 의 약속은 "모든 항목이 편집 좌표를 들고 있어서 읽고 바로
            // 고치는 고리가 닫힌다" 다. 빠뜨리면 그 고리가 끊어진다.
            //
            // 실측(2026-10-10): s13.hwp 는 65단락 중 9개(1·4·7·12·25·32·43·49·64)가
            // 사라졌고, **s16.hwp 는 13단락이 전부 사라져 본문 단락이 하나도
            // 없는 문서로 보였다**(13개가 다 표·그림만 든 빈 단락이다). 그 상태로는
            // 어디에 줄을 넣어야 할지 고를 좌표가 없다. 사람은 화면 왼쪽 여백에서
            // 그 번호를 본다.
            out.push(BodyItem::Para {
                section: *section,
                para: *para,
                text: text.clone(),
            });
            // 한 단락에 표가 여럿이면 control 순으로 — 순서가 흔들리면
            // LLM 이 "두 번째 표" 를 가리킬 수 없다.
            let mut here: Vec<&Table> = tables
                .iter()
                .filter(|t| t.section == *section && t.para == *para)
                .collect();
            here.sort_by_key(|t| (t.control, t.id.clone()));
            for t in here {
                out.push(BodyItem::Table {
                    section: *section,
                    para: *para,
                    reference: t.id.clone(),
                });
            }
        }
        Some(out)
    } else {
        None
    };

    Ok(DocJson {
        schema_version: SCHEMA_VERSION,
        source: Source {
            path: path.display().to_string(),
            format: info.format,
            pages: info.page_count,
            paragraphs: paras.len(),
            section_paragraphs: matches!(profile, Profile::Full)
                .then(|| witness.section_counts()),
            profile: match profile {
                Profile::Text => "text",
                Profile::Outline => "outline",
                Profile::Doc => "doc",
                Profile::Full => "full",
            },
        },
        untrusted: true,
        untrusted_fields: vec!["outline[].text", "body[].text", "tables[].grid"],
        outline: outline_tree,
        body,
        tables: profile.wants_tables().then_some(tables),
    })
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

fn convert_node(n: &outline::Node) -> OutlineNode {
    OutlineNode {
        section: n.section,
        para: n.para,
        level: n.level,
        marker: n.marker.clone(),
        text: n.text.clone(),
        note: n.is_note,
        children: n.children.iter().map(convert_node).collect(),
    }
}

/// 평평한 cells 를 2D 격자로 펼친다. 병합된 자리는 비우고 목록으로 따로 적는다.
fn convert_table(t: &crate::rhwp::RawTable) -> Table {
    let rows = t.rows.max(t.cells.iter().map(|c| c.row + c.row_span).max().unwrap_or(0));
    let cols = t.cols.max(t.cells.iter().map(|c| c.col + c.col_span).max().unwrap_or(0));
    let mut grid = vec![vec![String::new(); cols]; rows];
    let mut merges = Vec::new();
    let mut header_row = false;

    for c in &t.cells {
        if c.row < rows && c.col < cols {
            grid[c.row][c.col] = c.text.clone();
        }
        if c.row_span > 1 || c.col_span > 1 {
            merges.push(Merge {
                row: c.row,
                col: c.col,
                row_span: c.row_span,
                col_span: c.col_span,
            });
        }
        if c.is_header && c.row == 0 {
            header_row = true;
        }
    }

    Table {
        id: format!("t{}", t.index),
        section: t.section,
        para: t.paragraph,
        control: t.control,
        rows,
        cols,
        grid,
        merges,
        header_row,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhwp::{RawCell, RawTable};

    fn cell(row: usize, col: usize, text: &str) -> RawCell {
        RawCell {
            row,
            col,
            row_span: 1,
            col_span: 1,
            is_header: false,
            text: text.into(),
        }
    }

    #[test]
    fn profiles_parse_and_nothing_else_does() {
        assert_eq!(Profile::parse("doc"), Some(Profile::Doc));
        assert_eq!(Profile::parse("outline"), Some(Profile::Outline));
        assert_eq!(Profile::parse("full"), Some(Profile::Full));
        assert_eq!(Profile::parse("everything"), None);
    }

    #[test]
    fn flat_cells_become_a_grid_with_merges_listed() {
        let raw = RawTable {
            index: 3,
            section: 0,
            paragraph: 17,
            control: 0,
            rows: 2,
            cols: 3,
            cells: vec![
                RawCell { row_span: 2, ..cell(0, 0, "기 관") },
                cell(0, 1, "제출처"),
                cell(0, 2, "문의처"),
                cell(1, 1, "iris.go.kr"),
                cell(1, 2, "061-338-9756"),
            ],
        };
        let t = convert_table(&raw);
        assert_eq!(t.id, "t3");
        assert_eq!((t.section, t.para), (0, 17));
        assert_eq!(t.grid[0], vec!["기 관", "제출처", "문의처"]);
        // 병합된 자리는 비어 있고, 왜 비었는지는 merges 가 말한다
        assert_eq!(t.grid[1][0], "");
        assert_eq!(t.merges.len(), 1);
        assert_eq!((t.merges[0].row, t.merges[0].row_span), (0, 2));
    }

    #[test]
    fn a_table_wider_than_declared_still_fits() {
        // 선언된 cols 보다 실제 셀이 더 바깥에 있으면 격자를 넓힌다 —
        // 잘라내면 내용을 잃는다.
        let raw = RawTable {
            index: 0,
            section: 0,
            paragraph: 0,
            control: 0,
            rows: 1,
            cols: 1,
            cells: vec![cell(0, 0, "가"), cell(0, 2, "다")],
        };
        let t = convert_table(&raw);
        assert_eq!(t.cols, 3);
        assert_eq!(t.grid[0], vec!["가", "", "다"]);
    }

    #[test]
    fn untrusted_marking_is_always_present() {
        // 승계한 관례다 — 빠뜨리면 문서 본문이 지시로 읽힐 수 있다.
        let json = serde_json::to_string(&DocJson {
            schema_version: SCHEMA_VERSION,
            source: Source {
                path: "x.hwp".into(),
                format: "hwp5".into(),
                pages: 1,
                paragraphs: 0,
                section_paragraphs: None,
                profile: "doc",
            },
            untrusted: true,
            untrusted_fields: vec!["body[].text"],
            outline: None,
            body: None,
            tables: None,
        })
        .unwrap();
        assert!(json.contains("\"untrusted\":true"));
        assert!(json.contains("untrustedFields"));
        // 비어 있는 축은 아예 내보내지 않는다 — 토큰을 아끼는 쪽이 기본이다
        assert!(!json.contains("\"outline\""));
    }
}
