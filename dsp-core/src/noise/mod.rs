//! 해양 배경잡음 스펙트럼 (M2B 기준선).
//!
//! - Coates의 Wenz 곡선 공학 근사: 난류·원거리 선박·바람/표면·열잡음
//! - 강우 게임 근사 (후속 교체 대상)
//!
//! 준위 단위는 dB re 1µPa²/Hz (단위 Hz당 스펙트럼 밀도).
//! 상수 출처: `docs/물리 상수 시트.md`.

/// Wenz 계열 공학 근사의 네 가지 독립 성분 (dB re 1µPa²/Hz).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmbientNoiseLevels {
    pub turbulence_db: f32,
    pub shipping_db: f32,
    pub wind_db: f32,
    pub thermal_db: f32,
}

/// Coates(1990)가 정리한 Wenz 곡선 공학 근사의 성분별 스펙트럼 준위.
///
/// `freq_hz`: Hz (식 내부에서는 반드시 kHz로 변환),
/// `shipping_activity`: 0(낮음)..1(높음), `wind_speed_ms`: m/s.
pub fn coates_wenz_components(
    freq_hz: f32,
    shipping_activity: f32,
    wind_speed_ms: f32,
) -> AmbientNoiseLevels {
    let f_khz = (freq_hz.max(1e-3)) / 1000.0;
    let shipping = shipping_activity.clamp(0.0, 1.0);
    let wind = wind_speed_ms.max(0.0);
    AmbientNoiseLevels {
        turbulence_db: 17.0 - 30.0 * f_khz.log10(),
        shipping_db: 40.0 + 20.0 * (shipping - 0.5) + 26.0 * f_khz.log10()
            - 60.0 * (f_khz + 0.03).log10(),
        wind_db: 50.0 + 7.5 * wind.sqrt() + 20.0 * f_khz.log10()
            - 40.0 * (f_khz + 0.4).log10(),
        thermal_db: -15.0 + 20.0 * f_khz.log10(),
    }
}

/// Coates/Wenz 네 성분을 전력 합성한 주변 소음 스펙트럼 준위.
pub fn coates_wenz_noise_level_db(
    freq_hz: f32,
    shipping_activity: f32,
    wind_speed_ms: f32,
) -> f32 {
    let levels = coates_wenz_components(freq_hz, shipping_activity, wind_speed_ms);
    combine_db_levels(&[
        levels.turbulence_db,
        levels.shipping_db,
        levels.wind_db,
        levels.thermal_db,
    ])
}

/// 강우 배경잡음 스펙트럼 준위 (dB re 1µPa²/Hz).
///
/// `rain_mm_hr <= 0`이면 기여 없음 (`NEG_INFINITY`).
/// 주파수/강우량에 대해 단조 증가 (게임 근사, 상수 시트 참조).
pub fn rain_noise_level_db(freq_hz: f32, rain_mm_hr: f32) -> f32 {
    if rain_mm_hr <= 0.0 {
        return f32::NEG_INFINITY;
    }
    let f = freq_hz.max(1e-3);
    30.0 + 20.0 * (1.0 + rain_mm_hr).log10() + 20.0 * (f / 1000.0).log10()
}

/// 다중 잡음원을 전력 합성 (dB).
///
/// `10·log10(Σ 10^(Li/10))`. `NEG_INFINITY` 항목은 0 전력으로 처리.
pub fn combine_db_levels(levels: &[f32]) -> f32 {
    let power: f32 = levels
        .iter()
        .filter(|l| l.is_finite())
        .map(|l| 10f32.powf(*l / 10.0))
        .sum();
    if power <= 0.0 {
        return f32::NEG_INFINITY;
    }
    10.0 * power.log10()
}

/// 종합 해양 배경잡음 NL (dB re 1µPa²/Hz).
///
/// Coates/Wenz 주변 소음 + 강우 근사를 전력 합성.
pub fn ocean_noise_level_db(
    freq_hz: f32,
    wind_speed_ms: f32,
    rain_mm_hr: f32,
    shipping_activity: f32,
) -> f32 {
    combine_db_levels(&[
        coates_wenz_noise_level_db(freq_hz, shipping_activity, wind_speed_ms),
        rain_noise_level_db(freq_hz, rain_mm_hr),
    ])
}

// ---------------------------------------------------------------------------
// 샘플 합성 (M2): 준위 모델(위) → 실제 잡음 신호
// ---------------------------------------------------------------------------

/// 해양 배경잡음 샘플 합성기 (M2A 호환 구현).
///
/// Coates/Wenz 준위 모델을 현재의 단순 신호 생성기에 연결:
/// - xorshift64 백색 잡음 (결정적 — 멀티플레이 동기화 용이)
/// - 1-pole 저역통과 (커트오프가 풍속에 비례 → 바람이 세면 저역 에너지 증가)
/// - 출력 RMS는 100Hz 기준 준위(NL) + 1kHz 대역폭으로 스케일
///
/// 준위 규약: 200 µPa = 1.0 full scale (120 dB re 1µPa).
#[derive(Debug, Clone)]
pub struct OceanNoise {
    state: u64,
    lp: f32,
    coeff: f32,
    gain: f32,
}

