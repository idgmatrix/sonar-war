/** 소나 표시계가 공유하는 순수 좌표/눈금 계산. */

export function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}

export function bearingFromRatio(ratio: number): number {
  // 오른쪽 끝을 0°로 되감으면 사용자가 359°를 선택할 수 없으므로 슬라이더
  // 유효 범위와 같은 0..359° 폐구간으로 매핑한다.
  return clamp01(ratio) * 359;
}

export function frequencyToRatio(frequencyHz: number, maxFrequencyHz: number): number {
  if (maxFrequencyHz <= 0) return 0;
  return clamp01(frequencyHz / maxFrequencyHz);
}

export function harmonicFrequencies(fundamentalHz: number, maxFrequencyHz: number): number[] {
  if (fundamentalHz <= 0 || maxFrequencyHz <= 0) return [];
  const count = Math.floor(maxFrequencyHz / fundamentalHz);
  return Array.from({ length: count }, (_, index) => fundamentalHz * (index + 1));
}

export function frequencyBin(
  frequencyHz: number,
  sampleRate: number,
  frequencyBinCount: number,
): number {
  if (sampleRate <= 0 || frequencyBinCount <= 0) return 0;
  const fftSize = frequencyBinCount * 2;
  return Math.min(
    frequencyBinCount - 1,
    Math.max(0, Math.round((frequencyHz * fftSize) / sampleRate)),
  );
}
