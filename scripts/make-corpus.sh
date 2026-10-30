#!/usr/bin/env bash
# 회귀 코퍼스를 **명세에서 만든다** — 실무 문서 없이.
#
# 왜 있는가. 이 저장소의 회귀 관문은 실무 공문서 14개였고, 그 문서에는 사람
# 이름과 기관 내용이 들어 있어서 공개할 수 없다. 그렇다고 `samples/` 만 비우면
# **코퍼스 시험이 조용히 건너뛰고 "통과"로 보고된다** — 클론한 사람은 전수
# 대조가 돌았다고 오해한다. 그래서 공개 쪽은 **우리가 만든 문서**로 돈다.
#
# 만드는 길은 이 도구 자신이다(`hwp api` 의 `create`). 그래서 이 스크립트는
# 코퍼스를 만드는 동시에 **만들기 경로의 종단 시험**이기도 하다.
#
# ## 솔직하게 — 합성 코퍼스는 실무 문서보다 약하다
#
# 이 저장소에서 찾은 결함 대부분은 실무 문서의 **괴상함**이 찾아 줬다: 한컴
# 글꼴의 사용자 영역 글립, U+2007 고정폭 공백, 글머리 (들여쓰기, 기호) 조합
# 42가지, 배포용 암호화, 구역 4개짜리 공고문. 합성 문서에는 그게 없다.
#
# 그러니 이 코퍼스가 통과하는 것은 "짜 놓은 모양에서는 맞는다" 는 뜻이고,
# 실무 문서 회귀 관문은 비공개 저장소가 계속 들고 있다. `hwp doctor` 가
# 코퍼스가 합성인지 실무 문서인지 말한다 — 둘을 섞어 보고하지 않는다.
#
# 쓰기: ./scripts/make-corpus.sh [목적지]   (기본 samples/)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${1:-$ROOT/samples}"
HWP="$ROOT/target/release/hwp"
RHWP="${HWP_RHWP_BIN:-$ROOT/vendor/rhwp/rhwp}"
SRC="$ROOT/corpus-src"

[ -x "$HWP" ] || { echo "먼저 빌드하세요: cargo build --release" >&2; exit 1; }
[ -x "$RHWP" ] || { echo "백엔드가 없습니다. ./scripts/fetch-rhwp.sh 를 실행하세요" >&2; exit 1; }
[ -d "$SRC" ] || { echo "$SRC 가 없습니다 (그림 fixture 가 들어 있어야 합니다)" >&2; exit 1; }

mkdir -p "$DEST"

# 만들기는 **있는 파일을 덮지 않는다**(일부러 그렇게 만들었다). 다시 만들려면
# 먼저 치운다 — 합성 코퍼스는 언제든 다시 만들 수 있으므로 안전하다.
rm -f "$DEST"/c*.hwpx "$DEST"/c*.hwp "$DEST"/c*.bak*

say() { printf '  %-14s %s\n' "$1" "$2"; }

# ---------------------------------------------------------------- c00: 공문서 꼴
#
# 글머리 기호를 **문서가 실제로 쓰는 모양**으로 넣는다. 레벨 규칙이
# `(들여쓰기, 기호분류)` 를 키로 쓰므로 같은 기호가 들여쓰기 0·1·2 에 나오는
# 경우를 만들어야 개요 트리 시험이 뜻을 갖는다.
cat > /tmp/c00.json <<'EOF'
{"version":"1","title":"과업지시서",
 "page_size":{"width_mm":210,"height_mm":297},
 "blocks":[
  {"type":"heading","level":1,"text":"1. 과업명"},
  {"type":"paragraph","text":" ○ 기후변화 대응 작물 재배안정성 강화 기술개발"},
  {"type":"paragraph","text":"   ◦ 세부 과제 셋으로 나누어 추진"},
  {"type":"paragraph","text":"   ◦ 연차별 목표를 따로 둔다"},
  {"type":"heading","level":1,"text":"2. 추진 배경"},
  {"type":"paragraph","text":" □ 현장 문제 해결이 늦다"},
  {"type":"paragraph","text":"  - 기술 개발과 적용 사이의 간격"},
  {"type":"paragraph","text":"  - 검증 절차가 길다"},
  {"type":"paragraph","text":" □ 유사 사업과의 차별성 확보 필요"},
  {"type":"paragraph","text":"※ 본 계획은 협의 결과에 따라 달라질 수 있다"},
  {"type":"heading","level":2,"text":"가. 예산"},
  {"type":"table",
   "rows":[["항목","1년차","2년차","3년차"],
           ["인건비","120,000","130,000","140,000"],
           ["재료비","80,000","90,000","95,000"],
           ["합계","200,000","220,000","235,000"]],
   "cell_align":"center"}
 ]}
