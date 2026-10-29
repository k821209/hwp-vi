//! 읽기전용 페이저 — R1.
//!
//! 화면 모델([`ScreenDoc`])을 행 목록으로 펴고, 그 위에 뷰포트를 올린다.
//! 그리는 부분은 순수 함수로 떼어 두었다 — 터미널 없이 `TestBackend` 로
//! 프레임을 찍어 검증할 수 있어야 하기 때문이다.

use crate::buffer::{Buffer, Ime};
use hwp_core::dots::Glyphs;
use hwp_core::ScreenDoc;
use hwp_core::wrap::display_width;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

/// 색. **색은 거들 뿐 뜻을 지지 않는다.**
///
/// 흑백 터미널, 색이 다른 테마, 색을 못 보는 사람이 모두 같은 정보를 읽어야
/// 하므로 **색만으로 말하지 않는다** — 쪽 경계는 선과 글자로, 지금 칸은 반전으로,
/// 편집 중은 글자로 이미 말한다. 색은 그 위에 얹는다.
///
/// `NO_COLOR` 가 있으면 전부 끈다 ([no-color.org](https://no-color.org) 의
/// 관례다 — 값은 보지 않고 있는지만 본다).
pub mod theme {
    use ratatui::style::{Color, Style};

    fn plain() -> bool {
        static PLAIN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *PLAIN.get_or_init(|| std::env::var_os("NO_COLOR").is_some())
    }

    /// **색을 쓸 수 있나.** 그림을 색으로 그릴지 가를 때 쓴다 —
    /// `NO_COLOR` 면 1비트 점 그림으로 간다.
    pub fn colored() -> bool {
        !plain()
    }

    fn c(color: Color) -> Style {
        if plain() { Style::new() } else { Style::new().fg(color) }
    }

    /// 머리글 — 파일명과 자리.
    pub fn header() -> Style {
        if plain() {
            Style::new().reversed()
        } else {
            Style::new().fg(Color::Black).bg(Color::Cyan)
        }
    }

    /// 왼쪽 여백의 단락 번호. 글자보다 뒤로 물러나야 한다.
    pub fn gutter() -> Style {
        c(Color::DarkGray).dim()
    }

    /// 쪽 경계 자.
    pub fn page_rule() -> Style {
        c(Color::Blue)
    }

    /// 표 테두리 — 글자보다 뒤로.
    pub fn border() -> Style {
        c(Color::DarkGray)
    }

    /// 그림을 글자로 그린 것.
    pub fn art() -> Style {
        c(Color::Green)
    }

    /// 그림 설명 줄.
    pub fn caption() -> Style {
        c(Color::DarkGray).dim()
    }

    /// 찾은 글자.
    pub fn hit() -> Style {
        if plain() {
            Style::new().reversed()
        } else {
            Style::new().fg(Color::Black).bg(Color::Yellow)
        }
    }

    /// 아래 도움줄의 **키**.
    pub fn key() -> Style {
        if plain() {
            Style::new().bold()
        } else {
            Style::new().fg(Color::Yellow).bold()
        }
    }

    /// 아래 도움줄의 설명.
    pub fn hint() -> Style {
        c(Color::DarkGray)
    }

    /// 지금 모드 이름.
    pub fn mode() -> Style {
        if plain() {
            Style::new().bold()
        } else {
            Style::new().fg(Color::Black).bg(Color::Green).bold()
        }
    }

    /// 저장 안 한 편집이 있다는 알림.
    pub fn dirty() -> Style {
        if plain() {
            Style::new().bold()
        } else {
            Style::new().fg(Color::Black).bg(Color::Yellow).bold()
        }
    }

    /// 개요 보기의 깊이별 색 — 깊이가 눈에 바로 들어오게.
    pub fn outline(depth: usize) -> Style {
        const WHEEL: [Color; 4] = [Color::Cyan, Color::Green, Color::Yellow, Color::Magenta];
        c(WHEEL[depth % WHEEL.len()])
    }
}

/// **`Ctrl-D`·`Ctrl-F` 를 도스 커서로 쓸 것인가** (`:set keys=dos`).
///
/// 도스 시절 단축키는 거의 다 vi 와 겹치지 않아 모드 없이 그냥 받는다. 딱 둘이
/// 겹친다 — 워드스타는 `Ctrl-D` 를 "글자 오른쪽", `Ctrl-F` 를 "낱말 오른쪽"
/// 으로 쓰고, vi 는 각각 반 화면·한 화면 아래로 쓴다. 양쪽을 다 만족시킬 수
/// 없으므로 **이 둘만** 설정으로 가른다.
static DOS_KEYS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_dos_keys(on: bool) {
    DOS_KEYS.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn dos_keys() -> bool {
    DOS_KEYS.load(std::sync::atomic::Ordering::Relaxed)
}

/// 뷰포트에 쌓이는 한 행.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// 쪽 경계. rhwp 조판값이다.
    PageRule { number: u32, total: u32, section: u32, printed: Option<u32> },
    /// 본문 한 줄.
    Line {
        /// 이 줄이 속한 단락의 편집 좌표
        section: u32,
        para: Option<u32>,
        /// 단락의 첫 줄인가 (왼쪽에 좌표를 붙일 자리)
        leads: bool,
        /// 앞 쪽에서 이어진 단락인가
        continued: bool,
        text: String,
        /// `text` 의 글자마다 꾸밈 한 바이트. **비어 있으면 전부 민짜**다 —
        /// 코퍼스 단락의 77% 가 그렇고, 그 경우 비용을 내지 않는다.
        deco: Vec<u8>,
    },
    /// 쪽 끝에서 단락이 이어짐을 알리는 줄
    Continues,
    /// 표 — 미리 그려 둔 한 줄.
    Grid {
        /// 몇 번째 표인가 (`doc.tables` 의 자리)
        table: usize,
        line: String,
        /// 그 표 안에서 몇 번째 줄인가 — 칸을 뒤집어 보여줄 때 쓴다
        y: usize,
    },
    /// 그림 — 미리 그려 둔 한 줄.
    ///
    /// 프레임마다 디더를 돌리면 안 된다. 코퍼스에는 압축 풀린 3.8MB BMP 도
    /// 있어서 매번 그리면 페이저가 멈춘다. 그래서 한 번 그려 행으로 펴 둔다.
    Art {
        image: usize,
        line: String,
        caption: bool,
        /// **칸마다 색.** 비어 있으면 1비트 점 그림이다(`NO_COLOR`·설명 줄).
        ///
        /// 글자 줄(`line`)을 그대로 두고 색만 더한다. 그래야 폭을 재는 쪽이
        /// 하나로 남는다 — 색 때문에 폭 계산을 또 적으면 어긋난다.
        colors: Vec<hwp_core::dots::ColorCell>,
    },
}

/// 왼쪽 여백(좌표 칸) 폭.
pub const GUTTER: usize = 6;

/// 그림을 어떻게 보여줄지.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMode {
    /// 자리와 정체만 네모로 — 늘 동작한다
    Placeholder,
    /// **색으로** 그린다 — 반블록 `▀` 에 글자색·바탕색을 따로 준다.
    ///
    /// 한 칸이 색 픽셀 둘이다. 1비트 점 그림은 로고를 알아볼 수 없었다
    /// (실측: 1315×345 로고가 34칸×4줄 = 34×8점). `NO_COLOR` 면 쓰지 않는다.
    Color,
    /// 그림을 글자로 그린다. 어떤 글자로 그릴지는 [`Glyphs`] 가 정한다.
    Art(Glyphs),
    /// 아무것도 그리지 않는다
    None,
}

impl ImageMode {
    /// 사람이 읽는 이름.
    pub fn label(self) -> &'static str {
        match self {
            Self::Placeholder => "자리만",
            Self::Color => "색",
            Self::Art(Glyphs::Braille) => "점자",
            Self::Art(Glyphs::Blocks) => "반블록",
            Self::Art(Glyphs::Ascii) => "ASCII",
            Self::None => "끔",
        }
    }

    /// 화면 안에서 `z` 로 돌리는 순서.
    ///
    /// **색에서 시작한다.** 1비트 점 그림으로는 로고를 알아볼 수 없다는
    /// 보고를 받았다(2026-10-12) — 실측으로 1315×345 로고가 34칸×4줄,
    /// 34×8점이었다. 색은 같은 칸에서 픽셀이 둘이고 **색 자체가 정보**다.
    ///
    /// 색을 못 쓰는 터미널(`NO_COLOR`)에서는 반블록으로 시작한다 — 어떤
    /// 글꼴에나 있는 글자다. 점자는 해상도가 4배지만 글꼴에 없으면 네모로
    /// 보이고, 그게 기본이면 처음 열어 본 사람이 고장으로 오해한다.
    pub fn next(self) -> Self {
        match self {
            Self::Color => Self::Art(Glyphs::Blocks),
            Self::Art(Glyphs::Blocks) => Self::Art(Glyphs::Braille),
            Self::Art(Glyphs::Braille) => Self::Art(Glyphs::Ascii),
            Self::Art(Glyphs::Ascii) => Self::Placeholder,
            Self::Placeholder => Self::None,
            Self::None => Self::default_mode(),
        }
    }

    /// 열 때 쓰는 방식 — 색을 쓸 수 있으면 색.
    pub fn default_mode() -> Self {
        if theme::colored() { Self::Color } else { Self::Art(Glyphs::Blocks) }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "placeholder" | "box" => Some(Self::Placeholder),
            "none" | "off" => Some(Self::None),
            "color" | "colour" | "rgb" => Some(Self::Color),
            other => Glyphs::parse(other).map(Self::Art),
        }
    }

    fn draws_art(self) -> bool {
        matches!(self, Self::Art(_) | Self::Color)
    }
}

/// 본문을 보는 중인가, 개요를 보는 중인가.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Body,
    Toc,
    /// 글자를 넣는 중
    Insert,
    /// **표 안.** 커서가 칸을 넘나든다.
    ///
    /// 표를 선으로만 보여주면 "지금 어느 칸인지" 를 사람이 눈으로 따라가야
    /// 하고, 선 문자가 터미널마다 폭이 달라 그 눈마저 못 믿는다. 칸을
    /// 뒤집어 보여주면 구조가 선이 아니라 **커서**로 전달된다.
    Table,
}

/// 개요 한 줄.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocRow {
    pub depth: usize,
    pub section: u32,
    pub para: u32,
    pub text: String,
    pub is_note: bool,
}

/// 개요 트리를 줄 목록으로 편다.
pub fn flatten_outline(nodes: &[hwp_core::outline::Node], depth: usize, out: &mut Vec<TocRow>) {
    for n in nodes {
        let head = match &n.marker {
            Some(m) => format!("{m} {}", n.text),
            None => n.text.clone(),
        };
        out.push(TocRow {
            depth,
            section: n.section,
            para: n.para,
            text: head,
            is_note: n.is_note,
        });
        flatten_outline(&n.children, depth + 1, out);
    }
}

