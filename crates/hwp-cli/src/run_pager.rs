//! 페이저의 터미널 구동부 — 키 입력과 화면 복구.
//!
//! 그리는 쪽([`crate::pager::draw`])과 떼어 두었다. 이 파일은 터미널이 있어야
//! 돌아가므로 시험할 수 없고, 그래서 **판단을 담지 않는다** — 키를 동작으로
//! 옮기는 일만 한다. 어떤 키가 무엇을 하는지는 [`Action`] 이 들고 있고 그건
//! 시험할 수 있다.

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use ratatui::prelude::*;
use std::io::stdout;
use std::time::Duration;

use crate::pager::{App, Mode, draw};
use hwp_core::edit::save;

/// 키가 뜻하는 일.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Quit,
    /// 행 단위 이동 (음수 = 위)
    Lines(isize),
    /// 화면 단위 이동
    Screens(isize),
    /// 반 화면
    HalfScreens(isize),
    Top,
    Bottom,
    /// 쪽 단위 이동
    Page(bool),
    /// 가로 스크롤
    Pan(isize),
    /// 검색 결과 이동
    Hit(bool),
    /// 명령줄 시작 (`/` 또는 `:`)
    BeginInput(char),
    /// **명령줄을 미리 채워** 띄운다 — 메뉴에서 경로를 받아야 하는 명령을
    /// 고를 때. `Action` 에 경로를 실어 보낼 수 없으니 사람에게 묻는다.
    Prompt(&'static str),
    InputChar(char),
    InputBackspace,
    InputSubmit,
    InputCancel,
    /// 개요 보기 켜기/끄기
    ToggleToc,
    /// 개요에서 고른 줄로 이동
    Select,
    /// 개요 보기에서 나가기
    LeaveToc,
    // ---- 편집 ----
    /// 글자 단위 이동
    Chars(isize),
    /// 단락 처음 / 끝
    LineStart,
    LineEnd,
    /// 끼워넣기 모드로 (`after` 면 커서 다음부터)
    Insert { after: bool, line_end: bool, line_start: bool },
    /// 끼워넣기 모드에서 나가기
    Normal,
    /// 글자 넣기
    Type(char),
    Backspace,
    /// 커서 글자 지우기
    DeleteChar,
    /// 빈 단락 열기 (`o` / `O`)
    OpenParagraph { before: bool },
    Undo,
    /// 연산자 대기 (`d`)
    Operator(char),
    /// 낱말 하나 지우기 (`Ctrl-T`, 워드스타 — vi 의 `dw` 자리)
    DeleteWord,
    /// 커서에서 단락 끝까지 지우기 (`Ctrl-Q Y`, 워드스타)
    DeleteToEnd,
    /// 책갈피를 넣는다 (`Ctrl-K <숫자>`)
    SetMark(usize),
    /// 책갈피로 간다 (`Ctrl-Q <숫자>`)
    GotoMark(usize),
    /// 글머리 단계를 올리거나 내린다 (`Tab` / `Shift-Tab`)
    Relevel(isize),
    /// 붙이기 (`p` 뒤 / `P` 앞)
    Paste { before: bool },
    /// 다시하기 (`Ctrl-r`)
    Redo,
    /// 도움 보기 켜기·끄기 (`?`)
    ToggleHelp,
    /// 저장 (`:w`)
    Save,
    /// 저장하고 나가기 (`ZZ` · `:wq` · `:x`)
    SaveQuit,
    /// 디스크에서 다시 읽기 (`:e!`)
    Reload,
    /// 버리고 나가기 (`ZQ` · `:q!`)
    ForceQuit,
    /// 낱말 단위 이동 (`w` `b` `e` · 도스 `Ctrl-F` `Ctrl-A`)
    MoveWord(crate::buffer::Word),
    /// 메뉴 — 화살표로 고른다
    OpenMenu,
    CloseMenu,
    /// 메뉴 안에서 위·아래
    MenuMove(isize),
    /// 메뉴 묶음 바꾸기 (왼쪽·오른쪽)
    MenuGroup(isize),
    /// 고른 것을 실행한다
    MenuPick,
    /// 지금 칸의 글자를 **고친다** — 명령줄에 그 글자를 채워 띄운다
    EditCell,
    /// **그림 크게 보기** (`v`) — 화면을 다 쓰고 색으로 그린다
    OpenViewer,
    /// 다음·앞 그림
    ViewerStep(isize),
    /// 기호표를 연다 (`Ctrl-K` · `Ctrl-F10` · 메뉴 · `:sym`)
    OpenSymbols,
    /// **한자표**를 연다 — 커서 앞의 한글 음절을 본다 (`F9` · 메뉴 · `:hanja`)
    OpenHanja,
    /// 문자표 안에서 `(가로, 세로)` 움직인다
    SymbolMove(isize, isize),
    /// 문자표 묶음을 바꾼다
    SymbolGroup(isize),
    /// 표 안으로 들어간다 / 나온다
    EnterTable,
    LeaveTable,
    /// 표 안에서 칸 이동 — `(행, 열)` 증감
    MoveCell(isize, isize),
    /// 다음·앞 칸 (`Tab` / `Shift-Tab`)
    NextCell(bool),
    /// 그림 보기 방식을 다음 것으로 (`z`)
    CycleImages,
    /// 문서 단 폭 / 터미널 폭 (`W`)
    ToggleWidth,
}

/// 키를 동작으로 옮긴다. 입력 중일 때와 아닐 때가 다르다.
#[cfg(test)]
fn action_for(key: KeyEvent, typing: bool) -> Action {
    action_for_mode(key, typing, Mode::Body)
}

/// 도스 시절 단축키. vi 와 겹치지 않는 것만 여기서 받는다.
///
/// 출처는 보르랜드 IDE 계보를 문서화한 Free Pascal IDE 키 목록이다 —
/// `F1` 도움 · `F2` 저장 · `F10` 메뉴 · `Alt-X` 끝내기 · `Ctrl-Y` 줄 지우기 ·
/// `Ctrl-L` 다시 찾기 · `Ctrl-E/X/S/D` 커서 · `Ctrl-A/F` 낱말 ·
/// `Ctrl-K …` 블록 · `Ctrl-Q …` 찾기·이동 · `Ctrl-Ins`/`Shift-Del`/`Shift-Ins`
/// 클립보드(IBM CUA).
///
/// **없는 것은 지어내지 않는다.** `F3`(파일 열기)은 이 프로그램에 파일을 고르는
/// 화면이 없어 비워 둔다. `Ctrl-K B/K`(블록 시작·끝)도 우리에게 선택 개념이
/// 없어 받지 않는다 — 단락이 단위다.
fn dos_key(key: KeyEvent, mode: Mode) -> Option<Action> {
    use crate::buffer::Word;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    // 글자를 치는 중이면 가로채지 않는다 — Ctrl 조합만 본다
    let typing_text = mode == Mode::Insert;

    Some(match key.code {
        KeyCode::F(1) => Action::ToggleHelp,
        KeyCode::F(2) => Action::Save,
        // Alt-X — 도스 프로그램의 끝내기
        KeyCode::Char('x') | KeyCode::Char('X') if alt => Action::Quit,
        // IBM CUA 클립보드. 우리 단위는 단락이다.
        KeyCode::Insert if ctrl => Action::Operator('y'),
        KeyCode::Delete if shift => Action::Operator('d'),
        KeyCode::Insert if shift => Action::Paste { before: false },
        // 워드스타: Ctrl-Y 줄 지우기 → 우리는 단락
        KeyCode::Char('y') if ctrl => Action::Operator('d'),
        KeyCode::Char('l') if ctrl => Action::Hit(true),
        // 워드스타: Ctrl-T 낱말 지우기. vi 의 `dw` 자리를 이 키가 채운다
        KeyCode::Char('t') if ctrl => Action::DeleteWord,
        // 커서 다이아몬드. Ctrl-D·Ctrl-F 는 vi 의 반쪽/한 화면과 겹쳐
        // `:set keys=dos` 를 켤 때만 커서가 된다.
        KeyCode::Char('e') if ctrl => Action::Lines(-1),
        KeyCode::Char('x') if ctrl && !typing_text => Action::Lines(1),
        KeyCode::Char('s') if ctrl => Action::Chars(-1),
        KeyCode::Char('a') if ctrl => Action::MoveWord(Word::Back),
        KeyCode::Char('d') if ctrl && crate::pager::dos_keys() => Action::Chars(1),
        KeyCode::Char('f') if ctrl && crate::pager::dos_keys() => {
            Action::MoveWord(Word::Forward)
        }
        // 두 키 묶음 — `Ctrl-K` 블록, `Ctrl-Q` 찾기·이동
        KeyCode::Char('k') if ctrl => Action::Operator('\u{1}'),
        KeyCode::Char('q') if ctrl => Action::Operator('\u{2}'),
        _ => return None,
    })
}

/// **그림 보기가 열려 있을 때의 키.** 넘기고 닫는 것뿐이다.
pub fn action_for_viewer(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('n' | 'j' | 'l') | KeyCode::Right | KeyCode::Down => Action::ViewerStep(1),
        KeyCode::Char('p' | 'k' | 'h') | KeyCode::Left | KeyCode::Up => Action::ViewerStep(-1),
        // `z` 는 본문과 같은 뜻 — 그리는 방식을 돌린다
        KeyCode::Char('z') => Action::CycleImages,
        KeyCode::Esc | KeyCode::Char('q' | 'v') => Action::CloseMenu,
        _ => Action::Nothing,
    }
}

/// **문자표가 열려 있을 때의 키.** 격자를 화살표로 걷는다.
///
/// 메뉴와 가른 이유: 메뉴는 목록이라 좌우가 *묶음*이지만, 문자표는 **격자**라
/// 좌우가 한 칸이고 묶음은 `Tab` 이다. 같은 키에 다른 뜻을 주는 쪽이 맞다 —
/// 눈에 보이는 모양이 다르다.
pub fn action_for_picker(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Left | KeyCode::Char('h' | 'a') => Action::SymbolMove(-1, 0),
        KeyCode::Right | KeyCode::Char('l' | 'd') => Action::SymbolMove(1, 0),
        KeyCode::Up | KeyCode::Char('k' | 'w') => Action::SymbolMove(0, -1),
        KeyCode::Down | KeyCode::Char('j' | 's') => Action::SymbolMove(0, 1),
        KeyCode::Tab => Action::SymbolGroup(1),
        KeyCode::BackTab => Action::SymbolGroup(-1),
        KeyCode::Char(']') => Action::SymbolGroup(1),
        KeyCode::Char('[') => Action::SymbolGroup(-1),
        KeyCode::Enter | KeyCode::Char(' ') => Action::MenuPick,
        KeyCode::Esc | KeyCode::Char('q') => Action::CloseMenu,
        _ => Action::Nothing,
    }
}

/// **메뉴가 열려 있을 때의 키.** 화살표로 고르고 `↵` 로 실행한다.
///
/// 메뉴는 모드가 아니라 **덮개**다 — 뒤의 모드를 그대로 두고 그 위에 뜬다.
/// 그래서 `Mode` 를 늘리지 않고 따로 가른다.
pub fn action_for_menu(key: KeyEvent) -> Action {
    match key.code {
        // 메뉴도 "고르는" 일이다 — 표 안과 같은 세 벌을 받는다
        KeyCode::Up | KeyCode::Char('k' | 'w') => Action::MenuMove(-1),
        KeyCode::Down | KeyCode::Char('j' | 's') => Action::MenuMove(1),
        KeyCode::Left | KeyCode::Char('h' | 'a') | KeyCode::BackTab => Action::MenuGroup(-1),
        KeyCode::Right | KeyCode::Char('l' | 'd') | KeyCode::Tab => Action::MenuGroup(1),
        KeyCode::Enter | KeyCode::Char(' ') => Action::MenuPick,
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::F(10) | KeyCode::Char('m') => {
            Action::CloseMenu
        }
        _ => Action::Nothing,
    }
}

