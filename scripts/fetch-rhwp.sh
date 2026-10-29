#!/usr/bin/env bash
# rhwp 백엔드를 핀대로 받아 vendor/rhwp/ 에 푼다.
#
# 버전과 sha256 은 rhwp-pin.toml 이 정한다. 이 스크립트는 그걸 읽어서 쓰기만 하고,
# sha256 이 다르면 **설치하지 않고 멈춘다** — 받은 파일을 신뢰할 수 없다는 뜻이므로.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PIN="$ROOT/rhwp-pin.toml"
DEST="$ROOT/vendor/rhwp"

[ -f "$PIN" ] || { echo "핀 파일이 없다: $PIN" >&2; exit 1; }

case "$(uname -s)" in
  Linux)  OS=linux ;;
  Darwin) OS=macos ;;
  *) echo "지원하지 않는 OS: $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  aarch64|arm64) ARCH=aarch64 ;;
  x86_64|amd64)  ARCH=x86_64 ;;
  *) echo "지원하지 않는 아키텍처: $(uname -m)" >&2; exit 1 ;;
esac
TARGET="$OS-$ARCH"

read -r FILE WANT <<<"$(
  awk -v t="\"$TARGET\"" '
    $1 == t {
      match($0, /file = "[^"]+"/);   f = substr($0, RSTART+8, RLENGTH-9)
      match($0, /sha256 = "[^"]+"/); s = substr($0, RSTART+10, RLENGTH-11)
      print f, s; exit
    }' "$PIN"
)"
URL="$(awk -F'"' '/^release_url/ {print $2; exit}' "$PIN")/$FILE"

[ -n "${FILE:-}" ] && [ -n "${WANT:-}" ] || { echo "핀에 $TARGET 항목이 없다" >&2; exit 1; }

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
echo "내려받는다: $URL"
curl -fsSL "$URL" -o "$TMP/$FILE"

GOT="$(sha256sum "$TMP/$FILE" | cut -d' ' -f1)"
if [ "$GOT" != "$WANT" ]; then
  echo "sha256 불일치 — 설치하지 않는다" >&2
  echo "  기대: $WANT" >&2
  echo "  실제: $GOT" >&2
  exit 1
fi
echo "sha256 확인: $GOT"

mkdir -p "$DEST"
tar xzf "$TMP/$FILE" -C "$TMP"
BIN="$(find "$TMP" -type f -name rhwp -perm -u+x | head -1)"
[ -n "$BIN" ] || { echo "타르볼 안에 rhwp 실행 파일이 없다" >&2; exit 1; }
install -m 0755 "$BIN" "$DEST/rhwp"
for f in LICENSE README.md; do
  src="$(find "$TMP" -type f -name "$f" | head -1)"
  [ -n "$src" ] && cp "$src" "$DEST/$f"
done

echo "설치 완료: $DEST/rhwp"

# **받은 것이 이 기계에서 도는지 확인한다.**
#
# sha256 이 맞아도 못 돌 수 있다 — 릴리스 바이너리는 새 glibc 에 링크되어 있고
# 조금 오래된 리눅스에서는 동적 링커가 거부한다(실측 2026-10-10, WSL Ubuntu
# 20.04 = glibc 2.31 인데 바이너리는 2.39 를 요구했다). 그 경우 ld.so 가 쏟는
# 줄만 보면 "받다가 깨졌나" 로 읽히므로, 무엇이 왜 안 되는지 우리가 말한다.
if ! SMOKE="$("$DEST/rhwp" --version 2>&1)"; then
  echo
  if printf '%s' "$SMOKE" | grep -q "GLIBC_"; then
    NEED="$(printf '%s' "$SMOKE" | grep -o "GLIBC_[0-9.]*" | sed 's/GLIBC_//' \
            | sort -t. -k1,1n -k2,2n | tail -1)"
    HAVE="$(ldd --version 2>/dev/null | head -1 | grep -o '[0-9]\+\.[0-9]\+$' || echo '?')"
    cat >&2 <<MSG
받은 rhwp 가 이 리눅스에서 돌지 않는다 — **glibc 가 낡았다.**

  필요: glibc $NEED 이상
  현재: glibc $HAVE

sha256 은 맞았다. 받은 파일이 깨진 것이 아니라 **빌드된 환경이 더 새 것**이다.
musl 정적 빌드는 아직 릴리스에 없다. 둘 중 하나로 간다:

  (1) 더 새 배포판에서 쓴다 — WSL 이면 윈도우 쪽에서
        wsl --install Ubuntu-24.04
  (2) 소스에서 직접 빌드한다 (그 기계의 glibc 에 링크된다)
        ./scripts/build-rhwp.sh

실행 관문은 sha256 이 아니라 \`capabilities\`(스키마 major + 필수 명령)만 보므로
직접 빌드한 것도 똑같이 검사받고 똑같이 쓸 수 있다.
MSG
    # 못 도는 바이너리를 남겨 두면 vendor/ 가 \$PATH 의 멀쩡한 rhwp 를 가린다
    mv "$DEST/rhwp" "$DEST/rhwp.unusable"
    echo >&2
    echo "못 도는 바이너리는 $DEST/rhwp.unusable 로 치워 뒀다." >&2
  else
    echo "받은 rhwp 가 실행되지 않는다:" >&2
    printf '  %s\n' "$SMOKE" >&2
  fi
  exit 1
fi
printf '%s\n' "$SMOKE"
