//! 편집 기록과 **자가검증 저장**.
//!
//! PLAN.md §2-6 과 §8-3. 두 가지를 지킨다.
//!
//! **쓰기는 항상 검증된 경로로 간다.** 타이핑은 메모리에 좌표 연산으로 쌓고,
//! 저장할 때 rhwp 에 일괄 재생한다. 우리는 바이트를 만들지 않는다.
//!
//! **검증자는 작성자와 달라야 한다.** rhwp 가 쓴 결과를 rhwp 로 확인하면 같은
//! 버그를 두 번 통과한다. 그래서 저장 경로는 이렇게 생겼다:
//!
//! 1. 메모리에서 "이렇게 되어야 한다"를 계산한다 (연산을 글자에 그대로 적용)
//! 2. rhwp 에 연산을 재생해 임시 파일을 만든다
//! 3. 임시 파일을 **독립 증인**([`crate::witness`])으로 읽는다
//! 4. 1 과 3 을 맞춰 본다 — 본문, 부품 목록, 단락 수
//! 5. 맞을 때만 원본 자리로 옮기고, 원본은 `.bak` 으로 남긴다
//! 6. 틀리면 **저장하지 않고 무엇이 달라졌는지 말한다**
//!
//! 레퍼런스 렌더러가 없다는 제약이 이 설계를 강제했다(§0). 우리 증인이 유일한
//! 검증자이니 그 증인을 저장 경로에 세운다.

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::offset;
use crate::rhwp::Rhwp;
use crate::witness::Testimony;

/// 좌표 하나 — rhwp 의 `--section` / `--para`.
pub type Coord = (u32, u32);

/// 편집 연산 하나. rhwp `edit` 하위명령에 일대일로 대응한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// `edit insert-text --section --para --offset --text`
    InsertText { at: Coord, offset: usize, text: String },
    /// `edit delete-text --section --para --offset --count`
    DeleteText { at: Coord, offset: usize, count: usize },
    /// `edit insert-paragraph --section --para` (앞 문단 서식 상속)
    InsertParagraph { at: Coord },
    /// `edit delete-paragraph --section --para`
    DeleteParagraph { at: Coord },
    /// `edit set-cell --table --row --col --text` — 표 양식 채우기
    /// `edit set-cell` — 표 양식 채우기. **문단 목록**을 받는다.
    ///
    /// 한 줄이 아니라 목록인 이유: 사용자 보고(2026-10-29) — "`set-cell` 이 한
    /// 줄만 받아서 문단이 여러 개인 칸은 기계가 채울 방법이 없습니다. 한국
    /// 서식에서 서술형 칸은 거의 다 여러 문단이라 서식 채우기의 큰 구멍입니다.
    /// 제가 XML 을 직접 고쳐 우회했는데, **그 경로는 자가검증을 타지 않습니다.**"
    ///
    /// 마지막 문장이 이 변경의 이유다. 도구의 구멍이 사용자를 검증 없는
    /// 경로로 밀어냈다. 실측으로 코퍼스의 글자 있는 칸 1,517개 중 **128개
    /// (8%)가 여러 문단**이고 전부 서술형 칸이다.
    ///
    /// 백엔드의 `set-cell --text` 는 줄바꿈을 **거부한다**(한 줄 값만 받는다).
    /// 그래서 한 연산이 백엔드 명령 여러 개로 펴진다 — `args` 가 명령 목록을
    /// 돌려주는 이유이고, `set-cell` 하나 뒤에 문단마다
    /// `split-paragraph-in-cell` + `insert-text-in-cell` 이 붙는다.
    SetCell { table: u32, row: usize, col: usize, lines: Vec<String> },
    /// `edit insert-table --rows --cols --section --para` — **빈 표 만들기**
    ///
    /// 백엔드는 표와 **함께 단락 하나를 넣는다**(실측: 65→66단락). 그래서 뒤쪽
    /// 단락 좌표가 밀리고, **표 번호도 밀린다** — 새 표가 그 자리의 번호를
    /// 가져가고 뒤의 표들이 +1 된다(s16.hwp 단락3 에 넣으니 t3 이 새 표가 되고
    /// 옛 t3·t4·t5 가 t4·t5·t6 이 되었다).
    ///
    /// `index` 는 **백엔드에 보내지 않는다.** 새 표가 받을 번호를 부르는 쪽이
    /// 미리 세어 적는 칸이다 — 그걸로 "이렇게 되어야 한다" 를 만들고 저장 뒤
    /// 증인과 대조한다. 잘못 세면 셀이 안 맞아 **저장이 멈춘다.**
    InsertTable { at: Coord, rows: usize, cols: usize, index: u32 },
    /// `edit insert-text-in-cell` — **칸을 통째로 덮지 않고** 그 안 문단에 끼운다.
    ///
    /// 사용자 보고(2026-10-29): "`set-cell` 은 통째 덮어쓰기뿐입니다. 한 글자만
    /// 고치려 해도 칸 전체를 다시 써야 합니다." 원문을 베껴 적어야 하므로
    /// **오타가 들어갈 자리**가 된다 — 체크박스 하나 바꾸는 일도 그랬다.
    ///
    /// 좌표는 이미 읽을 수 있다: `grid[행][열]` 이 칸 안 문단을 `\n` 으로 이어
    /// 주므로 쪼개면 `cell_para`, 그 조각 안에서 글자를 세면 `offset` 이다.
    InsertTextInCell { table: u32, row: usize, col: usize, cell_para: usize, offset: usize, text: String },
    /// `edit delete-text-in-cell` — 칸 안 문단에서 글자를 지운다.
    DeleteTextInCell { table: u32, row: usize, col: usize, cell_para: usize, offset: usize, count: usize },
    /// `edit delete-table` — 표를 지운다. **단락은 남는다.**
    ///
    /// 사용자 보고(2026-10-29): 다른 세션이 HWPX 의 zip 을 풀어 XML 을 직접
    /// 고쳐 양식을 채웠고, 거기서 "작성방법 쪽·안 쓰는 칸·첨부 1 을 문단째
    /// 지웠다". 우리는 **개체 달린 단락 삭제를 거부**해서 그 일을 할 수 없었고,
    /// 그래서 사용자가 자가검증이 **아예 없는** 경로로 밀려났다. 세 번째였다
    /// (`set-cell` 한 줄 제한 → XML, 칸 문단 좌표 없음 → XML, 이번).
    ///
    /// 거부한 이유는 "개체를 만들 수 없으니 그렇게 되어야 한다 를 쓸 수 없다"
    /// 였는데, 재 보니 **표는 쓸 수 있다.** 실측(s03.hwpx, 표 13개):
    ///
    /// - 본문 단락 수 **60 → 60** — 표가 올라앉은 단락은 **남는다**(빈 단락으로)
    /// - 표 13 → 12, 뒤쪽 번호가 당겨진다 (t3 → t2 …)
    /// - 남은 표의 단락 좌표는 **그대로** (0:15 는 0:15)
    /// - 단락 글자는 하나도 안 바뀐다
    ///
    /// **처음엔 단락도 같이 빠지는 줄 알았다.** `read:["text"]` 의 `body` 가
    /// 73 → 72 로 줄어든 것을 보고 그렇게 읽었는데, 그 하나는 단락이 아니라
    /// `body` 안의 **표 항목**이었다. 틀린 기대값으로 저장을 걸었고
    /// **자가검증이 멈춰 세웠다**("기대 59 / 실제 60"). 파일은 안 바뀌었다 —
    /// 이 장치가 막으려는 것이 정확히 이 종류의 틀림이다.
    ///
    /// `at` 은 그 표가 올라앉은 단락이다. 증인은 표의 앵커를 모르므로
    /// (백엔드가 말해 준다) 부르는 쪽이 넣어 준다 — `InsertTable` 과 같다.
    DeleteTable { at: Coord, table: u32 },
    /// `edit delete-control` — 단락이 담은 **개체 하나만** 지운다. 단락은 남는다.
    ///
    /// 실측: 그림 하나 든 단락에 걸면 단락 수는 그대로고 그림만 빠진다
    /// (s00.hwp 단락 54, 단락 196→196 · 그림 6→5). 그래서 **개체를 먼저 빼고
    /// 빈 단락을 평범하게 지우는** 두 걸음이 된다 — 두 걸음 다 검증을 지난다.
    ///
    /// `ctrl` 은 그 단락 안에서의 개체 번호(0부터)다. 개체가 **하나뿐인** 단락은
    /// 0 이 유일한 답이라 안전하고, 여럿인 단락은 부르는 쪽이 번호를 알아야
    /// 한다 — 표·그림·글상자가 한 줄에 섞여 번호를 공유하기 때문이다.
    ///
    /// `kept` 는 지운 뒤 그 단락에 남아야 하는 개체 수다. 부르는 쪽이 세어
    /// 적고, 틀리면 저장이 멈춘다 — `InsertTable` 의 `index` 와 같은 자리다.
    DeleteControl { at: Coord, ctrl: usize, kept: usize },
}