/// 키를 동작으로 옮긴다. 개요 보기에서는 `j/k` 가 커서를 움직이고 `Enter` 가 고른다.
///
/// 판단이 전부 여기 있고 터미널을 모르므로 시험할 수 있다.
pub fn action_for_mode(key: KeyEvent, typing: bool, mode: Mode) -> Action {
    // 표 안에서는 hjkl 이 **칸**을 옮긴다. 글자 이동이 아니다 — 표에서 사람이
    // 하고 싶은 일은 칸을 고르는 것이고, 칸 안의 글자는 들어가서 고친다.
    if mode == Mode::Table && !typing {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        return match key.code {
            KeyCode::Char('c') if ctrl => Action::Quit,
            // **세 벌을 다 받는다 — 화살표 · wasd · hjkl.**
            //
            // 표 안은 "칸을 고르는" 일이고 그건 vi 보다 게임에 가깝다. 손이
            // 기억하는 쪽으로 눌러도 되게 둔다. 표 모드는 분기가 먼저 돌아서
            // 본문의 `a`(넣기)·`d`(지우기)·`w`(낱말)와 부딪히지 않는다 —
            // 네 자리가 비어 있었다.
            KeyCode::Char('h' | 'a') | KeyCode::Left => Action::MoveCell(0, -1),
            KeyCode::Char('l' | 'd') | KeyCode::Right => Action::MoveCell(0, 1),
            KeyCode::Char('k' | 'w') | KeyCode::Up => Action::MoveCell(-1, 0),
            KeyCode::Char('j' | 's') | KeyCode::Down => Action::MoveCell(1, 0),
            // **칸 고치기.** 칸을 넘나들 수는 있는데 고칠 길이 없었다
            // (2026-10-10 보고). `i` 는 본문과 같은 뜻이고, `Enter` 는 "이 칸을
            // 열어라" 로 자연스럽다.
            KeyCode::Char('i') | KeyCode::Enter => Action::EditCell,
            // **되돌리기도 표 안에서 되어야 한다.** 칸을 고칠 수 있게 됐으니
            // 되돌릴 수도 있어야 한다 — 나가서 `u` 를 누르게 하면 그게 뭘
            // 되돌리는지 알 수 없다.
            KeyCode::Char('u') => Action::Undo,
            KeyCode::Char('r') if ctrl => Action::Redo,
            KeyCode::Tab => Action::NextCell(true),
            KeyCode::BackTab => Action::NextCell(false),
            KeyCode::Esc | KeyCode::Char('q') => Action::LeaveTable,
            // 표 안에서 `T` 는 다음 표 — 표를 넘어 다니는 길
            KeyCode::Char('T') => Action::EnterTable,
            KeyCode::Char('t') => Action::ToggleToc,
            KeyCode::Char(':') => Action::BeginInput(':'),
            KeyCode::Char('/') => Action::BeginInput('/'),
            KeyCode::Char('z') => Action::CycleImages,
            // 칸 안의 그림을 크게 — 칸에 그려진 것은 특히 작다
            KeyCode::Char('v') => Action::OpenViewer,
            KeyCode::Char('W') => Action::ToggleWidth,
            _ => Action::Nothing,
        };
    }

    // 끼워넣기 모드가 먼저다 — 여기서는 모든 글자키가 글자다.
    if mode == Mode::Insert && !typing {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        return match key.code {
            // **기호 넣기.** 터미널에는 한글 IME 가 있어도 `○` `※` `①` 을 넣을
            // 길이 없고, 공문서는 그 기호들로 계층을 만든다.
            //
            // `Ctrl-K` 는 vi 가 **끼워넣기 모드**에서 digraph(특수문자)를 받는
            // 자리다. 보통 모드의 `Ctrl-K`(워드스타 블록 묶음)와 겹치지 않는다 —
            // 끼워넣기 모드는 이 분기에서 끝나고 도스 키 표를 보지 않는다.
            // 두 계보가 같은 결론에 와 있다.
            KeyCode::Char('k') if ctrl => Action::OpenSymbols,
            // HWP 의 문자표 키. 터미널이 보내 주면 받는다.
            KeyCode::F(10) if ctrl => Action::OpenSymbols,
            // **한자로 바꾸기.** HWP 의 `F9` 와 같다 — 커서 앞 음절을 본다.
            KeyCode::F(9) => Action::OpenHanja,
            KeyCode::Esc => Action::Normal,
            KeyCode::Backspace => Action::Backspace,
            KeyCode::Enter => Action::OpenParagraph { before: false },
            KeyCode::Left => Action::Chars(-1),
            KeyCode::Right => Action::Chars(1),
            KeyCode::Up => Action::Lines(-1),
            KeyCode::Down => Action::Lines(1),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Normal,
            KeyCode::Char(c) => Action::Type(c),
            _ => Action::Nothing,
        };
    }
    if typing {
        return match key.code {
            KeyCode::Esc => Action::InputCancel,
            KeyCode::Enter => Action::InputSubmit,
            KeyCode::Backspace => Action::InputBackspace,
            KeyCode::Char(c) => Action::InputChar(c),
            _ => Action::Nothing,
        };
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    // `?` 는 **어느 모드에서나** 도움 보기를 켠다. 명령이 늘어난 뒤로 "뭘
    // 눌러야 하지" 가 가장 흔한 막힘이고, 그 답은 모드마다 다르다.
    if key.code == KeyCode::Char('?') {
        return Action::ToggleHelp;
    }
    // **메뉴** — `F10` 은 도스 시절 편집기의 관례이고(HWP 도 거기서 왔다),
    // 터미널이 F10 을 가로채는 경우가 있어 `m` 도 받는다. 메뉴가 열려 있을
    // 때의 키는 `apply` 쪽에서 메뉴 상태를 보고 가른다.
    if matches!(key.code, KeyCode::F(10)) {
        return Action::OpenMenu;
    }
    // ---- 도스 시절 단축키 ----
    //
    // HWP 는 도스에서 왔고, 그 시절 편집기의 키는 보르랜드 IDE(터보 파스칼)와
    // 워드스타 계열로 널리 굳어져 있었다. 근거는 그 계보를 그대로 문서화한
    // Free Pascal IDE 의 키 목록이다.
    //
    // **vi 와 겹치지 않는 것은 모드 없이 그냥 받는다.** 두 벌을 외우게 하는
    // 대신, 손이 기억하는 쪽으로 눌러도 되게 한다. 겹치는 둘(`Ctrl-D`
    // `Ctrl-F`)만 `:set keys=dos` 로 가른다.
    if let Some(a) = dos_key(key, mode) {
        return a;
    }
    if mode == Mode::Toc {
        return match key.code {
            KeyCode::Enter => Action::Select,
            KeyCode::Esc => Action::LeaveToc,
            KeyCode::Char('q') => Action::LeaveToc,
            KeyCode::Char('j') | KeyCode::Down => Action::Lines(1),
            KeyCode::Char('k') | KeyCode::Up => Action::Lines(-1),
            KeyCode::Char('d') if ctrl => Action::HalfScreens(1),
            KeyCode::Char('u') if ctrl => Action::HalfScreens(-1),
            KeyCode::Char('g') | KeyCode::Home => Action::Top,
            KeyCode::Char('G') | KeyCode::End => Action::Bottom,
            KeyCode::Char('t') => Action::ToggleToc,
            KeyCode::Char('c') if ctrl => Action::Quit,
            KeyCode::Char(':') => Action::BeginInput(':'),
            _ => Action::Nothing,
        };
    }
    match key.code {
        KeyCode::Char('q') => Action::Quit,
        // **Esc 는 끝내지 않는다.** vi 에서 Esc 는 "보통 모드로" 이고, 끼워넣기
        // 중이면 위에서 이미 그 일을 한다. 보통 모드의 Esc 가 종료였던 탓에
        // 습관적으로 누르면 편집이 날아갔다.
        KeyCode::Esc => Action::Nothing,
        // vi 의 `ZZ` 저장하고 나가기 · `ZQ` 버리고 나가기
        KeyCode::Char('Z') => Action::Operator('Z'),
        KeyCode::Char('Q') => Action::Operator('Q'),
        KeyCode::Char('c') if ctrl => Action::Quit,
        KeyCode::Char('j') | KeyCode::Down | KeyCode::Enter => Action::Lines(1),
        KeyCode::Char('k') | KeyCode::Up => Action::Lines(-1),
        KeyCode::Char('d') if ctrl => Action::HalfScreens(1),
        KeyCode::Char('u') if ctrl => Action::HalfScreens(-1),
        KeyCode::Char('f') if ctrl => Action::Screens(1),
        KeyCode::Char('b') if ctrl => Action::Screens(-1),
        KeyCode::Char('r') if ctrl => Action::Redo,
        KeyCode::Char(' ') | KeyCode::PageDown => Action::Screens(1),
        // 한 화면 뒤로는 `Ctrl-B`·`PageUp` 이다. 맨 `b` 는 vi 에서 **낱말
        // 뒤로**이고, 그쪽이 더 자주 쓴다 — 전에 `less` 처럼 묶어 뒀다.
        KeyCode::PageUp => Action::Screens(-1),
        KeyCode::Char('g') | KeyCode::Home => Action::Top,
        KeyCode::Char('G') | KeyCode::End => Action::Bottom,
        KeyCode::Char(']') => Action::Page(true),
        KeyCode::Char('[') => Action::Page(false),
        KeyCode::Char('h') | KeyCode::Left => Action::Chars(-1),
        KeyCode::Char('l') | KeyCode::Right => Action::Chars(1),
        KeyCode::Char('<') => Action::Pan(-4),
        KeyCode::Char('>') => Action::Pan(4),
        KeyCode::Char('n') => Action::Hit(true),
        KeyCode::Char('N') => Action::Hit(false),
        KeyCode::Char('t') => Action::ToggleToc,
        KeyCode::Char('m') => Action::OpenMenu,
        // 낱말 이동 — 이 자리를 비워 뒀던 이유가 이것이다
        KeyCode::Char('w') => Action::MoveWord(crate::buffer::Word::Forward),
        KeyCode::Char('b') => Action::MoveWord(crate::buffer::Word::Back),
        KeyCode::Char('e') => Action::MoveWord(crate::buffer::Word::End),
        // **Enter — 커서가 있는 단락에 표가 있으면 그 표로 들어간다.**
        //
        // 표를 선으로만 보여주면 어느 칸인지 사람이 눈으로 따라가야 하고,
        // 선 문자는 터미널마다 폭이 달라 그 눈마저 못 믿는다. 들어가면
        // 커서가 칸을 쥐고 상태줄이 `표N 3행2열` 로 말한다 — 그게 그대로
        // `hwp set-cell` 의 좌표다.
        //
        // `Enter` 는 이미 한 줄 내려가기(`less` 와 같다)라 `T` 를 쓴다.
        KeyCode::Char('T') => Action::EnterTable,
        // 보기 방식은 **옵션이 아니라 키다.** 명령줄에서 외울 것을 줄인다.
        // `w`·`b`·`e` 는 나중에 낱말 이동으로 쓸 수 있게 비워 둔다.
        KeyCode::Char('z') => Action::CycleImages,
        // **그림 크게 보기.** 본문에 그린 그림은 문서가 놓은 크기를 따라
        // 로고가 34×8점으로 줄어 1비트로는 아무것도 안 보인다 — 화면을 다
        // 쓰는 창이 따로 있어야 한다.
        //
        // `v` 는 vi 에서 블록 지정이다. 아직 블록이 없어 비어 있고, 넣을 때는
        // `V`(또는 HWP 의 `F3`)로 간다.
        KeyCode::Char('v') => Action::OpenViewer,
        KeyCode::Char('W') => Action::ToggleWidth,
        // 편집 — `hwp view` 에서는 apply 가 무시한다
        KeyCode::Char('i') => Action::Insert { after: false, line_end: false, line_start: false },
        KeyCode::Char('a') => Action::Insert { after: true, line_end: false, line_start: false },
        KeyCode::Char('A') => Action::Insert { after: false, line_end: true, line_start: false },
        KeyCode::Char('I') => Action::Insert { after: false, line_end: false, line_start: true },
        KeyCode::Char('x') => Action::DeleteChar,
        KeyCode::Char('d') => Action::Operator('d'),
        KeyCode::Char('o') => Action::OpenParagraph { before: false },
        KeyCode::Char('O') => Action::OpenParagraph { before: true },
        KeyCode::Char('u') => Action::Undo,
        KeyCode::Char('y') => Action::Operator('y'),
        KeyCode::Char('p') => Action::Paste { before: false },
        KeyCode::Char('P') => Action::Paste { before: true },
        // 들여쓰기 — 문서가 쓰는 단계 사다리를 따라간다.
        // `<`/`>` 는 넓은 표를 볼 때 가로 스크롤로 이미 쓰고 있어 건드리지 않는다.
        KeyCode::Tab => Action::Relevel(1),
        KeyCode::BackTab => Action::Relevel(-1),
        KeyCode::Char('0') => Action::LineStart,
        KeyCode::Char('$') => Action::LineEnd,
        KeyCode::Char('/') => Action::BeginInput('/'),
        KeyCode::Char(':') => Action::BeginInput(':'),
        _ => Action::Nothing,
    }
}

/// 키 하나를 처리하는 **한 걸음.** 대화형 루프와 `--keys` 가 **같은 함수**를 쓴다.
///
/// 왜 합쳤나: 둘이 따로 있던 동안 `--keys`(= `--frame` 으로 화면을 글로 보는 길)가
/// **화면을 다시 그리지 않았다.** 그래서 "글자를 치면 화면에 안 나타난다" 는 버그를
/// 그 길로는 볼 수 없었고, 그 길로 돌아가는 시험도 전부 통과했다. 화면 없이 화면을
/// 확인하는 도구가 실제와 다르면 **그 도구가 거짓말을 한다.**
pub fn step(app: &mut App, key: KeyEvent, viewport: usize) {
    let typing = app.input.is_some();
    // **연산 개수가 아니라 판 번호를 본다.**
    //
    // 개수로 보던 동안 이어 치는 글자가 화면에 안 나타났다 — 같은 단락의 연속
    // 입력은 **한 연산으로 모이므로** 개수가 안 바뀌고, 조합 중인 자모는 커밋
    // 전까지 연산이 아예 없다. 첫 글자만 보이고 그 뒤는 Enter 를 눌러야
    // 나왔다(2026-10-10 보고, 한글·영문 둘 다).
    let before = app.buf.revision();
    let before_cells = app.buf.cell_revision();
    // 메뉴가 열려 있으면 그 키 묶음이 먼저다 — 메뉴는 모드가 아니라 덮개이므로
    // `Mode` 를 늘리지 않고 여기서 가른다
    let action = if app.viewer.is_some() && !typing {
        action_for_viewer(key)
    } else if app.picker.is_some() && !typing {
        action_for_picker(key)
    } else if app.menu.is_some() && !typing {
        action_for_menu(key)
    } else if app.pending_op.is_some() && !typing {
        // **두 키 묶음의 두 번째 키는 글자 그대로 받는다.**
        //
        // 짝 맞추기는 `apply` 안에 있고 두 번째 키도 `Operator` 로 와야
        // 한다. vi 의 `dd`·`ZZ` 는 그 글자 자체가 `Operator` 라 맞았지만,
        // 도스 묶음(`Ctrl-K S`)의 `s` 는 보통 모드에서 아무 데도 안 걸려
        // `Nothing` 이 되었다 — **그래서 도스 두 키 묶음이 실제 키로는 한
        // 번도 동작하지 않았다**(2026-10-11 실측: `Ctrl-K S` 가 저장하지
        // 않고 `Ctrl-Q S` 가 커서를 옮기지 않았다. `Ctrl-K z` 는 그림 보기
        // 방식을 돌렸다).
        //
        // 시험은 `apply(Operator('\u{1}'))` + `apply(Operator('s'))` 를 직접
        // 불러서 통과했다 — **버그가 사는 층을 건너뛴 시험**이다.
        match key.code {
            KeyCode::Char(c) => Action::Operator(c),
            KeyCode::Esc => Action::InputCancel,
            _ => Action::Nothing,
        }
    } else {
        action_for_mode(key, typing, app.mode)
    };
    apply(app, action, viewport);
    // 편집이 생겼으면 화면 행을 다시 만든다 — 조판은 그대로 두고 접힘만 다시
    // 계산한다(상태줄이 "조판 미반영" 이라고 말한다).
    // 칸이 바뀌었으면 격자를 다시 그린다. **되돌리기도 여기로 온다** —
    // 되돌린 칸을 화면 쪽으로 흘려보내지 않아서 `u` 가 "되돌렸다" 고만 하고
    // 화면은 그대로였다.
    // 칸이 바뀌었으면 격자를 다시 그린다. **되돌리기도 여기로 온다.**
    //
    // 칸을 고치는 **중**이면 판 번호만 올라도 다시 그린다 — 조합 중인 자모는
    // 칸 글자를 바꾸지 않으므로(확정 전이다) 칸 판 번호가 안 올라가는데,
    // 화면에는 보여야 한다. 안 보이면 한글을 칠 때 손이 허공을 짚는다.
    let cell_changed = app.buf.cell_revision() != before_cells;
    let composing = app.buf.editing_cell().is_some() && app.buf.revision() != before;
    if app.editable && (cell_changed || composing) {
        app.sync_cells();
    }
    if app.editable && app.buf.revision() != before {
        let width = app.doc.column.cells;
        app.refresh(width, viewport);
    } else if app.editable {
        app.reveal_cursor(viewport);
    }
}

/// 동작을 상태에 적용한다. 터미널을 모른다 — 그래서 시험할 수 있다.
pub fn apply(app: &mut App, action: Action, viewport_rows: usize) {
    // **칸을 고치는 중에는 단락을 건드리는 것을 막는다.**
    //
    // 끼워넣기 모드를 그대로 쓰므로(두벌식 조합을 베끼지 않으려면 그래야 한다)
    // `Enter`(빈 단락 열기)와 `↑↓`(단락 이동)가 같이 들어온다. 칸을 고치다
    // Enter 를 누르면 **본문에 빈 단락이 생긴다** — 사람은 "칸 입력을 끝냈다"
    // 고 생각한 순간이다. 그래서 Enter 는 끝내는 것으로 받고, 세로 이동은
    // 거절하며 왜인지 말한다.
    if app.buf.editing_cell().is_some() {
        match action {
            Action::OpenParagraph { .. } => {
                apply(app, Action::Normal, viewport_rows);
                return;
            }
            Action::Lines(_)
            | Action::Screens(_)
            | Action::HalfScreens(_)
            | Action::Top
            | Action::Bottom
            | Action::Page(_)
            | Action::MoveCell(_, _)
            | Action::NextCell(_) => {
                app.message = "칸을 고치는 중입니다 — Esc 로 끝낸 뒤 옮깁니다".into();
                return;
            }
            _ => {}
        }
    }
    let in_toc = app.mode == Mode::Toc;
    // **도움 보기가 켜져 있으면 나가기 키가 그걸 먼저 닫는다.** 도움을 보다가
    // `q` 를 눌러 프로그램이 끝나면 놀란다.
    if app.help {
        match action {
            Action::ToggleHelp | Action::Quit | Action::LeaveTable | Action::InputCancel => {
                app.help = false;
                app.help_scroll = 0;
                return;
            }
            // **도움 보기 안에서는 화살표가 그 내용을 넘긴다.** 키가 늘어 한
            // 화면에 안 들어가는데, 잘린 채로 두면 "없는 기능" 으로 읽힌다.
            Action::Lines(n) | Action::MenuMove(n) => {
                app.help_scroll = (app.help_scroll as isize + n).max(0) as usize;
                return;
            }
            Action::HalfScreens(n) | Action::Screens(n) => {
                app.help_scroll = (app.help_scroll as isize + n * 8).max(0) as usize;
                return;
            }
            Action::Top => {
                app.help_scroll = 0;
                return;
            }
            // 도움을 보면서도 움직일 수는 있게 둔다 (뒤의 본문이 따라 움직인다)
            _ => {}
        }
    }
    match action {
        Action::ToggleHelp => {
            app.help = true;
            app.help_scroll = 0;
        }
        Action::Nothing => {}
        // **저장 안 한 편집을 조용히 버리지 않는다.** vi 가 `:q` 를 거부하고
        // `:q!` 를 요구하는 이유와 같다. 전에는 `q`·`Esc`·`:q` 가 전부 그냥
        // 끝냈고, 그러면 친 글이 말없이 사라진다.
        Action::Quit => quit(app, false),
        Action::ToggleToc => app.toggle_toc(),
        Action::Select => app.jump_to_selected(),
        Action::LeaveToc => app.mode = Mode::Body,
        Action::Lines(n) if in_toc => app.move_toc(n),
        Action::HalfScreens(n) if in_toc => {
            app.move_toc(n * (viewport_rows as isize / 2).max(1))
        }
        Action::Top if in_toc => app.toc_cursor = 0,
        Action::Bottom if in_toc => app.toc_cursor = app.toc.len().saturating_sub(1),
        Action::Lines(n) if app.editable => {
            app.buf.move_paragraph(n);
            app.pending_op = None;
        }
        Action::Lines(n) => app.scroll(n, viewport_rows),
        // **고치기 모드에서는 커서를 옮긴다.**
        //
        // 보기만 옮기면 한 걸음 뒤에 `reveal_cursor` 가 커서 쪽으로 되돌려
        // 아무 일도 일어나지 않는다 — `G`·`Ctrl-F`·`]` 가 그렇게 죽어 있었다
        // (읽기 전용에서는 멀쩡했다. 2026-10-11 실측).
        Action::Screens(n) if app.editable => {
            app.move_cursor_rows(n * viewport_rows as isize);
            app.pending_op = None;
        }
        Action::HalfScreens(n) if app.editable => {
            app.move_cursor_rows(n * (viewport_rows as isize / 2).max(1));
            app.pending_op = None;
        }
        Action::Top if app.editable => {
            app.cursor_to_edge(false);
            app.pending_op = None;
        }
        Action::Bottom if app.editable => {
            app.cursor_to_edge(true);
            app.pending_op = None;
        }
        Action::Page(forward) if app.editable => {
            app.go_page(forward);
            // 그 쪽의 첫 본문 줄로 커서를 옮긴다 — 보기만 옮기면 되돌려진다
            let top = app.top;
            app.cursor_to_row(top, true);
            app.pending_op = None;
        }
        Action::Screens(n) => app.scroll(n * viewport_rows as isize, viewport_rows),
        Action::HalfScreens(n) => app.scroll(n * (viewport_rows as isize / 2).max(1), viewport_rows),
        Action::Top => app.top = 0,
        Action::Bottom => app.top = app.max_top(viewport_rows),
        Action::Page(forward) => app.go_page(forward),
        Action::Pan(n) => {
            app.left = (app.left as isize + n).max(0) as usize;
        }
        Action::Hit(forward) => app.next_hit(forward),

        // 보기 방식 — **읽기전용에서도 된다.** 읽기만 할 때 더 필요한 값이고,
        // 아래 catch-all 뒤에 두면 `hwp view` 에서 `z` 가 "읽기전용이다" 라고만
        // 답한다(실제로 그랬다).
        Action::Paste { before } => {
            match app.buf.paste_paragraph(before) {
                Some(n) => {
                    app.message = format!("{n}자를 붙였습니다");
                    app.refresh(app.doc.column.cells.max(20), viewport_rows);
                }
                None => app.message = "담아 둔 것이 없습니다 — yy 또는 dd 가 담습니다".into(),
            }
        }
        Action::Redo => {
            app.message = app.buf.redo().unwrap_or_else(|| "다시 할 것이 없습니다".into());
            app.refresh(app.doc.column.cells.max(20), viewport_rows);
        }
        Action::MoveWord(how) => {
            app.buf.move_word(how);
            app.pending_op = None;
        }
        Action::Save => { write_file(app); }
        Action::SaveQuit => save_and_quit(app),
        Action::Reload => reload(app),
        Action::ForceQuit => quit(app, true),
        Action::OpenMenu => app.open_menu(),
        Action::CloseMenu => {
            // 그림 보기 · 골라 넣기 상자가 열려 있으면 그쪽을 닫는다
            if app.viewer.take().is_some() {
                app.message = "그림 보기를 닫았습니다".into();
            } else if app.picker.take().is_some() {
                app.message = "골라 넣기를 닫았습니다".into();
            } else {
                app.menu = None;
            }
        }
        Action::MenuMove(d) => app.menu_move(d),
        Action::MenuGroup(d) => app.menu_group(d),
        // 골라 넣기 상자가 열려 있으면 **그쪽을** 고른다 — 메뉴와 같은 키다
        Action::MenuPick if app.picker.is_some() => {
            match app.picker_pick() {
                Some((glyph, replace)) => {
                    app.picker = None;
                    // 한자 변환이면 앞의 음절을 **지우고** 넣는다. 더하면
                    // `한韓` 이 되고, 그건 아무도 원한 적이 없다.
                    if replace > 0 {
                        let off = app.buf.cursor.offset;
                        app.buf.cursor.offset = off.saturating_sub(replace);
                        app.buf.delete_chars(replace);
                    }
                    app.buf.insert_str(&glyph);
                    app.message = format!("{glyph} 를 넣었습니다");
                }
                None => app.picker = None,
            }
        }
        Action::MenuPick => {
            if let Some(act) = app.menu_pick() {
                app.menu = None;
                apply(app, act, viewport_rows);
            }
        }
        Action::EnterTable => {
            if app.mode == Mode::Table {
                app.next_table();
            } else if !app.enter_table() {
                app.message = "이 문서에 표가 없습니다".into();
            }
        }
        Action::LeaveTable => app.leave_table(),
        Action::MoveCell(dr, dc) => app.move_cell(dr, dc),
        Action::NextCell(forward) => app.next_cell(forward),
        Action::CycleImages => app.cycle_images(viewport_rows),
        Action::ToggleWidth => {
            // 터미널 폭은 그리는 쪽이 아는 값이다. 여기서는 지금 쓰는 폭에
            // 여백을 더해 되돌린다 — 그리기 직후 같은 값으로 다시 계산된다.
            let area = (app.doc.column.cells + crate::pager::GUTTER) as u16;
            app.toggle_fit_term(area, viewport_rows);
        }

        // ---- 편집 ----
        // `hwp view` 로 열었으면 편집 동작은 아무 일도 하지 않고 이유를 말한다.
        _ if !app.editable => {
            app.message = "읽기전용으로 열렸습니다 — 고치려면 `hwp edit <파일>`".into();
        }
        Action::Chars(n) => {
            app.buf.move_horizontal(n);
            app.pending_op = None;
        }
        Action::LineStart => app.buf.move_line_start(),
        Action::LineEnd => app.buf.move_line_end(),
        Action::Insert { after, line_end, line_start } => {
            if line_end {
                app.buf.move_line_end();
            } else if line_start {
                app.buf.move_line_start();
            } else if after {
                app.buf.move_horizontal(1);
            }
            app.mode = Mode::Insert;
            app.pending_op = None;
            app.message.clear();
        }
        Action::Normal => {
            app.buf.commit_jamo();
            // **칸을 고치던 중이면 표로 돌아간다.** 본문으로 튀어나오면 방금
            // 고친 칸이 어디였는지 사람이 다시 찾아야 한다.
            if app.buf.editing_cell().is_some() {
                app.buf.end_cell_edit();
                app.mode = Mode::Table;
                app.message = "칸 고치기를 끝냈습니다 — 저장은 `:w`".into();
            } else {
                app.mode = Mode::Body;
            }
        }
        Action::Type(c) => app.buf.type_key(c),
        Action::Backspace => app.buf.backspace(),
        Action::DeleteChar => app.buf.delete_chars(1),
        Action::OpenParagraph { before } => {
            app.buf.open_paragraph(before);
            app.mode = Mode::Insert;
        }
        Action::Undo => {
            app.message = app.buf.undo().unwrap_or_else(|| "되돌릴 것이 없습니다".into());
        }
        Action::Operator(c) => {
            // 같은 키를 두 번(`dd` `yy` `ZZ`) 또는 짝(`ZQ`). vi 의 모양대로.
            match (app.pending_op.take(), c) {
                // ---- 도스: Ctrl-K 블록 묶음 ----
                (Some('\u{1}'), 's') | (Some('\u{1}'), 'S') => { write_file(app); }
                (Some('\u{1}'), 'q') | (Some('\u{1}'), 'Q') => quit(app, false),
                (Some('\u{1}'), 'y') | (Some('\u{1}'), 'Y') => {
                    app.buf.delete_paragraph();
                    app.message = "단락을 지웠습니다 (Ctrl-K Y) — u 로 되돌립니다".into();
                }
                (Some('\u{1}'), 'c') | (Some('\u{1}'), 'C') => {
                    match app.buf.paste_paragraph(false) {
                        Some(n) => app.message = format!("{n}자를 붙였다 (Ctrl-K C)"),
                        None => app.message = "담아 둔 것이 없습니다".into(),
                    }
                }
                (Some('\u{1}'), 'l') | (Some('\u{1}'), 'L') => {
                    let n = app.buf.yank_paragraph();
                    app.message = format!("{n}자를 담았다 (Ctrl-K L)");
                }
                // **책갈피** — HWP 가 지금도 쓰는 워드스타의 마크.
                // `Ctrl-K <숫자>` 로 자리를 적고 `Ctrl-Q <숫자>` 로 돌아간다.
                (Some('\u{1}'), d) if d.is_ascii_digit() => {
                    let slot = d.to_digit(10).unwrap_or(0) as usize;
                    apply(app, Action::SetMark(slot), viewport_rows);
                }
                (Some('\u{2}'), d) if d.is_ascii_digit() => {
                    let slot = d.to_digit(10).unwrap_or(0) as usize;
                    apply(app, Action::GotoMark(slot), viewport_rows);
                }
                (Some('\u{2}'), 'y') | (Some('\u{2}'), 'Y') => {
                    apply(app, Action::DeleteToEnd, viewport_rows);
                }
                (Some('\u{1}'), 'i') | (Some('\u{1}'), 'I') => {
                    apply(app, Action::Relevel(1), viewport_rows);
                }
                (Some('\u{1}'), 'u') | (Some('\u{1}'), 'U') => {
                    apply(app, Action::Relevel(-1), viewport_rows);
                }
                // ---- 도스: Ctrl-Q 찾기·이동 묶음 ----
                (Some('\u{2}'), 'f') | (Some('\u{2}'), 'F') => {
                    apply(app, Action::BeginInput('/'), viewport_rows);
                }
                (Some('\u{2}'), 's') | (Some('\u{2}'), 'S') => app.buf.move_line_start(),
                (Some('\u{2}'), 'd') | (Some('\u{2}'), 'D') => app.buf.move_line_end(),
                (Some('d'), 'd') => {
                    app.buf.delete_paragraph();
                    app.message = "단락을 지웠습니다 — u 로 되돌리고 p 로 붙입니다".into();
                }
                (Some('y'), 'y') => {
                    let n = app.buf.yank_paragraph();
                    app.message = format!("{n}자를 담았습니다 — p 로 붙입니다");
                }
                // `ZZ` 저장하고 나가기 · `ZQ` 버리고 나가기
                (Some('Z'), 'Z') => save_and_quit(app),
                (Some('Z'), 'Q') => quit(app, true),
                // **도스 묶음에 없는 키면 다시 기다리지 않고 말한다.**
                // 전에는 그 키로 다시 기다렸는데, `Ctrl-K` 뒤에 아무 글자나
                // 누르면 그 글자를 새 묶음으로 삼아 영원히 기다리는 셈이었다.
                (Some(pfx @ ('\u{1}' | '\u{2}')), c) => {
                    let name = if pfx == '\u{1}' { "Ctrl-K" } else { "Ctrl-Q" };
                    app.message = format!("{name} {c:?} 는 없습니다 — Esc 로 버립니다");
                }
                // 짝이 아니면 그 키로 다시 기다린다
                (_, c) => {
                    app.pending_op = Some(c);
                    app.message = match c {
                        'y' => "y — 한 번 더 누르면 단락 담기".into(),
                        'd' => "d — 한 번 더 누르면 단락 삭제".into(),
                        'Z' => "Z — ZZ 저장하고 나가기 · ZQ 버리고 나가기".into(),
                        // 도스 두 키 묶음 — 무엇을 기다리는지 말한다
                        '\u{1}' => "Ctrl-K — S 저장 · Q 나가기 · Y 단락삭제 · \
                                    L 담기 · C 붙이기 · I/U 글머리 단계"
                            .into(),
                        '\u{2}' => "Ctrl-Q — F 찾기 · S 단락 처음 · D 단락 끝".into(),
                        other => format!("{other} — 짝이 되는 키를 누르거나 Esc"),
                    };
                }
            }
        }
        // 읽기전용이면 위의 catch-all 이 이미 막는다
        Action::Relevel(delta) => {
            let at = app.buf.cursor.at;
            let levels = app.levels();
            if levels.is_empty() {
                app.message = "이 문서에는 글머리 단계가 없습니다 — 사다리를 읽을 수 없습니다".into();
            } else {
                match hwp_core::outline::relevel(app.buf.text_at(at), &levels, delta) {
                    Some(r) => {
                        app.buf.replace_paragraph(at, &r.text);
                        let what = match r.level {
                            Some(l) => format!("{l}단계"),
                            None => "들여쓰기".into(),
                        };
                        app.message = match r.note {
                            Some(n) => format!("{what} — {n}"),
                            None => format!("{what}로 옮겼습니다"),
                        };
                        app.refresh(app.doc.column.cells.max(20), viewport_rows);
                    }
                    None => {
                        app.message = if delta > 0 {
                            "더 들어갈 단계가 없습니다".into()
                        } else {
                            "더 나올 단계가 없습니다".into()
                        }
                    }
                }
            }
        }
        Action::Prompt(seed) => {
            app.input = Some((':', seed.to_string()));
        }
        Action::EditCell => edit_cell(app),
        Action::OpenViewer => match app.open_viewer() {
            Ok(()) => app.message = "그림 크게 보기 — n/p 로 넘기고 Esc 로 닫습니다".into(),
            Err(e) => app.message = e,
        },
        Action::ViewerStep(d) => app.viewer_step(d),
        Action::OpenSymbols => {
            if !app.editable {
                app.message = "읽기전용으로 열렸습니다 — 고치려면 `hwp edit <파일>`".into();
            } else {
                app.open_symbols();
                app.message = "기호를 고릅니다 — ↵ 로 넣습니다".into();
            }
        }
        Action::DeleteWord => {
            if !app.editable {
                app.message = "읽기전용으로 열렸습니다 — 고치려면 `hwp edit <파일>`".into();
            } else {
                match app.buf.delete_word() {
                    0 => app.message = "지울 낱말이 없습니다".into(),
                    n => app.message = format!("{n}자를 지웠습니다 (Ctrl-T) — u 로 되돌립니다"),
                }
            }
        }
        Action::DeleteToEnd => {
            if !app.editable {
                app.message = "읽기전용으로 열렸습니다 — 고치려면 `hwp edit <파일>`".into();
            } else {
                match app.buf.delete_to_end() {
                    0 => app.message = "커서 뒤에 글자가 없습니다".into(),
                    n => app.message = format!("{n}자를 지웠습니다 (Ctrl-Q Y) — u 로 되돌립니다"),
                }
            }
        }
        Action::SetMark(slot) => match app.buf.set_mark(slot) {
            Some(at) => {
                app.message = format!("책갈피 {slot} ← 구역{} 단락{} (Ctrl-Q {slot} 로 돌아옵니다)", at.0, at.1);
            }
            None => app.message = "표 칸을 고치는 중에는 책갈피를 넣지 않습니다".into(),
        },
        Action::GotoMark(slot) => match app.buf.goto_mark(slot) {
            Some(at) => {
                app.message = format!("책갈피 {slot} — 구역{} 단락{}", at.0, at.1);
                app.reveal_cursor(viewport_rows);
            }
            None => {
                app.message = format!("책갈피 {slot} 가 비었습니다 (Ctrl-K {slot} 로 넣습니다)")
            }
        },
        Action::OpenHanja => {
            if !app.editable {
                app.message = "읽기전용으로 열렸습니다 — 고치려면 `hwp edit <파일>`".into();
            } else {
                match app.open_hanja() {
                    Ok(()) => app.message = "한자를 고릅니다 — ↵ 로 바꿉니다".into(),
                    Err(e) => app.message = e,
                }
            }
        }
        Action::SymbolMove(dx, dy) => app.picker_move(dx, dy),
        Action::SymbolGroup(d) => app.picker_group(d),
        Action::BeginInput(prefix) => {
            app.input = Some((prefix, String::new()));
            app.message.clear();
        }
        Action::InputChar(c) => {
            if let Some((_, buf)) = app.input.as_mut() {
                buf.push(c);
            }
        }
        Action::InputBackspace => {
            if let Some((_, buf)) = app.input.as_mut() {
                buf.pop();
            }
        }
        // 두 키 묶음을 기다리는 중이면 그 묶음을 버린다. 무엇을 기다리는지
        // 말해 놓고 빠져나갈 길이 없으면 사람은 아무 키나 누른다.
        Action::InputCancel if app.pending_op.is_some() => {
            app.pending_op = None;
            app.message = "묶음을 버렸습니다".into();
        }
        Action::InputCancel => app.input = None,
        Action::InputSubmit => {
            if let Some((prefix, buf)) = app.input.take() {
                match prefix {
                    '/' if !buf.is_empty() => app.search(&buf),
                    ':' => run_ex(app, buf.trim()),
                    _ => {}
                }
            }
        }
    }
}

/// 나가기 — **저장하지 않은 편집이 있으면 거부한다.**
///
/// vi 가 `:q` 에 "E37: No write since last change" 를 내고 `:q!` 를 요구하는
/// 것과 같다. 전에는 `q`·`Esc`·`:q` 가 전부 그냥 끝냈고, 그러면 친 글이
/// 말없이 사라진다. 읽기전용으로 열었으면 버퍼가 깨끗하니 그대로 나간다.
fn quit(app: &mut App, force: bool) {
    app.buf.commit_jamo();
    if !force && app.buf.is_dirty() {
        app.message = format!(
            "저장하지 않은 편집이 {}개 있다 — `:w` 로 저장, `ZZ` 저장하고 나가기, \
             `:q!`·`ZQ` 로 버리고 나가기",
            app.buf.pending_edits()
        );
        return;
    }
    app.quit = true;
}

/// `ZZ` · `:wq` · `:x` — 저장하고 나간다. **저장이 멈추면 나가지 않는다.**
fn save_and_quit(app: &mut App) {
    if app.editable && !app.buf.is_dirty() {
        // 바뀐 것이 없으면 그냥 나가는 것이 vi 의 `ZZ` 다
        app.quit = true;
        return;
    }
    // **문구로 판단하지 않는다.** 전에는 상태줄에 "멈췄" 가 들어 있는지 보고
    // 나갈지 정했다 — 저장 성공 여부를 **사람에게 보여 줄 말**에 매달아 둔
    // 것이다. 말투를 일괄로 고친 적이 있는데("저장을 멈췄다" → "저장을
    // 멈췄습니다") 어간이 우연히 같아서 살았다. "중단했습니다" 로 썼다면
    // `ZZ` 가 저장 실패를 모른 채 나가 **쌓인 편집이 사라졌을** 것이다.
    // 프로즈와 제어 흐름을 잇지 않는다.
    if write_file(app) {
        app.quit = true;
    }
}

/// 폭 기준이 바뀌었으니 표와 그림을 다시 그린다.
fn relayout(app: &mut App) {
    let mode = app.image_mode;
    let images = std::mem::take(&mut app.images);
    app.load_images(images, mode);
}

/// `:e!` — 디스크에서 다시 읽는다. 쌓인 편집을 **버린다.**
///
/// 되돌리기가 한 단계씩 가는 데 비해 이쪽은 한 번에 저장 전으로 간다.
fn reload(app: &mut App) {
    match app.reload() {
        Ok(()) => {
            app.message = format!("다시 읽었습니다 — {}", app.doc.column.provenance());
        }
        Err(e) => app.message = format!("다시 읽을 수 없습니다: {e:#}"),
    }
}

/// `:` 명령 — R1 에서는 최소한만.
///
/// `pub(crate)` 인 이유: `api` 의 할 수 있는 일 목록이 `:` 명령을 손으로 적고
/// 있고(키 쓸기로는 찾을 수 없다 — `:` 는 명령줄을 여는 키일 뿐이다), 그 목록이
/// **실제로 받아들여지는지** 시험이 여기 물어본다.
pub(crate) fn run_ex(app: &mut App, cmd: &str) {
    match cmd {
        "q" | "quit" => quit(app, false),
        // vi 의 `!` 는 "그래도 하라" 다 — 버리는 것을 **명시적으로** 말하게 한다
        "q!" | "quit!" => quit(app, true),
        "toc" => app.toggle_toc(),
        // **표 칸에 쓴다.** `i` 가 지금 글자를 채워 이 명령을 띄운다.
        other if other.starts_with("cell ") => {
            set_current_cell(app, other["cell ".len()..].trim_end_matches('\r'));
        }
        "cell" => set_current_cell(app, ""),
        // **표 만들기** — `:table 3x4` 로 커서 단락에 빈 표를 넣는다
        other if other.starts_with("table ") => {
            insert_table(app, other["table ".len()..].trim());
        }
        "table" => {
            app.message = "크기가 필요합니다 — `:table 3x4`".into();
        }
        // 문자표 — 화면에서 `Ctrl-K` 와 같은 것
        // 적어 둔 책갈피 — 열 자리 중 어디에 뭐가 있는지
        "marks" | "책갈피" => {
            let marks = app.buf.mark_list();
            app.message = if marks.is_empty() {
                "책갈피가 없습니다 — `Ctrl-K <숫자>` 로 넣습니다".into()
            } else {
                let list: Vec<String> = marks
                    .iter()
                    .map(|(i, at)| format!("{i}:{}:{}", at.0, at.1))
                    .collect();
                format!("책갈피 — {} (Ctrl-Q <숫자> 로 갑니다)", list.join(" · "))
            };
        }
        "sym" | "symbol" | "기호" => apply(app, Action::OpenSymbols, 20),
        // 한자 — 화면에서 `F9` 와 같은 것
        "hanja" | "한자" => apply(app, Action::OpenHanja, 20),
        "w" | "write" | "w!" | "write!" => { write_file(app); }
        "wq" | "x" | "xit" | "wq!" => save_and_quit(app),
        // **다른 이름으로.** `:w <경로>` 는 사본을 쓰고 **원본을 계속 고친다**
        // (vi 와 같다). `:saveas <경로>` 는 그 뒤로 새 이름을 고친다.
        other
            if other.starts_with("w ")
                || other.starts_with("w! ")
                || other.starts_with("write ")
                || other.starts_with("write! ") =>
        {
            let (head, rest) = other.split_once(' ').unwrap_or((other, ""));
            write_copy(app, rest.trim(), head.ends_with('!'), false);
        }
        other if other.starts_with("saveas ") || other.starts_with("saveas! ") => {
            let (head, rest) = other.split_once(' ').unwrap_or((other, ""));
            write_copy(app, rest.trim(), head.ends_with('!'), true);
        }
        "saveas" | "saveas!" | "w " | "write " => {
            app.message = "경로가 필요합니다 — `:saveas 새이름.hwp`".into();
        }
        // 디스크에서 다시 읽는다 — 쌓인 편집을 버린다
        "e!" | "edit!" => reload(app),
        "e" | "edit" => {
            app.message = if app.buf.is_dirty() {
                format!(
                    "저장하지 않은 편집이 {}개 있습니다 — `:e!` 로 버리고 다시 읽습니다",
                    app.buf.pending_edits()
                )
            } else {
                reload(app);
                return;
            };
        }
        "set ime=2" | "set ime=dubeolsik" => {
            app.buf.ime = crate::buffer::Ime::Dubeolsik;
            app.message = "두벌식 — 터미널 IME 없이 한글을 조합합니다".into();
        }
        "set ime=off" => {
            app.buf.commit_jamo();
            app.buf.ime = crate::buffer::Ime::Off;
            app.message = "키를 그대로 넣는다 (터미널 IME 를 쓸 때)".into();
        }
        "set keys=dos" => {
            crate::pager::set_dos_keys(true);
            app.message =
                "도스 키 — Ctrl-D 글자 오른쪽 · Ctrl-F 낱말 오른쪽 (반화면은 PgDn)".into();
        }
        "set keys=vi" => {
            crate::pager::set_dos_keys(false);
            app.message = "vi 키 — Ctrl-D 반화면 · Ctrl-F 한 화면".into();
        }
        other if other.starts_with("set keys=") => {
            app.message = format!("keys 는 dos 또는 vi 입니다 (받은 값: {})", &other[9..]);
        }
        "set amb=wide" | "set amb=2" => {
            hwp_core::wrap::set_ambiguous_wide(true);
            relayout(app);
            app.message = "─ │ ├ 같은 글자를 2칸으로 센다 (CJK 폭)".into();
        }
        "set amb=narrow" | "set amb=1" => {
            hwp_core::wrap::set_ambiguous_wide(false);
            relayout(app);
            app.message = "─ │ ├ 같은 글자를 1칸으로 셉니다".into();
        }
        other if other.starts_with("set amb=") => {
            app.message = format!("amb 은 wide 또는 narrow 입니다 (받은 값: {})", &other[8..]);
        }
        "set fit=term" => {
            app.fit_term = true;
            app.left = 0;
            app.message = "터미널 폭에 맞춥니다 — 문서의 줄바꿈과 달라집니다".into();
        }
        "set fit=doc" => {
            app.fit_term = false;
            app.message = format!("문서 단 폭에 맞춥니다 — {}", app.doc.column.provenance());
        }
        other if other.starts_with("set fit=") => {
            app.message = format!("fit 은 term 또는 doc 입니다 (받은 값: {})", &other[8..]);
        }
        "" => {}
        other => {
            // 숫자면 그 쪽으로
            if let Ok(n) = other.parse::<u32>() {
                goto_page(app, n);
            } else {
                app.message = format!("모르는 명령: {other}");
            }
        }
    }
}

/// `:w <경로>` · `:saveas <경로>` — **다른 이름으로.**
///
/// 두 가지가 다르다. `:w <경로>` 는 사본을 하나 떨어뜨리고 **원본을 계속
/// 고친다**(그래서 저장 안 한 편집 표시가 남아 있다 — vi 와 같고, 여기서는
/// 글자 그대로 사실이다). `:saveas <경로>` 는 그 뒤로 **새 이름을** 고친다.
///
/// 있는 파일은 **덮지 않는다.** `!` 를 붙여 말해야 덮는다 — 이 도구가 남의
/// 문서를 덮지 않는다는 규칙은 저장 경로에만 있는 것이 아니다.
fn write_copy(app: &mut App, dest: &str, force: bool, switch: bool) {
    use std::path::PathBuf;

    if !app.editable {
        app.message = "읽기전용으로 열렸습니다 — 고치려면 `hwp edit <파일>`".into();
        return;
    }
    if dest.is_empty() {
        app.message = "경로가 필요합니다 — `:saveas 새이름.hwp`".into();
        return;
    }
    app.buf.commit_jamo();

    let src = PathBuf::from(app.doc.path.clone());
    let dest = PathBuf::from(dest);
    // 같은 파일을 가리키면 그냥 평소 저장이다
    if dest == src || (dest.exists() && same_file(&dest, &src)) {
        write_file(app);
        return;
    }
    if dest.exists() && !force {
        app.message = format!(
            "{} 이 이미 있습니다 — 덮으려면 `:{}! {}`",
            dest.display(),
            if switch { "saveas" } else { "w" },
            dest.display()
        );
        return;
    }

    // **텍스트는 백엔드 없이.** 원본을 사본으로 복사해 두고 그 사본에
    // 자가검증 저장을 돌린다 — `save_as` 가 HWP 에서 하는 것과 같은 모양이고,
    // 원본은 끝까지 손대지 않는다.
    let saved = if app.doc.format == "text" {
        std::fs::copy(&src, &dest)
            .map_err(anyhow::Error::from)
            .and_then(|_| hwp_core::text::TextFile::read(&dest))
            .and_then(|f| f.save(app.buf.log()))
            // 사본 저장이 멈추면 **반쯤 쓴 사본을 남기지 않는다**
            .map_err(|e| {
                let _ = std::fs::remove_file(&dest);
                e
            })
    } else {
        let pin = match hwp_core::Pin::load_default() {
            Ok(p) => p,
            Err(e) => {
                app.message = format!("핀을 읽을 수 없습니다: {e}");
                return;
            }
        };
        let rhwp = match hwp_core::Rhwp::discover(&pin) {
            Ok(r) => r,
            Err(e) => {
                app.message = format!("백엔드를 쓸 수 없습니다: {e}");
                return;
            }
        };
        hwp_core::edit::save_as(&rhwp, &src, &dest, app.buf.log())
    };

    match saved {
        Ok(report) => {
            let n = report.ops_applied;
            if switch {
                // 이제부터 **새 이름**을 고친다. `:w` 도 `:e!` 도 그쪽으로 간다.
                app.doc.path = dest.display().to_string();
                app.buf.mark_saved();
                app.message = format!("{} 로 저장했습니다 — 이제 이 파일을 고칩니다", dest.display());
            } else {
                // 사본만 떨어뜨렸다. **원본은 아직 저장 안 된 상태다** —
                // `mark_saved` 하면 `q` 가 편집을 말없이 버리게 된다.
                app.message = format!(
                    "{} 로 썼다 (연산 {n}개) — 고치는 것은 여전히 {}",
                    dest.display(),
                    src.display()
                );
            }
        }
        Err(e) => app.message = format!("쓰지 못했습니다: {e:#}"),
    }
}

/// 두 경로가 **같은 파일**인가 (심볼릭 링크·`./` 같은 표기 차이).
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    // **판정은 한 곳에만 있다** (`hwp_core::edit::same_file`). 전에는 여기서
    // `canonicalize` 로 비교해 하드링크를 놓쳤고, 그 틈으로 문서가 0바이트가
    // 됐다. 기계 쪽도 같은 함수를 쓴다.
    hwp_core::edit::same_file(a, b)
}

