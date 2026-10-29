#!/usr/bin/env bash
# 제3자 고지를 **다시 만든다**. 의존성을 바꿨으면 이것을 돌린다.
#
# 왜 스크립트인가. 고지는 손으로 적으면 반드시 낡는다 — 크레이트를 하나 더하고
# 고지를 안 고치면 **아무 경고도 없다**. `--fail` 을 쓰면 `about.toml` 에 없는
# 라이선스가 끼어들었을 때 만들기가 실패하므로, 새 라이선스는 사람이 보고
# 판단한 뒤에야 배포에 들어간다.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

command -v cargo-about >/dev/null \
  || { echo "cargo install cargo-about --locked --features cli 를 먼저 하세요" >&2; exit 1; }

cargo about generate --fail about.hbs -o THIRD-PARTY.md
n=$(grep -cE '^- \[' THIRD-PARTY.md || true)
echo "THIRD-PARTY.md 를 다시 만들었습니다 — 크레이트 $n개"
echo
echo "NOTICE.md 는 손으로 적은 것입니다(크레이트가 아닌 자원). 다음이 그대로인지 보세요:"
grep -nE '^## ' NOTICE.md