/// 표 셀 좌표 — rhwp `--table/--row/--col`.
pub type CellAt = (u32, usize, usize);

impl Op {
    /// 이 연산을 백엔드 명령 **목록**으로 옮긴다.
    ///
    /// 대부분 한 개다. `SetCell` 만 문단 수에 따라 늘어난다 — 백엔드가 한 줄
    /// 값만 받으므로 여러 문단은 쪼개고 채우기를 되풀이해야 한다. 중간 산출물
    /// 이름은 부르는 쪽이 정한다(사슬로 이어 붙이므로).
    fn commands(&self, input: &str, output: &str) -> Vec<Vec<String>> {
        match self {
            Op::SetCell { table, row, col, lines } => {
                let t = table.to_string();
                let r = row.to_string();
                let c = col.to_string();
                let first = lines.first().cloned().unwrap_or_default();
                // 1) 칸을 **통째로** 첫 문단으로 바꾼다. `set-cell` 이 여러
                //    문단 칸을 한 문단으로 접는 것을 실측으로 확인했다.
                let mut out = vec![vec![
                    "edit".into(), "set-cell".into(), input.into(),
                    "--table".into(), t.clone(), "--row".into(), r.clone(),
                    "--col".into(), c.clone(), "--text".into(), first,
                    "-o".into(), output.into(),
                ]];
                // 2) 문단마다 끝에서 쪼개고 채운다. `--offset` 은 **글자 수**다
                //    (실측: "첫 문단" 에 offset 3 → "첫 문" / "단").
                for i in 1..lines.len() {
                    let prev_len = lines[i - 1].chars().count().to_string();
                    out.push(vec![
                        "edit".into(), "split-paragraph-in-cell".into(), input.into(),
                        "--table".into(), t.clone(), "--row".into(), r.clone(),
                        "--col".into(), c.clone(),
                        "--cell-para".into(), (i - 1).to_string(),
                        "--offset".into(), prev_len,
                        "-o".into(), output.into(),
                    ]);
                    out.push(vec![
                        "edit".into(), "insert-text-in-cell".into(), input.into(),
                        "--table".into(), t.clone(), "--row".into(), r.clone(),
                        "--col".into(), c.clone(),
                        "--cell-para".into(), i.to_string(),
                        "--offset".into(), "0".into(),
                        "--text".into(), lines[i].clone(),
                        "-o".into(), output.into(),
                    ]);
                }
                out
            }
            other => vec![other.single_command(input, output)],
        }
    }

    fn single_command(&self, input: &str, output: &str) -> Vec<String> {
        let s = |n: u32| n.to_string();
        match self {
            Op::InsertText { at, offset, text } => vec![
                "edit".into(), "insert-text".into(), input.into(),
                "--section".into(), s(at.0), "--para".into(), s(at.1),
                "--offset".into(), offset.to_string(),
                "--text".into(), text.clone(),
                "-o".into(), output.into(),
            ],
            Op::DeleteText { at, offset, count } => vec![
                "edit".into(), "delete-text".into(), input.into(),
                "--section".into(), s(at.0), "--para".into(), s(at.1),
                "--offset".into(), offset.to_string(),
                "--count".into(), count.to_string(),
                "-o".into(), output.into(),
            ],
            Op::InsertParagraph { at } => vec![
                "edit".into(), "insert-paragraph".into(), input.into(),
                "--section".into(), s(at.0), "--para".into(), s(at.1),
                "-o".into(), output.into(),
            ],
            Op::DeleteParagraph { at } => vec![
                "edit".into(), "delete-paragraph".into(), input.into(),
                "--section".into(), s(at.0), "--para".into(), s(at.1),
                "-o".into(), output.into(),
            ],
            // `commands` 가 먼저 가로챈다 — 여러 문단이면 명령이 여럿이라
            // 한 개를 돌려주는 이 자리로 올 수 없다.
            Op::SetCell { .. } => unreachable!("SetCell 은 commands 가 처리한다"),
            Op::InsertTextInCell { table, row, col, cell_para, offset, text } => vec![
                "edit".into(), "insert-text-in-cell".into(), input.into(),
                "--table".into(), table.to_string(),
                "--row".into(), row.to_string(),
                "--col".into(), col.to_string(),
                "--cell-para".into(), cell_para.to_string(),
                "--offset".into(), offset.to_string(),
                "--text".into(), text.clone(),
                "-o".into(), output.into(),
            ],
            Op::DeleteTextInCell { table, row, col, cell_para, offset, count } => vec![
                "edit".into(), "delete-text-in-cell".into(), input.into(),
                "--table".into(), table.to_string(),
                "--row".into(), row.to_string(),
                "--col".into(), col.to_string(),
                "--cell-para".into(), cell_para.to_string(),
                "--offset".into(), offset.to_string(),
                "--count".into(), count.to_string(),
                "-o".into(), output.into(),
            ],
            Op::DeleteTable { table, .. } => vec![
                "edit".into(), "delete-table".into(), input.into(),
                "--table".into(), table.to_string(),
                "-o".into(), output.into(),
            ],
            Op::DeleteControl { at, ctrl, .. } => vec![
                "edit".into(), "delete-control".into(), input.into(),
                "--section".into(), s(at.0), "--para".into(), s(at.1),
                "--ctrl".into(), ctrl.to_string(),
                "-o".into(), output.into(),
            ],
            Op::InsertTable { at, rows, cols, .. } => vec![
                "edit".into(), "insert-table".into(), input.into(),
                "--rows".into(), rows.to_string(),
                "--cols".into(), cols.to_string(),
                "--section".into(), s(at.0), "--para".into(), s(at.1),
                "-o".into(), output.into(),
            ],
        }
    }

