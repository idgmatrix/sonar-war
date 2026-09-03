//! 해양 배경잡음 스펙트럼 (M1).
//!
//! - Knudsen C₀ 모델 (풍속 스케일) — 주 배경잡음
//! - 강우 / 함선 옵션 (광대역 기여, 전력 합성)
//!
//! 준위 단위는 dB re 1µPa²/Hz (단위 Hz당 스펙트럼 밀도).
//! 상수 출처: `docs/물리 상수 시트.md`.

/// Knudsen C₀ 해양 배경잡음 스펙트럼 준위 (dB re 1µPa²/Hz).
///
/// Knudsen et al. (1960) C₀(무풍) 스펙트럼 + 풍속 스케일 항.
///
/// ```text
/// NL = Nw + 26.2·log10(f/100) + 1.3·(f/100)
///        + (40 − Nw)·log10(1 + 0.03·f)
///        + 0.014·sqrt(1 + 0.4·f)
///        + 20·log10(1 + 3e-5·W^2.5)
/// ```
///
/// `freq_hz`: 주파수 (Hz), `wind_speed_ms`: 풍속 (m/s).
pub fn knudsen_noise_level_db(freq_hz: f32, wind_speed_ms: f32) -> f32 {
    const NW: f32 = 50.0; // C₀: 100Hz 무풍 스펙트럼 준위
    let f = freq_hz.max(1e-3);
    let f100 = f / 100.0;
    let wind_term = 20.0 * (1.0 + 3e-5 * wind_speed_ms.max(0.0).powf(2.5)).log10();
    NW + 26.2 * f100.log10()
        + 1.3 * f100
        + (40.0 - NW) * (1.0 + 0.03 * f).log10()
        + 0.014 * (1.0 + 0.4 * f).sqrt()
        + wind_term
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

/// 함선 자체/주변 함선 광대역 잡음 스펙트럼 준위 (dB re 1µPa²/Hz).
///
/// `ship_speed_kn <= 0`이면 기여 없음 (`NEG_INFINITY`).
/// 저주파에 편중, 속도에 단조 증가 (게임 근사, 상수 시트 참조).
pub fn ship_noise_level_db(freq_hz: f32, ship_speed_kn: f32) -> f32 {
    if ship_speed_kn <= 0.0 {
        return f32::NEG_INFINITY;
    }
    let f = freq_hz.max(1e-3);
    110.0 + 20.0 * ship_speed_kn.max(0.0).log10() + 20.0 * (100.0 / f).log10()
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
/// Knudsen C₀ + 강우 + 함선을 전력 합성.
/// `rain_mm_hr`/`ship_speed_kn`이 0이면 해당 기여는 생략.
pub fn ocean_noise_level_db(
    freq_hz: f32,
    wind_speed_ms: f32,
    rain_mm_hr: f32,
    ship_speed_kn: f32,
) -> f32 {
    combine_db_levels(&[
        knudsen_noise_level_db(freq_hz, wind_speed_ms),
        rain_noise_level_db(freq_hz, rain_mm_hr),
        ship_noise_level_db(freq_hz, ship_speed_kn),
    ])
}

// ---------------------------------------------------------------------------
// 샘플 합성 (M2): 준위 모델(위) → 실제 잡음 신호
// ---------------------------------------------------------------------------

/// 해양 배경잡음 샘플 합성기 (M2).
///
/// M1의 Knudsen C₀ 준위 모델을 실제 신호로 구현:
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
        let nl_ref = ocean_noise_level_db(100.0, wind_speed_ms, rain_mm_hr, 0.0);
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
        let calm = knudsen_noise_level_db(100.0, 0.0);
        let storm = knudsen_noise_level_db(100.0, 15.0);
        assert!(storm > calm);
    }

    #[test]
    fn knudsen_matches_reference_at_100hz_calm() {
        // C₀ 무풍: f=100Hz에서 Nw + 풍항(0) + (40-Nw)·log10(1.03f/100... ) 등.
        // f=100, W=0: 26.2·log10(1)=0, 1.3·1=1.3, (40-50)·log10(1+3)=−10·log10(4),
        // 0.014·sqrt(1+40)=0.014·6.403≈0.0896, 풍항 0.
        let nl = knudsen_noise_level_db(100.0, 0.0);
        let expected = 50.0 + 1.3 + (-10.0 * 4.0f32.log10()) + 0.014 * 41.0f32.sqrt();
        assert!((nl - expected).abs() < 1e-4);
    }

    #[test]
    fn knudsen_rises_with_frequency() {
        let low = knudsen_noise_level_db(50.0, 5.0);
        let high = knudsen_noise_level_db(500.0, 5.0);
        assert!(high > low);
    }

    #[test]
    fn rain_and_ship_absent_when_zero() {
        // 0 강우/0 함선 → 종합 NL == 순수 Knudsen
        let f = 200.0;
        let wind = 8.0;
        let pure = knudsen_noise_level_db(f, wind);
        let combined = ocean_noise_level_db(f, wind, 0.0, 0.0);
        assert!((pure - combined).abs() < 1e-4);
    }

    #[test]
    fn ocean_noise_increases_with_rain() {
        let dry = ocean_noise_level_db(1000.0, 5.0, 0.0, 0.0);
        let wet = ocean_noise_level_db(1000.0, 5.0, 20.0, 0.0);
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
