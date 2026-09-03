//! 빔포밍 (M2) — 풀 지연-합 (delay-and-sum).
//!
//! 명세서 §3.2: `τᵢ = pᵢ · û / c`
//!
//! 스트리밍 제약: 실시간에서는 과거 샘플만 접근 가능하므로,
//! 전체 채널에 상수 시프트 `p_max = maxᵢ(pᵢ·û/c)`를 적용한다.
//! 빔 출력은 `p_max/c`만큼 지연되지만 채널 간 상대 지연은 정확하다.
//!
//! 분수 지연은 링 버퍼 + 선형 보간으로 구현.

/// 수중 음속 기본값 (m/s) — 배열 지연 계산용.
pub const DEFAULT_SOUND_SPEED: f32 = 1500.0;

/// 하이드로폰 i의 빔포밍 시간 지연 (s).
///
/// 명세서 §3.2: `τᵢ = pᵢ · û / c`.
///
/// `hydrophone`: 하이드로폰 위치 (m), `steer`: 조향 단위 벡터,
/// `sound_speed_ms`: 음속 (m/s).
pub fn beam_delay(hydrophone: [f32; 3], steer: [f32; 3], sound_speed_ms: f32) -> f32 {
    let dot = hydrophone[0] * steer[0] + hydrophone[1] * steer[1] + hydrophone[2] * steer[2];
    dot / sound_speed_ms
}

/// 풀 지연-합 빔포머 (스트리밍, M2).
///
/// 하이드로폰별로 링 버퍼를 유지하고, 조향 방향 `û`에 대해
/// 각 채널을 `p_max − pᵢ·û/c`만큼 지연시킨 뒤 합산(평균)한다.
#[derive(Debug, Clone)]
pub struct DelayAndSum {
    hydrophones: Vec<[f32; 3]>,
    sound_speed: f32,
    sample_rate: f32,
    buffers: Vec<Vec<f32>>,
    pos: usize,
}

impl DelayAndSum {
    /// 하이드로폰 배열 + 음속 + 샘플레이트로 생성.
    ///
    /// 버퍼 용량 = 최대 지연(`max|p|/c·fs`)의 2배 + 마진.
    pub fn new(hydrophones: Vec<[f32; 3]>, sound_speed: f32, sample_rate: f32) -> Self {
        let max_r = hydrophones
            .iter()
            .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
            .fold(0f32, f32::max);
        let max_delay_samples = (max_r / sound_speed * sample_rate).ceil() as usize;
        let capacity = max_delay_samples * 2 + 16;
        let buffers = (0..hydrophones.len())
            .map(|_| vec![0.0f32; capacity])
            .collect();
        Self {
            hydrophones,
            sound_speed,
            sample_rate,
            buffers,
            pos: 0,
        }
    }

    /// 하이드로폰 수.
    pub fn hydrophone_count(&self) -> usize {
        self.hydrophones.len()
    }

    /// 조향 방향 `û`에 대한 하이드로폰별 지연 `τᵢ = pᵢ·û/c` (s).
    pub fn delays(&self, steer: [f32; 3]) -> Vec<f32> {
        self.hydrophones
            .iter()
            .map(|p| beam_delay(*p, steer, self.sound_speed))
            .collect()
    }

    /// 하이드로폰별 1샘플을 넣고, 조향 방향 `û`로 지연-합 → 빔 출력 (평균, 정규화).
    pub fn process_sample(&mut self, samples: &[f32], steer: [f32; 3]) -> f32 {
        let n = self.hydrophones.len();
        debug_assert_eq!(samples.len(), n);
        let mut pmax = 0f32;
        let mut dots = Vec::with_capacity(n);
        for p in &self.hydrophones {
            let d = beam_delay(*p, steer, self.sound_speed);
            pmax = pmax.max(d);
            dots.push(d);
        }
        let mut sum = 0f32;
        for (h, &d) in dots.iter().enumerate() {
            let delay_s = pmax - d; // ≥ 0
            sum += self.read_delayed(h, delay_s * self.sample_rate);
        }
        for (h, s) in samples.iter().enumerate() {
            self.buffers[h][self.pos] = *s;
        }
        self.pos = (self.pos + 1) % self.buffers[0].len();
        sum / n as f32
    }

    /// 현재 링 버퍼 상태를 전진시키지 않고 다른 방향의 빔을 읽는다.
    ///
    /// 오디오용 주 빔과 별개로 BASS 전 방위 스캔을 만들 때 사용한다. 동일한
    /// 하이드로폰 샘플을 여러 방향에서 읽기만 하므로 엔진 시간은 변하지 않는다.
    pub fn beam_sample(&self, steer: [f32; 3]) -> f32 {
        let n = self.hydrophones.len();
        if n == 0 {
            return 0.0;
        }
        let mut pmax = 0f32;
        let mut dots = Vec::with_capacity(n);
        for p in &self.hydrophones {
            let d = beam_delay(*p, steer, self.sound_speed);
            pmax = pmax.max(d);
            dots.push(d);
        }
        let sum = dots
            .iter()
            .enumerate()
            .map(|(h, &d)| self.read_delayed(h, (pmax - d) * self.sample_rate))
            .sum::<f32>();
        sum / n as f32
    }

