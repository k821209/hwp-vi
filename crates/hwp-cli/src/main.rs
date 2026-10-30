//! `hwp` — 터미널 한글 문서 도구.
//!
//! R0 단계에서는 `doctor` 하나만 있다. 백엔드가 제대로 고정되어 있고
//! 호환되는지 사람이 한눈에 확인할 수 있어야, 그 위에 올릴 것을 믿을 수 있다.

mod api;
mod buffer;
mod pager;
mod run_pager;

/// 닫힌 파이프를 만나면 **조용히 끝낸다.**
///
/// Rust 는 시작할 때 `SIGPIPE` 를 무시하게 해 두므로, `hwp … | head` 에서 파이프가
/// 닫히면 `outln!` 이 EPIPE 로 **패닉한다**(exit 101 과 백트레이스). 파이프에
/// 꽂는 것이 이 도구의 요지인데 그게 터지면 안 된다.
///
/// `head` 가 할 일을 다 하고 떠난 것은 오류가 아니다. 그래서 0 으로 끝낸다 —
/// 다른 유닉스 도구가 `SIGPIPE` 로 죽을 때와 같은 결과다.
macro_rules! outln {
    () => { outln!("") };
    ($($arg:tt)*) => {{
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        if writeln!(out, $($arg)*).is_err() {
            std::process::exit(0);
        }
    }};
}

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use hwp_core::witness::Testimony;
use hwp_core::{Pin, Rhwp, ScreenDoc};
use std::path::PathBuf;

/// `hwp 파일.hwp` 하나로 끝나야 한다 — `vi 파일` 처럼.
///
/// 하위명령이 13개로 늘었는데 사람이 쓰는 것은 둘(`view`·`edit`)이고, 그 둘조차
/// 옵션 없이 떠야 한다. 그래서 **파일만 주면 고치기로 연다**(`-R` 이면 읽기만).
/// 나머지 명령은 파이프·스크립트에서 쓰는 것이고 사람이 외울 필요가 없다.
#[derive(Parser)]
#[command(
    name = "hwp",
    version,
    about = "터미널에서 한글(.hwp/.hwpx) 문서 다루기",
    after_help = "\
자주 쓰는 것은 둘이다.

  hwp 문서.hwp          고치기로 연다 (vi 처럼 — i 끼워넣기, ZZ 저장하고 나가기)
  hwp -R 문서.hwp       읽기만 (= hwp view 문서.hwp)