/// `:table 3x4` — **커서 단락에 빈 표를 만든다.**
///
/// 백엔드가 표와 함께 단락 하나를 넣으므로 뒤쪽 좌표가 밀리고 표 번호도
/// 밀린다. 새 표가 받을 번호를 여기서 세어 연산에 적는다 — 틀리면 저장이
/// 멈춘다(그게 안전장치다).
///
/// 조판은 저장 뒤 `:e!` 로 다시 읽어야 보인다. 표가 들어가면 쪽 경계와 줄
/// 수가 달라지는데 그건 조판기가 하는 일이고, 우리가 흉내 내면 화면과 문서가
/// 어긋난다 — 상태줄이 "조판 미반영" 이라고 말하는 그 상태다.
fn insert_table(app: &mut App, spec: &str) {
    if !app.editable {
        app.message = "읽기전용으로 열렸습니다 — 고치려면 `hwp edit <파일>`".into();
        return;
    }
    let Some((r, c)) = spec.split_once(['x', 'X', '*', ' ']) else {
        app.message = format!("크기는 `3x4` 꼴입니다 (받은 값: {spec})");
        return;
    };
    let (rows, cols) = match (r.trim().parse::<usize>(), c.trim().parse::<usize>()) {
        (Ok(r), Ok(c)) if r >= 1 && c >= 1 && c <= 256 => (r, c),
        _ => {
            app.message = format!("행은 1 이상, 열은 1~256 입니다 (받은 값: {spec})");
            return;
        }
    };
    // 표를 만들기 전에 **쌓인 편집을 먼저 보낸다.**
    //
    // 표가 들어가면 뒤쪽 단락 좌표가 밀리는데, 쌓아 둔 연산은 밀리기 **전**의
    // 좌표로 적혀 있다. 섞으면 어느 쪽이 먼저인지에 따라 엉뚱한 단락을 고친다.
    if app.buf.is_dirty() {
        app.message = "먼저 저장하세요 (`:w`) — 표를 넣으면 단락 좌표가 밀립니다".into();
        return;
    }
    let at = app.buf.cursor.at;
    // 새 표가 받을 번호 = 그 자리 앞에 있는 표의 수
    let index = app
        .doc
        .tables
        .iter()
        .filter(|t| t.at < at)
        .count() as u32;

    let pin = match hwp_core::Pin::load_default() {
        Ok(p) => p,
        Err(e) => {
            app.message = format!("핀을 읽을 수 없습니다: {e}");
            return;
        }
    };
    let rhwp = match hwp_core::Rhwp::discover(&pin) {
        Ok(r) => r,
        Err(e) => {
            app.message = format!("백엔드를 쓸 수 없습니다: {e}");
            return;
        }
    };
    let mut log = hwp_core::edit::EditLog::new();
    log.push(hwp_core::edit::Op::InsertTable { at, rows, cols, index });
    let path = std::path::PathBuf::from(app.doc.path.clone());
    match hwp_core::edit::save(&rhwp, &path, &log) {
        Ok(_) => {
            // 조판이 달라졌으니 디스크에서 다시 읽는다 — 흉내 내지 않는다
            match app.reload() {
                Ok(()) => {
                    app.message = format!(
                        "구역{} 단락{} 에 {rows}×{cols} 표를 만들었습니다 (표{index}) — T 로 들어갑니다",
                        at.0, at.1
                    );
                }
                Err(e) => app.message = format!("표는 만들었는데 다시 읽을 수 없습니다: {e:#}"),
            }
        }
        Err(e) => app.message = format!("표를 만들지 못했습니다: {e:#}"),
    }
}

