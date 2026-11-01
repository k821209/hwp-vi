//! 일반 텍스트 파일 — 읽기·고치기·저장.
//!
//! 사용자 요청(2026-10-13): `hwp PLAN.md` 가 거부되었고, **간단한 텍스트는
//! 소화해야 한다**는 지적. 이 도구의 두벌식 오토마톤은 터미널 IME 가 없는
//! ssh 에서 한글을 칠 수 있는 유일한 길이라, 텍스트를 *고칠* 수 있다는 것이
//! 보기만 하는 것보다 훨씬 쓸모가 크다(보기는 `less`·`cat` 이 이미 한다).
//!
//! ## 여기에는 백엔드도 증인도 없다
//!
//! HWP 경로는 조판을 rhwp 에서, 본문을 독립 증인에서 받아 둘을 대조한다.
//! 텍스트 파일에는 둘 다 없다. 대신 **파일 자체가 증인이다** — 쓰고 다시
//! 읽어 기대와 글자 하나까지 맞춰 보고, 맞을 때만 원본을 바꾼다. 자가검증의
//! 규율은 그대로이고 `--force` 도 여기에 없다.
//!
//! ## 망가뜨리지 않기 위해 지키는 세 가지
//!
//! 1. **UTF-8 이 아니면 열지 않는다.** 한국 텍스트 파일은 CP949(EUC-KR)가
//!    흔하다. 그걸 UTF-8 로 억지로 읽어 저장하면 파일이 통째로 망가지는데,
//!    열 때는 아무 소리도 나지 않는다. 그래서 유효한 UTF-8 이 아니면 무엇이
//!    문제인지 말하고 거부한다 — 조용히 손상시키는 것보다 못 여는 쪽이 낫다.
//! 2. **줄끝을 보존한다.** CRLF 파일을 LF 로 저장하면 모든 줄이 바뀐 것으로
//!    보인다(git diff 가 파일 전체를 바꿈으로 보고한다). 읽을 때 무엇이었는지
//!    적어 두고 쓸 때 그대로 쓴다. 섞여 있으면 많은 쪽을 따르고 그렇다고 말한다.
//! 3. **마지막 줄바꿈의 유무를 보존한다.** POSIX 텍스트 파일은 줄바꿈으로
//!    끝나지만 아닌 파일도 있다. 없던 것을 붙이거나 있던 것을 떼면 그것도
//!    내용 변경이다.
//!
//! ## 좌표
//!
//! 구역은 0 하나뿐이고 단락 순번이 **줄 번호**(0부터)다. 그래서 편집 좌표가
//! HWP 와 같은 `(구역, 단락)` 꼴이고, 버퍼·되돌리기·두벌식·`hwp api` 가 모두
//! 손대지 않고 그대로 돈다.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::edit::{EditLog, Op, SaveReport};
use crate::screen::{Block, Column, Coord, Page, ScreenDoc};
use crate::wrap::wrap;

/// 줄끝. 파일에서 읽은 것을 그대로 돌려주기 위해 들고 있는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    Crlf,
}

impl Eol {
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::Crlf => "\r\n",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::Crlf => "CRLF",
        }
    }
}

/// 읽어들인 텍스트 파일. 줄과, 원본을 되만들기 위한 사실들.
#[derive(Debug, Clone)]
pub struct TextFile {
    pub path: PathBuf,
    /// 줄끝을 떼어 낸 줄들. 빈 줄도 그대로 들어 있다.
    pub lines: Vec<String>,
    pub eol: Eol,
    /// 원본이 줄바꿈으로 끝났는가
    pub final_newline: bool,
    /// 줄끝이 섞여 있었는가 — 저장하면 한 가지로 통일된다는 뜻이므로 말해야 한다
    pub mixed_eol: bool,
}