화면 안에서 바꾸는 것은 옵션이 아니라 키다.
  Ctrl-K  기호 넣기 (○ □ ※ ① …) — 글자 넣는 중에. 코퍼스가 쓰는 순서로 담았다
  F9      한자로 바꾸기 — 커서 앞의 한글 음절을 본다 (HWP 와 같은 키)
  z  그림 보기 방식 돌리기 (색 → 반블록 → 점자 → ASCII → 자리만 → 끔)
  v  그림 크게 보기 — 화면을 다 쓰고 **색으로** 그린다 (n/p 넘김, Esc 닫기)
  W  문서 단 폭 / 터미널 폭 바꾸기
  t  개요 보기          /  검색          ] [  쪽 넘김
:table 3x4  커서 단락에 **빈 표를 만든다** (메뉴에도 있다)
  T  표 안으로 — 커서가 칸을 넘나든다
     칸 이동은 화살표 · wasd · hjkl 셋 다 된다 (Tab 다음 칸, T 다음 표, Esc 나가기)
     i 또는 ↵ 로 **그 칸을 고친다** — 커서가 칸 안에 선다. Esc 로 끝, u 되돌리기
  ?  도움 — 아래 도움줄이 늘 모드별 키를 말하고, `?` 가 전부 펼친다
  m  메뉴 — 화살표로 골라 실행한다 (F10 도 된다)

도스 시절 키도 받는다 (보르랜드 IDE·워드스타 계보).
  F1 도움 · F2 저장 · F10 메뉴 · Alt-X 끝내기 · Ctrl-Y 단락 지우기
  Ctrl-K S/Q/Y/L/C · Ctrl-Q F/S/D · Ctrl-Ins · Shift-Del/Ins
  Ctrl-T 낱말 지우기 · Ctrl-Q Y 단락 끝까지 지우기
  Ctrl-K 1~0 책갈피 넣기 · Ctrl-Q 1~0 그 자리로 (HWP 도 같은 키)
  두 키 묶음은 Esc 로 버린다
  Ctrl-D·Ctrl-F 는 vi 와 겹쳐서 `:set keys=dos` 를 켤 때만 커서가 된다

저장과 나가기는 vi 를 따른다.
  :w              저장 (자가검증 — 통과 못 하면 원본을 건드리지 않는다)
  :saveas <경로>  다른 이름으로 — 그 뒤로 **새 이름**을 고친다
  :w <경로>       사본만 떨어뜨린다 — 고치는 것은 **여전히 원본**
                  있는 파일은 덮지 않는다. `:saveas! <경로>` 로 말해야 덮는다
  ZZ  저장하고 나가기 (= :wq :x)      ZQ  버리고 나가기 (= :q!)
  q   나가기 — 저장 안 한 편집이 있으면 거부한다
  :e! 디스크에서 다시 읽기 (쌓인 편집을 버린다)

파이프·스크립트용 명령 (기본 목록에서 숨겼다 — `hwp help <명령>` 으로 본다)
  json       문서를 JSON 한 덩이로 (LLM 에 넣을 때)
  toc        개요 트리 + 이 문서의 글머리 단계 사다리
  img        그림 목록 / 점자로 그리기
  cells      표를 격자로 (셀 좌표 확인)
  set-cell   표 칸에 값 쓰기 — 서식 채우기
  set-para   본문 단락 글자 바꾸기
  para       단락 넣기·빼기
  witness    구조를 독립 증인으로 세기 (대조용)
  api        **기계(LLM·스크립트)용 입구** — JSON 요청 하나, JSON 응답 하나
             읽기와 쓰기를 한 요청에 섞고, 쓰기는 한 번만 저장한다
             `hwp api --capabilities` 가 할 수 있는 일 전부를 말한다",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    /// 열 문서 (.hwp 또는 .hwpx). 하위명령 없이 이것만 줘도 된다.
    file: Option<PathBuf>,
    /// 고치지 않고 읽기만 한다
    #[arg(short = 'R', long)]
    read_only: bool,
    /// 열 때부터 두벌식 조합을 켠다 (터미널 IME 가 없을 때)
    #[arg(long)]
    dubeolsik: bool,
    /// 띄우지 않고 **한 프레임만 글로** 찍는다 — 예) `--frame 100x30`
    #[arg(long, value_name = "WxH")]
    frame: Option<String>,
    /// `--frame` 전에 키를 먹인다 — 예) `--keys "Tjl"`
    #[arg(long, value_name = "키들")]
    keys: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// 백엔드(rhwp)가 고정대로 붙어 있는지 진단한다
    Doctor,
    /// 읽기만 한다 (= `hwp -R 문서.hwp`)
    View {
        file: PathBuf,
        /// 띄우지 않고 **한 프레임만 글로** 찍는다 — 예) `--frame 100x30`
        ///
        /// 터미널 없이 화면을 확인하는 길이다. 문제를 알릴 때 이 출력을 그대로
        /// 붙이면 무엇이 그려졌는지 상대가 볼 수 있다.
        #[arg(long, value_name = "WxH")]
        frame: Option<String>,
        /// `--frame` 전에 **키를 먹인다** — 예) `--keys "Tll"` (표로 들어가 오른쪽 두 칸)
        ///
        /// 대화형 상태를 글로 확인하는 길이다. 글자는 그 키 그대로이고,
        /// `<tab>` `<stab>` `<esc>` `<cr>` `<down>` `<up>` 은 이름으로 쓴다.
        ///
        /// **꾸밈 표시(`«» * _ /`)는 한 칸씩 차지해 뒤를 민다.** 자리를 재려면
        /// 표시가 없는 줄로 보라 — 꾸밈이 어디 붙었는지 보는 용도다.
        #[arg(long, value_name = "키들")]
        keys: Option<String>,
    },
    /// 쪽 단위로 본문을 찍는다 (비대화형 — 페이저와 같은 화면 모델)
    Page {
        file: PathBuf,
        /// 이 쪽만 (1부터). 없으면 전부
        #[arg(long)]
        only: Option<u32>,
        /// 단락 번호를 왼쪽에 붙인다
        #[arg(long, default_value_t = true)]
        numbers: bool,
        /// 그림을 어떻게 — color | blocks | braille | ascii | placeholder | none
        ///
        /// `page` 는 파이프로 가는 글이라 **색을 쓰지 않는다**(ANSI 를 섞으면
        /// 받는 쪽이 폭을 잘못 센다). 색으로 보려면 화면에서 보라.
        #[arg(long, default_value = "blocks")]
        images: String,
    },
    /// 고친다 (= `hwp 문서.hwp`)
    Edit {
        file: PathBuf,
        /// 열 때부터 두벌식 조합을 켠다 (터미널 IME 가 없을 때)
        #[arg(long)]
        dubeolsik: bool,
    },
    /// LLM 이 읽기 쉬운 단일 JSON 객체로 내보낸다
    ///
    /// 기본 목록에서는 숨긴다 — 사람이 외울 명령은 `view`·`edit` 둘이다.
    /// 동작과 `hwp help` 는 그대로다.
    #[command(hide = true)]
    Json {
        file: PathBuf,
        /// text | outline | doc | full
        #[arg(long, default_value = "doc")]
        profile: String,
        /// 줄바꿈·들여쓰기 없이 한 줄로 (파이프·NDJSON 용)
        #[arg(long)]
        compact: bool,
    },
    /// 포맷을 바꾼다 — `.hwp` ↔ `.hwpx`, **증인이 대조한 뒤에만** 파일을 남긴다
    ///
    /// 방향은 목적지 확장자가 정한다. 백엔드의 `--verify` 는 자기 산출물을
    /// 자기가 다시 파싱하는 것이라, 우리 독립 판독기로 원본과 산출물의 글자·
    /// 꾸밈·표 칸·그림 자리를 대조하고 같을 때만 자리에 놓는다.
    #[command(hide = true)]
    Convert {
        /// 원본 (`.hwp` 또는 `.hwpx`)
        file: PathBuf,
        /// 만들 파일. 확장자가 방향을 정한다
        dest: PathBuf,
        /// 있는 파일을 덮는다
        #[arg(long)]
        overwrite: bool,
        /// 바꾸지 않고 무엇을 할지만 말한다
        #[arg(long)]
        dry_run: bool,
    },
    /// 표 셀에 값을 기록한다 (서식 채우기 — 비대화형, 자가검증 저장)
    ///
    /// 기본 목록에서는 숨긴다 — 사람이 외울 명령은 `view`·`edit` 둘이다.
    /// 동작과 `hwp help` 는 그대로다.
    #[command(hide = true)]
    SetCell {
        file: PathBuf,
        /// `표:행:열=값` 을 여러 번. 예) `0:1:1=홍길동`
        #[arg(required = true)]
        assignments: Vec<String>,
        /// 원본을 덮지 않고 이 파일로 쓴다
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// 본문 단락의 글자를 바꾼다 (비대화형, 자가검증 저장)
    ///
    /// 기본 목록에서는 숨긴다 — 사람이 외울 명령은 `view`·`edit` 둘이다.
    /// 동작과 `hwp help` 는 그대로다.
    #[command(hide = true)]
    SetPara {
        file: PathBuf,
        /// `구역:단락=새 글자` 를 여러 번. 예) `0:10=○ 소요예산: 금25,400,000원`
        #[arg(required = true)]
        assignments: Vec<String>,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// 단락을 넣거나 뺀다 (비대화형, 자가검증 저장)
    ///
    /// 기본 목록에서는 숨긴다 — 사람이 외울 명령은 `view`·`edit` 둘이다.
    /// 동작과 `hwp help` 는 그대로다.
    #[command(hide = true)]
    Para {
        file: PathBuf,
        /// 이 자리에 빈 단락을 넣는다 — `구역:단락`
        #[arg(long, value_name = "구역:단락")]
        insert: Vec<String>,
        /// 이 단락을 지운다 — `구역:단락`
        #[arg(long, value_name = "구역:단락")]
        delete: Vec<String>,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// 표를 격자로 보여준다 (셀 좌표 확인용)
    ///
    /// 기본 목록에서는 숨긴다 — 사람이 외울 명령은 `view`·`edit` 둘이다.
    /// 동작과 `hwp help` 는 그대로다.
    #[command(hide = true)]
    Cells {
        file: PathBuf,
        /// 이 표만 (없으면 전부)
        #[arg(long)]
        table: Option<u32>,
    },
    /// 문서의 그림을 나열하거나 점자로 그린다
    ///
    /// 기본 목록에서는 숨긴다 — 사람이 외울 명령은 `view`·`edit` 둘이다.
    /// 동작과 `hwp help` 는 그대로다.
    #[command(hide = true)]
    Img {
        file: PathBuf,
        /// 이 그림을 점자로 그린다 (1부터). 없으면 목록만
        #[arg(long)]
        dots: Option<usize>,
        /// 폭 (칸)
        #[arg(long, default_value_t = 60)]
        width: usize,
        /// 글자 — braille | blocks | ascii. 터미널 글꼴에 점자가 없으면 blocks
        #[arg(long, default_value = "braille")]
        glyphs: String,
        /// 이 그림을 파일로 꺼낸다
        #[arg(long)]
        extract: Option<usize>,
    },
    /// 개요 트리를 보여준다 (공문서 글머리로 계층을 만든다)
    ///
    /// 기본 목록에서는 숨긴다 — 사람이 외울 명령은 `view`·`edit` 둘이다.
    /// 동작과 `hwp help` 는 그대로다.
    #[command(hide = true)]
    Toc { file: PathBuf },
    /// 기계(LLM·스크립트)가 쓰는 입구 — JSON 요청 하나, JSON 응답 하나
    ///
    /// 사람이 키로 하는 일을 **같은 범위로** 할 수 있어야 한다. 그래서 읽기와
    /// 쓰기가 한 요청에 들어가고, 쓰기는 종류가 섞여도 **한 번만** 저장한다.
    /// 오류도 JSON 이다 — 산문을 파싱하게 두지 않는다.
    ///
    ///   hwp api --capabilities
    ///   echo '{"file":"a.hwp","read":["outline"]}' | hwp api
    ///   hwp api -r '{"file":"a.hwp","ops":[{"op":"set-cell","table":0,"row":1,"col":1,"text":"홍길동"}],"dryRun":true}'
    #[command(hide = true)]
    Api {
        /// 요청 JSON. 없으면 표준입력에서 읽는다.
        #[arg(short = 'r', long)]
        request: Option<String>,
        /// 문서 없이 **할 수 있는 일 전부**만 말한다 (`?` 도움의 기계 쪽)
        #[arg(long)]
        capabilities: bool,
        /// 줄바꿈·들여쓰기 없이 한 줄로
        #[arg(long)]
        compact: bool,
    },
    /// 문서 구조를 독립 증인으로 센다 (스트림·단락 수)
    ///
    /// 기본 목록에서는 숨긴다 — 사람이 외울 명령은 `view`·`edit` 둘이다.
    /// 동작과 `hwp help` 는 그대로다.
    #[command(hide = true)]
    Witness {
        /// .hwp 또는 .hwpx
        file: PathBuf,
        /// 단락 텍스트도 보여준다
        #[arg(long)]
        text: bool,
        /// 문단서식(정렬·들여쓰기)을 함께 찍는다 — 조판값과 대조용
        #[arg(long)]
        style: bool,
        /// 글자 꾸밈 구간을 함께 찍는다 (B 진하게 I 기울임 U 밑줄 S 취소선)
        #[arg(long)]
        deco: bool,
    },
}

fn main() -> Result<()> {
    // 비대화형 명령(`page`·`json` 등)은 터미널에 물어볼 수 없다. 환경변수로만
    // 받고, 없으면 좁은 쪽이다. 페이저는 띄울 때 터미널에 직접 물어본다.
    if let Ok(v) = std::env::var("HWP_AMBIGUOUS") {
        hwp_core::wrap::set_ambiguous_wide(matches!(v.trim(), "wide" | "2" | "cjk"));
    }
    let cli = Cli::parse();
    // `hwp 문서.hwp` — 하위명령 없이 파일만. 이게 기본 입구다.
    if let Some(file) = cli.file {
        return if cli.read_only {
            view(&file, cli.frame, cli.keys)
        } else {
            edit(&file, cli.dubeolsik, cli.frame, cli.keys)
        };
    }
    let Some(cmd) = cli.cmd else {
        // 인자가 없으면 쓰는 법을 보여준다. 빈 문서를 새로 만들지는 않는다 —
        // 서식 없는 HWP 를 지어내는 것은 이 도구가 할 일이 아니다.
        use clap::CommandFactory;
        Cli::command().print_help()?;
        outln!();
        return Ok(());
    };
    match cmd {
        Cmd::Doctor => doctor(),
        Cmd::Witness { file, text, style, deco } => witness(&file, text, style, deco),
        Cmd::Page { file, only, numbers, images } => page(&file, only, numbers, &images),
        Cmd::View { file, frame, keys } => view(&file, frame, keys),
        Cmd::Toc { file } => toc(&file),
        Cmd::Json { file, profile, compact } => json(&file, &profile, compact),
        Cmd::Edit { file, dubeolsik } => edit(&file, dubeolsik, None, None),
        Cmd::Convert { file, dest, overwrite, dry_run } => {
            convert_cmd(&file, &dest, overwrite, dry_run)
        }
        Cmd::SetCell { file, assignments, output } => set_cell(&file, &assignments, output),
        Cmd::Cells { file, table } => cells(&file, table),
        Cmd::SetPara { file, assignments, output } => set_para(&file, &assignments, output),
        Cmd::Para { file, insert, delete, output } => para_ops(&file, &insert, &delete, output),
        Cmd::Img { file, dots, width, extract, glyphs } => img(&file, dots, width, extract, &glyphs),
        Cmd::Api { request, capabilities, compact } => api_cmd(request, capabilities, compact),
    }
}

fn img(
    file: &std::path::Path,
    dots: Option<usize>,
    width: usize,
    extract: Option<usize>,
    glyphs: &str,
) -> Result<()> {
    use hwp_core::{dots as art, images};

    let glyphs = art::Glyphs::parse(glyphs)
        .ok_or_else(|| anyhow::anyhow!("glyphs 는 braille | blocks | ascii 다 (받은 값: {glyphs})"))?;

    let imgs = images::read(file)?;
    if imgs.is_empty() {
        outln!("{} — 그림이 없습니다", file.display());
        return Ok(());
    }

    if let Some(n) = extract {
        let img = imgs
            .get(n.saturating_sub(1))
            .ok_or_else(|| anyhow::anyhow!("그림 {n} 은 없다 (1~{})", imgs.len()))?;
        let name = std::path::Path::new(&img.name)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("image{n}"));
        std::fs::write(&name, &img.bytes)?;
        outln!("{name} ({} 바이트)", img.bytes.len());
        return Ok(());
    }

    if let Some(n) = dots {
        let img = imgs
            .get(n.saturating_sub(1))
            .ok_or_else(|| anyhow::anyhow!("그림 {n} 은 없다 (1~{})", imgs.len()))?;
        outln!("{}", img.label());
        let opts = art::Options { glyphs, ..Default::default() };
        match art::render_with(&img.bytes, width, 200, opts) {
            Some(a) => {
                for line in &a.lines {
                    outln!("{line}");
                }
                // **요청한 글리프가 아니라 쓴 글리프를 말한다.** 넓게 그리는
                // 터미널에서는 점자·반블록이 ASCII 로 바뀌는데, 말하지 않으면
                // 왜 거칠어졌는지 알 수 없다.
                if a.glyphs == glyphs {
                    outln!("  ({}×{} 칸 · {:?})", a.cols, a.rows, a.glyphs);
                } else {
                    outln!(
                        "  ({}×{} 칸 · {:?} — {:?} 는 터미널이 넓게 그려 표가 어긋나므로 바꿨다)",
                        a.cols, a.rows, a.glyphs, glyphs
                    );
                }
            }
            None => {
                for line in art::placeholder(&img.label(), width) {
                    outln!("{line}");
                }
                outln!("  (이 형식은 그릴 수 없어 자리만 표시했습니다)");
            }
        }
        return Ok(());
    }

    outln!("{} — 그림 {}개", file.display(), imgs.len());
    for img in &imgs {
        let at = match img.at {
            Some((s, p)) => format!("구역{s} 단락{p:<4}"),
            None => "표·글상자 안   ".into(),
        };
        // 정렬·글자처럼취급은 자리를 정하는 값이라 목록에서 보이게 둔다.
        // 화면에 왜 그 자리에 그려졌는지 이 줄로 확인할 수 있다.
        let how = match img.align {
            hwp_core::images::Align::Left => "왼쪽",
            hwp_core::images::Align::Center => "가운데",
            hwp_core::images::Align::Right => "오른쪽",
        };
        let how = if img.inline { format!("{how}·글자처럼") } else { how.to_string() };
        let how = match img.cell {
            Some(c) => format!("표{} {}행{}열", c.table, c.row + 1, c.col + 1),
            None => how,
        };
        outln!("  {:>3}. {at}  {} · {how}", img.index + 1, img.label());
    }
    outln!();
    outln!("점자로 보려면 `hwp img {} --dots 1`", file.display());
    Ok(())
}

fn parse_coord(spec: &str) -> Result<(u32, u32)> {
    let (sec, para) = spec
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("좌표는 `구역:단락` 이다 (받은 값: {spec})"))?;
    Ok((
        sec.trim().parse().map_err(|_| anyhow::anyhow!("구역이 수가 아닙니다: {sec}"))?,
        para.trim().parse().map_err(|_| anyhow::anyhow!("단락이 수가 아닙니다: {para}"))?,
    ))
}

fn para_ops(
    file: &std::path::Path,
    insert: &[String],
    delete: &[String],
    output: Option<PathBuf>,
) -> Result<()> {
    use hwp_core::edit::{EditLog, Op, save};
    if insert.is_empty() && delete.is_empty() {
        anyhow::bail!("--insert 또는 --delete 를 줘야 합니다");
    }
    let pin = Pin::load_default()?;
    let rhwp = Rhwp::discover(&pin)?;

    let target = match &output {
        Some(dest) => {
            std::fs::copy(file, dest)?;
            dest.clone()
        }
        None => file.to_path_buf(),
    };

    let mut log = EditLog::new();
    // 지우기를 **뒤에서 앞으로** 먼저 한다 — 앞을 지우면 뒤 좌표가 당겨지므로
    // 사용자가 준 좌표가 어긋난다.
    let mut dels: Vec<(u32, u32)> = delete.iter().map(|s| parse_coord(s)).collect::<Result<_>>()?;
    dels.sort_by(|a, b| b.cmp(a));
    for at in dels {
        log.push(Op::DeleteParagraph { at });
    }
    let mut ins: Vec<(u32, u32)> = insert.iter().map(|s| parse_coord(s)).collect::<Result<_>>()?;
    ins.sort_by(|a, b| b.cmp(a));
    for at in ins {
        log.push(Op::InsertParagraph { at });
    }

    for op in log.ops() {
        outln!("  {}", op.describe());
    }
    let report = save(&rhwp, &target, &log)?;
    outln!(
        "저장했습니다 — {} · 연산 {}개, 본문 {}단락{}",
        report.path.display(),
        report.ops_applied,
        report.paragraphs,
        report
            .backup
            .as_ref()
            .map(|b| format!(" · 백업 {}", b.display()))
            .unwrap_or_default()
    );
    Ok(())
}

/// `구역:단락=새 글자` — 단락 하나를 통째로 바꾼다.
///
/// 지우기 + 넣기 두 연산으로 번역한다. 비대화형 창구가 있어야 쓰기 경로를
/// 터미널 없이 시험할 수 있다.
fn set_para(file: &std::path::Path, assignments: &[String], output: Option<PathBuf>) -> Result<()> {
    use hwp_core::edit::{EditLog, Op, save};
    use hwp_core::offset;

    let pin = Pin::load_default()?;
    let rhwp = Rhwp::discover(&pin)?;

    let target = match &output {
        Some(dest) => {
            std::fs::copy(file, dest)?;
            dest.clone()
        }
        None => file.to_path_buf(),
    };

    let witness = Testimony::read(&target)?;
    let mut log = EditLog::new();
    for spec in assignments {
        let (coord, text) = spec
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("`구역:단락=새 글자` 꼴이어야 한다 (받은 값: {spec})"))?;
        let (sec, para) = coord
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("좌표는 `구역:단락` 이다 (받은 값: {coord})"))?;
        let section: u32 = sec.trim().parse().map_err(|_| anyhow::anyhow!("구역이 수가 아닙니다: {sec}"))?;
        let index: u32 = para.trim().parse().map_err(|_| anyhow::anyhow!("단락이 수가 아닙니다: {para}"))?;

        let current = witness
            .at(section, index)
            .map(|p| offset::visible(&p.text))
            .ok_or_else(|| anyhow::anyhow!("없는 단락입니다: 구역{section} 단락{index}"))?;
        let n = current.chars().count();
        if n > 0 {
            log.push(Op::DeleteText { at: (section, index), offset: 0, count: n });
        }
        if !text.is_empty() {
            log.push(Op::InsertText { at: (section, index), offset: 0, text: text.to_string() });
        }
        outln!("  구역{section} 단락{index}: {current:.40} → {text:.40}");
    }

    let report = save(&rhwp, &target, &log)?;
    outln!(
        "저장했습니다 — {} · 연산 {}개, 본문 {}단락 · 표 셀 {}개 확인{}",
        report.path.display(),
        report.ops_applied,
        report.paragraphs,
        report.cells,
        report
            .backup
            .as_ref()
            .map(|b| format!(" · 백업 {}", b.display()))
            .unwrap_or_default()
    );
    Ok(())
}

