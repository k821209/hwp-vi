//! 샘플 코퍼스 회귀 — R0 의 본체.
//!
//! PLAN.md §8-5 와 §7 의 교훈: ★ 수와 README 를 믿지 말고 **코퍼스로 재라.**
//! 백엔드를 고르며 시험한 판독기 중에는 README 상으로는 멀쩡한데 실제로는
//! 14개 중 3개만 읽히는 것이 있었다. 그래서 백엔드 핀을 올릴 때 통과해야
//! 하는 관문을 코드로 박아 둔다 — 남의 것을 평하려는 게 아니라 **우리가
//! 핀을 올릴 때** 같은 일을 당하지 않으려는 것이다.
//!
//! 샘플은 실명이 든 실무 문서라 저장소에 올리지 않는다(`.gitignore`). 없으면
//! 테스트는 **건너뛴다** — 조용히 통과시키지는 않고, 건너뛴다고 출력한다.

use hwp_core::witness::Testimony;
use hwp_core::{Pin, Rhwp};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("워크스페이스 루트")
        .to_path_buf()
}

/// 이 코퍼스가 **실무 문서**인가, 우리가 만든 합성 문서인가.
///
/// `samples/MANIFEST.json` 의 `kind` 가 말한다. 없으면 실무 문서로 본다 —
/// 선언하지 않은 코퍼스를 "약하다" 고 가정하면 진짜 관문이 조용히 꺼진다.
fn corpus_is_synthetic() -> bool {
    let m = workspace_root().join("samples/MANIFEST.json");
    let Ok(s) = std::fs::read_to_string(m) else { return false };
    s.contains("\"kind\": \"synthetic\"") || s.contains("\"kind\":\"synthetic\"")
}

/// **실무 문서가 있어야 뜻이 있는 시험들.** 이 목록이 유일한 진실이다.
///
/// 이 저장소의 코퍼스 시험 다수는 전제에 "비었으면 거부한다" 를 박아 두었다
/// (`기호를 5개만 세었다 — 시험이 비었다`, `구역이 여럿인 문서가 코퍼스에
/// 없다`). 그 관문은 **약하게 만들면 안 된다** — 공허한 시험이 통과하는 것을
/// 막는 장치이고, 이 저장소가 그 함정에 세 번 빠졌다.
///
/// 그래서 합성 코퍼스에서는 끄되, **끈 것을 목록으로 선언한다.** 수가
/// `samples/MANIFEST.json` 의 `inactive_guards` 와 다르면 아래 시험이 깨진다.
///
/// 처음에는 각 시험이 임시 파일에 이름을 적고 선언 시험이 그걸 세게 만들었다.
/// **틀린 설계였다** — 시험 하나만 돌리면(`cargo test <이름>`) 로그가 비어서
/// 선언 시험이 거짓으로 깨졌다. 시험이 다른 시험의 실행 순서에 매달리면 안 된다.
const REAL_CORPUS_ONLY: &[&str] = &[
    "a_drawn_table_keeps_the_column_proportions_of_the_document",
    "character_decoration_agrees_across_the_two_formats",
    "the_two_formats_read_the_same_text_including_forced_line_breaks",
    "column_width_is_measured_for_real_documents",
    "decoration_runs_stay_inside_their_paragraph",
    "deleting_a_paragraph_that_holds_a_drawing_is_refused_before_anything_is_written",
    "edit_preserves_structure_and_changes_only_the_target",
    "every_document_answers_every_read_name",
    "every_drawing_object_is_counted_on_the_paragraph_that_holds_it",
    "every_witness_coordinate_is_a_real_edit_coordinate",
    "hanging_indents_are_read_and_are_the_common_case",
    "image_art_never_uses_a_glyph_wider_than_one_cell",
    "images_inside_table_cells_land_on_a_real_cell",
    "in_a_wide_terminal_our_own_chrome_never_shifts_a_line",
    "indents_fit_inside_the_column",
    "only_center_and_right_alignment_is_carried",
    "placed_width_maps_into_the_column_and_actually_varies",
    "tables_line_up_under_both_width_policies",
    "the_cell_widths_we_read_add_up_to_what_the_typesetter_measured",
    "the_symbol_table_covers_what_the_corpus_actually_uses",
    "witness_first_indent_agrees_with_the_typesetter",
];

/// 이 시험이 지금 코퍼스에서 뜻이 있는가.
fn needs_real_corpus(test: &str) -> bool {
    // 목록에 없는 이름으로 부르면 오타다 — 조용히 통과시키지 않는다.
    assert!(
        REAL_CORPUS_ONLY.contains(&test),
        "`{test}` 가 REAL_CORPUS_ONLY 에 없다 — 이름을 적었는지 확인하라"
    );
    !corpus_is_synthetic()
}

fn samples() -> Vec<PathBuf> {
    let dir = workspace_root().join("samples");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|s| s.to_str()),
                Some("hwp") | Some("hwpx")
            )
        })
        .collect();
    v.sort();
    v
}

/// 백엔드를 열거나, 없으면 건너뛰라고 알린다.
fn backend() -> Option<(Rhwp, Pin)> {
    let pin = Pin::load_default().expect("핀 파일은 소스에 박혀 있다");
    match Rhwp::discover(&pin) {
        Ok(r) => Some((r, pin)),
        Err(e) => {
            eprintln!("건너뜀 — 백엔드를 쓸 수 없다:\n{e}");
            None
        }
    }
}

#[test]
fn gate_accepts_the_pinned_backend() {
    let Some((rhwp, pin)) = backend() else { return };
    let caps = rhwp.capabilities();
    assert_eq!(caps.tool, "rhwp");
    assert_eq!(caps.version, pin.version);
    for name in &pin.required_commands {
        let c = caps
            .command(name)
            .unwrap_or_else(|| panic!("필수 명령 {name} 이 capabilities 에 없다"));
        assert!(c.json, "{name} 이 --json 을 내지 않는다");
    }
}

/// 코퍼스 전수 읽기. 백엔드 후보를 고를 때 갈라낸 것이 이 시험이다.
#[test]
fn every_sample_reads() {
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        eprintln!("건너뜀 — samples/ 가 비었다");
        return;
    }

    let mut failures = Vec::new();
    for f in &files {
        match rhwp.info(f) {
            Ok(info) => {
                if info.page_count == 0 {
                    failures.push(format!("{}: 쪽 수가 0", f.display()));
                }
            }
            Err(e) => failures.push(format!("{}: {e}", f.display())),
        }
    }
    assert!(
        failures.is_empty(),
        "{}/{} 개 샘플을 읽지 못했다:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}

/// 조판값이 온다 — R1 페이저가 화면을 그릴 재료.
#[test]
fn layout_is_available_for_every_sample() {
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }
    for f in &files {
        let pages = rhwp
            .dump_pages(f)
            .unwrap_or_else(|e| panic!("{} 의 조판값을 못 받았다: {e}", f.display()));
        assert_eq!(
            pages.pages.len(),
            pages.page_count as usize,
            "{}: pageCount 와 pages 길이가 다르다",
            f.display()
        );
        assert!(
            pages.pages.iter().all(|p| p.body_area.width > 0.0),
            "{}: 본문 영역 폭이 0 인 쪽이 있다",
            f.display()
        );
    }
}