EOF

# ---------------------------------------------------------------- c01: 표가 여럿
cat > /tmp/c01.json <<'EOF'
{"version":"1","title":"신청서",
 "blocks":[
  {"type":"heading","level":1,"text":"신청 개요"},
  {"type":"table","rows":[["구분","내용"],["사업명","차세대 종자 기술 개발"],["기간","2026. 3. ~ 2028. 2."]]},
  {"type":"paragraph","text":" ○ 아래 표에 세부 내역을 적는다"},
  {"type":"table","rows":[["연번","항목","금액"],["1","장비","50,000"],["2","소모품","12,000"]],"cell_align":"right"},
  {"type":"paragraph","text":"끝."}
 ]}
EOF

# ---------------------------------------------------------------- c02: 아주 긴 단락
#
# 접히는 단락이 있어야 `wrap` 과 커서 오프셋 쪽 시험이 뜻을 갖는다 — 접힌 줄을
# 되붙여 본문을 만들던 결함이 바로 거기서 나왔다.
cat > /tmp/c02.json <<'EOF'
{"version":"1","title":"장문",
 "blocks":[
  {"type":"heading","level":1,"text":"긴 단락"},
  {"type":"paragraph","text":" ○ 이 단락은 한 줄에 들어가지 않을 만큼 길어서 터미널 폭에 따라 여러 줄로 접히게 되는데, 접힌 줄을 되붙여 본문을 만들면 줄을 끊은 자리의 공백이 사라져 커서 오프셋이 밀리므로 그 결함의 회귀를 지키려면 이렇게 긴 단락이 코퍼스에 있어야 한다."},
  {"type":"paragraph","text":"짧은 단락"},
  {"type":"paragraph","text":" ○ AI·디지털 전환, 첨단바이오, 기후변화 대응, 스마트농업, 미래식품 등 여러 분야를 가운뎃점으로 이어 적는 것이 공문서의 흔한 꼴이고, 그 가운뎃점은 East Asian Ambiguous 폭이라 터미널마다 다르게 그려진다."}
 ]}
EOF

# ---------------------------------------------------------------- c03: 체크박스
#
# 실측(실무 코퍼스 14개 문서): 박스가 있는 문서 9개, `□` 123개 · `☑` 10개 ·
# `☐` 2개. 본문 단락에 62개(단락마다 꼭 1개) 와 표 칸에 73개로 갈리고, **박스
# 든 칸의 절반은 박스가 2~9개**다. 그래서 둘 다 둔다 — 칸만 두면 절반을 놓치고,
# 칸에 하나만 두면 "어느 박스냐" 를 묻는 시험을 쓸 수 없다.
cat > /tmp/c03.json <<'EOF'
{"version":"1","title":"점검표",
 "blocks":[
  {"type":"heading","level":1,"text":"제출 서류 점검"},
  {"type":"paragraph","text":" □ 사업 참여 신청서"},
  {"type":"paragraph","text":" □ 사업자등록증 사본"},
  {"type":"paragraph","text":" ☑ 제출공문"},
  {"type":"table","rows":[["구분","해당 여부"],["지원 분야","내용"]]}
 ]}
EOF

for n in c00 c01 c02 c03; do
  echo "{\"create\":$(cat /tmp/$n.json),\"output\":\"$DEST/$n.hwpx\"}" \
    | "$HWP" api --compact > /tmp/$n.out 2>&1 \
    || { echo "만들기 실패 ($n):" >&2; cat /tmp/$n.out >&2; exit 1; }
  say "$n.hwpx" "$(python3 -c "
import json,sys
d=json.load(open('/tmp/$n.out'))
c=d['created']; print(f\"블록 {c['blocks']} · 단락 {c['paragraphs']} · 표 {c['tables']} · 칸 {c['cells']}\")")"
done