    /// 사람에게 보여 줄 한 줄.
    pub fn describe(&self) -> String {
        match self {
            Op::InsertText { at, offset, text } => {
                format!("구역{} 단락{} 의 {}번째에 {:?} 넣기", at.0, at.1, offset, text)
            }
            Op::DeleteText { at, offset, count } => {
                format!("구역{} 단락{} 의 {}번째부터 {}자 지우기", at.0, at.1, offset, count)
            }
            Op::InsertParagraph { at } => format!("구역{} 단락{} 앞에 빈 단락", at.0, at.1),
            Op::DeleteParagraph { at } => format!("구역{} 단락{} 삭제", at.0, at.1),
            Op::SetCell { table, row, col, lines } => match lines.len() {
                0 | 1 => format!(
                    "표{table} ({row},{col}) 에 {:?} 기록",
                    lines.first().map(String::as_str).unwrap_or("")
                ),
                n => format!(
                    "표{table} ({row},{col}) 에 {n}문단 기록 (첫 문단 {:?})",
                    lines[0]
                ),
            },
            Op::DeleteTable { at, table } => {
                format!("구역{} 단락{} 의 표{table} 지우기 (단락은 빈 채로 남습니다)", at.0, at.1)
            }
            Op::DeleteControl { at, ctrl, kept } => format!(
                "구역{} 단락{} 의 개체 {ctrl}번 지우기 (단락은 남고 개체 {kept}개)",
                at.0, at.1
            ),
            Op::InsertTextInCell { table, row, col, cell_para, offset, text } => format!(
                "표{table} ({row},{col}) 문단{cell_para} 의 {offset}번째에 {text:?} 넣기"
            ),
            Op::DeleteTextInCell { table, row, col, cell_para, offset, count } => format!(
                "표{table} ({row},{col}) 문단{cell_para} 의 {offset}번째부터 {count}자 지우기"
            ),
            Op::InsertTable { at, rows, cols, index } => {
                format!("구역{} 단락{} 에 {rows}×{cols} 표 만들기 (표{index} 가 됩니다)", at.0, at.1)
            }
        }
    }
}

/// `.bak` 경로를 고른다. **이미 있으면 덮지 않고 비켜 간다.**
///
/// 전에는 `<이름>.<확장자>.bak` 고정이라 저장마다 덮였다 — 두 번 고친 문서는
/// 원본이 영구히 사라졌고(실측: 1회 후 `.bak`=원본, 2회 후 `.bak`=1차
/// 수정본) 보고 문구는 두 번 다 똑같이 "백업 …" 이었다. `.bak` 은 저장이
/// 깨졌을 때의 **유일한 복구 수단**이므로 세대를 남긴다.
///
/// 확장자가 없는 파일에 `"hwp"` 를 가정하던 것도 고쳤다 — 같은 폴더의
/// `문서` 와 `문서.hwp` 가 같은 `.bak` 을 다퉜다.
pub fn backup_path(path: &Path) -> std::path::PathBuf {
    let base = match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => path.with_extension(format!("{ext}.bak")),
        // 확장자가 없으면 이름 뒤에 붙인다. 지어내지 않는다.
        None => {
            let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            path.with_file_name(format!("{name}.bak"))
        }
    };
    if !base.exists() {
        return base;
    }
    let s = base.to_string_lossy().to_string();
    for n in 1..1000 {
        let cand = std::path::PathBuf::from(format!("{s}.{n}"));
        if !cand.exists() {
            return cand;
        }
    }
    base
}

/// 두 경로가 **같은 파일인가.** 쓰기 전에 반드시 이걸로 본다.
///
/// `canonicalize()` 비교로는 **하드링크를 못 본다** — 두 이름이 같은 inode 인데
/// 서로 다른 경로로 정규화되므로 "다른 파일" 로 떨어진다. 그 틈으로
/// `fs::copy(자기, 자기)` 가 원본을 `O_TRUNC` 로 자른 뒤 0바이트를 복사해
/// **문서를 통째로 날렸다**(실측 2026-10-13: `:w! <하드링크>` → 0바이트, 백업
/// 없음). `rsync --link-dest`·`cp -al` 스냅샷 트리에서 흔한 모양이다.
///
/// 그래서 `dev`+`ino` 로 본다. 사람 쪽과 기계 쪽이 **같은 함수**를 써야 한다 —
/// 따로 적으면 한쪽이 옛 판정에 남고, 남은 쪽으로 문서가 사라진다.
pub fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        // 한쪽이 없으면 같을 수 없다
        _ => false,
    }
}

/// 쌓인 편집. 아직 디스크에 가지 않았다.
#[derive(Debug, Clone, Default)]
pub struct EditLog {
    ops: Vec<Op>,
}

impl EditLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, op: Op) {
        self.ops.push(op);
    }

    pub fn undo(&mut self) -> Option<Op> {
        self.ops.pop()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    pub fn clear(&mut self) {
        self.ops.clear();
    }
}

/// 저장이 어떻게 끝났는지.
#[derive(Debug)]
pub struct SaveReport {
    pub path: PathBuf,
    pub backup: Option<PathBuf>,
    pub ops_applied: usize,
    /// 증인이 확인한 본문 단락 수
    pub paragraphs: usize,
    /// 증인이 확인한 표 셀 수
    pub cells: usize,
}

/// 그리기개체(표·그림·글상자). 구역정의(2)·쪽번호(21)·머리말(16)은 **본문에
/// 보일 내용이 아니고** rhwp 가 재배치할 수 있다 — 11 만 사람이 볼 것이고,
/// 우리가 지켜야 할 것이다. 증인 쪽과 같은 상수를 쓴다.
use crate::witness::CTRL_DRAWING_OR_TABLE as DRAWING;