/// `표:행:열=값` 을 파싱한다.
fn parse_assignment(spec: &str) -> Result<hwp_core::edit::Op> {
    let (coord, text) = spec
        .split_once('=')
        .ok_or_else(|| anyhow::anyhow!("`표:행:열=값` 꼴이어야 한다 (받은 값: {spec})"))?;
    let parts: Vec<&str> = coord.split(':').collect();
    if parts.len() != 3 {
        anyhow::bail!("좌표는 `표:행:열` 세 수여야 한다 (받은 값: {coord})");
    }
    let n = |s: &str, what: &str| -> Result<usize> {
        s.trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("{what} 이 수가 아닙니다: {s}"))
    };
    Ok(hwp_core::edit::Op::SetCell {
        table: n(parts[0], "표 번호")? as u32,
        row: n(parts[1], "행")?,
        col: n(parts[2], "열")?,
        // `\n` 을 문단 경계로 받는다 — 셸에서 `$'가\n나'` 로 줄 수 있다.
        lines: text.split('\n').map(String::from).collect(),
    })
}

/// `hwp convert` — 포맷을 바꾼다.
fn convert_cmd(file: &std::path::Path, dest: &std::path::Path, overwrite: bool, dry_run: bool) -> Result<()> {
    if dry_run {
        let plan = hwp_core::convert::plan(file, dest)?;
        outln!("{} → {}", file.display(), dest.display());
        outln!("  백엔드         {}", plan["backend"].as_str().unwrap_or("?"));
        outln!(
            "  대조할 것      본문 {}단락 · 표 {}개 · 칸 {}개 · 그림 {}개",
            plan["willCompare"]["paragraphs"], plan["willCompare"]["tables"],
            plan["willCompare"]["cells"], plan["willCompare"]["images"]
        );
        outln!("  (바꾸지 않았습니다 — `--dry-run` 입니다)");
        return Ok(());
    }
    let pin = hwp_core::Pin::load_default()?;
    let rhwp = hwp_core::Rhwp::discover(&pin)?;
    let r = hwp_core::convert::convert(&rhwp, file, dest, overwrite)?;
    outln!("{} 로 바꿨습니다 ({})", r.path.display(), r.to);
    outln!(
        "  증인이 대조했습니다 — 본문 {}단락 · 표 칸 {}개 · 그림 {}개",
        r.paragraphs, r.cells, r.images
    );
    // **경고는 조용히 넘기지 않는다.** 막지 않은 차이도 사용자가 알아야 한다.
    for w in &r.warnings {
        outln!("  ⚠ {w}");
    }
    Ok(())
}