/// 이 파일을 텍스트로 열 수 있는가. 열 수 있으면 `Ok`, 아니면 이유.
///
/// **바이트를 보고 판단한다.** 확장자는 믿지 않는다 — `.md` 가 CP949 일 수
/// 있고 확장자 없는 파일이 멀쩡한 UTF-8 일 수 있다.
pub fn looks_like_text(bytes: &[u8]) -> Result<()> {
    // NUL 이 있으면 이진 파일이다. 텍스트 편집기로 열면 저장할 때 망가진다.
    if bytes.contains(&0) {
        bail!("이진 파일로 보입니다 (NUL 바이트가 있습니다).");
    }
    if let Err(e) = std::str::from_utf8(bytes) {
        let at = e.valid_up_to();
        // 그 자리의 바이트를 보여 준다 — CP949 한글은 0x80~0xFD 로 시작한다
        let byte = bytes.get(at).copied().unwrap_or(0);
        let guess = if (0x81..=0xFD).contains(&byte) {
            "\n  CP949(EUC-KR)로 보입니다. UTF-8 로 바꾼 뒤 열어 주세요:\
             \n    iconv -f CP949 -t UTF-8 <파일> > <새파일>"
        } else {
            ""
        };
        bail!(
            "UTF-8 이 아닙니다 ({at}번째 바이트 0x{byte:02X} 부터 읽을 수 없습니다).\
             \n  UTF-8 이 아닌 파일은 **열지 않습니다** — 억지로 읽어 저장하면 \
             파일이 망가지고, 열 때는 아무 표시도 나지 않습니다.{guess}"
        );
    }
    Ok(())
}