/// 버퍼의 현재 본문으로 행 목록을 다시 만들고, 그림을 제자리에 끼운다.
///
/// 쪽 경계는 **원래 조판값 자리에 그대로** 둔다 — 글자를 고치면 쪽나눔이 달라질
/// 수 있지만 그건 rhwp 가 다시 조판해야 알 수 있는 일이다. 그래서 상태줄이
/// "조판 미반영" 이라고 말한다. 추측한 쪽 경계를 진짜처럼 보여주지 않는다.
pub fn flatten_with_art(
    doc: &ScreenDoc,
    buf: &Buffer,
    width: usize,
    art: &std::collections::HashMap<usize, Vec<String>>,
    art_colors: &std::collections::HashMap<usize, Vec<Vec<hwp_core::dots::ColorCell>>>,
    images: &[hwp_core::images::DocImage],
    captions: bool,
    grids: &[Vec<String>],
) -> Vec<Row> {
    use std::collections::HashMap;

    // 좌표 → 표 번호들 (한 단락에 여럿일 수 있다)
    let mut at_table: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, t) in doc.tables.iter().enumerate() {
        at_table.entry(t.at).or_default().push(i);
    }

    // 좌표 → 그림 번호들
    let mut at_para: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, img) in images.iter().enumerate() {
        if let Some(at) = img.at {
            at_para.entry(at).or_default().push(i);
        }
    }

    // 좌표마다 몇 번 나오는지 (쪽을 넘는 단락)
    let mut appearances: HashMap<(u32, u32), usize> = HashMap::new();
    for page in &doc.pages {
        for b in &page.blocks {
            if let Some(p) = b.para_index {
                *appearances.entry((b.section, p)).or_insert(0) += 1;
            }
        }
    }

    let mut rows = Vec::new();
    let mut consumed: HashMap<(u32, u32), usize> = HashMap::new();
    let mut seen: HashMap<(u32, u32), usize> = HashMap::new();
    for page in &doc.pages {
        rows.push(Row::PageRule {
            number: page.number,
            total: doc.page_count,
            section: page.section,
            printed: page.printed,
        });
        for b in &page.blocks {
            let Some(p) = b.para_index else {
                for line in &b.lines {
                    rows.push(Row::Line {
                        section: b.section,
                        para: None,
                        leads: true,
                        continued: false,
                        text: line.clone(),
                        deco: Vec::new(),
                    });
                }
                continue;
            };
            let key = (b.section, p);
            // **문단 들여쓰기.** 조판기가 그 줄을 어디서 시작했는지를 칸으로
            // 바꿔 글자 앞에 넣는다. 접히는 폭도 그만큼 줄여야 줄이 단을
            // 넘지 않는다.
            //
            // 첫 줄과 이어지는 줄이 다를 수 있어(들여쓰기/내어쓰기) 둘 중
            // 넓은 쪽으로 접는다 — 이 코퍼스는 둘이 늘 같아서 그 경우 정확하고,
            // 다르면 줄이 조금 짧아지는 쪽으로 틀린다. 넘치는 쪽보다 낫다.
            let (ind_first, ind_rest) = doc.indent_cells(key, width);
            let inner = width.saturating_sub(ind_first.max(ind_rest)).max(8);
            let all = buf.wrapped(key, inner);
            let mut deco_at = 0usize;
            // 단락 글자별 꾸밈. 빈 벡터면 이 단락에 꾸밈이 없다.
            //
            // **접힌 줄은 원문을 자른 것이지만 공백이 버려질 수 있다**(`wrap` 이
            // 공백에서 끊으면 그 공백은 버린다). 그래서 줄 길이를 더해 가며
            // 세면 어긋난다 — 줄의 글자를 원문에서 **찾아** 자리를 맞춘다.
            let para_deco = doc.deco_bytes(key, buf.text_at(key).chars().count());
            let start = *consumed.get(&key).unwrap_or(&0);
            let nth = seen.entry(key).or_insert(0);
            *nth += 1;
            let is_last = *nth >= appearances.get(&key).copied().unwrap_or(1);
            let take = if is_last {
                all.len().saturating_sub(start)
            } else {
                b.lines.len().min(all.len().saturating_sub(start))
            };
            for (i, line) in all[start..(start + take).min(all.len())].iter().enumerate() {
                // 문단의 **첫 줄**에만 first 를 쓴다 — 쪽을 넘어온 조각의
                // 첫 줄은 문단의 첫 줄이 아니다
                let pad = if start + i == 0 { ind_first } else { ind_rest };
                let line_deco = if para_deco.is_empty() {
                    Vec::new()
                } else {
                    slice_deco(&para_deco, buf.text_at(key), line, &mut deco_at)
                };
                // 들여쓰기 뒤에 정렬을 얹는다. 문서가 가운데라고 하면 들여쓴
                // 글을 그 안에서 가운데로 — HWP 가 그렇게 한다.
                //
                // **빈 줄은 밀지 않는다.** 가운데 정렬된 빈 단락(서식 문서에
                // 흔하다)에 공백만 한 줄 들어가면 꼬리 공백이 되고, 복사해
                // 붙이면 그게 따라간다.
                let pad = if line.trim().is_empty() {
                    0
                } else {
                    pad + doc.align_pad(key, pad + display_width(line), width)
                };
                // 그리기 직전 — 탭을 공백으로. 안 바꾸면 터미널이 탭 스톱까지
                // 띄워서 그 줄과 아래 표가 밀린다.
                let line = hwp_core::wrap::for_screen(line);
                let text =
                    if pad > 0 { format!("{}{line}", " ".repeat(pad)) } else { line.to_string() };
                // 들여쓰기·정렬로 앞에 넣은 공백은 민짜다
                let deco = if line_deco.is_empty() {
                    Vec::new()
                } else {
                    let mut v = vec![0u8; pad];
                    v.extend_from_slice(&line_deco);
                    v
                };
                rows.push(Row::Line {
                    section: b.section,
                    para: Some(p),
                    leads: i == 0,
                    continued: start > 0,
                    text,
                    deco,
                });
            }
            let now = (start + take).min(all.len());
            consumed.insert(key, now);
            if now < all.len() {
                rows.push(Row::Continues);
            }
            // 단락이 끝났으면 그 자리의 그림을 그린다. 쪽을 넘는 단락은
            // 마지막 조각 뒤에만 — 같은 그림을 두 번 그리지 않는다.
            //
            // `is_last` 를 꼭 같이 봐야 한다. **빈 단락은 `now >= all.len()`
            // 이 0 >= 0 으로 늘 참**이어서, 조판기가 같은 단락을 두 번 올리면
            // (그림만 있는 단락은 `FullParagraph` 와 `Shape` 로 두 줄이다)
            // 그림이 두 번 그려졌다. s00.hwp 5쪽의 배너가 실제로 두 장으로
            // 나왔다.
            if now >= all.len() && is_last {
                // 표를 먼저 — 문서에서 표가 그 단락의 내용이다
                for &i in at_table.get(&key).into_iter().flatten() {
                    let mut gy = 0usize;
                    if let Some(lines) = grids.get(i) {
                        for line in lines {
                            rows.push(Row::Grid {
                                table: i,
                                line: line.clone(),
                                y: gy,
                            });
                            gy += 1;
                        }
                    }
                }
                for &i in at_para.get(&key).into_iter().flatten() {
                    if let Some(lines) = art.get(&i) {
                        for (ri, line) in lines.iter().enumerate() {
                            rows.push(Row::Art {
                                image: i,
                                line: line.clone(),
                                caption: false,
                                colors: art_colors
                                    .get(&i)
                                    .and_then(|rs| rs.get(ri))
                                    .cloned()
                                    .unwrap_or_default(),
                            });
                        }
                        // 캡션은 **점자일 때만** 붙인다. placeholder 는 네모
                        // 안에 이미 이름이 있어 두 번 읽히게 된다.
                        if captions {
                            if let Some(img) = images.get(i) {
                                rows.push(Row::Art {
                                    image: i,
                                    line: img.label(),
                                    caption: true,
                                    colors: Vec::new(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    rows
}

/// 페이저 상태.
pub struct App {
    pub doc: ScreenDoc,
    pub rows: Vec<Row>,
    /// 맨 위에 보이는 행 번호
    pub top: usize,
    /// 가로 스크롤 (칸)
    pub left: usize,
    /// 검색어와 결과 행 번호들
    pub query: Option<String>,
    pub hits: Vec<usize>,
    pub hit_cursor: usize,
    /// 명령줄 입력 중인 내용 (`/` 또는 `:`)
    pub input: Option<(char, String)>,
    pub message: String,
    pub quit: bool,
    pub mode: Mode,
    pub toc: Vec<TocRow>,
    /// 개요에서 고른 줄
    pub toc_cursor: usize,
    /// **표 안의 커서** — `(doc.tables 자리, 행, 열)`.
    ///
    /// 이 좌표가 그대로 `hwp set-cell` 의 좌표다(표번호:행:열). 화면이
    /// 가리키는 자리와 고치는 자리가 하나여야 한다.
    pub cell_cursor: Option<(usize, usize, usize)>,
    /// 표마다 칸이 그린 줄의 어디 있는지 — `grids` 와 같은 순서.
    pub spots: Vec<Vec<hwp_core::grid::CellSpot>>,
    /// `?` 도움 보기가 켜져 있는가.
    /// **골라 넣기 상자** — 기호표와 한자표가 같은 것을 쓴다. 메뉴처럼
    /// 모드가 아니라 덮개다.
    pub picker: Option<Picker>,
    /// **그림 크게 보기** — 몇 번째 그림인가.
    ///
    /// 본문에 그린 그림은 **문서가 놓은 크기**를 따른다. 배치 충실도에는
    /// 맞지만 알아보기엔 틀렸다: 1315×345 로고가 34칸×4줄(34×8점)로 줄어
    /// 글자가 살지 못한다. 그래서 화면을 다 쓰는 창을 따로 둔다.
    pub viewer: Option<usize>,
    pub help: bool,
    /// 도움 보기를 몇 줄 내렸는가. 키가 늘어 한 화면에 안 들어간다.
    pub help_scroll: usize,
    /// 메뉴가 열려 있으면 `(묶음, 항목)`.
    ///
    /// 키를 외우지 않고도 **무엇을 할 수 있는지 보고 고르는** 자리다.
    /// 도움 보기는 "키가 무엇인가" 를 말하고, 메뉴는 "지금 뭘 할까" 를 받는다.
    pub menu: Option<(usize, usize)>,
    /// 편집 버퍼. 읽기전용으로 열어도 커서를 보여주려고 늘 들고 있다.
    pub buf: Buffer,
    /// 편집을 허용하는가 (`hwp view` 는 아니다)
    pub editable: bool,
    /// `d` 처럼 다음 키를 기다리는 연산자
    pub pending_op: Option<char>,
    /// 문서의 그림들
    pub images: Vec<hwp_core::images::DocImage>,
    pub image_mode: ImageMode,
    /// 미리 그려 둔 그림 (그림 번호 → 줄들)
    art: std::collections::HashMap<usize, Vec<String>>,
    /// 그림마다 **줄별 색 칸**. 색으로 그릴 때만 채운다.
    art_colors: std::collections::HashMap<usize, Vec<Vec<hwp_core::dots::ColorCell>>>,
    /// 미리 그려 둔 표 (표 번호 → 줄들). 폭이 정해지면 바뀌지 않으므로
    /// 프레임마다 다시 그리지 않는다.
    pub(crate) grids: Vec<Vec<String>>,
    /// 문서 폭에 맞출지(false) 터미널 폭에 맞출지(true)
    pub fit_term: bool,
}

impl App {
    pub fn new(doc: ScreenDoc, fit_term: bool) -> Self {
        let mut toc = Vec::new();
        flatten_outline(&doc.outline, 0, &mut toc);
        let buf = Buffer::from_doc(&doc);
        let width = doc.column.cells.max(20);
        let (grids, spots): (Vec<Vec<String>>, Vec<Vec<hwp_core::grid::CellSpot>>) = doc
            .tables
            .iter()
            .map(|t| t.table.render_with_spots(width))
            .unzip();
        let rows = flatten_with_art(
            &doc,
            &buf,
            width,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            &[],
            false,
            &grids,
        );
        Self {
            grids,
            buf,
            editable: false,
            pending_op: None,
            images: Vec::new(),
            image_mode: ImageMode::Placeholder,
            art: std::collections::HashMap::new(),
            art_colors: std::collections::HashMap::new(),
            doc,
            rows,
            mode: Mode::Body,
            toc,
            toc_cursor: 0,
            cell_cursor: None,
            spots,
            picker: None,
            viewer: None,
            help: false,
            help_scroll: 0,
            menu: None,
            top: 0,
            left: 0,
            query: None,
            hits: Vec::new(),
            hit_cursor: 0,
            input: None,
            message: String::new(),
            quit: false,
            fit_term,
        }
    }

    /// 디스크에서 **다시 읽는다** — `:e!`. 쌓인 편집을 버린다.
    ///
    /// 백엔드를 다시 찾는다. `App` 은 `Rhwp` 를 들고 있지 않다 — 백엔드를
    /// 쥐고 있으면 그 수명이 화면 상태에 묶이고, 저장도 같은 이유로 그때그때
    /// 다시 찾는다.
    pub fn reload(&mut self) -> anyhow::Result<()> {
        let path = std::path::PathBuf::from(self.doc.path.clone());
        let pin = hwp_core::Pin::load_default()?;
        let rhwp = hwp_core::Rhwp::discover(&pin)?;
        let doc = ScreenDoc::build(&rhwp, &path)?;

        let mode = self.image_mode;
        let images = hwp_core::images::read(&path).unwrap_or_default();
        self.doc = doc;
        self.buf = Buffer::from_doc(&self.doc);
        self.toc.clear();
        flatten_outline(&self.doc.outline, 0, &mut self.toc);
        self.toc_cursor = 0;
        self.mode = Mode::Body;
        self.pending_op = None;
        self.top = 0;
        self.left = 0;
        self.query = None;
        self.hits.clear();
        self.hit_cursor = 0;
        // 그림과 표를 다시 그린다 (폭이 달라질 수 있다)
        self.load_images(images, mode);
        Ok(())
    }

    /// 그림 보기 방식을 다음 것으로 돌린다. 옵션 대신 쓰는 길이다.
    pub fn cycle_images(&mut self, viewport_rows: usize) {
        let mode = self.image_mode.next();
        let images = std::mem::take(&mut self.images);
        self.load_images(images, mode);
        self.message = format!("그림 — {}", mode.label());
        self.reveal_cursor(viewport_rows);
    }

    /// 문서 단 폭 / 터미널 폭을 바꾼다.
    pub fn toggle_fit_term(&mut self, area_width: u16, viewport_rows: usize) {
        self.fit_term = !self.fit_term;
        let width = self.content_width(area_width);
        // 폭이 바뀌면 그림도 다시 그려야 한다 — 칸 수가 달라진다
        let mode = self.image_mode;
        let images = std::mem::take(&mut self.images);
        self.load_images(images, mode);
        self.refresh(width, viewport_rows);
        self.message = if self.fit_term {
            format!("터미널 폭 {width}칸에 맞춤 — 문서의 줄바꿈과 달라집니다")
        } else {
            format!("문서 단 폭 {}칸", self.doc.column.cells)
        };
    }

    /// 본문에 쓸 폭(칸). 터미널에 맞추면 터미널 폭, 아니면 문서 단 폭.
    pub fn content_width(&self, area_width: u16) -> usize {
        let avail = (area_width as usize).saturating_sub(GUTTER);
        if self.fit_term {
            avail
        } else {
            self.doc.column.cells
        }
    }

    pub fn max_top(&self, viewport_rows: usize) -> usize {
        self.rows.len().saturating_sub(viewport_rows.max(1))
    }

    pub fn scroll(&mut self, delta: isize, viewport_rows: usize) {
        let max = self.max_top(viewport_rows);
        self.top = (self.top as isize + delta).clamp(0, max as isize) as usize;
    }

    pub fn go_page(&mut self, forward: bool) {
        let here = self.top;
        let found = if forward {
            self.rows
                .iter()
                .enumerate()
                .find(|(i, r)| *i > here && matches!(r, Row::PageRule { .. }))
                .map(|(i, _)| i)
        } else {
            self.rows
                .iter()
                .enumerate()
                .filter(|(i, r)| *i < here && matches!(r, Row::PageRule { .. }))
                .map(|(i, _)| i)
                .next_back()
        };
        if let Some(i) = found {
            self.top = i;
        }
    }

    /// 현재 맨 위 행이 속한 쪽.
    pub fn current_page(&self) -> u32 {
        self.rows[..=self.top.min(self.rows.len().saturating_sub(1))]
            .iter()
            .rev()
            .find_map(|r| match r {
                Row::PageRule { number, .. } => Some(*number),
                _ => None,
            })
            .unwrap_or(1)
    }

    pub fn open_menu(&mut self) {
        self.menu = Some((0, 0));
        self.help = false;
    }

    /// 메뉴 안에서 위·아래. 묶음 끝에서 **멈춘다** — 다음 묶음으로 넘어가면
    /// 어디 있는지 놓친다.
    pub fn menu_move(&mut self, delta: isize) {
        let Some((g, i)) = self.menu else { return };
        let groups = crate::run_pager::menu_groups();
        let n = groups.get(g).map(|x| x.1.len()).unwrap_or(0);
        if n == 0 {
            return;
        }
        let next = (i as isize + delta).clamp(0, n as isize - 1) as usize;
        self.menu = Some((g, next));
    }

    /// 묶음 바꾸기 — 끝에서 **감는다**(묶음은 넷뿐이라 돌아도 헷갈리지 않는다).
    pub fn menu_group(&mut self, delta: isize) {
        let Some((g, _)) = self.menu else { return };
        let groups = crate::run_pager::menu_groups();
        if groups.is_empty() {
            return;
        }
        let n = groups.len() as isize;
        let next = ((g as isize + delta) % n + n) % n;
        self.menu = Some((next as usize, 0));
    }

    /// 고른 항목의 동작.
    pub fn menu_pick(&self) -> Option<crate::run_pager::Action> {
        let (g, i) = self.menu?;
        crate::run_pager::menu_groups()
            .get(g)?
            .1
            .get(i)
            .map(|(_, a)| a.clone())
    }

    /// 커서가 있는 단락에 표가 있으면 **그 표로 들어간다.**
    ///
    /// 한 단락에 표가 여럿일 수 있다(s16.hwp 는 단락 0 에 셋이다). 첫 표로
    /// 들어가고, 표 안에서 `T` 를 다시 누르면 다음 표로 넘어간다.
    pub fn enter_table(&mut self) -> bool {
        let at = self.buf.cursor.at;
        match self.doc.tables.iter().position(|t| t.at == at) {
            Some(i) => self.enter_table_index(i),
            // 커서 단락에 표가 없으면 **뒤쪽에서 가장 가까운 표**로 간다 —
            // 표를 찾아 돌아다니는 것이 사람이 하려는 일이다
            None => match self.doc.tables.iter().position(|t| t.at > at) {
                Some(i) => self.enter_table_index(i),
                None => false,
            },
        }
    }

    /// 지정한 표로 들어간다.
    pub fn enter_table_index(&mut self, i: usize) -> bool {
        let Some(first) = self.spots.get(i).and_then(|v| {
            let mut v: Vec<_> = v.iter().collect();
            v.sort_by_key(|s| (s.row, s.col));
            v.first().copied().copied()
        }) else {
            return false;
        };
        self.mode = Mode::Table;
        self.cell_cursor = Some((i, first.row, first.col));
        // 커서도 그 표의 단락으로 옮긴다 — 나왔을 때 엉뚱한 자리에 있지 않게
        if let Some(t) = self.doc.tables.get(i) {
            let at = t.at;
            self.buf.goto(at);
        }
        self.reveal_cell();
        self.say_cell("표 안 — 화살표 또는 wasd 로 칸 이동 (hjkl 도 됩니다) · Tab 다음 칸 · T 다음 표 · Esc 나가기");
        true
    }

    /// 다음 표로 (`T` 를 표 안에서 다시 누를 때).
    pub fn next_table(&mut self) {
        let i = self.cell_cursor.map(|(t, _, _)| t + 1).unwrap_or(0);
        let i = if i >= self.doc.tables.len() { 0 } else { i };
        if !self.enter_table_index(i) {
            self.message = "표가 없습니다".into();
        }
    }

    pub fn leave_table(&mut self) {
        self.mode = Mode::Body;
        self.cell_cursor = None;
    }

    /// 표 안에서 칸을 옮긴다.
    ///
    /// 병합된 칸을 **두 번 세지 않는다** — 좌표를 하나씩 더해 가며 실제로
    /// 존재하는 칸(그 칸의 좌상단)만 찾는다. 병합된 칸 안쪽 좌표에는 칸이
    /// 없으므로 그냥 지나친다.
    pub fn move_cell(&mut self, dr: isize, dc: isize) {
        let Some((t, row, col)) = self.cell_cursor else { return };
        let Some(spots) = self.spots.get(t) else { return };
        let Some(table) = self.doc.tables.get(t) else { return };
        let (rows, cols) = (table.table.rows as isize, table.table.cols as isize);

        let mut r = row as isize;
        let mut c = col as isize;
        for _ in 0..(rows.max(cols) + 1) {
            r += dr;
            c += dc;
            if r < 0 || c < 0 || r >= rows || c >= cols {
                return; // 표 밖 — 아무것도 하지 않는다
            }
            if let Some(sp) = spots
                .iter()
                .find(|s| s.row == r as usize && s.col == c as usize)
            {
                self.cell_cursor = Some((t, sp.row, sp.col));
                self.reveal_cell();
                self.say_cell("칸");
                return;
            }
        }
    }

    /// 다음(또는 앞) 칸 — `Tab` / `Shift-Tab`. 행 끝에서 다음 행으로 넘어간다.
    pub fn next_cell(&mut self, forward: bool) {
        let Some((t, row, col)) = self.cell_cursor else { return };
        let Some(spots) = self.spots.get(t) else { return };
        let mut order: Vec<&hwp_core::grid::CellSpot> = spots.iter().collect();
        order.sort_by_key(|s| (s.row, s.col));
        let Some(i) = order.iter().position(|s| s.row == row && s.col == col) else {
            return;
        };
        let next = if forward {
            i + 1
        } else if i == 0 {
            return;
        } else {
            i - 1
        };
        if let Some(sp) = order.get(next) {
            self.cell_cursor = Some((t, sp.row, sp.col));
            self.reveal_cell();
            self.say_cell("칸");
        }
    }

    /// 지금 칸이 화면 안으로 들어오게 민다.
    fn reveal_cell(&mut self) {
        let Some((t, row, col)) = self.cell_cursor else { return };
        let Some(sp) = self
            .spots
            .get(t)
            .and_then(|v| v.iter().find(|s| s.row == row && s.col == col))
        else {
            return;
        };
        // 그 표의 그 줄이 전체 행에서 몇 번째인가
        if let Some(i) = self.rows.iter().position(
            |r| matches!(r, Row::Grid { table, y, .. } if *table == t && *y == sp.y),
        ) {
            if i < self.top {
                self.top = i;
            }
        }
    }

    /// 지금 칸의 글자를 상태줄에 알린다 — 스프레드시트의 수식 줄과 같은 몫.
    ///
    /// 칸이 좁아 글자가 접혀 보일 때 **그 칸에 실제로 뭐가 들었는지**는 이것뿐이다.
    fn say_cell(&mut self, head: &str) {
        let text = self.cell_text().unwrap_or("").replace('\n', " ⏎ ");
        let short: String = text.chars().take(60).collect();
        self.message = if short.trim().is_empty() {
            format!("{head} · (빈 칸)")
        } else {
            format!("{head} · {short}")
        };
    }

    /// 지금 칸의 글자. 표 안에서 상태줄·편집이 쓴다.
    pub fn cell_text(&self) -> Option<&str> {
        let (t, row, col) = self.cell_cursor?;
        let table = self.doc.tables.get(t)?;
        table
            .table
            .cells
            .iter()
            .find(|c| c.row == row && c.col == col)
            .map(|c| c.text.as_str())
    }

    /// **버퍼의 칸 글자를 화면 쪽으로 흘려 격자를 다시 만든다.**
    ///
    /// 칸 편집은 버퍼가 들고 있다(되돌리기가 거기 있어야 하므로). 그리는 쪽은
    /// `ScreenDoc` 의 칸 글자를 보므로 한 번 옮겨 준다 — 옮기지 않으면 고친
    /// 글자가 화면에 안 나타난다.
    pub fn sync_cells(&mut self) {
        // **그릴 글자**를 쓴다 — 조합 중인 자모가 커서 자리에 끼워진 것.
        let mut want: Vec<(usize, usize, String)> = Vec::new();
        for (ti, t) in self.doc.tables.iter().enumerate() {
            for (ci, c) in t.table.cells.iter().enumerate() {
                let key = (t.index, c.row, c.col);
                if self.buf.cells().contains_key(&key) {
                    let text = self.buf.cell_display(key);
                    if text != c.text {
                        want.push((ti, ci, text));
                    }
                }
            }
        }
        if want.is_empty() && !self.grids.is_empty() {
            return;
        }
        for (ti, ci, text) in want {
            self.doc.tables[ti].table.cells[ci].text = text;
        }
        let column = self.doc.column.cells.max(20);
        let (g, sp): (Vec<_>, Vec<_>) = self
            .doc
            .tables
            .iter()
            .map(|t| t.table.render_with_spots(column))
            .unzip();
        self.grids = g;
        self.spots = sp;
    }

    /// **칸 안에서 커서가 설 자리** — `(줄, 칸 안 가로 칸수, 그 글자의 폭)`.
    ///
    /// 폭을 함께 돌려주는 이유: 한글은 두 칸이다. 한 칸만 뒤집으려 하면 글자를
    /// 반만 집을 수 없어서 **빈칸이 끼어들고 뒷글자가 한 칸 밀린다** — 처음에
    /// 그렇게 나왔다(`사`[빈칸 반전]`업명`).
    ///
    /// 그리는 쪽과 **같은 함수**(`grid::lay_cell`)로 접고 정렬한다. 따로
    /// 세면 접는 자리나 정렬 여백이 한 칸 어긋나고, 그러면 커서가 글자 사이에
    /// 떠서 "어디를 고치는지" 를 알 수 없다.
    pub fn cell_caret(
        &self,
        table: usize,
        row: usize,
        col: usize,
        sp: &hwp_core::grid::CellSpot,
    ) -> Option<(usize, usize, usize)> {
        let t = self.doc.tables.get(table)?;
        let cell = t.table.cells.iter().find(|c| c.row == row && c.col == col)?;
        let text = self.buf.cell_display((t.index, row, col));
        let want = self.buf.cell_cursor_offset();
        let lines = hwp_core::grid::lay_cell(&text, sp.w.max(1), cell.align);
        // 정렬 여백은 `lay_cell` 이 줄 앞에 넣은 공백이다. 글자를 셀 때는
        // 그 공백도 줄의 일부로 세어야 가로 자리가 맞는다.
        let pads: Vec<usize> = lines
            .iter()
            .map(|l| l.len() - l.trim_start().len())
            .collect();
        let mut seen = 0usize;
        for (i, line) in lines.iter().enumerate() {
            let body = line.trim_start();
            let n = body.chars().count();
            if want <= seen + n {
                let col_in = want - seen;
                let cells: usize = body
                    .chars()
                    .take(col_in)
                    .map(hwp_core::wrap::char_width)
                    .sum();
                let pad = line[..pads[i]].chars().count();
                // 커서 밑 글자의 폭. 글자 끝에 서 있으면 뒤집을 글자가 없으니 1.
                let w = body
                    .chars()
                    .nth(col_in)
                    .map(hwp_core::wrap::char_width)
                    .filter(|w| *w > 0)
                    .unwrap_or(1);
                return Some((sp.y + i, pad + cells, w));
            }
            seen += n;
        }
        // 글자가 없는 칸 — 첫 줄 맨 앞
        Some((sp.y, 0, 1))
    }

    /// 지금 칸의 좌표 — `(표번호, 행, 열)`. 그대로 `hwp set-cell` 의 좌표다.
    pub fn cell_at(&self) -> Option<hwp_core::edit::CellAt> {
        let (t, row, col) = self.cell_cursor?;
        Some((self.doc.tables.get(t)?.index, row, col))
    }

    /// **기호표를 연다** (`Ctrl-K` · 메뉴 · `:sym`).
    ///
    /// 터미널에는 한글 IME 가 있어도 `○` `※` `①` 을 넣을 길이 없다. HWP 는 이
    /// 일을 문자표(`Ctrl-F10`)로 하고, 공문서는 그 기호들로 계층을 만든다.
    pub fn open_symbols(&mut self) {
        let groups: Vec<String> =
            hwp_core::symbols::GROUPS.iter().map(|(n, _)| n.to_string()).collect();
        let items: Vec<Vec<(String, String)>> = hwp_core::symbols::GROUPS
            .iter()
            .map(|(_, it)| it.iter().map(|(s, w)| (s.to_string(), w.to_string())).collect())
            .collect();
        self.picker = Some(Picker {
            title: " 기호 넣기 ".into(),
            groups,
            items,
            group: 0,
            item: 0,
            top: 0,
            replace: 0,
        });
        self.help = false;
        self.menu = None;
    }

    /// **한자표를 연다** (`F9` · 메뉴 · `:hanja`) — 커서 **앞**의 음절을 본다.
    ///
    /// HWP 의 `F9`(한자로 바꾸기)와 같다. 바로 앞 글자가 한글 음절이 아니면
    /// 열지 않고 왜인지 말한다 — 빈 상자를 띄우면 "표가 비었나" 로 읽힌다.
    pub fn open_hanja(&mut self) -> Result<(), String> {
        // **조합 중인 음절을 먼저 확정한다.** `hk` 를 쳐서 `과` 가 보이는
        // 상태는 아직 글자가 아니다 — 확정하지 않으면 커서 앞에 아무것도
        // 없다고 답한다(처음에 그렇게 나왔다).
        self.buf.commit_jamo();
        let text = match self.buf.editing_cell() {
            Some(at) => self.buf.cell_text(at).unwrap_or("").to_string(),
            None => self.buf.text_at(self.buf.cursor.at).to_string(),
        };
        let off = self.buf.cursor.offset;
        let Some(syl) = text.chars().take(off).last() else {
            return Err("커서 앞에 글자가 없습니다 — 한글 음절을 먼저 치세요".into());
        };
        if !hwp_core::hanja::is_hangul_syllable(syl) {
            return Err(format!(
                "커서 앞이 {syl:?} 입니다 — 한자로 바꿀 한글 음절이 있어야 합니다"
            ));
        }
        let cands = hwp_core::hanja::candidates(syl);
        if cands.is_empty() {
            return Err(format!("{syl} 로 읽는 한자가 표에 없습니다"));
        }
        self.picker = Some(Picker {
            title: format!(" {syl} → 한자 "),
            groups: vec![syl.to_string()],
            items: vec![cands
                .into_iter()
                .map(|(c, d)| (c.to_string(), d.to_string()))
                .collect()],
            group: 0,
            item: 0,
            top: 0,
            // 앞의 음절을 **바꾼다** — 더하는 것이 아니다
            replace: 1,
        });
        self.help = false;
        self.menu = None;
        Ok(())
    }

    /// 격자 한 줄에 몇 개를 놓나.
    pub const PICK_PER_ROW: usize = 8;
    /// 한 번에 보여 줄 줄 수. 한자는 후보가 백 개를 넘어 상자가 화면을 덮는다.
    pub const PICK_ROWS: usize = 8;

    /// 골라 넣기 상자 안에서 움직인다. `(가로, 세로)` 증감.
    pub fn picker_move(&mut self, dx: isize, dy: isize) {
        let Some(p) = self.picker.as_mut() else { return };
        let n = p.len() as isize;
        if n == 0 {
            return;
        }
        let step = dx + dy * Self::PICK_PER_ROW as isize;
        // **끝에서 멈춘다** — 감으면 어디 있는지 놓친다(메뉴에서 같은 판단)
        p.item = (p.item as isize + step).clamp(0, n - 1) as usize;
        // 고른 것이 보이게 넘긴다
        let row = p.item / Self::PICK_PER_ROW;
        if row < p.top {
            p.top = row;
        } else if row >= p.top + Self::PICK_ROWS {
            p.top = row + 1 - Self::PICK_ROWS;
        }
    }

    /// 묶음을 바꾼다. 묶음은 **감는다** — 몇 개뿐이라 돌아오는 쪽이 빠르다.
    pub fn picker_group(&mut self, delta: isize) {
        let Some(p) = self.picker.as_mut() else { return };
        let n = p.groups.len() as isize;
        if n <= 1 {
            return;
        }
        p.group = ((p.group as isize + delta).rem_euclid(n)) as usize;
        p.item = 0;
        p.top = 0;
    }

    /// 고른 글자와 **지울 글자 수**.
    pub fn picker_pick(&self) -> Option<(String, usize)> {
        let p = self.picker.as_ref()?;
        p.here().map(|(c, _)| (c.clone(), p.replace))
    }

    /// **그림 크게 보기를 연다** (`v`).
    ///
    /// 커서 단락(또는 지금 표 칸)의 그림을 고른다. 없으면 **문서의 첫 그림**
    /// 으로 간다 — 그림을 찾아 돌아다니는 것이 사람이 하려는 일이다(표에서
    /// `T` 가 같은 판단을 한다).
    pub fn open_viewer(&mut self) -> Result<(), String> {
        if self.images.is_empty() {
            return Err("이 문서에는 그림이 없습니다".into());
        }
        let at = self.buf.cursor.at;
        let here = self.cell_at().and_then(|(t, r, c)| {
            self.images.iter().position(|i| {
                i.cell.is_some_and(|x| (x.table, x.row, x.col) == (t, r, c))
            })
        });
        let i = here
            .or_else(|| self.images.iter().position(|i| i.at == Some(at)))
            .or_else(|| self.images.iter().position(|i| i.at.is_some_and(|x| x > at)))
            .unwrap_or(0);
        self.viewer = Some(i);
        self.picker = None;
        self.menu = None;
        self.help = false;
        Ok(())
    }

    /// 다음·앞 그림.
    pub fn viewer_step(&mut self, delta: isize) {
        let Some(i) = self.viewer else { return };
        let n = self.images.len() as isize;
        if n == 0 {
            return;
        }
        self.viewer = Some(((i as isize + delta).rem_euclid(n)) as usize);
    }

    /// 개요 보기를 켜고 끈다.
    pub fn toggle_toc(&mut self) {
        match self.mode {
            // 표 안에서 개요를 켜면 표에서 나간다
            Mode::Table => {
                self.mode = Mode::Body;
                self.cell_cursor = None;
            }
            Mode::Body => {
                if self.toc.is_empty() {
                    self.message = "개요를 만들 글머리가 없습니다".into();
                    return;
                }
                // 지금 보고 있는 자리에서 가장 가까운 개요 줄로 커서를 맞춘다
                if let Some((sec, para)) = self.current_coordinate() {
                    if let Some(i) = self
                        .toc
                        .iter()
                        .rposition(|r| (r.section, r.para) <= (sec, para))
                    {
                        self.toc_cursor = i;
                    }
                }
                self.mode = Mode::Toc;
                self.message = "개요 — Enter 로 이동, Esc 로 돌아가기".into();
            }
            Mode::Toc => {
                self.mode = Mode::Body;
                self.message.clear();
            }
            // 입력 중에는 개요로 가지 않는다 — 먼저 Esc 로 나와야 한다
            Mode::Insert => {}
        }
    }

    /// 이 좌표에 표나 그림을 **실제로 그렸는가.**
    pub fn drew_something_at(&self, at: (u32, u32)) -> bool {
        self.doc.tables.iter().any(|t| t.at == at)
            || self
                .images
                .iter()
                .any(|i| i.at == Some(at) && self.art.contains_key(&i.index))
    }

    /// 지금 화면 맨 위 줄의 편집 좌표.
    pub fn current_coordinate(&self) -> Option<(u32, u32)> {
        self.rows[self.top.min(self.rows.len().saturating_sub(1))..]
            .iter()
            .find_map(|r| match r {
                Row::Line { section, para: Some(p), .. } => Some((*section, *p)),
                _ => None,
            })
    }

    pub fn move_toc(&mut self, delta: isize) {
        if self.toc.is_empty() {
            return;
        }
        let max = self.toc.len() as isize - 1;
        self.toc_cursor = (self.toc_cursor as isize + delta).clamp(0, max) as usize;
    }

    /// 개요에서 고른 줄의 단락으로 본문을 옮긴다.
    pub fn jump_to_selected(&mut self) {
        let Some(target) = self.toc.get(self.toc_cursor) else { return };
        let (sec, para) = (target.section, target.para);
        match self.rows.iter().position(|r| {
            matches!(r, Row::Line { section, para: Some(p), leads: true, .. }
                     if *section == sec && *p == para)
        }) {
            Some(i) => {
                self.top = i;
                self.mode = Mode::Body;
                if self.editable {
                    self.buf.goto((sec, para));
                }
                self.message = format!("구역 {sec} 단락 {para}");
            }
            None => self.message = format!("구역 {sec} 단락 {para} 을 본문에서 찾지 못했습니다"),
        }
    }

    /// 그림 하나를 `want` 칸 폭으로 그린다. 높이는 폭에서 비율로 따라온다.
    ///
    /// `want` 가 너무 좁으면 점을 찍어도 아무것도 안 보여서 한 줄 표시로
    /// 줄인다 — 지어내지 않고 줄이는 쪽이다.
    fn draw_image(
        img: &hwp_core::images::DocImage,
        want: usize,
        mode: ImageMode,
    ) -> Vec<String> {
        use hwp_core::dots;
        const TOO_SMALL: usize = 10;
        if want == 0 {
            return Vec::new();
        }
        if want < TOO_SMALL {
            // **표시도 폭에 맞춰 줄인다.** 한 가지 모양으로 찍고 자르면
            // p.hwpx 의 4칸 화살표 칸이 `[그 ` 로 잘린다 — 잘린 글자는
            // 아무것도 알려주지 못한다. 번호만이라도 온전히 보이는 쪽이 낫다.
            let n = img.index + 1;
            for form in [format!("[그림 {n}]"), format!("[{n}]"), "▣".to_string()] {
                if display_width(&form) <= want {
                    return vec![form];
                }
            }
            return Vec::new();
        }
        match mode {
            ImageMode::None => Vec::new(),
            ImageMode::Placeholder => dots::placeholder(&img.label(), want),
            // **표 칸 안에서는 색을 쓸 수 없다.** 격자가 글자 줄(`Vec<String>`)
            // 을 합쳐 테두리를 그리므로 칸마다 색을 실을 자리가 없다. 거기서는
            // 반블록으로 떨어뜨린다 — 칸 안 그림을 색으로 보려면 `v`(크게 보기).
            ImageMode::Color => Self::draw_image(img, want, ImageMode::Art(Glyphs::Blocks)),
            ImageMode::Art(glyphs) => {
                let opts = dots::Options { glyphs, ..Default::default() };
                match dots::render_with(&img.bytes, want, 24, opts) {
                    Some(a) => a.lines,
                    // 그릴 수 없으면 자리만 — 조용히 사라지게 두지 않는다
                    None => dots::placeholder(&img.label(), want),
                }
            }
        }
    }

    /// 그림을 싣고 미리 그린다. 폭이 바뀌면 다시 불러야 한다.
    pub fn load_images(&mut self, images: Vec<hwp_core::images::DocImage>, mode: ImageMode) {
        self.images = images;
        self.image_mode = mode;
        self.art.clear();
        self.art_colors.clear();

        let column = self.doc.column.cells.max(8);
        let body = self.doc.body_width;

        // **표 칸 안의 그림은 그 칸에 그린다.** 표 아래에 쌓으면 자리가 완전히
        // 틀린다 — s00.hwp 2쪽의 로고 두 개는 1행 2열 표의 두 칸에 들어 있어서
        // 문서에서는 **같은 줄 양끝**이다. 코퍼스 그림 43개 중 23개가 칸 안이다.
        //
        // 칸 폭은 표를 그릴 때 정해지므로 `cell_widths` 로 먼저 물어보고, 그
        // 폭으로 그린 뒤 칸에 넣고 표를 다시 그린다.
        for t in self.doc.tables.iter_mut() {
            for c in t.table.cells.iter_mut() {
                c.art.clear();
            }
        }
        if mode != ImageMode::None {
            // 어느 칸에 어떤 그림인지 먼저 찾는다
            let mut spots: Vec<(usize, usize, usize)> = Vec::new(); // (표, 칸, 그림)
            for (i, img) in self.images.iter().enumerate() {
                let Some(at) = img.cell else { continue };
                let Some(ti) = self.doc.tables.iter().position(|t| t.index == at.table) else {
                    continue;
                };
                let Some(ci) = self.doc.tables[ti]
                    .table
                    .cells
                    .iter()
                    .position(|c| c.row == at.row && c.col == at.col)
                else {
                    continue;
                };
                spots.push((ti, ci, i));
            }
            // 1) 원하는 폭을 표에 알린다 — 글자 없는 칸이 4칸으로 쪼그라드는 것을 막는다
            for &(ti, ci, i) in &spots {
                let want = self.images[i].cells_wide(body, column);
                let cell = &mut self.doc.tables[ti].table.cells[ci];
                cell.min_width = cell.min_width.max(want);
            }
            // 2) 표가 정한 실제 폭으로 그려 넣는다
            let mut fills: Vec<(usize, usize, Vec<String>)> = Vec::new();
            for &(ti, ci, i) in &spots {
                let w = self.doc.tables[ti]
                    .table
                    .cell_widths(column)
                    .get(ci)
                    .copied()
                    .unwrap_or(0);
                fills.push((ti, ci, Self::draw_image(&self.images[i], w, mode)));
            }
            for (ti, ci, art) in fills {
                self.doc.tables[ti].table.cells[ci].art = art;
            }
        }
        let (g, sp): (Vec<_>, Vec<_>) = self
            .doc
            .tables
            .iter()
            .map(|t| t.table.render_with_spots(column))
            .unzip();
        self.grids = g;
        self.spots = sp;

        for (i, img) in self.images.iter().enumerate() {
            if mode == ImageMode::None {
                continue;
            }
            // 칸 안의 그림은 위에서 표에 그려 넣었다 — 본문에 또 그리지 않는다
            if img.cell.is_some() {
                continue;
            }
            // **문서가 놓은 크기대로** 그린다. 모든 그림을 단 폭 전체로 그리면
            // 본문 폭의 2% 짜리 아이콘(p.hwpx 의 11×33pt)이 전면 배너가 된다.
            let lines = Self::draw_image(img, img.cells_wide(body, column), mode);
            if lines.is_empty() {
                continue;
            }

            // 가로 자리. 문서가 가운데·오른쪽이라고 하면 그만큼 민다.
            let drawn = lines.first().map(|l| display_width(l)).unwrap_or(0);
            let pad = match img.align {
                hwp_core::images::Align::Left => 0,
                hwp_core::images::Align::Center => column.saturating_sub(drawn) / 2,
                hwp_core::images::Align::Right => column.saturating_sub(drawn),
            };
            let lines = if pad > 0 {
                let indent = " ".repeat(pad);
                lines.into_iter().map(|l| format!("{indent}{l}")).collect()
            } else {
                lines
            };
            self.art.insert(i, lines);
        }

        // **본문 그림을 색으로 다시 그린다.** 글자 줄은 폭을 재는 쪽이 쓰고,
        // 색은 그리는 쪽이 쓴다 — 폭 계산을 두 군데 두지 않는다.
        if mode == ImageMode::Color {
            for (i, img) in self.images.iter().enumerate() {
                if img.cell.is_some() || !self.art.contains_key(&i) {
                    continue;
                }
                let want = self.art[&i]
                    .first()
                    .map(|l| display_width(l))
                    .unwrap_or(0);
                if want == 0 {
                    continue;
                }
                let rows = self.art[&i].len().max(1);
                let half = !hwp_core::wrap::ambiguous_wide();
                if let Some(a) = hwp_core::dots::render_color(&img.bytes, want, rows, half) {
                    self.art_colors.insert(i, a.rows);
                }
            }
        }
        self.rows = flatten_with_art(
            &self.doc,
            &self.buf,
            column,
            &self.art,
            &self.art_colors,
            &self.images,
            mode.draws_art(),
            &self.grids,
        );
    }

    /// 버퍼가 바뀐 뒤 화면 행을 다시 만들고 커서를 보이게 한다.
    pub fn refresh(&mut self, width: usize, viewport_rows: usize) {
        self.rows = flatten_with_art(
            &self.doc,
            &self.buf,
            width,
            &self.art,
            &self.art_colors,
            &self.images,
            self.image_mode.draws_art(),
            &self.grids,
        );
        self.reveal_cursor(viewport_rows);
    }

    /// **커서를 화면 줄 기준으로 옮긴다** — `Ctrl-F`·`Ctrl-D`·`g`·`G` 가 쓴다.
    ///
    /// 고치기 모드에서는 보기만 옮기면 안 된다: 한 걸음 뒤에 `reveal_cursor`
    /// 가 커서 쪽으로 보기를 되돌리므로 **아무 일도 일어나지 않는다.**
    /// 2026-10-11 실측: `hwp edit` 에서 `G`·`Ctrl-F`·`]` 가 전부 죽어 있었고
    /// (읽기 전용에서는 멀쩡했다) `j`/`k` 로만 다닐 수 있었다.
    ///
    /// 단락 수가 아니라 **그려진 줄** 수로 센다. 한 단락이 여러 줄로 접히므로
    /// 단락으로 세면 한 화면이 아니라 열 화면을 뛴다.
    pub fn move_cursor_rows(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let here = self.cursor_row().unwrap_or(0) as isize;
        let want = (here + delta).clamp(0, self.rows.len() as isize - 1) as usize;
        self.cursor_to_row(want, delta >= 0);
    }

    /// 그 줄의 단락으로 커서를 옮긴다. 그 줄이 본문이 아니면(쪽 구분선·표·그림)
    /// `forward` 쪽으로 가장 가까운 본문 줄을 찾는다.
    pub fn cursor_to_row(&mut self, row: usize, forward: bool) {
        let n = self.rows.len();
        if n == 0 {
            return;
        }
        let order: Vec<usize> = if forward {
            (row..n).chain((0..row).rev()).collect()
        } else {
            (0..=row.min(n - 1)).rev().chain((row + 1)..n).collect()
        };
        for i in order {
            if let Row::Line { section, para, .. } = self.rows[i] {
                let at = (section, para.unwrap_or(0));
                if para.is_some() {
                    self.buf.goto(at);
                    return;
                }
            }
        }
    }

    /// 커서를 문서의 첫/마지막 본문 단락으로.
    pub fn cursor_to_edge(&mut self, last: bool) {
        let coords = self.buf.coords().to_vec();
        let Some(&at) = (if last { coords.last() } else { coords.first() }) else { return };
        self.buf.goto(at);
    }

    /// 커서가 있는 줄을 화면 안으로 끌어온다.
    pub fn reveal_cursor(&mut self, viewport_rows: usize) {
        let Some(i) = self.cursor_row() else { return };
        if i < self.top {
            self.top = i;
        } else if viewport_rows > 0 && i >= self.top + viewport_rows {
            self.top = i + 1 - viewport_rows;
        }
    }

    /// **이 문서가 쓰는 글머리 단계 사다리.** 버퍼에서 읽으므로 방금 고친
    /// 글자도 반영된다.
    pub fn levels(&self) -> hwp_core::outline::Levels {
        let items: Vec<hwp_core::outline::Item> = self
            .buf
            .coords()
            .iter()
            .map(|at| hwp_core::outline::Item {
                section: at.0,
                para: at.1,
                text: self.buf.text_at(*at).to_string(),
            })
            .collect();
        hwp_core::outline::Levels::build(&items)
    }

    /// 커서의 `(문단 안 줄 번호, 칸)` — **문단 들여쓰기를 반영한다.**
    ///
    /// 들여쓴 문단은 글자가 밀려 그려지므로, 들여쓰기를 안 더하면 커서가
    /// 글자보다 왼쪽에 서고 접히는 폭도 달라져 줄 번호까지 틀린다.
    pub fn cursor_cell(&self, width: usize) -> (usize, usize) {
        let (first, rest) = self.doc.indent_cells(self.buf.cursor.at, width);
        let inner = width.saturating_sub(first.max(rest)).max(8);
        let (row, col) = self.buf.cursor_screen(inner);
        let ind = if row == 0 { first } else { rest };
        // 정렬로 밀린 양까지 더해야 커서가 글자 위에 선다 — 그리는 쪽과 같은
        // 계산이어야 한다
        let at = self.buf.cursor.at;
        let lines = self.buf.wrapped(at, inner);
        let drawn = lines.get(row).map(|l| display_width(l)).unwrap_or(0);
        (row, col + ind + self.doc.align_pad(at, ind + drawn, width))
    }

    /// 커서가 놓인 행 번호.
    pub fn cursor_row(&self) -> Option<usize> {
        let (at, _) = (self.buf.cursor.at, self.buf.cursor.offset);
        let lead = self.rows.iter().position(|r| {
            matches!(r, Row::Line { section, para: Some(p), leads: true, .. }
                     if (*section, *p) == at)
        })?;
        let (line_in_para, _) = self.cursor_cell(self.doc.column.cells);
        Some(lead + line_in_para)
    }

    pub fn search(&mut self, needle: &str) {
        self.hits = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| matches!(r, Row::Line { text, .. } if text.contains(needle)))
            .map(|(i, _)| i)
            .collect();
        self.query = Some(needle.to_string());
        self.hit_cursor = 0;
        if let Some(&first) = self.hits.first() {
            self.top = first;
            self.message = format!("/{needle} — {}곳", self.hits.len());
        } else {
            self.message = format!("/{needle} — 찾지 못했습니다");
        }
    }

    pub fn next_hit(&mut self, forward: bool) {
        if self.hits.is_empty() {
            self.message = "검색 결과가 없습니다".into();
            return;
        }
        self.hit_cursor = if forward {
            (self.hit_cursor + 1) % self.hits.len()
        } else {
            (self.hit_cursor + self.hits.len() - 1) % self.hits.len()
        };
        self.top = self.hits[self.hit_cursor];
        self.message = format!(
            "{} / {}곳",
            self.hit_cursor + 1,
            self.hits.len()
        );
    }
}

/// 한 프레임을 그린다. 터미널 없이도 부를 수 있게 상태만 읽는다.
pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    // 머리글 / 본문 / 알림 / **도움줄**.
    //
    // 도움줄은 늘 보인다. 명령이 늘어나면 외우게 하는 대신 화면에 둬야 한다 —
    // 알림이 그 자리를 빼앗으면 "뭘 눌러야 하지" 가 다시 돌아온다. 그래서
    // 알림과 도움줄을 **다른 줄**에 둔다.
    let chunks = Layout::vertical([
        Constraint::Length(1), // 머리글
        Constraint::Min(1),    // 본문
        Constraint::Length(1), // 알림
        Constraint::Length(1), // 도움줄
    ])
    .split(area);

    frame.render_widget(header(app), chunks[0]);
    frame.render_widget(body(app, chunks[1]), chunks[1]);
    frame.render_widget(status(app), chunks[2]);
    frame.render_widget(hint_bar(app), chunks[3]);

    // 도움 보기는 **아래 도움줄만 남기고** 덮는다. 본문 칸만 쓰면 키가 서른
    // 개를 넘어 아래가 잘린다 — 머리글·알림 줄까지 쓰면 세 줄을 더 번다.
    // 도움줄은 남겨야 "어떻게 닫지" 가 화면에 남는다.
    if app.help {
        let mut over = area;
        over.height = over.height.saturating_sub(1);
        draw_help(frame, over, app.help_scroll);
    }
    // 메뉴는 본문 가운데에 작게 — 뒤가 보여야 "무엇에 대고 하는 일" 인지 안다
    if let Some((g, i)) = app.menu {
        draw_menu(frame, chunks[1], g, i);
    }
    if let Some(p) = app.picker.as_ref() {
        draw_picker(frame, chunks[1], p);
    }
    // 그림 보기는 **화면을 다 쓴다** — 크게 보려고 여는 창이다
    if let Some(i) = app.viewer {
        draw_viewer(frame, frame.area(), app, i);
    }
}

/// 상자 아래쪽에 작은 표시를 놓을 x 자리.
fn box_x(area: Rect, w: u16) -> u16 {
    area.x + area.width.saturating_sub(w + 2)
}

/// 겹쳐 띄우는 상자의 **왼쪽 경계에 걸친 2칸 글자를 지운다.**
///
/// `Clear` 는 상자 안의 칸만 비운다. 그런데 한글처럼 두 칸을 쓰는 글자가
/// 경계에 걸쳐 있으면 — 첫 칸은 상자 밖, 둘째 칸은 상자 안 — 글리프 전체가
/// 그려지면서 **테두리를 덮는다.** 실제로 그렇게 테두리가 사라졌다.
///
/// 라틴 글자만 쓰는 화면에서는 생기지 않는 일이라, 한글 문서를 다루는 쪽이
/// 따로 챙겨야 한다.
fn clear_left_straddle(frame: &mut Frame, area: Rect) {
    if area.x == 0 {
        return;
    }
    let x = area.x - 1;
    let buf = frame.buffer_mut();
    for y in area.y..area.y.saturating_add(area.height) {
        if y >= buf.area.height || x >= buf.area.width {
            continue;
        }
        let pos = (x, y);
        if display_width(buf[pos].symbol()) > 1 {
            buf[pos].set_symbol(" ");
        }
    }
}

/// **메뉴 — 화살표로 골라 실행한다.**
///
/// 키를 외우지 않고도 무엇을 할 수 있는지 보고 고르는 자리다. 도움 보기(`?`)는
/// "키가 무엇인가" 를 말하고 메뉴는 "지금 뭘 할까" 를 받는다.
///
/// 본문 **가운데에 작게** 띄운다. 화면을 다 덮으면 무엇에 대고 하는 일인지
/// 보이지 않는다 — 단락이 보이는 채로 고르게 둔다.
/// 꾸밈틀 조각 하나 — **폭 정책을 여기서 통과한다.**
///
/// 넓게 그리는 터미널에서 `· → ↓ │ ─` 같은 Ambiguous 글자는 두 칸으로 나오는데
/// ratatui 는 한 칸으로 셈한다. 그래서 우리가 **고른** 장식 글자는 ASCII 로
/// 바꾼다. 본문 글자는 여기 오지 않는다 — 문서를 고쳐 보여 주는 것은 다른
/// 일이다([`hwp_core::wrap::narrow`] 의 주석).
fn ui(s: impl AsRef<str>, style: Style) -> Span<'static> {
    Span::styled(hwp_core::wrap::narrow(s.as_ref()).into_owned(), style)
}

/// 덮개(도움·메뉴) 테두리. 표를 ASCII 로 그리기로 한 것과 같은 판단이다 —
/// 유니코드에 좁다고 보장된 선 문자가 없다.
fn ui_borders() -> ratatui::symbols::border::Set<'static> {
    use ratatui::symbols::border;
    if hwp_core::wrap::ambiguous_wide() {
        border::Set {
            top_left: "+",
            top_right: "+",
            bottom_left: "+",
            bottom_right: "+",
            vertical_left: "|",
            vertical_right: "|",
            horizontal_top: "-",
            horizontal_bottom: "-",
        }
    } else {
        border::PLAIN
    }
}

/// **골라 넣기 상자의 상태.**
///
/// 기호표(`Ctrl-K`)와 한자표(`F9`)가 같은 것을 쓴다. 둘은 같은 일이다 —
/// 격자에서 글자를 골라 커서 자리에 넣는다. 다른 것은 **무엇을 지우고
/// 넣는가**(`replace`)뿐이다: 기호는 더하고, 한자는 앞의 음절을 **바꾼다**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    pub title: String,
    /// 묶음 이름들. 한 묶음뿐이면 탭을 그리지 않는다.
    pub groups: Vec<String>,
    /// 묶음별 `(넣을 글자, 뜻)`.
    pub items: Vec<Vec<(String, String)>>,
    pub group: usize,
    pub item: usize,
    /// 보이는 첫 줄. 후보가 백 개를 넘는 한자 때문에 필요하다.
    pub top: usize,
    /// 넣을 때 커서 **앞의** 글자를 몇 개 지우나. 한자 변환이면 1.
    pub replace: usize,
}

impl Picker {
    fn here(&self) -> Option<&(String, String)> {
        self.items.get(self.group)?.get(self.item)
    }

    fn len(&self) -> usize {
        self.items.get(self.group).map(|v| v.len()).unwrap_or(0)
    }
}

/// **그림 크게 보기** — 화면을 다 쓰고 **색으로** 그린다.
///
/// 본문 인라인 렌더링은 문서가 놓은 크기를 따르므로 로고가 34×8점으로 줄어
/// 1비트로는 아무것도 안 보인다. 여기서는 두 가지를 바꾼다:
///
/// 1. **화면을 다 쓴다** — 점 수가 수십 배가 된다.
/// 2. **색을 쓴다** — 반블록 `▀` 에 글자색·바탕색을 따로 주면 한 칸이 색
///    픽셀 둘이다. 로고는 모양보다 색으로 가린다.
///
/// 넓게 그리는 터미널에서는 `▀`(Ambiguous)이 두 칸으로 나와 줄이 밀리므로
/// **공백 + 바탕색**으로 간다 — 세로 해상도를 절반 잃는 대신 밀리지 않는다.
/// `NO_COLOR` 가 있으면 색을 쓰지 않고 1비트 점 그림으로 간다.
fn draw_viewer(frame: &mut Frame, area: Rect, app: &App, which: usize) {
    use ratatui::style::Color;
    use ratatui::widgets::{Block, Borders, Clear};

    let Some(img) = app.images.get(which) else { return };

    frame.render_widget(Clear, area);
    clear_left_straddle(frame, area);
    let where_ = match (img.at, img.cell) {
        (_, Some(c)) => format!("표{} {}행{}열", c.table, c.row + 1, c.col + 1),
        (Some((s, p)), None) => format!("구역{s} 단락{p}"),
        (None, None) => "자리 모름".to_string(),
    };
    let title = format!(" 그림 {}/{} — {where_} ", which + 1, app.images.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(ui_borders())
        .title(hwp_core::wrap::narrow(&title).into_owned())
        .border_style(theme::page_rule());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 4 || inner.height < 4 {
        return;
    }

    // 아래 두 줄은 설명과 안내 — 그림은 그 위를 다 쓴다
    let art_rows = inner.height.saturating_sub(2) as usize;
    let cols = inner.width as usize;

    let mut lines: Vec<Line> = Vec::new();
    let mut shown = String::new();
    if theme::colored() {
        let half = !hwp_core::wrap::ambiguous_wide();
        match hwp_core::dots::render_color(&img.bytes, cols, art_rows, half) {
            Some(art) => {
                shown = format!(
                    "{}칸×{}줄 · 색 픽셀 {}개",
                    art.cols,
                    art.rows.len(),
                    art.pixels()
                );
                for row in &art.rows {
                    let spans: Vec<Span> = row
                        .iter()
                        .map(|c| {
                            let up = Color::Rgb(c.upper[0], c.upper[1], c.upper[2]);
                            let lo = Color::Rgb(c.lower[0], c.lower[1], c.lower[2]);
                            if half {
                                // 위는 글자색, 아래는 바탕색 — 한 칸에 두 픽셀
                                Span::styled("▀", Style::new().fg(up).bg(lo))
                            } else {
                                Span::styled(" ", Style::new().bg(up))
                            }
                        })
                        .collect();
                    lines.push(Line::from(spans));
                }
            }
            None => lines.push(Line::from(ui("그림을 읽을 수 없습니다", Style::new()))),
        }
    } else {
        // NO_COLOR — 1비트 점 그림. 그래도 화면을 다 쓰므로 본문보다 낫다.
        let opts = hwp_core::dots::Options::default();
        match hwp_core::dots::render_with(&img.bytes, cols, art_rows, opts) {
            Some(art) => {
                shown = format!("{}칸×{}줄 · {:?} (NO_COLOR)", art.cols, art.rows, art.glyphs);
                for l in &art.lines {
                    lines.push(Line::from(Span::styled(l.clone(), theme::art())));
                }
            }
            None => lines.push(Line::from(ui("그림을 읽을 수 없습니다", Style::new()))),
        }
    }

    // 설명 — 원본 크기와 형식. 무엇을 보고 있는지 알아야 한다.
    let what = match img.dimensions() {
        Some((w, h)) => format!(
            " {} · {} {w}×{h} · {} KB · {shown}",
            img.name.rsplit('/').next().unwrap_or(&img.name),
            img.format(),
            img.bytes.len() / 1024
        ),
        None => format!(" {} · 읽을 수 없습니다", img.name),
    };
    lines.push(Line::from(ui(
        hwp_core::wrap::pad_to(&what, cols),
        theme::caption(),
    )));
    lines.push(Line::from(ui(
        hwp_core::wrap::pad_to(" n/p 다음·앞 그림 · Esc 닫기", cols),
        theme::hint(),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

/// **골라 넣기 상자** — 격자로 골라 넣는다.
///
/// 테두리와 안내줄은 **우리 장식**이라 넓은 터미널에서 ASCII 로 바꾼다
/// ([`ui`]). 기호 자체는 **내용**이라 바꾸지 않는다 — `○` 를 `o` 로 보여
/// 주면 고른 것과 넣는 것이 달라진다. 그래서 기호 하나에 네 칸을 주고
/// 표시 폭으로 채운다: 넓게 그리는 터미널에서 두 칸인 기호가 와도 상자를
/// 넘지 않는다(한 칸 왼쪽으로 몰릴 수는 있다 — 본문 글자와 같은 한계다).
fn draw_picker(frame: &mut Frame, area: Rect, p: &Picker) {
    use ratatui::widgets::{Block, Borders, Clear};

    let Some(items) = p.items.get(p.group) else { return };

    // 기호 하나에 **네 칸**을 준다 — 넓게 그리는 터미널에서 두 칸인 글자가
    // 와도 상자를 넘지 않는다.
    const CELL: usize = 4;
    let per_row = App::PICK_PER_ROW;
    let rows = items.len().div_ceil(per_row);
    // 한자는 후보가 백 개를 넘는다 — 상자가 화면을 다 덮지 않게 넘긴다.
    let view_rows = rows.min(App::PICK_ROWS);

    // 묶음 탭. 한 묶음뿐이면 그리지 않는다 (한자표가 그렇다).
    let mut tabs: Vec<Span> = Vec::new();
    let mut tabs_w = 0usize;
    if p.groups.len() > 1 {
        for (gi, name) in p.groups.iter().enumerate() {
            if gi > 0 {
                tabs.push(ui(" ", theme::hint()));
                tabs_w += 1;
            }
            let st = if gi == p.group { theme::mode() } else { theme::hint() };
            tabs.push(ui(format!(" {name} "), st));
            tabs_w += display_width(name) + 2;
        }
    }

    // 고른 글자의 **뜻** — 모양만 보면 `○ ◦ ◯` 를, 한자는 더더욱 가릴 수 없다
    let what = p
        .here()
        .map(|(c, d)| {
            if d.is_empty() {
                format!(" {c}  (새김이 없는 드문 글자)")
            } else {
                format!(" {c}  {d}")
            }
        })
        .unwrap_or_default();
    let more = if rows > view_rows {
        format!("  [{}/{}]", p.item + 1, items.len())
    } else {
        String::new()
    };
    let hint = format!(
        " ←→/ad 고르기 · ↑↓/ws 줄 · {}↵ 넣기 · Esc 닫기{more}",
        if p.groups.len() > 1 { "Tab 묶음 · " } else { "" }
    );

    let inner_w = (per_row * CELL)
        .max(tabs_w)
        .max(display_width(&hint))
        .max(display_width(&what).min(64));
    let w = (inner_w as u16 + 2).min(area.width);
    // 테두리 2 + 탭 + 빈 줄 + 격자 + 빈 줄 + 뜻 + 안내
    let chrome = 2 + u16::from(!tabs.is_empty()) + 1 + 1 + 1 + 1;
    let h = (view_rows as u16 + chrome).min(area.height);
    let box_area = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };

    frame.render_widget(Clear, box_area);
    clear_left_straddle(frame, box_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(ui_borders())
        .title(hwp_core::wrap::narrow(&p.title).into_owned())
        .border_style(theme::page_rule());
    let inner = block.inner(box_area);
    frame.render_widget(block, box_area);

    let cells = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    if !tabs.is_empty() {
        lines.push(Line::from(tabs));
    }
    lines.push(Line::raw(""));
    for r in p.top..(p.top + view_rows).min(rows) {
        let mut spans: Vec<Span> = Vec::new();
        for c in 0..per_row {
            let i = r * per_row + c;
            let Some((glyph, _)) = items.get(i) else { break };
            // **글자는 통과시키지 않는다** — 내용이다. `○` 를 `o` 로 보여
            // 주면 고른 것과 넣는 것이 달라진다.
            let cellstr = hwp_core::wrap::pad_to(&format!(" {glyph}"), CELL);
            let st = if i == p.item { Style::new().reversed() } else { Style::new() };
            spans.push(Span::styled(cellstr, st));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        hwp_core::wrap::pad_to(&what, cells),
        theme::caption(),
    )));
    lines.push(Line::from(ui(hwp_core::wrap::pad_to(&hint, cells), theme::hint())));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_menu(frame: &mut Frame, area: Rect, group: usize, item: usize) {
    use ratatui::widgets::{Block, Borders, Clear};
    let groups = crate::run_pager::menu_groups();
    let Some((_, items)) = groups.get(group) else { return };

    const HINT: &str = " ↑↓/ws 고르기 · ←→/ad 묶음 · ↵ 실행 · Esc 닫기";

    // 묶음 탭
    let mut tabs: Vec<Span> = Vec::new();
    let mut tabs_w = 0usize;
    for (gi, (name, _)) in groups.iter().enumerate() {
        if gi > 0 {
            tabs.push(ui(" ", theme::hint()));
            tabs_w += 1;
        }
        let st = if gi == group { theme::mode() } else { theme::hint() };
        tabs.push(ui(format!(" {name} "), st));
        tabs_w += display_width(name) + 2;
    }

    // **줄을 먼저 만들고 그 줄들로 상자 크기를 정한다.** 거꾸로 하면 어느
    // 줄이 길어질 때마다 상자를 넘는다 — 안내 줄이 항목보다 길어서 실제로
    // 테두리를 지웠다.
    let inner_w = items
        .iter()
        .map(|(label, _)| display_width(label) + 1)
        .chain([tabs_w, display_width(HINT)])
        .max()
        .unwrap_or(28)
        .max(24);
    let w = (inner_w as u16 + 2).min(area.width);
    // 높이: 테두리 2 + 묶음탭 1 + 빈 줄 1 + 항목 + 빈 줄 1 + 안내 1.
    //
    // **전에는 5 였고 안내줄이 잘려 나갔다.** 폭은 내용에서 재면서 높이는
    // 손으로 센 수를 썼고, 그래서 "↑↓ 고르기 · ↵ 실행" 이 한 번도 화면에
    // 뜨지 않았다 — 잘린 도움은 **없는 기능**으로 읽힌다.
    //
    // 그리고 **화면보다 긴 묶음은 넘긴다.** 항목이 늘어 상자가 화면을 넘자
    // 안내줄이 또 잘렸다(같은 모양이 네 번째다). 고른 항목이 보이는 창을
    // 계산하므로 상태를 늘리지 않는다.
    const CHROME: u16 = 6;
    let avail = area.height.saturating_sub(CHROME).max(1) as usize;
    let view = items.len().min(avail);
    let top = item
        .saturating_sub(view.saturating_sub(1))
        .min(items.len().saturating_sub(view));
    let h = (view as u16 + CHROME).min(area.height);
    let box_area = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };

    frame.render_widget(Clear, box_area);
    clear_left_straddle(frame, box_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(ui_borders())
        .title(hwp_core::wrap::narrow(" 메뉴 ").into_owned())
        .border_style(theme::page_rule());
    let inner = block.inner(box_area);
    frame.render_widget(block, box_area);

    let cells = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(tabs));
    lines.push(Line::raw(""));
    for (ii, (label, _)) in items.iter().enumerate().skip(top).take(view) {
        let st = if ii == item { Style::new().reversed() } else { Style::new() };
        // 고른 줄은 **끝까지** 뒤집어 어디 있는지 분명하게.
        //
        // 채울 때 `{:<w$}` 를 쓰면 안 된다 — 글자 수로 채우므로 한글이 두
        // 배가 되어 테두리를 넘는다(실제로 그렇게 나왔다).
        lines.push(Line::from(ui(
            hwp_core::wrap::pad_to(&format!(" {label}"), cells),
            st,
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(ui(hwp_core::wrap::pad_to(HINT, cells), theme::hint())));
    frame.render_widget(Paragraph::new(lines), inner);
}

/// 아래 도움줄 — **지금 모드에서 쓸 수 있는 키.**
///
/// 모드마다 다른 것만 싣는다. 좁은 터미널에서는 뒤가 잘리므로 **막혔을 때
/// 가장 필요한 것을 맨 앞에** 둔다 — `?`(도움)와 나가는 법이다. 처음엔
/// `?` 와 `ZZ` 를 뒤에 뒀는데, 90칸 터미널에서 그 둘이 먼저 잘렸다.
fn hint_bar(app: &App) -> Paragraph<'_> {
    let (name, pairs): (&str, &[(&str, &str)]) = match app.mode {
        Mode::Insert if app.buf.editing_cell().is_some() => (
            "표 칸",
            &[
                ("Esc", "칸 고치기 끝"),
                ("⌫", "지우기"),
                ("←→", "커서"),
                ("Ctrl-K", "기호"),
                ("F9", "한자"),
                (":set ime=2", "두벌식"),
            ],
        ),
        Mode::Insert => (
            "끼워넣기",
            &[
                ("Esc", "보통 모드로"),
                ("⌫", "지우기"),
                // **보기로 든 기호를 적지 않는다.** 이 줄은 우리 장식이고
                // `○ □ ※ ①` 는 전부 Ambiguous 라 넓은 터미널에서 줄을 민다 —
                // 넣어 봤고 폭 시험이 바로 잡았다. 기호는 문자표 안에서 보면
                // 되고, 거기서는 **내용**이라 바꾸지 않는다.
                ("Ctrl-K", "기호"),
                ("F9", "한자"),
                (":set ime=2", "두벌식"),
            ],
        ),
        Mode::Toc => (
            "개요",
            &[("?", "도움"), ("Esc", "본문"), ("j k", "이동"), ("↵", "그 자리로")],
        ),
        Mode::Table => (
            "표",
            &[
                ("?", "도움"),
                ("Esc", "나가기"),
                ("←↓↑→ wasd", "칸"),
                ("i ↵", "칸 고치기"),
                ("u", "되돌리기"),
                ("Tab", "다음 칸"),
                ("T", "다음 표"),
            ],
        ),
        Mode::Body if app.editable => (
            "고치기",
            &[
                ("?", "도움"),
                ("m", "메뉴"),
                ("ZZ", "저장하고 나가기"),
                ("i", "넣기"),
                ("u", "되돌리기"),
                ("x dd", "지우기"),
                ("yy p", "담기·붙이기"),
                ("Tab", "글머리"),
                ("T", "표"),
            ],
        ),
        Mode::Body => (
            "읽기",
            &[
                ("?", "도움"),
                ("m", "메뉴"),
                ("q", "나가기"),
                ("jk ^d", "이동"),
                ("] [", "쪽"),
                ("/", "찾기"),
                ("t", "개요"),
                ("T", "표"),
                ("z", "그림 방식"),
                ("v", "그림 크게"),
            ],
        ),
    };
    let mut spans = vec![ui(format!(" {name} "), theme::mode()), Span::raw(" ")];
    for (i, (k, what)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(ui(" · ", theme::hint()));
        }
        spans.push(ui(*k, theme::key()));
        spans.push(ui(format!(" {what}"), theme::hint()));
    }
    Paragraph::new(Line::from(spans))
}

/// `?` — **전부 보여주는 도움 보기.**
///
/// 도움줄 한 줄에는 다 들어가지 않는다. 키가 서른 개를 넘으므로 **두 칸**으로
/// 나눠야 보통 터미널에 들어간다. 뒤의 본문은 [`Clear`] 로 지운다 — 안 지우면
/// 글자가 겹쳐 둘 다 못 읽는다(처음에 그렇게 나왔다).
fn draw_help(frame: &mut Frame, area: Rect, scroll: usize) {
    use ratatui::widgets::{Block, Borders, Clear};

    type Group = (&'static str, &'static [(&'static str, &'static str)]);
    const LEFT: &[Group] = &[
        (
            "보기",
            &[
                ("j k", "줄·단락 아래·위"),
                ("^d ^u", "반 화면"),
                ("space b", "한 화면"),
                ("g G", "처음·끝"),
                ("] [", "다음·앞 쪽"),
                (":N", "N쪽으로"),
                ("h l", "글자 좌·우"),
                ("w b e", "낱말 앞·뒤·끝"),
                ("0 $", "단락 처음·끝"),
                ("< >", "가로 스크롤"),
            ],
        ),
        (
            "보기 방식",
            &[
                ("z", "그림 방식 (색→반블록→점자→ASCII→자리만→끔)"),
                ("v", "그림 크게 보기 — 화면을 다 쓰고 색으로"),
                ("W", "문서 단 폭 / 터미널 폭"),
                (":set amb=", "표가 어긋날 때 (wide/narrow)"),
            ],
        ),
        (
            "도스 시절 키",
            &[
                ("F1 F2 F10", "도움 · 저장 · 메뉴"),
                ("Alt-X", "끝내기"),
                ("Ctrl-Y", "단락 지우기"),
                ("Ctrl-L", "다시 찾기"),
                ("Ctrl-E X S", "커서 위·아래·왼쪽"),
                ("Ctrl-T", "낱말 지우기 (vi 의 dw 자리)"),
                ("Ctrl-Q Y", "커서에서 단락 끝까지 지우기"),
                ("Ctrl-K 1~0", "책갈피 넣기 — HWP 도 같은 키"),
                ("Ctrl-Q 1~0", "그 책갈피로 이동"),
                ("Ctrl-A", "낱말 뒤로"),
                ("Ctrl-K …", "S 저장 Q 나가기 Y 삭제 L 담기 C 붙이기"),
                ("Ctrl-Q …", "F 찾기 S 단락처음 D 단락끝"),
                ("Ctrl-Ins", "담기 (CUA)"),
                ("Shift-Del Ins", "잘라내기 · 붙이기"),
                ("Esc", "두 키 묶음 버리기 (Ctrl-K·Ctrl-Q 뒤)"),
                (":set keys=dos", "Ctrl-D/F 를 커서로"),
            ],
        ),
        (
            "메뉴·찾기·개요·표",
            &[
                ("m F10", "메뉴 — 화살표로 골라 실행"),
                ("/", "찾기"),
                ("n N", "다음·앞 결과"),
                ("t", "개요 (↵ 로 그 자리로)"),
                (":table 3x4", "빈 표 만들기 — 커서 단락에"),
                ("T", "표 안으로 — 칸을 넘나든다"),
                ("←↓↑→ wasd", "표 안: 칸 이동 (hjkl 도 됩니다)"),
                ("i ↵", "표 안: 그 칸 고치기 — 커서가 칸 안에 선다"),
            ],
        ),
    ];
    const RIGHT: &[Group] = &[
        (
            "고치기",
            &[
                ("i a I A", "끼워넣기 (Esc 로 나옴)"),
                ("o O", "빈 단락 아래·위"),
                ("x", "글자 지우기"),
                ("dd", "단락 지우기 (담긴다)"),
                ("yy", "단락 담기"),
                ("p P", "붙이기 아래·위"),
                ("u", "되돌리기"),
                ("^r", "다시하기"),
                ("Tab ⇧Tab", "글머리 단계"),
                (":set ime=2", "두벌식 (off 로 끔)"),
            ],
        ),
        (
            "저장·나가기",
            &[
                ("Ctrl-K", "기호 넣기 — 글자 넣는 중에 (글머리·번호·화살표…)"),
                ("F9", "한자로 바꾸기 — 커서 앞 음절 (HWP 와 같은 키)"),
                (":w", "저장 (자가검증)"),
                (":saveas <경로>", "다른 이름으로 — 그 뒤로 새 이름을 고칩니다"),
                (":w <경로>", "사본만 — 원본을 계속 고칩니다"),
                ("ZZ", "저장하고 나가기"),
                ("q", "나가기 — 편집이 남으면 거부"),
                ("ZQ", "버리고 나가기"),
                (":e!", "디스크에서 다시 읽기"),
            ],
        ),
    ];

    fn build(groups: &[Group]) -> Vec<Line<'static>> {
        let mut lines: Vec<Line> = Vec::new();
        for (i, (title, items)) in groups.iter().enumerate() {
            if i > 0 {
                lines.push(Line::raw(""));
            }
            lines.push(Line::from(ui(format!(" {title} "), theme::mode())));
            for (k, what) in *items {
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    ui(hwp_core::wrap::pad_to(k, 11), theme::key()),
                    ui(*what, Style::new()),
                ]));
            }
        }
        lines
    }

    frame.render_widget(Clear, area);
    clear_left_straddle(frame, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(ui_borders())
        .title(hwp_core::wrap::narrow(" 도움 — ? 또는 Esc 로 닫는다 ").into_owned())
        .border_style(theme::page_rule());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);
    let keep = inner.height as usize;
    let (l, r) = (build(LEFT), build(RIGHT));
    // **넘치면 화살표로 넘긴다.** 키가 늘어 한 화면에 안 들어가는데, 잘린
    // 채로 두면 "없는 기능" 으로 읽힌다.
    let longest = l.len().max(r.len());
    let max_scroll = longest.saturating_sub(keep);
    let at = scroll.min(max_scroll);
    let cut = |v: Vec<Line<'static>>| -> Vec<Line<'static>> {
        v.into_iter().skip(at).take(keep).collect()
    };
    frame.render_widget(Paragraph::new(cut(l)), cols[0]);
    frame.render_widget(Paragraph::new(cut(r)), cols[1]);
    if max_scroll > 0 {
        let mark = if at == 0 {
            " ↓ 더 있다 "
        } else if at >= max_scroll {
            " ↑ 위에 더 "
        } else {
            " ↑↓ 더 있다 "
        };
        let w = display_width(mark) as u16;
        let spot = Rect {
            x: box_x(area, w),
            y: area.y + area.height.saturating_sub(1),
            width: w.min(area.width),
            height: 1,
        };
        frame.render_widget(Paragraph::new(Line::from(ui(mark, theme::key()))), spot);
    }
}



fn header(app: &App) -> Paragraph<'_> {
    let d = &app.doc;
    let name = std::path::Path::new(&d.path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| d.path.clone());
    Paragraph::new(Line::from(vec![
        // 파일 이름은 **문서가 준 글자**다 — 바꾸지 않는다.
        Span::styled(name, Style::new().bold()),
        ui(match app.mode {
            Mode::Toc => format!("  [{}]  개요 {} / {}줄", d.format, app.toc_cursor + 1, app.toc.len()),
            // 표 안에서는 **칸 좌표**를 보여준다 — 그게 `set-cell` 의 좌표다
            Mode::Table => match app.cell_cursor {
                Some((t, row, col)) => format!(
                    "  [{}]  표{} {}행{}열  ·  Tab 다음칸 · Esc 나가기",
                    d.format,
                    app.doc.tables.get(t).map(|x| x.index).unwrap_or(0),
                    row + 1,
                    col + 1
                ),
                None => format!("  [{}]  표", d.format),
            },
            // 칸을 고치는 중이면 **칸 좌표**다 — 본문 단락 번호는 지금 쓸모가 없다
            Mode::Insert if app.buf.editing_cell().is_some() => {
                let (t, r, c) = app.buf.editing_cell().expect("위에서 확인했다");
                format!(
                    "  [{}]  표{t} {}행{}열 안  ·  Esc 로 끝냅니다",
                    d.format,
                    r + 1,
                    c + 1
                )
            }
            Mode::Body | Mode::Insert => format!(
                "  [{}]  {} / {} 쪽  ·  구역{} 단락{}:{}",
                d.format,
                app.current_page(),
                d.page_count,
                app.buf.cursor.at.0,
                app.buf.cursor.at.1,
                app.buf.cursor.offset
            ),
        }, Style::new()),
    ]))
    .style(theme::header())
}

fn status(app: &App) -> Paragraph<'_> {
    if let Some((prefix, buf)) = &app.input {
        return Paragraph::new(Line::from(vec![Span::raw(format!("{prefix}{buf}"))]));
    }
    let mut spans = Vec::new();
    if app.mode == Mode::Insert {
        let ime = match app.buf.ime {
            Ime::Dubeolsik => "두벌식",
            Ime::Off => "그대로",
        };
        spans.push(ui(format!("-- 끼워넣기 ({ime}) -- "), Style::new().bold()));
    }
    if !app.message.is_empty() {
        // 알림줄은 **우리 말**이다 — 표 칸 미리보기처럼 문서 글자를 인용하는
        // 경우가 있으나, 그 인용은 이미 잘라 붙인 것이라 장식과 같은 줄이다.
        spans.push(ui(app.message.clone(), Style::new()));
    } else {
        // 텍스트 파일은 **잴 단 폭이 없다.** "측정 못 함" 이라고 말하면
        // 재려고 했다는 거짓이 된다 — 문서가 정한 폭이 애초에 없다.
        // 쪽도 문서의 쪽이 아니라 우리가 나눈 것이라고 말한다.
        spans.push(ui(
            if app.doc.format == "text" {
                format!("터미널 폭 {}칸 · 쪽은 우리 구분", app.doc.column.cells)
            } else {
                format!("단 폭 {}", app.doc.column.provenance())
            },
            Style::new().dim(),
        ));
    }
    if app.buf.is_dirty() {
        spans.push(ui(
            format!(" 편집 {}건 · 조판 미반영 ", app.buf.pending_edits()),
            theme::dirty(),
        ));
    }
    Paragraph::new(Line::from(spans))
}

fn body<'a>(app: &'a App, area: Rect) -> Paragraph<'a> {
    if app.mode == Mode::Toc {
        return toc_view(app, area);
    }
    let width = app.content_width(area.width);
    let height = area.height as usize;
    let mut lines: Vec<Line<'a>> = Vec::with_capacity(height);

    let cursor_row = app.editable.then(|| app.cursor_row()).flatten();
    for (n, row) in app.rows.iter().skip(app.top).take(height).enumerate() {
        let index = app.top + n;
        lines.push(match row {
            Row::PageRule { number, total, section, printed } => {
                page_rule(*number, *total, *section, *printed, width)
            }
            Row::Continues => Line::from(vec![
                Span::raw(" ".repeat(GUTTER)),
                Span::styled("⋯ 다음 쪽에서 이어짐", Style::new().dim().italic()),
            ]),
            Row::Grid { table, line, y } => grid_line(app, *table, line, *y),
            Row::Art { line, caption, colors, .. } => {
                let gut = Span::raw(" ".repeat(GUTTER));
                if *caption {
                    Line::from(vec![gut, Span::styled(line.clone(), theme::caption().italic())])
                } else if colors.is_empty() {
                    Line::from(vec![gut, Span::styled(line.clone(), theme::art())])
                } else {
                    // **색으로 그린 줄.** 칸마다 글자색·바탕색을 준다.
                    use ratatui::style::Color;
                    let half = !hwp_core::wrap::ambiguous_wide();
                    let mut spans = vec![gut];
                    for c in colors {
                        let up = Color::Rgb(c.upper[0], c.upper[1], c.upper[2]);
                        let lo = Color::Rgb(c.lower[0], c.lower[1], c.lower[2]);
                        spans.push(if half {
                            Span::styled("▀", Style::new().fg(up).bg(lo))
                        } else {
                            Span::styled(" ", Style::new().bg(up))
                        });
                    }
                    Line::from(spans)
                }
            }
            Row::Line { section, para, leads, continued, text, deco } => {
                // 커서가 이 행에 있으면 그 칸을 반전시킨다. 행 번호를 아는
                // 여기서만 판단할 수 있다.
                let cursor_col = (cursor_row == Some(index))
                    .then(|| app.cursor_cell(width).1);
                body_line(app, *section, *para, *leads, *continued, text, deco, width, cursor_col)
            }
        });
    }
    Paragraph::new(lines)
}

/// 개요 보기. 고른 줄을 뒤집어 보여준다.
fn toc_view<'a>(app: &'a App, area: Rect) -> Paragraph<'a> {
    let height = area.height as usize;
    // 커서가 화면 안에 들어오도록 창을 민다
    let start = app.toc_cursor.saturating_sub(height / 2).min(
        app.toc.len().saturating_sub(height.max(1)),
    );
    let lines: Vec<Line<'a>> = app
        .toc
        .iter()
        .enumerate()
        .skip(start)
        .take(height)
        .map(|(i, r)| {
            let coord = format!("{:>4}  ", r.para);
            let indent = "  ".repeat(r.depth);
            // 깊이를 **색으로도** 보여준다 — 들여쓰기만으로는 깊은 트리에서
            // 눈이 따라가기 어렵다. 색이 없는 터미널에서도 들여쓰기와 굵기로
            // 같은 정보가 남는다.
            let style = if i == app.toc_cursor {
                Style::new().reversed()
            } else if r.is_note {
                theme::caption()
            } else if r.depth == 0 {
                theme::outline(0).bold()
            } else {
                theme::outline(r.depth)
            };
            Line::from(vec![
                Span::styled(coord, Style::new().dim()),
                Span::styled(format!("{indent}{}", r.text), style),
            ])
        })
        .collect();
    Paragraph::new(lines)
}

