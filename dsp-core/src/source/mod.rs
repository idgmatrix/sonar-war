//! 엔티티 소음 모델 (M2).
//!
//! 패시브 소나에서 함선이 만드는 신호의 성분:
//! - **토널 (LOFAR 라인)**: 블레이드 패싱 레이트 기본파 + 고조파 (고조파당 −6 dB)
//! - **캐비테이션 광대역**: 프로펠러 팁 속도(rpm)·캐비테이션 강도에 비례
//! - **DEMON 변조**: 캐비테이션 광대역의 포락선을 블레이드 레이트로 AM
//!   → 스펙트럼에 DEMON 라인(블레이드 레이트 + 고조파)이 나타남
//! - **도플러 시프트**: 상대 속도 기반 주파수 스케일 `1 + v/c`
//!
//! 상수 출처: `docs/물리 상수 시트.md`.

use std::f32::consts::PI;

/// 합성하는 토널 고조파 수.
pub const TONAL_HARMONICS: u32 = 5;

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

/// 캐비테이션 광대역 준위 (dB re 1µPa).
///
/// 프로펠러 팁 속도에 비례 (rpm, 100rpm 기준 100 dB) +
/// 캐비테이션 강도(0..1)에 20 dB 스케일.
pub fn broadband_level_db(rpm: f32, cavitation: f32) -> f32 {
    100.0 + 20.0 * (rpm.max(1.0) / 100.0).log10() + 20.0 * cavitation.clamp(0.0, 1.0)
}

/// 도플러 주파수 시프트 계수.
///
/// `rel_vel_ms`: 수신기 방향 상대 속도 (m/s, 접근 시 +),
/// `sound_speed_ms`: 음속 (m/s).
/// 도플러 계수 = 1 + v/c.
pub fn doppler_factor(rel_vel_ms: f32, sound_speed_ms: f32) -> f32 {
    1.0 + rel_vel_ms / sound_speed_ms
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
    fn approaching_target_raises_frequency() {
        let f = doppler_factor(10.0, 1500.0);
        assert!(f > 1.0);
    }

    #[test]
    fn receding_target_lowers_frequency() {
        let f = doppler_factor(-10.0, 1500.0);
        assert!(f < 1.0);
    }
}
