//! SSN-X DSP 엔진 (WASM) — 장면 합성 (M2).
//!
//! 파이프라인 (샘플당):
//! ```text
//! 표적 신호 (토널 + DEMON 캐비테이션 + 도플러)
//!   → 하이드로폰별 지연 (τᵢ = pᵢ·û/c, 명세서 §3.2)
//!   → 지연-합 빔포밍 (메인 빔)
//!   → + 해양 배경잡음 (Knudsen C₀)
//!   → 소프트 클립 (tanh — 수신기 동역학 압축)
//! ```
//!
//! 준위 규약: 200 µPa = 1.0 full scale (120 dB re 1µPa).
//! 좌표계: x=전방, y=하향(수심), z=우현.
//! 준위 모델: `docs/물리 상수 시트.md`.

pub mod beamform;
pub mod noise;
pub mod physics;
pub mod source;

use wasm_bindgen::prelude::*;

use beamform::{beam_delay, DelayAndSum, DEFAULT_SOUND_SPEED};
use noise::OceanNoise;
use physics::transmission_loss_db;
use source::{
    blade_rate_hz, broadband_level_db, demon_envelope, doppler_factor, tonal_harmonic_level_db,
    TONAL_HARMONICS,
};

/// 캐비테이션 광대역 TL 기준 주파수 (kHz).
const BROADBAND_REF_KHZ: f32 = 1.0;
/// Full scale 기준 (dB re 1µPa) — 200 µPa = 1.0.
const FULL_SCALE_DB: f32 = 120.0;
/// 가상 구형 어레이 하이드로폰 수.
const HYDROPHONE_COUNT: usize = 16;
/// 구형 어레이 반지름 (m) — 보우 어레이 DIA 8.4m.
const ARRAY_RADIUS_M: f32 = 4.2;
/// BASS 전 방위 스캔 해상도 (5° 간격).
const BASS_BINS: usize = 72;
/// 매 샘플이 아니라 16샘플마다 스캔해 AudioWorklet 예산을 지킨다.
const BASS_DECIMATION: u64 = 16;

/// 표적 1개당 float 수: [bearing_deg, range_m, depth_m, rpm, blades, tonal_db, cavitation, rel_vel_ms]
const TARGET_STRIDE: usize = 8;

/// Fibonacci 구: 반지름 `radius` 구에 균등한 n점.
fn fibonacci_sphere(n: usize, radius: f32) -> Vec<[f32; 3]> {
    let golden = std::f32::consts::PI * (3.0 - 5.0f32.sqrt());
    (0..n)
        .map(|i| {
            let y = 1.0 - 2.0 * (i as f32 + 0.5) / n as f32;
            let r = (1.0 - y * y).sqrt();
            let theta = golden * i as f32;
            [radius * r * theta.cos(), radius * y, radius * r * theta.sin()]
        })
        .collect()
}

/// 표적 1개 (수중 소음원).
struct Target {
    /// 하이드로폰별 읽기 지연 (s) = p_max − p_h·û/c (≥ 0).
    delays: Vec<f32>,
    /// 토널 진폭 (고조파별, 선형).
    tonal_amp: [f32; TONAL_HARMONICS as usize],
    /// 도플러 시프트된 고조파 주파수 (Hz).
    f_dop: [f32; TONAL_HARMONICS as usize],
    /// 캐비테이션 광대역 진폭 (선형).
    cav_amp: f32,
    /// 도플러 시프트된 블레이드 레이트 (Hz).
    blade_dop: f32,
    /// 캐비테이션 백색잡음 링 버퍼 (단위 잡음 — 읽기 시 스케일).
    noise_buf: Vec<f32>,
    noise_pos: usize,
    rng: u64,
}

