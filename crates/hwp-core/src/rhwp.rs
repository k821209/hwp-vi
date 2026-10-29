//! rhwp 프로세스 래퍼 + 호환성 게이트.
//!
//! PLAN.md §8-5: rhwp 는 v1.0 조판 엔진 체계화 중이다. 스키마가 움직였을 때
//! **조용히 오동작하는 것**이 가장 나쁜 결과이므로, 기동 시 `capabilities` 를
//! 읽어 핀과 대조하고 어긋나면 거부한다.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::pin::Pin;

/// `rhwp capabilities` 가 돌려주는 자기서술 중 우리가 보는 부분.
#[derive(Debug, Deserialize)]
pub struct Capabilities {
    pub tool: String,
    pub version: String,
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    pub commands: Vec<CommandSpec>,
}

#[derive(Debug, Deserialize)]
pub struct CommandSpec {
    pub name: String,
    #[serde(default)]
    pub json: bool,
    #[serde(default)]
    pub batch: bool,
    #[serde(default)]
    pub category: String,
}

impl Capabilities {
    pub fn command(&self, name: &str) -> Option<&CommandSpec> {
        self.commands.iter().find(|c| c.name == name)
    }
}

/// 게이트가 거부한 이유. 사람에게 보여줄 문장을 들고 있다.
#[derive(Debug)]
pub enum GateFailure {
    NotFound { searched: Vec<String> },
    WrongTool { got: String },
    VersionMismatch { want: String, got: String },
    SchemaMajorMismatch { want: String, got: String },
    MissingCommands { names: Vec<String> },
    CommandsWithoutJson { names: Vec<String> },
}

impl std::fmt::Display for GateFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { searched } => write!(
                f,
                "rhwp 바이너리를 찾을 수 없습니다. 찾아본 곳:\n{}\n\n  해결: scripts/fetch-rhwp.sh 를 실행하거나 HWP_RHWP_BIN 으로 경로를 지정하세요.",
                searched.iter().map(|s| format!("    {s}")).collect::<Vec<_>>().join("\n")
            ),
            Self::WrongTool { got } => write!(f, "rhwp 가 아닌 도구다 (tool={got})"),
            Self::VersionMismatch { want, got } => write!(
                f,
                "rhwp 버전이 핀과 다르다 — 기대 {want}, 실제 {got}.\n  \
                 스키마가 움직였을 수 있어 그대로 쓰지 않는다. 핀을 올리려면 \
                 샘플 회귀(cargo test -p hwp-core)를 먼저 통과시켜라."
            ),
            Self::SchemaMajorMismatch { want, got } => write!(
                f,
                "rhwp JSON 스키마 major 가 다릅니다 — 기대 {want}.x, 실제 {got}. 의미가 바뀌었을 수 있어 거부합니다."
            ),
            Self::MissingCommands { names } => {
                write!(f, "rhwp 에 필요한 명령이 없습니다: {}", names.join(", "))
            }
            Self::CommandsWithoutJson { names } => {
                write!(f, "rhwp 의 다음 명령이 --json 을 내지 않습니다: {}", names.join(", "))
            }
        }
    }
}

impl std::error::Error for GateFailure {}

/// 호환성이 확인된 rhwp.
pub struct Rhwp {
    bin: PathBuf,
    caps: Capabilities,
}

impl Rhwp {
    /// 바이너리를 찾고 호환성 게이트를 통과시킨다.
    pub fn discover(pin: &Pin) -> Result<Self> {
        let (bin, searched) = locate();
        let Some(bin) = bin else {
            return Err(GateFailure::NotFound { searched }.into());
        };
        let caps = read_capabilities(&bin)?;
        gate(pin, &caps)?;
        Ok(Self { bin, caps })
    }

    /// 게이트를 **건너뛰고** 연다. `hwp doctor` 가 "무엇이 왜 안 맞는지"를
    /// 보고하려면 거부된 rhwp 도 들여다볼 수 있어야 한다.
    pub fn open_ungated(bin: PathBuf) -> Result<Self> {
        let caps = read_capabilities(&bin)?;
        Ok(Self { bin, caps })
    }