fn set_cell(file: &std::path::Path, assignments: &[String], output: Option<PathBuf>) -> Result<()> {
    use hwp_core::edit::{EditLog, save};
    let pin = Pin::load_default()?;
    let rhwp = Rhwp::discover(&pin)?;

    let mut log = EditLog::new();
    for spec in assignments {
        log.push(parse_assignment(spec)?);
    }

    // 다른 파일로 내보내라면 먼저 복사해 두고 거기에 쓴다 — 원본은 손대지 않는다.
    let target = match &output {
        Some(dest) => {
            std::fs::copy(file, dest)?;
            dest.clone()
        }
        None => file.to_path_buf(),
    };

    for op in log.ops() {
        outln!("  {}", op.describe());
    }
    let report = save(&rhwp, &target, &log)?;
    outln!(
        "저장했습니다 — {} · 연산 {}개, 본문 {}단락 · 표 셀 {}개 확인{}",
        report.path.display(),
        report.ops_applied,
        report.paragraphs,
        report.cells,
        report
            .backup
            .as_ref()
            .map(|b| format!(" · 백업 {}", b.display()))
            .unwrap_or_default()
    );
    Ok(())
}

fn cells(file: &std::path::Path, table: Option<u32>) -> Result<()> {
    let w = Testimony::read(file)?;
    outln!("{}  — 표 {}개", file.display(), w.tables.len());
    for t in &w.tables {
        if table.is_some_and(|n| n != t.index) {
            continue;
        }
        outln!();
        outln!("표 {}  {}×{}  셀 {}개", t.index, t.rows, t.cols, t.cells.len());
        for row in 0..t.rows {
            let mut line = String::new();
            for col in 0..t.cols {
                let text = t
                    .cell(row, col)
                    .map(|c| c.text.replace('\n', "⏎"))
                    .unwrap_or_else(|| "·".into());
                let shown: String = text.chars().take(16).collect();
                line.push_str(&format!("| {shown:16} "));
            }
            outln!("{row:>3} {line}|");
        }
    }
    Ok(())
}

fn edit(
    file: &std::path::Path,
    dubeolsik: bool,
    frame: Option<String>,
    keys: Option<String>,
) -> Result<()> {
    let mut app = build_app(file)?;
    app.editable = true;
    if dubeolsik {
        app.buf.ime = buffer::Ime::Dubeolsik;
    }
    // 키는 아래 도움줄이 말한다. 알림줄에는 **화면만 보고 알 수 없는 것**만.
    if dubeolsik {
        app.message = "두벌식 켜짐 — 터미널 IME 없이 한글을 조합합니다".into();
    }
    if let Some(k) = keys.as_deref() {
        feed_keys(&mut app, k)?;
    }
    match frame {
        Some(spec) => print_frame(&app, &spec),
        None => run_pager::run(&mut app),
    }
}

fn json(file: &std::path::Path, profile: &str, compact: bool) -> Result<()> {
    use hwp_core::json::{Profile, build};
    let Some(p) = Profile::parse(profile) else {
        anyhow::bail!("profile 은 text | outline | doc | full 이다 (받은 값: {profile})");
    };
    let pin = Pin::load_default()?;
    let rhwp = Rhwp::discover(&pin)?;
    let doc = build(&rhwp, file, p)?;
    let text = if compact {
        serde_json::to_string(&doc)?
    } else {
        serde_json::to_string_pretty(&doc)?
    };
    outln!("{text}");
    Ok(())
}

fn toc(file: &std::path::Path) -> Result<()> {
    use hwp_core::outline;
    let w = Testimony::read(file)?;
    let items: Vec<outline::Item> = w
        .body_paragraphs()
        .into_iter()
        .filter_map(|p| {
            p.index.map(|i| outline::Item {
                section: p.section,
                para: i,
                text: hwp_core::offset::visible(&p.text),
            })
        })
        .collect();
    let tree = outline::build(&items);
    let mut flat = Vec::new();
    outline::flatten(&tree, 0, &mut flat);
    outln!("{}  — 개요 {}줄 / 본문 {}단락", file.display(), flat.len(), items.len());
    for (depth, line) in &flat {
        let text: String = line.chars().take(78 - (depth * 2).min(40)).collect();
        outln!("{}{}", "  ".repeat(*depth), text);
    }

    // **이 문서가 쓰는 단계 사다리.** 에디터의 `>>`/`<<`(= Tab/Shift-Tab)가
    // 이 표를 따라 움직인다 — 들여쓰기 칸 수도 기호도 문서에서 읽은 것이다.
    let levels = outline::Levels::build(&items);
    if !levels.is_empty() {
        outln!();
        outln!("단계 사다리 (에디터에서 Tab / ⇧Tab 이 이 표를 따라갑니다)");
        for st in levels.steps() {
            // 세어지는 글머리는 대표값을 보여주면 오해를 산다 — 그 단계로
            // 옮겨도 번호는 바꾸지 않고 원래 것을 남긴다
            let how = if st.class.counts() {
                format!("번호 글머리 ({:?}, 예 `{}`) — 번호는 직접", st.class, st.marker)
            } else {
                format!("기호 `{}` 로 바꿈", st.marker)
            };
            outln!("  {}단계  들여쓰기 {}칸  {}", st.level, st.indent, how);
        }
    }
    Ok(())
}