impl Target {
    fn new(
        bearing_deg: f32,
        range_m: f32,
        depth_m: f32,
        rpm: f32,
        blade_count: u32,
        tonal_level_db: f32,
        cavitation: f32,
        rel_vel_ms: f32,
        hydrophones: &[[f32; 3]],
        max_delay_samples: usize,
    ) -> Self {
        // 표적 방향: 수평 거리 + 수심
        let az = bearing_deg.to_radians();
        let horizontal = (range_m * range_m - depth_m * depth_m).max(0.0).sqrt();
        let r = range_m.max(1.0);
        let unit = [
            horizontal * az.cos() / r,
            depth_m / r,
            horizontal * az.sin() / r,
        ];

        // 실시간 제약: p_max 상수 시프트 (빔포머와 동일한 규칙)
        let pmax = hydrophones
            .iter()
            .map(|p| beam_delay(*p, unit, DEFAULT_SOUND_SPEED))
            .fold(0f32, f32::max);
        let delays = hydrophones
            .iter()
            .map(|p| (pmax - beam_delay(*p, unit, DEFAULT_SOUND_SPEED)).max(0.0))
            .collect::<Vec<_>>();

        let dop = doppler_factor(rel_vel_ms, DEFAULT_SOUND_SPEED);
        let blade = blade_rate_hz(rpm, blade_count).max(0.1);

        let mut tonal_amp = [0.0f32; TONAL_HARMONICS as usize];
        let mut f_dop = [0.0f32; TONAL_HARMONICS as usize];
        for (i, n) in (1..=TONAL_HARMONICS).enumerate() {
            let f = blade * n as f32 * dop;
            let level = tonal_harmonic_level_db(n, tonal_level_db);
            let tl = transmission_loss_db(range_m, f / 1000.0);
            tonal_amp[i] = 10f32.powf((level - tl - FULL_SCALE_DB) / 20.0);
            f_dop[i] = f;
        }

        let bb_level = broadband_level_db(rpm, cavitation);
        let bb_tl = transmission_loss_db(range_m, BROADBAND_REF_KHZ);
        let cav_amp = 10f32.powf((bb_level - bb_tl - FULL_SCALE_DB) / 20.0);

        Self {
            delays,
            tonal_amp,
            f_dop,
            cav_amp,
            blade_dop: blade * dop,
            // 지연 범위 0..2R/c (구형 어레이) → 2× 최대단일지연 용량
            noise_buf: vec![0.0f32; max_delay_samples * 2 + 2],
            noise_pos: 0,
            rng: 0x9E37_79B9_7F4A_7C15 ^ (blade_count as u64).wrapping_mul(0x2545_F491_4F6C_DD1D),
        }
    }

    /// `delay_samples`만큼 과거의 캐비테이션 잡음 (선형 보간 분수 지연).
    fn read_noise(&self, delay_samples: f32) -> f32 {
        let cap = self.noise_buf.len();
        let d = delay_samples.max(0.0).min(cap as f32 - 2.0);
        let i = d.floor() as usize;
        let frac = d - i as f32;
        let newest = if self.noise_pos == 0 { cap - 1 } else { self.noise_pos - 1 };
        let idx0 = newest.wrapping_sub(i) % cap;
        let idx1 = newest.wrapping_sub(i + 1) % cap;
        self.noise_buf[idx0] * (1.0 - frac) + self.noise_buf[idx1] * frac
    }

    /// 현재 시각의 백색잡음 1샘플 push.
    fn push_noise(&mut self) {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        let white = ((x >> 11) as f32 / 9007199254740992.0) * 2.0 - 1.0;
        self.noise_buf[self.noise_pos] = white;
        self.noise_pos = (self.noise_pos + 1) % self.noise_buf.len();
    }
}

/// SSN-X DSP 엔진 (WASM).
///
/// 상태 포함 — `process`는 블럭당 **1회만** 호출할 것 (채널 복사는 호스트가 담당).
#[wasm_bindgen]
pub struct DspEngine {
    sample_rate: f32,
    t: f64,
    hydrophones: Vec<[f32; 3]>,
    array: DelayAndSum,
    beam: [f32; 3],
    targets: Vec<Target>,
    ocean: OceanNoise,
    max_delay_samples: usize,
    mixed: Vec<f32>,
    bass_power: Vec<f32>,
    bass_samples: u32,
    sample_index: u64,
}