    pub fn bin(&self) -> &Path {
        &self.bin
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    /// stdout 을 그대로 돌려준다.
    pub fn run(&self, args: &[&str]) -> Result<Vec<u8>> {
        let out = Command::new(&self.bin)
            .args(args)
            .output()
            .with_context(|| format!("rhwp 실행 실패: {}", self.bin.display()))?;
        if !out.status.success() {
            let code = out.status.code().unwrap_or(-1);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            // rhwp 의 jsonContract: 단건 명령 실패 시 stdout 은 0바이트이고
            // 사람이 읽을 메시지는 stdout 또는 stderr 로 간다.
            let msg = if stderr.trim().is_empty() { stdout } else { stderr };
            bail!("rhwp {} 실패 (exit {code}): {}", args.join(" "), msg.trim());
        }
        Ok(out.stdout)
    }

    /// `--json` 을 붙여 호출하고 역직렬화한다.
    pub fn json<T: DeserializeOwned>(&self, args: &[&str]) -> Result<T> {
        let mut v: Vec<&str> = args.to_vec();
        if !v.contains(&"--json") {
            v.push("--json");
        }
        let bytes = self.run(&v)?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("rhwp {} 의 JSON 해석 실패", v.join(" ")))
    }

    /// 사람이 읽는 한 문단 요약.
    pub fn explain(&self, doc: &Path) -> Result<String> {
        let out = self.run(&["explain", &doc.to_string_lossy()])?;
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }

    pub fn info(&self, doc: &Path) -> Result<Info> {
        self.json(&["info", &doc.to_string_lossy()])
    }

    pub fn dump_pages(&self, doc: &Path) -> Result<DumpPages> {
        self.json(&["dump-pages", &doc.to_string_lossy()])
    }

    pub fn export_tables(&self, doc: &Path) -> Result<TableExport> {
        self.json(&["export-tables", &doc.to_string_lossy()])
    }
}

/// `rhwp export-tables --json`. 표마다 `(section, paragraph)` 편집 좌표를 들고 온다.
#[derive(Debug, Deserialize)]
pub struct TableExport {
    #[serde(rename = "tableCount", default)]
    pub table_count: u32,
    #[serde(default)]
    pub tables: Vec<RawTable>,
}

#[derive(Debug, Deserialize)]
pub struct RawTable {
    /// 문서 안의 표 번호 (0부터)
    #[serde(default)]
    pub index: u32,
    #[serde(default)]
    pub section: u32,
    /// 이 표가 놓인 본문 단락
    #[serde(default)]
    pub paragraph: u32,
    /// 한 단락 안에 컨트롤이 여럿일 때의 순번 — 표 여럿이 같은 단락에 걸린다
    /// (s16.hwp 에서 t0·t1·t2 가 모두 단락 0 이다).
    #[serde(default)]
    pub control: u32,
    #[serde(default)]
    pub rows: usize,
    #[serde(default)]
    pub cols: usize,
    #[serde(default)]
    pub cells: Vec<RawCell>,
}

#[derive(Debug, Deserialize)]
pub struct RawCell {
    #[serde(default)]
    pub row: usize,
    #[serde(default)]
    pub col: usize,
    #[serde(rename = "rowSpan", default = "one")]
    pub row_span: usize,
    #[serde(rename = "colSpan", default = "one")]
    pub col_span: usize,
    #[serde(rename = "isHeader", default)]
    pub is_header: bool,
    #[serde(default)]
    pub text: String,
}

fn one() -> usize {
    1
}

/// `rhwp info --json` 중 우리가 쓰는 부분. 모르는 필드는 버린다.
#[derive(Debug, Deserialize)]
pub struct Info {
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    pub format: String,
    #[serde(rename = "pageCount")]
    pub page_count: u32,
    #[serde(rename = "paraCount", default)]
    pub para_count: u32,
    #[serde(default)]
    pub sections: u32,
}