/// `i` (표 안) — **지금 칸의 글자를 명령줄에 채워 띄운다.**
///
/// 칸 안에 커서를 넣어 글자를 고치는 쪽이 더 좋겠지만, 버퍼의 편집 단위는
/// 단락이고 칸 글자는 그 바깥에 있다. 그래서 먼저 **되는 길**을 낸다: 지금
/// 글자를 보여 주고 고쳐서 넣게 한다. 칸을 넘나들 수는 있는데 고칠 길이
/// 아예 없던 것이 문제였다(2026-10-10 보고).
fn edit_cell(app: &mut App) {
    if !app.editable {
        app.message = "읽기전용으로 열렸습니다 — 고치려면 `hwp edit <파일>`".into();
        return;
    }
    let Some(at) = app.cell_at() else {
        app.message = "표 안이 아닙니다 — `T` 로 들어갑니다".into();
        return;
    };
    match app.buf.begin_cell_edit(at) {
        Ok(()) => {
            // 끼워넣기 모드와 **같은 모드**다 — 두벌식 조합도 그대로 돈다.
            // 다른 것은 `Esc` 가 본문이 아니라 표로 돌아가는 것뿐이다.
            app.mode = Mode::Insert;
            app.message = format!(
                "표{} {}행{}열 안 — Esc 로 끝냅니다",
                at.0,
                at.1 + 1,
                at.2 + 1
            );
        }
        Err(e) => app.message = e,
    }
}

/// `:cell <글자>` — 지금 칸에 쓴다. 저장은 평소대로 `:w` 가 한다.
fn set_current_cell(app: &mut App, text: &str) {
    if !app.editable {
        app.message = "읽기전용으로 열렸습니다 — 고치려면 `hwp edit <파일>`".into();
        return;
    }
    let Some(at) = app.cell_at() else {
        app.message = "표 안이 아닙니다 — `T` 로 들어간 뒤 `i`".into();
        return;
    };
    // **무엇을 덮었는지 말한다.** 덮어쓰기는 한 번에 사라지는 일이라
    // 전 글자를 보여 주는 쪽이 맞다.
    let was = app.buf.cell_text(at).unwrap_or("").to_string();
    match app.buf.set_cell(at, text) {
        Ok(()) => {
            app.sync_cells();
            app.message = format!(
                "표{} {}행{}열: {was:?} → {text:?} (저장은 `:w`)",
                at.0,
                at.1 + 1,
                at.2 + 1
            );
        }
        Err(e) => app.message = e,
    }
}

/// `:w` — 자가검증 저장. 검증을 통과하지 못하면 원본을 건드리지 않는다.
fn write_file(app: &mut App) -> bool {
    if !app.editable {
        app.message = "읽기전용으로 열렸습니다 — 고치려면 `hwp edit <파일>`".into();
        return false;
    }
    app.buf.commit_jamo();
    if !app.buf.is_dirty() {
        app.message = "바뀐 것이 없습니다".into();
        return false;
    }
    let path = std::path::PathBuf::from(app.doc.path.clone());
    // **텍스트는 백엔드를 거치지 않는다.** 재생할 조판기가 없고 증인도 없다 —
    // 대신 파일 자체가 증인이다(임시본에 쓰고 되읽어 대조). 규율은 같고
    // `--force` 도 없다. `hwp_core::text` 가 그 경로를 들고 있다.
    let saved = if app.doc.format == "text" {
        match hwp_core::text::TextFile::read(&path) {
            Ok(f) => f.save(app.buf.log()),
            Err(e) => Err(e),
        }
    } else {
        let pin = match hwp_core::Pin::load_default() {
            Ok(p) => p,
            Err(e) => {
                app.message = format!("핀을 읽을 수 없습니다: {e}");
                return false;
            }
        };
        let rhwp = match hwp_core::Rhwp::discover(&pin) {
            Ok(r) => r,
            Err(e) => {
                app.message = format!("백엔드를 쓸 수 없습니다: {e}");
                return false;
            }
        };
        save(&rhwp, &path, app.buf.log())
    };
    match saved {
        Ok(report) => {
            app.buf.mark_saved();
            app.message = format!(
                "저장했습니다 — 연산 {}개, {} {}, 백업 {}",
                report.ops_applied,
                if app.doc.format == "text" { "줄" } else { "본문" },
                if app.doc.format == "text" {
                    format!("{}개", report.paragraphs)
                } else {
                    format!("{}단락", report.paragraphs)
                },
                report
                    .backup
                    .as_ref()
                    .and_then(|b| b.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "없음".into())
            );
        }
        Err(e) => {
            // 첫 줄만 상태줄에 — 전문은 종료 후 stderr 로도 나간다
            let text = format!("{e:#}");
            app.message = format!(
                "저장을 멈췄습니다 (원본은 그대로). {}",
                text.lines().next().unwrap_or("")
            );
            return false;
        }
    }
    true
}

fn goto_page(app: &mut App, number: u32) {
    use crate::pager::Row;
    let found = app.rows.iter().position(
        |r| matches!(r, Row::PageRule { number: n, .. } if *n == number),
    );
    match found {
        Some(i) => {
            app.top = i;
            app.message = format!("{number} 쪽");
        }
        None => app.message = format!("{number} 쪽은 없습니다 (1~{})", app.doc.page_count),
    }
}

