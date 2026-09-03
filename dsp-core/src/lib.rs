//! # dsp-core
//!
//! SSN-X 소나 시뮬레이터의 사실적 음향 합성 DSP 코어.
//!
//! 아키텍처 (docs/개발 계획.md):
//! - Rust/WASM이 DSP 본체 — 해양 잡음 합성, 엔티티 소음 합성(토널/캐비테이션/DEMON/도플러),
//!   풀 지연-합 빔포밍.
//! - Web Audio API는 출력 경로(게인/라우팅/사후 필터)만 담당.
//!
//! WASM 바인딩은 `process()` 단방향 API로 유지한다. JS ↔ WASM 경계에서
//! Float32Array 링버퍼를 재사용해 할당을 0회로 만든다.

pub mod beamform;
pub mod noise;
pub mod physics;
pub mod source;

use wasm_bindgen::prelude::*;

/// DSP 엔진. AudioWorklet이 1개 인스턴스를 보유하며, 매 process() 블록을 처리한다.
///
/// M0 스텁: 44.1kHz 정현파를 출력해 "WASM이 AudioWorklet에서 실행된다"는 것을 검증한다.
#[wasm_bindgen]
pub struct DspEngine {
    sample_rate: f32,
    phase: f64,
    /// 정현파 주파수 (Hz) — M0 검증용
    sine_freq: f32,
}

#[wasm_bindgen]
impl DspEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
            sine_freq: 440.0,
        }
    }

    /// 정현파 주파수 설정 (M0 검증용)
    pub fn set_sine_freq(&mut self, hz: f32) {
        self.sine_freq = hz;
    }

    /// `frames` 프레임(채널당 샘플 수)을 처리해 `out` (len >= frames)에 mono 샘플을 쓴다.
    ///
    /// M0: 440Hz 정현파. 이후: 엔티티 소음 합성 + 빔포밍 출력.
    pub fn process(&mut self, out: &mut [f32]) {
        let two_pi = std::f32::consts::TAU;
        let inc = two_pi * self.sine_freq / self.sample_rate;
        for slot in out.iter_mut() {
            let s = (self.phase as f32).sin();
            *slot = s * 0.5; // -0.5..0.5, 클리핑 여지 확보
            self.phase += inc as f64;
            if self.phase >= two_pi as f64 {
                self.phase -= two_pi as f64;
            }
        }
    }
}