impl TextFile {
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).with_context(|| format!("{} 를 읽을 수 없습니다", path.display()))?;
        looks_like_text(&bytes).with_context(|| format!("{} 를 텍스트로 열 수 없습니다", path.display()))?;
        let s = String::from_utf8(bytes).expect("looks_like_text 가 확인했다");
        Ok(Self::from_str_with_path(&s, path))
    }

    /// 문자열에서. 시험과 `read` 가 같이 쓴다.
    pub fn from_str_with_path(s: &str, path: &Path) -> Self {
        let crlf = s.matches("\r\n").count();
        let lf_total = s.matches('\n').count();
        let lone_lf = lf_total - crlf;
        let eol = if crlf > lone_lf { Eol::Crlf } else { Eol::Lf };
        let mixed_eol = crlf > 0 && lone_lf > 0;
        let final_newline = s.ends_with('\n');

        // 줄끝을 떼어 낸다. `split` 뒤 마지막 빈 조각은 줄바꿈으로 끝난
        // 표시이므로 버린다 — 그 사실은 `final_newline` 이 들고 있다.
        let body = if final_newline { &s[..s.len() - 1] } else { s };
        let body = body.strip_suffix('\r').unwrap_or(body);
        let lines: Vec<String> = if body.is_empty() && !final_newline {
            Vec::new()
        } else {
            body.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect()
        };
        Self { path: path.to_path_buf(), lines, eol, final_newline, mixed_eol }
    }

    /// 줄들을 원본 꼴의 바이트로. 읽은 것을 안 고치고 쓰면 **원본과 같아야 한다.**
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.lines.join(self.eol.as_str());
        if self.final_newline {
            out.push_str(self.eol.as_str());
        }
        out.into_bytes()
    }

    /// 페이저가 그릴 화면 모델.
    ///
    /// 조판기가 없으므로 쪽을 우리가 나눈다 — `rows` 줄마다 한 쪽. 표·그림·
    /// 들여쓰기·꾸밈은 없다.
    pub fn to_screen_doc(&self, column: usize, rows: usize) -> ScreenDoc {
        let cells = column.max(8);
        let rows = rows.max(1);
        let paras: HashMap<Coord, String> =
            self.lines.iter().enumerate().map(|(i, l)| ((0u32, i as u32), l.clone())).collect();

        // 줄마다 접은 결과를 쪽에 나눠 담는다. 접힌 줄 수로 쪽을 센다 —
        // 그래야 쪽 넘기기가 화면과 맞는다.
        let mut pages: Vec<Page> = Vec::new();
        let mut blocks: Vec<Block> = Vec::new();
        let mut used = 0usize;
        for (i, line) in self.lines.iter().enumerate() {
            let wrapped = wrap(line, cells);
            if used + wrapped.len() > rows && !blocks.is_empty() {
                let n = pages.len() as u32 + 1;
                pages.push(Page { number: n, printed: Some(n), section: 0, blocks: std::mem::take(&mut blocks) });
                used = 0;
            }
            used += wrapped.len();
            blocks.push(Block {
                section: 0,
                para_index: Some(i as u32),
                lines: wrapped,
                continues: false,
                continued: false,
            });
        }
        let n = pages.len() as u32 + 1;
        pages.push(Page { number: n, printed: Some(n), section: 0, blocks });

        ScreenDoc {
            path: self.path.display().to_string(),
            format: "text".into(),
            page_count: pages.len() as u32,
            para_count: self.lines.len(),
            paras,
            section_counts: vec![self.lines.len()],
            outline: Vec::new(),
            body_width: 0,
            indents: HashMap::new(),
            runs: HashMap::new(),
            aligns: HashMap::new(),
            objects: HashMap::new(),
            tables: Vec::new(),
            column: Column { cells, measured: None },
            pages,
        }
    }

    /// 쌓인 연산을 줄에 적용한다. **저장 전에 "이렇게 되어야 한다" 를 만드는 쪽.**
    pub fn applied(&self, log: &EditLog) -> Result<Vec<String>> {
        let mut lines = self.lines.clone();
        for (n, op) in log.ops().iter().enumerate() {
            let step = n + 1;
            match op {
                Op::InsertText { at, offset, text } => {
                    let line = line_mut(&mut lines, *at, step)?;
                    let chars: Vec<char> = line.chars().collect();
                    if *offset > chars.len() {
                        bail!("{step}번째 연산: 줄{} 은 {}자인데 {offset}번째에 넣으려 합니다", at.1, chars.len());
                    }
                    let head: String = chars[..*offset].iter().collect();
                    let tail: String = chars[*offset..].iter().collect();
                    *line = format!("{head}{text}{tail}");
                }
                Op::DeleteText { at, offset, count } => {
                    let line = line_mut(&mut lines, *at, step)?;
                    let chars: Vec<char> = line.chars().collect();
                    if offset + count > chars.len() {
                        bail!(
                            "{step}번째 연산: 줄{} 은 {}자인데 {offset}번째부터 {count}자를 지우려 합니다",
                            at.1,
                            chars.len()
                        );
                    }
                    let head: String = chars[..*offset].iter().collect();
                    let tail: String = chars[offset + count..].iter().collect();
                    *line = format!("{head}{tail}");
                }
                Op::InsertParagraph { at } => {
                    let i = at.1 as usize;
                    if i > lines.len() {
                        bail!("{step}번째 연산: 줄이 {}개인데 줄{i} 앞에 넣으려 합니다", lines.len());
                    }
                    lines.insert(i, String::new());
                }
                Op::DeleteParagraph { at } => {
                    let i = at.1 as usize;
                    if i >= lines.len() {
                        bail!("{step}번째 연산: 줄이 {}개인데 줄{i} 을 지우려 합니다", lines.len());
                    }
                    lines.remove(i);
                }
                // 일반 텍스트 파일에는 서식이 없다
                Op::SetParaAlign { .. } => {
                    bail!("{step}번째 연산: 텍스트 파일에는 문단 정렬이 없습니다 — {}", op.describe());
                }
                Op::SetDeco { .. } => {
                    bail!("{step}번째 연산: 텍스트 파일에는 글자 꾸밈이 없습니다 — {}", op.describe());
                }
                Op::SetCell { .. }
                | Op::InsertTable { .. }
                | Op::DeleteTable { .. }
                | Op::DeleteControl { .. }
                | Op::InsertTextInCell { .. }
                | Op::DeleteTextInCell { .. } => {
                    bail!("{step}번째 연산: 텍스트 파일에는 표가 없습니다 — {}", op.describe());
                }
            }
        }
        Ok(lines)
    }

    /// 자가검증 저장. **파일 자체가 증인이다.**
    ///
    /// 임시본에 쓰고 **다시 읽어** 기대와 글자 하나까지 맞춰 본 뒤에만 원본을
    /// 바꾼다. 다르면 멈추고 원본을 건드리지 않는다. `--force` 는 없다.
    pub fn save(&self, log: &EditLog) -> Result<SaveReport> {
        if log.len() == 0 {
            bail!("저장할 편집이 없습니다");
        }
        let want = self.applied(log)?;
        let after = TextFile {
            path: self.path.clone(),
            lines: want.clone(),
            eol: self.eol,
            final_newline: self.final_newline,
            mixed_eol: false,
        };

        // 임시본은 **같은 폴더**에 쓴다 — /tmp 에 쓰면 폴더가 다른 파일
        // 시스템일 때 교체가 복사가 되고, 중간에 끊기면 반쯤 쓴 파일이 남는다.
        let dir = self.path.parent().unwrap_or(Path::new("."));
        let tmp = dir.join(format!(
            ".{}.hwp-cmd.tmp",
            self.path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
        ));
        std::fs::write(&tmp, after.to_bytes())
            .with_context(|| format!("{} 에 쓸 수 없습니다", tmp.display()))?;

        // 되읽어 대조한다.
        let back = TextFile::read(&tmp).with_context(|| {
            let _ = std::fs::remove_file(&tmp);
            format!("{} 를 다시 읽을 수 없습니다", tmp.display())
        })?;
        if back.lines != want {
            let _ = std::fs::remove_file(&tmp);
            let at = back.lines.iter().zip(&want).position(|(a, b)| a != b);
            let detail = match at {
                Some(i) => format!(
                    "줄{i} 이 다릅니다\n    기대: {:?}\n    실제: {:?}",
                    want.get(i),
                    back.lines.get(i)
                ),
                None => format!("줄 수가 다릅니다 — 기대 {} / 실제 {}", want.len(), back.lines.len()),
            };
            bail!(
                "저장을 멈췄습니다 — 저장본이 기대와 다릅니다. 원본은 그대로 두었습니다.\n  {detail}"
            );
        }

        let backup = self.path.with_extension(match self.path.extension() {
            Some(e) => format!("{}.bak", e.to_string_lossy()),
            None => "bak".to_string(),
        });
        std::fs::copy(&self.path, &backup)
            .with_context(|| format!("백업을 만들 수 없습니다: {}", backup.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("{} 로 바꿔 끼울 수 없습니다", self.path.display()))?;

        Ok(SaveReport {
            path: self.path.clone(),
            backup: Some(backup),
            ops_applied: log.len(),
            paragraphs: want.len(),
            cells: 0,
        })
    }
}

