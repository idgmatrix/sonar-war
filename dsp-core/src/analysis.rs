//! 수신기 출력을 읽기만 하는 표시·분석 계층.
//!
//! 음원 생성, 전파, 수신기 상태 변경은 하지 않는다. 실시간 경로에서는 배열의 최신
//! 링 버퍼를 여러 방위로 읽어 BASS 전력만 누적한다.

/// BASS 전 방위 스캔 해상도 (5° 간격).
pub const BASS_BINS: usize = 72;
/// 매 샘플이 아니라 16샘플마다 스캔해 AudioWorklet 예산을 지킨다.
const BASS_DECIMATION: u64 = 16;

/// 전 방위 광대역 수동소나(BASS) 전력 누산기.
#[derive(Debug, Clone)]
pub struct BassAnalyzer {
    power: [f32; BASS_BINS],
    samples: u32,
    sample_index: u64,
}

impl BassAnalyzer {
    pub fn new() -> Self {
        Self {
            power: [0.0; BASS_BINS],
            samples: 0,
            sample_index: 0,
        }
    }

    /// 수신기 배열의 현재 상태를 방위별로 읽어 전력을 누적한다.
    ///
    /// 콜백에는 파동 진행 방향 단위 벡터를 전달한다. 분석기는 콜백을 통해 샘플을
    /// 읽을 뿐 수신기나 합성 시간축을 전진시키지 않는다.
    #[inline]
    pub fn observe<F>(&mut self, mut beam_sample: F)
    where
        F: FnMut([f32; 3]) -> f32,
    {
        if self.sample_index.is_multiple_of(BASS_DECIMATION) {
            for (bin, power) in self.power.iter_mut().enumerate() {
                let az = 2.0 * std::f32::consts::PI * bin as f32 / BASS_BINS as f32;
                // 파원 방위의 반대가 파동 진행 방향이다.
                let travel = [-az.cos(), 0.0, -az.sin()];
                let sample = beam_sample(travel);
                *power += sample * sample;
            }
            self.samples += 1;
        }
        self.sample_index += 1;
    }

    /// 현재 누적 구간의 방위별 평균 전력을 dBFS로 반환하고 구간을 비운다.
    pub fn read_levels(&mut self, out: &mut [f32]) {
        let count = self.samples.max(1) as f32;
        for (index, value) in out.iter_mut().enumerate() {
            let power = self.power.get(index).copied().unwrap_or(0.0) / count;
            *value = (10.0 * power.max(1e-12).log10()).clamp(-120.0, 0.0);
        }
        self.power.fill(0.0);
        self.samples = 0;
    }
}

impl Default for BassAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzer_reads_all_bearings_without_advancing_callback_state() {
        let mut analyzer = BassAnalyzer::new();
        let mut directions = Vec::new();
        analyzer.observe(|direction| {
            directions.push(direction);
            if direction[0] < -0.999 {
                0.5
            } else {
                0.0
            }
        });

        assert_eq!(directions.len(), BASS_BINS);
        assert!((directions[0][0] + 1.0).abs() < 1e-6);
        let mut levels = [0.0; BASS_BINS];
        analyzer.read_levels(&mut levels);
        assert!((levels[0] - 20.0 * 0.5f32.log10()).abs() < 1e-5);
        assert_eq!(levels[1], -120.0);
    }

    #[test]
    fn analyzer_decimates_and_resets_each_read_window() {
        let mut analyzer = BassAnalyzer::new();
        let mut calls = 0;
        for _ in 0..BASS_DECIMATION {
            analyzer.observe(|_| {
                calls += 1;
                1.0
            });
        }
        assert_eq!(calls, BASS_BINS);

        let mut levels = [0.0; BASS_BINS];
        analyzer.read_levels(&mut levels);
        assert!(levels.iter().all(|level| *level == 0.0));
        analyzer.read_levels(&mut levels);
        assert!(levels.iter().all(|level| *level == -120.0));
    }
}