/// 왼쪽 여백 — **`view` 와 `page` 가 같은 것을 쓴다.**
///
/// 처음엔 두 곳에 따로 적었다가 갈라졌다: `◻`(못 그린 개체) 표시가 TUI 에만
/// 있어서 `hwp page` 에는 안 나왔다. 같은 화면 모델을 쓴다고 해 놓고 여백만
/// 따로 두면 그런 일이 생긴다.
///
/// 6칸: 번호 4 + 이어짐/편집 1 + 개체 1.
///   `↳` 앞 쪽에서 이어진 단락 · `*` 고친 단락 · `◻` 우리가 못 그린 개체
pub fn gutter_for(
    app: &App,
    section: u32,
    para: Option<u32>,
    leads: bool,
    continued: bool,
    numbers: bool,
) -> String {
    if !numbers {
        return String::new();
    }
    if !leads {
        return " ".repeat(GUTTER);
    }
    match para {
        Some(n) => {
            let coord = (section, n);
            let mark = if continued {
                '↳'
            } else if app.buf.dirty_paragraphs().contains(&coord) {
                '*'
            } else {
                ' '
            };
            // 그리기개체·표가 있는데 우리가 못 그린 자리만 표시한다. 개수만
            // 세면 구역 정의·쪽번호까지 걸려 "그림이 있는데 안 보인다" 는
            // 오해를 부른다(s00.hwp 단락 0 이 그랬다).
            let objects = app.doc.objects.get(&coord).copied().unwrap_or(0);
            let obj = if objects > 0 && !app.drew_something_at(coord) { '◻' } else { ' ' };
            format!("{n:>4}{mark}{obj}")
        }
        None => "   ·  ".to_string(),
    }
}

