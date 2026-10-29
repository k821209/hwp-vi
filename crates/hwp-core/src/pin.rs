//! `rhwp-pin.toml` 읽기와 검증.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct PinFile {
    pub rhwp: Pin,
}

#[derive(Debug, Deserialize)]
pub struct Pin {
    pub version: String,
    pub schema_version: String,
    pub release_url: String,
    pub artifacts: BTreeMap<String, Artifact>,
    pub required_commands: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Artifact {
    pub file: String,
    pub sha256: String,
}

impl Pin {
    /// 기본 위치(워크스페이스 루트의 `rhwp-pin.toml`)에서 읽는다.
    pub fn load_default() -> Result<Self> {
        // 설치된 바이너리에서도 동작하도록 핀을 컴파일 시점에 박아 넣는다.
        // 핀은 소스의 일부다 — 런타임에 바꿀 수 있으면 고정의 의미가 없다.
        let text = include_str!("../../../rhwp-pin.toml");
        Self::from_str(text)
    }

    pub fn from_str(text: &str) -> Result<Self> {
        let f: PinFile = toml::from_str(text).context("rhwp-pin.toml 파싱 실패")?;
        Ok(f.rhwp)
    }

    /// 이 플랫폼의 `target` 키 — 핀 파일의 `[rhwp.artifacts]` 키와 같은 형식.
    pub fn host_target() -> String {
        let os = if cfg!(target_os = "macos") { "macos" } else { "linux" };
        let arch = if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" };
        format!("{os}-{arch}")
    }

    pub fn artifact_for_host(&self) -> Result<&Artifact> {
        let key = Self::host_target();
        self.artifacts
            .get(&key)
            .with_context(|| format!("이 플랫폼({key})용 rhwp 산출물이 핀에 없다"))
    }

    pub fn download_url_for_host(&self) -> Result<String> {
        Ok(format!("{}/{}", self.release_url, self.artifact_for_host()?.file))
    }

    /// 다운로드한 타르볼이 핀의 sha256 과 같은지 확인한다.
    pub fn verify_artifact(&self, tarball: &Path) -> Result<()> {
        let want = &self.artifact_for_host()?.sha256;
        let got = sha256_file(tarball)?;
        if &got != want {
            bail!(
                "rhwp 산출물 sha256 불일치 — 받은 파일을 신뢰할 수 없다\n  파일: {}\n  기대: {want}\n  실제: {got}",
                tarball.display()
            );
        }
        Ok(())
    }

    /// `capabilities.schemaVersion` 의 major 만 비교한다.
    /// 필드 추가(minor)는 받아들이고, 의미 변경·삭제(major)는 거부한다.
    pub fn schema_major(&self) -> &str {
        self.schema_version.split('.').next().unwrap_or("0")
    }
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("{} 읽기 실패", path.display()))?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(format!("{:x}", h.finalize()))
}