/// `rhwp dump-pages --json` — R1 페이저가 화면을 그릴 조판값.
#[derive(Debug, Deserialize)]
pub struct DumpPages {
    #[serde(rename = "pageCount")]
    pub page_count: u32,
    pub pages: Vec<PageLayout>,
}

#[derive(Debug, Deserialize)]
pub struct PageLayout {
    /// **구역 번호.** `paraIndex` 는 구역 안에서의 순번이다 — 실측으로 확인했다
    /// (s14.hwpx: 구역 0 이 0~44, 1 이 0~3, 2 가 0~15, 3 이 0~36, 합 102).
    /// 편집 좌표 `edit --section N --para M` 과 같은 쌍이다.
    #[serde(default)]
    pub section: u32,
    /// 문서가 **찍는** 쪽번호. 되돌아갈 수 있다 — s00.hwp 는 5번째 쪽에서
    /// 다시 1 이 된다(쪽번호 새로 시작). 그래서 이것으로 쪽을 가리키면 안 된다.
    #[serde(rename = "pageNumber", default)]
    pub page_number: Option<u32>,
    /// 문서 **순서**대로의 쪽 번호 (1부터). 이것이 주소다.
    #[serde(rename = "displayPage", default)]
    pub display_page: Option<u32>,
    #[serde(rename = "bodyArea")]
    pub body_area: BodyArea,
    pub columns: Vec<ColumnLayout>,
}

#[derive(Debug, Deserialize)]
pub struct BodyArea {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Deserialize)]
pub struct ColumnLayout {
    pub index: u32,
    pub items: Vec<LayoutItem>,
}

/// 쪽 안에 놓인 한 항목. `para_index` 가 §8-2 의 그 좌표다.
#[derive(Debug, Deserialize)]
pub struct LayoutItem {
    #[serde(rename = "paraIndex")]
    pub para_index: Option<u32>,
    /// **40자에서 잘린다** (실측). 전문은 독립 증인에서 가져온다.
    #[serde(rename = "textPreview", default)]
    pub text_preview: Option<String>,
    /// `fullParagraph` | `partialParagraph` | …
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(rename = "lineSegs", default)]
    pub line_segs: Option<LineSegs>,
    /// 이 쪽에 놓인 줄들의 세로 위치 (HWPUNIT).
    #[serde(default)]
    pub vpos: Option<Vpos>,
    /// **이 자리에 놓인 표의 크기** — 조판기가 잰 값이다.
    ///
    /// 열별 폭은 주지 않는다(그건 증인이 셀 레코드에서 읽는다). 표 **전체**
    /// 폭을 알면 우리가 읽은 열 폭의 합이 맞는지 대조할 수 있다 — 실측으로
    /// s16.hwp 표1 이 `4836+565+42758 = 48159` 이고 조판값이 `642.12px × 75 =
    /// 48159` 다. 그 일치가 셀 폭 판독의 시험이다.
    #[serde(default)]
    pub table: Option<LayoutTable>,
}

/// 조판기가 잰 표 크기.
#[derive(Debug, Clone, Deserialize)]
pub struct LayoutTable {
    #[serde(rename = "colCount", default)]
    pub col_count: usize,
    #[serde(rename = "rowCount", default)]
    pub row_count: usize,
    /// 96dpi 픽셀. `1px = 75 HWPUNIT` 다 (단 폭에서 쓰는 그 환산).
    #[serde(rename = "widthPx", default)]
    pub width_px: f64,
    #[serde(rename = "heightPx", default)]
    pub height_px: f64,
}

impl LayoutItem {
    /// 이 단락의 **전체** 줄 수. 쪽을 넘는 단락은 닿는 쪽마다 같은 값이 반복된다
    /// — 더하면 안 된다(실측: para 36 이 2쪽·3쪽에 각각 n=2 로 나온다).
    pub fn total_lines(&self) -> Option<u32> {
        self.line_segs.as_ref().map(|l| l.count)
    }