fn page_rule<'a>(
    number: u32,
    total: u32,
    section: u32,
    printed: Option<u32>,
    width: usize,
) -> Line<'a> {
    // 문서가 찍는 쪽번호가 순서와 다르면 둘 다 보여준다 — 문서가 쪽번호를
    // 다시 시작한 것이고, 그걸 숨기면 "4쪽이 두 번 나온다" 로 보인다.
    let mark = match printed {
        Some(p) if p != number => format!(" (문서 표기 {p}쪽)"),
        _ => String::new(),
    };
    let label = if section > 0 {
        format!(" {number} / {total} 쪽{mark} · 구역 {section} ")
    } else {
        format!(" {number} / {total} 쪽{mark} ")
    };
    let label_w = display_width(&label);
    let total_w = width + GUTTER;
    let lead = total_w.saturating_sub(label_w) / 2;
    let tail = total_w.saturating_sub(lead + label_w);
    Line::from(vec![
        Span::styled(hwp_core::wrap::box_h().repeat(lead), theme::page_rule()),
        Span::styled(label, Style::new().bold()),
        Span::styled(hwp_core::wrap::box_h().repeat(tail), theme::page_rule()),
    ])
}

fn body_line<'a>(
    app: &'a App,
    _section: u32,
    para: Option<u32>,
    leads: bool,
    continued: bool,
    text: &'a str,
    deco: &[u8],
    width: usize,
    cursor_col: Option<usize>,
) -> Line<'a> {
    let gutter = gutter_for(app, _section, para, leads, continued, true);

    let mut spans = vec![Span::styled(gutter, theme::gutter())];

    // 인라인 컨트롤 표시(U+FFFC)는 지운다. 터미널에서 빈 네모로 보여 "그림이
    // 안 보인다" 가 되는데, 그림은 아래 `Row::Art` 블록으로 따로 그린다.
    // 꾸밈도 같이 걸러야 한다 — 글자만 빼면 꾸밈이 한 칸씩 밀린다
    let mut cleaned = String::new();
    let mut cleaned_deco: Vec<u8> = Vec::new();
    for (i, c) in text.chars().enumerate() {
        if c == '\u{FFFC}' {
            continue;
        }
        cleaned.push(c);
        cleaned_deco.push(deco.get(i).copied().unwrap_or(0));
    }
    // 가로 스크롤 — 표시 폭 기준으로 자른다.
    let (shown, shown_deco) = slice_by_width_deco(&cleaned, &cleaned_deco, app.left, width);

    match app.query.as_deref() {
        Some(q) if !q.is_empty() && shown.contains(q) => {
            let mut rest = shown.as_str();
            while let Some(at) = rest.find(q) {
                if at > 0 {
                    spans.push(Span::raw(rest[..at].to_string()));
                }
                spans.push(Span::styled(q.to_string(), theme::hit()));
                rest = &rest[at + q.len()..];
            }
            if !rest.is_empty() {
                spans.push(Span::raw(rest.to_string()));
            }
        }
        _ => match cursor_col {
            // 커서 자리를 쪼개서 그 한 칸만 반전시킨다. 조합 중인 글자가 있으면
            // 그 자리에 보여준다 — 두벌식은 확정 전 음절을 화면에 내야 한다.
            Some(col) => {
                let (head, head_d) = slice_by_width_deco(&shown, &shown_deco, 0, col);
                let (rest, rest_d) =
                    slice_by_width_deco(&shown, &shown_deco, col, width.saturating_sub(col));
                let preedit = app.buf.jamo.preedit();
                spans.extend(styled_spans(&head, &head_d));
                if !preedit.is_empty() {
                    spans.push(Span::styled(preedit, Style::new().underlined().bold()));
                    spans.extend(styled_spans(&rest, &rest_d));
                } else {
                    let mut it = rest.chars();
                    match it.next() {
                        Some(c) => {
                            // 커서 칸은 반전이 이긴다 — 어디 있는지가 먼저다
                            spans.push(Span::styled(c.to_string(), Style::new().reversed()));
                            let tail: String = it.collect();
                            let n = rest_d.len().saturating_sub(tail.chars().count());
                            spans.extend(styled_spans(&tail, &rest_d[n.min(rest_d.len())..]));
                        }
                        None => spans.push(Span::styled(" ", Style::new().reversed())),
                    }
                }
            }
            // 찾은 글자의 노란 바탕은 꾸밈보다 세게 — 찾는 중에는 그게 먼저다
            None => spans.extend(styled_spans(&shown, &shown_deco)),
        },
    }
    Line::from(spans)
}