fn parse_image_mode(s: &str) -> Result<pager::ImageMode> {
    pager::ImageMode::parse(s)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "images 는 placeholder | braille(=dots) | blocks | ascii | none 이다 \
                 (받은 값: {s}). 터미널 글꼴에 점자가 없으면 blocks 를 쓴다."
            )
        })
}

/// 텍스트 파일용 화면 모델. **쪽은 우리가 나눈 것이다.**
///
/// 텍스트 파일에는 쪽이 없다. 그래도 `]`·`[` 가 돌아야 하므로 일정한 줄 수로
/// 나누는데, 그것이 문서의 쪽이 아니라 우리 구분이라는 점을 상태줄이 말한다 —
/// 조판값과 추정을 섞어 보여 주지 않는 것이 이 프로젝트의 원칙이다.
///
/// 폭은 **터미널**을 따른다(`fit_term`). 문서가 정한 단 폭이 없으므로 재어
/// 볼 것도 없고, "측정 못 함" 이라고 말하면 재려고 했다는 거짓이 된다.
fn build_text_app(file: &std::path::Path) -> Result<pager::App> {
    /// 한 쪽에 담을 줄 수. 문서의 쪽이 아니라 우리 구분이다.
    const ROWS_PER_PAGE: usize = 50;

    let text = hwp_core::text::TextFile::read(file)?;
    let cols = crossterm::terminal::size()
        .map(|(w, _)| (w as usize).saturating_sub(6))
        .ok()
        .filter(|w| *w >= 8)
        .unwrap_or(hwp_core::screen::FALLBACK_COLUMN_CELLS);
    let mut app = pager::App::new(text.to_screen_doc(cols, ROWS_PER_PAGE), true);
    if text.mixed_eol {
        // 저장하면 한 가지로 통일된다 — 말하지 않으면 git diff 가 파일 전체를
        // 바꿈으로 보고하는 것을 열어 본 사람이 설명할 수 없다.
        app.message = format!(
            "줄끝이 섞여 있습니다 — 저장하면 {} 하나로 통일됩니다",
            text.eol.label()
        );
    }
    Ok(app)
}

/// 화면 모델을 세우고 그림까지 올린다. `view` 와 `page` 가 **같은 것**을 쓴다 —
/// 그래야 글로 확인한 것이 화면에서도 같다.
/// 보기든 고치기든 **같은 입구**를 지난다. 그림도 늘 싣는다.
///
/// 전에는 `edit` 가 그림을 아예 싣지 않았다 — 그래서 `hwp edit` 로 열면 문서에
/// 그림이 있는데 자리도 안 보였다. 옵션으로 켜야 하는 것이 아니라 기본이어야
/// 하는 쪽이다.
fn build_app(file: &std::path::Path) -> Result<pager::App> {
    // **텍스트는 백엔드를 쓰지 않는다.** 조판기도 증인도 필요 없고, 그래서
    // rhwp 가 깔려 있지 않은 기계에서도 텍스트는 열린다. 핀 검사를 여기서
    // 하면 텍스트를 열려는 사람이 쓰지도 않는 백엔드 때문에 막힌다.
    if check_openable(file)? == Openable::Text {
        return build_text_app(file);
    }
    let pin = Pin::load_default()?;
    let rhwp = Rhwp::discover(&pin)?;

    // **큰 문서는 조판에 오래 걸린다. 말없이 기다리게 두지 않는다.**
    //
    // 화면을 그리려면 백엔드 조판값이 필요하고, 그 비용은 쪽 수에 비례한다.
    // 아무 말도 없으면 사용자는 멈춘 줄 안다. 그래서 알린다.
    //
    // **걸리는 시간을 숫자로 약속하지 않는다.** 실측이 두 자리로 갈린다 —
    // 공개 시험 문서 big_file.hwp(10.6MB · 16,384쪽 · 147,456단락)는
    // `rhwp info` 혼자 62초 · 457MB 인데, 이 문턱(1MB)을 막 넘는 코퍼스 문서는
    // 조판까지 0.19~0.35초로 끝난다(s14.hwpx 2.8MB 0.19s · s02.hwp 2.1MB 0.35s).
    // 그래서 "수십 초" 라고 적어 두면 이 문턱에 걸리는 문서 대다수에게 거짓이
    // 된다. 문턱을 10MB 로 올려 숫자를 맞추는 쪽도 있었지만, 그러면 느린
    // 기계에서 2MB 가 멈춘 듯 보일 때 알려 줄 말이 없어진다. 문턱은 넉넉히
    // 두고 약속만 거둔다.
    //
    // stderr 로 보낸다 — stdout 은 파이프에 가는 값이다.
    let big = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0) > 1_000_000;
    if big {
        eprintln!(
            "{} 를 조판하는 중입니다… (큰 문서는 시간이 걸립니다)",
            file.display()
        );
    }
    let doc = ScreenDoc::build(&rhwp, file)?;
    let mut app = pager::App::new(doc, false);
    // **기본은 색이다.** 1비트 점 그림으로는 로고를 알아볼 수 없다는 보고를
    // 받았다(2026-10-12) — 실측으로 1315×345 로고가 34칸×4줄, 34×8점이었다.
    // 색은 같은 칸에서 픽셀이 둘이고 색 자체가 정보다. `NO_COLOR` 면 반블록.
    // 화면에서 `z` 로 돌리고, `v` 로 크게 본다.
    app.load_images(
        hwp_core::images::read(file).unwrap_or_default(),
        pager::ImageMode::default_mode(),
    );
    Ok(app)
}

fn view(file: &std::path::Path, frame: Option<String>, keys: Option<String>) -> Result<()> {
    let mut app = build_app(file)?;
    // 키는 **아래 도움줄**이 말한다 — 알림줄에 또 적으면 같은 말이 두 줄이다.
    // 알림이 비면 상태줄이 단 폭을 어디서 재었는지 보여준다.
    if let Some(k) = keys.as_deref() {
        feed_keys(&mut app, k)?;
    }
    match frame {
        Some(spec) => print_frame(&app, &spec),
        None => run_pager::run(&mut app),
    }
}

/// 키를 글자로 받아 먹인다 — `--frame` 과 짝이다.
///
/// 대화형 상태(표 안, 개요 보기, 검색 결과…)를 글로 확인할 길이 없으면,
/// "표에 들어갔는데 어느 칸인지 모르겠다" 같은 것을 주고받을 수가 없다.
/// 키를 넣는 쪽과 그리는 쪽이 모두 순수 함수라서 할 수 있다.
fn feed_keys(app: &mut pager::App, keys: &str) -> Result<()> {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut rest = keys;
    while !rest.is_empty() {
        let mut ctrl = false;
        let (code, used) = if let Some(end) = rest.strip_prefix('<').and_then(|r| r.find('>')) {
            let name = &rest[1..1 + end];
            let code = match name {
                "tab" => KeyCode::Tab,
                "stab" => KeyCode::BackTab,
                "esc" => KeyCode::Esc,
                "cr" | "enter" => KeyCode::Enter,
                "down" => KeyCode::Down,
                "up" => KeyCode::Up,
                "left" => KeyCode::Left,
                "right" => KeyCode::Right,
                // 지우기와 쪽 이동이 없어서 "칸 글자를 고쳐 넣는" 것을 이 길로
                // 시험할 수 없었다. 화면 없이 화면을 확인하는 길이 좁으면
                // 거기서 못 보는 버그가 생긴다.
                "bs" | "backspace" => KeyCode::Backspace,
                "del" | "delete" => KeyCode::Delete,
                "home" => KeyCode::Home,
                "end" => KeyCode::End,
                "pgdn" | "pagedown" => KeyCode::PageDown,
                "pgup" | "pageup" => KeyCode::PageUp,
                "space" => KeyCode::Char(' '),
                // `<c-r>` 처럼 Ctrl 조합. 이게 없어서 `Ctrl-R`(다시하기)를
                // 이 길로 시험할 수 없었다 — 그래서 셀 다시하기가 깨진 채
                // 있었다.
                n if n.len() == 3 && n.starts_with("c-") => {
                    let c = n.chars().nth(2).unwrap();
                    ctrl = true;
                    KeyCode::Char(c)
                }
                // `<f1>`~`<f12>`. 처음엔 셋만 적었다가 `<f9>`(한자)를 못 써서
                // 바로 걸렸다 — 쓸 수 있는 이름을 좁혀 두면 그 길로 못 보는
                // 버그가 생긴다.
                n if n.len() >= 2
                    && n.starts_with('f')
                    && n[1..].chars().all(|c| c.is_ascii_digit()) =>
                {
                    match n[1..].parse::<u8>() {
                        Ok(k @ 1..=12) => KeyCode::F(k),
                        _ => anyhow::bail!("기능키는 <f1>~<f12> 다 (받은 값: <{n}>)"),
                    }
                }
                other => anyhow::bail!(
                    "모르는 키 이름이다: <{other}>\n  \
                     쓸 수 있는 것: <tab> <stab> <esc> <cr> <bs> <del> <space> \
                     <down> <up> <left> <right> <home> <end> <pgdn> <pgup> <f1> <f2> <f10>"
                ),
            };
            (code, end + 2)
        } else {
            let c = rest.chars().next().expect("비어 있지 않다");
            (KeyCode::Char(c), c.len_utf8())
        };
        rest = &rest[used..];
        // **대화형 루프와 같은 함수를 쓴다.** 따로 적던 동안 이쪽이 화면을
        // 다시 그리지 않아서, 글자를 쳐도 안 나타나는 버그를 이 길로는 볼 수
        // 없었다 — 화면 없이 화면을 확인하는 도구가 거짓말을 했다.
        let mods = if ctrl { KeyModifiers::CONTROL } else { KeyModifiers::NONE };
        run_pager::step(app, KeyEvent::new(code, mods), 20);
    }
    Ok(())
}

