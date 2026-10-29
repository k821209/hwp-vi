# 고지 (Notices)

이 배포에 들어 있는 **남의 것**을 적는다. 두 파일로 갈라 둔다 —

- **[THIRD-PARTY.md](THIRD-PARTY.md)** — Rust 크레이트. `cargo about` 이 만든다
  (`./scripts/make-notices.sh`). 기계가 세므로 낡지 않는다.
- **이 파일** — 크레이트가 아닌 것. 기계가 셀 수 없으므로 **손으로 적는다.**
  그래서 여기 적힌 것이 빠지면 아무 경고도 없다. 자원을 더하면 여기도 더할 것.

## 한자 음절 표 (BSD-3-Clause)

`crates/hwp-core/data/hanja-syllables.txt` — libhangul 의 한자 사전에서 왔고
실행 파일에 `include_str!` 로 **들어간다**. 전문은
[`crates/hwp-core/data/LICENSE.hanja`](crates/hwp-core/data/LICENSE.hanja).

```
Copyright (c) 2005,2006 Choe Hwanjin
All rights reserved.
```

BSD-3-Clause 2항은 **바이너리 배포에도 이 고지를 함께 실으라**고 요구한다.
그래서 이 파일이 릴리스에 같이 올라간다 — 실행 파일만 올리면 조건을 못 지킨다.

## rhwp 백엔드 (MIT) — **들어 있지 않다. 받아 온다**

이 도구는 HWP 판독·쓰기를 [rhwp](https://github.com/edwardkim-dev/rhwp)(MIT,
edwardkim) 에 맡긴다. 그 실행 파일은 **이 저장소에 없고 배포에도 없다** —
`./scripts/fetch-rhwp.sh` 가 `rhwp-pin.toml` 에 적힌 버전과 해시로 받아 온다.

그래서 rhwp 의 라이선스 전문은 그쪽 배포가 들고 오며, 우리가 다시 실을 것이
없다. 대신 **어느 버전에 기대는지**는 `rhwp-pin.toml` 이 기록한다 — 태그는
불변이 아니어서(상류가 같은 태그의 산출물을 교체한 적이 있다) 해시를 함께 둔다.

## 코퍼스 그림 (우리 것)

`corpus-src/logo.png`·`spectrum.png` 는 시험용으로 **직접 만든** 그림이다
(zlib 로 손으로 짰고, 외부 자료가 아니다). 이 저장소의 라이선스를 따른다.
