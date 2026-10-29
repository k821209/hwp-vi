#!/usr/bin/env bash
# rhwp 백엔드를 **소스에서 직접** 빌드해 vendor/rhwp/ 에 놓는다.
#
# 왜 이게 필요한가: 공식 릴리스 바이너리는 새 glibc 에 링크되어 있어서
# 조금 오래된 리눅스에서 **동적 링커가 거부한다**(실측 2026-10-10, WSL
# Ubuntu 20.04 = glibc 2.31 인데 바이너리는 2.39 를 요구했다). musl 정적
# 빌드는 아직 릴리스에 없다. 직접 빌드하면 **그 기계의 glibc** 에 링크된다.
#
# 신뢰 모델이 바뀌는 점을 분명히 해 둔다: 타르볼의 sha256 대신 **git 태그**를
# 믿는다. 실행 관문(`capabilities`: 스키마 major + 필수 명령)은 그대로 걸린다 —
# 거기에는 sha256 이 들어가지 않으므로 직접 빌드한 것도 똑같이 검사받는다.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PIN="$ROOT/rhwp-pin.toml"
DEST="$ROOT/vendor/rhwp"
SRC="${HWP_RHWP_SRC:-$ROOT/vendor/rhwp-src}"

[ -f "$PIN" ] || { echo "핀 파일이 없다: $PIN" >&2; exit 1; }
VER="$(awk -F'"' '/^version/ {print $2; exit}' "$PIN")"
REPO="${HWP_RHWP_REPO:-https://github.com/edwardkim/rhwp}"
TAG="v$VER"

command -v cargo >/dev/null || {
  echo "cargo 가 없다. Rust 를 먼저 깔아라:" >&2
  echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
  exit 1
}
command -v git >/dev/null || { echo "git 이 없다." >&2; exit 1; }

# **핀이 정한 태그만 받는다.** 핀의 뜻이 "이 버전" 이므로 main 을 받으면 안 된다.
if [ -d "$SRC/.git" ]; then
  echo "이미 있다: $SRC"
  git -C "$SRC" fetch --depth 1 origin "refs/tags/$TAG:refs/tags/$TAG" 2>/dev/null || true
  git -C "$SRC" checkout -q "$TAG"
else
  echo "소스를 받는다: $REPO @ $TAG  (얕게 — 저장소 전체는 수 GB다)"
  mkdir -p "$(dirname "$SRC")"
  git clone --depth 1 --branch "$TAG" "$REPO" "$SRC"
fi

GOT_TAG="$(git -C "$SRC" describe --tags --exact-match 2>/dev/null || echo '?')"
[ "$GOT_TAG" = "$TAG" ] || {
  echo "받은 것이 $TAG 이 아니다 (현재: $GOT_TAG) — 핀과 다른 것을 빌드하지 않는다" >&2
  exit 1
}

# rust-toolchain.toml 이 버전을 박아 두었으므로 rustup 이 알아서 맞춰 받는다.
echo "빌드한다 (몇 분 걸린다 — 디스크도 몇 GB 쓴다)"
( cd "$SRC" && cargo build --release --bin rhwp )

BIN="$SRC/target/release/rhwp"
[ -x "$BIN" ] || { echo "빌드했는데 $BIN 이 없다" >&2; exit 1; }

mkdir -p "$DEST"
# 받아둔 것이 있으면 밀어내지 않고 옆에 치워 둔다
[ -f "$DEST/rhwp" ] && mv "$DEST/rhwp" "$DEST/rhwp.release-binary.bak"
install -m 0755 "$BIN" "$DEST/rhwp"
for f in LICENSE README.md; do
  [ -f "$SRC/$f" ] && cp "$SRC/$f" "$DEST/$f"
done

echo "설치 완료: $DEST/rhwp"
"$DEST/rhwp" --version
echo
echo "관문을 확인한다:"
( cd "$ROOT" && cargo run -q --release --bin hwp -- doctor ) || true