impl OceanNoise {
    /// 풍속/강우로 잡음 합성기 생성.
    pub fn new(sample_rate: f32, wind_speed_ms: f32, rain_mm_hr: f32) -> Self {
        // 아직 합성 필터는 M2A의 1-pole 근사다. M2B 필터뱅크 전환 전까지
        // Coates/Wenz의 100 Hz 중간 선박 활동 기준으로 절대 RMS만 교정한다.
        let nl_ref = ocean_noise_level_db(100.0, wind_speed_ms, rain_mm_hr, 0.5);
        // 100Hz 기준 스펙트럼 밀도(dB/Hz) + 1kHz 대역 → RMS amplitude
        let gain = 10f32.powf((nl_ref + 30.0 - 120.0) / 20.0);
        // 바람이 세면 저역 커트오프 상승 (에너지가 고역으로 퍼짐)
        let cutoff = 20.0 + 10.0 * wind_speed_ms.max(0.0);
        let coeff = 1.0 - (-2.0 * std::f32::consts::PI * cutoff / sample_rate).exp();
        Self {
            state: 0x9E37_79B9_7F4A_7C15,
            lp: 0.0,
            coeff,
            gain,
        }
    }

    /// 다음 샘플 (−1..1, 준위 스케일 적용됨).
    pub fn next_sample(&mut self) -> f32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        let white = ((x >> 11) as f32 / 9007199254740992.0) * 2.0 - 1.0;
        self.lp += self.coeff * (white - self.lp);
        // 1-pole LP가 RMS를 줄이므로 3배 보상 (근사)
        self.lp * self.gain * 3.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_scales_with_wind() {
        let calm = coates_wenz_noise_level_db(1000.0, 0.5, 0.0);
        let storm = coates_wenz_noise_level_db(1000.0, 0.5, 15.0);
        assert!(storm > calm);
    }

    #[test]
    fn coates_wenz_matches_golden_table() {
        let csv = include_str!("../../../data/acoustics/golden/coates_wenz_deep_water.csv");
        for line in csv.lines().filter(|line| {
            !line.is_empty() && !line.starts_with('#') && !line.starts_with("frequency_hz")
        }) {
            let values: Vec<f32> = line
                .split(',')
                .map(|value| value.parse::<f32>().expect("골든 CSV 숫자"))
                .collect();
            assert_eq!(values.len(), 6);
            let levels = coates_wenz_components(values[0], 0.5, 5.0);
            let actual = [
                levels.turbulence_db,
                levels.shipping_db,
                levels.wind_db,
                levels.thermal_db,
                coates_wenz_noise_level_db(values[0], 0.5, 5.0),
            ];
            for (got, expected) in actual.iter().zip(&values[1..]) {
                assert!(
                    (got - expected).abs() < 0.002,
                    "{} Hz: {got} != {expected}",
                    values[0]
                );
            }
        }
    }

    #[test]
    fn coates_wenz_components_dominate_expected_bands() {
        let low = coates_wenz_components(5.0, 0.5, 5.0);
        assert!(low.turbulence_db > low.shipping_db);

        let traffic = coates_wenz_components(50.0, 0.5, 0.0);
        assert!(traffic.shipping_db > traffic.wind_db);

        let surface = coates_wenz_components(1000.0, 0.5, 5.0);
        assert!(surface.wind_db > surface.shipping_db);

        let ultrasonic = coates_wenz_components(500_000.0, 0.5, 5.0);
        assert!(ultrasonic.thermal_db > ultrasonic.wind_db);
    }

    #[test]
    fn rain_absent_when_zero() {
        // 0 강우 → 종합 NL == Coates/Wenz 네 성분 합계
        let f = 200.0;
        let wind = 8.0;
        let pure = coates_wenz_noise_level_db(f, 0.5, wind);
        let combined = ocean_noise_level_db(f, wind, 0.0, 0.5);
        assert!((pure - combined).abs() < 1e-4);
    }

    #[test]
    fn ocean_noise_increases_with_rain() {
        let dry = ocean_noise_level_db(1000.0, 5.0, 0.0, 0.5);
        let wet = ocean_noise_level_db(1000.0, 5.0, 20.0, 0.5);
        assert!(wet > dry);
    }

    #[test]
    fn combine_is_dominant_source() {
        // 훨씬 큰 준위가 합성을 지배
        let combined = combine_db_levels(&[60.0, 40.0]);
        // 10log10(10^6 + 10^4) = 10log10(1.01e6) ≈ 60.043
        assert!((combined - 60.043).abs() < 0.01);
    }

    // --- M2: 샘플 합성 ---

    fn rms(n: &mut OceanNoise, count: usize) -> f32 {
        let mut acc = 0f32;
        for _ in 0..count {
            let s = n.next_sample();
            acc += s * s;
        }
        (acc / count as f32).sqrt()
    }

    #[test]
    fn ocean_noise_is_deterministic() {
        let mut a = OceanNoise::new(44100.0, 5.0, 0.0);
        let mut b = OceanNoise::new(44100.0, 5.0, 0.0);
        for _ in 0..1000 {
            assert_eq!(a.next_sample(), b.next_sample());
        }
    }

    #[test]
    fn ocean_noise_rms_rises_with_wind() {
        let mut calm = OceanNoise::new(44100.0, 0.0, 0.0);
        let mut storm = OceanNoise::new(44100.0, 15.0, 0.0);
        let rms_calm = rms(&mut calm, 16384);
        let rms_storm = rms(&mut storm, 16384);
        assert!(rms_storm > rms_calm * 1.5, "{rms_storm} vs {rms_calm}");
    }

    #[test]
    fn ocean_noise_bounded_and_nonzero() {
        let mut n = OceanNoise::new(44100.0, 8.0, 5.0);
        for _ in 0..16384 {
            let s = n.next_sample();
            assert!(s.abs() < 1.0);
        }
        assert!(rms(&mut n, 16384) > 1e-5);
    }
}
