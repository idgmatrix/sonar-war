import { describe, expect, it } from 'vitest';
import { DEMO_SCENES } from '../src/dsp/demoScenes.ts';

const legacyData = (key: string): number[] => {
  const scene = DEMO_SCENES[key];
  if (scene.kind !== 'legacy') throw new Error(`${key} is not a legacy scene`);
  return scene.data;
};

describe('M2 청감 비교 장면', () => {
  it('각 프리셋의 주 비교 표적에 빔을 맞춘다', () => {
    for (const scene of Object.values(DEMO_SCENES)) {
      const targetBearings =
        scene.kind === 'legacy'
          ? Array.from({ length: scene.data.length / 8 }, (_, index) => scene.data[index * 8])
          : scene.targets.map((target) => target.bearingDeg);
      expect(targetBearings).toContain(scene.beamAzimuthDeg);
    }
  });

  it('접근과 이탈 프리셋은 나머지 조건이 같고 6% 이상 주파수 차이가 난다', () => {
    const approaching = legacyData('approach');
    const receding = legacyData('recede');
    expect(approaching.slice(0, 7)).toEqual(receding.slice(0, 7));
    expect(approaching[7]).toBe(50);
    expect(receding[7]).toBe(-50);

    const soundSpeedMs = 1500;
    const separation =
      (1 + approaching[7] / soundSpeedMs) / (1 + receding[7] / soundSpeedMs) - 1;
    expect(separation).toBeGreaterThan(0.06);
  });

  it('RPM 프리셋은 잡음 조건을 고정하고 blade-rate를 3배 벌린다', () => {
    const high = legacyData('rpmUp');
    const low = legacyData('rpmDown');
    expect(high.slice(0, 3)).toEqual(low.slice(0, 3));
    expect(high.slice(4)).toEqual(low.slice(4));

    const highBladeRate = (high[3] / 60) * high[4];
    const lowBladeRate = (low[3] / 60) * low[4];
    expect(highBladeRate / lowBladeRate).toBe(3);
    expect(high[5]).toBeGreaterThanOrEqual(160);
    expect(high[6]).toBeLessThanOrEqual(0.1);
  });
});