    /// **이 쪽에** 놓인 줄 수. `vpos` 가 걸친 범위를 줄 간격으로 나눈다.
    ///
    /// 줄 간격 = `lineHeight + lineSpacing`. 실측으로 맞췄다: para 20 은
    /// vpos 2727..4954 (폭 2227) 이고 1783+445 = 2228 이라 2줄이다.
    pub fn lines_on_page(&self) -> u32 {
        let Some(segs) = &self.line_segs else { return 1 };
        let Some(vpos) = &self.vpos else { return segs.count };
        // 세그먼트가 없으면 줄 간격을 모른다 — 단락 전체 줄 수로 둔다
        let Some(first) = &segs.first else { return segs.count.max(1) };
        let pitch = first.pitch();
        if pitch <= 0 || vpos.last <= vpos.first {
            return 1;
        }
        let span = vpos.last - vpos.first;
        // 반올림으로 붙인다 — 조판값이 정확히 배수가 아닐 수 있다.
        let lines = (span + pitch / 2) / pitch + 1;
        (lines.max(1) as u32).min(segs.count.max(1))
    }

    pub fn is_partial(&self) -> bool {
        self.kind.as_deref() == Some("partialParagraph")
    }
}

#[derive(Debug, Deserialize)]
pub struct LineSegs {
    pub count: u32,
    /// **없을 수 있다.** 백엔드가 줄 세그먼트를 못 만든 단락은 `count` 만 준다 —
    /// 배포용 스펙 문서(한글문서파일형식 5.0, 70쪽)에서 650항목 중 **96개**가
    /// 그랬고, 필수로 받고 있어서 `missing field 'first'` 로 **문서 전체가
    /// 열리지 않았다.** 조판값 하나가 비는 것은 화면 몇 줄의 문제지만, 파싱이
    /// 터지면 문서를 못 본다.
    #[serde(default)]
    pub first: Option<Segment>,
    #[serde(default)]
    pub last: Option<Segment>,
}

#[derive(Debug, Deserialize)]
pub struct Segment {
    /// 이 줄의 텍스트 폭 (HWPUNIT, 1인치 = 7200)
    #[serde(rename = "segmentWidth", default)]
    pub segment_width: i64,
    #[serde(rename = "lineHeight", default)]
    pub line_height: i64,
    /// **음수가 올 수 있다** — 실측으로 -700 을 봤다(s00.hwp). 고정 줄간격을
    /// 음수로 표현하는 문서가 있다. u32 로 받으면 파싱이 깨진다.
    #[serde(rename = "lineSpacing", default)]
    pub line_spacing: i64,
    #[serde(rename = "textStart", default)]
    pub text_start: i64,
    /// 이 줄이 **단 왼쪽에서 얼마나 들어가 시작하는가** (HWPUNIT). 문단의
    /// 들여쓰기·왼쪽 여백이 여기로 합쳐져 나온다.
    ///
    /// `textStart` 와 혼동하지 말 것 — 그쪽은 이 줄이 단락의 몇 번째 글자에서
    /// 시작하는지(글자 위치)다. 코퍼스에서 `textStart` 는 179줄 중 177줄이 0 이고
    /// 나머지가 50·51 인데, 그게 들여쓰기라면 500pt 라 쪽을 벗어난다.
    #[serde(rename = "columnStart", default)]
    pub column_start: i64,
}

impl Segment {
    /// 줄 간격 — 한 줄에서 다음 줄까지. 음수 줄간격이면 글자 높이로 떨어진다.
    pub fn pitch(&self) -> i64 {
        let p = self.line_height + self.line_spacing;
        if p > 0 { p } else { self.line_height.max(1) }
    }
}

#[derive(Debug, Deserialize)]
pub struct Vpos {
    #[serde(default)]
    pub first: i64,
    #[serde(default)]
    pub last: i64,
}