/// 열기 전에 **사람이 읽을 수 있는 말로** 막는다.
///
/// 이게 없으면 없는 파일에 백엔드 날오류가 그대로 나온다
/// (`rhwp info … 실패 (exit 1): 오류: 파일을 읽을 수 없습니다 …`). 대표 입구가
/// `hwp 문서.hwp` 인데 거기서 그런 문장이 나오면 안 된다.
///
/// 확장자로 판단하지 않는다 — 증인이 내용(CFB `D0CF11E0…` / ZIP `504B`)으로
/// 컨테이너를 가리듯, 여기서도 앞 몇 바이트를 본다. `.bak` 이나 이름이 없는
/// 파일도 열 수 있어야 한다.
/// 열 수 있는 것의 종류. 텍스트는 **백엔드도 증인도 쓰지 않는 다른 경로**다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Openable {
    /// .hwp(CFB) · .hwpx(ZIP)
    Hwp,
    /// 일반 UTF-8 텍스트
    Text,
}

fn check_openable(file: &std::path::Path) -> Result<Openable> {
    use std::io::Read;

    if !file.exists() {
        anyhow::bail!("{} 가 없습니다.", file.display());
    }
    if file.is_dir() {
        anyhow::bail!("{} 는 폴더입니다. 문서 파일을 지정하세요.", file.display());
    }
    let mut head = [0u8; 8];
    let n = std::fs::File::open(file)
        .with_context(|| format!("{} 를 열 수 없습니다", file.display()))?
        .read(&mut head)
        .unwrap_or(0);
    let cfb = n >= 8 && head == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    let zip = n >= 2 && head[0] == 0x50 && head[1] == 0x4B;
    if cfb {
        // **배포용·암호 문서를 조용히 지나가게 두면 안 된다.**
        //
        // 배포용(읽기전용 배포) 문서는 본문이 `BodyText/` 가 아니라
        // `ViewText/` 에 구역별로 암호화되어 들어 있다. `BodyText/Section0`
        // 은 337바이트 껍데기만 남는다. 그래서 읽으면 **오류 없이 1단락**이
        // 나오고, 화면에는 "70쪽" 이라고 적힌 빈 쪽이 뜬다 — 실제로 공개된
        // 한글문서파일형식 5.0 스펙 문서(70쪽 619단락)에서 그렇게 됐다.
        let flags = hwp_core::witness::cfb_header_flags(file).unwrap_or(0);
        if flags & 0b100 != 0 {
            anyhow::bail!(
                "{} 는 **배포용** 문서입니다 — 본문이 ViewText 에 암호화되어 있어 \
                 그대로는 읽을 수 없습니다.\n  \
                 편집 가능한 형태로 먼저 바꾸세요:\n    \
                 rhwp convert {} 바꾼이름.hwp\n  \
                 (`hwp doctor` 가 rhwp 경로를 알려줍니다)",
                file.display(),
                file.display()
            );
        }
        if flags & 0b10 != 0 {
            anyhow::bail!("{} 는 암호가 걸린 문서입니다 — 암호를 풀어 저장한 뒤 열어 주세요.", file.display());
        }
    }
    if !cfb && !zip {
        // **한글 문서가 아니면 텍스트로 열어 본다.** 사용자 요청(2026-10-13):
        // `hwp PLAN.md` 가 거부되었고 간단한 텍스트는 소화해야 한다는 지적.
        // 두벌식 오토마톤이 터미널 IME 없이 한글을 칠 수 있는 유일한 길이라
        // 텍스트를 고칠 수 있다는 것이 쓸모가 크다.
        //
        // 판단은 **바이트로** 한다 — 확장자는 믿지 않는다. UTF-8 이 아니거나
        // NUL 이 있으면 열지 않고 이유를 말한다(`hwp_core::text` 참고).
        let bytes = std::fs::read(file)
            .with_context(|| format!("{} 를 읽을 수 없습니다", file.display()))?;
        return hwp_core::text::looks_like_text(&bytes)
            .map(|()| Openable::Text)
            // 겉말이 속을 잘못 말하지 않게 한다 — CP949 파일은 **텍스트이고**
            // 인코딩만 다르다. "텍스트도 아니다" 는 거짓이므로 아래 원인이
            // 무엇인지 말하게 두고, 겉말은 한글 문서가 아니라는 사실만 적는다.
            .with_context(|| {
                format!(
                    "{} 를 열 수 없습니다 — 한글 문서가 아닙니다 \
                     (.hwp 는 CFB, .hwpx 는 ZIP 입니다. 옛 .hwp(3.0 이하)·.hml 은 \
                     아직 읽지 못합니다).",
                    file.display()
                )
            });
    }
    Ok(Openable::Hwp)
}

/// 페이저 화면을 한 번 그려 글로 찍는다.
///
/// 터미널이 없는 곳에서 **화면에 실제로 무엇이 나오는지** 확인하는 유일한 길이다.
/// 그리는 쪽이 순수 함수라서 가능하다 — 쪽 경계, 그림, 표, 커서가 전부 여기 나온다.
fn print_frame(app: &pager::App, spec: &str) -> Result<()> {
    for line in frame_lines(app, spec)? {
        outln!("{line}");
    }
    Ok(())
}

/// 화면 한 판을 글 줄들로. `--frame` 과 `hwp api` 의 `frame` 이 같은 것을 본다.
fn frame_lines(app: &pager::App, spec: &str) -> Result<Vec<String>> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (w, h) = spec
        .split_once(['x', 'X', '*'])
        .ok_or_else(|| anyhow::anyhow!("--frame 은 `가로x세로` 다 (받은 값: {spec})"))?;
    let w: u16 = w.trim().parse().map_err(|_| anyhow::anyhow!("가로가 수가 아닙니다: {w}"))?;
    let h: u16 = h.trim().parse().map_err(|_| anyhow::anyhow!("세로가 수가 아닙니다: {h}"))?;

    let mut term = Terminal::new(TestBackend::new(w, h))?;
    term.draw(|f| pager::draw(f, app))?;
    let buf = term.backend().buffer().clone();

    // **꾸밈을 글자로 드러낸다.** 글자만 찍으면 진하게·반전이 보이지 않아서,
    // "표에 들어갔는데 어느 칸인지 모르겠다" 같은 것을 글로 확인할 수 없다.
    // 그게 `--frame` 의 존재 이유이므로 표시를 붙인다.
    //
    //   «…»  반전 (커서·지금 칸·찾은 글자)
    //   *…*  진하게        _…_  밑줄        /…/  기울임
    //
    // 머리글 줄은 전체가 반전이라 표시하지 않는다 — 온 줄이 «»로 감싸이면
    // 읽을 수가 없다.
    use ratatui::style::Modifier;
    let mark = |m: Modifier| -> Option<(&'static str, &'static str)> {
        if m.contains(Modifier::REVERSED) {
            Some(("«", "»"))
        } else if m.contains(Modifier::BOLD) {
            Some(("*", "*"))
        } else if m.contains(Modifier::UNDERLINED) {
            Some(("_", "_"))
        } else if m.contains(Modifier::ITALIC) {
            Some(("/", "/"))
        } else {
            None
        }
    };

    let mut lines = Vec::new();
    for y in 0..buf.area.height {
        let mut line = String::new();
        let mut open: Option<(&str, &str)> = None;
        let mut x = 0u16;
        while x < buf.area.width {
            let cell = &buf[(x, y)];
            let sym = cell.symbol();
            let want = if y == 0 { None } else { mark(cell.style().add_modifier) };
            if want != open {
                if let Some((_, close)) = open {
                    line.push_str(close);
                }
                if let Some((start, _)) = want {
                    line.push_str(start);
                }
                open = want;
            }
            line.push_str(sym);
            // 2칸 글자는 다음 칸을 함께 차지한다 — 폭만큼 건너뛰지 않으면
            // "과 업 지 시 서" 가 된다
            x += hwp_core::wrap::display_width(sym).max(1) as u16;
        }
        if let Some((_, close)) = open {
            line.push_str(close);
        }
        lines.push(line.trim_end().to_string());
    }
    Ok(lines)
}

