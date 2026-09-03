//! 네이티브 오프라인 렌더 결과의 직렬화와 수치 요약.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::DspTraceSample;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockMetrics {
    pub sample_count: usize,
    pub rms_fs: f32,
    pub peak_abs_fs: f32,
    pub mean_fs: f32,
    pub zero_crossings: usize,
    pub fnv1a64: u64,
}

pub fn measure_block(samples: &[f32]) -> BlockMetrics {
    let count = samples.len().max(1) as f64;
    let sum = samples.iter().map(|sample| *sample as f64).sum::<f64>();
    let power = samples
        .iter()
        .map(|sample| (*sample as f64).powi(2))
        .sum::<f64>();
    let peak_abs_fs = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0f32, f32::max);
    let zero_crossings = samples
        .windows(2)
        .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
        .count();

    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in samples.iter().flat_map(|sample| sample.to_le_bytes()) {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    BlockMetrics {
        sample_count: samples.len(),
        rms_fs: (power / count).sqrt() as f32,
        peak_abs_fs,
        mean_fs: (sum / count) as f32,
        zero_crossings,
        fnv1a64: hash,
    }
}

/// IEEE float32 mono WAV를 만들어 DSP 출력을 양자화 없이 보존한다.
pub fn encode_float_wav(sample_rate: u32, samples: &[f32]) -> Vec<u8> {
    let data_len = u32::try_from(samples.len().saturating_mul(4)).expect("WAV data too large");
    let sample_count = u32::try_from(samples.len()).expect("WAV sample count too large");
    let mut wav = Vec::with_capacity(56 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(48 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&3u16.to_le_bytes()); // WAVE_FORMAT_IEEE_FLOAT
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&32u16.to_le_bytes());
    wav.extend_from_slice(b"fact");
    wav.extend_from_slice(&4u32.to_le_bytes());
    wav.extend_from_slice(&sample_count.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

pub fn write_float_wav(path: &Path, sample_rate: u32, samples: &[f32]) -> io::Result<()> {
    std::fs::write(path, encode_float_wav(sample_rate, samples))
}

pub fn write_trace_csv(path: &Path, trace: &[DspTraceSample]) -> io::Result<()> {
    let mut writer = BufWriter::new(File::create(path)?);
    writeln!(
        writer,
        "sample,source_1m_upa,hydrophone_0_upa,receiver_fs,output_fs"
    )?;
    for (index, sample) in trace.iter().enumerate() {
        writeln!(
            writer,
            "{index},{:.6},{:.6},{:.9},{:.9}",
            sample.source_1m_upa, sample.hydrophone_0_upa, sample.receiver_fs, sample.output_fs
        )?;
    }
    writer.flush()
}

pub fn write_summary_json(path: &Path, sample_rate: u32, metrics: BlockMetrics) -> io::Result<()> {
    let mut writer = BufWriter::new(File::create(path)?);
    writeln!(writer, "{{")?;
    writeln!(writer, "  \"format\": \"f32le-mono\",")?;
    writeln!(writer, "  \"sample_rate_hz\": {sample_rate},")?;
    writeln!(writer, "  \"sample_count\": {},", metrics.sample_count)?;
    writeln!(writer, "  \"rms_fs\": {:.9},", metrics.rms_fs)?;
    writeln!(writer, "  \"peak_abs_fs\": {:.9},", metrics.peak_abs_fs)?;
    writeln!(writer, "  \"mean_fs\": {:.9},", metrics.mean_fs)?;
    writeln!(writer, "  \"zero_crossings\": {},", metrics.zero_crossings)?;
    writeln!(writer, "  \"fnv1a64\": \"{:016x}\"", metrics.fnv1a64)?;
    writeln!(writer, "}}")?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DspEngine;

    #[test]
    fn metrics_and_hash_are_deterministic() {
        let samples = [-0.5, 0.25, -0.125, 0.0, 1.0];
        let first = measure_block(&samples);
        let second = measure_block(&samples);
        assert_eq!(first, second);
        assert_eq!(first.sample_count, 5);
        assert_eq!(first.peak_abs_fs, 1.0);
        assert_eq!(first.zero_crossings, 3);
        assert_ne!(first.fnv1a64, 0);
    }

    #[test]
    fn wav_header_describes_float32_mono_samples() {
        let wav = encode_float_wav(44_100, &[0.25, -0.5]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 3);
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 44_100);
        assert_eq!(&wav[36..40], b"fact");
        assert_eq!(u32::from_le_bytes(wav[44..48].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(wav[52..56].try_into().unwrap()), 8);
        assert_eq!(&wav[56..60], &0.25f32.to_le_bytes());
    }

    #[test]
    fn reference_scene_matches_golden_block_metrics() {
        let row = include_str!("../../data/acoustics/golden/dsp_reference_scene.csv")
            .lines()
            .nth(1)
            .unwrap();
        let fields = row.split(',').collect::<Vec<_>>();
        let sample_rate = fields[0].parse::<u32>().unwrap();
        let warmup_seconds = fields[1].parse::<f32>().unwrap();
        let duration_seconds = fields[2].parse::<f32>().unwrap();
        let expected_rms = fields[3].parse::<f32>().unwrap();
        let rms_tolerance = fields[4].parse::<f32>().unwrap();
        let expected_peak = fields[5].parse::<f32>().unwrap();
        let peak_tolerance = fields[6].parse::<f32>().unwrap();
        let expected_crossings = fields[7].parse::<usize>().unwrap();
        let crossing_tolerance = fields[8].parse::<usize>().unwrap();

        let mut engine = DspEngine::new(sample_rate as f32);
        engine.set_targets(&[0.0, 1000.0, 30.0, 120.0, 5.0, 155.0, 0.4, 5.0]);
        engine.set_beam(0.0, 0.0);
        let mut warmup = vec![0.0; (warmup_seconds * sample_rate as f32) as usize];
        engine.process(&mut warmup);
        let mut output = vec![0.0; (duration_seconds * sample_rate as f32) as usize];
        engine.process(&mut output);
        let actual = measure_block(&output);

        assert!((actual.rms_fs - expected_rms).abs() <= rms_tolerance);
        assert!((actual.peak_abs_fs - expected_peak).abs() <= peak_tolerance);
        assert!(actual.zero_crossings.abs_diff(expected_crossings) <= crossing_tolerance);
    }
}