/// **터미널이 Ambiguous 글자를 몇 칸으로 그리는지 물어본다.**
///
/// 유니코드가 정하지 않고 터미널이 정하는 값이라 코드로는 알 수 없다. 그래서
/// 글자를 하나 찍어 보고 커서가 몇 칸 갔는지 센다 — 1칸이면 좁게, 2칸이면
/// 넓게. `─ │ ├ ┤` 같은 **표 선 문자가 전부 Ambiguous** 여서, 틀리면 표가
/// 어긋난다(실측: 선 22칸 vs 글자 줄 14칸).
///
/// 방법: 커서를 숨기고 맨 왼쪽으로 옮겨 탐침 글자를 찍고, `ESC [ 6 n`
/// (Device Status Report)로 위치를 물어 `ESC [ row ; col R` 을 읽는다.
/// 그리고 그 줄을 지운다.
///
/// **못 알아내면 좁은 쪽으로 둔다.** 요즘 터미널 대부분이 그쪽이고, 틀렸을 때
/// 표가 넓어지는 쪽보다 좁아지는 쪽이 읽기 쉽다. 응답을 기다리느라 멈추지
/// 않도록 150ms 만 기다린다 — DSR 을 모르는 터미널도 있다.
fn detect_ambiguous_width() {
    use std::io::{Read, Write};

    // 사용자가 정했으면 그 말을 따른다 — 물어보지 않는다
    if let Ok(v) = std::env::var("HWP_AMBIGUOUS") {
        let wide = matches!(v.trim(), "wide" | "2" | "cjk");
        hwp_core::wrap::set_ambiguous_wide(wide);
        return;
    }

    let probe = || -> std::io::Result<Option<usize>> {
        let mut out = std::io::stdout();
        // 탐침 글자는 `─`(U+2500) — 우리가 표를 그릴 때 가장 많이 쓰는 글자다
        write!(out, "\x1b[?25l\r─\x1b[6n")?;
        out.flush()?;

        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(150);
        let mut stdin = std::io::stdin();
        while std::time::Instant::now() < deadline {
            // raw 모드에서는 읽을 것이 없으면 바로 돌아온다
            match stdin.read(&mut byte) {
                Ok(1) => {
                    buf.push(byte[0]);
                    if byte[0] == b'R' {
                        break;
                    }
                }
                Ok(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
                Err(_) => break,
            }
        }
        // 줄을 치운다 — 탐침 글자가 화면에 남으면 안 된다
        write!(out, "\r\x1b[2K\x1b[?25h")?;
        out.flush()?;

        // `ESC [ row ; col R` 에서 col 을 뽑는다. 글자 하나를 찍었으니
        // 커서는 2(1칸) 또는 3(2칸) 에 있다.
        let text = String::from_utf8_lossy(&buf);
        let col = text
            .rsplit(';')
            .next()
            .and_then(|t| t.trim_end_matches('R').trim().parse::<usize>().ok());
        Ok(col)
    };

    let wide = match probe() {
        Ok(Some(col)) if col >= 3 => true,
        // 1칸으로 그렸거나(col == 2) 못 알아냈으면 좁은 쪽
        _ => false,
    };
    hwp_core::wrap::set_ambiguous_wide(wide);
}

/// 페이저를 띄운다. 어떻게 끝나든 터미널을 되돌려 놓는다.
pub fn run(app: &mut App) -> Result<()> {
    // 파이프로 돌릴 때 알아보기 어려운 오류로 죽는 대신 길을 알려 준다.
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        anyhow::bail!(
            "표준입력이 터미널이 아니라 페이저를 띄울 수 없다.\n  \
             파이프·스크립트에서는 `hwp page <파일>` 을 쓰면 같은 화면 모델을 글로 받는다."
        );
    }
    enable_raw_mode().context("raw 모드 진입 실패")?;
    // **터미널에 직접 물어본다** — Ambiguous 글자를 몇 칸으로 그리는가.
    // raw 모드에 들어온 뒤여야 응답을 읽을 수 있다.
    detect_ambiguous_width();
    let mut out = stdout();
    crossterm::execute!(out, EnterAlternateScreen).context("대체 화면 진입 실패")?;
    let mut terminal = Terminal::new(CrosstermBackend::new(out)).context("터미널 초기화 실패")?;

    let result = event_loop(&mut terminal, app);

    // 되돌리기는 실패해도 삼키지 않는다 — 터미널이 망가진 채로 끝나면
    // 사용자가 reset 을 쳐야 한다.
    disable_raw_mode().ok();
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    result
}

