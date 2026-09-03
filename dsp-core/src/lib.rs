//! SSN-X DSP 엔진 (WASM) — 장면 합성 (M2).
//!
//! 파이프라인 (샘플당):
//! ```text
//! 표적 신호 (토널 + DEMON 캐비테이션 + 도플러)
//!   → 하이드로폰별 지연 (τᵢ = pᵢ·û/c, 명세서 §3.2)
//!   → 지연-합 빔포밍 (메인 빔)
//!   → + 해양 배경잡음 (Coates/Wenz 네 성분 기준선)
//!   → BASS 분석(읽기 전용 분기) / 소프트 클립 출력(tanh)
//! ```
//!
//! 준위 규약: 1 Pa = 1,000,000 µPa = 1.0 full scale (120 dB re 1µPa).
//! 좌표계: x=전방, y=하향(수심), z=우현.
//! 준위 모델: `docs/물리 상수 시트.md`.

pub mod analysis;
pub mod beamform;
pub mod noise;
#[cfg(not(target_arch = "wasm32"))]
pub mod offline;
pub mod output;
pub mod physics;
pub mod propagation;
pub mod receiver;
pub mod source;

use wasm_bindgen::prelude::*;

use analysis::BassAnalyzer;
use beamform::DEFAULT_SOUND_SPEED;
use propagation::{propagate, HydrophoneFrame, PropagationGeometry, PropagationProcessor};
use receiver::{ReceiverArray, DEFAULT_FULL_SCALE_DB_RE_1UPA};
use source::{source_spectrum, SourceVoice};

/// 가상 구형 어레이 하이드로폰 수.
const HYDROPHONE_COUNT: usize = 16;
/// 구형 어레이 반지름 (m) — 보우 어레이 DIA 8.4m.
const ARRAY_RADIUS_M: f32 = 4.2;
/// 표적 1개당 float 수: [bearing_deg, range_m, depth_m, rpm, blades, tonal_db, cavitation, rel_vel_ms]
const TARGET_STRIDE: usize = 8;

/// JS/WASM flat array를 계층 입력으로 넘기기 전에 단위가 있는 필드로 해석한 값.
#[derive(Debug, Clone, Copy)]
struct TargetDescriptor {
    bearing_deg: f32,
    range_m: f32,
    depth_m: f32,
    rpm: f32,
    blade_count: u32,
    tonal_level_db_re_1upa_at_1m: f32,
    cavitation: f32,
    relative_velocity_ms: f32,
}

impl TargetDescriptor {
    fn from_flat(values: &[f32]) -> Option<Self> {
        (values.len() >= TARGET_STRIDE).then(|| Self {
            bearing_deg: values[0],
            range_m: values[1],
            depth_m: values[2],
            rpm: values[3],
            blade_count: values[4] as u32,
            tonal_level_db_re_1upa_at_1m: values[5],
            cavitation: values[6],
            relative_velocity_ms: values[7],
        })
    }
}

/// Fibonacci 구: 반지름 `radius` 구에 균등한 n점.
fn fibonacci_sphere(n: usize, radius: f32) -> Vec<[f32; 3]> {
    let golden = std::f32::consts::PI * (3.0 - 5.0f32.sqrt());
    (0..n)
        .map(|i| {
            let y = 1.0 - 2.0 * (i as f32 + 0.5) / n as f32;
            let r = (1.0 - y * y).sqrt();
            let theta = golden * i as f32;
            [
                radius * r * theta.cos(),
                radius * y,
                radius * r * theta.sin(),
            ]
        })
        .collect()
}

/// 표적 1개 (수중 소음원).
struct Target {
    source: SourceVoice,
    propagation: PropagationProcessor,
}

/// 네이티브 오프라인 검증에서 한 샘플의 계층 경계를 기록한 값.
///
/// `source_*_1m_upa`는 수신기 시각에 각 Source가 방사한 원시 음압의 합,
/// `hydrophone_0_upa`는 전파 후 첫 하이드로폰 음압이다. 두 FS 값은 출력 제한 전/후다.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DspTraceSample {
    pub source_1m_upa: f32,
    pub source_tonal_1m_upa: f32,
    pub source_broadband_1m_upa: f32,
    pub hydrophone_0_upa: f32,
    pub receiver_fs: f32,
    pub output_fs: f32,
}