/// 표시 폭 기준으로 `skip` 칸을 버리고 `take` 칸을 가져온다. 2칸 글자를 쪼개지
/// 않는다 — 쪼개면 터미널 출력이 어긋난다.
/// [`slice_by_width`] 와 **같은 걸음**으로 자르면서 글자별 꾸밈도 같이 자른다.
///
/// 따로 자르면 한 글자라도 어긋나는 순간 꾸밈이 옆 글자로 번진다.
fn slice_by_width_deco(s: &str, deco: &[u8], skip: usize, take: usize) -> (String, Vec<u8>) {
    let mut out = String::new();
    let mut marks = Vec::new();
    let mut seen = 0usize;
    let mut used = 0usize;
    for (i, ch) in s.chars().enumerate() {
        let w = hwp_core::wrap::char_width(ch);
        if seen + w <= skip {
            seen += w;
            continue;
        }
        if used + w > take {
            break;
        }
        out.push(ch);
        marks.push(deco.get(i).copied().unwrap_or(0));
        used += w;
    }
    (out, marks)
}

/// 표의 한 줄. **표 안에 들어가 있으면 지금 칸을 뒤집어 보여준다.**
///
/// 선으로만 보여주면 "어느 칸인지" 를 사람이 눈으로 따라가야 하고, 선 문자는
/// 터미널마다 폭이 달라 그 눈마저 못 믿는다. 칸을 뒤집으면 구조가 선이 아니라
/// **커서**로 전달된다 — 선이 ASCII 로 바뀌어도 같다.
fn grid_line<'a>(app: &'a App, table: usize, line: &'a str, y: usize) -> Line<'a> {
    let gut = Span::raw(" ".repeat(GUTTER));
    let plain = || Line::from(vec![Span::raw(" ".repeat(GUTTER)), Span::styled(line.to_string(), theme::border())]);
    let Some((t, row, col)) = app.cell_cursor else {
        return plain();
    };
    if t != table {
        return plain();
    }
    let Some(sp) = app
        .spots
        .get(t)
        .and_then(|v| v.iter().find(|s| s.row == row && s.col == col))
    else {
        return plain();
    };
    // 이 줄이 그 칸이 걸친 범위 안인가
    if y < sp.y || y >= sp.y + sp.h.max(1) {
        return plain();
    }
    // 칸의 가로 범위만 건드린다 — 테두리는 그대로 둔다
    let head = slice_by_width_deco(line, &[], 0, sp.x).0;
    let body = slice_by_width_deco(line, &[], sp.x, sp.w).0;
    let tail_from = sp.x + sp.w;
    let tail = slice_by_width_deco(line, &[], tail_from, usize::MAX / 4).0;

    // **칸을 고치는 중이면 칸 전체가 아니라 커서 글자 하나를 뒤집는다.**
    //
    // 칸 전체를 뒤집으면 어디를 고치는지 알 수 없다 — 글자를 치는 동안 커서가
    // 어디 있는지가 전부다.
    if app.buf.editing_cell() == Some((app.doc.tables[t].index, row, col)) {
        if let Some((cy, cx, cw)) = app.cell_caret(t, row, col, sp) {
            if cy == y {
                let before = slice_by_width_deco(&body, &[], 0, cx).0;
                let caret = slice_by_width_deco(&body, &[], cx, cw).0;
                let after = slice_by_width_deco(&body, &[], cx + cw, usize::MAX / 4).0;
                // 끝에 서 있으면 뒤집을 글자가 없다 — 빈 칸을 하나 세운다
                let caret = if caret.trim().is_empty() { " ".to_string() } else { caret };
                return Line::from(vec![
                    gut,
                    Span::styled(head, theme::border()),
                    Span::styled(before, Style::new()),
                    Span::styled(caret, Style::new().reversed()),
                    Span::styled(after, Style::new()),
                    Span::styled(tail, theme::border()),
                ]);
            }
            return Line::from(vec![
                gut,
                Span::styled(head, theme::border()),
                Span::styled(body, Style::new()),
                Span::styled(tail, theme::border()),
            ]);
        }
    }

    Line::from(vec![
        gut,
        Span::styled(head, theme::border()),
        Span::styled(body, Style::new().reversed()),
        Span::styled(tail, theme::border()),
    ])
}