/// 바이너리를 찾는다. 찾은 경로와, 찾아본 곳 전부를 함께 돌려준다.
fn locate() -> (Option<PathBuf>, Vec<String>) {
    let mut searched = Vec::new();
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(p) = std::env::var("HWP_RHWP_BIN") {
        candidates.push(PathBuf::from(p));
    }
    // 저장소 안에 받아둔 것 (scripts/fetch-rhwp.sh 가 여기에 푼다)
    candidates.push(PathBuf::from("vendor/rhwp/rhwp"));
    if let Some(dir) = workspace_root() {
        candidates.push(dir.join("vendor/rhwp/rhwp"));
    }
    // 실행 파일 옆
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            candidates.push(d.join("rhwp"));
        }
    }

    for c in candidates {
        searched.push(c.display().to_string());
        if c.is_file() {
            return (Some(c), searched);
        }
    }

    // PATH
    searched.push("$PATH 의 rhwp".to_string());
    if let Ok(out) = Command::new("rhwp").arg("--version").output() {
        if out.status.success() {
            return (Some(PathBuf::from("rhwp")), searched);
        }
    }

    (None, searched)
}

/// 소스 나무 안에서 돌고 있다면 그 루트. 아니면 `None`.
///
/// **`env!("CARGO_MANIFEST_DIR")` 로 하지 않는다.** 그건 빌드한 사람의 절대
/// 경로를 실행 파일에 **문자열로 박는다** — 공개 배포본을 `strings` 로 열면
/// 만든 사람의 계정명이 보인다(실측: 그래서 2개가 남았다). `--remap-path-prefix`
/// 는 소스 경로만 바꾸고 이 환경변수에는 닿지 않는다.
///
/// 대신 **실행 파일이 있는 자리에서 거슬러 올라가** 워크스페이스 표지
/// (`rhwp-pin.toml`)를 찾는다. 개발 나무에서는 찾아지고
/// (`target/release/hwp` → `target/` → 루트), 설치된 배포본에서는 찾아지지
/// 않는다 — 그게 맞는 답이다. 그리고 **박히는 문자열이 없다.**
///
/// 깊이를 고정하지 않는 이유: `CARGO_TARGET_DIR` 를 바꾸면
/// (`target/dist/release/hwp`) 깊이가 달라진다. 표지를 찾는 쪽은 둘 다 된다.
fn workspace_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?;
    // 8단계면 어떤 target 배치도 덮는다. 무한정 올라가면 사용자의 집이나
    // 루트까지 뒤지게 되는데, 거기서 표지를 만나는 것은 우연이다.
    for _ in 0..8 {
        if dir.join("rhwp-pin.toml").is_file() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
    None
}

/// 소스 나무의 루트 — 다른 크레이트도 쓴다(`hwp doctor` 의 코퍼스 보고).
pub fn source_tree_root() -> Option<PathBuf> {
    workspace_root()
}

fn read_capabilities(bin: &Path) -> Result<Capabilities> {
    let out = Command::new(bin)
        .arg("capabilities")
        .output()
        .with_context(|| format!("rhwp capabilities 실행 실패: {}", bin.display()))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        // **동적 링커가 거부한 것은 "바이너리가 맞는지" 의 문제가 아니다.**
        //
        // 전에는 stderr 를 버리고 "rhwp 바이너리가 맞는지 확인하라" 고만 했다.
        // 그런데 그 말이 맞는 경우는 드물고, 실제로 받은 보고(2026-10-10, WSL
        // Ubuntu 20.04)는 **핀과 sha256 이 정확히 맞는 바로 그 바이너리**가
        // 리눅스 배포판의 glibc 보다 새 것을 요구한 것이었다. 설명해 주는 줄은
        // stderr 에 있었고 우리가 그걸 버렸다.
        if let Some(msg) = glibc_too_old(&stderr) {
            bail!("{msg}\n  바이너리: {}", bin.display());
        }
        bail!(
            "rhwp capabilities 가 실패했다 (exit {}) — {}\n  {}",
            out.status.code().unwrap_or(-1),
            bin.display(),
            if stderr.trim().is_empty() {
                "아무 말도 하지 않았습니다. rhwp 바이너리가 맞는지 확인하세요.".to_string()
            } else {
                stderr.trim().replace('\n', "\n  ")
            }
        );
    }
    serde_json::from_slice(&out.stdout).context("rhwp capabilities 의 JSON 해석 실패")
}