impl Target {
    fn new(
        descriptor: TargetDescriptor,
        hydrophones: &[[f32; 3]],
        max_delay_samples: usize,
    ) -> Self {
        let source = source_spectrum(
            descriptor.rpm,
            descriptor.blade_count,
            descriptor.tonal_level_db_re_1upa_at_1m,
            descriptor.cavitation,
        );
        let propagated = propagate(
            &source,
            PropagationGeometry {
                bearing_deg: descriptor.bearing_deg,
                range_m: descriptor.range_m,
                source_depth_m: descriptor.depth_m,
                // 기존 8-float WASM 계약에는 자함 수심이 없어 원점을 유지한다.
                receiver_depth_m: 0.0,
                relative_velocity_ms: descriptor.relative_velocity_ms,
            },
            hydrophones,
            DEFAULT_SOUND_SPEED,
        );
        let propagation = PropagationProcessor::new(&source, &propagated);
        let seed = 0x9E37_79B9_7F4A_7C15
            ^ (descriptor.blade_count as u64).wrapping_mul(0x2545_F491_4F6C_DD1D);

        Self {
            // 지연 범위 0..2R/c (구형 어레이) → 2× 최대단일지연 용량
            source: SourceVoice::new(source, max_delay_samples * 2 + 2, seed),
            propagation,
        }
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
    receiver: ReceiverArray,
    beam: [f32; 3],
    targets: Vec<Target>,
    hydrophone_frame: HydrophoneFrame,
    max_delay_samples: usize,
    bass: BassAnalyzer,
}

#[wasm_bindgen]
impl DspEngine {
    /// 샘플레이트로 생성 (데모 씬 3표적 포함).
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: f32) -> Self {
        let hydrophones = fibonacci_sphere(HYDROPHONE_COUNT, ARRAY_RADIUS_M);
        let max_delay_samples =
            (ARRAY_RADIUS_M / DEFAULT_SOUND_SPEED * sample_rate).ceil() as usize;
        let receiver = ReceiverArray::new(
            hydrophones.clone(),
            DEFAULT_SOUND_SPEED,
            sample_rate,
            DEFAULT_FULL_SCALE_DB_RE_1UPA,
        );
        let mut engine = Self {
            sample_rate,
            t: 0.0,
            hydrophones,
            receiver,
            beam: [1.0, 0.0, 0.0],
            targets: Vec::new(),
            hydrophone_frame: HydrophoneFrame::new(HYDROPHONE_COUNT),
            max_delay_samples,
            bass: BassAnalyzer::new(),
        };
        engine.set_targets(&demo_scene());
        engine
    }

    /// 표적 씬 설정 (표적당 8 float: bearing_deg, range_m, depth_m, rpm, blades, tonal_db, cavitation, rel_vel_ms).
    pub fn set_targets(&mut self, data: &[f32]) {
        self.targets.clear();
        for chunk in data.chunks(TARGET_STRIDE) {
            if let Some(descriptor) = TargetDescriptor::from_flat(chunk) {
                self.targets.push(Target::new(
                    descriptor,
                    &self.hydrophones,
                    self.max_delay_samples,
                ));
            }
        }
    }

    /// 해양 배경잡음 설정 (풍속 m/s, 강우 mm/hr).
    pub fn set_ocean(&mut self, wind_speed_ms: f32, rain_mm_hr: f32) {
        self.receiver
            .set_ocean(self.sample_rate, wind_speed_ms, rain_mm_hr);
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
        self.bass.read_levels(out);
    }

    /// 샘플 블럭 1개 합성 (모노 — 채널 복사는 호스트).
    pub fn process(&mut self, out: &mut [f32]) {
        for sample in out {
            *sample = self.process_sample::<false>(None);
        }
    }
}