/// 꾸밈 바이트 하나를 터미널 꾸밈으로. ANSI SGR 1·3·4·9 에 그대로 있다.
fn deco_style(b: u8) -> Style {
    use ratatui::style::Modifier;
    let mut st = Style::new();
    let d = hwp_core::witness::Deco::from_u8(b);
    if d.bold {
        st = st.add_modifier(Modifier::BOLD);
    }
    if d.italic {
        st = st.add_modifier(Modifier::ITALIC);
    }
    if d.underline {
        st = st.add_modifier(Modifier::UNDERLINED);
    }
    if d.strike {
        st = st.add_modifier(Modifier::CROSSED_OUT);
    }
    st
}

/// 글자별 꾸밈을 **붙은 것끼리 묶어** Span 으로 만든다.
///
/// 꾸밈이 없으면 Span 하나다 — 코퍼스 단락의 77% 가 그 경우이므로 그 길이
/// 싸야 한다.
fn styled_spans(text: &str, deco: &[u8]) -> Vec<Span<'static>> {
    if deco.iter().all(|b| *b == 0) {
        return vec![Span::raw(text.to_string())];
    }
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut cur = String::new();
    let mut cur_b: Option<u8> = None;
    for (i, c) in text.chars().enumerate() {
        let b = deco.get(i).copied().unwrap_or(0);
        if cur_b.is_some_and(|x| x != b) {
            out.push(Span::styled(std::mem::take(&mut cur), deco_style(cur_b.unwrap())));
        }
        cur_b = Some(b);
        cur.push(c);
    }
    if let Some(b) = cur_b {
        out.push(Span::styled(cur, deco_style(b)));
    }
    out
}