fn event_loop<B>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()>
where
    B: Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    loop {
        let mut viewport = 1usize;
        terminal.draw(|f| {
            viewport = f.area().height.saturating_sub(2).max(1) as usize;
            draw(f, app);
        })?;

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            step(app, key, viewport);
            if app.quit {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn key_code(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    /// **`:e!` 는 디스크에서 다시 읽어 쌓인 편집을 버린다.**
    ///
    /// 되돌리기가 한 단계씩 가는 데 비해 한 번에 저장 전으로 간다. `:e`(! 없이)는
    /// 편집이 있으면 거부한다 — `!` 가 "그래도 하라" 라는 vi 의 뜻을 지킨다.
    #[test]
    fn reload_throws_away_edits_only_when_told() {
        use hwp_core::{Pin, Rhwp, ScreenDoc};
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
        let dir = std::env::temp_dir().join("hwp-reload-test");
        let _ = std::fs::create_dir_all(&dir);
        let work = dir.join("s13.hwp");
        std::fs::copy(&src, &work).expect("복사");

        let doc = ScreenDoc::build(&rhwp, &work).expect("화면 모델");
        let mut app = App::new(doc, false);
        app.editable = true;
        let target = *app.buf.coords().first().expect("단락");
        let before = app.buf.text_at(target).to_string();

        app.buf.goto(target);
        app.buf.insert_str("지워질 글자");
        assert_ne!(app.buf.text_at(target), before);

        // `:e` — 편집이 있으니 거부한다
        run_ex(&mut app, "e");
        assert!(app.message.contains("저장하지 않은"), "{}", app.message);
        assert_ne!(app.buf.text_at(target), before, ":e 가 버렸다");

        // `:e!` — 버리고 다시 읽는다
        run_ex(&mut app, "e!");
        assert_eq!(app.buf.text_at(target), before, "다시 읽지 않았다");
        assert!(!app.buf.is_dirty(), "다시 읽었는데 편집이 남았다");
        // 그러니 그냥 나갈 수 있다
        apply(&mut app, Action::Quit, 20);
        assert!(app.quit);
    }

    /// **저장하지 않은 편집을 조용히 버리지 않는다** — vi 의 `:q` 와 같다.
    ///
    /// 전에는 `q`·`Esc`·`:q` 가 전부 그냥 끝냈다. 친 글이 말없이 사라지는
    /// 쪽이라 이 시험이 제일 중요하다.
    #[test]
    fn quitting_with_unsaved_edits_is_refused_until_told_otherwise() {
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        app.buf.insert_str("가");
        assert!(app.buf.is_dirty());

        // q — 거부하고 길을 알려준다
        apply(&mut app, Action::Quit, 20);
        assert!(!app.quit, "저장 안 한 편집이 있는데 끝냈다");
        assert!(app.message.contains("저장하지 않은"), "{}", app.message);
        assert!(app.message.contains(":q!"), "버리는 길을 알려주지 않았다: {}", app.message);

        // :q 도 같다
        run_ex(&mut app, "q");
        assert!(!app.quit, ":q 가 끝냈다");

        // :q! — 버리고 나간다
        run_ex(&mut app, "q!");
        assert!(app.quit, ":q! 가 안 끝냈다");
    }

    /// **Esc 는 끝내지 않는다.** vi 에서 Esc 는 "보통 모드로" 다.
    #[test]
    fn escape_does_not_quit_from_normal_mode() {
        assert_eq!(action_for(key_code(KeyCode::Esc), false), Action::Nothing);
        // 끼워넣기 중이면 보통 모드로 돌아간다 (그건 그대로)
        assert_eq!(
            action_for_mode(key_code(KeyCode::Esc), false, Mode::Insert),
            Action::Normal
        );
        // `:`/`/` 입력 중이면 그 입력을 취소한다
        assert_eq!(action_for(key_code(KeyCode::Esc), true), Action::InputCancel);
        // 끝내는 키는 q 다
        assert_eq!(action_for(key('q'), false), Action::Quit);
    }

    /// `ZZ` 저장하고 나가기 · `ZQ` 버리고 나가기 — 짝으로 받는다.
    #[test]
    fn zz_saves_and_zq_discards() {
        assert_eq!(action_for(key('Z'), false), Action::Operator('Z'));
        assert_eq!(action_for(key('Q'), false), Action::Operator('Q'));

        // ZQ — 편집이 있어도 버리고 나간다
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        app.buf.insert_str("가");
        apply(&mut app, Action::Operator('Z'), 20);
        assert!(!app.quit, "Z 하나로 끝냈다");
        assert!(app.message.contains("ZZ"), "무엇을 기다리는지 말하지 않았다");
        apply(&mut app, Action::Operator('Q'), 20);
        assert!(app.quit, "ZQ 가 안 끝냈다");

        // Z 뒤에 엉뚱한 키가 오면 그 키로 다시 기다린다 (vi 와 같다)
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        apply(&mut app, Action::Operator('Z'), 20);
        apply(&mut app, Action::Operator('d'), 20);
        assert_eq!(app.pending_op, Some('d'), "짝이 아닌 키를 삼켰다");
        assert!(!app.quit);
    }

    /// 바뀐 것이 없으면 `q` 가 그냥 나간다 — 읽기만 할 때 막히면 안 된다.
    #[test]
    fn a_clean_buffer_quits_without_asking() {
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        assert!(!app.buf.is_dirty());
        apply(&mut app, Action::Quit, 20);
        assert!(app.quit, "깨끗한데 막았다");

        // 읽기전용도 막히지 않는다
        let mut app = App::new(crate::pager::tests::fixture(), false);
        apply(&mut app, Action::Quit, 20);
        assert!(app.quit, "읽기전용인데 막았다");
    }

    /// **`dd` → `u` 가 글을 되살리고, `dd` → `p` 가 옮겨 붙인다.**
    ///
    /// 그리고 그 뒤에 자가검증 저장이 통과해야 한다 — 되돌리기가 버퍼만
    /// 고치고 재생 로그를 그대로 두면, 화면은 멀쩡한데 저장이 지운 상태를
    /// 보낸다. 둘이 어긋나는지는 저장까지 가 봐야 안다.
    #[test]
    fn delete_undo_paste_and_then_save_agree() {
        use hwp_core::{Pin, Rhwp, ScreenDoc};

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("워크스페이스 루트")
            .to_path_buf();
        let src = root.join("samples/s13.hwp");
        if !src.exists() {
            eprintln!("건너뜀 — samples/s13.hwp 가 없다");
            return;
        }
        let Ok(pin) = Pin::load_default() else { return };
        let Ok(rhwp) = Rhwp::discover(&pin) else {
            eprintln!("건너뜀 — rhwp 백엔드가 없다");
            return;
        };
        let dir = std::env::temp_dir().join("hwp-yank-test");
        let _ = std::fs::create_dir_all(&dir);
        let work = dir.join("s13.hwp");
        std::fs::copy(&src, &work).expect("복사");

        let doc = ScreenDoc::build(&rhwp, &work).expect("화면 모델");
        let mut app = App::new(doc, false);
        app.editable = true;

        // 글이 있는 단락으로 간다
        let target = app
            .buf
            .coords()
            .iter()
            .copied()
            .find(|at| !app.buf.text_at(*at).trim().is_empty())
            .expect("글이 있는 단락");
        app.buf.goto(target);
        let before = app.buf.text_at(target).to_string();
        let total = app.buf.coords().len();

        // dd — 지우고 담는다
        apply(&mut app, Action::Operator('d'), 20);
        apply(&mut app, Action::Operator('d'), 20);
        assert_eq!(app.buf.coords().len(), total - 1, "단락이 줄지 않았다");

        // u — 되살아난다
        apply(&mut app, Action::Undo, 20);
        assert_eq!(app.buf.coords().len(), total, "단락이 안 돌아왔다");
        assert_eq!(app.buf.text_at(target), before, "글이 달라졌다");
        // 되돌렸으니 보낼 것이 없다
        assert_eq!(app.buf.pending_edits(), 0, "되돌렸는데 연산이 남았다");

        // p — 담아 둔 것을 하나 더 붙인다
        apply(&mut app, Action::Paste { before: false }, 20);
        assert_eq!(app.buf.coords().len(), total + 1, "붙인 단락이 안 생겼다");
        assert_eq!(app.buf.text_at(app.buf.cursor.at), before, "붙인 글이 다르다");

        // 그 상태로 저장이 통과한다
        let log = app.buf.log().clone();
        hwp_core::edit::save(&rhwp, &work, &log).expect("자가검증 저장");
        let reread = ScreenDoc::build(&rhwp, &work).expect("다시 읽기");
        let got = reread
            .pages
            .iter()
            .flat_map(|p| &p.blocks)
            .filter(|b| b.para_index.is_some())
            .filter(|b| b.lines.join("").contains(before.trim()))
            .count();
        assert!(got >= 2, "붙인 단락이 저장된 문서에 없다 (같은 글 {got}군데)");
    }

    /// 키 매핑 — `y` 는 연산자(두 번), `p`/`P` 는 바로, `Ctrl-r` 은 다시하기.
    #[test]
    fn yank_paste_and_redo_are_bound_like_vi() {
        assert_eq!(action_for(key('y'), false), Action::Operator('y'));
        assert_eq!(action_for(key('p'), false), Action::Paste { before: false });
        assert_eq!(action_for(key('P'), false), Action::Paste { before: true });
        assert_eq!(action_for(ctrl('r'), false), Action::Redo);
        // 담은 것이 없으면 그렇다고 말한다
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        apply(&mut app, Action::Paste { before: false }, 20);
        assert!(app.message.contains("담아 둔 것이 없"), "{}", app.message);
    }

    /// **표 모드의 키가 칸을 옮긴다** — 본문 모드와 다르게 묶여야 한다.
    ///
    /// 표에서 `h`/`l` 이 글자를 옮기면 쓸 수가 없다. 사람이 표에서 하려는
    /// 일은 칸을 고르는 것이고, 칸 안 글자는 들어가서 고친다.
    #[test]
    fn table_mode_rebinds_hjkl_to_cells() {
        use crate::pager::Mode;
        assert_eq!(action_for_mode(key('l'), false, Mode::Table), Action::MoveCell(0, 1));
        assert_eq!(action_for_mode(key('h'), false, Mode::Table), Action::MoveCell(0, -1));
        assert_eq!(action_for_mode(key('j'), false, Mode::Table), Action::MoveCell(1, 0));
        assert_eq!(action_for_mode(key('k'), false, Mode::Table), Action::MoveCell(-1, 0));
        assert_eq!(
            action_for_mode(key_code(KeyCode::Tab), false, Mode::Table),
            Action::NextCell(true)
        );
        assert_eq!(
            action_for_mode(key_code(KeyCode::BackTab), false, Mode::Table),
            Action::NextCell(false)
        );
        assert_eq!(
            action_for_mode(key_code(KeyCode::Esc), false, Mode::Table),
            Action::LeaveTable
        );
        // 표 안에서 T 는 다음 표
        assert_eq!(action_for_mode(key('T'), false, Mode::Table), Action::EnterTable);
        // 본문 모드에서는 그대로 글자·줄 이동이다
        assert_eq!(action_for_mode(key('l'), false, Mode::Body), Action::Chars(1));
        assert_eq!(action_for_mode(key('T'), false, Mode::Body), Action::EnterTable);
    }

    /// **표 안에서는 세 벌이 같은 칸으로 간다 — 화살표 · wasd · hjkl.**
    ///
    /// 칸을 고르는 일은 vi 보다 게임에 가깝다. 표 모드는 분기가 먼저 돌아서
    /// 본문의 `a`(넣기)·`d`(지우기)·`w`(낱말 앞)와 부딪히지 않는다 — 네 자리가
    /// 비어 있었다. **본문에서는 그대로여야 한다**는 것이 이 시험의 반쪽이다.
    #[test]
    fn three_key_sets_move_the_same_cell_and_the_body_keeps_its_own() {
        use crate::pager::Mode;
        let moves = [
            (Action::MoveCell(-1, 0), 'w', 'k', KeyCode::Up),
            (Action::MoveCell(1, 0), 's', 'j', KeyCode::Down),
            (Action::MoveCell(0, -1), 'a', 'h', KeyCode::Left),
            (Action::MoveCell(0, 1), 'd', 'l', KeyCode::Right),
        ];
        for (want, game, vi, arrow) in moves {
            assert_eq!(action_for_mode(key(game), false, Mode::Table), want, "{game:?}");
            assert_eq!(action_for_mode(key(vi), false, Mode::Table), want, "{vi:?}");
            assert_eq!(action_for_mode(key_code(arrow), false, Mode::Table), want, "{arrow:?}");
        }

        // **본문은 그대로다.** 여기서 `d`·`a`·`w` 가 칸 이동이 되면 vi 가 깨진다.
        assert_eq!(action_for_mode(key('d'), false, Mode::Body), Action::Operator('d'));
        assert_eq!(
            action_for_mode(key('a'), false, Mode::Body),
            Action::Insert { after: true, line_end: false, line_start: false }
        );
        assert_eq!(
            action_for_mode(key('w'), false, Mode::Body),
            Action::MoveWord(crate::buffer::Word::Forward)
        );
        // 끼워넣기 중에는 전부 **글자**다
        assert_eq!(action_for_mode(key('w'), false, Mode::Insert), Action::Type('w'));
        assert_eq!(action_for_mode(key('s'), false, Mode::Insert), Action::Type('s'));
    }

    /// 메뉴도 고르는 일이다 — 같은 세 벌을 받는다.
    #[test]
    fn the_menu_takes_the_game_keys_too() {
        assert_eq!(action_for_menu(key('w')), Action::MenuMove(-1));
        assert_eq!(action_for_menu(key('s')), Action::MenuMove(1));
        assert_eq!(action_for_menu(key('a')), Action::MenuGroup(-1));
        assert_eq!(action_for_menu(key('d')), Action::MenuGroup(1));
        // 기존 벌도 그대로
        assert_eq!(action_for_menu(key('k')), Action::MenuMove(-1));
        assert_eq!(action_for_menu(key_code(KeyCode::Down)), Action::MenuMove(1));
        assert_eq!(action_for_menu(key_code(KeyCode::Enter)), Action::MenuPick);
    }

    /// **보기 방식이 옵션이 아니라 키로 바뀐다** — `z` 가 돌리고 `W` 가 폭을 바꾼다.
    ///
    /// 명령줄 옵션이 13개 명령에 걸쳐 늘어났고, 그중 `--images`·`--fit-term` 은
    /// 화면을 보면서 바꾸는 값이다. 켜 보고 결정하는 것을 열기 전에 정하라고
    /// 하면 안 된다.
    #[test]
    fn display_choices_are_keys_not_flags() {
        use crate::pager::ImageMode;
        use hwp_core::dots::Glyphs;
        let fixture = crate::pager::tests::fixture;

        assert_eq!(action_for(key('z'), false), Action::CycleImages);
        assert_eq!(action_for(key('W'), false), Action::ToggleWidth);
        // 비워 뒀던 그 자리에 이제 낱말 이동이 들어갔다
        assert_eq!(
            action_for(key('w'), false),
            Action::MoveWord(crate::buffer::Word::Forward)
        );

        let mut app = App::new(fixture(), false);
        app.load_images(Vec::new(), ImageMode::Art(Glyphs::Blocks));

        // **한 바퀴 돌아 제자리로 온다.** 방식의 **개수**에 묶지 않는다 —
        // 색을 더하자 "다섯 번에 제자리" 가 틀렸고, 그건 방식이 늘 때마다
        // 고쳐야 하는 수다. 돌아오는지와 중복이 없는지만 본다.
        let start = app.image_mode;
        assert_eq!(start, ImageMode::Art(Glyphs::Blocks), "시작을 못 세웠다");
        let mut seen = vec![start];
        for _ in 0..12 {
            apply(&mut app, Action::CycleImages, 20);
            let m = app.image_mode;
            if m == start {
                break;
            }
            seen.push(m);
        }
        assert_eq!(app.image_mode, start, "한 바퀴에 제자리로 안 온다: {seen:?}");
        // 같은 방식이 두 번 나오면 하나를 못 본다
        let labels: std::collections::BTreeSet<&str> = seen.iter().map(|m| m.label()).collect();
        assert_eq!(labels.len(), seen.len(), "같은 방식이 두 번 나온다: {seen:?}");
        // 색도 그 바퀴 안에 있어야 한다 — 기본값으로 쓰는 방식이다
        assert!(seen.contains(&ImageMode::Color), "색이 순환에 없다: {seen:?}");
        // 바뀔 때마다 사람에게 어느 방식인지 말한다
        assert!(!app.message.is_empty(), "무엇으로 바뀌었는지 말하지 않는다");

        // 폭 토글
        let before = app.fit_term;
        apply(&mut app, Action::ToggleWidth, 20);
        assert_ne!(app.fit_term, before, "폭이 바뀌지 않았다");
        assert!(app.message.contains('칸'), "바뀐 폭을 말하지 않는다: {}", app.message);
        apply(&mut app, Action::ToggleWidth, 20);
        assert_eq!(app.fit_term, before, "되돌아오지 않았다");
    }

    /// **Tab 이 문서의 단계 사다리를 따라 움직이고, 저장까지 간다.**
    ///
    /// 글머리 기호가 문단 속성이 아니라 실제 글자이므로(코퍼스 664단락 전수
    /// 대조) 단계 옮기기는 글자 편집이고 검증된 저장 경로를 그대로 탄다.
    #[test]
    fn tab_moves_a_paragraph_down_the_document_own_ladder() {
        use hwp_core::outline::Levels;
        use hwp_core::{Pin, Rhwp, ScreenDoc};

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("워크스페이스 루트")
            .to_path_buf();
        let src = root.join("samples/s13.hwp");
        if !src.exists() {
            eprintln!("건너뜀 — samples/s13.hwp 가 없다");
            return;
        }
        let Ok(pin) = Pin::load_default() else { return };
        let Ok(rhwp) = Rhwp::discover(&pin) else {
            eprintln!("건너뜀 — rhwp 백엔드가 없다");
            return;
        };
        let dir = std::env::temp_dir().join("hwp-tab-test");
        let _ = std::fs::create_dir_all(&dir);
        let work = dir.join("s13.hwp");
        std::fs::copy(&src, &work).expect("복사");

        let doc = ScreenDoc::build(&rhwp, &work).expect("화면 모델");
        let mut app = App::new(doc, false);
        app.editable = true;

        // 사다리: s13 은 1단계 번호 / 2단계 `○` 0칸들여 / 3단계 `-` 3칸들여
        let ladder = app.levels();
        assert!(ladder.len() >= 3, "사다리가 {}단계뿐이다", ladder.len());

        // `○` 로 시작하는 2단계 단락을 하나 찾는다
        let target = app
            .buf
            .coords()
            .iter()
            .copied()
            .find(|at| ladder.level_of_text(app.buf.text_at(*at)) == Some(2))
            .expect("2단계 단락");
        let before = app.buf.text_at(target).to_string();
        app.buf.goto(target);

        apply(&mut app, Action::Relevel(1), 20);
        let after = app.buf.text_at(target).to_string();
        assert_ne!(after, before, "Tab 이 아무것도 바꾸지 않았다: {before:?}");
        assert_eq!(
            ladder.level_of_text(&after),
            Some(3),
            "3단계로 가지 않았다: {before:?} -> {after:?}"
        );
        // 3단계의 들여쓰기와 기호를 그 문서에서 읽어 썼다
        let st = ladder.step(3).expect("3단계");
        assert!(
            after.starts_with(&format!("{}{}", " ".repeat(st.indent), st.marker)),
            "사다리의 3단계({}칸 + `{}`)와 다르다: {after:?}",
            st.indent,
            st.marker
        );

        // 되돌리면 제자리
        apply(&mut app, Action::Relevel(-1), 20);
        assert_eq!(
            ladder.level_of_text(app.buf.text_at(target)),
            Some(2),
            "Shift-Tab 으로 2단계로 못 돌아왔다"
        );

        // 그리고 저장이 통과한다 — 자가검증이 받아들인다는 뜻
        apply(&mut app, Action::Relevel(1), 20);
        let log = app.buf.log().clone();
        hwp_core::edit::save(&rhwp, &work, &log).expect("자가검증 저장");
        let reread = ScreenDoc::build(&rhwp, &work).expect("다시 읽기");
        let got = reread
            .pages
            .iter()
            .flat_map(|p| &p.blocks)
            .find(|b| (b.section, b.para_index) == (target.0, Some(target.1)))
            .map(|b| b.lines.join(""))
            .unwrap_or_default();
        assert_eq!(
            Levels::build(&[]).is_empty(),
            true,
            "빈 사다리는 비어 있어야 한다"
        );
        assert!(
            got.contains(&st.marker),
            "저장된 문서에 3단계 기호 `{}` 가 없다: {got:?}",
            st.marker
        );
    }

    /// **도스 시절 단축키가 vi 와 나란히 돈다.**
    ///
    /// 출처는 보르랜드 IDE 계보를 문서화한 Free Pascal IDE 키 목록이다.
    /// 겹치지 않는 것은 모드 없이 그냥 받는다 — 두 벌을 외우게 하는 대신
    /// 손이 기억하는 쪽으로 눌러도 되게 한다.
    #[test]
    fn dos_era_shortcuts_work_beside_the_vi_ones() {
        use crate::buffer::Word;
        let f = |n: u8| KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE);
        let alt = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT);
        let sh = |c: KeyCode| KeyEvent::new(c, KeyModifiers::SHIFT);
        let cti = |c: KeyCode| KeyEvent::new(c, KeyModifiers::CONTROL);

        // F-키 — 도스 프로그램의 공통 언어
        assert_eq!(action_for(f(1), false), Action::ToggleHelp);
        assert_eq!(action_for(f(2), false), Action::Save);
        assert_eq!(action_for(f(10), false), Action::OpenMenu);
        assert_eq!(action_for(alt('x'), false), Action::Quit);
        // 워드스타: Ctrl-Y 줄(우리는 단락) 지우기 · Ctrl-L 다시 찾기
        assert_eq!(action_for(ctrl('y'), false), Action::Operator('d'));
        assert_eq!(action_for(ctrl('l'), false), Action::Hit(true));
        // 커서 다이아몬드 (겹치지 않는 셋)
        assert_eq!(action_for(ctrl('e'), false), Action::Lines(-1));
        assert_eq!(action_for(ctrl('x'), false), Action::Lines(1));
        assert_eq!(action_for(ctrl('s'), false), Action::Chars(-1));
        assert_eq!(action_for(ctrl('a'), false), Action::MoveWord(Word::Back));
        // IBM CUA 클립보드
        assert_eq!(action_for(cti(KeyCode::Insert), false), Action::Operator('y'));
        assert_eq!(action_for(sh(KeyCode::Delete), false), Action::Operator('d'));
        assert_eq!(
            action_for(sh(KeyCode::Insert), false),
            Action::Paste { before: false }
        );
        // 두 키 묶음의 첫 키
        assert_eq!(action_for(ctrl('k'), false), Action::Operator('\u{1}'));
        assert_eq!(action_for(ctrl('q'), false), Action::Operator('\u{2}'));

        // **겹치는 둘만 설정으로 가른다.** 기본은 vi 다.
        crate::pager::set_dos_keys(false);
        assert_eq!(action_for(ctrl('d'), false), Action::HalfScreens(1));
        assert_eq!(action_for(ctrl('f'), false), Action::Screens(1));
        crate::pager::set_dos_keys(true);
        assert_eq!(action_for(ctrl('d'), false), Action::Chars(1));
        assert_eq!(action_for(ctrl('f'), false), Action::MoveWord(Word::Forward));
        crate::pager::set_dos_keys(false); // 다른 시험에 영향 주지 않게 되돌린다
    }

    /// `Ctrl-K` · `Ctrl-Q` 두 키 묶음이 실제로 그 일을 한다.
    #[test]
    fn the_wordstar_two_key_sequences_do_the_work() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        let n = app.buf.coords().len();
        // **실제 키로 넣는다.**
        //
        // 전에는 `apply(Operator('\u{1}'))` + `apply(Operator('s'))` 를 직접
        // 불렀다. 그건 두 번째 키가 이미 `Operator` 로 와 있다고 가정한 것이고,
        // 실제 키 표는 보통 모드의 `s` 를 아무 데도 걸지 않아 `Nothing` 을
        // 냈다 — **도스 두 키 묶음이 한 번도 동작하지 않았는데 이 시험은
        // 통과하고 있었다**(2026-10-11 실측). 버그가 사는 층을 건너뛴 시험이다.
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        let plain = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);

        // Ctrl-K 를 누르면 **무엇을 기다리는지 말한다**
        step(&mut app, ctrl('k'), 20);
        assert_eq!(app.pending_op, Some('\u{1}'));
        assert!(app.message.contains("Ctrl-K"), "{}", app.message);
        assert!(app.message.contains("저장"), "{}", app.message);

        // Ctrl-K Y — 단락 지우기
        step(&mut app, plain('y'), 20);
        assert_eq!(app.buf.coords().len(), n - 1, "Ctrl-K Y 가 안 지웠다");
        assert!(app.message.contains("Ctrl-K Y"), "{}", app.message);

        // Ctrl-K C — 담긴 것을 붙인다 (지울 때 담겼다)
        step(&mut app, ctrl('k'), 20);
        step(&mut app, plain('c'), 20);
        assert_eq!(app.buf.coords().len(), n, "Ctrl-K C 가 안 붙였다");

        // Ctrl-Q 는 다른 묶음이다
        step(&mut app, ctrl('q'), 20);
        assert_eq!(app.pending_op, Some('\u{2}'));
        assert!(app.message.contains("Ctrl-Q"), "{}", app.message);
        // Ctrl-Q D — 단락 끝으로
        step(&mut app, plain('d'), 20);
        assert_eq!(app.buf.cursor.offset, app.buf.max_offset());

        // **묶음에 없는 키는 삼키지 않고 말한다.** 전에는 그 키로 다시
        // 기다려서 `Ctrl-K z` 가 그림 보기 방식을 돌렸다.
        step(&mut app, ctrl('k'), 20);
        step(&mut app, plain('z'), 20);
        assert!(app.message.contains("는 없"), "{}", app.message);
        assert_eq!(app.pending_op, None, "없는 키로 계속 기다린다");

        // Esc 로 버린다 — 무엇을 기다리는지 말해 놓고 빠져나갈 길이 있어야 한다
        step(&mut app, ctrl('k'), 20);
        step(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 20);
        assert_eq!(app.pending_op, None, "Esc 가 묶음을 안 버렸다");
    }

    /// **고치기 모드에서도 멀리 움직일 수 있는가.**
    ///
    /// 2026-10-11 실측: `hwp edit` 에서 `g`·`G`·`Ctrl-F`·`Ctrl-B`·`Ctrl-D`·
    /// `]`·`[` 가 **전부 죽어 있었다.** 그 동작들이 보기(`app.top`)만 옮기는데,
    /// 한 걸음 뒤에 `reveal_cursor` 가 커서 쪽으로 보기를 되돌려서 아무 일도
    /// 일어나지 않았다. 읽기 전용에서는 멀쩡했으므로 **고치기 모드만** 그랬고,
    /// 65단락 문서를 `j`/`k` 로만 다닐 수 있었다.
    ///
    /// 시험이 없었던 자리다. `apply` 를 직접 부르는 시험은 `app.top` 이
    /// 움직이는 것을 보고 통과한다 — `reveal_cursor` 는 `step` 에 있다.
    #[test]
    fn in_edit_mode_the_big_motions_move_the_cursor_not_just_the_view() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        // 줄이 넉넉한 문서를 세운다 — 한 화면보다 길어야 `Ctrl-F` 가 의미 있다
        let mut doc = crate::pager::tests::fixture();
        let blocks: Vec<hwp_core::screen::Block> = (0..40u32)
            .map(|i| hwp_core::screen::Block {
                section: 0,
                para_index: Some(i),
                lines: vec![format!("{i}번째 단락")],
                continues: false,
                continued: false,
            })
            .collect();
        doc.pages = vec![
            hwp_core::screen::Page {
                number: 1,
                printed: Some(1),
                section: 0,
                blocks: blocks[..20].to_vec(),
            },
            hwp_core::screen::Page {
                number: 2,
                printed: Some(2),
                section: 0,
                blocks: blocks[20..].to_vec(),
            },
        ];
        doc.page_count = 2;
        doc.para_count = 40;
        doc.section_counts = vec![40];
        // 쪽을 갈아끼웠으니 원문도 다시 채운다 — `paras` 가 버퍼의 본문이다
        doc.paras.clear();
        let doc = doc.with_paras_from_blocks();

        let mut app = App::new(doc, false);
        app.editable = true;
        let k = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);

        let first = app.buf.cursor.at;
        for (name, key) in [
            ("G", k('G')),
            ("Ctrl-F", ctrl('f')),
            ("Ctrl-D", ctrl('d')),
            ("]", k(']')),
        ] {
            app.buf.goto(first);
            step(&mut app, key, 10);
            assert_ne!(
                app.buf.cursor.at, first,
                "고치기 모드에서 `{name}` 이 커서를 옮기지 않았다"
            );
        }

        // 끝에서 처음으로 돌아온다
        step(&mut app, k('G'), 10);
        let last = app.buf.cursor.at;
        step(&mut app, k('g'), 10);
        assert_eq!(app.buf.cursor.at, first, "`g` 가 처음으로 안 갔다");
        assert_ne!(last, first, "`G` 가 끝으로 안 갔다 — 시험이 비었다");

        // **읽기 전용에서는 보기를 옮긴다** — 그쪽은 전부터 맞았다
        let mut ro = App::new(crate::pager::tests::fixture(), false);
        ro.editable = false;
        let before = ro.top;
        step(&mut ro, k('G'), 10);
        assert!(ro.top >= before, "읽기 전용에서 보기가 안 움직였다");
    }

    /// **책갈피** — `Ctrl-K <숫자>` 로 적고 `Ctrl-Q <숫자>` 로 돌아간다.
    ///
    /// 워드스타의 마크이고 HWP 가 지금도 그 키를 쓴다(`Ctrl+K,1-0` 책갈피
    /// 바로 넣기 · `Ctrl+Q,1-0` 그 자리로 이동).
    #[test]
    fn a_bookmark_remembers_a_spot_and_brings_you_back() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        let plain = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);

        // 비어 있는 책갈피로 가려 하면 **넣는 법을 말한다**
        step(&mut app, ctrl('q'), 20);
        step(&mut app, plain('3'), 20);
        assert!(app.message.contains("비었"), "{}", app.message);

        step(&mut app, plain('j'), 20);
        let here = app.buf.cursor.at;
        step(&mut app, ctrl('k'), 20);
        step(&mut app, plain('3'), 20);
        assert!(app.message.contains("책갈피 3"), "{}", app.message);

        // 멀리 갔다가 — **`g`/`G` 가 고치기 모드에서 커서를 옮긴다**(전에는
        // 보기만 옮기고 `reveal_cursor` 가 되돌려 아무 일도 안 했다).
        // 손으로 세운 문서는 단락이 둘뿐이라 `g`(처음)로 멀어진다.
        step(&mut app, plain('g'), 20);
        assert_ne!(app.buf.cursor.at, here, "`g` 가 커서를 안 옮겼다");
        // 돌아온다
        step(&mut app, ctrl('q'), 20);
        step(&mut app, plain('3'), 20);
        assert_eq!(app.buf.cursor.at, here, "책갈피로 안 돌아왔다");
    }

    /// `Ctrl-T` 낱말 지우기 · `Ctrl-Q Y` 단락 끝까지 — 워드스타의 지우기 묶음.
    ///
    /// `Ctrl-T` 가 vi 의 `dw` 자리를 채운다. 한국어는 띄어쓰기가 경계라 `w` 가
    /// 가는 자리와 **같은 셈**을 써야 한다 — 두 군데서 따로 세면 어긋난다.
    #[test]
    fn the_wordstar_deletions_cut_a_word_and_the_rest_of_the_line() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        let plain = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);

        let at = app.buf.cursor.at;
        app.buf.replace_paragraph(at, "첫째 낱말 그리고 나머지");
        app.buf.cursor.offset = 0;

        // **`Ctrl-T` 는 `w` 가 가는 자리까지 지운다.** 처음엔 따로 세서
        // 낱말만 지우고 뒤 공백을 남겼다(` 낱말 그리고 나머지`) — 같은 일을
        // 두 군데서 세면 어긋난다.
        let mut probe = App::new(crate::pager::tests::fixture(), false);
        probe.buf.replace_paragraph(at, "첫째 낱말 그리고 나머지");
        probe.buf.cursor.offset = 0;
        probe.buf.move_word(crate::buffer::Word::Forward);
        let want = probe.buf.cursor.offset;
        assert_eq!(want, 3, "`w` 가 가는 자리가 달라졌다 — 시험의 전제가 바뀌었다");

        step(&mut app, ctrl('t'), 20);
        assert_eq!(
            app.buf.text_at(at),
            "낱말 그리고 나머지",
            "`w` 가 가는 자리({want})까지 지워야 한다"
        );

        // Ctrl-Q Y — 커서에서 단락 끝까지
        app.buf.cursor.offset = 2;
        step(&mut app, ctrl('q'), 20);
        step(&mut app, plain('y'), 20);
        assert_eq!(app.buf.text_at(at), "낱말", "단락 끝까지 안 지웠다");

        // 되돌리기로 둘 다 돌아온다
        step(&mut app, plain('u'), 20);
        assert_eq!(app.buf.text_at(at), "낱말 그리고 나머지");
    }

    #[test]
    fn vi_keys_map_to_motions() {
        assert_eq!(action_for(key('j'), false), Action::Lines(1));
        assert_eq!(action_for(key('k'), false), Action::Lines(-1));
        assert_eq!(action_for(key('G'), false), Action::Bottom);
        assert_eq!(action_for(key(']'), false), Action::Page(true));
        assert_eq!(action_for(ctrl('d'), false), Action::HalfScreens(1));
        assert_eq!(action_for(key('q'), false), Action::Quit);
        assert_eq!(action_for(key('/'), false), Action::BeginInput('/'));
    }

    /// 키 입력부터 자가검증 저장까지, 실제 문서로 끝까지.
    ///
    /// 화면은 띄울 수 없으니 동작 열거형으로 구동한다 — 그게 키 처리의 전부이고
    /// 터미널 쪽은 판단을 담지 않는다.
    #[test]
    fn typing_and_saving_a_real_document_end_to_end() {
        use crate::buffer::Ime;
        use hwp_core::witness::Testimony;
        use hwp_core::{Pin, Rhwp, ScreenDoc};

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("워크스페이스 루트")
            .to_path_buf();
        let Ok(entries) = std::fs::read_dir(root.join("samples")) else {
            eprintln!("건너뜀 — samples/ 가 없다");
            return;
        };
        let Some(src) = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("hwp"))
            .min()
        else {
            return;
        };

        let pin = Pin::load_default().expect("핀");
        let Ok(rhwp) = Rhwp::discover(&pin) else {
            eprintln!("건너뜀 — 백엔드를 쓸 수 없다");
            return;
        };

        let dir = std::env::temp_dir().join("hwp-cmd-e2e");
        std::fs::create_dir_all(&dir).expect("임시");
        let work = dir.join("doc.hwp");
        std::fs::copy(&src, &work).expect("복사");

        let doc = ScreenDoc::build(&rhwp, &work).expect("화면 모델");
        let mut app = App::new(doc, false);
        app.editable = true;
        app.buf.ime = Ime::Dubeolsik;

        // 글자가 있는 단락으로 커서를 옮긴다
        while app.buf.max_offset() < 2 {
            let before = app.buf.cursor.at;
            apply(&mut app, Action::Lines(1), 20);
            if app.buf.cursor.at == before {
                eprintln!("건너뜀 — 글자 있는 단락이 없다");
                return;
            }
        }
        let at = app.buf.cursor.at;
        let original = app.buf.text_at(at).to_string();

        // `i` 로 끼워넣기 → 두벌식으로 '검수' → Esc
        apply(&mut app, Action::Insert { after: false, line_end: false, line_start: false }, 20);
        assert_eq!(app.mode, Mode::Insert);
        for k in "rjatn".chars() {
            apply(&mut app, action_for_mode(key(k), false, Mode::Insert), 20);
        }
        apply(&mut app, Action::Normal, 20);
        assert_eq!(app.mode, Mode::Body);
        assert_eq!(
            app.buf.text_at(at),
            format!("검수{original}"),
            "두벌식으로 조합된 글자가 버퍼에 들어가야 한다"
        );
        assert!(app.buf.is_dirty());

        // `:w` — 자가검증 저장
        apply(&mut app, Action::BeginInput(':'), 20);
        for c in "w".chars() {
            apply(&mut app, Action::InputChar(c), 20);
        }
        apply(&mut app, Action::InputSubmit, 20);
        assert!(
            app.message.starts_with("저장했"),
            "저장이 통과해야 한다. 상태줄: {}",
            app.message
        );
        assert!(!app.buf.is_dirty(), "저장 후에는 깨끗해야 한다");

        // 디스크를 독립 증인으로 확인한다
        let saved = Testimony::read(&work).expect("증인");
        let now = saved
            .at(at.0, at.1)
            .map(|p| hwp_core::offset::visible(&p.text))
            .expect("그 단락");
        assert_eq!(now, format!("검수{original}"));
        assert!(work.with_extension("hwp.bak").exists(), "백업이 남아야 한다");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_only_mode_refuses_edits_and_says_why() {
        use hwp_core::screen::{Block, Column, Page};
        use hwp_core::ScreenDoc;
        let doc = ScreenDoc {
            paras: Default::default(),
            path: "/tmp/x.hwp".into(),
            format: "hwp5".into(),
            page_count: 1,
            para_count: 1,
            section_counts: vec![1],
            outline: Vec::new(),
            tables: Vec::new(),
            body_width: 0,
            indents: Default::default(),
            aligns: Default::default(),
            runs: Default::default(),
            objects: Default::default(),
            column: Column { cells: 20, measured: None },
            pages: vec![Page {
                number: 1,
                printed: Some(1),
                section: 0,
                blocks: vec![Block {
                    section: 0,
                    para_index: Some(0),
                    lines: vec!["과업명".into()],
                    continues: false,
                    continued: false,
                }],
            }],
        }.with_paras_from_blocks();
        let mut app = App::new(doc, false); // editable = false
        apply(&mut app, Action::DeleteChar, 10);
        assert_eq!(app.buf.text_at((0, 0)), "과업명", "읽기전용이면 안 바뀐다");
        assert!(app.message.contains("읽기전용"), "{}", app.message);
    }

    #[test]
    fn toc_mode_rebinds_enter_and_jk() {
        // 본문에서 Enter 는 한 줄 내려가고, 개요에서는 고른 줄로 이동한다.
        assert_eq!(
            action_for_mode(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), false, Mode::Body),
            Action::Lines(1)
        );
        assert_eq!(
            action_for_mode(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), false, Mode::Toc),
            Action::Select
        );
        // 개요에서 q 는 끝내지 않고 본문으로 돌아간다 — 실수로 종료되면 곤란하다
        assert_eq!(action_for_mode(key('q'), false, Mode::Toc), Action::LeaveToc);
        assert_eq!(action_for_mode(key('q'), false, Mode::Body), Action::Quit);
        assert_eq!(action_for_mode(key('t'), false, Mode::Body), Action::ToggleToc);
    }

    #[test]
    fn typing_swallows_motion_keys() {
        // 검색어에 'q' 를 치면 끝나버리면 안 된다.
        assert_eq!(action_for(key('q'), true), Action::InputChar('q'));
        assert_eq!(action_for(key('j'), true), Action::InputChar('j'));
        assert_eq!(
            action_for(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), true),
            Action::InputCancel
        );
    }
    /// **다른 이름으로 저장의 관문들** — 쓰기 전에 걸리는 것.
    ///
    /// 백엔드를 타기 전에 막아야 하는 것이 셋이다: 읽기전용, 경로 없음,
    /// 그리고 **있는 파일**. 마지막이 제일 중요하다 — 이 도구가 남의 문서를
    /// 덮지 않는다는 규칙은 저장 경로에만 있는 것이 아니다.
    #[test]
    fn saving_under_another_name_asks_before_it_overwrites() {
        let mut app = App::new(crate::pager::tests::fixture(), false);

        // 읽기전용이면 거부한다
        run_ex(&mut app, "saveas /tmp/hwp_guard_ro.hwp");
        assert!(app.message.contains("읽기전용"), "{}", app.message);

        app.editable = true;
        // 경로가 없으면 달라고 한다. 조용히 아무것도 안 하면 "저장됐나?" 가 된다
        for cmd in ["saveas", "saveas!", "w "] {
            app.message.clear();
            run_ex(&mut app, cmd);
            assert!(app.message.contains("경로"), "`:{cmd}` → {}", app.message);
        }

        // **있는 파일은 덮지 않고, 덮는 방법을 말한다.**
        let existing = std::env::temp_dir().join("hwp_guard_exists.hwp");
        std::fs::write(&existing, "손대지 말 것").expect("시험 파일");
        let bytes = std::fs::read(&existing).expect("시험 파일");
        for (cmd, forced) in [("w", "w!"), ("saveas", "saveas!")] {
            app.message.clear();
            run_ex(&mut app, &format!("{cmd} {}", existing.display()));
            assert!(app.message.contains("이미 있"), "`:{cmd}` → {}", app.message);
            // 덮는 **그 명령**을 적어 줘야 한다. "덮어쓰기 옵션을 쓰라" 는 쓸모없다
            assert!(app.message.contains(forced), "`:{cmd}` → {}", app.message);
        }
        assert_eq!(bytes, std::fs::read(&existing).expect("시험 파일"), "거부했는데 덮었다");
        let _ = std::fs::remove_file(&existing);
    }

    /// **메뉴에서 고르면 명령줄이 채워져 뜬다.**
    ///
    /// `Action` 에 경로를 실어 보낼 수 없으니 사람에게 묻는다. 메뉴만 보고는
    /// 경로를 받아야 하는 명령이 **없는 것처럼** 보이던 자리다.
    #[test]
    fn the_menu_can_reach_a_command_that_needs_a_path() {
        let groups = menu_groups();
        let file = groups.iter().find(|(n, _)| *n == "파일").expect("파일 묶음");
        let saveas = file
            .1
            .iter()
            .find(|(label, _)| label.starts_with("다른 이름으로"))
            .expect("메뉴에 `다른 이름으로 저장` 이 있어야 한다");
        assert_eq!(saveas.1, Action::Prompt("saveas "));

        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        apply(&mut app, saveas.1.clone(), 20);
        assert_eq!(
            app.input.as_ref().map(|(p, b)| (*p, b.as_str())),
            Some((':', "saveas ")),
            "명령줄이 채워져 뜨지 않았다"
        );
    }

    /// **친 글자가 바로 화면에 나타나는가.**
    ///
    /// 이 버그는 시험이 하나도 없었던 자리다. 화면을 다시 그릴지 *연산
    /// 개수*로 판단했는데, 같은 단락에서 이어 치는 글자는 **한 연산으로
    /// 모이므로** 개수가 안 바뀐다 — 첫 글자만 보이고 그 뒤는 Enter 를 눌러야
    /// 나타났다(2026-10-10 보고, 한글·영문 둘 다).
    ///
    /// `apply` 만 부르는 시험으로는 볼 수 없다. **`step`**(= 대화형 루프와
    /// `--keys` 가 쓰는 한 걸음)을 지나야 한다.
    #[test]
    fn every_typed_character_shows_up_without_waiting_for_enter() {
        use crate::pager::Mode;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        let at = app.buf.cursor.at;
        let k = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);

        step(&mut app, k('i'), 20);
        assert_eq!(app.mode, Mode::Insert);

        // 한 글자씩 치면서 **그릴 줄**에 그 글자가 들어왔는지 본다.
        // `buf.text_at` 이 아니라 `app.rows` 를 보는 것이 요점이다 — 버퍼는
        // 처음부터 맞았고 **화면이 안 따라왔다.**
        for (n, c) in "hello".chars().enumerate() {
            step(&mut app, k(c), 20);
            let want: String = "hello".chars().take(n + 1).collect();
            let drawn = app
                .rows
                .iter()
                .filter_map(|r| match r {
                    crate::pager::Row::Line { section, para, text, .. }
                        if (*section, *para) == (at.0, Some(at.1)) =>
                    {
                        Some(text.clone())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            assert!(
                drawn.contains(&want),
                "{}글자까지 쳤는데 화면에는 {drawn:?} — {want:?} 가 없다",
                n + 1
            );
        }
    }

    /// 두벌식도 같다 — **조합 중인 자모**가 커밋 전에 보여야 한다.
    ///
    /// 조합 중에는 연산이 **아예 생기지 않으므로** 연산 개수로 보면 영원히
    /// 안 그린다. `한` 은 세 키(ㅎㅏㄴ)를 받는 동안 ㅎ → 하 → 한 으로 보여야
    /// 한다.
    #[test]
    fn a_composing_syllable_is_visible_while_it_is_still_composing() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        app.buf.ime = crate::buffer::Ime::Dubeolsik;
        let k = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        step(&mut app, k('i'), 20);

        // g k s = ㅎ ㅏ ㄴ
        for (key, want) in [('g', "ㅎ"), ('k', "하"), ('s', "한")] {
            let before = app.buf.revision();
            step(&mut app, k(key), 20);
            assert_ne!(
                app.buf.revision(),
                before,
                "{key:?} 를 쳤는데 판 번호가 안 올랐다 — 화면을 다시 안 그린다"
            );
            // 조합 중인 글자는 preedit 로 그려진다
            let seen = app.buf.jamo.preedit();
            assert_eq!(seen, want, "{key:?} 뒤의 조합 글자");
        }
    }

    /// **표 칸을 칸 안에서 고칠 수 있는가 — 커서까지.**
    ///
    /// 처음엔 `i` 가 명령줄에 글자를 채워 띄웠다(`:cell …`). 되기는 했지만
    /// 칸을 보면서 고치는 것이 아니었다. 이제 **커서가 칸 안에 선다.**
    ///
    /// 끼워넣기 모드를 그대로 쓴다 — 두벌식 조합·지우기·커서 이동을 베끼지
    /// 않으려면 그래야 한다. 다른 것은 `Esc` 가 본문이 아니라 표로 돌아가는
    /// 것뿐이다.
    #[test]
    fn a_cell_is_edited_in_place_with_the_caret_inside_it() {
        use crate::pager::Mode;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new(fixture_with_table(), false);
        app.editable = true;
        let k = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let named = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);

        app.sync_cells();
        assert!(app.enter_table_index(0), "표로 못 들어갔다");
        step(&mut app, named(KeyCode::Down), 20);
        let at = app.cell_at().expect("칸 좌표");
        let was = app.buf.cell_text(at).expect("칸 글자").to_string();

        // `i` — 칸 **안으로** 들어간다. 명령줄이 뜨면 안 된다.
        step(&mut app, k('i'), 20);
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(app.buf.editing_cell(), Some(at), "칸 고치기가 안 켜졌다");
        assert!(app.input.is_none(), "명령줄이 떴다 — 칸 안에서 고쳐야 한다");
        // 커서는 글자 끝에 선다
        assert_eq!(app.buf.cursor.offset, was.chars().count());

        // 한 글자씩 치면 **격자에** 바로 들어온다
        for (n, c) in "XY".chars().enumerate() {
            step(&mut app, k(c), 20);
            let want: String = format!("{was}{}", "XY".chars().take(n + 1).collect::<String>());
            assert!(
                app.grids.iter().flatten().any(|l| l.contains(&want)),
                "{}글자까지 쳤는데 격자에 {want:?} 가 없다",
                n + 1
            );
        }

        // 지우기와 커서 이동도 같은 길을 쓴다
        step(&mut app, named(KeyCode::Backspace), 20);
        assert_eq!(app.buf.cell_text(at), Some(format!("{was}X").as_str()));
        step(&mut app, named(KeyCode::Left), 20);
        step(&mut app, k('Z'), 20);
        let mid: String = {
            let mut c: Vec<char> = was.chars().collect();
            c.push('Z');
            c.push('X');
            c.into_iter().collect()
        };
        assert_eq!(app.buf.cell_text(at), Some(mid.as_str()), "커서 자리에 안 들어갔다");

        // **한 번의 고치기는 연산 하나다.** 글자마다 쌓으면 저장 때 같은 칸에
        // 열 번 쓰고 되돌리기도 글자 단위가 된다.
        step(&mut app, named(KeyCode::Esc), 20);
        assert_eq!(app.mode, Mode::Table, "Esc 가 본문으로 튀어나갔다");
        assert_eq!(app.buf.editing_cell(), None);
        assert_eq!(app.buf.log().ops().len(), 1, "{:?}", app.buf.log().ops());

        // 되돌리면 한 번에 처음으로
        step(&mut app, k('u'), 20);
        assert_eq!(app.buf.cell_text(at), Some(was.as_str()), "한 번에 안 돌아왔다");
    }

    /// **칸을 고치는 중에 단락을 건드리면 안 된다.**
    ///
    /// 끼워넣기 모드를 그대로 쓰므로 `Enter`(빈 단락 열기)와 `↑↓`(단락 이동)가
    /// 같이 들어온다. 칸을 고치다 Enter 를 누르면 본문에 **빈 단락이 생기는데**,
    /// 사람은 "칸 입력을 끝냈다" 고 생각한 순간이다.
    #[test]
    fn editing_a_cell_never_touches_the_body_paragraphs() {
        use crate::pager::Mode;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new(fixture_with_table(), false);
        app.editable = true;
        let before_paras = app.buf.coords().len();
        let k = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let named = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);

        app.sync_cells();
        app.enter_table_index(0);
        step(&mut app, k('i'), 20);
        step(&mut app, k('X'), 20);

        // Enter 는 **끝내는 것**으로 받는다
        step(&mut app, named(KeyCode::Enter), 20);
        assert_eq!(app.mode, Mode::Table, "Enter 가 칸 고치기를 끝내지 않았다");
        assert_eq!(
            app.buf.coords().len(),
            before_paras,
            "본문 단락이 늘었다 — 칸을 고치다 Enter 를 눌렀을 뿐이다"
        );

        // 세로 이동은 거절하고 **왜인지 말한다**
        step(&mut app, k('i'), 20);
        step(&mut app, named(KeyCode::Down), 20);
        assert!(app.message.contains("Esc"), "{}", app.message);
        assert_eq!(app.buf.editing_cell().is_some(), true, "칸에서 밀려났다");
    }

    /// 두벌식 조합이 **칸 안에서도** 한 키씩 보인다.
    ///
    /// 조합 중에는 칸 글자가 바뀌지 않으므로(확정 전이다) 칸 판 번호가 안
    /// 올라간다 — 그것만 보고 다시 그리면 한글을 칠 때 손이 허공을 짚는다.
    #[test]
    fn a_composing_syllable_shows_inside_the_cell_too() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(fixture_with_table(), false);
        app.editable = true;
        app.buf.ime = crate::buffer::Ime::Dubeolsik;
        let k = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);

        app.sync_cells();
        app.enter_table_index(0);
        let at = app.cell_at().expect("칸");
        let was = app.buf.cell_text(at).expect("글자").to_string();
        step(&mut app, k('i'), 20);

        // g k s = ㅎ ㅏ ㄴ → 격자에 ㅎ · 하 · 한 이 차례로 보여야 한다
        for (key, want) in [('g', "ㅎ"), ('k', "하"), ('s', "한")] {
            step(&mut app, k(key), 20);
            let line = format!("{was}{want}");
            assert!(
                app.grids.iter().flatten().any(|l| l.contains(&line)),
                "{key:?} 뒤 격자에 {line:?} 가 없다"
            );
        }
        // **확정 전 글자는 담아 두지 않는다** — 화면에만 보인다
        assert_eq!(app.buf.cell_text(at), Some(was.as_str()));
    }

    /// **커서가 고치는 글자 위에 서는가.**
    ///
    /// 그리는 쪽(`grid::lay_cell`)과 커서를 놓는 쪽이 따로 세면 접는 자리나
    /// 정렬 여백이 한 칸 어긋나고, 그러면 커서가 글자 사이에 떠서 "어디를
    /// 고치는지" 를 알 수 없다. 그래서 **화면에서 뒤집힌 칸**을 찾아 커서
    /// 자리와 맞는지 본다.
    #[test]
    fn the_caret_stands_on_the_character_it_will_change() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Modifier;

        let mut app = App::new(fixture_with_table(), false);
        app.editable = true;
        let k = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        app.sync_cells();
        app.enter_table_index(0);
        step(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), 20);
        step(&mut app, k('i'), 20);

        // 커서를 글자 안쪽으로 옮긴다 — 끝이 아니라 중간이어야 의미가 있다
        for _ in 0..2 {
            step(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE), 20);
        }

        let mut term = Terminal::new(TestBackend::new(60, 20)).expect("백엔드");
        term.draw(|f| crate::pager::draw(f, &app)).expect("그리기");
        let buf = term.backend().buffer().clone();

        // **뒤집힌 칸이 딱 하나여야 한다.** 칸 전체가 뒤집혀 있으면 어디를
        // 고치는지 알 수 없다 — 고치기 전의 모양이 그랬다.
        let mut flipped: Vec<(u16, u16, String)> = Vec::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let c = &buf[(x, y)];
                if c.style().add_modifier.contains(Modifier::REVERSED)
                    && !c.symbol().trim().is_empty()
                {
                    flipped.push((x, y, c.symbol().to_string()));
                }
            }
        }
        // 2칸 글자는 뒤 칸을 함께 차지하므로 1~2칸이 뒤집힌다
        assert!(
            (1..=2).contains(&flipped.len()),
            "뒤집힌 칸이 {}개다 — 커서가 글자 하나를 짚어야 한다: {flipped:?}",
            flipped.len()
        );

        // 그 글자가 **정말 커서가 가리키는 글자**인가
        let at = app.buf.editing_cell().expect("칸 고치기 중");
        let text: Vec<char> = app.buf.cell_display(at).chars().collect();
        let want = text
            .get(app.buf.cell_cursor_offset())
            .copied()
            .expect("커서가 글자 위에 있어야 한다");
        assert_eq!(
            flipped[0].2,
            want.to_string(),
            "화면이 뒤집은 글자와 커서가 가리키는 글자가 다르다"
        );
    }

    /// 표 하나를 가진 문서 — 칸 편집 시험이 쓴다.
    fn fixture_with_table() -> hwp_core::ScreenDoc {
        let mut doc = crate::pager::tests::fixture();
        let cell = |row: usize, col: usize, text: &str| hwp_core::grid::Cell {
            row,
            col,
            row_span: 1,
            col_span: 1,
            text: text.to_string(),
            art: Vec::new(),
            align: Default::default(),
            min_width: 0, doc_width: 0
        };
        doc.tables.push(hwp_core::screen::PlacedTable {
            at: (0, 0),
            control: 0,
            index: 0,
            table: hwp_core::grid::Table {
                rows: 2,
                cols: 2,
                cells: vec![
                    cell(0, 0, "항목"),
                    cell(0, 1, "값"),
                    cell(1, 0, "사업명"),
                    cell(1, 1, "비어 있다"),
                ],
            },
        });
        doc
    }

    /// **기호를 골라 넣는다** — 터미널 IME 에 없는 글자.
    ///
    /// 공문서는 `○ □ ※ ①` 로 계층을 만드는데 두벌식 오토마톤은 한글만
    /// 만든다. HWP 는 이 일을 문자표(`Ctrl-F10`)로 하고, 여기서는 끼워넣기
    /// 모드의 `Ctrl-K` 다 — vi 가 digraph(특수문자)를 받는 자리이고, 보통
    /// 모드의 `Ctrl-K`(워드스타 블록 묶음)와 겹치지 않는다.
    #[test]
    fn a_symbol_is_picked_from_the_table_and_inserted_at_the_cursor() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        let k = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let named = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);

        step(&mut app, k('i'), 20);
        let at = app.buf.cursor.at;
        let was = app.buf.text_at(at).to_string();

        // `Ctrl-K` 가 표를 띄운다. **글자 `k` 가 들어가면 안 된다.**
        step(&mut app, KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL), 20);
        assert!(app.picker.is_some(), "기호표가 안 열렸다");
        assert_eq!(app.buf.text_at(at), was, "`k` 가 글자로 들어갔다");

        // 첫 묶음 첫 기호는 **코퍼스가 가장 많이 쓰는 글머리**다
        assert_eq!(app.picker_pick(), Some(("○".to_string(), 0)));
        step(&mut app, named(KeyCode::Enter), 20);
        assert!(app.picker.is_none(), "넣고도 표가 열려 있다");
        assert!(app.buf.text_at(at).starts_with('○'), "{:?}", app.buf.text_at(at));

        // 격자를 걷는다 — 좌우는 한 칸, 위아래는 한 줄, Tab 은 묶음
        step(&mut app, KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL), 20);
        step(&mut app, named(KeyCode::Right), 20);
        assert_eq!(app.picker_pick().map(|(c, _)| c), Some("□".to_string()));
        step(&mut app, named(KeyCode::Down), 20);
        let after_row = app.picker_pick().map(|(c, _)| c).expect("기호");
        assert_ne!(after_row, "□", "아래로 안 갔다");
        step(&mut app, named(KeyCode::Tab), 20);
        assert_eq!(app.picker_pick().map(|(c, _)| c), Some("①".to_string()), "묶음이 안 바뀌었다");

        // Esc 로 닫으면 **아무것도 넣지 않는다**
        let before = app.buf.text_at(at).to_string();
        step(&mut app, named(KeyCode::Esc), 20);
        assert!(app.picker.is_none());
        assert_eq!(app.buf.text_at(at), before, "닫았는데 글자가 들어갔다");
    }

    /// 표 칸 안에서도 같은 길이다 — 칸 고치기가 끼워넣기 모드를 그대로 쓴다.
    #[test]
    fn symbols_go_into_a_table_cell_too() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(fixture_with_table(), false);
        app.editable = true;
        app.sync_cells();
        app.enter_table_index(0);
        let at = app.cell_at().expect("칸");
        let was = app.buf.cell_text(at).expect("글자").to_string();

        step(&mut app, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE), 20);
        step(&mut app, KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL), 20);
        step(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 20);
        assert_eq!(app.buf.cell_text(at), Some(format!("{was}○").as_str()));
        // 격자에도 들어왔는가
        assert!(
            app.grids.iter().flatten().any(|l| l.contains(&format!("{was}○"))),
            "격자에 기호가 없다"
        );
    }

    /// **읽기전용에서는 열지 않는다.** 열어 놓고 넣을 때 거절하면 고른 수고가 버려진다.
    #[test]
    fn a_read_only_document_does_not_open_the_symbol_table() {
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = false;
        apply(&mut app, Action::OpenSymbols, 20);
        assert!(app.picker.is_none(), "읽기전용인데 열렸다");
        assert!(app.message.contains("읽기전용"), "{}", app.message);
    }

    /// **한자 변환은 앞의 음절을 바꾼다 — 더하는 것이 아니다.**
    ///
    /// 더하면 `과果` 가 되고, 그건 아무도 원한 적이 없다. 기호 넣기와 같은
    /// 상자를 쓰면서 이 하나가 다르다(`Picker::replace`).
    #[test]
    fn converting_to_hanja_replaces_the_syllable_instead_of_adding_to_it() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        let k = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let named = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);

        step(&mut app, k('i'), 20);
        let at = app.buf.cursor.at;
        let was = app.buf.text_at(at).to_string();
        // 두벌식으로 `과` = r h k
        app.buf.ime = crate::buffer::Ime::Dubeolsik;
        for c in "rhk".chars() {
            step(&mut app, k(c), 20);
        }

        step(&mut app, named(KeyCode::F(9)), 20);
        let p = app.picker.as_ref().expect("한자표가 안 열렸다");
        assert_eq!(p.replace, 1, "음절을 바꾸지 않고 더하려 한다");
        assert_eq!(p.groups.len(), 1, "한자표에는 묶음이 하나다");
        // 첫 후보는 흔한 글자 — 가나다순이 아니다
        assert_eq!(app.picker_pick().map(|(c, _)| c), Some("果".to_string()));

        step(&mut app, named(KeyCode::Enter), 20);
        assert_eq!(
            app.buf.text_at(at),
            format!("果{was}"),
            "음절이 남아 있다 — 바꾸지 않고 더했다"
        );
    }

    /// **커서 앞이 한글 음절이 아니면 열지 않고 왜인지 말한다.**
    ///
    /// 빈 상자를 띄우면 "표가 비었나" 로 읽힌다. 조합 중인 자모(`ㅘ`)도
    /// 음절이 아니다 — 확정되기 전에는 글자가 아니다.
    #[test]
    fn asking_for_hanja_where_there_is_no_syllable_says_why() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        let named = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);

        // 단락 맨 앞 — 앞에 글자가 없다
        step(&mut app, named(KeyCode::Char('i')), 20);
        app.buf.cursor.offset = 0;
        step(&mut app, named(KeyCode::F(9)), 20);
        assert!(app.picker.is_none(), "앞에 글자가 없는데 열렸다");
        assert!(app.message.contains("글자가 없"), "{}", app.message);

        // 한자 다음에서 — 바꿀 것이 없다
        app.buf.insert_str("漢");
        step(&mut app, named(KeyCode::F(9)), 20);
        assert!(app.picker.is_none(), "한자 뒤인데 열렸다");
        assert!(app.message.contains("한글 음절"), "{}", app.message);
    }

    /// 표 칸 안에서도 한자로 바꾼다 — 칸 고치기가 끼워넣기 모드를 그대로 쓴다.
    #[test]
    fn hanja_works_inside_a_table_cell() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(fixture_with_table(), false);
        app.editable = true;
        app.sync_cells();
        app.enter_table_index(0);
        let at = app.cell_at().expect("칸");
        let was = app.buf.cell_text(at).expect("글자").to_string();

        step(&mut app, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE), 20);
        app.buf.ime = crate::buffer::Ime::Dubeolsik;
        for c in "rhk".chars() {
            step(&mut app, KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), 20);
        }
        step(&mut app, KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE), 20);
        assert!(app.picker.is_some(), "칸 안에서 한자표가 안 열렸다");
        step(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 20);
        assert_eq!(app.buf.cell_text(at), Some(format!("{was}果").as_str()));
    }

    /// **후보가 백 개를 넘으면 넘긴다.** 상자가 화면을 다 덮으면 무엇에 대고
    /// 하는 일인지 보이지 않는다.
    #[test]
    fn a_long_candidate_list_scrolls_instead_of_filling_the_screen() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(crate::pager::tests::fixture(), false);
        app.editable = true;
        step(&mut app, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE), 20);
        app.buf.insert_str("과");
        step(&mut app, KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE), 20);

        let n = app.picker.as_ref().map(|p| p.items[0].len()).expect("후보");
        assert!(n > 100, "`과` 의 후보가 {n}개뿐이다");
        assert_eq!(app.picker.as_ref().map(|p| p.top), Some(0));

        // 아래로 계속 내리면 **보이는 창이 따라온다**
        for _ in 0..12 {
            step(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), 20);
        }
        let p = app.picker.as_ref().expect("후보");
        let row = p.item / App::PICK_PER_ROW;
        assert!(
            (p.top..p.top + App::PICK_ROWS).contains(&row),
            "고른 줄({row})이 보이는 창({}..{}) 밖이다",
            p.top,
            p.top + App::PICK_ROWS
        );
        // 끝에서 멈춘다
        for _ in 0..200 {
            step(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), 20);
        }
        assert_eq!(app.picker.as_ref().map(|p| p.item), Some(n - 1), "끝을 넘었다");
    }
}