/// **독립 증인이 백엔드와 같은 좌표를 센다.**
///
/// 이것이 §8-2 의 근거다. 깊이 0 단락의 순번이 rhwp 의 `paraIndex` 이고, 그
/// 좌표 위에 TUI 커서와 `hwp json` 이 함께 올라간다. 어긋나면 둘 다 무너진다.
#[test]
fn witness_agrees_with_backend_on_paragraph_count() {
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut mismatches = Vec::new();
    for f in &files {
        let info = rhwp.info(f).expect("info");
        let t = Testimony::read(f).expect("증인");
        if t.body_count() != info.para_count as usize {
            mismatches.push(format!(
                "{}: 증인 {} vs rhwp {}",
                f.display(),
                t.body_count(),
                info.para_count
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "증인과 백엔드의 단락 좌표가 어긋난다:\n{}",
        mismatches.join("\n")
    );
}

/// 편집 왕복 — 백엔드가 쓴 결과를 **백엔드 아닌 코드**로 검증한다.
#[test]
fn edit_preserves_structure_and_changes_only_the_target() {
    if !needs_real_corpus("edit_preserves_structure_and_changes_only_the_target") {
        return;
    }
    let Some((rhwp, _)) = backend() else { return };
    let Some(src) = samples()
        .into_iter()
        .find(|p| p.extension().and_then(|s| s.to_str()) == Some("hwp"))
    else {
        eprintln!("건너뜀 — .hwp 샘플이 없다");
        return;
    };

    let tmp = std::env::temp_dir().join("hwp-cmd-corpus-edit");
    std::fs::create_dir_all(&tmp).expect("임시 디렉터리");
    let input = tmp.join("in.hwp");
    let output = tmp.join("out.hwp");
    std::fs::copy(&src, &input).expect("샘플 복사");
    let _ = std::fs::remove_file(&output);

    let before = Testimony::read(&input).expect("편집 전 증인");

    const MARKER: &str = "〔증인〕";
    rhwp.run(&[
        "edit",
        "insert-text",
        &input.to_string_lossy(),
        "--section",
        "0",
        "--para",
        "0",
        "--offset",
        "0",
        "--text",
        MARKER,
        "-o",
        &output.to_string_lossy(),
    ])
    .expect("insert-text");

    let after = Testimony::read(&output).expect("편집 후 증인");
    let d = before.diff(&after);

    assert!(
        d.structure_preserved(),
        "구조가 보존되지 않았다 — 부품 추가 {:?} / 제거 {:?} / 단락 {} → {}",
        d.parts_added,
        d.parts_removed,
        d.paragraph_count_before,
        d.paragraph_count_after
    );
    assert_eq!(
        d.paragraphs_changed.len(),
        1,
        "단락 하나만 바뀌어야 한다. 바뀐 것: {:?}",
        d.paragraphs_changed
    );
    let (idx, before_text, now) = &d.paragraphs_changed[0];
    assert_eq!(*idx, 0, "0번 단락이 바뀌어야 한다");

    // 실측 사실: `--offset 0` 은 단락 **맨 앞**이 아니라 선두 인라인 컨트롤
    // 뒤에 넣는다. s00.hwp 의 0번 단락은 확장 제어문자 3개로 시작하고
    // (증인은 U+FFFC 로 표시한다) 표시는 그 뒤에 들어갔다.
    //
    // R2 에서 커서를 매핑할 때 이것이 걸린다 — 화면상 n번째 글자가 rhwp 의
    // offset n 이 아니다. 컨트롤을 세는 보정이 필요하다.
    let leading_controls = now.chars().take_while(|c| *c == '\u{FFFC}').count();
    let body: String = now.chars().skip(leading_controls).collect();
    assert!(
        body.starts_with(MARKER),
        "선두 컨트롤 {leading_controls}개 뒤에 표시가 와야 한다: {now:?}"
    );
    assert_eq!(
        before_text.chars().take_while(|c| *c == '\u{FFFC}').count(),
        leading_controls,
        "편집이 선두 컨트롤 수를 바꿨다"
    );
}

/// 화면 모델이 본문을 **잃지도 중복하지도** 않는가.
///
/// R1 페이저의 유일한 정확성 조건이다. 접히는 위치는 우리 계산이고 쪽 경계는
/// rhwp 값이므로, 둘을 합치는 과정에서 줄이 새거나 겹칠 수 있다. 공백을 뺀
/// 글자열이 원문과 같아야 한다.
#[test]
fn screen_model_loses_no_text_and_duplicates_none() {
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut failures = Vec::new();
    for f in &files {
        let doc = match hwp_core::ScreenDoc::build(&rhwp, f) {
            Ok(d) => d,
            Err(e) => {
                failures.push(format!("{}: 화면 모델을 세우지 못했다: {e}", f.display()));
                continue;
            }
        };
        let source = Testimony::read(f).expect("증인");
        // rhwp 좌표 (구역, 단락) 로 원문을 색인한다.
        let mut originals: BTreeMap<(u32, u32), String> = BTreeMap::new();
        for p in source.body_paragraphs() {
            if let Some(i) = p.index {
                originals.insert(
                    (p.section, i),
                    p.text.chars().filter(|c| !c.is_whitespace()).collect(),
                );
            }
        }

        // 쪽을 가로질러 단락별로 모은다.
        let mut rebuilt: BTreeMap<(u32, u32), String> = BTreeMap::new();
        for page in &doc.pages {
            for b in &page.blocks {
                let Some(pi) = b.para_index else { continue };
                let entry = rebuilt.entry((b.section, pi)).or_default();
                for line in &b.lines {
                    entry.extend(line.chars().filter(|c| !c.is_whitespace()));
                }
            }
        }

        for (key, want) in &originals {
            let got = rebuilt.get(key).cloned().unwrap_or_default();
            if *want != got {
                failures.push(format!(
                    "{} 구역{} 단락{}: 원문 {}자 → 화면 {}자\n    원문: {:.60}\n    화면: {:.60}",
                    f.display(),
                    key.0,
                    key.1,
                    want.chars().count(),
                    got.chars().count(),
                    want,
                    got
                ));
                break; // 파일당 첫 불일치만
            }
        }
        for key in rebuilt.keys() {
            if !originals.contains_key(key) {
                failures.push(format!(
                    "{}: 화면에 원문 없는 좌표가 있다 — 구역{} 단락{}",
                    f.display(),
                    key.0,
                    key.1
                ));
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "화면 모델이 본문을 보존하지 못했다:\n{}",
        failures.join("\n")
    );
}

/// 단 폭이 실제로 **측정**되는가 — 기본값으로 떨어지지 않는가.
#[test]
fn column_width_is_measured_for_real_documents() {
    if !needs_real_corpus("column_width_is_measured_for_real_documents") {
        return;
    }
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }
    let mut guessed = Vec::new();
    for f in &files {
        let Ok(doc) = hwp_core::ScreenDoc::build(&rhwp, f) else { continue };
        if !doc.column.measured.is_some_and(|m| m.is_measured()) {
            guessed.push(format!("{}: {}", f.display(), doc.column.provenance()));
        }
    }
    // 짧은 문서(여러줄 단락이 없는 서식 양식 등)는 잴 근거가 없다. 그건 정상이다.
    // 다만 절반 이상이 기본값이면 추론 방식이 잘못된 것이다.
    assert!(
        guessed.len() * 2 <= files.len(),
        "{}/{} 개 문서에서 단 폭을 못 재었다 — 추론이 약하다:\n{}",
        guessed.len(),
        files.len(),
        guessed.join("\n")
    );
}

/// `hwp json` 이 본문 단락과 표를 **하나도 빠뜨리지 않는가.**
///
/// LLM 에 넣는 출구이므로 조용히 빠지는 것이 가장 위험하다 — 모델은 없는 내용을
/// 두고 "문서에 그런 말이 없다"고 답할 것이고, 그 답은 그럴듯하게 들린다.
/// (실측한 실패 모양: 82KB 문서에서 23자만 뽑고 **성공으로 보고**하는 것.
/// 읽다 실패하면 받는 쪽이 아는데, 적게 읽고 ok 라고 하면 모른다.)
///
/// **이 시험은 한동안 아무것도 보지 않았다.** 전제에 `!t.trim().is_empty()` 가
/// 들어 있어서 양쪽에서 빈 단락을 똑같이 걸러냈고, 그래서 "빈 단락이 빠진다" 를
/// 구조적으로 잡을 수 없었다 — 요구가 아니라 **그때의 동작**을 적어 둔 것이다.
/// 2026-10-10 가로세로 대조에서 s16.hwp 의 본문 13단락이 JSON 에 하나도 없다는
/// 것이 드러났고(13개가 다 표·그림만 든 빈 단락이다), 이 시험은 통과하고 있었다.
/// 지금은 **좌표를 전부** 요구한다.
#[test]
fn json_keeps_every_paragraph_and_table() {
    use hwp_core::json::{Profile, build};
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut failures = Vec::new();
    for f in &files {
        let doc = match build(&rhwp, f, Profile::Doc) {
            Ok(d) => d,
            Err(e) => {
                failures.push(format!("{}: {e}", f.display()));
                continue;
            }
        };
        let body = doc.body.as_ref().expect("doc 프로파일에는 body 가 있다");
        let tables = doc.tables.as_ref().expect("doc 프로파일에는 tables 가 있다");

        // **모든** 본문 단락이 body 에 있어야 한다 — 빈 단락도.
        //
        // 빈 단락의 좌표는 "여기에 줄을 넣어라" 의 주소다. 숨기면 읽는 쪽이
        // 고칠 자리를 고를 수 없다. 사람은 화면 왼쪽 여백에서 그 번호를 본다.
        let witness = Testimony::read(f).expect("증인");
        let want: BTreeMap<(u32, u32), String> = witness
            .body_paragraphs()
            .into_iter()
            .filter_map(|p| p.index.map(|i| ((p.section, i), p.text.replace('\u{FFFC}', ""))))
            .collect();

        let got: BTreeMap<(u32, u32), String> = body
            .iter()
            .filter_map(|b| match b {
                hwp_core::json::BodyItem::Para { section, para, text } => {
                    Some(((*section, *para), text.clone()))
                }
                _ => None,
            })
            .collect();

        if want.len() != got.len() {
            let missing: Vec<String> = want
                .keys()
                .filter(|k| !got.contains_key(k))
                .map(|k| format!("{}:{}", k.0, k.1))
                .take(12)
                .collect();
            failures.push(format!(
                "{}: 본문 단락 {}개인데 JSON 에 {}개 — 빠진 좌표 {:?}",
                f.display(),
                want.len(),
                got.len(),
                missing
            ));
            continue;
        }
        for (k, v) in &want {
            // 글자는 보이는 것끼리 비교한다 — 컨트롤(`\u{FFFC}`)은 양쪽에서 뺀다.
            //
            // **`trim()` 하지 않는다.** 전에는 양쪽을 trim 해서 비교했는데,
            // 공문서 단락은 `" ○ …"` 처럼 앞에 공백이 있고(p.hwpx 42단락 중
            // 10개) 그 공백이 **오프셋의 일부**다. trim 하고 비교하면 출구가
            // 앞뒤 공백을 떼기 시작해도 이 시험이 통과한다 — 그러면 LLM 이
            // 센 오프셋이 밀리고, 접힌 줄 결함과 같은 모양이 기계 쪽에서
            // 되풀이된다. 지금은 떼지 않는 것을 요구한다.
            if got.get(k) != Some(v) {
                failures.push(format!(
                    "{}: 구역{} 단락{} 의 본문이 다르다 — 기대 {:?} / 실제 {:?}",
                    f.display(),
                    k.0,
                    k.1,
                    v.chars().take(40).collect::<String>(),
                    got.get(k).map(|g| g.chars().take(40).collect::<String>())
                ));
                break;
            }
        }

        // 모든 표가 body 에서 한 번씩 참조되어야 한다.
        let refs: Vec<&String> = body
            .iter()
            .filter_map(|b| match b {
                hwp_core::json::BodyItem::Table { reference, .. } => Some(reference),
                _ => None,
            })
            .collect();
        if refs.len() != tables.len() {
            failures.push(format!(
                "{}: 표 {}개인데 본문 참조는 {}개 — 표가 어디 있는지 알 수 없다",
                f.display(),
                tables.len(),
                refs.len()
            ));
        }
        for t in tables {
            if !refs.iter().any(|r| **r == t.id) {
                failures.push(format!("{}: 표 {} 가 본문에서 참조되지 않는다", f.display(), t.id));
                break;
            }
            // 격자가 선언된 크기를 담아야 한다
            if t.grid.len() != t.rows || t.grid.iter().any(|r| r.len() != t.cols) {
                failures.push(format!(
                    "{}: 표 {} 격자가 {}×{} 선언과 다르다",
                    f.display(),
                    t.id,
                    t.rows,
                    t.cols
                ));
                break;
            }
        }
    }
    assert!(failures.is_empty(), "JSON 이 내용을 빠뜨렸다:\n{}", failures.join("\n"));
}

/// 개요 트리가 **모든** 본문 단락을 담는가 — 글머리 없는 단락도 포함해서.
#[test]
fn outline_tree_contains_every_paragraph() {
    use hwp_core::outline;
    let files = samples();
    if files.is_empty() {
        return;
    }

    fn collect(nodes: &[outline::Node], out: &mut BTreeMap<(u32, u32), ()>) {
        for n in nodes {
            out.insert((n.section, n.para), ());
            collect(&n.children, out);
        }
    }

    let mut failures = Vec::new();
    for f in &files {
        let witness = Testimony::read(f).expect("증인");
        let items: Vec<outline::Item> = witness
            .body_paragraphs()
            .into_iter()
            .filter_map(|p| {
                p.index.map(|i| outline::Item {
                    section: p.section,
                    para: i,
                    text: p.text.replace('\u{FFFC}', ""),
                })
            })
            .filter(|i| !i.text.trim().is_empty())
            .collect();

        let tree = outline::build(&items);
        let mut seen = BTreeMap::new();
        collect(&tree, &mut seen);

        for it in &items {
            if !seen.contains_key(&(it.section, it.para)) {
                failures.push(format!(
                    "{}: 구역{} 단락{} 이 개요에서 사라졌다 — {:.40}",
                    f.display(),
                    it.section,
                    it.para,
                    it.text
                ));
                break;
            }
        }
        if seen.len() != items.len() {
            failures.push(format!(
                "{}: 단락 {}개인데 개요에 {}개 — 중복이 있다",
                f.display(),
                items.len(),
                seen.len()
            ));
        }
    }
    assert!(failures.is_empty(), "개요가 단락을 잃었다:\n{}", failures.join("\n"));
}

/// 프로파일이 토큰을 실제로 아끼는가 — outline 이 doc 보다 작아야 한다.
#[test]
fn profiles_actually_differ_in_size() {
    use hwp_core::json::{Profile, build};
    let Some((rhwp, _)) = backend() else { return };
    let Some(f) = samples().into_iter().max_by_key(|p| {
        std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
    }) else {
        return;
    };

    let size = |p: Profile| {
        serde_json::to_string(&build(&rhwp, &f, p).expect("build"))
            .expect("직렬화")
            .len()
    };
    let (outline, doc) = (size(Profile::Outline), size(Profile::Doc));
    assert!(
        outline < doc,
        "{}: outline {outline}B 가 doc {doc}B 보다 작아야 한다",
        f.display()
    );
}

/// 자가검증 저장이 실제 문서에서 **통과하는가**.
///
/// R2 는 처음으로 남의 문서를 고치는 단계다. 저장 경로가 서지 않으면 편집기를
/// 만들 수 없다.
#[test]
fn self_verifying_save_round_trips_a_real_document() {
    use hwp_core::edit::{EditLog, Op, save};
    let Some((rhwp, _)) = backend() else { return };
    let Some(src) = samples()
        .into_iter()
        .find(|p| p.extension().and_then(|s| s.to_str()) == Some("hwp"))
    else {
        return;
    };

    let dir = std::env::temp_dir().join("hwp-cmd-save-test");
    std::fs::create_dir_all(&dir).expect("임시 디렉터리");
    let work = dir.join("doc.hwp");
    std::fs::copy(&src, &work).expect("복사");

    // 편집할 좌표를 고른다 — 글자가 있는 첫 본문 단락.
    let before = Testimony::read(&work).expect("증인");
    let target = before
        .body_paragraphs()
        .into_iter()
        .find_map(|p| {
            let visible = hwp_core::offset::visible(&p.text);
            (visible.chars().count() >= 4).then(|| (p.section, p.index.unwrap(), visible))
        })
        .expect("글자 있는 단락");
    let (section, para, original) = target;

    let mut log = EditLog::new();
    log.push(Op::InsertText {
        at: (section, para),
        offset: 0,
        text: "〔검증〕".into(),
    });

    let report = save(&rhwp, &work, &log).expect("자가검증 저장이 통과해야 한다");
    assert_eq!(report.ops_applied, 1);
    assert!(report.backup.as_ref().is_some_and(|b| b.exists()), "백업이 남아야 한다");

    // 저장된 파일을 다시 읽어 확인한다.
    let after = Testimony::read(&work).expect("증인");
    let now = after
        .at(section, para)
        .map(|p| hwp_core::offset::visible(&p.text))
        .expect("그 단락이 있어야 한다");
    assert_eq!(now, format!("〔검증〕{original}"));

    // 백업은 원본 그대로여야 한다.
    let backup = report.backup.unwrap();
    let backed = Testimony::read(&backup).expect("백업 증인");
    assert_eq!(
        backed.at(section, para).map(|p| hwp_core::offset::visible(&p.text)),
        Some(original),
        "백업이 원본을 담고 있어야 한다"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 자가검증이 **멈추는가**. 검증이 통과만 하면 검증이 아니다.
///
/// 기대와 다른 결과가 나올 상황을 일부러 만든다: 메모리의 기대를 비틀면
/// 저장이 중단되고 원본이 그대로 남아야 한다.
#[test]
fn self_verifying_save_refuses_when_the_result_would_differ() {
    use hwp_core::edit::{EditLog, Op, expected, save};
    let Some((rhwp, _)) = backend() else { return };
    let Some(src) = samples()
        .into_iter()
        .find(|p| p.extension().and_then(|s| s.to_str()) == Some("hwp"))
    else {
        return;
    };

    let dir = std::env::temp_dir().join("hwp-cmd-refuse-test");
    std::fs::create_dir_all(&dir).expect("임시 디렉터리");
    let work = dir.join("doc.hwp");
    std::fs::copy(&src, &work).expect("복사");
    let sha_before = hwp_core::pin::sha256_file(&work).expect("해시");

    // 없는 좌표를 가리키는 연산 — 기대 계산 단계에서 먼저 걸린다.
    let mut log = EditLog::new();
    log.push(Op::InsertText {
        at: (99, 99),
        offset: 0,
        text: "x".into(),
    });
    let err = save(&rhwp, &work, &log).expect_err("없는 좌표는 거부해야 한다");
    assert!(format!("{err}").contains("없는 좌표"), "{err}");

    // 원본이 그대로여야 한다 — 바이트까지.
    let sha_after = hwp_core::pin::sha256_file(&work).expect("해시");
    assert_eq!(sha_before, sha_after, "거부했는데 원본이 바뀌었다");
    assert!(!work.with_extension("hwp.bak").exists(), "거부했으면 백업도 없다");

    // 범위를 넘는 삭제도 파일에 닿기 전에 걸린다.
    let before = Testimony::read(&work).expect("증인");
    let (sec, para) = before
        .body_paragraphs()
        .into_iter()
        .find_map(|p| p.index.map(|i| (p.section, i)))
        .expect("단락");
    let err = expected(
        &hwp_core::edit::Snapshot {
            paras: [((sec, para), "짧다".to_string())].into_iter().collect(),
            cells: Default::default(),
            drawings: Default::default(),
        },
        &[Op::DeleteText { at: (sec, para), offset: 0, count: 500 }],
    )
    .expect_err("범위를 넘는 삭제는 거부해야 한다");
    assert!(format!("{err}").contains("넘습"), "{err}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 증인의 **표 셀 좌표**가 백엔드와 같은가.
///
/// 셀 편집(`set-cell`)을 독립 검증하려면 증인이 `(표, 행, 열)` 을 백엔드와 같은
/// 뜻으로 읽어야 한다. 단락에서 한 번 당한 일이므로(개수만 맞고 순번이 달랐다)
/// 여기서는 처음부터 **셀 내용까지** 맞춰 본다.
#[test]
fn witness_agrees_with_backend_on_table_cells() {
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut failures = Vec::new();
    for f in &files {
        let export = rhwp.export_tables(f).expect("export-tables");
        let witness = Testimony::read(f).expect("증인");

        if witness.tables.len() != export.tables.len() {
            failures.push(format!(
                "{}: 표 개수 — 증인 {} vs rhwp {}",
                f.display(),
                witness.tables.len(),
                export.tables.len()
            ));
            continue;
        }

        for raw in &export.tables {
            let Some(mine) = witness.table(raw.index) else {
                failures.push(format!("{}: 표 {} 가 증인에게 없다", f.display(), raw.index));
                break;
            };
            // 비어 있지 않은 셀의 글자가 같은 좌표에 있어야 한다.
            let mut mismatched = 0usize;
            let mut checked = 0usize;
            for c in raw.cells.iter().filter(|c| !c.text.trim().is_empty()) {
                checked += 1;
                let got = mine.cell(c.row, c.col).map(|x| x.text.trim().to_string());
                let want = c.text.trim().to_string();
                if got.as_deref() != Some(want.as_str()) {
                    mismatched += 1;
                    if mismatched == 1 {
                        failures.push(format!(
                            "{} 표{} ({},{}): rhwp {:.40} / 증인 {:?}",
                            f.display(),
                            raw.index,
                            c.row,
                            c.col,
                            want,
                            got.map(|g| g.chars().take(40).collect::<String>())
                        ));
                    }
                }
            }
            if mismatched > 0 {
                failures.push(format!(
                    "  └ 표{} 에서 {checked}개 중 {mismatched}개 어긋남",
                    raw.index
                ));
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "증인과 백엔드의 셀 좌표가 어긋난다:\n{}",
        failures.join("\n")
    );
}

/// 셀 편집이 자가검증을 통과하고 **그 셀만** 바뀌는가.
///
/// 한국 서식은 대개 누름틀이 아니라 표로 되어 있다(이 코퍼스에서 누름틀은 3개
/// 파일에 7개뿐이고 표는 14개 파일에 126개다). 그래서 서식 채우기의 실제 경로는
/// 셀 쓰기다.
#[test]
fn setting_a_cell_changes_only_that_cell() {
    use hwp_core::edit::{EditLog, Op, save};
    let Some((rhwp, _)) = backend() else { return };
    let Some(src) = samples().into_iter().find(|p| {
        p.extension().and_then(|s| s.to_str()) == Some("hwp")
            && Testimony::read(p).is_ok_and(|w| w.tables.iter().any(|t| t.cells.len() > 3))
    }) else {
        eprintln!("건너뜀 — 표 있는 .hwp 샘플이 없다");
        return;
    };

    let dir = std::env::temp_dir().join("hwp-cmd-cell-test");
    std::fs::create_dir_all(&dir).expect("임시");
    let work = dir.join("form.hwp");
    std::fs::copy(&src, &work).expect("복사");

    let before = Testimony::read(&work).expect("증인");
    let before_cells: BTreeMap<(u32, usize, usize), String> = before
        .tables
        .iter()
        .flat_map(|t| {
            t.cells
                .iter()
                .map(move |c| ((t.index, c.row, c.col), c.text.clone()))
        })
        .collect();

    // 글자가 있는 셀을 하나 고른다 (좌표가 확실한 곳)
    let (&(table, row, col), _) = before_cells
        .iter()
        .find(|(_, v)| !v.trim().is_empty() && !v.contains('\n'))
        .expect("글자 있는 셀");

    const NEW: &str = "○○대학교 홍길동";
    let mut log = EditLog::new();
    log.push(Op::SetCell {
        table,
        row,
        col,
        lines: vec![NEW.into()],
    });

    let report = save(&rhwp, &work, &log).expect("셀 쓰기가 자가검증을 통과해야 한다");
    assert_eq!(report.ops_applied, 1);

    let after = Testimony::read(&work).expect("증인");
    let after_cells: BTreeMap<(u32, usize, usize), String> = after
        .tables
        .iter()
        .flat_map(|t| {
            t.cells
                .iter()
                .map(move |c| ((t.index, c.row, c.col), c.text.clone()))
        })
        .collect();

    assert_eq!(
        after_cells.get(&(table, row, col)).map(|s| s.as_str()),
        Some(NEW),
        "고른 셀에 값이 들어가야 한다"
    );

    // 다른 셀은 하나도 움직이지 않아야 한다.
    let moved: Vec<String> = before_cells
        .iter()
        .filter(|(k, _)| **k != (table, row, col))
        .filter(|(k, v)| after_cells.get(*k) != Some(*v))
        .map(|(k, v)| format!("표{} ({},{}) 이 {:.30} 에서 바뀌었다", k.0, k.1, k.2, v))
        .collect();
    assert!(
        moved.is_empty(),
        "셀 {}개 중 {}개가 덤으로 바뀌었다:\n{}",
        before_cells.len(),
        moved.len(),
        moved.join("\n")
    );

    // 본문 단락도 그대로여야 한다.
    assert_eq!(
        before.body_count(),
        after.body_count(),
        "셀만 고쳤는데 본문 단락 수가 바뀌었다"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 그림이 **빠짐없이 읽히는가.**
///
/// 처음엔 BMP 가 전부 "읽을 수 없는 형식" 으로 나왔다. 이름이 `BIN0001.bmp` 인데
/// 내용은 raw deflate 로 눌린 바이트였기 때문이다(HWP5 는 BinData 를 개별로
/// 압축한다). 그 종류의 실패는 **조용하다** — 그림이 없는 문서처럼 보인다.
#[test]
fn every_image_in_the_corpus_decodes() {
    use hwp_core::images;
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut unreadable = Vec::new();
    let mut total = 0usize;
    for f in &files {
        let imgs = images::read(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        for img in &imgs {
            total += 1;
            if img.dimensions().is_none() {
                unreadable.push(format!(
                    "{} 그림 {} ({}, {} 바이트)",
                    f.display(),
                    img.index + 1,
                    img.name,
                    img.bytes.len()
                ));
            }
        }
    }
    assert!(total > 0, "코퍼스에 그림이 있어야 이 시험이 뜻이 있다");
    assert!(
        unreadable.is_empty(),
        "그림 {total}개 중 {}개를 읽지 못했다:\n{}",
        unreadable.len(),
        unreadable.join("\n")
    );
}

/// 그림이 **본문 좌표에 붙는가.** 좌표가 없으면 페이저가 그릴 자리를 모른다.
#[test]
fn images_are_anchored_to_real_paragraphs() {
    use hwp_core::images;
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut problems = Vec::new();
    for f in &files {
        let imgs = images::read(f).expect("그림");
        if imgs.is_empty() {
            continue;
        }
        let witness = Testimony::read(f).expect("증인");
        let mut anchored = 0usize;
        for img in &imgs {
            let Some((sec, para)) = img.at else { continue };
            anchored += 1;
            if witness.at(sec, para).is_none() {
                problems.push(format!(
                    "{} 그림 {} 이 없는 단락을 가리킨다 — 구역{sec} 단락{para}",
                    f.display(),
                    img.index + 1
                ));
            }
        }
        // 표 셀·글상자 안의 그림은 본문 좌표가 없는 게 맞다. 다만 하나도
        // 붙지 않으면 좌표를 세는 쪽이 틀린 것이다.
        if anchored == 0 {
            problems.push(format!(
                "{}: 그림 {}개가 전부 좌표 없이 떠 있다",
                f.display(),
                imgs.len()
            ));
        }
    }
    assert!(problems.is_empty(), "그림 좌표가 어긋난다:\n{}", problems.join("\n"));
}

/// 점자로 그린 그림의 **줄 폭이 고르게 나오는가.** 한 줄이라도 어긋나면 화면이 밀린다.
#[test]
fn dot_art_lines_all_have_the_same_width() {
    use hwp_core::{dots, images};
    let Some(f) = samples().into_iter().find(|p| {
        images::read(p).map(|v| v.iter().any(|i| i.dimensions().is_some())).unwrap_or(false)
    }) else {
        return;
    };

    let imgs = images::read(&f).expect("그림");
    for img in imgs.iter().filter(|i| i.dimensions().is_some()) {
        let Some(art) = dots::render(&img.bytes, 40, 20) else {
            panic!("{} 그림 {} 을 그리지 못했다", f.display(), img.index + 1);
        };
        for line in &art.lines {
            assert_eq!(
                unicode_width::UnicodeWidthStr::width(line.as_str()),
                art.cols,
                "{} 그림 {}: 줄 폭이 {} 이어야 한다",
                f.display(),
                img.index + 1,
                art.cols
            );
        }
    }
}

/// 코퍼스의 **모든 표**가 줄 폭이 고르게 그려지는가.
///
/// 한 줄만 어긋나도 화면 전체가 밀린다. 그리고 병합 조합은 손으로 상상할 수
/// 있는 것보다 훨씬 다양하다 — 코퍼스에 표 126개, 병합 셀이 든 표가 여럿이다.
#[test]
fn every_table_in_the_corpus_draws_evenly() {
    use hwp_core::wrap::display_width;
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut problems = Vec::new();
    let mut drawn = 0usize;
    for f in &files {
        let doc = match hwp_core::ScreenDoc::build(&rhwp, f) {
            Ok(d) => d,
            Err(e) => {
                problems.push(format!("{}: {e}", f.display()));
                continue;
            }
        };
        for (i, placed) in doc.tables.iter().enumerate() {
            for width in [40usize, 80, 120] {
                let lines = placed.table.render(width);
                if lines.is_empty() {
                    continue;
                }
                drawn += 1;
                let first = display_width(&lines[0]);
                if let Some((n, bad)) = lines
                    .iter()
                    .enumerate()
                    .find(|(_, l)| display_width(l) != first)
                {
                    problems.push(format!(
                        "{} 표{i} (폭 {width}): {n}번째 줄이 {}칸, 첫 줄은 {first}칸\n    {bad:?}",
                        f.display(),
                        display_width(bad)
                    ));
                    break;
                }
            }
        }
    }
    assert!(drawn > 0, "코퍼스에 표가 있어야 이 시험이 뜻이 있다");
    assert!(problems.is_empty(), "표가 어긋나게 그려진다:\n{}", problems.join("\n"));
}

/// 표가 **본문 좌표에 붙는가.** 안 붙으면 페이저가 그릴 자리를 모른다.
#[test]
fn tables_are_anchored_to_real_paragraphs() {
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut problems = Vec::new();
    for f in &files {
        let Ok(doc) = hwp_core::ScreenDoc::build(&rhwp, f) else { continue };
        if doc.tables.is_empty() {
            continue;
        }
        let witness = Testimony::read(f).expect("증인");
        for (i, t) in doc.tables.iter().enumerate() {
            let (sec, para) = t.at;
            if witness.at(sec, para).is_none() {
                problems.push(format!(
                    "{} 표{i} 가 없는 단락을 가리킨다 — 구역{sec} 단락{para}",
                    f.display()
                ));
            }
        }
    }
    assert!(problems.is_empty(), "표 좌표가 어긋난다:\n{}", problems.join("\n"));
}

/// 개체 표시가 **뜻을 가지는가.**
///
/// 처음엔 인라인 컨트롤이 있으면 다 표시했는데, 구역 정의·쪽번호 위치까지 걸려
/// "여기 그림이 있는데 안 보인다" 는 오해를 불렀다. 볼 내용은 그리기개체(11)
/// 뿐이고, 그중 우리가 표·그림으로 그린 것은 표시할 이유가 없다.
#[test]
fn object_counts_only_include_drawings_not_section_definitions() {
    use hwp_core::witness::CTRL_DRAWING_OR_TABLE;
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut checked = 0usize;
    for f in &files {
        let Ok(doc) = hwp_core::ScreenDoc::build(&rhwp, f) else { continue };
        let witness = Testimony::read(f).expect("증인");

        for p in witness.body_paragraphs() {
            let Some(i) = p.index else { continue };
            let drawings = p
                .objects
                .iter()
                .filter(|c| **c == CTRL_DRAWING_OR_TABLE)
                .count();
            let counted = doc.objects.get(&(p.section, i)).copied().unwrap_or(0);
            assert_eq!(
                counted, drawings,
                "{} 구역{} 단락{i}: 그리기개체만 세야 한다 (컨트롤 {:?})",
                f.display(),
                p.section,
                p.objects
            );
            if !p.objects.is_empty() {
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "컨트롤이 든 단락이 있어야 이 시험이 뜻이 있다");
}

/// 코퍼스의 어떤 그림도 **통째로 칠해져 나오지 않는가.**
///
/// 꽉 찬 칸(`⣿`)만 늘어선 그림은 정보가 0 이다. 그렇게 나오는 원인은 조용하다:
/// 투명한 PNG 의 알파를 무시하면 배경 RGB 가 (0,0,0) 이라 **새까맣게** 읽힌다.
/// 코퍼스 첫 그림이 그랬다 — 84%가 완전 투명한 로고 띠인데 알파를 무시하면
/// 픽셀의 75%가 어둡고(흰 종이에 합성하면 14.5%), 자동 반전까지 걸려 사람이
/// 보는 것과 정반대가 나왔다.
#[test]
fn no_corpus_image_renders_as_a_solid_block() {
    use hwp_core::{dots, images};
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut flat = Vec::new();
    let mut drawn = 0usize;
    for f in &files {
        let imgs = images::read(f).expect("그림");
        for img in imgs.iter().filter(|i| i.dimensions().is_some()) {
            let Some(art) = dots::render(&img.bytes, 60, 24) else { continue };
            drawn += 1;
            let total: usize = art.lines.iter().map(|l| l.chars().count()).sum();
            if total == 0 {
                continue;
            }
            let full = art
                .lines
                .iter()
                .flat_map(|l| l.chars())
                .filter(|c| *c == '\u{28FF}')
                .count();
            let empty = art
                .lines
                .iter()
                .flat_map(|l| l.chars())
                .filter(|c| *c == '\u{2800}')
                .count();
            // 전부 차 있거나 전부 비어 있으면 볼 것이 없다
            if full * 10 > total * 9 {
                flat.push(format!(
                    "{} 그림 {}: {}/{} 칸이 꽉 찼다 — 통째로 칠해졌다",
                    f.display(),
                    img.index + 1,
                    full,
                    total
                ));
            } else if empty == total {
                flat.push(format!(
                    "{} 그림 {}: 한 점도 켜지지 않았다",
                    f.display(),
                    img.index + 1
                ));
            }
        }
    }
    assert!(drawn > 0, "그릴 수 있는 그림이 있어야 이 시험이 뜻이 있다");
    assert!(flat.is_empty(), "그림이 정보 없이 나온다:\n{}", flat.join("\n"));
}

/// **모든 그림이 문서가 놓은 크기를 들고 있는가**, 그리고 그 크기가 제정신인가.
///
/// 처음 만들 때 HWPX 쪽은 형제 `hp:sz` 를 읽었는데, 그것은 맨 바깥 개체에만
/// 있어서 묶음 안의 그림 8개(s14.hwpx 15개 중)가 빈칸으로 나왔다. 빈칸이면
/// 단 폭 전체로 그려지므로 **본문 폭 2% 짜리 아이콘이 전면 배너로 보인다** —
/// 조용히 틀리는 쪽이라 이 시험이 있다.
#[test]
fn every_image_knows_the_size_the_document_gave_it() {
    use hwp_core::images;
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut problems = Vec::new();
    let mut counted = 0usize;
    for f in &files {
        let imgs = images::read(f).expect("그림");
        for img in &imgs {
            counted += 1;
            let Some((w, h)) = img.placed else {
                problems.push(format!(
                    "{} 그림 {}: 놓인 크기가 없다",
                    f.display(),
                    img.index + 1
                ));
                continue;
            };
            // A4 한 장이 595×842pt = 59500×84200 HWPUNIT. 그보다 10배 크면
            // 단위를 잘못 읽은 것이다.
            if w == 0 || h == 0 || w > 600_000 || h > 900_000 {
                problems.push(format!(
                    "{} 그림 {}: 크기가 이상하다 — {w}×{h} HWPUNIT",
                    f.display(),
                    img.index + 1
                ));
            }
        }
    }
    assert!(counted > 0, "코퍼스에 그림이 하나도 없다");
    assert!(
        problems.is_empty(),
        "그림 {counted}개 중 놓인 크기가 어긋난다:\n{}",
        problems.join("\n")
    );
}

/// 놓인 크기를 **본문 폭에 대한 칸 수로 바꿀 때** 단을 넘지 않는가.
///
/// 그리고 코퍼스에 **작은 그림과 큰 그림이 둘 다 있어야** 이 계산이 뭔가를
/// 하고 있다고 말할 수 있다. 전부 단 폭 전체로 나오면 고치기 전과 같다.
#[test]
fn placed_width_maps_into_the_column_and_actually_varies() {
    if !needs_real_corpus("placed_width_maps_into_the_column_and_actually_varies") {
        return;
    }
    use hwp_core::images;
    use hwp_core::screen::ScreenDoc;
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut problems = Vec::new();
    let mut ratios: Vec<f64> = Vec::new();
    for f in &files {
        let imgs = images::read(f).expect("그림");
        if imgs.is_empty() {
            continue;
        }
        let doc = ScreenDoc::build(&rhwp, f).expect("화면 모델");
        if doc.body_width == 0 {
            problems.push(format!("{}: 본문 폭이 0 이다", f.display()));
            continue;
        }
        let column = doc.column.cells.max(8);
        for img in &imgs {
            let cells = img.cells_wide(doc.body_width, column);
            if cells == 0 || cells > column {
                problems.push(format!(
                    "{} 그림 {}: {cells}칸 — 단 폭 {column}칸을 벗어난다",
                    f.display(),
                    img.index + 1
                ));
            }
            if let Some(r) = img.width_ratio(doc.body_width) {
                ratios.push(r);
            }
        }
    }
    assert!(problems.is_empty(), "칸 수 환산이 어긋난다:\n{}", problems.join("\n"));

    let small = ratios.iter().filter(|r| **r < 0.25).count();
    let large = ratios.iter().filter(|r| **r > 0.75).count();
    assert!(
        small > 0 && large > 0,
        "그림 {}개의 본문 폭 비율이 한쪽으로 쏠렸다 — 25% 미만 {small}개, 75% 초과 {large}개. \
         크기를 반영하고 있다고 말할 수 없다",
        ratios.len()
    );
}

/// **표 칸 안의 그림이 실제로 그 칸에 있는가** — 백엔드의 표와 좌표까지 맞춰 본다.
///
/// 칸 좌표는 우리가 레코드(HWP5)·OWPML(HWPX)에서 직접 센 것이고, 표와 셀은
/// 백엔드 `export-tables` 가 따로 읽은 것이다. 두 판독이 같은 표 번호와 같은
/// 칸을 가리켜야 한다. 어긋나면 그림이 엉뚱한 칸에 그려지는데, 표는 멀쩡히
/// 그려지므로 **보기에 아무 문제가 없다** — 그래서 이 대조가 필요하다.
#[test]
fn images_inside_table_cells_land_on_a_real_cell() {
    if !needs_real_corpus("images_inside_table_cells_land_on_a_real_cell") {
        return;
    }
    use hwp_core::images;
    use hwp_core::screen::ScreenDoc;
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut problems = Vec::new();
    let mut matched = 0usize;
    for f in &files {
        let imgs = images::read(f).expect("그림");
        if imgs.is_empty() {
            continue;
        }
        let doc = ScreenDoc::build(&rhwp, f).expect("화면 모델");
        for img in &imgs {
            let Some(at) = img.cell else { continue };
            let Some(t) = doc.tables.iter().find(|t| t.index == at.table) else {
                problems.push(format!(
                    "{} 그림 {}: 표 {} 가 백엔드에 없다",
                    f.display(),
                    img.index + 1,
                    at.table
                ));
                continue;
            };
            // 그림이 붙은 본문 단락과 그 표가 놓인 단락이 같아야 한다
            if let Some(a) = img.at {
                if t.at != a {
                    problems.push(format!(
                        "{} 그림 {}: 표 {} 는 구역{} 단락{} 인데 그림은 구역{} 단락{}",
                        f.display(),
                        img.index + 1,
                        at.table,
                        t.at.0,
                        t.at.1,
                        a.0,
                        a.1
                    ));
                    continue;
                }
            }
            if !t.table.cells.iter().any(|c| c.row == at.row && c.col == at.col) {
                problems.push(format!(
                    "{} 그림 {}: 표 {}({}×{}) 에 {}행 {}열 칸이 없다",
                    f.display(),
                    img.index + 1,
                    at.table,
                    t.table.rows,
                    t.table.cols,
                    at.row + 1,
                    at.col + 1
                ));
                continue;
            }
            matched += 1;
        }
    }
    assert!(problems.is_empty(), "칸 좌표가 어긋난다:\n{}", problems.join("\n"));
    // 코퍼스에 칸 안 그림이 실제로 있다 — 없으면 이 시험은 아무것도 안 한 것이다
    assert!(
        matched >= 20,
        "표 칸 안 그림이 {matched}개밖에 맞지 않았다. 실측은 43개 중 23개다 — \
         칸을 세는 쪽이 그림을 놓치고 있다"
    );
}

/// **증인이 읽은 첫줄 들여쓰기가 조판기의 `columnStart` 와 같은가.**
///
/// 이것이 문단서식 판독 전체의 시험이다. 증인은 파일을 직접 읽고
/// (HWP5 는 `DocInfo` 의 `PARA_SHAPE` + `PARA_HEADER` 의 `paraShapeId`,
/// HWPX 는 `header.xml` 의 `hh:paraPr` + `hp:p@paraPrIDRef`), 조판기는 따로
/// 조판한다. 한쪽이 틀리면 갈라진다.
///
/// 특히 **HWP5 의 저장 단위가 HWPUNIT 의 절반**이라는 실측과, HWPX 의
/// `hp:switch` 에서 HWPUNIT 판을 골라야 한다는 것을 이 시험이 지킨다. 둘 중
/// 하나만 틀려도 숫자가 2배로 갈라진다.
#[test]
fn witness_first_indent_agrees_with_the_typesetter() {
    if !needs_real_corpus("witness_first_indent_agrees_with_the_typesetter") {
        return;
    }
    use hwp_core::witness::Testimony;
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut problems = Vec::new();
    let mut compared = 0usize;
    for f in &files {
        let w = Testimony::read(f).expect("증인");
        let layout = rhwp.dump_pages(f).expect("조판");
        // 조판기의 (구역, 구역내 단락) → columnStart
        let mut cs: BTreeMap<(u32, u32), i64> = BTreeMap::new();
        for page in &layout.pages {
            for col in &page.columns {
                for item in &col.items {
                    let Some(para) = item.para_index else { continue };
                    let Some(segs) = &item.line_segs else { continue };
                    let Some(first) = &segs.first else { continue };
                    cs.insert((page.section, para), first.column_start);
                }
            }
        }
        for p in w.body_paragraphs() {
            let Some(i) = p.index else { continue };
            let Some(want) = cs.get(&(p.section, i)) else { continue };
            compared += 1;
            // 1 HWPUNIT 은 0.01pt — 반올림 차이만 허용한다
            if (p.style.first_indent - want).abs() > 1 {
                problems.push(format!(
                    "{} 구역{} 단락{}: 증인 {} vs 조판기 {}",
                    f.display(),
                    p.section,
                    i,
                    p.style.first_indent,
                    want
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "문단서식 판독이 조판기와 갈라진다 ({compared}개 중 {}개):\n{}",
        problems.len(),
        problems.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
    );
    // 실측 670개. 크게 줄면 좌표를 못 맞추고 있다는 뜻이다
    assert!(compared >= 600, "대조한 단락이 {compared}개뿐이다 — 실측은 670개다");
}

/// **내어쓰기가 읽히는가.** 코퍼스 본문의 63%가 음수 들여쓰기를 쓴다.
///
/// `columnStart` 로는 알 수 없는 값이다(내어쓰기 문단에서 0 을 돌려준다).
/// 하나도 안 잡히면 다수 문단의 모양을 잃고 있다는 뜻이고, 조판기와 대조하는
/// 위의 시험은 그걸 잡지 못한다 — 첫 줄은 맞기 때문이다.
#[test]
fn hanging_indents_are_read_and_are_the_common_case() {
    if !needs_real_corpus("hanging_indents_are_read_and_are_the_common_case") {
        return;
    }
    use hwp_core::witness::Testimony;
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut total = 0usize;
    let mut hanging = 0usize;
    for f in &files {
        let w = Testimony::read(f).expect("증인");
        for p in w.body_paragraphs() {
            if p.index.is_none() {
                continue;
            }
            total += 1;
            if p.style.rest_indent > p.style.first_indent {
                hanging += 1;
            }
        }
    }
    assert!(total > 0, "본문 단락이 없다");
    // 실측 795단락 중 501개(63%)
    assert!(
        hanging * 100 / total >= 40,
        "내어쓰기 문단이 {hanging}/{total} ({}%) 밖에 안 잡혔다 — 실측은 63% 다",
        hanging * 100 / total
    );
}

/// 들여쓰기를 칸으로 바꿀 때 **단을 먹지 않는가.**
#[test]
fn indents_fit_inside_the_column() {
    if !needs_real_corpus("indents_fit_inside_the_column") {
        return;
    }
    use hwp_core::screen::ScreenDoc;
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut problems = Vec::new();
    let mut nonzero = 0usize;
    for f in &files {
        let doc = ScreenDoc::build(&rhwp, f).expect("화면 모델");
        let column = doc.column.cells.max(8);
        for (at, ind) in &doc.indents {
            if ind.first > doc.body_width as i64 || ind.rest > doc.body_width as i64 {
                problems.push(format!(
                    "{} {at:?}: 들여쓰기 {}·{} 가 본문 폭 {} 보다 넓다",
                    f.display(),
                    ind.first,
                    ind.rest,
                    doc.body_width
                ));
                continue;
            }
            let (c1, c2) = doc.indent_cells(*at, column);
            if c1 + 8 > column || c2 + 8 > column {
                problems.push(format!(
                    "{} {at:?}: {c1}·{c2}칸이 단 {column}칸을 먹는다",
                    f.display()
                ));
            }
            if c1 > 0 || c2 > 0 {
                nonzero += 1;
            }
        }
    }
    assert!(problems.is_empty(), "들여쓰기가 단을 벗어난다:\n{}", problems.join("\n"));
    assert!(nonzero > 0, "들여쓰기를 읽었는데 칸으로는 전부 0 이다 — 화면에 안 나타난다");
}

/// **가운데·오른쪽 정렬이 읽히는가**, 그리고 그것만 담겼는가.
#[test]
fn only_center_and_right_alignment_is_carried() {
    if !needs_real_corpus("only_center_and_right_alignment_is_carried") {
        return;
    }
    use hwp_core::screen::ScreenDoc;
    use hwp_core::witness::ParaAlign;
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut center = 0usize;
    let mut right = 0usize;
    for f in &files {
        let doc = ScreenDoc::build(&rhwp, f).expect("화면 모델");
        for a in doc.aligns.values() {
            match a {
                ParaAlign::Center => center += 1,
                ParaAlign::Right => right += 1,
                // 왼쪽으로 그릴 것은 담지 않는다 — 담으면 align_pad 가
                // 0 을 돌려주느라 헛일을 한다
                other => panic!("{} 에 {other:?} 가 담겼다", f.display()),
            }
        }
    }
    // 실측: 본문 795단락 중 가운데 52 · 오른쪽 44
    assert!(center >= 30, "가운데 정렬이 {center}개뿐이다 — 실측은 52개다");
    assert!(right >= 25, "오른쪽 정렬이 {right}개뿐이다 — 실측은 44개다");
}

/// **글머리 단계 사다리를 문서마다 따로 읽는가.**
///
/// 한국 공문서의 단계는 문서마다 다르다. 하나의 사다리를 박아 두면 다른
/// 문서에서 엉뚱한 기호를 넣는다 — 실측: s00 은 `□(0칸) □(2칸) -(3칸)`,
/// s13 은 `번호(0칸) ○(1칸) -(3칸)`, s02 는 5단계다.
#[test]
fn the_outline_ladder_comes_from_each_document() {
    use hwp_core::outline::{Item, Levels};
    use hwp_core::witness::Testimony;
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut ladders: Vec<(String, Vec<(usize, String)>)> = Vec::new();
    for f in &files {
        let w = Testimony::read(f).expect("증인");
        let items: Vec<Item> = w
            .body_paragraphs()
            .into_iter()
            .filter_map(|p| {
                p.index.map(|i| Item {
                    section: p.section,
                    para: i,
                    text: p.text.replace('\u{FFFC}', ""),
                })
            })
            .collect();
        let lv = Levels::build(&items);
        if lv.is_empty() {
            continue;
        }
        let steps: Vec<(usize, String)> =
            lv.steps().into_iter().map(|s| (s.indent, s.marker)).collect();
        // 단계마다 기호가 비어 있지 않다 — 비면 Tab 이 기호를 지울 것이다
        for s in lv.steps() {
            assert!(
                !s.marker.is_empty(),
                "{} 의 {}단계에 대표 기호가 없다",
                f.display(),
                s.level
            );
        }
        ladders.push((f.display().to_string(), steps));
    }
    assert!(ladders.len() >= 3, "사다리를 읽은 문서가 {}개뿐이다", ladders.len());
    // 전부 같은 사다리면 문서에서 읽는 게 아니라 박아 둔 것이다
    let distinct: std::collections::BTreeSet<&Vec<(usize, String)>> =
        ladders.iter().map(|(_, s)| s).collect();
    assert!(
        distinct.len() > 1,
        "문서 {}개의 사다리가 모두 같다 — 문서에서 읽고 있지 않다",
        ladders.len()
    );
}

/// **표 칸의 정렬이 읽히고, 가운데가 다수인가.**
///
/// 실측: 코퍼스의 표·글상자 단락 2357개 중 **64%(1518개)가 가운데**이고
/// 양쪽맞춤은 25%뿐이다. 본문과 정반대다(본문은 양쪽맞춤 76%, 가운데 6%).
/// 전부 왼쪽으로 그리면 공문서 표가 서식처럼 보이지 않는다.
///
/// 격자의 칸 정렬은 **증인이 읽고 백엔드의 표에 좌표로 맞춰 채운다** — 백엔드
/// `export-tables` 는 글자만 주고 정렬을 주지 않는다. 그 맞추기가 깨지면
/// 정렬이 전부 기본값으로 떨어지므로, 비율로 지킨다.
#[test]
fn table_cells_carry_their_alignment_and_center_is_the_common_case() {
    use hwp_core::screen::ScreenDoc;
    use hwp_core::witness::ParaAlign;
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut total = 0usize;
    let mut center = 0usize;
    let mut with_text = 0usize;
    for f in &files {
        let doc = ScreenDoc::build(&rhwp, f).expect("화면 모델");
        for t in &doc.tables {
            for c in &t.table.cells {
                total += 1;
                if !c.text.trim().is_empty() {
                    with_text += 1;
                }
                if c.align == ParaAlign::Center {
                    center += 1;
                }
            }
        }
    }
    assert!(total > 0, "코퍼스에 표 칸이 없다");
    assert!(with_text > 0, "글자가 든 칸이 없다 — 표를 못 읽고 있다");
    // 실측 64%. 절반 아래로 떨어지면 증인↔백엔드 좌표 맞추기가 깨진 것이다
    assert!(
        center * 100 / total >= 40,
        "가운데 정렬 칸이 {center}/{total} ({}%) 뿐이다 — 실측은 64% 다. \
         증인의 표 번호·행·열이 백엔드와 안 맞는지 보라",
        center * 100 / total
    );
}

/// **증인이 내놓는 `(구역, 단락)` 이 실제 편집 좌표인가.**
///
/// 단락 번호는 **구역 안에서** 센다. 구역을 가로질러 센 번호를 좌표처럼 쓰면
/// 구역이 여럿인 문서에서 엉뚱한 곳을 가리키거나 "없는 단락이다" 가 된다 —
/// 공개된 한글문서파일형식 5.0 스펙 문서(구역 6개)에서 `hwp witness --text` 가
/// 찍은 번호를 그대로 `set-para 0:12` 에 넣었다가 거부당했다.
///
/// 이 프로젝트가 같은 실수를 두 번 한 자리다(R0 에서 "개수 일치"를 "순번 일치"로
/// 착각했다). 그래서 구조로 못 박는다: 모든 본문 단락의 `index` 가 그 구역의
/// 단락 수 안에 들어야 하고, 구역마다 0 부터 빈틈없이 이어져야 한다.
#[test]
fn every_witness_coordinate_is_a_real_edit_coordinate() {
    if !needs_real_corpus("every_witness_coordinate_is_a_real_edit_coordinate") {
        return;
    }
    use hwp_core::witness::Testimony;
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut problems = Vec::new();
    let mut multi_section = 0usize;
    for f in &files {
        let w = Testimony::read(f).expect("증인");
        let counts = w.section_counts();
        if counts.len() > 1 {
            multi_section += 1;
        }
        // 구역마다 본 index 들
        let mut seen: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for p in w.body_paragraphs() {
            let Some(i) = p.index else { continue };
            seen.entry(p.section).or_default().push(i);
        }
        for (sec, mut idx) in seen {
            let n = counts.get(sec as usize).copied().unwrap_or(0);
            idx.sort_unstable();
            if let Some(&max) = idx.last() {
                if max as usize >= n {
                    problems.push(format!(
                        "{} 구역{sec}: 단락 번호가 {max} 까지 가는데 그 구역은 {n}단락이다",
                        f.display()
                    ));
                }
            }
            // 0 부터 빈틈없이
            let want: Vec<u32> = (0..idx.len() as u32).collect();
            if idx != want {
                problems.push(format!(
                    "{} 구역{sec}: 번호가 0부터 이어지지 않는다 — 앞 5개 {:?}",
                    f.display(),
                    &idx[..idx.len().min(5)]
                ));
            }
        }
    }
    assert!(problems.is_empty(), "좌표가 편집 좌표가 아니다:\n{}", problems.join("\n"));
    // 구역이 하나뿐인 문서만으로는 이 시험이 아무것도 안 한다
    assert!(
        multi_section > 0,
        "구역이 여럿인 문서가 코퍼스에 없다 — 이 시험이 잡을 것이 없다"
    );
}

/// **글자 꾸밈을 HWP5 와 HWPX 두 길로 읽어 맞춰 본다.**
///
/// 같은 문서를 `rhwp export-hwpx` 로 바꿔 두 포맷으로 읽는다. 우리 쪽 코드
/// 경로가 완전히 다르므로(레코드 비트 vs OWPML 엘리먼트) 한쪽이 틀리면
/// 갈라진다. 표 셀을 이 방법으로 맞추다가 디코더 버그 5건이 나왔다.
///
/// **컨트롤 글자는 비교에서 뺀다.** 그림·필드가 구간 안에 드는지가 두 포맷에서
/// 다르고(실측: 115단락 중 3단락), 컨트롤에 진하게가 붙든 말든 화면에는 차이가
/// 없다. 비교하는 것은 "**어느 보이는 글자가 진하게인가**" 다.
#[test]
fn character_decoration_agrees_across_the_two_formats() {
    if !needs_real_corpus("character_decoration_agrees_across_the_two_formats") {
        return;
    }
    use hwp_core::witness::Testimony;
    let Some((rhwp, _)) = backend() else { return };
    let files: Vec<PathBuf> = samples()
        .into_iter()
        .filter(|f| f.extension().and_then(|s| s.to_str()) == Some("hwp"))
        .collect();
    if files.is_empty() {
        return;
    }

    /// 보이는 글자별 꾸밈 — 컨트롤 자리는 `None`.
    fn per_char(t: &Testimony) -> BTreeMap<(u32, u32), Vec<Option<String>>> {
        let mut out = BTreeMap::new();
        for p in t.body_paragraphs() {
            let Some(i) = p.index else { continue };
            let chars: Vec<char> = p.text.chars().collect();
            let mut marks: Vec<Option<String>> = chars
                .iter()
                .map(|c| (*c != '\u{FFFC}').then(String::new))
                .collect();
            for r in &p.runs {
                let len = marks.len();
                let from = r.start.min(len);
                let end = (r.start + r.len).min(len);
                for m in marks[from..end].iter_mut() {
                    if let Some(s) = m {
                        let d = &r.deco;
                        *s = format!(
                            "{}{}{}{}",
                            if d.bold { "B" } else { "" },
                            if d.italic { "I" } else { "" },
                            if d.underline { "U" } else { "" },
                            if d.strike { "S" } else { "" }
                        );
                    }
                }
            }
            out.insert((p.section, i), marks);
        }
        out
    }

    let dir = std::env::temp_dir().join("hwp-deco-cross");
    let _ = std::fs::create_dir_all(&dir);
    let mut problems = Vec::new();
    let mut compared = 0usize;
    let mut decorated = 0usize;
    for f in &files {
        let stem = f.file_stem().unwrap().to_string_lossy().to_string();
        let out = dir.join(format!("{stem}.hwpx"));
        let _ = std::fs::remove_file(&out);
        if rhwp
            .run(&["export-hwpx", &f.to_string_lossy(), &out.to_string_lossy()])
            .is_err()
        {
            continue;
        }
        let a = per_char(&Testimony::read(f).expect("증인 hwp5"));
        let b = per_char(&Testimony::read(&out).expect("증인 hwpx"));
        for (at, left) in &a {
            let Some(right) = b.get(at) else { continue };
            // 컨트롤이 아닌 자리만 비교한다
            let l: Vec<&String> = left.iter().flatten().collect();
            let r: Vec<&String> = right.iter().flatten().collect();
            if l.len() != r.len() {
                // 글자 수가 다르면 꾸밈 이전의 문제다 — 다른 시험이 본다
                continue;
            }
            compared += 1;
            if l.iter().any(|s| !s.is_empty()) {
                decorated += 1;
            }
            if l != r {
                let bad = l
                    .iter()
                    .zip(&r)
                    .enumerate()
                    .filter(|(_, (x, y))| x != y)
                    .map(|(i, (x, y))| format!("{i}자: hwp5={x:?} hwpx={y:?}"))
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(", ");
                problems.push(format!("{} {at:?}: {bad}", f.display()));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "글자 꾸밈이 두 포맷에서 갈라진다 ({}군데):\n{}",
        problems.len(),
        problems.iter().take(8).cloned().collect::<Vec<_>>().join("\n")
    );
    assert!(compared > 200, "대조한 단락이 {compared}개뿐이다");
    // 꾸밈이 하나도 없으면 이 시험이 아무것도 안 한 것이다
    assert!(decorated > 20, "꾸밈이 있는 단락이 {decorated}개뿐이다");
}

/// 구간이 **글자 범위 안에 있고, 겹치지 않고, 꾸밈이 있는가.**
///
/// 빈 단락은 `PARA_TEXT` 레코드를 아예 내지 않으면서 `PARA_CHAR_SHAPE` 는
/// 낸다(s13.hwp 단락1). 위치표를 단락마다 비우지 않으면 **앞 단락의 표를
/// 물려받아** 빈 단락에 11자 구간이 붙는다 — 실제로 그렇게 나왔다. 이 시험이
/// 그 종류를 전부 잡는다.
#[test]
fn decoration_runs_stay_inside_their_paragraph() {
    if !needs_real_corpus("decoration_runs_stay_inside_their_paragraph") {
        return;
    }
    use hwp_core::witness::Testimony;
    let files = samples();
    if files.is_empty() {
        return;
    }
    let mut problems = Vec::new();
    let mut runs_seen = 0usize;
    for f in &files {
        let t = Testimony::read(f).expect("증인");
        for p in t.body_paragraphs() {
            let n = p.text.chars().count();
            let mut prev_end = 0usize;
            for r in &p.runs {
                runs_seen += 1;
                if r.len == 0 || r.start + r.len > n {
                    problems.push(format!(
                        "{} 구역{} 단락{:?}: 구간 {}+{} 이 글자 {n}개를 벗어난다",
                        f.display(),
                        p.section,
                        p.index,
                        r.start,
                        r.len
                    ));
                }
                if r.start < prev_end {
                    problems.push(format!(
                        "{} 구역{} 단락{:?}: 구간이 겹친다 ({} < {prev_end})",
                        f.display(),
                        p.section,
                        p.index,
                        r.start
                    ));
                }
                if !r.deco.any() {
                    problems.push(format!(
                        "{} 구역{} 단락{:?}: 꾸밈 없는 구간이 담겼다",
                        f.display(),
                        p.section,
                        p.index
                    ));
                }
                prev_end = r.start + r.len;
            }
        }
    }
    assert!(problems.is_empty(), "꾸밈 구간이 어긋난다:\n{}", problems.join("\n"));
    assert!(runs_seen > 50, "구간이 {runs_seen}개뿐이다 — 읽는 쪽이 죽었다");
}

/// **두 폭 정책에서 모두 표가 맞는가.**
///
/// `─ │ ┼ ├ ┤ ┌ ┐ └ ┘` 는 전부 East Asian **Ambiguous** 다. 터미널이 그걸
/// 2칸으로 그리면 선은 두 배가 되는데 한글은 그대로라 끝선이 어긋난다 —
/// 실측: `┌────┬────┐` 22칸 vs `│가나│다라│` 14칸. 코퍼스를 찍은 줄의 76%에
/// Ambiguous 글자가 들어 있고 54,754개 중 53,000개가 선 문자다.
///
/// 그래서 넓게 그리는 터미널에서는 **ASCII(`+ - |`)로 그린다.** 유니코드에
/// 좁다고 보장된 선 문자가 없다.
///
/// 우리 바이너리를 **따로 띄워** 두 정책으로 찍고 재는 이유: 폭 정책이 전역
/// 하나라서, 시험 안에서 뒤집으면 같이 돌는 다른 시험이 영향을 받는다.
#[test]
fn tables_line_up_under_both_width_policies() {
    if !needs_real_corpus("tables_line_up_under_both_width_policies") {
        return;
    }
    use hwp_core::wrap::display_width_with;
    let files = samples();
    if files.is_empty() {
        return;
    }
    let exe = workspace_root().join("target/release/hwp");
    if !exe.exists() {
        eprintln!("건너뜀 — target/release/hwp 가 없다 (cargo build --release)");
        return;
    }

    let mut problems = Vec::new();
    let mut checked = 0usize;
    for wide in [false, true] {
        // 넓은 쪽은 ASCII 로 그리고, 바깥 테두리는 `+===+` 로 안쪽 `+---+` 와
        // 구분된다 — 그래야 표가 어디서 끝나는지 알 수 있다(사람도 이 시험도).
        let frame = |l: &str| -> bool {
            let t = l.trim_start();
            if wide {
                t.starts_with("+=")
            } else {
                t.starts_with('┌') || t.starts_with('└')
            }
        };
        let inside = |l: &str| -> bool {
            let t = l.trim_start();
            match t.chars().next() {
                Some(c) => {
                    if wide {
                        (c == '+' || c == '|') && !t.starts_with("+=")
                    } else {
                        c == '│' || c == '├'
                    }
                }
                None => false,
            }
        };
        for f in &files {
            let out = std::process::Command::new(&exe)
                .args(["page", &f.to_string_lossy(), "--images", "none"])
                .env("HWP_AMBIGUOUS", if wide { "wide" } else { "narrow" })
                .output();
            let Ok(out) = out else { continue };
            let text = String::from_utf8_lossy(&out.stdout);

            // 표 하나 = 바깥 테두리로 시작해 바깥 테두리로 끝나는 연속 줄
            let mut cur: Vec<&str> = Vec::new();
            for line in text.lines() {
                if cur.is_empty() {
                    if frame(line) {
                        cur.push(line);
                    }
                    continue;
                }
                if frame(line) {
                    cur.push(line);
                    checked += 1;
                    let widths: std::collections::BTreeSet<usize> =
                        cur.iter().map(|l| display_width_with(l, wide)).collect();
                    if widths.len() > 1 {
                        problems.push(format!(
                            "{} ({}): 한 표의 줄 폭이 {:?} — {}",
                            f.display(),
                            if wide { "넓게" } else { "좁게" },
                            widths.iter().take(4).collect::<Vec<_>>(),
                            cur.get(1).unwrap_or(&"").trim().chars().take(40).collect::<String>()
                        ));
                    }
                    cur.clear();
                } else if inside(line) {
                    cur.push(line);
                } else {
                    cur.clear();
                }
            }
        }
    }
    assert!(checked > 100, "검사한 표가 {checked}개뿐이다");
    assert!(
        problems.is_empty(),
        "표가 어긋난다 ({}군데 / 검사 {checked}개):\n{}",
        problems.len(),
        problems.iter().take(6).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// **기계 쪽 입구가 사람 쪽과 같은 범위를 가지는가 — 끝까지.**
///
/// 전에는 단락 글자와 표 칸을 같이 고칠 길이 없었다. `set-para` 로 저장하고
/// `set-cell` 로 또 저장해야 했고, 저장이 두 번이면 중간에 실패한 문서가
/// 남는다. 사람은 둘 다 쌓아 두고 `ZZ` 를 한 번 누른다.
///
/// 그래서 **섞인 요청 하나**를 보내고, 저장된 파일을 **독립 증인**으로 다시
/// 읽어 셋이 다 들어갔는지 본다. 단락 수가 하나 늘었는지도 함께 본다 —
/// 단락을 넣는 연산은 뒤쪽 좌표를 밀므로, 밀린 셈이 틀리면 글자가 엉뚱한
/// 줄에 들어가고 그것은 겉보기로는 멀쩡해 보인다.
#[test]
fn the_machine_entry_point_mixes_edit_kinds_and_saves_once() {
    let exe = workspace_root().join("target/release/hwp");
    if !exe.exists() {
        eprintln!("건너뜀 — target/release/hwp 가 없다 (cargo build --release)");
        return;
    }
    // **문서를 손으로 고르지 않는다.** 전제를 코퍼스에 맞춰 적으면 그 파일이
    // 바뀔 때 시험이 거짓으로 통과하거나 엉뚱하게 터진다 — s13 을 박아 뒀다가
    // "표가 있는 문서여야 한다" 로 터졌다. 조건으로 찾는다: 셀이 있는 표가
    // 하나 이상이고 본문 단락이 넉넉한 것.
    let Some((src, before)) = samples().into_iter().find_map(|f| {
        let t = Testimony::read(&f).ok()?;
        let ok = t.body_count() >= 6
            && t.tables.iter().any(|tb| !tb.cells.is_empty())
            && t.at(0, 2).is_some()
            && t.at(0, 3).is_some();
        ok.then_some((f, t))
    }) else {
        eprintln!("건너뜀 — 표와 본문을 함께 가진 표본이 없다");
        return;
    };
    let n_before = before.body_count();
    let table = before
        .tables
        .iter()
        .find(|tb| !tb.cells.is_empty())
        .expect("셀이 있는 표를 위에서 골랐다");
    let cell = &table.cells[0];
    let (ti, cr, cc) = (table.index, cell.row, cell.col);

    // **확장자를 원본과 맞춘다.** 백엔드는 확장자로 형식을 정하므로 `.hwpx`
    // 를 `.hwp` 로 내보내면 변환이 일어난다 — 실제로 이 시험에서 당했고,
    // 자가검증이 "컨테이너 부품이 바뀌었다" 로 막아 주었다.
    let ext = src.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
    let dest = std::env::temp_dir().join(format!("hwp_api_mixed.{ext}"));
    let bak = std::env::temp_dir().join(format!("hwp_api_mixed.{ext}.bak"));
    let _ = std::fs::remove_file(&dest);
    let _ = std::fs::remove_file(&bak);

    let req = format!(
        r#"{{"file":"{}","output":"{}","ops":[
             {{"op":"set-para","section":0,"para":2,"text":"기계가 고친 단락"}},
             {{"op":"set-cell","table":{ti},"row":{cr},"col":{cc},"text":"기계가 쓴 칸"}},
             {{"op":"insert-para","section":0,"para":3,"text":"기계가 넣은 줄"}}
           ]}}"#,
        src.display(),
        dest.display()
    );
    let out = std::process::Command::new(&exe)
        .args(["api", "--compact", "-r", &req])
        .output()
        .expect("hwp api 를 띄운다");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "요청이 실패했다: {stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // 응답이 JSON 이고 ok 다 — 산문을 파싱하게 두지 않는다는 약속이다.
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("응답이 JSON 이어야 한다");
    assert_eq!(v["ok"], true, "{stdout}");
    assert_eq!(v["applied"].as_array().unwrap().len(), 3);
    assert!(v["saved"]["backup"].is_string(), "백업을 남기지 않았다");

    // 독립 증인으로 다시 읽는다 — 백엔드의 말을 그대로 믿지 않는다.
    let after = Testimony::read(&dest).expect("저장된 문서를 읽는다");
    assert_eq!(
        after.body_count(),
        n_before + 1,
        "단락이 하나 늘어야 한다 ({} → {})",
        n_before,
        after.body_count()
    );
    assert_eq!(
        after.at(0, 2).map(|p| hwp_core::offset::visible(&p.text)),
        Some("기계가 고친 단락".to_string())
    );
    assert_eq!(
        after.at(0, 3).map(|p| hwp_core::offset::visible(&p.text)),
        Some("기계가 넣은 줄".to_string())
    );
    let c = after
        .tables
        .iter()
        .find(|t| t.index == ti)
        .and_then(|t| t.cell(cr, cc))
        .map(|c| c.text.clone());
    assert_eq!(c.as_deref(), Some("기계가 쓴 칸"));

    let _ = std::fs::remove_file(&dest);
    let _ = std::fs::remove_file(&bak);
}

/// **`--dryRun` 은 디스크를 건드리지 않는다.** 쓰기 전에 좌표를 확인하는 길이
/// 없으면 기계는 틀린 좌표를 알아내기 위해 파일을 망가뜨려 봐야 한다.
#[test]
fn a_dry_run_reports_the_change_without_touching_the_file() {
    let exe = workspace_root().join("target/release/hwp");
    let src = workspace_root().join("samples/s13.hwp");
    if !exe.exists() || !src.exists() {
        return;
    }
    let sha_before = std::fs::read(&src).map(|b| {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(&b))
    });
    let req = format!(
        r#"{{"file":"{}","dryRun":true,"ops":[{{"op":"set-para","section":0,"para":2,"text":"안 쓴다"}}]}}"#,
        src.display()
    );
    let out = std::process::Command::new(&exe)
        .args(["api", "--compact", "-r", &req])
        .output()
        .expect("hwp api");
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("응답이 JSON 이어야 한다");
    assert_eq!(v["dryRun"], true);
    assert_eq!(v["changed"][0]["after"], "안 쓴다");
    assert!(v["saved"].is_null(), "미리보기가 저장했다");
    let sha_after = std::fs::read(&src).map(|b| {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(&b))
    });
    assert_eq!(
        sha_before.unwrap(),
        sha_after.unwrap(),
        "미리보기가 원본을 바꿨다"
    );
}

// ─────────────────────────── 가로세로 대조 ───────────────────────────
//
// 한 문서를 한 방식으로 보는 시험은 그 조합만 지킨다. 아래는 **축을 곱해서**
// 돈다 — 문서 × 읽는 방식 × 폭 정책 × 편집 종류. 2026-10-10 에 이 방식으로
// 여섯 개를 찾았고, 그중 둘(그림 조용히 삭제, 빈 단락 좌표 은폐)은 손으로 쓴
// 시험이 통과하고 있는 동안 살아 있었다.

/// **모든 그리기개체가 "개체가 달린 단락" 으로 세어지는가.**
///
/// 이 셈이 `delete-para` 의 관문이다 — 개체가 달린 단락을 지우면 그 표·그림이
/// 함께 사라지는데 우리는 개체를 만들거나 옮기지 못해 결과를 검증할 수 없다.
/// 세지 못하는 개체는 **막지 못하는 개체**다.
///
/// 이것이 틀렸던 모양: OWPML 읽는 쪽이 개체를 아예 세지 않아서 HWPX 문서 8개
/// 전부가 0개였다. 그래서 (1) 여백의 `◻`(못 그린 개체) 표시가 HWPX 에서 뜨지
/// 않았고 (2) 표가 달린 단락을 지우는 것이 HWPX 에서만 통과했다.
#[test]
fn every_drawing_object_is_counted_on_the_paragraph_that_holds_it() {
    if !needs_real_corpus("every_drawing_object_is_counted_on_the_paragraph_that_holds_it") {
        return;
    }
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }
    let mut failures = Vec::new();
    let mut checked = 0usize;
    for f in &files {
        let witness = Testimony::read(f).expect("증인");
        let drawings: BTreeMap<(u32, u32), usize> = witness
            .body_paragraphs()
            .into_iter()
            .filter_map(|p| {
                let i = p.index?;
                let n = p
                    .objects
                    .iter()
                    .filter(|&&o| o == hwp_core::witness::CTRL_DRAWING_OR_TABLE)
                    .count();
                (n > 0).then_some(((p.section, i), n))
            })
            .collect();

        // 표의 앵커 — 백엔드가 말한 자리
        let doc = match hwp_core::json::build(&rhwp, f, hwp_core::json::Profile::Doc) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for t in doc.tables.as_ref().map(|v| v.as_slice()).unwrap_or(&[]) {
            checked += 1;
            if drawings.get(&(t.section, t.para)).copied().unwrap_or(0) == 0 {
                failures.push(format!(
                    "{}: 표 {} 가 구역{} 단락{} 에 있는데 그 단락은 개체 0개로 세어졌다",
                    f.display(),
                    t.id,
                    t.section,
                    t.para
                ));
            }
        }
        // 표 칸 **밖**의 그림 — 본문 단락에 직접 달린 것
        for img in hwp_core::images::read(f).unwrap_or_default() {
            let Some(at) = img.at else { continue };
            if img.cell.is_some() {
                continue;
            }
            checked += 1;
            if drawings.get(&at).copied().unwrap_or(0) == 0 {
                failures.push(format!(
                    "{}: 그림 {} 이 구역{} 단락{} 에 있는데 그 단락은 개체 0개다",
                    f.display(),
                    img.name,
                    at.0,
                    at.1
                ));
            }
        }
    }
    assert!(checked > 40, "앵커를 {checked}개밖에 못 봤다 — 시험이 비었다");
    assert!(failures.is_empty(), "개체를 못 센 자리:\n  {}", failures.join("\n  "));
}

/// **개체가 달린 단락은 지울 수 없다 — 그리고 `dryRun` 이 미리 말한다.**
///
/// 전에는 두 갈래로 갈렸고 둘 다 나빴다. 그림이 달린 단락은 저장이 **성공하고
/// 그림이 조용히 사라졌다**(s00.hwp 단락 54, 그림 6→5개). 표가 달린 단락은 셀
/// 수가 안 맞아 저장은 멈췄지만 "표0 (0,0) 의 글자가 다르다" 같은 엉뚱한 차이를
/// 쏟아냈고, `dryRun` 은 "괜찮다" 고 말했다.
#[test]
fn deleting_a_paragraph_that_holds_a_drawing_is_refused_before_anything_is_written() {
    if !needs_real_corpus("deleting_a_paragraph_that_holds_a_drawing_is_refused_before_anything_is_written") {
        return;
    }
    let exe = workspace_root().join("target/release/hwp");
    if !exe.exists() {
        eprintln!("건너뜀 — target/release/hwp 가 없다");
        return;
    }
    let files = samples();
    let mut tried = 0usize;
    for f in &files {
        let witness = match Testimony::read(f) {
            Ok(w) => w,
            Err(_) => continue,
        };
        // 개체가 달린 본문 단락 하나
        let Some(at) = witness.body_paragraphs().into_iter().find_map(|p| {
            let i = p.index?;
            let has = p
                .objects
                .iter()
                .any(|&o| o == hwp_core::witness::CTRL_DRAWING_OR_TABLE);
            has.then_some((p.section, i))
        }) else {
            continue;
        };
        tried += 1;

        let before = std::fs::read(f).expect("원본");
        let req = format!(
            r#"{{"file":"{}","dryRun":true,"ops":[{{"op":"delete-para","section":{},"para":{}}}]}}"#,
            f.display(),
            at.0,
            at.1
        );
        let out = std::process::Command::new(&exe)
            .args(["api", "--compact", "-r", &req])
            .output()
            .expect("hwp api");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            !out.status.success(),
            "{}: 구역{} 단락{} 에 개체가 있는데 미리보기가 통과시켰다 — {text}",
            f.display(),
            at.0,
            at.1
        );
        assert!(
            text.contains("그리기개체"),
            "{}: 거부는 했지만 이유를 말하지 않았다 — {text}",
            f.display()
        );
        // 미리보기는 원본을 건드리지 않는다
        assert_eq!(before, std::fs::read(f).expect("원본"), "{}: 원본이 바뀌었다", f.display());
    }
    assert!(tried >= 5, "개체가 달린 단락을 {tried}개밖에 못 찾았다 — 시험이 비었다");
}

/// **그림을 그리는 글자가 폭 정책을 따르는가.**
///
/// 점자(U+28xx)와 반블록(`▀▄█`)은 선 문자와 똑같이 East Asian **Ambiguous** 다.
/// 넓게 그리는 터미널은 한 글리프를 두 칸으로 내는데 우리는 한 칸으로 셈했다 —
/// 실측(s00.hwp 2쪽, 70칸): 바깥 테두리는 40칸인데 그림이 든 줄은 26~35칸으로
/// 잘려 오른쪽 세로줄이 사라졌다. **표 안의 그림이 표를 부순다.**
#[test]
fn image_art_never_uses_a_glyph_wider_than_one_cell() {
    if !needs_real_corpus("image_art_never_uses_a_glyph_wider_than_one_cell") {
        return;
    }
    let files = samples();
    if files.is_empty() {
        return;
    }
    let mut checked = 0usize;
    for wide in [false, true] {
        hwp_core::wrap::set_ambiguous_wide(wide);
        for f in &files {
            for img in hwp_core::images::read(f).unwrap_or_default().into_iter().take(3) {
                for want in [hwp_core::dots::Glyphs::Braille, hwp_core::dots::Glyphs::Blocks] {
                    let opts = hwp_core::dots::Options { glyphs: want, ..Default::default() };
                    let Some(art) = hwp_core::dots::render_with(&img.bytes, 30, 12, opts) else {
                        continue;
                    };
                    checked += 1;
                    for line in &art.lines {
                        for c in line.chars() {
                            assert_eq!(
                                hwp_core::wrap::char_width(c),
                                1,
                                "{} ({}): {:?}({c:?}) 가 넓은 글자인데 그림에 쓰였다 \
                                 — 표 안에 그리면 표가 어긋난다",
                                f.display(),
                                if wide { "넓게" } else { "좁게" },
                                art.glyphs
                            );
                        }
                    }
                    // 줄 폭이 실제로 요청한 칸 수를 넘지 않는가
                    for line in &art.lines {
                        assert!(
                            hwp_core::wrap::display_width(line) <= 30,
                            "{}: 그림 줄이 {}칸으로 30칸을 넘었다",
                            f.display(),
                            hwp_core::wrap::display_width(line)
                        );
                    }
                }
            }
        }
    }
    hwp_core::wrap::set_ambiguous_wide(false);
    assert!(checked > 20, "그림을 {checked}개밖에 못 그렸다 — 시험이 비었다");
}

/// **넓게 그리는 터미널에서 우리 장식이 줄을 밀지 않는가.**
///
/// 이 프로그램은 폭을 `wrap::char_width` 로 재지만 ratatui 는 언제나
/// `unicode-width` 의 좁은 폭(Ambiguous = 1)으로 잰다. 둘이 어긋나는 글자를
/// 화면에 내면 넓은 터미널에서 줄이 밀린다.
///
/// 실측(2026-10-10, 문서 6 × 크기 7 × 키 12 × 폭정책 2 = 23,040줄): 넘치는 줄이
/// **1,070개**였고 대부분이 덮개(도움·메뉴)의 유니코드 테두리와 안내줄의
/// `· → ↓` 였다. 표는 이미 ASCII(`+ - |`)로 바꿔 두었는데 **나머지 장식은 그
/// 결정을 못 받았다** — 가로세로로 곱해 보기 전까지 아무 시험도 그걸 보지
/// 않았다.
///
/// 두 가지를 본다. 선 문자는 **어디에도** 없어야 하고(문서 글자에 나오지
/// 않는다), 머리글·상태줄·도움줄은 Ambiguous 를 **하나도** 써서는 안 된다
/// (그 세 줄에는 본문이 오지 않으므로 전부 우리가 고른 글자다).
#[test]
fn in_a_wide_terminal_our_own_chrome_never_shifts_a_line() {
    if !needs_real_corpus("in_a_wide_terminal_our_own_chrome_never_shifts_a_line") {
        return;
    }
    let exe = workspace_root().join("target/release/hwp");
    if !exe.exists() {
        eprintln!("건너뜀 — target/release/hwp 가 없다");
        return;
    }
    let files = samples();
    if files.is_empty() {
        return;
    }

    // 선 문자 — 유니코드에 좁다고 보장된 것이 없어 ASCII 로 그리기로 했다
    const LINES: &str = "─│┌┐└┘├┤┬┴┼━┃";
    let ambiguous = |c: char| {
        hwp_core::wrap::char_width_with(c, true) != hwp_core::wrap::char_width_with(c, false)
    };

    let mut failures = Vec::new();
    let mut rows_seen = 0usize;
    // 덮개를 띄우는 키까지 함께 — 도움(`?`)·메뉴(`m`)·표(`T`)·개요(`t`)
    for keys in ["", "?", "m", "T", "t", "i"] {
        for f in files.iter().take(6) {
            let out = std::process::Command::new(&exe)
                .args([
                    "api",
                    "--compact",
                    "-r",
                    &format!(
                        r#"{{"file":"{}","frame":"72x16","keys":"{keys}"}}"#,
                        f.display()
                    ),
                ])
                .env("HWP_AMBIGUOUS", "wide")
                .output();
            let Ok(out) = out else { continue };
            if !out.status.success() {
                continue;
            }
            let v: serde_json::Value = match serde_json::from_slice(&out.stdout) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let frame: Vec<String> = v["frame"]
                .as_array()
                .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            if frame.is_empty() {
                continue;
            }
            let last = frame.len() - 1;
            for (y, line) in frame.iter().enumerate() {
                rows_seen += 1;
                if let Some(c) = line.chars().find(|c| LINES.contains(*c)) {
                    failures.push(format!(
                        "{} (keys={keys:?}) 줄{y}: 선 문자 {c:?} — 넓은 터미널에서 두 칸이 된다\n    {line}",
                        f.display()
                    ));
                }
                // 머리글(0) · 상태줄 · 도움줄 — 본문이 오지 않는 줄
                if y == 0 || y + 2 >= last + 1 {
                    if let Some(c) = line.chars().find(|c| ambiguous(*c) && *c != ' ') {
                        failures.push(format!(
                            "{} (keys={keys:?}) 줄{y}: 장식에 Ambiguous {c:?} (U+{:04X})\n    {line}",
                            f.display(),
                            c as u32
                        ));
                    }
                }
            }
        }
    }
    assert!(rows_seen > 300, "줄을 {rows_seen}개밖에 못 봤다 — 시험이 비었다");
    assert!(
        failures.is_empty(),
        "넓은 터미널에서 밀릴 장식 {}건:\n  {}",
        failures.len(),
        failures.iter().take(8).cloned().collect::<Vec<_>>().join("\n  ")
    );
}

/// **모든 문서 × 모든 읽는 이름.** 한 조합이 터지거나 비어 오지 않는가.
///
/// 읽는 이름이 여덟 개(`text` `outline` `tables` `styles` `deco` `images`
/// `levels` `all`)인데 손으로 쓴 시험은 늘 한둘만 본다. 형식(hwp5/hwpx)마다
/// 읽는 길이 다르므로 **곱해서** 봐야 한 쪽만 죽은 것을 알 수 있다 — OWPML 이
/// 개체를 하나도 세지 않던 것이 그런 모양이었다.
#[test]
fn every_document_answers_every_read_name() {
    if !needs_real_corpus("every_document_answers_every_read_name") {
        return;
    }
    let exe = workspace_root().join("target/release/hwp");
    if !exe.exists() {
        eprintln!("건너뜀 — target/release/hwp 가 없다");
        return;
    }
    let files = samples();
    if files.is_empty() {
        return;
    }
    // 이름 → 응답에서 그 이름으로 와야 하는 칸
    const NAMES: &[&str] =
        &["text", "outline", "tables", "styles", "deco", "images", "levels", "all"];

    let mut failures = Vec::new();
    let mut asked = 0usize;
    for f in &files {
        for name in NAMES {
            asked += 1;
            let out = std::process::Command::new(&exe)
                .args([
                    "api",
                    "--compact",
                    "-r",
                    &format!(r#"{{"file":"{}","read":["{name}"]}}"#, f.display()),
                ])
                .output();
            let Ok(out) = out else { continue };
            let text = String::from_utf8_lossy(&out.stdout);
            if !out.status.success() {
                failures.push(format!("{} read={name}: {}", f.display(), text.trim()));
                continue;
            }
            let v: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    failures.push(format!("{} read={name}: JSON 이 아니다 ({e})", f.display()));
                    continue;
                }
            };
            if v["ok"] != serde_json::json!(true) {
                failures.push(format!("{} read={name}: ok 가 아니다", f.display()));
                continue;
            }
            // 모든 문서에 있어야 하는 것: 본문 단락은 **빈 단락도** 하나는 있다
            let key = if *name == "text" { "body" } else { *name };
            if *name == "text" || *name == "all" {
                let n = v["body"].as_array().map(|a| a.len()).unwrap_or(0);
                if n == 0 {
                    failures.push(format!(
                        "{} read={name}: 본문이 비었다 — 빈 단락도 좌표를 가진다",
                        f.display()
                    ));
                }
            } else if v.get(key).is_none() {
                failures.push(format!("{} read={name}: `{key}` 칸이 없다", f.display()));
            }
        }
    }
    assert!(asked > 50, "{asked}개만 물었다 — 시험이 비었다");
    assert!(failures.is_empty(), "읽기 실패:\n  {}", failures.join("\n  "));
}

/// **모든 편집 종류 × 두 형식.** 저장까지 가고, 증인이 다시 읽어 확인되는가.
///
/// 종류가 일곱이고 형식이 둘인데, 손으로 쓴 시험은 보통 `set-cell` 하나를
/// 본다. hwp5 와 hwpx 는 읽는 길이 다르고(CFB 레코드 vs OWPML) 쓰는 길도
/// 백엔드 안에서 갈린다 — 한 쪽만 되는 것을 알려면 곱해야 한다.
#[test]
fn every_edit_kind_lands_on_both_formats() {
    let exe = workspace_root().join("target/release/hwp");
    if !exe.exists() {
        eprintln!("건너뜀 — target/release/hwp 가 없다");
        return;
    }
    // hwp5 하나, hwpx 하나 — 표가 있고 글자가 든 단락이 있는 것
    let pick = |want_ext: &str| -> Option<(PathBuf, Testimony)> {
        samples().into_iter().find_map(|f| {
            if f.extension().and_then(|e| e.to_str()) != Some(want_ext) {
                return None;
            }
            let t = Testimony::read(&f).ok()?;
            let ok = t.tables.iter().any(|tb| !tb.cells.is_empty())
                && t.body_paragraphs().iter().filter(|p| plain_para(p)).count() >= 3;
            ok.then_some((f, t))
        })
    };

    let mut tried = 0usize;
    let mut formats = 0usize;
    for ext in ["hwp", "hwpx"] {
        let Some((src, witness)) = pick(ext) else {
            eprintln!("건너뜀 — .{ext} 표본이 없다");
            continue;
        };
        formats += 1;
        // **개체가 없고 글자가 있는** 단락을 고른다. 개체가 달린 단락은
        // 일부러 못 지우게 되어 있다.
        let targets: Vec<(u32, u32, String)> = witness
            .body_paragraphs()
            .into_iter()
            .filter(|p| plain_para(p))
            .filter_map(|p| {
                let i = p.index?;
                Some((p.section, i, hwp_core::offset::visible(&p.text)))
            })
            .take(3)
            .collect();
        let (sec, para, text) = targets[0].clone();
        let n = text.chars().count();
        let table = witness.tables.iter().find(|t| !t.cells.is_empty()).expect("표");
        let cell = &table.cells[0];

        let cases: Vec<(&str, String, String)> = vec![
            ("set-para", format!(r#"{{"op":"set-para","section":{sec},"para":{para},"text":"가로세로"}}"#), "가로세로".into()),
            ("insert-text", format!(r#"{{"op":"insert-text","section":{sec},"para":{para},"offset":0,"text":"앞"}}"#), format!("앞{text}")),
            ("insert-text-end", format!(r#"{{"op":"insert-text","section":{sec},"para":{para},"offset":{n},"text":"뒤"}}"#), format!("{text}뒤")),
            ("delete-text", format!(r#"{{"op":"delete-text","section":{sec},"para":{para},"offset":0,"count":1}}"#), text.chars().skip(1).collect()),
            ("insert-para", format!(r#"{{"op":"insert-para","section":{sec},"para":{para},"text":"넣은 줄"}}"#), "넣은 줄".into()),
            // 지운 뒤 그 자리에 오는 것은 **문서 순서로 바로 다음 단락**이다.
            // 글자가 든 다음 단락이 아니다 — 처음에 그렇게 적었다가 빈 단락이
            // 올라와서 틀렸다.
            ("delete-para", format!(r#"{{"op":"delete-para","section":{sec},"para":{para}}}"#),
             witness.at(sec, para + 1).map(|p| hwp_core::offset::visible(&p.text)).unwrap_or_default()),
            ("set-cell", format!(r#"{{"op":"set-cell","table":{},"row":{},"col":{},"text":"칸 값"}}"#, table.index, cell.row, cell.col), "칸 값".into()),
        ];

        for (name, op, want) in cases {
            tried += 1;
            let dest = std::env::temp_dir().join(format!("hwp_kind_{name}.{ext}"));
            let bak = std::env::temp_dir().join(format!("hwp_kind_{name}.{ext}.bak"));
            let _ = std::fs::remove_file(&dest);
            let _ = std::fs::remove_file(&bak);
            std::fs::copy(&src, &dest).expect("복사");

            let out = std::process::Command::new(&exe)
                .args([
                    "api",
                    "--compact",
                    "-r",
                    &format!(r#"{{"file":"{}","ops":[{op}]}}"#, dest.display()),
                ])
                .output()
                .expect("hwp api");
            let said = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success(),
                "{} {name}: 저장이 실패했다 — {said}",
                src.display()
            );

            // **증인이 다시 읽어** 확인한다 — 백엔드의 말을 그대로 믿지 않는다
            let after = Testimony::read(&dest).expect("저장본");
            let got = if name == "set-cell" {
                after
                    .tables
                    .iter()
                    .find(|t| t.index == table.index)
                    .and_then(|t| t.cell(cell.row, cell.col))
                    .map(|c| c.text.clone())
            } else {
                after.at(sec, para).map(|p| hwp_core::offset::visible(&p.text))
            };
            assert_eq!(
                got.as_deref().map(str::trim),
                Some(want.trim()),
                "{} {name}: 저장본의 글자가 기대와 다르다",
                src.display()
            );
            let _ = std::fs::remove_file(&dest);
            let _ = std::fs::remove_file(&bak);
        }
    }
    // **두 형식이 다 돌았는지 못으로 박는다.** `tried >= 7` 만 보면 한 형식이
    // 조용히 빠져도 통과한다 — 그러면 "두 형식 × 모든 종류" 라는 이름이
    // 거짓이 된다.
    assert_eq!(formats, 2, "형식 {formats}개만 돌았다 (hwp·hwpx 둘 다 있어야 한다)");
    assert_eq!(tried, 14, "{tried}개만 해 봤다 — 종류 7 × 형식 2 여야 한다");
}

/// 개체가 없고 글자가 든 본문 단락 — 편집 시험이 안전하게 건드릴 수 있는 자리.
fn plain_para(p: &hwp_core::witness::Para) -> bool {
    p.index.is_some()
        && !p
            .objects
            .iter()
            .any(|&o| o == hwp_core::witness::CTRL_DRAWING_OR_TABLE)
        && !hwp_core::offset::visible(&p.text).trim().is_empty()
}

/// **다른 이름으로 저장이 원본을 건드리지 않는가.**
///
/// 사람 쪽 `:w <경로>`·`:saveas <경로>` 와 기계 쪽 `api` 의 `output` 이 같은
/// 함수(`edit::save_as`)를 탄다. 네 가지를 본다 — 원본이 그대로인지, 사본에
/// 편집이 들어갔는지, **고친 것이 없어도 사본이 나오는지**(처음에 "저장할
/// 편집이 없다" 로 거부해서 파일이 안 생겼다), 그리고 검증에 실패하면
/// **만들던 파일을 남기지 않는지**.
#[test]
fn saving_under_another_name_leaves_the_original_alone() {
    use hwp_core::edit::Op;
    let Some((rhwp, _)) = backend() else { return };
    let Some((src, witness)) = samples().into_iter().find_map(|f| {
        let t = Testimony::read(&f).ok()?;
        let ok = f.extension().and_then(|e| e.to_str()) == Some("hwp")
            && t.body_paragraphs().iter().filter(|p| plain_para(p)).count() >= 2;
        ok.then_some((f, t))
    }) else {
        return;
    };
    let before = std::fs::read(&src).expect("원본");
    let (sec, para, text) = witness
        .body_paragraphs()
        .into_iter()
        .filter(|p| plain_para(p))
        .find_map(|p| Some((p.section, p.index?, hwp_core::offset::visible(&p.text))))
        .expect("글자가 든 단락");

    let dir = std::env::temp_dir().join("hwp_saveas");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("임시 폴더");

    // 1) 편집을 담아 다른 이름으로
    let dest = dir.join("사본.hwp");
    let mut log = hwp_core::edit::EditLog::new();
    log.push(Op::InsertText { at: (sec, para), offset: 0, text: "앞".into() });
    let report = hwp_core::edit::save_as(&rhwp, &src, &dest, &log).expect("다른 이름으로 저장");
    assert_eq!(report.ops_applied, 1);

    // 원본은 **한 바이트도** 달라지지 않았다
    assert_eq!(before, std::fs::read(&src).expect("원본"), "원본이 바뀌었다");
    // 사본에는 편집이 들어갔다 — 증인이 다시 읽어 확인한다
    let after = Testimony::read(&dest).expect("사본");
    assert_eq!(
        after.at(sec, para).map(|p| hwp_core::offset::visible(&p.text)),
        Some(format!("앞{text}")),
    );

    // 2) 고친 것이 없어도 사본은 나온다 (바이트가 같다)
    let plain = dir.join("그대로.hwp");
    let r = hwp_core::edit::save_as(&rhwp, &src, &plain, &hwp_core::edit::EditLog::new())
        .expect("편집이 없어도 사본은 나와야 한다");
    assert_eq!(r.ops_applied, 0);
    assert_eq!(
        std::fs::read(&plain).expect("사본"),
        before,
        "편집이 없는 사본인데 바이트가 다르다"
    );

    // 3) 확장자가 다르면 거부하고 **아무것도 만들지 않는다**
    let other = dir.join("엉뚱.hwpx");
    let e = hwp_core::edit::save_as(&rhwp, &src, &other, &log).expect_err("변환은 거부해야 한다");
    assert!(format!("{e}").contains("확장자"), "{e}");
    assert!(!other.exists(), "거부했는데 파일이 남았다");

    // 4) 검증에 실패하면 만들던 파일을 치운다
    let bad = dir.join("실패.hwp");
    let mut impossible = hwp_core::edit::EditLog::new();
    impossible.push(Op::SetCell { table: 9999, row: 0, col: 0, lines: vec!["없는 표".into()] });
    let e = hwp_core::edit::save_as(&rhwp, &src, &bad, &impossible)
        .expect_err("없는 셀에 쓰는 것은 실패해야 한다");
    assert!(!bad.exists(), "실패했는데 반쯤 된 파일이 남았다: {e}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **코퍼스가 쓰는 기호를 문자표가 담고 있는가.**
///
/// 기호 표는 "이 문서들이 실제로 쓰는 것" 을 세어서 짰다(2026-10-11, 서로 다른
/// 기호 57개). 그 근거가 유지되는지 본다 — 코퍼스에 새 문서를 넣었는데 그
/// 문서가 쓰는 글머리를 표에서 고를 수 없으면, 사람은 그걸 넣을 길이 없다.
///
/// 전부를 요구하지는 않는다. 담지 않기로 한 것들이 있다:
/// 사용자 영역(한컴 글꼴 자체 글립), 공백·마침표의 변종(`U+2007`, `U+2024`),
/// 그리고 한 번만 나오는 꼬리들. **두 번 이상 나온 기호**를 요구한다.
#[test]
fn the_symbol_table_covers_what_the_corpus_actually_uses() {
    if !needs_real_corpus("the_symbol_table_covers_what_the_corpus_actually_uses") {
        return;
    }
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }

    let mut seen: BTreeMap<char, usize> = BTreeMap::new();
    for f in &files {
        let Ok(doc) = hwp_core::json::build(&rhwp, f, hwp_core::json::Profile::Doc) else {
            continue;
        };
        let mut texts: Vec<String> = Vec::new();
        for b in doc.body.as_ref().map(|v| v.as_slice()).unwrap_or(&[]) {
            if let hwp_core::json::BodyItem::Para { text, .. } = b {
                texts.push(text.clone());
            }
        }
        for t in doc.tables.as_ref().map(|v| v.as_slice()).unwrap_or(&[]) {
            for row in &t.grid {
                texts.extend(row.iter().cloned());
            }
        }
        for s in texts {
            for c in s.chars() {
                let o = c as u32;
                if o < 128 {
                    continue;
                }
                // 한글 음절·자모는 두벌식이 만든다
                if (0xAC00..=0xD7A3).contains(&o)
                    || (0x1100..=0x11FF).contains(&o)
                    || (0x3130..=0x318F).contains(&o)
                {
                    continue;
                }
                // 한자는 문자표의 일이 아니다 (따로 다룰 문제다)
                if (0x4E00..=0x9FFF).contains(&o) {
                    continue;
                }
                // 사용자 영역 — 한컴 글꼴 자체 글립. 넣을 기호가 아니다
                if (0xE000..=0xF8FF).contains(&o) || (0xF0000..=0x10FFFD).contains(&o) {
                    continue;
                }
                // 공백·마침표의 변종 — 골라 넣는 것이 아니다
                if matches!(c, '\u{2007}' | '\u{2024}' | '\u{00A0}') {
                    continue;
                }
                // 글자처럼 쓰이는 수정 문자(`ʻ ʼ ˮ`)도 고를 것이 아니다
                if unicode_is_modifier(c) {
                    continue;
                }
                *seen.entry(c).or_default() += 1;
            }
        }
    }

    let have: std::collections::BTreeSet<char> = hwp_core::symbols::GROUPS
        .iter()
        .flat_map(|(_, items)| items.iter().filter_map(|(s, _)| s.chars().next()))
        .collect();

    let missing: Vec<String> = seen
        .iter()
        .filter(|(_, n)| **n >= 2)
        .filter(|(c, _)| !have.contains(c))
        .map(|(c, n)| format!("{c:?} U+{:04X} ({n}회)", *c as u32))
        .collect();

    assert!(seen.len() > 20, "기호를 {}개만 세었다 — 시험이 비었다", seen.len());
    assert!(
        missing.is_empty(),
        "코퍼스가 두 번 이상 쓰는데 문자표에 없는 기호 {}개:\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
}

/// 수정 문자(Lm·Sk) — 글자에 붙어 쓰이고 골라 넣을 것이 아니다.
fn unicode_is_modifier(c: char) -> bool {
    matches!(c, '\u{02B0}'..='\u{02FF}' | '\u{1AB0}'..='\u{1AFF}')
}

/// **표를 만들 수 있는가 — 그리고 번호 밀림을 맞게 예측하는가.**
///
/// 백엔드는 표와 **함께 단락 하나를 넣고**, 그 자리부터의 **표 번호를 +1**
/// 한다(실측: s16.hwp 단락3 에 넣으니 t3 이 새 표가 되고 옛 t3·t4·t5 가
/// t4·t5·t6 이 되었다). 우리 자가검증은 그 셈을 미리 해야 한다 — 틀리면
/// 저장이 멈추고, 그게 안전장치다. 여기서는 **맞게 세는지**를 본다.
#[test]
fn a_new_table_lands_and_the_renumbering_is_predicted_correctly() {
    use hwp_core::edit::{EditLog, Op, save_as};
    let Some((rhwp, _)) = backend() else { return };

    // 표가 여럿이고 그 중간에 넣을 수 있는 문서
    let Some((src, doc)) = samples().into_iter().find_map(|f| {
        let d = hwp_core::json::build(&rhwp, &f, hwp_core::json::Profile::Doc).ok()?;
        let n = d.tables.as_ref().map(|t| t.len()).unwrap_or(0);
        (n >= 4 && f.extension().and_then(|e| e.to_str()) == Some("hwp")).then_some((f, d))
    }) else {
        eprintln!("건너뜀 — 표 4개 이상인 .hwp 표본이 없다");
        return;
    };
    let tables = doc.tables.as_ref().expect("표");
    let anchors: Vec<(u32, u32, u32)> = {
        let mut v: Vec<(u32, u32, u32)> = tables
            .iter()
            .map(|t| (t.id.trim_start_matches('t').parse().unwrap_or(0), t.section, t.para))
            .collect();
        v.sort();
        v
    };
    // 표가 **중간에** 끼도록, 맨 앞 표의 뒤이면서 마지막 표의 앞인 자리
    let first_after = anchors.iter().map(|(_, _, p)| *p).max().expect("앵커");
    let at = (0u32, (first_after.saturating_sub(1)).max(1));
    let index = anchors.iter().filter(|(_, s, p)| (*s, *p) < at).count() as u32;

    let dir = std::env::temp_dir().join("hwp_newtable");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("임시 폴더");
    let dest = dir.join("new.hwp");

    let mut log = EditLog::new();
    log.push(Op::InsertTable { at, rows: 3, cols: 4, index });
    // **자가검증을 통과해야 한다.** 번호를 잘못 세면 여기서 멈춘다.
    let report = save_as(&rhwp, &src, &dest, &log).unwrap_or_else(|e| {
        panic!("{}: 단락{} 에 표를 넣다 멈췄다 (표{index} 로 예측) — {e:#}", src.display(), at.1)
    });
    assert_eq!(report.ops_applied, 1);

    // 증인이 다시 읽어 확인한다
    let after = hwp_core::json::build(&rhwp, &dest, hwp_core::json::Profile::Doc)
        .expect("저장본");
    let at_tables = after.tables.as_ref().expect("표");
    assert_eq!(at_tables.len(), tables.len() + 1, "표가 하나 늘어야 한다");
    let made = at_tables
        .iter()
        .find(|t| t.id == format!("t{index}"))
        .unwrap_or_else(|| panic!("표{index} 가 없다 — 번호 예측이 틀렸다"));
    assert_eq!((made.rows, made.cols), (3, 4), "크기가 다르다");
    assert_eq!(made.para, at.1, "엉뚱한 단락에 생겼다");
    assert!(
        made.grid.iter().flatten().all(|c| c.trim().is_empty()),
        "새 표가 비어 있지 않다"
    );

    // **옛 표들의 내용이 그대로 옮겨 갔는가** — 번호만 밀려야 한다
    for (old_i, _, _) in &anchors {
        let new_i = if *old_i >= index { old_i + 1 } else { *old_i };
        let was = tables.iter().find(|t| t.id == format!("t{old_i}")).expect("옛 표");
        let now = at_tables
            .iter()
            .find(|t| t.id == format!("t{new_i}"))
            .unwrap_or_else(|| panic!("옛 표{old_i} 가 표{new_i} 로 안 갔다"));
        assert_eq!(was.grid, now.grid, "옛 표{old_i} 의 내용이 달라졌다");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// **컴파일 시점에 박아 넣는 파일이 git 에 들어 있는가.**
///
/// 2026-10-11: 한자 표를 `vendor/` 에 두었는데 그 자리는 `.gitignore` 에
/// 있다(`fetch-rhwp.sh` 가 받아 오는 자리다). `git add -A` 가 **조용히
/// 빼놓았고**, 커밋은 통과했고, 이 기계에서는 파일이 있어서 빌드도 됐다 —
/// **새로 클론한 기계에서만** `include_str!` 이 컴파일 시점에 터졌다(맥에서
/// 보고받았다).
///
/// 그 뒤로는 깨끗한 클론에서 빌드해 확인하지만, 그건 내가 기억해야 하는 일이다.
/// 여기서 못으로 박는다: `include_str!`·`include_bytes!` 가 가리키는 파일은
/// **git 이 추적하고 있어야** 한다.
#[test]
fn every_embedded_file_is_tracked_by_git() {
    let root = workspace_root();
    // git 저장소가 아니면(배포 타르볼 등) 볼 것이 없다
    let tracked = std::process::Command::new("git")
        .args(["-C", &root.to_string_lossy(), "ls-files"])
        .output();
    let Ok(out) = tracked else { return };
    if !out.status.success() {
        return;
    }
    let tracked: std::collections::BTreeSet<String> =
        String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect();
    assert!(tracked.len() > 10, "git 이 파일을 {}개만 센다", tracked.len());

    // **찾는 글자를 소스에 그대로 적지 않는다.** 적으면 이 시험 파일이
    // 스스로에게 걸린다 — 처음에 그렇게 터졌다.
    let needles = [format!("include_{}!(", "str"), format!("include_{}!(", "bytes")];

    let mut checked = 0usize;
    let mut missing = Vec::new();
    for src in glob_rs(&root.join("crates")) {
        let text = match std::fs::read_to_string(&src) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for (kind, rest) in text.split_inclusive(')').filter_map(|chunk| {
            for k in &needles {
                if let Some(i) = chunk.find(k.as_str()) {
                    return Some((k.clone(), &chunk[i + k.len()..]));
                }
            }
            None
        }) {
            let Some(path) = rest.split('"').nth(1) else { continue };
            checked += 1;
            // `include_str!` 의 경로는 **그 소스 파일 기준**이다
            let abs = src.parent().expect("부모").join(path);
            let Ok(abs) = abs.canonicalize() else {
                missing.push(format!("{} 가 가리키는 {path} 가 없다", src.display()));
                continue;
            };
            let Ok(rel) = abs.strip_prefix(&root) else { continue };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if !tracked.contains(&rel) {
                missing.push(format!(
                    "{} 의 {kind}{path:?} → `{rel}` 이 git 에 없다 \
                     (.gitignore 에 걸렸을 것이다 — 새 클론에서 빌드가 깨진다)",
                    src.display()
                ));
            }
        }
    }
    assert!(checked >= 2, "박아 넣는 파일을 {checked}개만 봤다 — 시험이 비었다");
    assert!(missing.is_empty(), "{}", missing.join("\n  "));
}

/// `crates/` 아래의 `.rs` 파일들.
fn glob_rs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            out.extend(glob_rs(&p));
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
    out
}

/// **그린 표의 열 비율이 문서와 같은가.**
///
/// 2026-10-12 보고: "실제 문서의 표 크기와 상동하지 않는다." 원인은 열 폭을
/// **내용 길이로 짐작한** 것이었다 — 긴 글이 든 열이 넓어지므로 문서가 20:80
/// 으로 적어 둔 2열 표가 거꾸로 그려졌다(s02.hwp 표20, 71.7%p 차이).
///
/// 조판값은 표의 **전체** 폭만 준다(`dump-pages` 의 `table.widthPx`). 열별
/// 폭은 증인이 셀 레코드에서 읽는다 — HWP5 는 `LIST_HEADER` payload 의 16·20
/// 바이트, OWPML 은 `hp:cellSz` 다.
///
/// 전수 측정(코퍼스 표 126개): 열 비율 차이 평균 **16.35%p → 1.77%p**.
#[test]
fn a_drawn_table_keeps_the_column_proportions_of_the_document() {
    if !needs_real_corpus("a_drawn_table_keeps_the_column_proportions_of_the_document") {
        return;
    }
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }
    let mut n = 0usize;
    let mut total_err = 0.0f64;
    let mut bad = Vec::new();
    for f in &files {
        let Ok(doc) = hwp_core::screen::ScreenDoc::build(&rhwp, f) else { continue };
        let column = doc.column.cells;
        for t in &doc.tables {
            let Some(want) = t.table.doc_column_widths() else { continue };
            let got = t.table.cell_widths_for_test(column);
            if got.len() != want.len() {
                continue;
            }
            let wsum: f64 = want.iter().map(|w| *w as f64).sum();
            let gsum: f64 = got.iter().map(|w| *w as f64).sum();
            if wsum == 0.0 || gsum == 0.0 {
                continue;
            }
            n += 1;
            // 열마다 비율 차이 (백분율 포인트) 중 가장 큰 것
            let mut mx = 0.0f64;
            // **바닥에 걸린 표는 세지 않는다.** 터미널에서 4칸보다 좁은 열은
            // 글자를 읽을 수 없어 바닥을 지키고, 그러면 비율이 깨진다 —
            // 어긋난 표보다 읽히는 표가 낫다는 판단이고 터미널의 한계다.
            let floored = got.iter().any(|g| *g <= 4);
            for (w, g) in want.iter().zip(got.iter()) {
                let a = *w as f64 / wsum * 100.0;
                let b = *g as f64 / gsum * 100.0;
                mx = mx.max((a - b).abs());
            }
            total_err += mx;
            if !floored && mx > 6.0 {
                bad.push(format!(
                    "{} 표{} ({}열): 가장 큰 차이 {mx:.1}%p — 문서 {:?} / 그린 것 {:?}",
                    f.display(),
                    t.index,
                    want.len(),
                    want,
                    got
                ));
            }
        }
    }
    assert!(n > 80, "표를 {n}개만 봤다 — 시험이 비었다");
    let avg = total_err / n as f64;
    eprintln!("표 {n}개 · 열 비율 차이 평균 {avg:.2}%p");
    // 내용으로 짐작하던 때가 16.35%p 였다. 그 근처로 돌아가면 뭔가 끊긴 것이다.
    assert!(avg < 4.0, "열 비율 차이가 평균 {avg:.2}%p 다 — 문서 폭을 안 쓰고 있다");
    assert!(
        bad.is_empty(),
        "바닥에 걸리지도 않았는데 비율이 어긋난 표 {}개:\n  {}",
        bad.len(),
        bad.join("\n  ")
    );
}

/// **증인이 읽은 열 폭의 합이 조판기가 잰 표 폭과 같은가.**
///
/// 셀 레코드의 바이트 자리를 맞게 짚었는지 보는 시험이다. 실측: s16.hwp 표1 이
/// `4836+565+42758 = 48159` 이고 조판값이 `642.12px × 75 = 48159` — 1px 오차
/// 안에서 코퍼스 123개가 맞는다. 자리를 잘못 짚으면 이 합이 흐트러진다.
#[test]
fn the_cell_widths_we_read_add_up_to_what_the_typesetter_measured() {
    if !needs_real_corpus("the_cell_widths_we_read_add_up_to_what_the_typesetter_measured") {
        return;
    }
    let Some((rhwp, _)) = backend() else { return };
    let mut ok = 0usize;
    let mut bad = Vec::new();
    for f in samples() {
        let Ok(t) = Testimony::read(&f) else { continue };
        let Ok(layout) = rhwp.dump_pages(&f) else { continue };
        let widths: Vec<f64> = layout
            .pages
            .iter()
            .flat_map(|p| p.columns.iter())
            .flat_map(|c| c.items.iter())
            .filter_map(|i| i.table.as_ref().map(|t| t.width_px))
            .collect();
        // 조판값이 센 표 수와 증인이 센 표 수가 같을 때만 짝을 믿는다 —
        // 칸 안에 든 표는 조판값의 최상위 목록에 안 나온다.
        if widths.len() != t.tables.len() {
            continue;
        }
        for (tb, px) in t.tables.iter().zip(widths.iter()) {
            let mut row0: Vec<_> = tb.cells.iter().filter(|c| c.row == 0).collect();
            row0.sort_by_key(|c| c.col);
            let sum: u32 = row0.iter().map(|c| c.width).sum();
            if sum == 0 {
                continue;
            }
            // 1px = 75 HWPUNIT (단 폭에서 쓰는 그 환산)
            let want = (px * 75.0).round() as i64;
            let d = (sum as i64 - want).abs();
            ok += 1;
            if d > 80 {
                bad.push(format!(
                    "{} 표{}: 증인 합 {sum} vs 조판 {want} (차 {d})",
                    f.display(),
                    tb.index
                ));
            }
        }
    }
    assert!(ok > 60, "표를 {ok}개만 대조했다 — 시험이 비었다");
    assert!(bad.is_empty(), "셀 폭이 조판값과 다른 표:\n  {}", bad.join("\n  "));
}

/// **화면 모델이 든 단락 원문이 증인의 원문과 글자 하나까지 같은가.**
///
/// 편집 버퍼의 본문은 `doc.paras` 에서 온다. 전에는 블록의 접힌 줄을 되붙여
/// 만들었는데, `wrap` 은 줄을 끊은 자리의 공백을 버리므로(표시에는 그게 맞다)
/// 되붙이면 접힌 자리마다 한 칸이 사라졌다 — 코퍼스 단락 **200개**가 그랬다.
/// 그러면 커서 오프셋이 그만큼 앞으로 밀려 엉뚱한 자리에 글자가 들어가고,
/// 자가검증도 못 잡는다(기대값과 재생이 같은 틀린 오프셋을 쓴다).
///
/// 그래서 공백을 걸러내고 비교하면 **안 된다.** 걸러내면 바로 그 결함이
/// 보이지 않는다 — `screen_model_loses_no_text_and_duplicates_none` 이
/// 양쪽에서 공백을 걸러내고 있었고, 그래서 통과하면서 이걸 가려 주었다.
#[test]
fn the_paragraph_text_the_editor_edits_is_the_witness_text_space_for_space() {
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }
    let mut bad = Vec::new();
    for f in &files {
        let Ok(doc) = hwp_core::ScreenDoc::build(&rhwp, f) else { continue };
        let Ok(src) = Testimony::read(f) else { continue };
        for p in src.body_paragraphs() {
            let Some(i) = p.index else { continue };
            let Some(got) = doc.paras.get(&(p.section, i)) else { continue };
            if got != &p.text {
                bad.push(format!(
                    "{} 구역{} 단락{}\n  증인: {:?}\n  모델: {:?}",
                    f.file_name().unwrap().to_string_lossy(),
                    p.section, i, p.text, got
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "모델의 원문이 증인과 다른 단락 {}개:\n{}",
        bad.len(),
        bad.iter().take(3).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// **텍스트 파일을 고쳐 저장하면 고친 것만 바뀌어야 한다** — 줄끝과 마지막
/// 줄바꿈까지 그대로.
///
/// 사용자 요청(2026-10-13)으로 들어온 경로다. 여기에는 백엔드도 증인도 없고
/// **파일 자체가 증인**이다: 임시본에 쓰고 되읽어 기대와 대조한 뒤에만 원본을
/// 바꾼다. 그래서 이 시험이 그 규율의 유일한 증거다.
#[test]
fn editing_a_text_file_changes_only_what_was_edited() {
    use hwp_core::edit::{EditLog, Op};
    use hwp_core::text::TextFile;

    let dir = std::env::temp_dir().join(format!("hwp-cmd-text-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);

    for (name, body) in [
        ("lf.txt", "첫 줄\n둘째 줄\n셋째 줄\n"),
        ("crlf.txt", "첫 줄\r\n둘째 줄\r\n"),
        ("noeol.txt", "끝줄바꿈이 없다"),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, body).expect("시험 파일");
        let before = std::fs::read(&path).expect("원본");

        let f = TextFile::read(&path).expect("텍스트로 읽기");
        let mut log = EditLog::new();
        log.push(Op::InsertText { at: (0, 0), offset: 0, text: "표시".into() });
        let report = f.save(&log).expect("자가검증 저장");

        // 백업이 원본 그대로여야 한다
        let backup = report.backup.expect("백업");
        assert_eq!(std::fs::read(&backup).expect("백업 읽기"), before, "{name}: 백업이 원본과 다르다");

        // 첫 줄만 바뀌고 나머지는 글자 하나까지 같아야 한다
        let after = std::fs::read_to_string(&path).expect("저장본");
        let want = {
            let mut t = TextFile::read(&backup).expect("백업 읽기");
            t.lines[0] = format!("표시{}", t.lines[0]);
            String::from_utf8(t.to_bytes()).unwrap()
        };
        assert_eq!(after, want, "{name}: 저장본이 기대와 다르다");

        // 줄끝이 그대로인지 바이트로 본다
        if body.contains("\r\n") {
            assert!(after.contains("\r\n"), "{name}: CRLF 가 사라졌다");
            assert!(!after.replace("\r\n", "").contains('\n'), "{name}: LF 가 섞였다");
        } else {
            assert!(!after.contains('\r'), "{name}: 없던 CR 이 생겼다");
        }
        assert_eq!(
            after.ends_with('\n'),
            body.ends_with('\n'),
            "{name}: 마지막 줄바꿈 유무가 바뀌었다"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// **UTF-8 이 아닌 파일은 열지 않는다.** 억지로 읽어 저장하면 파일이 망가지고,
/// 열 때는 아무 표시도 나지 않는다 — 이 프로젝트가 가장 싫어하는 모양이다.
#[test]
fn a_text_file_that_is_not_utf8_is_refused_before_anything_is_read() {
    use hwp_core::text::TextFile;
    let dir = std::env::temp_dir().join(format!("hwp-cmd-cp949-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("cp949.txt");
    // "한글" in CP949
    std::fs::write(&path, [0xC7u8, 0xD1, 0xB1, 0xDB, 0x0A]).expect("시험 파일");
    let err = TextFile::read(&path).expect_err("거부해야 한다");
    let msg = format!("{err:#}");
    assert!(msg.contains("CP949"), "무엇인지 말해야 한다: {msg}");
    assert!(msg.contains("iconv"), "바꾸는 길을 알려줘야 한다: {msg}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **기계 출구의 본문과 편집 오프셋이 같은 "보이는 글자" 정의를 쓰는가.**
///
/// 감사(2026-10-13)에서 나온 것: `offset::visible` 이 그 변환을 "독점한다"고
/// 적어 두었는데 사실이 아니었다 — 제품 코드 **5곳**이 `replace('\u{FFFC}', "")`
/// 로 손수 다시 만들고 있었다. 지금은 결과가 같지만, `visible` 이 바뀌고 그
/// 5곳이 안 바뀌면 **기계가 받는 글자와 편집 좌표가 어긋난다.** 그러면 LLM 이
/// 센 오프셋이 다른 자리를 가리키고, 자가검증은 그걸 잡지 못한다(양쪽이 같은
/// 오프셋을 쓰므로 일치해 버린다 — 접힌 줄 결함과 같은 모양).
///
/// 그래서 **출구를 통째로 대조한다.** 손으로 흉내 내지 않고 `visible` 을 직접
/// 불러 비교하므로, 정의가 갈라지는 순간 이 시험이 걸린다.
#[test]
fn the_machine_output_and_the_edit_offsets_share_one_definition_of_visible_text() {
    let Some((rhwp, _)) = backend() else { return };
    let files = samples();
    if files.is_empty() {
        return;
    }
    let mut bad = Vec::new();
    for f in &files {
        let Ok(src) = Testimony::read(f) else { continue };
        // (1) 화면 모델이 든 원문 — 편집 버퍼가 쓰는 것
        let Ok(doc) = hwp_core::ScreenDoc::build(&rhwp, f) else { continue };
        // (2) 기계 출구
        let Ok(json) = hwp_core::json::build(&rhwp, f, hwp_core::json::Profile::Doc) else {
            continue;
        };
        let body: std::collections::BTreeMap<(u32, u32), String> = json
            .body
            .iter()
            .flatten()
            .filter_map(|b| match b {
                hwp_core::json::BodyItem::Para { section, para, text } => {
                    Some(((*section, *para), text.clone()))
                }
                _ => None,
            })
            .collect();

        for p in src.body_paragraphs() {
            let Some(i) = p.index else { continue };
            let want = hwp_core::offset::visible(&p.text);
            if let Some(got) = body.get(&(p.section, i)) {
                if got != &want {
                    bad.push(format!(
                        "{} 구역{} 단락{} — 기계 출구가 다르다\n  visible: {:?}\n  json:    {:?}",
                        f.file_name().unwrap().to_string_lossy(), p.section, i, want, got
                    ));
                }
            }
            // 화면 모델은 컨트롤을 **붙인 채** 들고 있고 버퍼가 뗀다.
            // 그 뗀 결과가 같은 정의여야 한다.
            if let Some(raw) = doc.paras.get(&(p.section, i)) {
                if hwp_core::offset::visible(raw) != want {
                    bad.push(format!(
                        "{} 구역{} 단락{} — 화면 모델이 다르다",
                        f.file_name().unwrap().to_string_lossy(), p.section, i
                    ));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "'보이는 글자' 정의가 갈라진 곳 {}개:\n{}",
        bad.len(),
        bad.iter().take(3).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// **쓰기는 목적지를 검증 전에 덮지 않는다.**
///
/// 프로덕션 감사(2026-10-13)가 찾은 것: `save_as` 가 목적지를 원본으로 먼저
/// 덮고 그 위에서 검증을 돌렸다. 그래서 검증·백업·쓰기 중 **어디서 실패해도
/// 목적지 문서는 이미 파괴된 상태**였다 — 실측으로 1.6MB 짜리 다른 문서가
/// 사라지고 도구는 "쓰지 못했습니다" 라고 말했다.
///
/// 이 시험은 실패하게 만들어 놓고 **목적지 바이트가 그대로인지**만 본다.
#[test]
fn a_failed_save_as_leaves_the_destination_untouched() {
    let Some(((rhwp, _), files)) = backend().zip(Some(samples())) else { return };
    let Some(src) = files.first() else { return };
    let Some(other) = files.iter().find(|f| f != &src) else { return };

    let dir = std::env::temp_dir().join(format!("hwp-cmd-saveas-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let keep = dir.join("보관");
    let _ = std::fs::create_dir_all(&keep);

    // 목적지는 **다른 문서**다. 같은 확장자여야 형식 검사를 지난다.
    let ext = src.extension().and_then(|s| s.to_str()).unwrap_or("hwp");
    let dest = keep.join(format!("남의것.{ext}"));
    let donor = files.iter().find(|f| f.extension() == src.extension() && f != &src).unwrap_or(other);
    std::fs::copy(donor, &dest).expect("목적지 문서");
    let before = std::fs::read(&dest).expect("목적지 읽기");

    // 쓰기를 금지해 실패하게 만든다
    let mut perm = std::fs::metadata(&keep).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perm.set_mode(0o500);
    std::fs::set_permissions(&keep, perm).unwrap();

    let mut log = hwp_core::edit::EditLog::new();
    log.push(hwp_core::edit::Op::InsertText {
        at: (0, 0),
        offset: 0,
        text: "덮".into(),
    });
    let err = hwp_core::edit::save_as(&rhwp, src, &dest, &log);

    let mut perm = std::fs::metadata(&keep).unwrap().permissions();
    perm.set_mode(0o700);
    std::fs::set_permissions(&keep, perm).unwrap();

    assert!(err.is_err(), "쓰기 금지 폴더인데 성공했다");
    let after = std::fs::read(&dest).expect("목적지 다시 읽기");
    assert_eq!(
        before.len(),
        after.len(),
        "실패했는데 목적지가 바뀌었다 — {}바이트 → {}바이트",
        before.len(),
        after.len()
    );
    assert_eq!(before, after, "실패했는데 목적지 바이트가 바뀌었다");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **같은 파일을 목적지로 주면 거부한다 — 하드링크까지.**
///
/// `canonicalize()` 비교는 하드링크를 못 봤고, 그 틈으로 `fs::copy(자기, 자기)`
/// 가 원본을 0바이트로 잘랐다(실측: 문서 0바이트, 사본 삭제, 백업 없음).
#[test]
fn a_hardlink_destination_is_refused_not_truncated() {
    let Some(((rhwp, _), files)) = backend().zip(Some(samples())) else { return };
    let Some(src) = files.first() else { return };

    let dir = std::env::temp_dir().join(format!("hwp-cmd-hl-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let ext = src.extension().and_then(|s| s.to_str()).unwrap_or("hwp");
    let doc = dir.join(format!("문서.{ext}"));
    let link = dir.join(format!("사본.{ext}"));
    std::fs::copy(src, &doc).expect("문서");
    std::fs::hard_link(&doc, &link).expect("하드링크");
    let before = std::fs::read(&doc).expect("읽기");

    assert!(
        hwp_core::edit::same_file(&doc, &link),
        "하드링크를 같은 파일로 봐야 한다"
    );

    let mut log = hwp_core::edit::EditLog::new();
    log.push(hwp_core::edit::Op::InsertText { at: (0, 0), offset: 0, text: "하".into() });
    let err = hwp_core::edit::save_as(&rhwp, &doc, &link, &log);

    assert!(err.is_err(), "하드링크 목적지를 받아들였다");
    let after = std::fs::read(&doc).expect("다시 읽기");
    assert_eq!(before, after, "거부했는데 원본이 바뀌었다 ({}바이트)", after.len());
    assert!(!after.is_empty(), "원본이 0바이트가 됐다");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **`.bak` 을 덮지 않는다.** 두 번 저장해도 원본이 남아 있어야 한다.
#[test]
fn saving_twice_keeps_the_original_backup() {
    let Some(((rhwp, _), files)) = backend().zip(Some(samples())) else { return };
    let Some(src) = files.iter().find(|f| f.extension().and_then(|s| s.to_str()) == Some("hwp"))
    else { return };

    let dir = std::env::temp_dir().join(format!("hwp-cmd-bak-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let doc = dir.join("d.hwp");
    std::fs::copy(src, &doc).expect("문서");
    let original = std::fs::read(&doc).expect("원본");

    for (n, text) in [(0usize, "일회"), (1, "이회")] {
        let mut log = hwp_core::edit::EditLog::new();
        log.push(hwp_core::edit::Op::InsertText { at: (0, 0), offset: 0, text: text.into() });
        hwp_core::edit::save(&rhwp, &doc, &log).unwrap_or_else(|e| panic!("{n}회 저장: {e:#}"));
    }

    // 어느 세대든 **원본 바이트가 하나는 남아 있어야** 한다
    let baks: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().contains(".bak"))
        .collect();
    let kept = baks.iter().any(|b| std::fs::read(b).map(|v| v == original).unwrap_or(false));
    assert!(
        kept,
        "두 번 저장하니 원본이 어느 백업에도 없다 — 남은 것: {:?}",
        baks.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// **명세에서 문서를 만들고, 만든 것을 증인이 대조한다.**
///
/// 사용자 요청(2026-10-29)으로 들어온 만들기 경로다. 백엔드의 `scaffold` 가
/// 문서를 내지만 그것이 명세대로인지는 **백엔드 자신이 센 수**로는 알 수 없다
/// (작성자가 자기 답을 채점하는 것이다). 그래서 저장과 같은 모양으로 증인이
/// 다시 읽어 글자까지 대조한다.
#[test]
fn creating_a_document_from_a_spec_is_verified_by_the_witness() {
    let Some((rhwp, _)) = backend() else { return };
    let dir = std::env::temp_dir().join(format!("hwp-cmd-create-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let dest = dir.join("만든것.hwpx");

    let spec = serde_json::json!({
        "version": "1",
        "title": "과업지시서",
        "blocks": [
            {"type": "heading", "level": 1, "text": "1. 과업명"},
            {"type": "paragraph", "text": "○ 기후변화 대응 작물 재배안정성 강화"},
            {"type": "heading", "level": 2, "text": "2. 예산"},
            {"type": "table",
             "rows": [["항목", "1년차"], ["인건비", "120,000"]],
             "cell_align": "center"}
        ]
    });

    let report = hwp_core::create::create(&rhwp, &spec, &dest).expect("만들기");
    assert_eq!(report.blocks, 4);
    assert_eq!(report.tables, 1, "표 1개여야 한다");
    assert_eq!(report.cells, 4, "2×2 = 4칸이어야 한다");

    // **우리 도구가 그 문서를 다시 읽는가** — 만들기만 되고 읽기가 안 되면
    // 고리가 닫히지 않는다.
    let doc = hwp_core::ScreenDoc::build(&rhwp, &dest).expect("만든 문서를 화면 모델로");
    let body: Vec<&String> = doc.paras.values().collect();
    for line in ["1. 과업명", "○ 기후변화 대응 작물 재배안정성 강화", "2. 예산"] {
        assert!(
            body.iter().any(|b| b.trim() == line),
            "만든 문서에 {line:?} 가 없다 — 읽은 것: {:?}",
            body.iter().map(|b| b.trim()).collect::<Vec<_>>()
        );
    }
    // 표가 좌표를 들고 있는가
    assert_eq!(doc.tables.len(), 1, "표가 좌표와 함께 있어야 한다");

    // **있는 파일은 만들지 않는다.**
    let again = hwp_core::create::create(&rhwp, &spec, &dest);
    assert!(again.is_err(), "있는 파일을 덮었다");
    assert!(format!("{}", again.unwrap_err()).contains("이미 있"));

    // **`.hwp` 는 만들 수 없다고 말한다** — 조용히 다른 형식을 주지 않는다.
    let wrong = hwp_core::create::create(&rhwp, &spec, &dir.join("x.hwp"));
    let m = format!("{}", wrong.unwrap_err());
    assert!(m.contains("hwpx"), "무엇만 되는지 말해야 한다: {m}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **문단이 여러 개인 칸을 기계가 채울 수 있는가 — 그리고 자가검증을 타는가.**
///
/// 사용자 보고(2026-10-29): "`set-cell` 이 한 줄만 받아서 문단이 여러 개인 칸은
/// 기계가 채울 방법이 없습니다. 한국 서식에서 서술형 칸은 거의 다 여러 문단이라
/// 서식 채우기의 큰 구멍입니다. **제가 XML 을 직접 고쳐 우회했는데, 그 경로는
/// 자가검증을 타지 않습니다.**"
///
/// 마지막 문장이 이 시험의 이유다 — 도구의 구멍이 사용자를 검증 없는 경로로
/// 밀어냈다. 실측: 코퍼스의 글자 있는 칸 1,517개 중 **128개(8%)가 여러 문단**.
///
/// 백엔드의 `set-cell --text` 는 줄바꿈을 거부하므로 한 연산이 여러 명령으로
/// 펴진다(`set-cell` → 문단마다 `split-paragraph-in-cell` + `insert-text-in-cell`).
/// 그 사슬이 실제로 요청한 문단을 만드는지, 그리고 **증인이 그것을 되읽어
/// 대조하는지**를 왕복으로 본다.
#[test]
fn a_cell_with_several_paragraphs_can_be_filled_by_a_machine() {
    let Some(((rhwp, _), files)) = backend().zip(Some(samples())) else { return };
    // 표가 있는 .hwp 하나를 고른다
    let Some(src) = files.iter().find(|f| {
        f.extension().and_then(|s| s.to_str()) == Some("hwp")
            && Testimony::read(f).map(|t| !t.tables.is_empty()).unwrap_or(false)
    }) else {
        return;
    };

    let dir = std::env::temp_dir().join(format!("hwp-cmd-multipara-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let doc = dir.join("d.hwp");
    std::fs::copy(src, &doc).expect("문서");

    // 첫 표의 (0,0) — 그 칸이 있는지 증인에게 묻는다
    let t = Testimony::read(&doc).expect("증인");
    let table = t.tables.first().expect("표");
    assert!(table.cells.iter().any(|c| c.row == 0 && c.col == 0), "(0,0) 칸이 있어야 한다");
    let idx = table.index;

    let want = vec![
        "제출 서류".to_string(),
        "  1) 제출공문 1부".to_string(),
        "  2) 사업 참여 신청서 1부".to_string(),
    ];

    let mut log = hwp_core::edit::EditLog::new();
    log.push(hwp_core::edit::Op::SetCell {
        table: idx,
        row: 0,
        col: 0,
        lines: want.clone(),
    });
    hwp_core::edit::save(&rhwp, &doc, &log).unwrap_or_else(|e| panic!("저장: {e:#}"));

    // **증인이 되읽어 문단 경계가 살아 있는가.** 칸 안 문단은 `\n` 으로 이어
    // 읽히므로, 그렇게 이은 것이 요청과 같아야 한다.
    let after = Testimony::read(&doc).expect("저장본 증인");
    let cell = after
        .tables
        .iter()
        .find(|x| x.index == idx)
        .and_then(|x| x.cells.iter().find(|c| c.row == 0 && c.col == 0))
        .expect("저장본의 (0,0)");
    assert_eq!(
        cell.text,
        want.join("\n"),
        "문단 경계가 살지 않았다 — 기대 {:?} / 실제 {:?}",
        want.join("\n"),
        cell.text
    );
    assert_eq!(
        cell.text.split('\n').count(),
        3,
        "문단이 3개여야 한다 — 실제 {:?}",
        cell.text
    );

    // **한 문단도 전과 같이 된다** (`lines` 가 하나인 경우)
    let mut log2 = hwp_core::edit::EditLog::new();
    log2.push(hwp_core::edit::Op::SetCell {
        table: idx,
        row: 0,
        col: 0,
        lines: vec!["한 문단으로".to_string()],
    });
    hwp_core::edit::save(&rhwp, &doc, &log2).unwrap_or_else(|e| panic!("한 문단 저장: {e:#}"));
    let back = Testimony::read(&doc).expect("증인");
    let one = back
        .tables
        .iter()
        .find(|x| x.index == idx)
        .and_then(|x| x.cells.iter().find(|c| c.row == 0 && c.col == 0))
        .expect("(0,0)");
    assert_eq!(one.text, "한 문단으로", "한 문단이 접히지 않았다: {:?}", one.text);

    let _ = std::fs::remove_dir_all(&dir);
}

/// **칸 안의 한 문단만 고친다 — 나머지 문단은 손대지 않는다.**
///
/// 사용자 보고(2026-10-29): `set-cell` 은 통째 덮어쓰기뿐이라 한 글자를
/// 고치려도 칸 전체를 다시 써야 했다. 그러려면 원문을 정확히 베껴야 하는데,
/// **베끼는 자리가 오타가 들어가는 자리**다 — 그래서 고친 문단 하나가 아니라
/// 베낀 문단 전부가 위험해진다. 여기서 지키는 것은 "고치지 않은 문단은
/// 글자까지 그대로" 다.
#[test]
fn editing_one_paragraph_in_a_cell_leaves_the_other_paragraphs_alone() {
    let Some(((rhwp, _), files)) = backend().zip(Some(samples())) else { return };
    let Some(src) = files.iter().find(|f| {
        f.extension().and_then(|s| s.to_str()) == Some("hwp")
            && Testimony::read(f).map(|t| !t.tables.is_empty()).unwrap_or(false)
    }) else {
        return;
    };

    let dir = std::env::temp_dir().join(format!("hwp-cmd-cellpart-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let doc = dir.join("d.hwp");
    std::fs::copy(src, &doc).expect("문서");

    let t = Testimony::read(&doc).expect("증인");
    let table = t.tables.first().expect("표");
    assert!(table.cells.iter().any(|c| c.row == 0 && c.col == 0), "(0,0) 칸이 있어야 한다");
    let idx = table.index;

    // 여러 문단인 칸을 만들어 둔다 — 부분 수정이 뜻을 가지려면 문단이 여럿이어야 한다.
    let before = vec![
        "가. 추진 배경".to_string(),
        "현장 문제 해결이 늦다".to_string(),
        "검증 절차가 길다".to_string(),
    ];
    let mut log = hwp_core::edit::EditLog::new();
    log.push(hwp_core::edit::Op::SetCell {
        table: idx,
        row: 0,
        col: 0,
        lines: before.clone(),
    });
    hwp_core::edit::save(&rhwp, &doc, &log).unwrap_or_else(|e| panic!("밑작업 저장: {e:#}"));

    let read_cell = |path: &std::path::Path| -> String {
        Testimony::read(path)
            .expect("증인")
            .tables
            .iter()
            .find(|x| x.index == idx)
            .and_then(|x| x.cells.iter().find(|c| c.row == 0 && c.col == 0))
            .expect("(0,0)")
            .text
            .clone()
    };
    assert_eq!(read_cell(&doc), before.join("\n"), "밑작업이 서지 않았다");

    // 가운데 문단의 "늦다" 를 "지연된다" 로 — 지우고 넣는 두 연산이 한 번에.
    let mid = &before[1];
    let at = mid.chars().count() - "늦다".chars().count();
    let mut log2 = hwp_core::edit::EditLog::new();
    log2.push(hwp_core::edit::Op::DeleteTextInCell {
        table: idx,
        row: 0,
        col: 0,
        cell_para: 1,
        offset: at,
        count: "늦다".chars().count(),
    });
    log2.push(hwp_core::edit::Op::InsertTextInCell {
        table: idx,
        row: 0,
        col: 0,
        cell_para: 1,
        offset: at,
        text: "지연된다".to_string(),
    });
    hwp_core::edit::save(&rhwp, &doc, &log2).unwrap_or_else(|e| panic!("부분 수정 저장: {e:#}"));

    let after: Vec<String> = read_cell(&doc).split('\n').map(String::from).collect();
    assert_eq!(after.len(), before.len(), "문단 수가 달라졌다: {after:?}");
    assert_eq!(after[1], "현장 문제 해결이 지연된다", "고친 문단: {:?}", after[1]);
    // **이것이 이 시험의 요점이다** — 통째 덮어쓰기와 다른 점.
    assert_eq!(after[0], before[0], "건드리지 않은 첫 문단이 바뀌었다");
    assert_eq!(after[2], before[2], "건드리지 않은 셋째 문단이 바뀌었다");

    // **좌표가 틀리면 아무것도 쓰지 않는다.** 없는 문단을 가리키면 저장 전에
    // 멈춘다 — 기계가 `tables` 를 다시 읽지 않고 번호를 짐작했을 때의 길이다.
    let keep = read_cell(&doc);
    let mut bad = hwp_core::edit::EditLog::new();
    bad.push(hwp_core::edit::Op::InsertTextInCell {
        table: idx,
        row: 0,
        col: 0,
        cell_para: 9,
        offset: 0,
        text: "x".to_string(),
    });
    let e = hwp_core::edit::save(&rhwp, &doc, &bad).expect_err("없는 문단은 거부되어야 한다");
    assert!(
        format!("{e:#}").contains("문단"),
        "어느 좌표가 틀렸는지 말해야 한다: {e:#}"
    );
    assert_eq!(read_cell(&doc), keep, "거부된 편집이 파일을 바꿨다");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **체크박스를 바꿔도 그 글자의 꾸밈이 살아야 한다.**
///
/// 사용자 보고(2026-10-29): 체크박스를 토글할 방법이 없었다. 짓고 나니 넣고
/// 지우는 **순서**가 문서를 조용히 고치는 자리였다.
///
/// `insert-text --offset N` 은 N 에서 끝나는 **왼쪽 구간**에 붙는다. 서식의
/// 박스는 꾸민 구간의 **첫 글자**인 경우가 많아서
/// (`[charPr 234: " "][charPr 266: "□ 유학생 유치 계획"]`), 먼저 지우고 그
/// 자리에 넣으면 새 글자가 앞 구간의 속성을 받는다 — 실측으로 진한 박스가
/// 진하지 않게 됐고 charPrIDRef 가 266 에서 234 로 갈려 **색·글꼴·크기까지**
/// 함께 바뀌었다. 한 칸 뒤에 넣고 원래 것을 지우면 266 이 그대로 남는다.
///
/// 이 시험이 지키는 것은 그 순서다. **꾸밈이 없는 문서에서는 두 순서가 똑같이
/// 통과하므로**, 꾸민 구간 안에 박스가 있는 문서가 코퍼스에 있어야 한다
/// (합성 코퍼스의 `c03.hwpx`).
#[test]
fn toggling_a_checkbox_keeps_the_character_formatting_of_that_glyph() {
    let Some(((rhwp, _), files)) = backend().zip(Some(samples())) else { return };

    // 꾸민 구간 **안**에 꺼진 박스가 있는 문서를 찾는다
    const OFF: char = '\u{25a1}'; // □
    const ON: char = '\u{2611}'; // ☑
    let mut found = None;
    for f in &files {
        let Ok(t) = Testimony::read(f) else { continue };
        for p in t.body_paragraphs() {
            let Some(i) = p.index else { continue };
            // `Run` 은 글자를 들지 않고 `start`·`len` 으로 가리킨다 — 편집
            // 오프셋과 같은 셈(보이는 글자)이라 그대로 썰면 된다.
            let chars: Vec<char> = hwp_core::offset::visible(&p.text).chars().collect();
            for r in &p.runs {
                if !(r.deco.bold || r.deco.italic || r.deco.underline || r.deco.strike) {
                    continue;
                }
                let end = (r.start + r.len).min(chars.len());
                if r.start >= end {
                    continue;
                }
                if let Some(n) = chars[r.start..end].iter().position(|&c| c == OFF) {
                    found = Some((f.clone(), p.section, i, r.start + n));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    let Some((src, section, para, offset)) = found else {
        panic!(
            "꾸민 구간 안에 꺼진 체크박스가 있는 문서가 없다 — 이 시험은 그 문서가 \
             있어야 뜻이 있다 (합성 코퍼스는 scripts/make-corpus.sh 가 c03.hwpx 에 \
             만든다). 코퍼스를 다시 만들어라"
        );
    };

    let dir = std::env::temp_dir().join(format!("hwp-cmd-chk-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let doc = dir.join(src.file_name().expect("이름"));
    std::fs::copy(&src, &doc).expect("문서");

    // 그 단락의 꾸밈 구간을 적어 둔다 — 글자만 바뀌고 구간은 그대로여야 한다
    let runs_of = |path: &std::path::Path| -> Vec<(usize, usize, bool, bool, bool, bool)> {
        Testimony::read(path)
            .expect("증인")
            .body_paragraphs()
            .into_iter()
            .filter(|p| p.section == section && p.index == Some(para))
            .flat_map(|p| p.runs.clone())
            .map(|r| {
                (
                    r.start,
                    r.len,
                    r.deco.bold,
                    r.deco.italic,
                    r.deco.underline,
                    r.deco.strike,
                )
            })
            .collect()
    };
    let before = runs_of(&doc);
    assert!(!before.is_empty(), "꾸밈 구간을 읽지 못했다");

    // **넣고 나서 지운다.** 뒤집으면 이 시험이 깨진다 — 그게 이 시험의 요점이다.
    let mut log = hwp_core::edit::EditLog::new();
    log.push(hwp_core::edit::Op::InsertText {
        at: (section, para),
        offset: offset + 1,
        text: ON.to_string(),
    });
    log.push(hwp_core::edit::Op::DeleteText { at: (section, para), offset, count: 1 });
    hwp_core::edit::save(&rhwp, &doc, &log).unwrap_or_else(|e| panic!("저장: {e:#}"));

    let after_text = Testimony::read(&doc)
        .expect("증인")
        .body_paragraphs()
        .into_iter()
        .find(|p| p.section == section && p.index == Some(para))
        .map(|p| hwp_core::offset::visible(&p.text))
        .expect("단락");
    assert_eq!(
        after_text.chars().nth(offset),
        Some(ON),
        "박스가 켜지지 않았다: {after_text:?}"
    );
    assert_eq!(
        runs_of(&doc),
        before,
        "박스를 켜면서 글자 꾸밈이 갈렸다 — 지우고 넣는 순서가 뒤집혔는지 보라\n           전: {before:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **양식의 안 쓰는 칸을 지우는 길이 있어야 한다 — 검증을 지나는 길로.**
///
/// 사용자 보고(2026-10-29): 다른 세션이 HWPX 의 zip 을 풀어 XML 을 직접 고쳐
/// 양식을 채웠고, 거기서 "작성방법 쪽·안 쓰는 칸·첨부 1 을 문단째 지웠다".
/// 우리는 개체 달린 단락 삭제를 **거부**했으므로 그 일을 할 수 없었고, 그래서
/// 사용자가 자가검증이 **아예 없는** 경로로 밀려났다. 세 번째였다.
///
/// 이 시험이 지키는 것은 **두 걸음 길**이다 — 표를 지우면 빈 단락이 남고, 그
/// 단락은 이제 평범하게 지울 수 있다(개체가 있는 동안은 거부된다).
///
/// 그리고 지우는 쪽은 **번호가 당겨진다**: 표3 을 지우면 표4 가 표3 이 된다.
/// 그 셈이 틀리면 "그 뒤 표 전부가 바뀌었다" 가 되는데, 넣을 때 이미 한 번
/// 당했다.
#[test]
fn deleting_a_table_leaves_an_empty_paragraph_that_can_then_be_deleted() {
    let Some(((rhwp, _), files)) = backend().zip(Some(samples())) else { return };
    // 표가 **둘 이상**인 문서여야 번호 당기기를 볼 수 있다
    let Some((src, anchors)) = files.iter().find_map(|f| {
        let t = Testimony::read(f).ok()?;
        (t.tables.len() >= 2).then(|| {
            let a: Vec<u32> = t.tables.iter().map(|tb| tb.index).collect();
            (f.clone(), a)
        })
    }) else {
        panic!("표가 둘 이상인 문서가 코퍼스에 없다 — 이 시험은 그 문서가 있어야 뜻이 있다");
    };

    let dir = std::env::temp_dir().join(format!("hwp-cmd-deltable-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let doc = dir.join(src.file_name().expect("이름"));
    std::fs::copy(&src, &doc).expect("문서");

    // 지울 표와 그 앵커 — 앵커는 증인이 모르므로 백엔드에게 묻는다
    let victim = anchors[0];
    let built = hwp_core::json::build(&rhwp, &doc, hwp_core::json::Profile::Full).expect("문서");
    let at = built
        .tables
        .as_ref()
        .and_then(|ts| {
            ts.iter()
                .find(|x| x.id.trim_start_matches('t').parse::<u32>().ok() == Some(victim))
                .map(|x| (x.section, x.para))
        })
        .expect("표 앵커");

    let before = Testimony::read(&doc).expect("증인");
    let n_tables = before.tables.len();
    let n_paras = before.body_paragraphs().len();
    // 지울 표 **다음** 표의 첫 칸 글자 — 번호가 당겨졌는지 이것으로 본다
    let next_first_cell = before
        .tables
        .iter()
        .find(|tb| tb.index == victim + 1)
        .and_then(|tb| tb.cells.iter().find(|c| c.row == 0 && c.col == 0))
        .map(|c| c.text.clone());

    let mut log = hwp_core::edit::EditLog::new();
    log.push(hwp_core::edit::Op::DeleteTable { at, table: victim });
    hwp_core::edit::save(&rhwp, &doc, &log).unwrap_or_else(|e| panic!("표 지우기: {e:#}"));

    let mid = Testimony::read(&doc).expect("증인");
    assert_eq!(mid.tables.len(), n_tables - 1, "표가 하나 줄어야 한다");
    // **단락은 남는다.** 넣을 때는 표와 단락이 함께 들어오지만 지울 때는 표만
    // 나간다 — 그 비대칭을 모르고 짰다가 자가검증에 걸렸다.
    assert_eq!(
        mid.body_paragraphs().len(),
        n_paras,
        "표를 지워도 단락 수는 그대로여야 한다"
    );
    // **번호가 당겨졌는가.**
    if let Some(want) = next_first_cell {
        let now = mid
            .tables
            .iter()
            .find(|tb| tb.index == victim)
            .and_then(|tb| tb.cells.iter().find(|c| c.row == 0 && c.col == 0))
            .map(|c| c.text.clone());
        assert_eq!(now, Some(want), "지운 표 다음 표가 그 번호로 당겨져야 한다");
    }

    // **이제 그 빈 단락을 지울 수 있다.** 개체가 있는 동안은 거부된다.
    let mut log2 = hwp_core::edit::EditLog::new();
    log2.push(hwp_core::edit::Op::DeleteParagraph { at });
    hwp_core::edit::save(&rhwp, &doc, &log2)
        .unwrap_or_else(|e| panic!("표를 뺀 뒤 빈 단락 지우기: {e:#}"));
    let after = Testimony::read(&doc).expect("증인");
    assert_eq!(
        after.body_paragraphs().len(),
        n_paras - 1,
        "빈 단락이 지워져야 한다"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **개체가 달린 단락은 여전히 거부한다 — 다만 갈 곳을 말해 준다.**
///
/// 지우는 길이 생겼다고 거부를 푸는 것이 아니다. 개체를 먼저 빼지 않으면 그
/// 개체가 조용히 사라지고(실측: s00.hwp 단락 54, 그림 6→5), 우리는 개체를
/// 만들 수 없으므로 "그렇게 되어야 한다" 를 쓸 수 없다. 거부는 남기고,
/// **어디로 가야 하는지**를 메시지가 말한다.
#[test]
fn a_paragraph_that_holds_a_drawing_still_refuses_but_names_the_way_out() {
    let Some(((rhwp, _), files)) = backend().zip(Some(samples())) else { return };
    let Some((src, at)) = files.iter().find_map(|f| {
        let t = Testimony::read(f).ok()?;
        let snap = hwp_core::edit::snapshot(&t);
        snap.drawings
            .iter()
            .find(|&(_, &n)| n > 0)
            .map(|(coord, _)| (f.clone(), *coord))
    }) else {
        panic!("개체가 달린 단락이 있는 문서가 코퍼스에 없다");
    };

    let dir = std::env::temp_dir().join(format!("hwp-cmd-refuse-draw-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let doc = dir.join(src.file_name().expect("이름"));
    std::fs::copy(&src, &doc).expect("문서");

    let mut log = hwp_core::edit::EditLog::new();
    log.push(hwp_core::edit::Op::DeleteParagraph { at });
    let e = hwp_core::edit::save(&rhwp, &doc, &log)
        .expect_err("개체 달린 단락은 거부되어야 한다");
    let msg = format!("{e:#}");
    // **갈 곳을 말하는가.** 전에는 "검증할 수 없다" 로 끝나서, 받는 쪽이 할 수
    // 있는 일이 없었다 — 그래서 도구 밖으로 나갔다.
    assert!(
        msg.contains("delete-table") && msg.contains("delete-control"),
        "거부만 하고 갈 곳을 말하지 않는다: {msg}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **두 포맷이 같은 글자를 읽어야 한다 — 강제 줄바꿈까지.**
///
/// 사용자가 "hwp→hwpx 변환 기능이 있나" 고 물어서 변환 충실도를 재다가 결함이
/// 드러났다. HWPX 를 HWP 로 바꿔 두 쪽을 읽으니 **본문이 다르게 읽혔고**,
/// 변환이 범인인 줄 알았다. 아니었다 — 우리 HWP5 판독기가 단독 컨트롤 10
/// (강제 줄바꿈)을 버리고 있었고, OWPML 쪽은 `hp:lineBreak` 를 `'\n'` 으로
/// 읽고 있었다. **같은 개념을 두 판독기가 다르게 읽은 것**이다.
///
/// 글자 하나로 끝나지 않는다. 백엔드의 `--offset` 은 그 글자를 **센다**(실측
/// 확인: 줄바꿈이 64번째인 단락의 65번째에 넣으면 줄바꿈 바로 뒤에 들어간다).
/// 그래서 버리는 동안 강제 줄바꿈이 있는 `.hwp` 의 모든 오프셋이 하나씩
/// 짧았다. **자가검증은 이걸 못 잡는다** — 기대값과 재생이 같은 틀린 오프셋을
/// 쓰므로 둘이 일치해 통과한다(접힌 줄 결함과 같은 종류).
///
/// 이 시험이 **실무 문서를 요구하는 이유**: 합성 코퍼스에는 강제 줄바꿈이
/// 없다. 백엔드의 `insert-text --text` 가 줄바꿈을 거부하므로 우리 생성기로는
/// 만들 수가 없다. 그래서 `REAL_CORPUS_ONLY` 이고, **줄바꿈을 하나도 못 봤으면
/// 실패한다** — 조용히 통과하는 쪽이 더 나쁘다.
#[test]
fn the_two_formats_read_the_same_text_including_forced_line_breaks() {
    if !needs_real_corpus("the_two_formats_read_the_same_text_including_forced_line_breaks") {
        return;
    }
    let Some((rhwp, _)) = backend() else { return };
    let files: Vec<PathBuf> = samples()
        .into_iter()
        .filter(|f| f.extension().and_then(|s| s.to_str()) == Some("hwpx"))
        .collect();
    if files.is_empty() {
        return;
    }

    let dir = std::env::temp_dir().join(format!("hwp-cmd-xfmt-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);

    // **보이는 글자로 비교한다** (`offset::visible`).
    //
    // 날 글자로 비교하면 실패하는데 그건 손실이 아니다 — 두 포맷은 개체 자리
    // 표시의 수가 다르다(p.hwpx 단락0: HWP5 쪽에 U+FFFC 3개가 더 있다. 구역
    // 정의·쪽번호 같은 확장 컨트롤이고 OWPML 은 본문 글자로 내지 않는다).
    // `offset::visible` 은 편집 오프셋과 기계 출력이 **함께 쓰는** 정의이므로
    // 여기서 비교해야 할 단위도 그것이다.
    let text_of = |path: &std::path::Path| -> BTreeMap<(u32, u32), String> {
        Testimony::read(path)
            .expect("증인")
            .body_paragraphs()
            .into_iter()
            .filter_map(|p| p.index.map(|i| ((p.section, i), hwp_core::offset::visible(&p.text))))
            .collect()
    };

    let mut breaks_seen = 0usize;
    let mut checked = 0usize;
    for src in &files {
        let out = dir.join(format!(
            "{}.hwp",
            src.file_stem().and_then(|s| s.to_str()).unwrap_or("x")
        ));
        // 백엔드의 `--verify` 는 **작성자가 자기 답을 채점하는 것**이므로 통과
        // 여부만 보고, 내용 대조는 우리 증인이 한다.
        let (sp, op) = (src.display().to_string(), out.display().to_string());
        let ok = rhwp.run(&["convert", &sp, &op, "--verify"]).is_ok();
        if !ok || !out.is_file() {
            continue;
        }
        let a = text_of(src);
        let b = text_of(&out);
        breaks_seen += a.values().filter(|s| s.contains('\n')).count();
        checked += 1;
        for (k, want) in &a {
            let got = b.get(k);
            assert_eq!(
                got,
                Some(want),
                "{} 의 구역{} 단락{} 이 포맷에 따라 다르게 읽힌다\n  hwpx: {want:?}\n  hwp : {:?}",
                src.display(),
                k.0,
                k.1,
                got
            );
        }
        assert_eq!(a.len(), b.len(), "{} 의 단락 수가 변환 뒤 달라졌다", src.display());
    }

    assert!(checked > 0, "변환에 성공한 문서가 없다 — 이 시험이 아무것도 보지 않았다");
    // **본 것이 있는지 확인한다.** 강제 줄바꿈이 없는 문서만 봤다면 이 시험은
    // 통과해도 뜻이 없다 — 고치기 전에도 통과했을 것이다(실제로 그랬다).
    assert!(
        breaks_seen > 0,
        "강제 줄바꿈이 있는 단락을 하나도 못 봤다 — 이 시험이 지키려는 글자가 \
         코퍼스에 없다. 그 글자가 있는 문서를 넣거나, 이 시험을 지워라"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **코퍼스는 무엇을 못 재는지 스스로 말해야 한다.**
///
/// 이 저장소의 1번 재발 패턴은 "시험을 깔끔하게 하려고 걸러낸 것이 결함이 사는
/// 자리" 였다 — 빈 단락을 걸러내고, 공백을 걸러내고, 또 공백을 걸러냈다. 공개
/// 저장소에 합성 코퍼스를 넣는 것은 **그 패턴의 네 번째가 될 수 있다**: 실무
/// 문서가 없어 관문이 꺼지는데 `cargo test` 는 "통과" 라고 말한다.
///
/// 그래서 꺼지는 관문의 수를 `samples/MANIFEST.json` 이 **선언하게** 한다.
/// 시험을 더하거나 빼면 수가 달라지고 여기서 깨진다 — 다시 선언하게 만드는
/// 것이 목적이다. 수만 맞추면 이 장치가 뜻을 잃으므로, 깨질 때 **무엇이
/// 꺼졌는지** 를 같이 보여 준다.
#[test]
fn the_corpus_declares_what_it_cannot_check() {
    let manifest = workspace_root().join("samples/MANIFEST.json");
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        // 코퍼스가 아예 없으면 할 말이 없다 — 다른 시험들이 "비었다" 로 거부한다.
        return;
    };
    if !corpus_is_synthetic() {
        return; // 실무 코퍼스는 관문을 다 켠다
    }

    for key in ["covers", "does_not_cover", "inactive_guards"] {
        assert!(
            text.contains(key),
            "합성 코퍼스의 MANIFEST 에 `{key}` 가 없다 — 무엇을 못 재는지 \
             적지 않은 코퍼스는 '통과' 를 거짓으로 만든다"
        );
    }

    let declared: usize = text
        .split("\"inactive_guards\"")
        .nth(1)
        .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).find(|p| !p.is_empty()))
        .and_then(|n| n.parse().ok())
        .expect("MANIFEST 의 `inactive_guards` 가 수가 아니다");

    assert_eq!(
        REAL_CORPUS_ONLY.len(),
        declared,
        "실무 문서가 있어야 뜻이 있는 시험이 {}개인데 MANIFEST 는 {declared}개라고 \
         적었다.\n  꺼지는 것:\n    {}\n\n  수가 달라졌으면 **무엇이 꺼졌는지 보고** \
         MANIFEST 를 다시 적어라 — 수만 맞추면 이 장치가 뜻을 잃는다.",
        REAL_CORPUS_ONLY.len(),
        REAL_CORPUS_ONLY.join("\n    ")
    );
}