/// 표시 폭 기준으로 자른다 — **시험용 겉면.**
///
/// 본체는 꾸밈까지 같이 자르는 [`slice_by_width_deco`] 다. 둘을 따로 짜면
/// 한 글자만 어긋나도 꾸밈이 옆 글자로 번지므로 걸음은 하나만 둔다. 자르는
/// 규칙 자체(2칸 글자를 쪼개지 않는다)는 꾸밈과 무관하게 시험할 값이라
/// 이 겉면으로 시험한다.
#[cfg(test)]
pub fn slice_by_width(s: &str, skip: usize, take: usize) -> String {
    slice_by_width_deco(s, &[], skip, take).0
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use hwp_core::screen::{Block as SBlock, Column, Page};
    use hwp_core::wrap::ColumnWidth;
    use ratatui::backend::TestBackend;

    /// **내어쓰기와 가운데 정렬이 그려지고, 빈 줄에는 꼬리 공백이 안 붙는다.**
    #[test]
    fn hanging_indent_and_centering_are_drawn_but_blank_lines_stay_clean() {
        use hwp_core::screen::Indent;
        use hwp_core::witness::ParaAlign;

        let column = 40usize;
        // 본문 폭 40칸 = 4000 HWPUNIT 로 두면 100 HWPUNIT = 1칸이다
        let body_width = 4000u32;
        let doc = ScreenDoc {
            paras: Default::default(),
            path: "/tmp/들여쓰기.hwp".into(),
            format: "hwp5".into(),
            page_count: 1,
            para_count: 3,
            section_counts: vec![3],
            tables: Vec::new(),
            body_width,
            // 단락0: 내어쓰기 4칸 · 단락1: 가운데 · 단락2: 가운데인데 빈 줄
            indents: [((0, 0), Indent { first: 0, rest: 400 })].into_iter().collect(),
            aligns: [((0, 1), ParaAlign::Center), ((0, 2), ParaAlign::Center)]
                .into_iter()
                .collect(),
            runs: Default::default(),
            objects: Default::default(),
            outline: Vec::new(),
            column: Column { cells: column, measured: None },
            pages: vec![Page {
                number: 1,
                printed: Some(1),
                section: 0,
                blocks: (0..3)
                    .map(|p| SBlock {
                        section: 0,
                        para_index: Some(p),
                        lines: vec![String::new()],
                        continues: false,
                        continued: false,
                    })
                    .collect(),
            }],
        }.with_paras_from_blocks();
        let mut buf = Buffer::from_doc(&doc);
        // 두 줄로 접히게 긴 글을 넣는다
        buf.goto((0, 0));
        buf.replace_paragraph((0, 0), "○ 가나다라마바사아자차카타파하 가나다라마바사아자차카타파하");
        buf.replace_paragraph((0, 1), "가운데");
        buf.replace_paragraph((0, 2), "");

        let rows = flatten_with_art(
            &doc,
            &buf,
            column,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            &[],
            false,
            &[],
        );
        let text = |p: u32| -> Vec<String> {
            rows.iter()
                .filter_map(|r| match r {
                    Row::Line { para: Some(x), text, .. } if *x == p => Some(text.clone()),
                    _ => None,
                })
                .collect()
        };

        // 내어쓰기: 첫 줄은 붙고 이어지는 줄은 4칸 들어간다
        let p0 = text(0);
        assert!(p0.len() >= 2, "한 줄로 끝났다 — 접히는 폭을 다시 보라: {p0:?}");
        assert!(!p0[0].starts_with(' '), "첫 줄이 들어갔다: {:?}", p0[0]);
        assert!(
            p0[1].starts_with("    ") && !p0[1].starts_with("     "),
            "이어지는 줄이 4칸 들어가지 않았다: {:?}",
            p0[1]
        );

        // 가운데: 글이 가운데로 간다
        let p1 = text(1);
        let lead = p1[0].len() - p1[0].trim_start().len();
        assert!(lead > 10, "가운데로 가지 않았다 (앞 공백 {lead}칸): {:?}", p1[0]);

        // 빈 줄: 가운데 정렬이어도 공백이 붙지 않는다
        for line in text(2) {
            assert!(line.is_empty(), "빈 줄에 꼬리 공백이 붙었다: {line:?}");
        }
    }

    /// **표 칸 안의 그림은 칸 안에만 그린다** — 본문에 또 그리지 않는다.
    ///
    /// 두 곳에 그리면 같은 그림이 표 안에도, 표 밑에도 나온다.
    #[test]
    fn an_image_in_a_cell_is_drawn_in_the_cell_and_not_again_below() {
        use hwp_core::images::{DocImage, ImageCell};
        use hwp_core::screen::PlacedTable;

        let table = hwp_core::grid::Table {
            rows: 1,
            cols: 2,
            cells: (0..2)
                .map(|col| hwp_core::grid::Cell {
                    row: 0,
                    col,
                    row_span: 1,
                    col_span: 1,
                    text: String::new(),
                    art: Vec::new(),
                    min_width: 0,
                    doc_width: 0,
                    align: Default::default(),
                })
                .collect(),
        };
        let doc = ScreenDoc {
            paras: Default::default(),
            path: "/tmp/로고표.hwp".into(),
            format: "hwp5".into(),
            page_count: 1,
            para_count: 1,
            section_counts: vec![1],
            tables: vec![PlacedTable { at: (0, 0), control: 0, index: 7, table }],
            body_width: 48188,
            indents: Default::default(),
            aligns: Default::default(),
            runs: Default::default(),
            objects: Default::default(),
            outline: Vec::new(),
            column: Column { cells: 60, measured: None },
            pages: vec![Page {
                number: 1,
                printed: Some(1),
                section: 0,
                blocks: vec![SBlock {
                    section: 0,
                    para_index: Some(0),
                    lines: vec![String::new()],
                    continues: false,
                    continued: false,
                }],
            }],
        }.with_paras_from_blocks();

        // placeholder 로 시험한다 — 자리를 고르는 쪽만 보면 되고, 그러면
        // 그릴 수 있는 그림 바이트가 필요 없다
        let img = DocImage {
            index: 0,
            at: Some((0, 0)),
            name: "BinData/BIN0001.png".into(),
            bytes: Vec::new(),
            // 본문 폭의 절반 — 칸 하나를 꽉 채울 크기
            placed: Some((24094, 6000)),
            inline: true,
            align: hwp_core::images::Align::Left,
            cell: Some(ImageCell { table: 7, row: 0, col: 1 }),
        };

        let mut app = App::new(doc, false);
        app.load_images(vec![img], ImageMode::Placeholder);

        // 본문 쪽에는 아무것도 없다
        assert!(app.art.is_empty(), "칸 안 그림이 본문에도 그려졌다");
        assert!(
            !app.rows.iter().any(|r| matches!(r, Row::Art { .. })),
            "표 밑에 그림 행이 생겼다"
        );
        // 표의 둘째 칸에 그림이 들어갔다
        let cells = &app.doc.tables[0].table.cells;
        assert!(cells[0].art.is_empty(), "엉뚱한 칸에 그렸다");
        assert!(!cells[1].art.is_empty(), "그림이 든 칸이 비어 있다");
        assert!(
            app.grids[0].iter().any(|l| l.contains("그림 1")),
            "그려진 표에 그림 자리가 없다:\n{}",
            app.grids[0].join("\n")
        );
    }

    /// **그림만 있는 빈 단락을 조판기가 두 번 올려도 그림은 한 번만 그린다.**
    ///
    /// 조판기는 그림 하나뿐인 단락을 `FullParagraph` 와 `Shape` 두 줄로
    /// 올린다(s00.hwp 의 `pi=54` 가 실제로 그렇다). 빈 단락이라 "단락을 다
    /// 찍었다"는 조건이 두 번 다 참이어서 같은 배너가 두 장 나왔다.
    #[test]
    fn a_shape_only_paragraph_listed_twice_draws_its_image_once() {
        let doc = ScreenDoc {
            paras: Default::default(),
            path: "/tmp/배너.hwp".into(),
            format: "hwp5".into(),
            page_count: 1,
            para_count: 1,
            section_counts: vec![1],
            tables: Vec::new(),
            body_width: 48188,
            indents: Default::default(),
            aligns: Default::default(),
            runs: Default::default(),
            objects: Default::default(),
            outline: Vec::new(),
            column: Column { cells: 30, measured: None },
            pages: vec![Page {
                number: 1,
                printed: Some(1),
                section: 0,
                blocks: vec![
                    // 조판기가 내놓는 그대로 — 같은 빈 단락이 두 줄
                    SBlock {
                        section: 0,
                        para_index: Some(0),
                        lines: vec![String::new()],
                        continues: false,
                        continued: false,
                    },
                    SBlock {
                        section: 0,
                        para_index: Some(0),
                        lines: vec![String::new()],
                        continues: false,
                        continued: false,
                    },
                ],
            }],
        }.with_paras_from_blocks();
        let buf = Buffer::from_doc(&doc);
        let img = hwp_core::images::DocImage {
            index: 0,
            at: Some((0, 0)),
            name: "BinData/BIN0001.png".into(),
            bytes: Vec::new(),
            placed: Some((41537, 3217)),
            inline: true,
            align: hwp_core::images::Align::Left,
            cell: None,
        };
        let mut art = std::collections::HashMap::new();
        art.insert(0usize, vec!["⣿⣿⣿".to_string()]);

        let rows = flatten_with_art(&doc, &buf, 30, &art, &Default::default(), &[img], false, &[]);
        let drawn = rows
            .iter()
            .filter(|r| matches!(r, Row::Art { image: 0, caption: false, .. }))
            .count();
        assert_eq!(drawn, 1, "그림이 {drawn}번 그려졌다 — 한 번이어야 한다");
    }

    /// **문자표가 상자 안에 머무는가 — 모든 묶음·모든 기호에서.**
    ///
    /// 기호는 **내용**이라 ASCII 로 바꿀 수 없다(`○` 를 `o` 로 보여 주면 고른
    /// 것과 넣는 것이 달라진다). 그래서 두 칸짜리 기호가 섞여 있고, 메뉴에서
    /// 당했던 "글자 수로 채워서 테두리를 지운다" 가 여기서도 날 수 있다.
    /// 안내줄이 상자 밖으로 잘렸던 것도 메뉴에서 났던 일이다.
    #[test]
    fn the_symbol_table_stays_inside_its_box_in_every_group() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::new(fixture(), false);
        app.editable = true;
        for g in 0..hwp_core::symbols::group_count() {
            let (_, items) = hwp_core::symbols::group(g).expect("묶음");
            for i in 0..items.len() {
                app.open_symbols();
                if let Some(pk) = app.picker.as_mut() {
                    pk.group = g;
                    pk.item = i;
                    pk.top = (i / App::PICK_PER_ROW)
                        .saturating_sub(App::PICK_ROWS.saturating_sub(1));
                }
                let mut term = Terminal::new(TestBackend::new(80, 24)).expect("백엔드");
                term.draw(|f| draw(f, &app)).expect("그리기");
                let buf = term.backend().buffer().clone();

                // 테두리 세로줄이 한 줄에 둘씩 — 글자가 넘치면 하나가 덮인다
                let wall = if hwp_core::wrap::ambiguous_wide() { "|" } else { "│" };
                for y in 0..buf.area.height {
                    let n = (0..buf.area.width)
                        .filter(|x| buf[(*x, y)].symbol() == wall)
                        .count();
                    assert!(
                        n != 1,
                        "묶음{g} {i}번째에서 {y}줄 테두리가 하나뿐이다 — 글자가 넘쳤다"
                    );
                }

                // 고른 기호의 **뜻**이 상자 안에 보여야 한다 — 모양만 보면
                // `○` `◦` `◯` 를 가릴 수 없다
                let screen: String = (0..buf.area.height)
                    .map(|y| {
                        let mut line = String::new();
                        let mut x = 0u16;
                        while x < buf.area.width {
                            let sym = buf[(x, y)].symbol();
                            line.push_str(sym);
                            x += hwp_core::wrap::display_width(sym).max(1) as u16;
                        }
                        line
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(screen.contains("넣기"), "묶음{g} {i}번째: 안내줄이 잘렸다");
            }
        }
    }

    /// **색이 화면에 실제로 서로 다르게 들어가는가.**
    ///
    /// "화면에서 거의 구분이 안 된다" 는 보고의 답이다(2026-10-12). 1비트로
    /// 떨구면 로고가 통째로 켜지거나 꺼진다 — 실측으로 1315×345 로고가
    /// 34칸×4줄(34×8점)이었다. 색은 같은 칸에서 픽셀이 둘이고 **색 자체가
    /// 정보**다.
    ///
    /// 눈으로 못 보니 **TestBackend 의 칸 색을 센다.**
    #[test]
    fn a_body_image_reaches_the_screen_as_many_different_colors() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use std::collections::BTreeSet;

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("루트")
            .to_path_buf();
        let src = root.join("samples/s00.hwp");
        if !src.exists() {
            return;
        }
        let Ok(pin) = hwp_core::Pin::load_default() else { return };
        let Ok(rhwp) = hwp_core::Rhwp::discover(&pin) else { return };
        let Ok(doc) = hwp_core::ScreenDoc::build(&rhwp, &src) else { return };

        let mut app = App::new(doc, false);
        app.load_images(
            hwp_core::images::read(&src).unwrap_or_default(),
            ImageMode::default_mode(),
        );
        assert_eq!(app.image_mode, ImageMode::Color, "기본이 색이어야 한다");

        // 본문 그림이 그려진 화면을 찾아 색을 센다
        let mut term = Terminal::new(TestBackend::new(90, 40)).expect("백엔드");
        let mut best = (0usize, 0usize);
        for top in 0..app.rows.len() {
            app.top = top;
            term.draw(|f| draw(f, &app)).expect("그리기");
            let buf = term.backend().buffer().clone();
            let mut pairs = BTreeSet::new();
            let mut cells = 0usize;
            for y in 0..buf.area.height {
                for x in 0..buf.area.width {
                    let c = &buf[(x, y)];
                    if c.symbol() == "▀" {
                        cells += 1;
                        pairs.insert((format!("{:?}", c.fg), format!("{:?}", c.bg)));
                    }
                }
            }
            if pairs.len() > best.1 {
                best = (cells, pairs.len());
            }
        }
        let (cells, colors) = best;
        eprintln!("본문 그림: 반블록 칸 {cells}개 · 서로 다른 (글자색,바탕색) {colors}가지");
        assert!(cells > 100, "반블록 칸이 {cells}개뿐이다 — 색으로 안 그렸다");
        // **1비트면 두 가지(켜짐·꺼짐)뿐이다.** 그보다 훨씬 많아야 한다.
        assert!(
            colors > 20,
            "색이 {colors}가지뿐이다 — 1비트와 다를 바 없다 (칸 {cells}개)"
        );
    }

    /// `NO_COLOR` 면 색을 쓰지 않는다 — 1비트 점 그림으로 간다.
    #[test]
    fn no_color_falls_back_to_one_bit_art() {
        // `theme::colored()` 는 한 번만 읽으므로 이 과정에서 바꿀 수 없다.
        // 대신 **모드 선택이 그 값을 따르는지**를 본다.
        let want = if theme::colored() {
            ImageMode::Color
        } else {
            ImageMode::Art(hwp_core::dots::Glyphs::Blocks)
        };
        assert_eq!(ImageMode::default_mode(), want);
        // 색 모드에서도 `z` 로 돌면 반블록 → 점자 → ASCII → 자리만 → 끔 이다
        let mut m = ImageMode::Color;
        let mut seen = vec![m];
        for _ in 0..6 {
            m = m.next();
            seen.push(m);
        }
        assert!(seen.contains(&ImageMode::Art(hwp_core::dots::Glyphs::Braille)));
        assert!(seen.contains(&ImageMode::Placeholder));
        assert!(seen.contains(&ImageMode::None));
        // 한 바퀴 돌아 처음으로 — 기본값으로 돌아온다
        assert_eq!(seen.last().copied(), Some(ImageMode::default_mode()));
    }

    /// rhwp 없이 손으로 세운 문서 — 그리는 쪽만 시험한다.
    pub(crate) fn fixture() -> ScreenDoc {
        ScreenDoc {
            paras: Default::default(),
            path: "/tmp/과업지시서.hwp".into(),
            format: "hwp5".into(),
            page_count: 2,
            para_count: 3,
            section_counts: vec![3],
            // 글머리가 있는 fixture — 개요 보기를 시험할 수 있게
            tables: Vec::new(),
            body_width: 0,
            indents: Default::default(),
            aligns: Default::default(),
            runs: Default::default(),
            objects: Default::default(),
            outline: hwp_core::outline::build(&[
                hwp_core::outline::Item { section: 0, para: 0, text: "□ 과 업 지 시 서".into() },
                hwp_core::outline::Item { section: 0, para: 1, text: " ○ 소요예산: 금19,800,000원".into() },
            ]),
            column: Column {
                cells: 30,
                measured: Some(ColumnWidth { cells: 30, narrowest: 29, samples: 4 }),
            },
            pages: vec![
                Page {
                    number: 1,
                    printed: Some(1),
                    section: 0,
                    blocks: vec![
                        SBlock {
                            section: 0,
                            para_index: Some(0),
                            lines: vec!["과 업 지 시 서".into()],
                            continues: false,
                            continued: false,
                        },
                        SBlock {
                            section: 0,
                            para_index: Some(1),
                            lines: vec!["○ 소요예산: 금19,800,000원".into()],
                            continues: true,
                            continued: false,
                        },
                    ],
                },
                Page {
                    number: 2,
                    printed: Some(2),
                    section: 0,
                    blocks: vec![SBlock {
                        section: 0,
                        para_index: Some(1),
                        lines: vec!["(부가세 포함)".into()],
                        continues: false,
                        continued: true,
                    }],
                },
            ],
        }.with_paras_from_blocks()
    }

    fn render(app: &App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        // 2칸 글자는 다음 칸을 함께 차지하고, 그 칸의 심볼은 공백이다. 한 칸씩
        // 이어붙이면 "과 업 지 시 서" 가 되므로, 방금 쓴 글자의 폭만큼 건너뛴다.
        (0..buf.area.height)
            .map(|y| {
                let mut line = String::new();
                let mut x = 0u16;
                while x < buf.area.width {
                    let sym = buf[(x, y)].symbol();
                    line.push_str(sym);
                    let w = display_width(sym).max(1) as u16;
                    x += w;
                }
                line.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn flatten_puts_a_rule_before_each_page() {
        let doc = fixture();
        let buf = Buffer::from_doc(&doc);
        let rows = flatten_with_art(
            &doc,
            &buf,
            doc.column.cells,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            &[],
            false,
            &[],
        );
        let rules: Vec<u32> = rows
            .iter()
            .filter_map(|r| match r {
                Row::PageRule { number, .. } => Some(*number),
                _ => None,
            })
            .collect();
        assert_eq!(rules, vec![1, 2]);
        assert!(rows.contains(&Row::Continues), "이어짐 표시가 있어야 한다");
    }

    /// **메뉴가 화살표로 움직이고, 테두리를 넘지 않는다.**
    ///
    /// 넘치는지는 `--frame` 출력으로 못 본다 — 꾸밈 표시(`«»`)가 한 칸씩
    /// 차지해 뒤를 밀기 때문이다. 터미널 칸을 직접 읽어야 한다.
    #[test]
    fn the_menu_moves_with_arrows_and_stays_inside_its_box() {
        use crate::run_pager::{Action, action_for_menu, apply, menu_groups};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::new(fixture(), false);
        app.editable = true;
        app.open_menu();
        assert_eq!(app.menu, Some((0, 0)));

        // 화살표가 묶음과 항목을 옮긴다
        let k = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);
        assert_eq!(action_for_menu(k(KeyCode::Down)), Action::MenuMove(1));
        assert_eq!(action_for_menu(k(KeyCode::Up)), Action::MenuMove(-1));
        assert_eq!(action_for_menu(k(KeyCode::Right)), Action::MenuGroup(1));
        assert_eq!(action_for_menu(k(KeyCode::Left)), Action::MenuGroup(-1));
        assert_eq!(action_for_menu(k(KeyCode::Enter)), Action::MenuPick);
        assert_eq!(action_for_menu(k(KeyCode::Esc)), Action::CloseMenu);

        apply(&mut app, Action::MenuMove(1), 20);
        assert_eq!(app.menu, Some((0, 1)), "아래로 안 갔다");
        // 묶음 끝에서 **멈춘다** — 다음 묶음으로 넘어가면 어디 있는지 놓친다
        for _ in 0..50 {
            apply(&mut app, Action::MenuMove(1), 20);
        }
        let (g, i) = app.menu.expect("메뉴");
        assert_eq!(g, 0, "묶음이 바뀌었다");
        assert_eq!(i, menu_groups()[0].1.len() - 1, "마지막 항목에 멈춰야 한다");

        // 묶음은 감는다
        apply(&mut app, Action::MenuGroup(-1), 20);
        assert_eq!(app.menu.map(|(g, _)| g), Some(menu_groups().len() - 1));
        apply(&mut app, Action::MenuGroup(1), 20);
        assert_eq!(app.menu.map(|(g, _)| g), Some(0), "감아서 돌아와야 한다");

        // **모든 묶음의 모든 항목에서** 그린 줄이 화면을 넘지 않는다
        for (gi, (_, items)) in menu_groups().iter().enumerate() {
            for ii in 0..items.len() {
                app.menu = Some((gi, ii));
                let mut term = Terminal::new(TestBackend::new(80, 20)).expect("백엔드");
                term.draw(|f| draw(f, &app)).expect("그리기");
                let buf = term.backend().buffer().clone();
                // 테두리 `│` 가 한 줄에 둘씩 — 글자가 그 밖으로 나가면 하나가
                // 글자에 덮여 사라진다
                for y in 0..buf.area.height {
                    let mut border = 0;
                    for x in 0..buf.area.width {
                        if buf[(x, y)].symbol() == "│" {
                            border += 1;
                        }
                    }
                    assert!(
                        border != 1,
                        "메뉴 {gi}묶음 {ii}번째에서 {y}줄의 테두리가 하나뿐이다 — 글자가 넘쳤다"
                    );
                }

                // **안내줄이 상자 안에 실제로 보이는가.**
                //
                // 폭은 내용에서 재면서 높이는 손으로 센 수(`items+5`)를 썼고,
                // 한 줄이 모자라 `↑↓ 고르기 · ↵ 실행` 이 **한 번도 뜨지
                // 않았다.** 잘린 도움은 "없는 기능" 으로 읽힌다. 그려 보고
                // 재지 않으면 알 수 없는 종류의 버그다.
                // 2칸 글자는 다음 칸을 **함께** 차지한다. 칸마다 기호를
                // 이어 붙이면 뒤 칸의 빈 기호가 끼어들어 `실 행` 이 된다 —
                // 처음에 그렇게 써서 보이는 줄을 "잘렸다" 고 했다.
                let screen: String = (0..buf.area.height)
                    .map(|y| {
                        let mut line = String::new();
                        let mut x = 0u16;
                        while x < buf.area.width {
                            let sym = buf[(x, y)].symbol();
                            line.push_str(sym);
                            x += hwp_core::wrap::display_width(sym).max(1) as u16;
                        }
                        line
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(
                    screen.contains("실행"),
                    "메뉴 {gi}묶음 {ii}번째: 안내줄이 상자 밖으로 잘렸다\n{screen}"
                );
            }
        }

        // 고른 것이 동작을 낸다 — 메뉴와 키가 **한 길**이다
        app.menu = Some((1, 0));
        assert_eq!(app.menu_pick(), Some(Action::Undo), "편집>되돌리기");

        // Esc 로 닫힌다
        apply(&mut app, Action::CloseMenu, 20);
        assert!(app.menu.is_none());
    }

    /// **도움줄이 늘 있고, 모드마다 다른 키를 말한다.**
    ///
    /// 명령이 늘어나면 외우게 하는 대신 화면에 둬야 한다. 알림이 그 자리를
    /// 빼앗으면 "뭘 눌러야 하지" 가 돌아오므로 **다른 줄**에 둔다.
    #[test]
    fn the_hint_bar_is_always_there_and_follows_the_mode() {
        let mut app = App::new(fixture(), false);

        // 읽기 — 알림이 있어도 도움줄은 그대로다
        app.message = "무슨 알림".into();
        let out = render(&app, 90, 10);
        assert!(out.contains("무슨 알림"), "알림이 사라졌다:\n{out}");
        assert!(out.contains("읽기"), "모드 이름이 없다:\n{out}");
        assert!(out.contains("도움"), "? 안내가 없다:\n{out}");
        assert!(out.contains("나가기"), "나가는 법이 없다:\n{out}");

        // 고치기 — 편집 키를 말한다
        app.editable = true;
        // **좁은 터미널에서도** 도움과 나가는 법은 보여야 한다 — 뒤는 잘린다
        let out = render(&app, 60, 10);
        assert!(out.contains("고치기"), "{out}");
        assert!(out.contains("도움"), "60칸에서 ? 안내가 잘렸다:\n{out}");
        assert!(out.contains("저장하고 나가기"), "60칸에서 저장법이 잘렸다:\n{out}");
        // 넓으면 뒤까지 나온다
        let wide = render(&app, 110, 10);
        assert!(wide.contains("담기·붙이기"), "넓은데도 뒤가 안 나온다:\n{wide}");

        // 표 — 칸 키로 바뀐다
        app.mode = Mode::Table;
        let out = render(&app, 90, 10);
        // 아래 도움줄이 **칸 이동 키를 말하는가.** 어느 벌을 적는지는
        // 바뀔 수 있으니(hjkl → 화살표·wasd) 한 벌이라도 적혔는지만 본다.
        assert!(
            ["wasd", "hjkl", "←↓↑→"].iter().any(|k| out.contains(k)),
            "칸 이동 키가 없다:\n{out}"
        );
        assert!(out.contains("다음 칸"), "{out}");
        // 본문 키는 이제 말하지 않는다 — 모드에 없는 것을 말하면 더 헷갈린다
        assert!(!out.contains("담기·붙이기"), "표 모드에 본문 키가 남았다:\n{out}");
    }

    /// **`?` 가 전부 보여주고 Esc 로 닫힌다.**
    #[test]
    fn the_help_panel_shows_everything_and_closes() {
        let mut app = App::new(fixture(), false);
        assert!(!app.help);

        app.help = true;
        let out = render(&app, 96, 24);
        // 첫 화면에 보이는 묶음
        for want in ["보기", "고치기", "저장·나가기"] {
            assert!(out.contains(want), "`{want}` 묶음이 없다:\n{out}");
        }
        // **넘칠 때는 그렇다고 말한다** — 잘린 채로 두면 없는 기능으로 읽힌다
        assert!(out.contains("더 있다"), "넘치는데 알리지 않았다:\n{out}");

        // **화살표로 넘기면 모든 묶음이 나온다.**
        //
        // 고정 횟수로 넘기지 않는다 — 키가 늘면 그 수가 모자라져 "안 나온다"
        // 로 터진다(실제로 그렇게 터졌다). 끝까지 넘기면서 **지나간 것을
        // 모아** 본다: 묶음이 늘어도 그대로 선다.
        let mut seen = String::new();
        for _ in 0..200 {
            crate::run_pager::apply(&mut app, crate::run_pager::Action::Lines(1), 20);
            seen.push_str(&render(&app, 96, 24));
        }
        let down = render(&app, 96, 24);
        for want in ["도스 시절 키", "메뉴·찾기·개요·표"] {
            assert!(seen.contains(want), "`{want}` 가 넘겨도 안 나온다:\n{down}");
        }
        // 끝에서 멈춘다
        assert!(down.contains("위에 더"), "끝에 왔다고 말하지 않는다:\n{down}");
        // 다시 켜면 처음으로 돌아간다
        crate::run_pager::apply(&mut app, crate::run_pager::Action::ToggleHelp, 20);
        crate::run_pager::apply(&mut app, crate::run_pager::Action::ToggleHelp, 20);
        assert_eq!(app.help_scroll, 0, "다시 켰는데 내려가 있다");
        // 닫는 법을 말한다
        assert!(out.contains("닫는다"), "닫는 법이 없다:\n{out}");
        // 뒤의 본문을 **지운다** — 안 지우면 글자가 겹쳐 둘 다 못 읽는다
        assert!(
            !out.contains("과 업 지 시 서"),
            "본문이 도움 보기 뒤로 비친다:\n{out}"
        );
        // 도움줄은 남는다 — 어떻게 닫는지가 화면에 있어야 한다
        assert!(out.contains("도움"), "도움줄이 사라졌다:\n{out}");
    }

    /// **표 안에서 커서가 칸을 넘나들고, 그 칸이 뒤집혀 보인다.**
    ///
    /// 실제 문서로 확인한다 — 어느 칸이 뒤집혔는지는 터미널 칸의 수식어를
    /// 직접 봐야 알 수 있다(`--frame` 출력은 글자만 찍는다).
    #[test]
    fn the_cursor_walks_table_cells_and_the_current_cell_is_highlighted() {
        use hwp_core::{Pin, Rhwp};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Modifier;

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("루트")
            .to_path_buf();
        let src = root.join("samples/s16.hwp");
        if !src.exists() {
            return;
        }
        let Ok(pin) = Pin::load_default() else { return };
        let Ok(rhwp) = Rhwp::discover(&pin) else { return };
        let doc = ScreenDoc::build(&rhwp, &src).expect("화면 모델");
        assert!(!doc.tables.is_empty(), "표가 있는 문서가 필요하다");

        let mut app = App::new(doc, false);
        app.load_images(Vec::new(), ImageMode::None);

        // **2행 2열 이상인 표를 골라야** 칸 이동을 시험할 수 있다 — 1칸짜리
        // 표도 흔하다(s16.hwp 의 첫 표가 그렇다).
        let Some(ti) = app
            .doc
            .tables
            .iter()
            .position(|t| t.table.rows >= 2 && t.table.cols >= 2 && t.table.cells.len() >= 4)
        else {
            eprintln!("건너뜀 — 2행 2열 이상인 표가 없다");
            return;
        };
        assert!(app.enter_table_index(ti), "표로 들어가지 못했다");
        assert_eq!(app.mode, Mode::Table);
        let first = app.cell_cursor.expect("칸 커서");

        /// 본문에서 뒤집힌 칸 수. **머리글 줄은 뺀다** — 그 줄 전체가
        /// 원래 반전이라(파일명·쪽 번호) 세면 95칸이 그냥 잡힌다.
        fn reversed(app: &App) -> usize {
            let mut term = Terminal::new(TestBackend::new(100, 24)).expect("백엔드");
            term.draw(|f| draw(f, app)).expect("그리기");
            let buf = term.backend().buffer().clone();
            let mut n = 0;
            for y in 1..buf.area.height {
                for x in 0..buf.area.width {
                    if buf[(x, y)].style().add_modifier.contains(Modifier::REVERSED) {
                        n += 1;
                    }
                }
            }
            n
        }

        let lit = reversed(&app);
        assert!(lit > 0, "지금 칸이 뒤집혀 보이지 않는다");

        // **어느 방향으로 움직이는지는 표가 정한다.** 첫 칸이 행 전체를 병합한
        // 제목 칸이면 오른쪽으로 갈 곳이 없는 게 맞다 — 그걸 실패로 보면
        // 시험이 표의 모양을 단정하는 셈이다. 네 방향 중 하나는 움직여야 한다.
        let mut moved = None;
        for (dr, dc) in [(0, 1), (1, 0), (0, -1), (-1, 0)] {
            app.move_cell(dr, dc);
            if app.cell_cursor != Some(first) {
                moved = app.cell_cursor;
                break;
            }
        }
        let moved = moved.expect("네 방향 어디로도 움직이지 못했다");
        assert_ne!(moved, first, "칸이 움직이지 않았다");
        assert!(reversed(&app) > 0, "옮긴 칸이 뒤집혀 보이지 않는다");

        // Tab 으로 한 칸 더, Shift-Tab 으로 되돌아온다
        let before = app.cell_cursor;
        app.next_cell(true);
        assert_ne!(app.cell_cursor, before, "Tab 이 안 움직였다");
        app.next_cell(false);
        assert_eq!(app.cell_cursor, before, "Shift-Tab 으로 안 돌아왔다");

        // 표 밖으로는 나가지 않는다
        for _ in 0..50 {
            app.move_cell(-1, -1);
        }
        let (t, r, c) = app.cell_cursor.expect("칸 커서");
        let table = &app.doc.tables[t].table;
        assert!(r < table.rows && c < table.cols, "표 밖으로 나갔다: {r}행{c}열");

        // 나오면 본문 모드다
        app.leave_table();
        assert_eq!(app.mode, Mode::Body);
        assert!(app.cell_cursor.is_none());
        assert_eq!(reversed(&app), 0, "나왔는데 아직 뒤집힌 칸이 있다");
    }

    /// **진하게가 화면에 실제로 붙는가** — 실제 문서로 확인한다.
    ///
    /// `--frame` 출력은 글자만 읽으므로 꾸밈이 보이지 않는다. 터미널 칸의
    /// 수식어를 직접 보는 것이 이 쪽에서 확인할 수 있는 유일한 길이다.
    #[test]
    fn bold_reaches_the_terminal_cells() {
        use hwp_core::{Pin, Rhwp};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Modifier;

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("루트")
            .to_path_buf();
        let src = root.join("samples/s13.hwp");
        if !src.exists() {
            return;
        }
        let Ok(pin) = Pin::load_default() else { return };
        let Ok(rhwp) = Rhwp::discover(&pin) else { return };
        let doc = ScreenDoc::build(&rhwp, &src).expect("화면 모델");
        // s13 은 "1. 과업명" 같은 제목 줄이 진하게다 (witness --deco 로 확인)
        assert!(!doc.runs.is_empty(), "꾸밈을 읽지 못했다");

        let mut app = App::new(doc, false);
        app.load_images(Vec::new(), ImageMode::None);
        let mut term = Terminal::new(TestBackend::new(90, 14)).expect("백엔드");
        term.draw(|f| draw(f, &app)).expect("그리기");
        let buf = term.backend().buffer().clone();

        // 줄마다 (글자, 진하게인가) 를 모은다
        let mut rows: Vec<(String, usize, usize)> = Vec::new();
        for y in 0..buf.area.height {
            let mut text = String::new();
            let (mut bold, mut plain) = (0usize, 0usize);
            let mut x = 0u16;
            while x < buf.area.width {
                let cell = &buf[(x, y)];
                let sym = cell.symbol();
                text.push_str(sym);
                if sym.trim() != "" && !cell.style().add_modifier.contains(Modifier::DIM) {
                    if cell.style().add_modifier.contains(Modifier::BOLD) {
                        bold += 1;
                    } else {
                        plain += 1;
                    }
                }
                x += hwp_core::wrap::display_width(sym).max(1) as u16;
            }
            rows.push((text, bold, plain));
        }

        // 제목 줄은 진하게 칸이 있고, 본문 줄은 없다
        let head = rows
            .iter()
            .find(|(t, _, _)| t.contains("1. 과업명"))
            .expect("제목 줄을 못 찾았다");
        assert!(head.1 > 0, "제목 줄에 진하게가 없다: {head:?}");

        let body = rows
            .iter()
            .find(|(t, _, _)| t.contains("자동 생성된"))
            .expect("본문 줄을 못 찾았다");
        assert_eq!(body.1, 0, "민짜 본문에 진하게가 붙었다: {body:?}");
        assert!(body.2 > 0, "본문 글자를 못 셌다: {body:?}");
    }

    #[test]
    fn frame_shows_header_page_rule_and_coordinates() {
        let app = App::new(fixture(), false);
        let out = render(&app, 44, 10);
        assert!(out.contains("과업지시서.hwp"), "머리글에 파일명:\n{out}");
        assert!(out.contains("1 / 2 쪽"), "쪽 경계:\n{out}");
        assert!(out.contains("과 업 지 시 서"), "본문:\n{out}");
        assert!(out.contains("   0"), "단락 좌표:\n{out}");
        assert!(out.contains("이어짐"), "이어짐 표시:\n{out}");
    }

    #[test]
    fn continued_paragraph_is_marked_on_the_next_page() {
        let mut app = App::new(fixture(), false);
        app.go_page(true);
        let out = render(&app, 44, 8);
        assert!(out.contains("2 / 2 쪽"), "둘째 쪽으로 갔어야 한다:\n{out}");
        assert!(out.contains("↳"), "이어진 단락 표시:\n{out}");
        assert_eq!(app.current_page(), 2);
    }

    #[test]
    fn search_highlights_and_jumps() {
        let mut app = App::new(fixture(), false);
        app.search("19,800,000");
        assert_eq!(app.hits.len(), 1);
        let out = render(&app, 44, 8);
        assert!(out.contains("19,800,000"), "검색 결과로 이동:\n{out}");
        assert!(app.message.contains("1곳"));

        app.search("없는말");
        assert!(app.hits.is_empty());
        assert!(app.message.contains("찾지 못했"));
    }

    #[test]
    fn status_line_never_hides_that_the_width_was_guessed() {
        let mut doc = fixture();
        doc.column = Column { cells: 80, measured: None };
        let app = App::new(doc, false);
        let out = render(&app, 60, 6);
        assert!(out.contains("측정 못 함"), "추측했음을 숨기면 안 된다:\n{out}");
    }

    #[test]
    fn toc_view_lists_the_outline_and_marks_the_cursor() {
        let mut app = App::new(fixture(), false);
        assert_eq!(app.toc.len(), 2, "개요 2줄: {:#?}", app.toc);
        app.toggle_toc();
        assert_eq!(app.mode, Mode::Toc);
        let out = render(&app, 50, 8);
        assert!(out.contains("개요 1 / 2줄"), "머리글이 개요 상태를 말해야 한다:\n{out}");
        assert!(out.contains("□ 과 업 지 시 서"), "개요 줄:\n{out}");
        assert!(out.contains("○ 소요예산"), "들여쓴 개요 줄:\n{out}");
    }

    #[test]
    fn selecting_an_outline_line_moves_the_body_there() {
        let mut app = App::new(fixture(), false);
        app.toggle_toc();
        app.move_toc(1);
        assert_eq!(app.toc_cursor, 1);
        app.jump_to_selected();
        assert_eq!(app.mode, Mode::Body, "고르면 본문으로 돌아온다");
        // 그 단락의 첫 줄이 맨 위에 와야 한다
        match &app.rows[app.top] {
            Row::Line { para: Some(p), leads: true, .. } => assert_eq!(*p, 1),
            other => panic!("단락 1 의 첫 줄이어야 한다: {other:?}"),
        }
        assert!(app.message.contains("단락 1"));
    }

    #[test]
    fn toc_without_markers_says_so_instead_of_showing_nothing() {
        let mut doc = fixture();
        doc.outline = Vec::new();
        let mut app = App::new(doc, false);
        app.toggle_toc();
        assert_eq!(app.mode, Mode::Body, "빈 개요로 들어가면 안 된다");
        assert!(app.message.contains("글머리가 없"));
    }

    #[test]
    fn width_slicing_never_splits_a_wide_char() {
        // '과' 는 2칸. 1칸만 가져가려 하면 아무것도 못 가져간다.
        assert_eq!(slice_by_width("과업", 0, 1), "");
        assert_eq!(slice_by_width("과업", 0, 2), "과");
        assert_eq!(slice_by_width("과업", 2, 2), "업");
        assert_eq!(display_width(&slice_by_width("가나다라", 0, 5)), 4);
    }
}

/// 접힌 줄 하나의 글자별 꾸밈을 단락 전체 표에서 잘라 낸다.
///
/// `wrap` 이 공백에서 끊으면 **그 공백을 버린다.** 그래서 줄 길이를 더해 가며
/// 세면 한 글자씩 밀린다. 원문에서 그 줄의 글자를 찾아 자리를 맞춘다 —
/// `cursor` 는 지금까지 소비한 자리이고, 거기서부터 앞으로만 찾는다.
fn slice_deco(para: &[u8], text: &str, line: &str, cursor: &mut usize) -> Vec<u8> {
    let chars: Vec<char> = text.chars().collect();
    let want: Vec<char> = line.chars().collect();
    if want.is_empty() {
        return Vec::new();
    }
    // `cursor` 부터 앞으로 가며 이 줄이 시작하는 자리를 찾는다. 버려진 공백은
    // 많아도 몇 칸이므로 멀리 가지 않는다.
    let mut at = *cursor;
    while at + want.len() <= chars.len() && chars[at..at + want.len()] != want[..] {
        at += 1;
    }
    if at + want.len() > chars.len() {
        // 못 찾았다 — 편집으로 글이 바뀐 뒤일 수 있다. 꾸밈 없이 그린다.
        return Vec::new();
    }
    *cursor = at + want.len();
    para[at..at + want.len()].to_vec()
}