fn page(file: &std::path::Path, only: Option<u32>, numbers: bool, images: &str) -> Result<()> {
    // `page` 는 파이프·스크립트용이라 그림 방식을 인자로 받는다 — 화면이
    // 없으니 키로 바꿀 수가 없다.
    let mut app = build_app(file)?;
    let mode = parse_image_mode(images)?;
    let imgs = std::mem::take(&mut app.images);
    app.load_images(imgs, mode);
    let doc = &app.doc;
    let w = doc.column.cells;
    outln!("{}  [{}]  {}쪽 · 본문 {}단락", doc.path, doc.format, doc.page_count, doc.para_count);
    outln!("단 폭 {}", doc.column.provenance());
    outln!("쪽 경계와 줄 수는 rhwp 조판값, 접히는 위치는 이 프로그램 계산");
    if !app.images.is_empty() {
        outln!("그림 {}개 — {}", app.images.len(), mode.label());
    }

    let gut = |on: bool| if on { " ".repeat(pager::GUTTER) } else { String::new() };
    let mut show = only.is_none();
    for row in &app.rows {
        match row {
            pager::Row::PageRule { number, total, printed, .. } => {
                show = only.is_none_or(|n| n == *number);
                if !show {
                    continue;
                }
                let mark = match printed {
                    Some(p) if p != number => format!(" (문서 표기 {p}쪽)"),
                    _ => String::new(),
                };
                let label = format!(" {number} / {total} 쪽{mark} ");
                let pad = w.saturating_sub(label.chars().count()) / 2;
                outln!();
                outln!(
                    "{}{}{}",
                    hwp_core::wrap::box_h().repeat(pad),
                    label,
                    hwp_core::wrap::box_h().repeat(w.saturating_sub(pad + label.chars().count()))
                );
            }
            _ if !show => {}
            pager::Row::Line { section, para, leads, continued, text, .. } => {
                // view 와 같은 함수를 쓴다 — 갈라지면 한쪽에만 표시가 빠진다
                let gutter = pager::gutter_for(&app, *section, *para, *leads, *continued, numbers);
                // 인라인 컨트롤 표시는 지운다 — 그림은 아래 블록으로 그린다
                let clean: String = hwp_core::offset::visible(text);
                outln!("{gutter}{clean}");
            }
            pager::Row::Continues => outln!("{}\u{22ef} 다음 쪽에서 이어짐", gut(numbers)),
            pager::Row::Grid { line, .. } | pager::Row::Art { line, .. } => {
                outln!("{}{line}", gut(numbers))
            }
        }
    }
    Ok(())
}

fn doctor() -> Result<()> {
    let pin = Pin::load_default()?;
    outln!("hwp doctor");
    outln!();
    outln!("  핀 (rhwp-pin.toml)");
    outln!("    rhwp 버전        {}", pin.version);
    outln!("    JSON 스키마      {}.x", pin.schema_major());
    outln!("    이 플랫폼        {}", Pin::host_target());
    match pin.artifact_for_host() {
        Ok(a) => {
            outln!("    산출물           {}", a.file);
            outln!("    sha256           {}", a.sha256);
        }
        Err(e) => outln!("    산출물           ⚠ {e}"),
    }
    outln!("    필수 명령        {}개", pin.required_commands.len());
    outln!();

    match Rhwp::discover(&pin) {
        Ok(r) => {
            let caps = r.capabilities();
            outln!("  백엔드");
            outln!("    경로             {}", r.bin().display());
            outln!("    tool/version     {} {}", caps.tool, caps.version);
            outln!("    schemaVersion    {}", caps.schema_version);
            outln!("    명령 표면        {}개", caps.commands.len());
            outln!();
            outln!("  ✅ 게이트 통과 — 이 백엔드를 쓸 수 있습니다.");
            outln!();
            report_corpus();
            Ok(())
        }
        Err(e) => {
            outln!("  ❌ 게이트 거부");
            outln!();
            for line in format!("{e}").lines() {
                outln!("    {line}");
            }
            outln!();
            outln!("  조용히 오동작하지 않기 위해 거부한다 (PLAN.md §8-5).");
            std::process::exit(2);
        }
    }
}

/// 회귀 코퍼스가 있는지 알려준다.
///
/// `cargo test` 는 코퍼스가 없으면 그 시험들을 건너뛰는데, **cargo 가 건너뜀
/// 메시지를 숨긴다**(`--nocapture` 없이는 안 보인다). 그래서 통과처럼 보인다.
/// 사람이 설치를 확인하는 자리인 여기서 말해 주는 것이 맞다.
fn report_corpus() {
    // 실행 파일 자리에서 거슬러 올라가 찾는다 — `CARGO_MANIFEST_DIR` 은
    // 빌드한 사람의 절대 경로를 실행 파일에 박는다(hwp_core::rhwp 의
    // `workspace_root` 주석 참고).
    let dir = hwp_core::rhwp::source_tree_root().map(|p| p.join("samples"));
    let count = dir
        .as_ref()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    matches!(
                        e.path().extension().and_then(|s| s.to_str()),
                        Some("hwp") | Some("hwpx")
                    )
                })
                .count()
        })
        .unwrap_or(0);

    outln!();
    outln!("  터미널 폭 자가확인 — 아래 두 줄의 **오른쪽 끝이 맞는지** 보세요");
    outln!("    ┌────┬────┐");
    outln!("    │가나│다라│");
    outln!("    └────┴────┘");
    outln!(
        "    어긋나면 `HWP_AMBIGUOUS=wide hwp …` 또는 화면에서 `:set amb=wide`.\n    \
         지금 기준: {} (페이저는 띄울 때 터미널에 직접 물어본다)",
        if hwp_core::wrap::ambiguous_wide() { "2칸 (CJK)" } else { "1칸" }
    );
    outln!();
    // **넣을 글자 표가 붙어 있는지 말한다.** 기호와 한자는 바이너리에 박아
    // 넣으므로 "왜 F9 가 안 되지" 를 여기서 가를 수 있어야 한다.
    outln!("  넣을 글자");
    let syms: usize =
        hwp_core::symbols::GROUPS.iter().map(|(_, it)| it.len()).sum();
    outln!(
        "    기호           {}묶음 {syms}개 — `Ctrl-K` (코퍼스가 쓰는 순서)",
        hwp_core::symbols::GROUPS.len()
    );
    outln!(
        "    한자           {}음절 — `F9` (libhangul hanja.txt, BSD-3)",
        hwp_core::hanja::syllable_count()
    );
    outln!();
    outln!("  회귀 코퍼스 (samples/)");
    if count == 0 {
        outln!("    문서 없음 — 코퍼스 시험은 **건너뛴다**");
        outln!("    (cargo 가 건너뜀 메시지를 숨기므로 `cargo test` 는 통과처럼 보인다)");
        outln!("    .hwp·.hwpx 를 samples/ 에 넣으면 그 문서들로 전수 대조가 돌아갑니다.");
    } else {
        // **코퍼스가 합성인지 실무 문서인지 말한다 — 섞어 보고하지 않는다.**
        //
        // 공개 저장소는 실무 공문서를 담을 수 없어(사람 이름·기관 내용) 우리가
        // 만든 문서로 돈다. 그 코퍼스로는 실무 문서의 괴상함을 못 잰다 — 한컴
        // 글꼴의 사용자 영역 글립, U+2007, 글머리 조합 42가지, 구역 여럿,
        // 배포용 암호화. **결함을 찾아 준 것이 대부분 그 괴상함이었다.**
        //
        // 그래서 "시험 266개 통과" 가 코퍼스에 따라 다른 뜻이라는 것을 여기서
        // 말한다. 말하지 않으면 통과한 수만 보고 같은 보증으로 읽는다.
        let manifest = hwp_core::rhwp::source_tree_root()
            .map(|p| p.join("samples/MANIFEST.json"))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        let synthetic = manifest.contains("\"synthetic\"");
        outln!("    문서 {count}개 — 코퍼스 시험이 이 문서들로 돌아갑니다");
        if synthetic {
            let off = manifest
                .split("\"inactive_guards\"")
                .nth(1)
                .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).find(|x| !x.is_empty()))
                .unwrap_or("?")
                .to_string();
            outln!("    종류           **합성** — `scripts/make-corpus.sh` 가 만든 것");
            outln!(
                "    못 재는 것     실무 문서 전용 시험 {off}개는 **돌지 않습니다**\n                     (무엇을 덮고 못 덮는지는 samples/MANIFEST.json 에 적혀 있습니다)"
            );
        } else {
            outln!("    종류           실무 문서 — 전수 대조가 전부 돌아갑니다");
        }
    }
}