/// 비교의 단위 — 본문 단락, 표 셀, 그리고 **단락이 든 그리기개체 수**.
///
/// 컨트롤은 글자 비교에서 뺀다. rhwp 가 컨트롤을 재배치할 수 있고(실측:
/// `--offset 0` 이 선두 컨트롤 뒤에 넣는다) 우리는 컨트롤을 만들거나 옮기지
/// 않으므로, 글자가 맞는지만 보는 것이 옳은 질문이다.
///
/// **개체 수는 다르다 — 세지 않으면 조용히 사라진다.** 실측(2026-10-10):
/// s00.hwp 의 단락 54(그림 하나가 달려 있다)를 `delete-para` 로 지우면 저장이
/// 성공으로 끝나고 그림이 6개에서 5개가 되었다. 글자와 셀만 비교하니 자가검증이
/// 볼 수 없는 손실이었다. 그래서 개체 수를 비교 단위에 넣는다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub paras: BTreeMap<Coord, String>,
    pub cells: BTreeMap<CellAt, String>,
    /// 이 단락에 달린 그리기개체(표·그림·글상자)의 수. 0 인 단락은 넣지 않는다.
    pub drawings: BTreeMap<Coord, usize>,
}

/// 지금 문서의 비교 단위를 뜬다. 저장 경로와 `hwp api` 가 **같은 것**을 본다 —
/// 좌표 셈을 두 군데 두면 반드시 어긋난다.
pub fn snapshot(t: &Testimony) -> Snapshot {
    let paras = t
        .body_paragraphs()
        .into_iter()
        .filter_map(|p| p.index.map(|i| ((p.section, i), offset::visible(&p.text))))
        .collect();
    let cells = t
        .tables
        .iter()
        .flat_map(|tb| {
            tb.cells
                .iter()
                .map(move |c| ((tb.index, c.row, c.col), c.text.clone()))
        })
        .collect();
    let drawings = t
        .body_paragraphs()
        .into_iter()
        .filter_map(|p| {
            let i = p.index?;
            let n = p.objects.iter().filter(|&&o| o == DRAWING).count();
            (n > 0).then_some(((p.section, i), n))
        })
        .collect();
    Snapshot { paras, cells, drawings }
}

/// **다른 이름으로 쓸 때 형식이 같은지 본다.**
///
/// 백엔드는 **확장자로 형식을 정한다.** `.hwpx` 를 `.hwp` 로 내보내면 조용히
/// **변환**이 일어난다 — 실측(2026-10-10): ZIP 부품 12개가 사라지고 CFB 스트림
/// 11개가 생겼다. 자가검증이 "컨테이너 부품이 바뀌었다" 로 막아 주지만, 애초에
/// 물어볼 일이다.
///
/// 사람 쪽(`:w <경로>`)과 기계 쪽(`api` 의 `output`)이 **같은 함수**를 쓴다.
/// 둘에 따로 적으면 한쪽이 빠지고, 빠진 쪽이 변환을 통과시킨다.
pub fn check_same_format(src: &Path, dest: &Path) -> Result<()> {
    let ext = |p: &Path| p.extension().map(|e| e.to_string_lossy().to_lowercase());
    if ext(src) != ext(dest) {
        bail!(
            "확장자가 원본과 다르다 ({} → {}). 백엔드는 확장자로 형식을 정하므로 \
             이것은 변환이다 — 변환은 `rhwp convert` 로 따로 하라.",
            src.display(),
            dest.display()
        );
    }
    Ok(())
}

/// 원본을 건드리지 않고 **다른 이름으로** 쓴다.
///
/// 원본을 복사해 두고 그 복사본에 연산을 재생한다 — 연산은 *디스크의 원본*
/// 상태를 기준으로 쌓였으므로 그 위에서 재생해야 맞는다. 검증은 평소와 같다:
/// 통과하지 못하면 **아무것도 남기지 않는다.**
pub fn save_as(rhwp: &Rhwp, src: &Path, dest: &Path, log: &EditLog) -> Result<SaveReport> {
    check_same_format(src, dest)?;

    // **같은 파일이면 거부한다.** 하드링크 목적지에서 `fs::copy(자기, 자기)` 가
    // 원본을 `O_TRUNC` 로 자른 뒤 0바이트를 복사해 문서를 날렸다(실측).
    if same_file(src, dest) {
        bail!(
            "{} 와 {} 는 같은 파일입니다 — 다른 이름을 주거나 제자리 저장(`:w`)을 하세요",
            src.display(),
            dest.display()
        );
    }
    if let Some(dir) = dest.parent() {
        if !dir.as_os_str().is_empty() && !dir.is_dir() {
            bail!("{} 폴더가 없습니다", dir.display());
        }
    }

    // **목적지를 먼저 덮지 않는다.**
    //
    // 전에는 `fs::copy(src, dest)` 로 목적지를 원본으로 덮고 그 위에서 검증을
    // 돌렸다. 그래서 검증·백업·쓰기 중 **어디서 실패해도 목적지 문서는 이미
    // 파괴된 상태**였고, 실패 경로는 그것을 복원하지 않고 지웠다 — 실측:
    // 쓰기 금지 폴더에 `:saveas!` 하니 1.6MB 짜리 다른 문서가 사라지고 도구는
    // "쓰지 못했습니다" 라고 말했다.
    //
    // 지금은 **목적지와 같은 폴더의 임시본**에서 다 끝내고 `rename` 으로
    // 들여놓는다. 같은 폴더여야 `rename` 이 원자적이다(파일시스템이 같으므로).
    // 실패하면 임시본만 지우고 목적지는 손대지 않는다.
    let tmp = {
        let stem = dest.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let ext = dest.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
        let dir = dest.parent().unwrap_or(Path::new("."));
        dir.join(format!(".{stem}.hwp-cmd-{}.{ext}", std::process::id()))
    };
    let _guard = TempFile(tmp.clone());
    std::fs::copy(src, &tmp)
        .with_context(|| format!("{} 로 복사할 수 없습니다", tmp.display()))?;

    // **고친 것이 없으면 그냥 사본이다.** `save` 는 "저장할 편집이 없다" 로
    // 거부하는데, 그건 제자리 저장에 맞는 말이고 여기서는 틀렸다 — 사용자가
    // 부탁한 것은 "이 문서를 다른 이름으로" 이고, 바이트가 같으므로 검증할
    // 것도 없다. 처음에 거부했고, 파일이 안 생기는 것으로 드러났다.
    let report = if log.is_empty() {
        // **고친 것이 없으면 그냥 사본이다.** `save` 는 "저장할 편집이 없다" 로
        // 거부하는데, 그건 제자리 저장에 맞는 말이고 여기서는 틀렸다 — 사용자가
        // 부탁한 것은 "이 문서를 다른 이름으로" 이고, 바이트가 같으므로 검증할
        // 것도 없다. 처음에 거부했고, 파일이 안 생기는 것으로 드러났다.
        let witness = Testimony::read(&tmp)
            .with_context(|| format!("{} 를 다시 읽을 수 없습니다", tmp.display()))?;
        let snap = snapshot(&witness);
        SaveReport {
            path: dest.to_path_buf(),
            backup: None,
            ops_applied: 0,
            paragraphs: snap.paras.len(),
            cells: snap.cells.len(),
        }
    } else {
        // 임시본에서 평소의 자가검증 저장을 돌린다. 실패하면 `_guard` 가
        // 임시본을 치우고, **목적지는 끝까지 손대지 않은 상태**로 남는다.
        let mut r = save(rhwp, &tmp, log)?;
        // `save` 가 임시본의 백업을 만들었다 — 임시본의 백업은 뜻이 없다
        if let Some(b) = r.backup.take() {
            let _ = std::fs::remove_file(b);
        }
        r.path = dest.to_path_buf();
        r
    };

    // **목적지가 이미 있으면 그 내용을 먼저 비켜 둔다.** 덮어도 된다는 판단은
    // 부르는 쪽이 이미 했고(사람 쪽 `!`, 기계 쪽 `overwrite`), 그래도 사라지는
    // 것은 목적지의 옛 내용이므로 돌아갈 자리를 남긴다. 전에는 `.bak` 에
    // **원본 쪽**이 들어가서 되돌리면 전혀 다른 문서가 나왔다(실측).
    let mut report = report;
    if dest.exists() {
        let bak = backup_path(dest);
        std::fs::rename(dest, &bak)
            .with_context(|| format!("{} 을 비켜 둘 수 없습니다", dest.display()))?;
        report.backup = Some(bak);
    }
    std::fs::rename(&tmp, dest)
        .with_context(|| format!("{} 로 들여놓을 수 없습니다", dest.display()))?;
    std::mem::forget(_guard); // 들여놨으니 치울 임시본이 없다
    Ok(report)
}