# ---------------------------------------------------------------- 여러 문단인 칸
#
# 한국 서식의 서술형 칸은 거의 다 여러 문단이다(실무 코퍼스 실측: 글자 있는 칸
# 1,517개 중 128개, 8%). 그게 없으면 `lines` 경로의 회귀를 지킬 수 없다.
echo '{"file":"'"$DEST"'/c01.hwpx","ops":[{"op":"set-cell","table":0,"row":1,"col":1,
  "lines":["차세대 종자 기술 개발",
           "  1) 1단계: 자원 확보 및 평가",
           "  2) 2단계: 계통 선발",
           "  3) 3단계: 현장 실증"]}]}' \
  | "$HWP" api --compact > /tmp/cell.out 2>&1 \
  || { echo "여러 문단 칸 쓰기 실패:" >&2; cat /tmp/cell.out >&2; exit 1; }
rm -f "$DEST"/c01.hwpx.bak*
say "c01.hwpx" "여러 문단 칸 1개 (4문단)"

# ---------------------------------------------------------------- 체크박스 마무리
#
# 칸 쪽 박스는 `lines` 로 넣는다 — 한 문단에 여러 개, 다른 문단에 하나.
echo '{"file":"'"$DEST"'/c03.hwpx","ops":[{"op":"set-cell","table":0,"row":1,"col":1,
  "lines":["□ 식량작물  □ 원예작물  □ 축산","□ 기타 (                    )"]}]}' \
  | "$HWP" api --compact > /tmp/c03cell.out 2>&1 \
  || { echo "칸 박스 쓰기 실패:" >&2; cat /tmp/c03cell.out >&2; exit 1; }
rm -f "$DEST"/c03.hwpx.bak*

# **꾸민 구간 안의 박스** — 이게 없으면 `check` 의 순서 관문이 뜻을 잃는다.
#
# 실무 서식에서 박스는 꾸민 구간의 **첫 글자**다
# (`[charPr 234: " "][charPr 266: "□ 유학생 유치 계획"]`). 그 자리에서 먼저
# 지우고 넣으면 새 글자가 **앞 구간**의 속성을 받아 색·글꼴·크기까지 갈린다.
# 꾸밈이 없는 문서에서는 두 순서가 똑같이 통과하므로 결함이 안 보인다.
if "$RHWP" edit apply-char-format "$DEST/c03.hwpx" --props '{"bold":true}' \
     --section 0 --para 2 --offset 1 -o "$DEST/.tmp.hwpx" >/dev/null 2>&1 \
   && [ -s "$DEST/.tmp.hwpx" ]; then
  mv "$DEST/.tmp.hwpx" "$DEST/c03.hwpx"
  say "c03.hwpx" "체크박스 — 본문 4개(1개는 진한 구간 안) · 칸 4개(한 문단에 3개)"
else
  rm -f "$DEST/.tmp.hwpx"
  echo "  ⚠ 진한 구간을 넣지 못했습니다 — check 의 순서 관문이 약해집니다" >&2
fi

# ---------------------------------------------------------------- 그림
#
# 그림은 `scaffold` 가 못 만들므로 백엔드의 `insert-image` 로 붙인다. 그림이
# 없으면 그림 시험 15개가 전부 건너뛴다 — 알파를 무시해 투명 배경이 새까맣게
# 읽히던 결함이 그 중 하나다. 그래서 fixture 에 **투명 여백이 있는** 그림을 둔다.
# `--page` 는 **0부터**이고 `--x/--y` 는 HWPUNIT 이다(픽셀이 아니다 — A4 세로가
# 59528×84188). 처음에 `--page 1` 로 줘서 "페이지 번호가 범위를 벗어났습니다
# (0~0)" 를 받았다. 백엔드가 분명히 거부해 줘서 금방 드러났다.
img() {
  "$RHWP" edit insert-image "$2" --image "$SRC/$1" \
    --page 0 --x "$4" --y "$5" -o "$3" 2>&1
}
place() {   # 파일, 그림, x, y
  local out="$DEST/.tmp.hwpx" err
  if err="$(img "$2" "$1" "$out" "$3" "$4")" && [ -s "$out" ]; then
    mv "$out" "$1"
    return 0
  fi
  rm -f "$out"
  echo "    ⚠ $2 를 붙이지 못했습니다: $err" >&2
  return 1
}
if place "$DEST/c00.hwpx" logo.png 10000 10000 \
   && place "$DEST/c00.hwpx" spectrum.png 10000 30000; then
  say "c00.hwpx" "그림 2개 (투명 여백 + 색이 많은 것)"