fn line_mut<'a>(lines: &'a mut [String], at: Coord, step: usize) -> Result<&'a mut String> {
    if at.0 != 0 {
        bail!("{step}번째 연산: 텍스트 파일은 구역이 0 하나뿐입니다 (받은 값: {})", at.0);
    }
    let i = at.1 as usize;
    let n = lines.len();
    lines
        .get_mut(i)
        .ok_or_else(|| anyhow::anyhow!("{step}번째 연산: 줄이 {n}개인데 줄{i} 을 고치려 합니다"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> PathBuf {
        PathBuf::from("/tmp/x.txt")
    }

    /// **고치지 않고 쓰면 원본 바이트가 그대로 나와야 한다.** 줄끝과 마지막
    /// 줄바꿈까지. 이게 깨지면 파일을 열었다 닫는 것만으로 내용이 바뀐다.
    #[test]
    fn reading_and_writing_back_is_byte_for_byte_the_same() {
        for s in [
            "한 줄\n",
            "한 줄",                       // 마지막 줄바꿈 없음
            "줄끝이\r\n씨알엪이다\r\n",      // CRLF
            "줄끝이\r\n씨알엪이다",          // CRLF, 마지막 줄바꿈 없음
            "빈 줄이\n\n사이에\n",
            "",                            // 빈 파일
            "\n",                          // 줄바꿈 하나
            "\n\n\n",
        ] {
            let f = TextFile::from_str_with_path(s, &p());
            assert_eq!(
                String::from_utf8(f.to_bytes()).unwrap(),
                s,
                "되쓰기가 원본과 다르다: {s:?} → 줄 {:?} eol {:?} 끝줄바꿈 {}",
                f.lines,
                f.eol.label(),
                f.final_newline
            );
        }
    }

    #[test]
    fn crlf_is_detected_and_kept() {
        let f = TextFile::from_str_with_path("가\r\n나\r\n", &p());
        assert_eq!(f.eol, Eol::Crlf);
        assert_eq!(f.lines, vec!["가", "나"]);
        assert!(!f.mixed_eol);
        let mixed = TextFile::from_str_with_path("가\r\n나\n", &p());
        assert!(mixed.mixed_eol, "섞인 줄끝을 알아채야 한다");
    }

    /// **CP949 는 열지 않는다.** 억지로 읽어 저장하면 파일이 망가진다.
    #[test]
    fn a_cp949_file_is_refused_and_says_how_to_convert() {
        // "한글" in CP949
        let bytes = [0xC7u8, 0xD1, 0xB1, 0xDB, 0x0A];
        let err = looks_like_text(&bytes).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("CP949"), "{msg}");
        assert!(msg.contains("iconv"), "바꾸는 길을 알려줘야 한다: {msg}");
    }

    #[test]
    fn a_binary_file_is_refused() {
        let err = looks_like_text(&[0x7F, 0x45, 0x4C, 0x46, 0x00, 0x01]).unwrap_err();
        assert!(format!("{err}").contains("이진"), "{err}");
    }

    /// 접은 줄은 표시일 뿐이고 **모델의 원문은 접히기 전 그대로**여야 한다.
    /// HWP 경로에서 바로 이것이 깨져 커서 오프셋이 밀렸다.
    #[test]
    fn the_model_keeps_the_line_untouched_even_when_it_wraps() {
        let long = "한 줄이 아주 길어서 좁은 폭에서는 여러 줄로 접히게 되는 그런 줄이다";
        let f = TextFile::from_str_with_path(&format!("{long}\n"), &p());
        let doc = f.to_screen_doc(12, 50);
        assert_eq!(doc.paras.get(&(0, 0)).map(String::as_str), Some(long));
        let drawn: usize = doc.pages[0].blocks[0].lines.len();
        assert!(drawn > 1, "좁은 폭에서 접혀야 한다 (줄 {drawn}개)");
    }

    #[test]
    fn editing_ops_apply_to_lines() {
        let f = TextFile::from_str_with_path("가나\n다라\n", &p());
        let mut log = EditLog::new();
        log.push(Op::InsertText { at: (0, 0), offset: 1, text: "X".into() });
        log.push(Op::DeleteParagraph { at: (0, 1) });
        log.push(Op::InsertParagraph { at: (0, 1) });
        assert_eq!(f.applied(&log).unwrap(), vec!["가X나".to_string(), String::new()]);
    }

    #[test]
    fn a_table_op_is_refused_because_text_has_no_tables() {
        let f = TextFile::from_str_with_path("가\n", &p());
        let mut log = EditLog::new();
        log.push(Op::SetCell { table: 0, row: 0, col: 0, lines: vec!["x".into()] });
        assert!(format!("{}", f.applied(&log).unwrap_err()).contains("표가 없습니다"));
    }
}