/// 메뉴에 올리는 것 — **이름과 그 이름이 내는 동작.**
///
/// 메뉴가 곧 [`Action`] 을 내므로 키로 하던 길과 **한 길**이다. 메뉴만의
/// 처리를 따로 쓰면 둘이 갈라지고, 메뉴로 한 일은 시험되지 않는다.
pub type MenuItem = (&'static str, Action);
pub type MenuGroup = (&'static str, &'static [MenuItem]);

/// 메뉴 묶음. 화살표 ←→ 로 묶음을, ↑↓ 로 항목을 고른다.
///
/// 키를 외우지 않고도 **무엇을 할 수 있는지 보고 고르게** 하는 자리다.
/// 도움 보기(`?`)는 "키가 무엇인가" 를 말하고, 메뉴는 "지금 뭘 할까" 를 받는다.
pub fn menu_groups() -> &'static [MenuGroup] {
    &[
        (
            "파일",
            &[
                ("저장", Action::Save),
                ("다른 이름으로 저장…", Action::Prompt("saveas ")),
                ("사본 쓰기 (원본을 계속 고친다)…", Action::Prompt("w ")),
                ("저장하고 나가기", Action::SaveQuit),
                ("디스크에서 다시 읽기 (편집 버림)", Action::Reload),
                ("나가기", Action::Quit),
                ("버리고 나가기", Action::ForceQuit),
            ],
        ),
        (
            "편집",
            &[
                ("되돌리기", Action::Undo),
                ("다시하기", Action::Redo),
                ("단락 담기", Action::Operator('y')),
                ("붙이기 (아래)", Action::Paste { before: false }),
                ("붙이기 (위)", Action::Paste { before: true }),
                // 메뉴 항목도 우리 장식이다 — 기호를 보기로 적지 않는다
                ("기호 넣기 (글머리·번호·화살표…)", Action::OpenSymbols),
                ("한자로 바꾸기 (커서 앞 음절)", Action::OpenHanja),
                ("표 만들기…", Action::Prompt("table 3x4")),
                ("빈 단락 넣기 (아래)", Action::OpenParagraph { before: false }),
                ("빈 단락 넣기 (위)", Action::OpenParagraph { before: true }),
                ("글머리 단계 들이기", Action::Relevel(1)),
                ("글머리 단계 내오기", Action::Relevel(-1)),
            ],
        ),
        (
            "이동",
            &[
                ("문서 처음", Action::Top),
                ("문서 끝", Action::Bottom),
                ("다음 쪽", Action::Page(true)),
                ("앞 쪽", Action::Page(false)),
                ("개요 보기", Action::ToggleToc),
                ("표 안으로", Action::EnterTable),
                ("찾기", Action::BeginInput('/')),
            ],
        ),
        (
            "보기",
            &[
                ("그림 보기 방식 바꾸기", Action::CycleImages),
                ("문서 단 폭 / 터미널 폭", Action::ToggleWidth),
                ("도움 (전체 키)", Action::ToggleHelp),
            ],
        ),
    ]

}