/// 떨어질 때 파일을 치운다. 실패 경로마다 `remove_file` 을 적으면 하나를
/// 빠뜨리고, 빠뜨린 그 경로가 반쯤 쓴 파일을 남긴다.
struct TempFile(std::path::PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 연산들을 메모리의 본문에 적용해 "이렇게 되어야 한다" 를 만든다.
///
/// 칸 하나를 빌려 **그 안의 문단 목록**으로 쪼갠다.
///
/// 빌려 온 칸 자리와 쪼갠 목록을 함께 돌려주므로, 고친 뒤 `join("\n")` 해서
/// 그 자리에 되돌려 놓으면 된다.
fn cell_paras<'a>(
    cur: &'a mut Snapshot,
    table: u32,
    row: usize,
    col: usize,
    verb: &str,
) -> Result<(&'a mut String, Vec<String>)> {
    let slot = cur
        .cells
        .get_mut(&(table, row, col))
        .with_context(|| format!("없는 셀에 {verb} 합니다: 표{table} ({row},{col})"))?;
    let paras = slot.split('\n').map(String::from).collect();
    Ok((slot, paras))
}

/// 단락을 넣고 빼는 연산은 뒤쪽 좌표를 밀고 당긴다 — 그 셈을 여기서 한다.
pub fn expected(before: &Snapshot, ops: &[Op]) -> Result<Snapshot> {
    let mut cur = before.clone();
    for op in ops {
        match op {
            Op::SetCell { table, row, col, lines } => {
                let key = (*table, *row, *col);
                // 증인은 칸 안 문단을 `\n` 으로 이어 읽는다. 그래서 기대값도
                // 그렇게 이으면 **자가검증이 공짜로 따라온다** — 여러 문단을
                // 쓰는 길이 생겼는데 검증은 손댈 데가 없다.
                let text = &lines.join("\n");
                let slot = cur
                    .cells
                    .get_mut(&key)
                    .with_context(|| format!("없는 셀에 쓰려 합니다: 표{table} ({row},{col})"))?;
                *slot = text.clone();
            }
            Op::InsertText { at, offset, text } => {
                let s = cur
                    .paras
                    .get_mut(at)
                    .with_context(|| format!("없는 좌표에 넣으려 합니다: {at:?}"))?;
                let chars: Vec<char> = s.chars().collect();
                if *offset > chars.len() {
                    bail!("구역{} 단락{}: 오프셋 {offset} 이 길이 {} 를 넘습니다", at.0, at.1, chars.len());
                }
                let mut next: String = chars[..*offset].iter().collect();
                next.push_str(text);
                next.extend(&chars[*offset..]);
                *s = next;
            }
            Op::DeleteText { at, offset, count } => {
                let s = cur
                    .paras
                    .get_mut(at)
                    .with_context(|| format!("없는 좌표에서 지우려 합니다: {at:?}"))?;
                let chars: Vec<char> = s.chars().collect();
                let end = offset.saturating_add(*count);
                if end > chars.len() {
                    bail!(
                        "구역{} 단락{}: {offset}부터 {count}자는 길이 {} 를 넘습니다",
                        at.0, at.1, chars.len()
                    );
                }
                let mut next: String = chars[..*offset].iter().collect();
                next.extend(&chars[end..]);
                *s = next;
            }
            Op::InsertParagraph { at } => {
                cur.paras = shift(cur.paras, at.0, at.1, 1);
                cur.drawings = shift(cur.drawings, at.0, at.1, 1);
                cur.paras.insert(*at, String::new());
            }
            Op::InsertTextInCell { table, row, col, cell_para, offset, text } => {
                // **칸 글자는 문단을 `\n` 으로 이어 둔 것**이다 — 증인이 그렇게
                // 읽는다. 그러니 쪼개 그 문단에만 적용하고 다시 이으면
                // 기대값이 선다. SetCell 과 같은 이유로 **검증은 손댈 데가 없다.**
                let (slot, mut paras) = cell_paras(&mut cur, *table, *row, *col, "넣으려")?;
                let p = paras.get_mut(*cell_para).with_context(|| {
                    format!("표{table} ({row},{col}) 에 문단이 {cell_para} 번째까지 없습니다")
                })?;
                let chars: Vec<char> = p.chars().collect();
                if *offset > chars.len() {
                    bail!(
                        "표{table} ({row},{col}) 문단{cell_para} 은 {}자인데 {offset}번째에 넣을 수 없습니다",
                        chars.len()
                    );
                }
                let mut next: String = chars[..*offset].iter().collect();
                next.push_str(text);
                next.extend(&chars[*offset..]);
                *p = next;
                *slot = paras.join("\n");
            }
            Op::DeleteTextInCell { table, row, col, cell_para, offset, count } => {
                let (slot, mut paras) = cell_paras(&mut cur, *table, *row, *col, "지우려")?;
                let p = paras.get_mut(*cell_para).with_context(|| {
                    format!("표{table} ({row},{col}) 에 문단이 {cell_para} 번째까지 없습니다")
                })?;
                let chars: Vec<char> = p.chars().collect();
                let end = offset.saturating_add(*count);
                if end > chars.len() {
                    bail!(
                        "표{table} ({row},{col}) 문단{cell_para} 은 {}자인데 {offset}번째부터 {count}자를 지울 수 없습니다",
                        chars.len()
                    );
                }
                let mut next: String = chars[..*offset].iter().collect();
                next.extend(&chars[end..]);
                *p = next;
                *slot = paras.join("\n");
            }
            Op::DeleteTable { at, table } => {
                // 표 번호가 당겨지는 것은 `InsertTable` 의 거울이다. 다만
                // **단락은 빠지지 않는다** — 넣을 때는 표와 단락이 함께 들어오고,
                // 지울 때는 표만 나가고 빈 단락이 남는다. 대칭이 아니다.
                // (그 비대칭을 모르고 짰다가 자가검증에 걸렸다. 변종 주석 참고.)
                if !cur.paras.contains_key(at) {
                    bail!("없는 단락입니다: 구역{} 단락{}", at.0, at.1);
                }
                if !cur.cells.keys().any(|(tb, _, _)| tb == table) {
                    bail!("없는 표입니다: 표{table}");
                }
                // 앵커는 부르는 쪽이 넣어 준다(증인은 표의 앵커를 모른다). 그러니
                // **여기서 한 번 본다** — 틀린 앵커로 내려가면 저장은 성공하고
                // 기대값만 어긋나서 "이 단락의 글자가 다르다" 같은 엉뚱한 차이가
                // 쏟아진다. 실제로 그렇게 나온 적이 있다.
                if cur.drawings.get(at).copied().unwrap_or(0) == 0 {
                    bail!(
                        "구역{} 단락{} 에는 그리기개체가 없습니다 — 표{table} 가 올라앉은 \
                         단락이 맞는지 보세요",
                        at.0, at.1
                    );
                }
                // **단락은 남는다** — 좌표를 밀지 않는다. 그 단락이 들고 있던
                // 개체만 하나 줄어든다.
                match cur.drawings.get(at).copied().unwrap_or(0) {
                    0 | 1 => {
                        cur.drawings.remove(at);
                    }
                    n => {
                        cur.drawings.insert(*at, n - 1);
                    }
                }
                cur.cells = std::mem::take(&mut cur.cells)
                    .into_iter()
                    .filter(|((tb, _, _), _)| tb != table)
                    .map(|((tb, r, c), v)| ((if tb > *table { tb - 1 } else { tb }, r, c), v))
                    .collect();
            }
            Op::DeleteControl { at, ctrl, kept } => {
                // **단락은 남는다** (실측: 단락 196→196, 그림 6→5). 글자와 좌표는
                // 손대지 않고 개체 수만 줄인다 — 그러면 그 다음에 빈 단락을
                // 평범한 `DeleteParagraph` 로 지울 수 있고, 두 걸음 다 검증을
                // 지난다. 전에는 개체 달린 단락을 아예 거부해서, 양식의 안 쓰는
                // 칸을 지우려는 사람이 **검증 없는 경로로 밀려났다.**
                let have = cur.drawings.get(at).copied().unwrap_or(0);
                if have == 0 {
                    bail!("구역{} 단락{} 에는 지울 개체가 없습니다", at.0, at.1);
                }
                if *ctrl >= have {
                    bail!(
                        "구역{} 단락{} 에는 개체가 {have}개인데 {ctrl}번을 지우려 합니다 \
                         (번호는 0부터)",
                        at.0, at.1
                    );
                }
                // `kept` 는 부르는 쪽이 **세어 적는 칸**이다 (`InsertTable` 의
                // `index` 와 같은 자리). 틀리면 여기서 멈춘다 — 개체 수는 증인이
                // 다시 세는 값이므로, 적어 둔 수가 틀렸다는 것은 부르는 쪽이
                // 문서를 잘못 보고 있다는 뜻이다.
                if *kept + 1 != have {
                    bail!(
                        "구역{} 단락{} 에는 개체가 {have}개입니다 — 하나 지우면 {}개가 \
                         남는데 {kept}개로 적었습니다",
                        at.0, at.1,
                        have - 1
                    );
                }
                if *kept == 0 {
                    cur.drawings.remove(at);
                } else {
                    cur.drawings.insert(*at, *kept);
                }
            }
            Op::InsertTable { at, rows, cols, index } => {
                // 표와 함께 **단락 하나**가 들어간다
                cur.paras = shift(cur.paras, at.0, at.1, 1);
                cur.drawings = shift(cur.drawings, at.0, at.1, 1);
                cur.paras.insert(*at, String::new());
                // 그 단락이 표를 든다
                cur.drawings.insert(*at, 1);
                // **번호가 밀린다** — 이 자리부터의 표가 +1
                cur.cells = std::mem::take(&mut cur.cells)
                    .into_iter()
                    .map(|((t, r, c), v)| {
                        ((if t >= *index { t + 1 } else { t }, r, c), v)
                    })
                    .collect();
                // 새 표의 빈 칸들
                for r in 0..*rows {
                    for c in 0..*cols {
                        cur.cells.insert((*index, r, c), String::new());
                    }
                }
            }
            Op::DeleteParagraph { at } => {
                // **개체가 달린 단락은 지우지 않는다.**
                //
                // 지우면 그 표·그림·글상자가 문서에서 함께 사라지는데, 우리는
                // 개체를 만들거나 옮기지 못하므로 "그렇게 되어야 한다" 를 쓸
                // 수 없다. 즉 **검증되지 않은 쓰기**다.
                //
                // 전에는 글자와 셀만 비교해서 두 가지로 갈렸다: 그림이 달린
                // 단락은 저장이 **성공하고 그림이 조용히 사라졌고**(s00.hwp
                // 단락 54, 그림 6→5개), 표가 달린 단락은 셀 수가 안 맞아
                // 저장이 멈추는 대신 "표0 (0,0) 의 글자가 다르다" 같은 **엉뚱한
                // 차이**를 쏟아냈다. 둘 다 여기서 막는다 — `expected` 에서
                // 막으면 `dryRun` 도 미리 알 수 있다.
                if let Some(n) = cur.drawings.get(at).copied().filter(|&n| n > 0) {
                    bail!(
                        "구역{} 단락{} 에는 그리기개체(표·그림·글상자)가 {n}개 달려 있어 \
                         단락째 지울 수 없습니다. 개체를 먼저 빼면 됩니다 — \
                         표면 `delete-table`(표와 단락을 함께 지웁니다), 그림·글상자면 \
                         `delete-control` 로 개체를 빼고 남은 빈 단락을 지우세요. \
                         글자만 지우려면 `delete-text` 나 `set-para` 로 빈 글자를 쓰면 됩니다.",
                        at.0,
                        at.1
                    );
                }
                if cur.paras.remove(at).is_none() {
                    bail!("없는 단락을 지우려 합니다: {at:?}");
                }
                cur.paras = shift(cur.paras, at.0, at.1, -1);
                cur.drawings.remove(at);
                cur.drawings = shift(cur.drawings, at.0, at.1, -1);
            }
        }
    }
    Ok(cur)
}