#[wasm_bindgen]
impl DspEngine {
    /// 샘플레이트로 생성 (데모 씬 3표적 포함).
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: f32) -> Self {
        let hydrophones = fibonacci_sphere(HYDROPHONE_COUNT, ARRAY_RADIUS_M);
        let max_delay_samples = (ARRAY_RADIUS_M / DEFAULT_SOUND_SPEED * sample_rate).ceil() as usize;
        let array = DelayAndSum::new(hydrophones.clone(), DEFAULT_SOUND_SPEED, sample_rate);
        let mut engine = Self {
            sample_rate,
            t: 0.0,
            hydrophones,
            array,
            beam: [1.0, 0.0, 0.0],
            targets: Vec::new(),
            ocean: OceanNoise::new(sample_rate, 5.0, 0.0),
            max_delay_samples,
            mixed: vec![0.0f32; HYDROPHONE_COUNT],
            bass_power: vec![0.0f32; BASS_BINS],
            bass_samples: 0,
            sample_index: 0,
        };
        engine.set_targets(&demo_scene());
        engine
    }

    /// 표적 씬 설정 (표적당 8 float: bearing_deg, range_m, depth_m, rpm, blades, tonal_db, cavitation, rel_vel_ms).
    pub fn set_targets(&mut self, data: &[f32]) {
        self.targets.clear();
        for chunk in data.chunks(TARGET_STRIDE) {
            if chunk.len() < TARGET_STRIDE {
                break;
            }
            self.targets.push(Target::new(
                chunk[0],
                chunk[1],
                chunk[2],
                chunk[3],
                chunk[4] as u32,
                chunk[5],
                chunk[6],
                chunk[7],
                &self.hydrophones,
                self.max_delay_samples,
            ));
        }
    }

    /// 해양 배경잡음 설정 (풍속 m/s, 강우 mm/hr).
    pub fn set_ocean(&mut self, wind_speed_ms: f32, rain_mm_hr: f32) {
        self.ocean = OceanNoise::new(self.sample_rate, wind_speed_ms, rain_mm_hr);
    }

    /// 메인 빔 조향 (azimuth deg: 0=전방, elevation deg: +=하향).
    pub fn set_beam(&mut self, azimuth_deg: f32, elevation_deg: f32) {
        let az = azimuth_deg.to_radians();
        let el = elevation_deg.to_radians();
        self.beam = [el.cos() * az.cos(), el.sin(), el.cos() * az.sin()];
    }

    /// 표적 수.
    pub fn target_count(&self) -> u32 {
        self.targets.len() as u32
    }

    /// 최근 처리 구간의 전 방위 BASS 레벨(dBFS)을 반환한다.
    ///
    /// `out`의 길이가 방위 빈 수가 되며 0°(전방)부터 시계방향으로 균등 배치된다.
    /// 읽은 뒤 누산기를 비워 다음 UI 프레임과 시간 구간이 겹치지 않게 한다.
    pub fn bass_scan(&mut self, out: &mut [f32]) {
        let count = self.bass_samples.max(1) as f32;
        for (i, value) in out.iter_mut().enumerate() {
            let power = self.bass_power.get(i).copied().unwrap_or(0.0) / count;
            *value = (10.0 * power.max(1e-12).log10()).clamp(-120.0, 0.0);
        }
        self.bass_power.fill(0.0);
        self.bass_samples = 0;
    }

    /// 샘플 블럭 1개 합성 (모노 — 채널 복사는 호스트).
    pub fn process(&mut self, out: &mut [f32]) {
        // 빔포머 조향(명세서 §3.2, τᵢ = pᵢ·û/c)의 û는 **파동 진행 방향** —
        // 소스 방향 û_src의 파동은 −û_src로 진행하므로 조향 벡터를 반전시킨다.
        let travel = [-self.beam[0], -self.beam[1], -self.beam[2]];
        let dt = 1.0 / self.sample_rate as f64;
        for o in out.iter_mut() {
            let t = self.t;
            for m in self.mixed.iter_mut() {
                *m = 0.0;
            }

            for tgt in &mut self.targets {
                for (h, &delay_s) in tgt.delays.iter().enumerate() {
                    let tt = t - delay_s as f64;
                    let mut s = 0.0f32;
                    for i in 0..TONAL_HARMONICS as usize {
                        s += tgt.tonal_amp[i]
                            * (2.0 * std::f64::consts::PI * tgt.f_dop[i] as f64 * tt).cos() as f32;
                    }
                    let noise = tgt.read_noise(delay_s * self.sample_rate);
                    let env = demon_envelope((tgt.blade_dop as f64 * tt) as f32);
                    s += tgt.cav_amp * noise * env;
                    self.mixed[h] += s;
                }
                tgt.push_noise();
            }

            let beam_out = self.array.process_sample(&self.mixed, travel);
            if self.sample_index % BASS_DECIMATION == 0 {
                for bin in 0..BASS_BINS {
                    let az = 2.0 * std::f32::consts::PI * bin as f32 / BASS_BINS as f32;
                    // 파원 방위의 반대가 파동 진행 방향이다.
                    let scan_travel = [-az.cos(), 0.0, -az.sin()];
                    let sample = self.array.beam_sample(scan_travel);
                    self.bass_power[bin] += sample * sample;
                }
                self.bass_samples += 1;
            }
            let x = beam_out + self.ocean.next_sample();
            *o = x.tanh();
            self.t += dt;
            self.sample_index += 1;
        }
    }
}

