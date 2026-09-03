//! 엔티티 소음 모델 (M2).
//!
//! 패시브 소나에서 함선이 만드는 신호의 성분:
//! - **토널 (LOFAR 라인)**: 블레이드 패싱 레이트 기본파 + 고조파 (고조파당 −6 dB)
//! - **캐비테이션 광대역**: 프로펠러 팁 속도(rpm)·캐비테이션 강도에 비례
//! - **DEMON 변조**: 캐비테이션 광대역의 포락선을 블레이드 레이트로 AM
//!   → 스펙트럼에 DEMON 라인(블레이드 레이트 + 고조파)이 나타남
//!
//! 상수 출처: `docs/물리 상수 시트.md`.

use std::f32::consts::PI;

/// 합성하는 토널 고조파 수.
pub const TONAL_HARMONICS: u32 = 5;

/// Source 계층이 내보내는 단일 협대역 선. 전파·수신기 효과는 포함하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourceLine {
    pub frequency_hz: f32,
    pub level_db_re_1upa_at_1m: f32,
}

/// 한 표적의 원시 방사 스펙트럼 제어 프레임.
///
/// 단위는 소스 고유의 `dB re 1 µPa @ 1 m`이며 거리, 도플러, 배열 응답을 포함하지 않는다.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceSpectrum {
    pub tonal_lines: [SourceLine; TONAL_HARMONICS as usize],
    pub broadband_level_db_re_1upa_at_1m: f32,
    pub modulation_rate_hz: f32,
}

/// 한 방사 시각의 1 m 기준 선형 음압 성분 (µPa).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourceSample {
    pub tonal_pressure_1m_upa: [f32; TONAL_HARMONICS as usize],
    pub broadband_pressure_1m_upa: f32,
}

/// 상태형 Source 합성기. 전파 환경과 수신기 설정은 갖지 않는다.
#[derive(Debug, Clone)]
pub struct SourceVoice {
    spectrum: SourceSpectrum,
    tonal_amplitude_1m_upa: [f32; TONAL_HARMONICS as usize],
    broadband_amplitude_1m_upa: f32,
    noise_history: Vec<f32>,
    noise_position: usize,
    rng: u64,
}

impl SourceVoice {
    pub fn new(spectrum: SourceSpectrum, history_samples: usize, seed: u64) -> Self {
        let tonal_amplitude_1m_upa = std::array::from_fn(|index| {
            10f32.powf(spectrum.tonal_lines[index].level_db_re_1upa_at_1m / 20.0)
        });
        let broadband_amplitude_1m_upa =
            10f32.powf(spectrum.broadband_level_db_re_1upa_at_1m / 20.0);
        Self {
            spectrum,
            tonal_amplitude_1m_upa,
            broadband_amplitude_1m_upa,
            noise_history: vec![0.0; history_samples.max(2)],
            noise_position: 0,
            rng: seed,
        }
    }

    pub fn spectrum(&self) -> &SourceSpectrum {
        &self.spectrum
    }

    /// `source_time_s`의 원시 방사 성분을 읽는다.
    ///
    /// 광대역 히스토리 오프셋은 Propagation이 계산하며 Source는 그 의미를 해석하지 않는다.
    pub fn sample_at(&self, source_time_s: f64, broadband_history_offset: f32) -> SourceSample {
        let tonal_pressure_1m_upa = std::array::from_fn(|index| {
            let frequency_hz = self.spectrum.tonal_lines[index].frequency_hz as f64;
            self.tonal_amplitude_1m_upa[index]
                * (2.0 * std::f64::consts::PI * frequency_hz * source_time_s).cos() as f32
        });
        let envelope =
            demon_envelope((self.spectrum.modulation_rate_hz as f64 * source_time_s) as f32);
        SourceSample {
            tonal_pressure_1m_upa,
            broadband_pressure_1m_upa: self.broadband_amplitude_1m_upa
                * self.read_noise(broadband_history_offset)
                * envelope,
        }
    }

    /// Source 시간축을 한 샘플 전진시켜 캐비테이션 난수 히스토리를 갱신한다.
    pub fn advance(&mut self) {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        let white = ((x >> 11) as f32 / 9_007_199_254_740_992.0) * 2.0 - 1.0;
        self.noise_history[self.noise_position] = white;
        self.noise_position = (self.noise_position + 1) % self.noise_history.len();
    }

    fn read_noise(&self, offset_samples: f32) -> f32 {
        let capacity = self.noise_history.len();
        let offset = offset_samples.max(0.0).min(capacity as f32 - 2.0);
        let whole = offset.floor() as usize;
        let fraction = offset - whole as f32;
        let newest = if self.noise_position == 0 {
            capacity - 1
        } else {
            self.noise_position - 1
        };
        let first = newest.wrapping_sub(whole) % capacity;
        let second = newest.wrapping_sub(whole + 1) % capacity;
        self.noise_history[first] * (1.0 - fraction) + self.noise_history[second] * fraction
    }
}