/// 동적 링커가 **glibc 가 낡아서** 거부했는가. 그렇다면 할 일을 적어 돌려준다.
///
/// 받은 보고의 모양:
/// ```text
/// …/rhwp: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found
/// ```
/// 요구하는 버전은 저 줄들에 다 적혀 있으므로 **가장 높은 것**을 뽑아 말한다 —
/// "2.32 가 없다" 만 보고 2.32 를 구하면 또 막힌다.
pub fn glibc_too_old(stderr: &str) -> Option<String> {
    let mut wanted: Vec<(u32, u32)> = Vec::new();
    for line in stderr.lines() {
        if !line.contains("not found") {
            continue;
        }
        let Some(rest) = line.split("GLIBC_").nth(1) else { continue };
        let v: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        let mut it = v.split('.');
        if let (Some(a), Some(b)) = (it.next(), it.next()) {
            if let (Ok(a), Ok(b)) = (a.parse(), b.parse()) {
                wanted.push((a, b));
            }
        }
    }
    let (hi_a, hi_b) = wanted.into_iter().max()?;
    Some(format!(
        "내려받은 rhwp 가 이 리눅스의 glibc 보다 새 것을 요구합니다 —          **glibc {hi_a}.{hi_b} 이상**이 필요합니다.\n           sha256 은 맞았습니다. 받은 파일이 깨진 것이 아니라 **빌드된 환경이 더 새 것**이라는 뜻입니다.\n           `ldd --version` 으로 지금 버전을 확인하고, 둘 중 하나로 가세요:\n             (1) 더 새 배포판에서 쓴다 — WSL 이면 `wsl --install Ubuntu-24.04`\n             (2) **소스에서 직접 빌드한다** — `./scripts/build-rhwp.sh`\n           실행 관문은 sha256 이 아니라 `capabilities` 만 보므로 직접 빌드한 것도 그대로 받습니다."
    ))
}

