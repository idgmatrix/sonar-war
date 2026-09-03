import { describe, expect, it } from 'vitest';
import {
  bearingFromRatio,
  frequencyBin,
  frequencyToRatio,
  harmonicFrequencies,
} from '../src/render/displays/sonarMath.ts';

describe('소나 표시 좌표 계산 (M3)', () => {
  it('화면 x 비율을 0..360도 방위로 바꾼다', () => {
    expect(bearingFromRatio(0)).toBe(0);
    expect(bearingFromRatio(0.5)).toBeCloseTo(179.5);
    expect(bearingFromRatio(1)).toBe(359);
  });

  it('주파수를 표시 폭에 맞게 제한한다', () => {
    expect(frequencyToRatio(250, 500)).toBe(0.5);
    expect(frequencyToRatio(800, 500)).toBe(1);
  });

  it('기본 주파수의 고조파 눈금을 최대 주파수까지 만든다', () => {
    expect(harmonicFrequencies(60, 250)).toEqual([60, 120, 180, 240]);
    expect(harmonicFrequencies(0, 250)).toEqual([]);
  });

  it('FFT 주파수 빈을 정확히 선택한다', () => {
    expect(frequencyBin(1_000, 48_000, 2_048)).toBe(85);
  });
});