#[cfg(test)]
mod quit_audit {
    use super::*;
    use crate::pager::App;

    fn text_app(body: &str) -> (App, std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "hwp-cmd-zz-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("a.txt");
        std::fs::write(&path, body).unwrap();
        let text = hwp_core::text::TextFile::read(&path).unwrap();
        let mut app = App::new(text.to_screen_doc(40, 50), true);
        app.editable = true;
        (app, path, dir)
    }

    /// **`ZZ` 는 저장이 실패하면 나가지 않는다 — 문구와 무관하게.**
    ///
    /// 감사(2026-10-13)에서 나온 것: `save_and_quit` 이 저장 성공 여부를
    /// **상태줄 문구에 "멈췄" 가 들어 있는지**로 판단하고 있었다. 저장 성공을
    /// 사람에게 보여 줄 말에 매달아 둔 것이고, 그래서 말투를 고치면 조용히
    /// 깨진다 — 실제로 이 저장소에서 "저장을 멈췄다" 를 "저장을 멈췄습니다"
    /// 로 일괄 고친 적이 있고 어간이 우연히 같아서 살았다.
    ///
    /// 그래서 이 시험은 **문구를 보지 않는다.** 저장이 실패했으면 나가지
    /// 않는다는 것만 요구한다.
    #[test]
    fn zz_does_not_quit_when_the_save_failed() {
        let (mut app, path, dir) = text_app("가나다\n");
        app.buf.insert_str("X");
        assert!(app.buf.is_dirty(), "편집이 쌓여 있어야 한다");

        std::fs::remove_file(&path).unwrap(); // 저장이 실패하게 만든다
        save_and_quit(&mut app);

        assert!(
            !app.quit,
            "저장이 실패했는데 나갔다 — 편집 {}건이 사라진다. 상태줄: {}",
            app.buf.pending_edits(),
            app.message
        );
        assert!(app.buf.is_dirty(), "실패했으면 편집이 남아 있어야 한다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 저장이 되면 나간다 — 위 시험이 "항상 안 나간다" 로 통과하지 않게.
    #[test]
    fn zz_quits_when_the_save_went_through() {
        let (mut app, path, dir) = text_app("가나다\n");
        app.buf.insert_str("X");
        save_and_quit(&mut app);
        assert!(app.quit, "저장이 됐는데 안 나갔다. 상태줄: {}", app.message);
        assert!(!app.buf.is_dirty(), "저장 뒤에는 깨끗해야 한다");
        assert!(
            std::fs::read_to_string(&path).unwrap().starts_with('X'),
            "파일에 안 들어갔다"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **성공 여부가 문구에서 오지 않는다는 것 자체를 못 박는다.**
    ///
    /// 저장이 실패한 뒤 상태줄을 "멈췄" 가 없는 말로 **바꿔 놓아도** 판단이
    /// 달라지지 않아야 한다. 전에는 이 조작만으로 `ZZ` 가 나가 버렸다.
    #[test]
    fn the_quit_decision_does_not_read_the_status_line() {
        let (mut app, path, dir) = text_app("가나다\n");
        app.buf.insert_str("X");
        std::fs::remove_file(&path).unwrap();

        let ok = write_file(&mut app);
        assert!(!ok, "실패를 실패라고 돌려줘야 한다");
        // 문구에서 단서를 지운다
        app.message = "무슨 일이 있었는지 아무 말도 하지 않는 문구".into();
        if write_file(&mut app) {
            app.quit = true;
        }
        assert!(!app.quit, "문구를 바꿨다고 판단이 달라졌다");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