/// 운항 상태로부터 전파 전 원시 방사 스펙트럼을 만든다.
pub fn source_spectrum(
    rpm: f32,
    blade_count: u32,
    tonal_level_db_re_1upa_at_1m: f32,
    cavitation: f32,
) -> SourceSpectrum {
    let blade_rate = blade_rate_hz(rpm, blade_count).max(0.1);
    let tonal_lines = std::array::from_fn(|index| {
        let harmonic = index as u32 + 1;
        SourceLine {
            frequency_hz: blade_rate * harmonic as f32,
            level_db_re_1upa_at_1m: tonal_harmonic_level_db(harmonic, tonal_level_db_re_1upa_at_1m),
        }
    });
    SourceSpectrum {
        tonal_lines,
        broadband_level_db_re_1upa_at_1m: broadband_level_db(rpm, cavitation),
        modulation_rate_hz: blade_rate,
    }
}

/// 블레이드 패싱 레이트 (Hz) = rpm/60 × 블레이드 수.
pub fn blade_rate_hz(rpm: f32, blade_count: u32) -> f32 {
    rpm / 60.0 * blade_count as f32
}

/// 토널 기본파 주파수 (Hz) — LOFAR 라인 = 블레이드 레이트.
pub fn tonal_fundamental_hz(rpm: f32, blade_count: u32) -> f32 {
    blade_rate_hz(rpm, blade_count)
}

/// 토널 고조파 준위 (dB).
///
/// 기본파(n=1)는 `fundamental_level_db`, 고조파 하나당 −6 dB.
pub fn tonal_harmonic_level_db(harmonic: u32, fundamental_level_db: f32) -> f32 {
    fundamental_level_db - 6.0 * (harmonic as f32 - 1.0)
}

/// DEMON 포락선 (0..1) — 캐비테이션 광대역을 블레이드 레이트로 AM.
///
/// `phase`: 블레이드 레이트 기준 위상 (cycle).
pub fn demon_envelope(phase: f32) -> f32 {
    0.5 + 0.5 * (2.0 * PI * phase).cos()
}

/// 캐비테이션 광대역 소스 준위 (dB re 1µPa @ 1m).
///
/// 프로펠러 팁 속도에 비례 (rpm, 100rpm 기준 100 dB) +
/// 캐비테이션 강도(0..1)에 20 dB 스케일.
pub fn broadband_level_db(rpm: f32, cavitation: f32) -> f32 {
    100.0 + 20.0 * (rpm.max(1.0) / 100.0).log10() + 20.0 * cavitation.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blade_rate_is_rpm_times_blades() {
        assert!((blade_rate_hz(60.0, 5) - 5.0).abs() < 1e-6);
        assert!((blade_rate_hz(90.0, 4) - 6.0).abs() < 1e-6);
    }

    #[test]
    fn tonal_fundamental_equals_blade_rate() {
        assert!((tonal_fundamental_hz(120.0, 6) - 12.0).abs() < 1e-6);
    }

    #[test]
    fn harmonics_drop_6db_each() {
        let l1 = tonal_harmonic_level_db(1, 150.0);
        let l3 = tonal_harmonic_level_db(3, 150.0);
        assert!((l1 - 150.0).abs() < 1e-6);
        assert!((l3 - 138.0).abs() < 1e-6);
    }

    #[test]
    fn demon_envelope_bounded_and_periodic() {
        for i in 0..100 {
            let e = demon_envelope(i as f32 / 100.0);
            assert!((0.0..=1.0).contains(&e));
        }
        assert!((demon_envelope(0.0) - demon_envelope(1.0)).abs() < 1e-6);
        assert!((demon_envelope(0.25) - demon_envelope(0.75)).abs() < 1e-6);
    }

    #[test]
    fn broadband_rises_with_rpm_and_cavitation() {
        assert!(broadband_level_db(120.0, 0.5) > broadband_level_db(60.0, 0.5));
        assert!(broadband_level_db(90.0, 0.8) > broadband_level_db(90.0, 0.1));
    }

    #[test]
    fn source_spectrum_contains_no_geometry_effects() {
        let spectrum = source_spectrum(120.0, 5, 150.0, 0.4);
        assert_eq!(spectrum.tonal_lines[0].frequency_hz, 10.0);
        assert_eq!(spectrum.tonal_lines[0].level_db_re_1upa_at_1m, 150.0);
        assert_eq!(spectrum.modulation_rate_hz, 10.0);
    }

    #[test]
    fn source_voice_is_deterministic_and_uses_source_level_units() {
        let spectrum = source_spectrum(120.0, 5, 120.0, 0.4);
        let mut first = SourceVoice::new(spectrum.clone(), 32, 7);
        let mut second = SourceVoice::new(spectrum, 32, 7);
        for index in 0..64 {
            assert_eq!(
                first.sample_at(index as f64 / 44100.0, 0.0),
                second.sample_at(index as f64 / 44100.0, 0.0)
            );
            first.advance();
            second.advance();
        }
        let sample = first.sample_at(0.0, 0.0);
        assert!((sample.tonal_pressure_1m_upa[0] - 1_000_000.0).abs() < 1.0);
    }
}