    /// `delay_samples`만큼 과거의 샘플 (선형 보간 분수 지연).
    fn read_delayed(&self, h: usize, delay_samples: f32) -> f32 {
        let cap = self.buffers[h].len();
        let d = delay_samples.max(0.0).min(cap as f32 - 2.0);
        let i = d.floor() as usize;
        let frac = d - i as f32;
        // 최신 샘플은 (pos − 1) mod cap
        let newest = if self.pos == 0 { cap - 1 } else { self.pos - 1 };
        let idx0 = newest.wrapping_sub(i) % cap;
        let idx1 = newest.wrapping_sub(i + 1) % cap;
        self.buffers[h][idx0] * (1.0 - frac) + self.buffers[h][idx1] * frac
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sphere_array() -> Vec<[f32; 3]> {
        // 지름 8.4m 구형 어레이: 단순 6점 (±축)
        let r = 4.2f32;
        vec![
            [r, 0.0, 0.0],
            [-r, 0.0, 0.0],
            [0.0, r, 0.0],
            [0.0, -r, 0.0],
            [0.0, 0.0, r],
            [0.0, 0.0, -r],
        ]
    }

    #[test]
    fn delay_is_zero_when_hydrophone_at_origin() {
        let d = beam_delay([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 1500.0);
        assert!((d - 0.0).abs() < 1e-9);
    }

    #[test]
    fn delay_positive_for_forward_hydrophone() {
        let d = beam_delay([4.2, 0.0, 0.0], [1.0, 0.0, 0.0], 1500.0);
        assert!((d - 4.2 / 1500.0).abs() < 1e-9);
    }

    #[test]
    fn delays_match_spec_formula() {
        let das = DelayAndSum::new(sphere_array(), 1500.0, 44100.0);
        let steer = [0.6, 0.0, 0.8]; // 단위 벡터
        for (p, tau) in das.hydrophones.iter().zip(das.delays(steer)) {
            let expect = (p[0] * steer[0] + p[1] * steer[1] + p[2] * steer[2]) / 1500.0;
            assert!((tau - expect).abs() < 1e-9, "{tau} vs {expect}");
        }
    }

    /// 하이드로폰별 신호를 `n`샘플 공급하고, `steer`로 빔 출력 피크 진폭 측정.
    fn feed_and_peak(
        das: &mut DelayAndSum,
        per_hydro: &[Vec<f32>],
        n: usize,
        steer: [f32; 3],
    ) -> f32 {
        let mut peak = 0f32;
        for i in 0..n {
            let row: Vec<f32> = per_hydro.iter().map(|v| v[i % v.len()]).collect();
            peak = peak.max(das.process_sample(&row, steer).abs());
        }
        peak
    }

    /// `dir` 방향에서 오는 f Hz 파동의 하이드로폰별 신호 (x_h(t) = cos(2πf(t − p_h·dir/c))).
    fn wave_from(
        arr: &[[f32; 3]],
        dir: [f32; 3],
        f: f32,
        fs: f32,
        c: f32,
        n: usize,
    ) -> Vec<Vec<f32>> {
        arr.iter()
            .map(|p| {
                let delay = (p[0] * dir[0] + p[1] * dir[1] + p[2] * dir[2]) / c;
                (0..n)
                    .map(|i| (2.0 * PI * f * (i as f32 / fs - delay)).cos())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn coherent_wave_from_steer_direction_gives_full_gain() {
        let fs = 44100.0f32;
        let c = 1500.0f32;
        let f = 100.0f32;
        let arr = sphere_array();
        let steer = [1.0f32, 0.0, 0.0];
        let n = 4096;
        let per_hydro = wave_from(&arr, steer, f, fs, c, n);
        let mut das = DelayAndSum::new(arr, c, fs);
        let peak = feed_and_peak(&mut das, &per_hydro, n, steer);
        // 동위 합 → 진폭 1.0 (평균화 후)
        assert!((peak - 1.0).abs() < 0.05, "coherent peak = {peak}");
    }

    #[test]
    fn off_axis_wave_is_attenuated() {
        let fs = 44100.0f32;
        let c = 1500.0f32;
        let f = 100.0f32;
        let arr = sphere_array();
        // 파동은 +z에서 오지만 빔은 +x로 조향 → 채널 간 위상 불일치
        let n = 4096;
        let per_hydro = wave_from(&arr, [0.0, 0.0, 1.0], f, fs, c, n);
        let mut das = DelayAndSum::new(arr, c, fs);
        let peak = feed_and_peak(&mut das, &per_hydro, n, [1.0, 0.0, 0.0]);
        assert!(peak < 0.5, "off-axis peak = {peak}");
    }
}