/// 한 구역의 `from` 이후 단락 좌표를 `delta` 만큼 민다.
fn shift<V>(map: BTreeMap<Coord, V>, section: u32, from: u32, delta: i64) -> BTreeMap<Coord, V> {
    map.into_iter()
        .map(|((s, i), v)| {
            if s == section && i >= from {
                ((s, (i as i64 + delta).max(0) as u32), v)
            } else {
                ((s, i), v)
            }
        })
        .collect()
}

/// 연산을 rhwp 에 재생해 새 파일을 만든다. 중간 파일은 임시 디렉터리에 둔다.
fn replay(rhwp: &Rhwp, src: &Path, ops: &[Op], workdir: &Path) -> Result<PathBuf> {
    let ext = src
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("hwp")
        .to_string();
    let mut input = workdir.join(format!("step0.{ext}"));
    std::fs::copy(src, &input).with_context(|| format!("{} 복사 실패", src.display()))?;

    // 연산 하나가 백엔드 명령 **여러 개**로 펴질 수 있다(여러 문단 칸).
    // 그래서 중간 산출물에 명령 번호까지 붙여 사슬로 잇는다.
    let mut step = 0usize;
    for (n, op) in ops.iter().enumerate() {
        let cmds = op.commands(&input.to_string_lossy(), "");
        for (k, _) in cmds.iter().enumerate() {
            step += 1;
            let output = workdir.join(format!("step{step}.{ext}"));
            // 입력·출력은 매 명령마다 다르므로 여기서 다시 만든다
            let cmds = op.commands(&input.to_string_lossy(), &output.to_string_lossy());
            let args = &cmds[k];
            let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            rhwp.run(&refs).with_context(|| {
                format!("{} 번째 연산 실패 ({}/{}) — {}", n + 1, k + 1, cmds.len(), op.describe())
            })?;
            if !output.exists() {
                bail!(
                    "{} 번째 연산이 출력을 내지 않았습니다 ({}/{}) — {}",
                    n + 1,
                    k + 1,
                    cmds.len(),
                    op.describe()
                );
            }
            input = output;
        }
    }
    Ok(input)
}