/// 데모 씬: 접근하는 함선 / 원거리 조용한 함선 / 근거리 시끄러운 함선.
fn demo_scene() -> Vec<f32> {
    vec![
        // bearing, range, depth, rpm, blades, tonal_db, cavitation, rel_vel
        45.0, 3000.0, 50.0, 90.0, 5.0, 150.0, 0.3, 5.0,
        300.0, 8000.0, 200.0, 70.0, 4.0, 145.0, 0.1, -3.0,
        0.0, 1500.0, 30.0, 110.0, 6.0, 155.0, 0.6, 0.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_with_targets(data: &[f32]) -> DspEngine {
        let mut e = DspEngine::new(44100.0);
        e.set_targets(data);
        e
    }

    fn process_block(e: &mut DspEngine, n: usize) -> Vec<f32> {
        let mut buf = vec![0.0f32; n];
        e.process(&mut buf);
        buf
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    fn zero_crossings(buf: &[f32]) -> usize {
        buf.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count()
    }

    #[test]
    fn process_produces_signal_with_target() {
        // 근접+대음량 표적 — 정확한 TL에서 잡음 바닥(≈7e-4)보다 확실히 위여야
        // "표적이 신호를 만든다"고 검증할 수 있다 (원거리 저 SL 표적은 잡음에 매몰).
        let mut e = engine_with_targets(&[45.0, 1000.0, 50.0, 90.0, 5.0, 150.0, 0.3, 5.0]);
        let _ = process_block(&mut e, 4096); // 빔포머 링 버퍼 채우는 과도 구간 스킵
        let buf = process_block(&mut e, 4096);
        assert!(rms(&buf) > 1e-2, "rms={}", rms(&buf));
    }

    #[test]
    fn farther_target_is_quieter() {
        // 150dB로 — 정확한 TL에서 8km 표적이 잡음 바닥(≈7e-4) 아래로 가면
        // 비도가 잡음에 의해 결정되어 근/원 거리비가 무너진다.
        let near = [0.0, 1000.0, 30.0, 90.0, 5.0, 150.0, 0.0, 0.0];
        let far = [0.0, 8000.0, 30.0, 90.0, 5.0, 150.0, 0.0, 0.0];
        let mut en = engine_with_targets(&near);
        let mut ef = engine_with_targets(&far);
        // 빔포머 링 버퍼 채워지는 과도 구간 스킵
        let _ = process_block(&mut en, 4096);
        let _ = process_block(&mut ef, 4096);
        let rn = rms(&process_block(&mut en, 8192));
        let rf = rms(&process_block(&mut ef, 8192));
        assert!(rn > rf * 3.0, "near={rn} far={rf}");
    }

    #[test]
    fn higher_rpm_raises_tonal_frequency() {
        // rpm 120/5블레이드(10Hz) vs 40/5(3.33Hz) — 제로크로스 수 비교.
        // 정확한 TL(1km≈60dB)에서 토널이 잡음 바닥(~7e-4)을 확실히 압도해야
        // 제로크로스가 토널 주파수를 따라감 → SL을 충분히 높임.
        // 고조파(5개, −6dB/개)는 두 신호 모두에 기본파보다 큰 크로스 수를 더해
        // 기본파 3:1 갭이 제로크로스 비율로는 ~1.5로 압축됨 (측정 ≈1.48).
        // 임계는 1.3 — 잡음/고조파에 무관하게 "더 높은 주파수"를 단단히 가리키되
        // 고조파 압축을 감안해 여유를 둔 값.
        let mut e_hi = engine_with_targets(&[0.0, 1000.0, 30.0, 120.0, 5.0, 155.0, 0.0, 0.0]);
        let mut e_lo = engine_with_targets(&[0.0, 1000.0, 30.0, 40.0, 5.0, 155.0, 0.0, 0.0]);
        let _ = process_block(&mut e_hi, 4096);
        let _ = process_block(&mut e_lo, 4096);
        let hi = zero_crossings(&process_block(&mut e_hi, 131072));
        let lo = zero_crossings(&process_block(&mut e_lo, 131072));
        assert!(hi * 10 > lo * 13, "hi={hi} lo={lo}");
    }

    #[test]
    fn approaching_target_has_higher_zero_crossings() {
        // 도플러: 접근(+50m/s) vs 이탈(−50m/s), 18Hz 블레이드 레이트
        let mut e_app = engine_with_targets(&[0.0, 1000.0, 30.0, 180.0, 6.0, 130.0, 0.0, 50.0]);
        let mut e_rec = engine_with_targets(&[0.0, 1000.0, 30.0, 180.0, 6.0, 130.0, 0.0, -50.0]);
        let _ = process_block(&mut e_app, 4096);
        let _ = process_block(&mut e_rec, 4096);
        let app = zero_crossings(&process_block(&mut e_app, 131072));
        let rec = zero_crossings(&process_block(&mut e_rec, 131072));
        assert!(app > rec, "app={app} rec={rec}");
    }

    #[test]
    fn ocean_noise_present_without_targets() {
        let mut e = DspEngine::new(44100.0);
        e.set_targets(&[]);
        let buf = process_block(&mut e, 16384);
        assert!(rms(&buf) > 1e-5);
    }

    #[test]
    fn engine_is_deterministic() {
        // 동일 씬 → 동일 출력 (멀티플레이 동기화 전제)
        let scene = [45.0, 3000.0, 50.0, 90.0, 5.0, 120.0, 0.3, 5.0];
        let mut a = DspEngine::new(44100.0);
        let mut b = DspEngine::new(44100.0);
        a.set_targets(&scene);
        b.set_targets(&scene);
        assert_eq!(process_block(&mut a, 4096), process_block(&mut b, 4096));
    }

    #[test]
    fn bass_scan_peaks_near_target_bearing() {
        let mut engine =
            engine_with_targets(&[45.0, 500.0, 0.0, 120.0, 6.0, 170.0, 1.0, 0.0]);
        let _ = process_block(&mut engine, 32768);
        let mut scan = vec![0.0; BASS_BINS];
        engine.bass_scan(&mut scan);
        let peak = scan
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap();
        let expected = (45.0 / (360.0 / BASS_BINS as f32)) as usize;
        let circular_error = peak
            .abs_diff(expected)
            .min(BASS_BINS - peak.abs_diff(expected));
        assert!(
            circular_error <= 1,
            "peak={}° expected=45°",
            peak * 5
        );
    }
}