/// 글자 수로 자르고, **자랐으면 `…` 를 붙인다.**
fn clip(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().nth(n).is_some() {
        out.push('…');
    }
    out
}

fn witness(
    file: &std::path::Path,
    show_text: bool,
    show_style: bool,
    show_deco: bool,
) -> Result<()> {
    let t = Testimony::read(file)?;
    outln!("{}", file.display());
    outln!("  컨테이너   {:?}", t.container);
    outln!("  부품       {}개", t.parts.len());
    outln!(
        "  단락       본문 {}개 (표 셀 포함 전체 {}개)",
        t.body_count(),
        t.paragraphs.len()
    );
    if show_deco {
        outln!();
        outln!("글자 꾸밈 — B 진하게 · I 기울임 · U 밑줄 · S 취소선");
        let mut n = 0usize;
        for p in t.body_paragraphs() {
            let Some(i) = p.index else { continue };
            if p.runs.is_empty() {
                continue;
            }
            n += 1;
            let visible: Vec<char> = p.text.chars().collect();
            outln!("  {}:{i}", p.section);
            for r in &p.runs {
                let mut mark = String::new();
                if r.deco.bold {
                    mark.push('B');
                }
                if r.deco.italic {
                    mark.push('I');
                }
                if r.deco.underline {
                    mark.push('U');
                }
                if r.deco.strike {
                    mark.push('S');
                }
                let end = (r.start + r.len).min(visible.len());
                let seg: String = visible[r.start.min(visible.len())..end].iter().collect();
                let seg = clip(&seg, 40);
                outln!("     {:<4} {:>3}+{:<3} {seg:?}", mark, r.start, r.len);
            }
        }
        outln!();
        outln!("  꾸밈이 있는 단락 {n}개");
    }
    if show_text || show_style {
        outln!();
        // **좌표를 그대로 찍는다 — `구역:단락`.**
        //
        // 전에는 목록 순번(`[ 12]`)을 찍었는데 그것은 구역을 가로질러 센 수라
        // 편집 좌표가 아니다. 구역이 여럿인 문서에서 그 번호를 그대로
        // `set-para 0:12` 에 넣으면 "없는 단락이다" 가 나온다 — 공개 스펙
        // 문서(구역 6개)에서 실제로 그랬다. 찍는 번호가 곧 쓰는 번호여야 한다.
        for p in t.body_paragraphs() {
            let Some(i) = p.index else { continue };
            let at = format!("{}:{i}", p.section);
            let head = if show_style {
                // 들여쓰기는 HWPUNIT. 1pt = 100.
                format!(
                    "  {at:>9}  {:>9} 첫{:>6} 이후{:>6}",
                    format!("{:?}", p.style.align),
                    p.style.first_indent,
                    p.style.rest_indent
                )
            } else {
                format!("  {at:>9}")
            };
            if show_text {
                // **잘랐으면 잘랐다고 말한다.** 60자에서 말없이 끊던 동안,
                // 이 출력으로 저장 결과를 대조하던 내 시험이 "예측과 실제가
                // 다르다" 를 60자 넘는 단락마다 쏟아냈다 — 실제로는 같았다.
                // 조용히 자른 출력은 틀린 것을 가르친다.
                let s = clip(&p.text, 60);
                outln!("{head} {s}");
            } else {
                outln!("{head}");
            }
        }
    }
    Ok(())
}

/// `hwp api` — JSON 요청 하나를 받아 JSON 응답 하나를 돌려준다.
///
/// **오류도 JSON 이다.** 사람은 상태줄의 한 줄을 읽고 다시 누르지만, 기계는
/// 오류 봉투를 **한 곳에서** 만든다.
///
/// 세 자리(빈 요청·JSON 파싱 실패·실행 실패)가 따로 봉투를 적고 있었고, 셋 다
/// `schemaVersion` 을 빠뜨렸다 — 성공 응답에는 있고 오류에만 없었다(실측).
/// 버전으로 분기하는 클라이언트가 오류를 "알 수 없는 스키마" 로 처리하거나,
/// `schemaVersion` 유무로 성공을 판정하게 된다. 응답 꼴을 바꿀 때 오류 봉투가
/// 버전 축에 올라타 있지 않으면 받는 쪽이 조용히 어긋난다.
fn err_envelope(error: &str, hint: Option<&str>) -> serde_json::Value {
    let mut o = serde_json::Map::new();
    o.insert("schemaVersion".into(), serde_json::json!(api::SCHEMA_VERSION));
    o.insert("ok".into(), serde_json::json!(false));
    o.insert("error".into(), serde_json::json!(error));
    if let Some(h) = hint {
        o.insert("hint".into(), serde_json::json!(h));
    }
    serde_json::Value::Object(o)
}

/// 그 산문을 파싱해야 한다. 그래서 실패해도 stdout 에 `{"ok":false,…}` 를
/// 찍고 1 로 끝낸다 — 끝 상태로 성공·실패를 가르고, 이유는 읽을 수 있게.
fn api_cmd(request: Option<String>, capabilities: bool, compact: bool) -> Result<()> {
    use std::io::Read;

    let emit = |v: &serde_json::Value| {
        let s = if compact {
            serde_json::to_string(v)
        } else {
            serde_json::to_string_pretty(v)
        }
        .unwrap_or_else(|e| format!("{{\"ok\":false,\"error\":\"응답을 만들 수 없다: {e}\"}}"));
        outln!("{s}");
    };

    // `--capabilities` 만 주면 문서가 필요 없다 — "무엇을 할 수 있나" 는
    // 문서를 열기 전에 묻는 질문이다.
    let raw = match request {
        Some(s) => s,
        None if capabilities => "{}".to_string(),
        None => {
            let mut buf = String::new();
            // **여기서 `?` 를 쓰면 안 된다.** 비UTF-8 이 들어오면 `?` 가
            // JSON 봉투를 만드는 이 함수를 지나쳐 `main` 의 anyhow 로 빠지고,
            // stdout 이 **비고** 평문이 stderr 로 나간다(실측). "stdout 은 늘
            // JSON" 을 믿고 `json.loads(stdout)` 하는 래퍼가 거기서 죽는다 —
            // 깨진 인코딩과 잘린 파이프는 에이전트 파이프라인에서 흔하다.
            if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                emit(&err_envelope(
                    &format!("표준입력을 읽을 수 없습니다: {e}"),
                    Some("요청은 UTF-8 JSON 이어야 합니다"),
                ));
                std::process::exit(1);
            }
            if buf.trim().is_empty() {
                emit(&err_envelope(
                    "요청이 비었습니다",
                    Some(
                        "표준입력으로 JSON 을 주거나 `-r '{…}'` 로 주거나, \
                         `--capabilities` 로 할 수 있는 일을 먼저 물어라",
                    ),
                ));
                std::process::exit(1);
            }
            buf
        }
    };

    let mut req: api::Request = match serde_json::from_str(&raw) {
        Ok(r) => r,
        Err(e) => {
            emit(&err_envelope(
                &format!("요청 JSON 을 읽을 수 없다: {e}"),
                Some("`hwp api --capabilities` 가 받는 칸을 전부 말합니다"),
            ));
            std::process::exit(1);
        }
    };
    if capabilities {
        req.capabilities = true;
    }

    match api::run(req) {
        Ok(v) => {
            emit(&v);
            Ok(())
        }
        Err(e) => {
            // 원인 사슬을 전부 담는다. 첫 줄만 주면 "없는 단락이다" 같은
            // 바깥 말만 남고 어느 좌표에서 막혔는지가 사라진다.
            let chain: Vec<String> = e.chain().map(|c| c.to_string()).collect();
            let mut env = err_envelope(&chain.first().cloned().unwrap_or_default(), None);
            if let Some(o) = env.as_object_mut() {
                o.insert("because".into(), serde_json::json!(&chain[1.min(chain.len())..]));
            }
            emit(&env);
            std::process::exit(1);
        }
    }
}