/// 기대와 실제가 어떻게 다른가. 저장을 멈출 때 사람에게 보여 줄 말.
fn diff_report(
    want: &Snapshot,
    got: &Snapshot,
    parts_before: &[String],
    parts_after: &[String],
) -> Option<String> {
    let mut lines = Vec::new();

    if parts_before != parts_after {
        let added: Vec<_> = parts_after.iter().filter(|p| !parts_before.contains(p)).collect();
        let removed: Vec<_> = parts_before.iter().filter(|p| !parts_after.contains(p)).collect();
        lines.push(format!("컨테이너 부품이 바뀌었습니다 — 추가 {added:?} / 제거 {removed:?}"));
    }
    if want.paras.len() != got.paras.len() {
        lines.push(format!(
            "본문 단락 수: 기대 {} / 실제 {}",
            want.paras.len(),
            got.paras.len()
        ));
    }
    if want.cells.len() != got.cells.len() {
        lines.push(format!(
            "표 셀 수: 기대 {} / 실제 {}",
            want.cells.len(),
            got.cells.len()
        ));
    }
    // **개체가 사라졌는지 본다.** 글자와 셀만 비교하던 동안 그림 삭제가
    // 성공으로 보고되었다 (s00.hwp 단락 54, 6→5개).
    {
        let w: usize = want.drawings.values().sum();
        let g: usize = got.drawings.values().sum();
        if w != g {
            lines.push(format!(
                "그리기개체(표·그림·글상자) 수: 기대 {w} / 실제 {g}"
            ));
        }
        for (k, n) in &want.drawings {
            let got_n = got.drawings.get(k).copied().unwrap_or(0);
            if got_n != *n {
                lines.push(format!(
                    "구역{} 단락{} 의 개체 수가 다릅니다 — 기대 {n} / 실제 {got_n}",
                    k.0, k.1
                ));
                break;
            }
        }
    }
    for (k, w) in &want.cells {
        match got.cells.get(k) {
            None => lines.push(format!("표{} ({},{}) 이 저장본에 없습니다", k.0, k.1, k.2)),
            Some(g) if g != w => lines.push(format!(
                "표{} ({},{}) 의 글자가 다르다\n    기대: {:.60}\n    실제: {:.60}",
                k.0, k.1, k.2, w, g
            )),
            Some(_) => {}
        }
        if lines.len() >= 6 {
            lines.push("…".into());
            return Some(lines.join("\n  "));
        }
    }
    for (k, w) in &want.paras {
        match got.paras.get(k) {
            None => lines.push(format!("구역{} 단락{} 이 저장본에 없습니다", k.0, k.1)),
            Some(g) if g != w => lines.push(format!(
                "구역{} 단락{} 의 본문이 다르다\n    기대: {:.70}\n    실제: {:.70}",
                k.0, k.1, w, g
            )),
            Some(_) => {}
        }
        if lines.len() >= 6 {
            lines.push("…".into());
            break;
        }
    }
    for k in got.paras.keys() {
        if !want.paras.contains_key(k) {
            lines.push(format!("구역{} 단락{} 이 저장본에 더 있습니다", k.0, k.1));
            break;
        }
    }

    (!lines.is_empty()).then(|| lines.join("\n  "))
}