/// 핀과 capabilities 를 대조한다. 어긋나면 이유를 들고 거부한다.
pub fn gate(pin: &Pin, caps: &Capabilities) -> Result<(), GateFailure> {
    if caps.tool != "rhwp" {
        return Err(GateFailure::WrongTool { got: caps.tool.clone() });
    }
    if caps.version != pin.version {
        return Err(GateFailure::VersionMismatch {
            want: pin.version.clone(),
            got: caps.version.clone(),
        });
    }
    let got_major = caps.schema_version.split('.').next().unwrap_or("0");
    if got_major != pin.schema_major() {
        return Err(GateFailure::SchemaMajorMismatch {
            want: pin.schema_major().to_string(),
            got: caps.schema_version.clone(),
        });
    }

    let mut missing = Vec::new();
    let mut no_json = Vec::new();
    for name in &pin.required_commands {
        match caps.command(name) {
            None => missing.push(name.clone()),
            Some(c) if !c.json => no_json.push(name.clone()),
            Some(_) => {}
        }
    }
    if !missing.is_empty() {
        return Err(GateFailure::MissingCommands { names: missing });
    }
    if !no_json.is_empty() {
        return Err(GateFailure::CommandsWithoutJson { names: no_json });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **줄 세그먼트가 없어도 파싱이 끝나야 한다.**
    ///
    /// 백엔드가 줄 세그먼트를 못 만든 단락은 `lineSegs` 에 `count` 만 준다.
    /// 필수로 받고 있어서 `missing field 'first'` 로 터졌고, 그러면 조판값을
    /// 하나도 못 받아 **문서 전체가 열리지 않았다.** 공개된 한글문서파일형식
    /// 5.0 스펙 문서(70쪽)에서 650항목 중 96개가 그랬다.
    ///
    /// 화면 몇 줄이 덜 정확한 것과 문서를 못 보는 것은 다른 일이다.
    #[test]
    fn a_layout_item_without_line_segments_still_parses() {
        let json = r#"{
            "pageCount": 1,
            "pages": [{
                "section": 0,
                "pageNumber": 1,
                "displayPage": 1,
                "bodyArea": {"x": 0.0, "y": 0.0, "width": 642.5, "height": 980.0},
                "columns": [{"index": 0, "items": [
                    {"kind": "fullParagraph", "paraIndex": 0, "lineSegs": {"count": 3}},
                    {"kind": "fullParagraph", "paraIndex": 1,
                     "lineSegs": {"count": 1, "first": {"columnStart": 600, "lineHeight": 1500,
                        "lineSpacing": 524, "segmentWidth": 47588, "textStart": 0},
                        "last": {"columnStart": 600, "lineHeight": 1500,
                        "lineSpacing": 524, "segmentWidth": 47588, "textStart": 0}}}
                ]}]
            }]
        }"#;
        let dp: DumpPages = serde_json::from_str(json).expect("세그먼트 없는 항목이 있어도 파싱");
        let items = &dp.pages[0].columns[0].items;
        assert_eq!(items.len(), 2);

        // 세그먼트가 없는 쪽: 단락 전체 줄 수로 떨어진다 (0 이나 패닉이 아니다)
        assert!(items[0].line_segs.as_ref().unwrap().first.is_none());
        assert_eq!(items[0].lines_on_page(), 3);

        // 있는 쪽은 그대로 읽는다
        let segs = items[1].line_segs.as_ref().unwrap();
        assert_eq!(segs.first.as_ref().unwrap().column_start, 600);
    }

    /// **보고된 그대로의 stderr** 로 진단이 서는가.
    ///
    /// 2026-10-10, WSL Ubuntu 20.04(glibc 2.31). 전에는 이 줄들을 버리고
    /// "rhwp 바이너리가 맞는지 확인하라" 고만 했는데, 바이너리는 맞았다 —
    /// sha256 까지 핀과 같았다.
    #[test]
    fn the_loader_complaint_becomes_a_diagnosis() {
        let stderr = "\
/home/k/hwp-cmd/vendor/rhwp/rhwp: /lib/x86_64-linux-gnu/libm.so.6: version `GLIBC_2.35' not found (required by /home/k/hwp-cmd/vendor/rhwp/rhwp)
/home/k/hwp-cmd/vendor/rhwp/rhwp: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.32' not found (required by /home/k/hwp-cmd/vendor/rhwp/rhwp)
/home/k/hwp-cmd/vendor/rhwp/rhwp: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found (required by /home/k/hwp-cmd/vendor/rhwp/rhwp)
";
        let msg = glibc_too_old(stderr).expect("glibc 문제로 알아봐야 한다");
        // **가장 높은 요구치를 말해야 한다.** 첫 줄의 2.35 나 둘째 줄의 2.32 를
        // 집으면, 그걸 맞춰 놓고 또 막힌다.
        assert!(msg.contains("glibc 2.39"), "{msg}");
        assert!(!msg.contains("2.32 이상"), "낮은 쪽을 집었다: {msg}");
        // 할 일이 적혀 있어야 한다 — 진단만 하고 길을 안 주면 반쪽이다
        assert!(msg.contains("build-rhwp.sh"), "{msg}");
        // sha256 이 맞았다는 사실을 말해야 한다. 안 그러면 "받다 깨졌나" 로 읽힌다
        assert!(msg.contains("sha256"), "{msg}");
    }

    /// 다른 이유로 실패한 것을 glibc 문제로 둘러대지 않는다.
    #[test]
    fn an_unrelated_failure_is_not_blamed_on_glibc() {
        assert!(glibc_too_old("rhwp: unknown subcommand `capabilities`").is_none());
        assert!(glibc_too_old("").is_none());
        // "not found" 가 있어도 GLIBC 가 아니면 아니다
        assert!(glibc_too_old("libfoo.so: not found").is_none());
    }
}
