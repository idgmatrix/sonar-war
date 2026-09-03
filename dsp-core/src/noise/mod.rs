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
// 샘플 합성 (M2B): 준위 모델(위) → 실제 잡음 신호
// ---------------------------------------------------------------------------

const THIRD_OCTAVE_CENTERS_HZ: [f32; 31] = [
    20.0, 25.2, 31.7, 40.0, 50.4, 63.5, 80.0, 100.8, 127.0, 160.0, 201.6, 254.0,
    320.0, 403.2, 508.0, 640.0, 806.3, 1015.9, 1280.0, 1612.7, 2031.9, 2560.0,
    3225.4, 4063.7, 5120.0, 6450.8, 8127.5, 10240.0, 12901.6, 16255.0, 20000.0,
];

/// RBJ constant-skirt-gain band-pass. 중심 주파수의 이득은 1이다.
#[derive(Debug, Clone)]
struct NoiseBand {
    b0: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
    z3: f32,
    z4: f32,
    amplitude: f32,
    rng: u64,
}

impl NoiseBand {
    fn new(sample_rate: f32, center_hz: f32, target_nl_db: f32, seed: u64) -> Self {
        // 두 개를 직렬 연결해 4차 1/3옥타브 대역을 만든다. 단일 2차 필터보다
        // 저주파 난류 에너지의 대역 외 누설이 작다.
        const THIRD_OCTAVE_Q: f32 = 4.318_473;
        let omega = 2.0 * std::f32::consts::PI * center_hz / sample_rate;
        let alpha = omega.sin() / (2.0 * THIRD_OCTAVE_Q);
        let a0 = 1.0 + alpha;

        // white ∈ [-1,1]의 분산은 1/3이고 one-sided PSD는 2σ²/fs.
        // target_nl_db를 120 dB FS 기준의 선형 PSD로 바꿔 중심 이득을 교정한다.
        let target_psd_fs = 10f32.powf((target_nl_db - 120.0) / 10.0);
        let amplitude = (target_psd_fs * sample_rate * 1.5).sqrt();
        Self {
            b0: alpha / a0,
            b2: -alpha / a0,
            a1: -2.0 * omega.cos() / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
            z3: 0.0,
            z4: 0.0,
            amplitude,
            rng: seed,
        }
    }

    #[inline]
    fn next(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        let white = ((x >> 40) as f32 / 16_777_216.0) * 2.0 - 1.0;

        let input = white * self.amplitude;
        let first = self.b0 * input + self.z1;
        self.z1 = -self.a1 * first + self.z2;
        self.z2 = self.b2 * input - self.a2 * first;
        let second = self.b0 * first + self.z3;
        self.z3 = -self.a1 * second + self.z4;
        self.z4 = self.b2 * first - self.a2 * second;
        second
    }
}

/// 해양 배경잡음 샘플 합성기.
///
/// 20 Hz–20 kHz의 1/3옥타브 중심마다 독립적인 결정적 백색잡음을 band-pass하고,
/// Coates/Wenz + 강우 목표 PSD로 각 대역의 이득을 교정한다. 준위 규약은
/// 200 µPa = 1.0 full scale (120 dB re 1µPa)이다.
#[derive(Debug, Clone)]
pub struct OceanNoise {
    bands: Vec<NoiseBand>,
}

impl OceanNoise {
    /// 풍속/강우로 잡음 합성기 생성.
    pub fn new(sample_rate: f32, wind_speed_ms: f32, rain_mm_hr: f32) -> Self {
        let nyquist_guard = sample_rate * 0.45;
        let bands = THIRD_OCTAVE_CENTERS_HZ
            .iter()
            .copied()
            .filter(|center| *center < nyquist_guard)
            .enumerate()
            .map(|(index, center)| {
                let nl = ocean_noise_level_db(center, wind_speed_ms, rain_mm_hr, 0.5);
                let seed = 0x9E37_79B9_7F4A_7C15u64
                    ^ (index as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03);
                NoiseBand::new(sample_rate, center, nl, seed)
            })
            .collect();
        Self { bands }
    }

    /// 활성 필터 대역 수. 오디오 샘플레이트별 Nyquist 제외 여부 검증용.
    pub fn band_count(&self) -> usize {
        self.bands.len()
    }

    /// 다음 샘플 (준위 스케일 적용됨).
    #[inline]
    pub fn next_sample(&mut self) -> f32 {
        let mut sample = 0.0;
        for band in &mut self.bands {
            sample += band.next();
        }
        sample
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

    fn measured_psd_db(samples: &[f32], sample_rate: f32, frequency_hz: f32) -> f32 {
        const SEGMENT: usize = 8192;
        let mut psd_sum = 0.0f64;
        let mut segments = 0usize;
        for chunk in samples.chunks_exact(SEGMENT) {
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            let mut window_energy = 0.0f64;
            for (index, &sample) in chunk.iter().enumerate() {
                let window = 0.5
                    - 0.5
                        * (2.0 * std::f64::consts::PI * index as f64
                            / (SEGMENT - 1) as f64)
                            .cos();
                let phase = 2.0 * std::f64::consts::PI * frequency_hz as f64 * index as f64
                    / sample_rate as f64;
                let value = sample as f64 * window;
                re += value * phase.cos();
                im -= value * phase.sin();
                window_energy += window * window;
            }
            psd_sum += 2.0 * (re * re + im * im) / (sample_rate as f64 * window_energy);
            segments += 1;
        }
        let psd_fs = psd_sum / segments as f64;
        10.0 * psd_fs.max(1e-20).log10() as f32 + 120.0
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
    fn ocean_noise_uses_only_bands_below_nyquist_guard() {
        assert_eq!(OceanNoise::new(44100.0, 5.0, 0.0).band_count(), 30);
        assert_eq!(OceanNoise::new(22050.0, 5.0, 0.0).band_count(), 27);
    }

    #[test]
    fn ocean_noise_psd_tracks_coates_wenz_curve() {
        let sample_rate = 44100.0;
        let wind = 5.0;
        let mut noise = OceanNoise::new(sample_rate, wind, 0.0);
        for _ in 0..32768 {
            noise.next_sample();
        }
        let samples: Vec<f32> = (0..262144).map(|_| noise.next_sample()).collect();
        for frequency in [50.4, 100.8, 201.6, 508.0, 1015.9, 2031.9, 5120.0, 10240.0] {
            let measured = measured_psd_db(&samples, sample_rate, frequency);
            let expected = coates_wenz_noise_level_db(frequency, 0.5, wind);
            assert!(
                (measured - expected).abs() <= 2.0,
                "{frequency} Hz: measured={measured:.2} expected={expected:.2} error={:.2} dB",
                measured - expected
            );
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