/// 자가검증 저장. 검증을 통과하지 못하면 **원본을 건드리지 않는다**.
pub fn save(rhwp: &Rhwp, path: &Path, log: &EditLog) -> Result<SaveReport> {
    if log.is_empty() {
        bail!("저장할 편집이 없습니다");
    }

    let before = Testimony::read(path)?;
    let want = expected(&snapshot(&before), log.ops())?;

    let workdir = tempdir_for(path)?;
    let result = replay(rhwp, path, log.ops(), &workdir);
    let staged = match result {
        Ok(p) => p,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&workdir);
            return Err(e);
        }
    };

    // 독립 증인으로 읽는다 — 여기가 검증의 본체다.
    let after = Testimony::read(&staged)?;
    let got = snapshot(&after);

    if let Some(report) = diff_report(&want, &got, &before.parts, &after.parts) {
        let _ = std::fs::remove_dir_all(&workdir);
        bail!(
            "저장을 멈췄습니다 — 저장본이 기대와 다릅니다. 원본은 그대로 두었습니다.\n  {report}\n\n  \
             연산 {}개를 재생했고 그 중 어디서 어긋났는지는 위 차이가 말해 줍니다. \
             `--force` 는 없습니다: 무엇이 틀렸는지 모르는 채로 남의 문서를 덮지 않습니다.",
            log.len()
        );
    }

    // 통과. 원본을 `.bak` 으로 남기고 자리를 바꾼다.
    let backup = backup_path(path);
    std::fs::copy(path, &backup)
        .with_context(|| format!("백업을 만들 수 없습니다: {}", backup.display()))?;

    // **자리를 바꾸는 것은 복사가 아니라 `rename` 이어야 한다.**
    //
    // 전에는 `fs::copy(&staged, path)` 였다. 그건 원본을 `O_TRUNC` 로 열어
    // 덮어쓰는 것이라 **원자적이지 않다** — 중간에 끊기거나 디스크가 차면
    // 원본 자리에 0바이트가 남는다(실측: ENOSPC 주입 → `doc.hwp` 0바이트,
    // SIGINT/TERM/KILL 3/3 동일). 그런데 메시지는 "저장 실패" 라고만 해서
    // 사용자는 "원본은 그대로" 를 믿는다.
    //
    // `rename` 은 같은 파일시스템에서만 원자적이므로 **원본과 같은 폴더**에
    // 임시본을 두고 거기서 옮긴다. 작업 디렉터리는 `/tmp` 일 수 있어서
    // 거기서 바로 `rename` 하면 EXDEV 로 깨진다.
    let landing = {
        let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let dir = path.parent().unwrap_or(Path::new("."));
        dir.join(format!(".{name}.hwp-cmd-land-{}", std::process::id()))
    };
    let _land_guard = TempFile(landing.clone());
    std::fs::copy(&staged, &landing)
        .with_context(|| format!("저장 실패: {}", landing.display()))?;
    // **권한을 원본에서 가져온다.** 임시본은 umask 로 만들어져서 그대로
    // 들여놓으면 600 문서가 664 가 된다(실측) — 대외비 문서의 열람 범위가
    // 말없이 넓어진다.
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&landing, meta.permissions());
    }
    std::fs::rename(&landing, path)
        .with_context(|| format!("저장 실패: {}", path.display()))?;
    std::mem::forget(_land_guard);
    let _ = std::fs::remove_dir_all(&workdir);

    Ok(SaveReport {
        path: path.to_path_buf(),
        backup: Some(backup),
        ops_applied: log.len(),
        paragraphs: got.paras.len(),
        cells: got.cells.len(),
    })
}

fn tempdir_for(path: &Path) -> Result<PathBuf> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("doc")
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(16)
        .collect::<String>();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("hwp-cmd-save-{stem}-{nanos}"));
    std::fs::create_dir_all(&dir).with_context(|| format!("{} 만들기 실패", dir.display()))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Snapshot {
        Snapshot {
            paras: [
                ((0, 0), "과 업 지 시 서".to_string()),
                ((0, 1), "1. 과업명".to_string()),
                ((0, 2), "○ 소요예산: 금19,800,000원".to_string()),
            ]
            .into_iter()
            .collect(),
            cells: [
                ((0u32, 0usize, 0usize), "사업명".to_string()),
                ((0, 0, 1), "바이오·의료기술개발".to_string()),
            ]
            .into_iter()
            .collect(),
            drawings: Default::default(),
        }
    }

    #[test]
    fn insert_text_lands_at_the_offset() {
        let out = expected(&doc(), &[Op::InsertText {
            at: (0, 1),
            offset: 3,
            text: "수정 ".into(),
        }])
        .unwrap();
        assert_eq!(out.paras[&(0, 1)], "1. 수정 과업명");
    }

    #[test]
    fn delete_text_removes_exactly_the_count() {
        let out = expected(&doc(), &[Op::DeleteText {
            at: (0, 2),
            offset: 0,
            count: 2,
        }])
        .unwrap();
        assert_eq!(out.paras[&(0, 2)], "소요예산: 금19,800,000원");
    }

    #[test]
    fn offsets_beyond_the_paragraph_are_refused_before_touching_the_file() {
        let err = expected(&doc(), &[Op::InsertText {
            at: (0, 1),
            offset: 99,
            text: "x".into(),
        }])
        .unwrap_err();
        assert!(format!("{err}").contains("넘습"), "{err}");

        let err = expected(&doc(), &[Op::DeleteText {
            at: (0, 0),
            offset: 5,
            count: 100,
        }])
        .unwrap_err();
        assert!(format!("{err}").contains("넘습"), "{err}");
    }

    #[test]
    fn inserting_a_paragraph_shifts_the_ones_after_it() {
        let out = expected(&doc(), &[Op::InsertParagraph { at: (0, 1) }]).unwrap();
        assert_eq!(out.paras[&(0, 0)], "과 업 지 시 서");
        assert_eq!(out.paras[&(0, 1)], "", "새 단락은 비어 있다");
        assert_eq!(out.paras[&(0, 2)], "1. 과업명", "뒤로 밀렸다");
        assert_eq!(out.paras[&(0, 3)], "○ 소요예산: 금19,800,000원");
        assert_eq!(out.paras.len(), 4);
    }

    #[test]
    fn deleting_a_paragraph_pulls_the_ones_after_it() {
        let out = expected(&doc(), &[Op::DeleteParagraph { at: (0, 1) }]).unwrap();
        assert_eq!(out.paras.len(), 2);
        assert_eq!(out.paras[&(0, 1)], "○ 소요예산: 금19,800,000원", "앞으로 당겨졌다");
    }

    #[test]
    fn ops_compose_in_order() {
        let out = expected(&doc(), &[
            Op::DeleteText { at: (0, 2), offset: 0, count: 2 },
            Op::InsertText { at: (0, 2), offset: 0, text: "□ ".into() },
        ])
        .unwrap();
        assert_eq!(out.paras[&(0, 2)], "□ 소요예산: 금19,800,000원");
    }

    #[test]
    fn the_log_undoes_one_at_a_time() {
        let mut log = EditLog::new();
        log.push(Op::InsertText { at: (0, 0), offset: 0, text: "가".into() });
        log.push(Op::InsertText { at: (0, 0), offset: 1, text: "나".into() });
        assert_eq!(log.len(), 2);
        let undone = log.undo().unwrap();
        assert!(undone.describe().contains("\"나\""));
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn diff_report_names_what_moved() {
        let want = doc();
        let mut got = doc();
        got.paras.insert((0, 1), "1. 과업명 아니고 다른 것".into());
        let parts = vec!["FileHeader".to_string()];
        let r = diff_report(&want, &got, &parts, &parts).expect("차이가 있어야 한다");
        assert!(r.contains("단락1 의 본문이 다르다"), "{r}");

        // 같으면 보고할 것이 없다
        assert!(diff_report(&want, &doc(), &parts, &parts).is_none());
    }

    #[test]
    fn diff_report_catches_a_changed_container() {
        let d = doc();
        let r = diff_report(
            &d,
            &d,
            &["FileHeader".to_string(), "DocInfo".to_string()],
            &["FileHeader".to_string()],
        )
        .expect("부품이 사라졌으면 보고해야 한다");
        assert!(r.contains("부품이 바뀌었"), "{r}");
    }
}
