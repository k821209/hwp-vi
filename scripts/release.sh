#!/usr/bin/env bash
# 배포용 실행 파일을 만든다 — **만든 사람의 집 경로가 박히지 않게.**
#
# 왜. rustc 는 패닉 메시지에 쓸 파일 경로를 실행 파일에 **문자열로 넣는다**.
# 그냥 `cargo build --release` 로 만든 바이너리에는
# `/home/<사용자>/.cargo/registry/src/...` 가 231번 들어 있었다(실측) — 받는
# 사람이 `strings` 한 번으로 만든 사람의 계정명을 읽는다. 공개 배포에서는
# 그게 의도가 아니다.
#
# `--remap-path-prefix` 로 바꿔 둔다. 디버그 정보가 아니라 **소스 경로 자체**를
# 바꾸므로 `strip` 으로는 지워지지 않는 자리까지 덮는다.
#
# 그리고 **만든 뒤에 확인한다.** 플래그를 적어 두는 것만으로는 "되어 있다" 가
# 아니다 — 빌드 캐시가 플래그 없이 만든 산출물을 재사용하면 조용히 섞인다.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TARGET="${CARGO_TARGET_DIR:-$ROOT/target/dist}"
REG="${CARGO_HOME:-$HOME/.cargo}/registry/src"

export CARGO_TARGET_DIR="$TARGET"
export RUSTFLAGS="--remap-path-prefix=$REG=/cargo --remap-path-prefix=$ROOT=/hwp-vi --remap-path-prefix=$HOME=/home/build"

echo "빌드합니다 (경로를 바꿔서)…"
cargo build --release --locked

BIN="$TARGET/release/hwp"
[ -x "$BIN" ] || { echo "실행 파일이 없습니다: $BIN" >&2; exit 1; }

# ---------------------------------------------------------------- 확인
#
# 사용자 이름과 집 경로가 **하나도** 남지 않아야 한다. 남으면 배포하지 않는다.
leak=0
for pat in "$HOME" "$(id -un)" "$ROOT"; do
  [ -n "$pat" ] || continue
  # `grep -c` 는 **줄 수**를 센다. 실행 파일의 문자열 한 줄은 수천 자라서
  # 한 줄에 열 번 들어 있어도 1로 세고, 보여 줄 때는 수천 자가 쏟아진다.
  # `-o` 로 일치한 조각만 센다.
  # `|| true` 가 필요하다 — `pipefail` 아래에서는 **일치가 없을 때** grep 이
  # 1 을 돌려주고 그게 파이프라인의 상태가 되어 `set -e` 가 스크립트를 죽인다.
  # 즉 확인이 **성공했을 때** 조용히 끝나 버렸다(실측: 아무 말 없이 종료).
  n=$(strings "$BIN" | { grep -oF -- "$pat" || true; } | wc -l)
  if [ "$n" -gt 0 ]; then
    echo "  ✗ $pat 가 $n번 남아 있습니다" >&2
    # 앞뒤 40자만 — 어디서 왔는지 알아볼 만큼
    strings "$BIN" | grep -oE ".{0,40}$(printf '%s' "$pat" | sed 's/[][\.*^$/]/\\&/g').{0,40}" \
      | head -3 | sed 's/^/      /' >&2
    leak=1
  else
    echo "  ✓ $pat 없음"
  fi
done
[ "$leak" -eq 0 ] || { echo "경로가 새서 배포할 수 없습니다" >&2; exit 1; }

size=$(du -h "$BIN" | cut -f1)
ver=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
pin=$(grep -m1 '^version' rhwp-pin.toml | sed 's/.*"\(.*\)".*/\1/')
echo
echo "$BIN  ($size)"
echo "  hwp-vi $ver · 백엔드 rhwp $pin 을 기대합니다"
echo
echo "릴리스에 같이 올릴 것: LICENSE · NOTICE.md · THIRD-PARTY.md"
echo "  (NOTICE.md 의 BSD-3-Clause 2항이 바이너리 배포에도 고지를 요구합니다)"