impl DspEngine {
    #[inline]
    fn process_sample<const TRACE: bool>(&mut self, mut trace: Option<&mut DspTraceSample>) -> f32 {
        // 빔포머 조향(명세서 §3.2, τᵢ = pᵢ·û/c)의 û는 **파동 진행 방향** —
        // 소스 방향 û_src의 파동은 −û_src로 진행하므로 조향 벡터를 반전시킨다.
        let travel = [-self.beam[0], -self.beam[1], -self.beam[2]];
        let dt = 1.0 / self.sample_rate as f64;
        let t = self.t;
        self.hydrophone_frame.clear();

        let mut source_tonal_1m_upa = 0.0;
        let mut source_broadband_1m_upa = 0.0;
        for target in &mut self.targets {
            if TRACE {
                let source = target.source.sample_at(t, 0.0);
                source_tonal_1m_upa += source.tonal_pressure_1m_upa.iter().sum::<f32>();
                source_broadband_1m_upa += source.broadband_pressure_1m_upa;
            }
            target.propagation.render_into(
                &target.source,
                t,
                self.sample_rate,
                &mut self.hydrophone_frame,
            );
            target.source.advance();
        }

        if TRACE {
            let trace = trace
                .as_deref_mut()
                .expect("TRACE=true requires a trace sample");
            trace.source_tonal_1m_upa = source_tonal_1m_upa;
            trace.source_broadband_1m_upa = source_broadband_1m_upa;
            trace.source_1m_upa = source_tonal_1m_upa + source_broadband_1m_upa;
            trace.hydrophone_0_upa = self
                .hydrophone_frame
                .pressure_upa
                .first()
                .copied()
                .unwrap_or(0.0);
        }

        let receiver_fs = self.receiver.process_frame(&self.hydrophone_frame, travel);
        let receiver = &self.receiver;
        self.bass
            .observe(|direction| receiver.beam_sample(direction));
        let output_fs = output::soft_limit(receiver_fs);

        if TRACE {
            let trace = trace.expect("TRACE=true requires a trace sample");
            trace.receiver_fs = receiver_fs;
            trace.output_fs = output_fs;
        }
        self.t += dt;
        output_fs
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl DspEngine {
    /// 실시간 경로와 같은 상태 전이를 수행하며 계층별 샘플을 함께 기록한다.
    pub fn process_traced(&mut self, out: &mut [f32], trace: &mut [DspTraceSample]) {
        assert_eq!(
            out.len(),
            trace.len(),
            "output and trace lengths must match"
        );
        for (sample, trace_sample) in out.iter_mut().zip(trace) {
            *sample = self.process_sample::<true>(Some(trace_sample));
        }
    }
}

/// 데모 씬: 접근하는 함선 / 원거리 조용한 함선 / 근거리 시끄러운 함선.
fn demo_scene() -> Vec<f32> {
    vec![
        // bearing, range, depth, rpm, blades, tonal_db, cavitation, rel_vel
        45.0, 3000.0, 50.0, 90.0, 5.0, 150.0, 0.3, 5.0, 300.0, 8000.0, 200.0, 70.0, 4.0, 145.0, 0.1,
        -3.0, 0.0, 1500.0, 30.0, 110.0, 6.0, 155.0, 0.6, 0.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::BASS_BINS;

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

    /// 동일 seed의 무표적 엔진 출력을 빼서 표적 성분만 관찰한다.
    /// 환경음 모델 변경이 Source/Propagation 회귀 테스트를 가리지 않게 한다.
    fn process_target_component(targets: &[f32], warmup: usize, n: usize) -> Vec<f32> {
        let mut scene = engine_with_targets(targets);
        let mut ambient = engine_with_targets(&[]);
        let _ = process_block(&mut scene, warmup);
        let _ = process_block(&mut ambient, warmup);
        process_block(&mut scene, n)
            .into_iter()
            .zip(process_block(&mut ambient, n))
            .map(|(mixed, noise)| mixed - noise)
            .collect()
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    fn zero_crossings(buf: &[f32]) -> usize {
        buf.windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count()
    }

    #[test]
    fn process_produces_signal_with_target() {
        // 근접+대음량 표적이 혼합 출력에 유의미한 신호를 만드는지 확인한다.
        let mut e = engine_with_targets(&[45.0, 1000.0, 50.0, 90.0, 5.0, 150.0, 0.3, 5.0]);
        let _ = process_block(&mut e, 4096); // 빔포머 링 버퍼 채우는 과도 구간 스킵
        let buf = process_block(&mut e, 4096);
        assert!(rms(&buf) > 1e-2, "rms={}", rms(&buf));
    }

    #[test]
    fn farther_target_is_quieter() {
        // 동일 환경 잡음을 상쇄하고 표적 성분의 거리 감쇠만 비교한다.
        let near = [0.0, 1000.0, 30.0, 90.0, 5.0, 150.0, 0.0, 0.0];
        let far = [0.0, 8000.0, 30.0, 90.0, 5.0, 150.0, 0.0, 0.0];
        let rn = rms(&process_target_component(&near, 4096, 8192));
        let rf = rms(&process_target_component(&far, 4096, 8192));
        assert!(rn > rf * 3.0, "near={rn} far={rf}");
    }

    #[test]
    fn higher_rpm_raises_tonal_frequency() {
        // rpm 120/5블레이드(10Hz) vs 40/5(3.33Hz) — 제로크로스 수 비교.
        // 동일 환경 잡음을 상쇄한 뒤 제로크로스가 토널 주파수를 따라가는지 본다.
        // 고조파(5개, −6dB/개)는 두 신호 모두에 기본파보다 큰 크로스 수를 더해
        // 기본파 3:1 갭이 제로크로스 비율로는 ~1.5로 압축됨 (측정 ≈1.48).
        // 임계는 1.3 — 잡음/고조파에 무관하게 "더 높은 주파수"를 단단히 가리키되
        // 고조파 압축을 감안해 여유를 둔 값.
        let hi_scene = [0.0, 1000.0, 30.0, 120.0, 5.0, 155.0, 0.0, 0.0];
        let lo_scene = [0.0, 1000.0, 30.0, 40.0, 5.0, 155.0, 0.0, 0.0];
        let hi = zero_crossings(&process_target_component(&hi_scene, 4096, 131072));
        let lo = zero_crossings(&process_target_component(&lo_scene, 4096, 131072));
        assert!(hi * 10 > lo * 13, "hi={hi} lo={lo}");
    }

    #[test]
    fn approaching_target_has_higher_zero_crossings() {
        // 도플러: 접근(+50m/s) vs 이탈(−50m/s), 18Hz 블레이드 레이트
        let approaching = [0.0, 1000.0, 30.0, 180.0, 6.0, 150.0, 0.0, 50.0];
        let receding = [0.0, 1000.0, 30.0, 180.0, 6.0, 150.0, 0.0, -50.0];
        let app = zero_crossings(&process_target_component(&approaching, 4096, 131072));
        let rec = zero_crossings(&process_target_component(&receding, 4096, 131072));
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn traced_processing_matches_realtime_output_exactly() {
        let scene = [0.0, 1000.0, 30.0, 120.0, 5.0, 155.0, 0.4, 5.0];
        let mut realtime = engine_with_targets(&scene);
        let mut traced = engine_with_targets(&scene);
        let expected = process_block(&mut realtime, 4096);
        let mut actual = vec![0.0; 4096];
        let mut trace = vec![DspTraceSample::default(); 4096];
        traced.process_traced(&mut actual, &mut trace);

        assert_eq!(actual, expected);
        assert!(trace.iter().all(|sample| sample.output_fs.is_finite()));
        assert!(trace
            .iter()
            .zip(&actual)
            .all(|(sample, output)| sample.output_fs == *output));
        assert!(trace.iter().any(|sample| sample.source_1m_upa != 0.0));
        assert!(trace.iter().any(|sample| sample.hydrophone_0_upa != 0.0));
    }

    #[test]
    fn bass_scan_peaks_near_target_bearing() {
        let mut engine = engine_with_targets(&[45.0, 500.0, 0.0, 120.0, 6.0, 170.0, 1.0, 0.0]);
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
        assert!(circular_error <= 1, "peak={}° expected=45°", peak * 5);
    }
}
