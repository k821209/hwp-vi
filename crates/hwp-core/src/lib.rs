//! hwp-cmd 코어.
//!
//! 이 크레이트는 HWP 를 **파싱하지 않는다.** 파싱·조판·쓰기는 rhwp 가 한다
//! (PLAN.md §7~§8). 여기 있는 것은 셋뿐이다:
//!
//! - [`pin`]    고정된 rhwp 버전·sha256·필수 명령
//! - [`rhwp`]   그 rhwp 프로세스를 찾고, 호환성을 확인하고, 호출하는 래퍼
//! - [`witness`] rhwp 와 **독립적인** 증인 — 저장 결과를 rhwp 아닌 코드로 센다
//!
//! 증인이 따로 있는 이유: 쓰기를 한 쪽이 그 쓰기를 검증하면 같은 버그를 두 번
//! 통과한다. §2-6 자가검증 저장이 서려면 검증자가 작성자와 달라야 한다.

pub mod dots;
pub mod edit;
pub mod grid;
pub mod images;
pub mod jamo;
pub mod json;
pub mod offset;
pub mod outline;
pub mod pin;
pub mod create;
pub mod text;
pub mod wrap;
pub mod rhwp;
pub mod screen;
pub mod hanja;
pub mod symbols;
pub mod witness;

pub use pin::Pin;
pub use rhwp::Rhwp;
pub use screen::ScreenDoc;