else
  # **조용히 넘기지 않는다.** 그림이 없으면 그림 시험 15개가 건너뛰므로
  # 코퍼스가 약해진 것을 만든 사람이 알아야 한다.
  echo "  ⚠ 그림이 빠졌습니다 — 그림 시험이 건너뜁니다" >&2
fi

# ---------------------------------------------------------------- 두 형식 다
#
# `scaffold` 는 HWPX 만 낸다. 그런데 이 도구는 **두 형식**(HWP5=CFB, HWPX=ZIP)을
# 다루고, 판독기도 저장 경로도 형식마다 다르다 — 한 형식만 있는 코퍼스는
# "두 형식에 다 되는가" 를 못 묻는다(`every_edit_kind_lands_on_both_formats` 가
# 바로 그걸 거부한다). `convert` 로 같은 문서의 HWP5 판을 만든다.
if "$RHWP" convert "$DEST/c00.hwpx" "$DEST/c00.hwp" --verify >/dev/null 2>&1; then
  say "c00.hwp" "HWP5(CFB) 판 — convert --verify 통과"
else
  echo "  ⚠ HWP5 판을 만들지 못했습니다 — 두 형식 시험이 한쪽만 돕니다" >&2
fi

# ---------------------------------------------------------------- 무엇을 덮는가
#
# 실무 코퍼스의 MANIFEST 는 **원본 경로**를 적어 사람 이름이 들어갔다. 여기는
# 경로가 아니라 **무엇을 덮는지**를 적는다 — 그게 코퍼스의 쓸모다.
cat > "$DEST/MANIFEST.json" <<EOF
{
  "kind": "synthetic",
  "made_by": "scripts/make-corpus.sh",
  "why": "실무 공문서에는 사람 이름과 기관 내용이 있어 공개할 수 없다. 그렇다고 코퍼스를 비우면 시험이 조용히 건너뛰고 통과로 보고된다.",
  "covers": [
    "공문서 글머리 (○ ◦ □ - ※) 와 들여쓰기 단계",
    "표 — 여러 개, 여러 행·열, 칸 정렬",
    "여러 문단인 칸 (서술형 칸의 꼴)",
    "접히는 긴 단락 (커서 오프셋 회귀)",
    "East Asian Ambiguous 글자 (가운뎃점)",
    "그림 — 투명 여백이 있는 것과 색이 많은 것",
    "체크박스 (□ ☑) — 본문과 표 칸, 한 칸에 여러 개",
    "꾸민 구간 안의 글자 (진하게) — 글자 속성이 사는지 보는 관문"
  ],
  "does_not_cover": [
    "한컴 글꼴의 사용자 영역 글립",
    "U+2007 고정폭 공백",
    "구역이 둘 이상인 문서",
    "배포용(복사 방지) 암호화",
    "글자 꾸밈 — 기울임·색·글꼴 (진하게 한 구간만 있다)",
    "글상자",
    "실무 문서의 글머리 조합 42가지",
    "강제 줄바꿈 (백엔드의 insert-text 가 줄바꿈을 거부해 만들 수 없다)"
  ],
  "inactive_guards": 21,
  "inactive_guards_note": "실무 문서가 있어야 뜻이 있는 시험 수. 합성 코퍼스에서는 꺼지고, 꺼진 수가 이 수와 다르면 the_corpus_declares_what_it_cannot_check 가 깨진다.",
  "note": "덮지 못하는 것이 결함을 찾아 준 것들이다. 실무 문서 회귀 관문은 비공개 저장소가 들고 있다."
}
EOF
say "MANIFEST.json" "덮는 것 / 못 덮는 것"

echo
echo "코퍼스를 만들었습니다: $DEST"
ls -la "$DEST"/c*.hwp "$DEST"/c*.hwpx 2>/dev/null | awk '{printf "  %6d B  %s\n", $5, $9}'
