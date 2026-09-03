import { describe, expect, it } from 'vitest';
import {
  encodeProfiledTargets,
  encodeProfiledTargetsV2,
  MERCHANT_PROFILE_CODES,
  MERCHANT_TONAL_OVERLAY_CODES,
} from '../src/dsp/sourceProfiles.ts';

describe('상선 Source 프로파일 WASM 경계', () => {
  it('문서화된 안정 코드와 stride 7로 인코딩한다', () => {
    const encoded = encodeProfiledTargets([
      {
        bearingDeg: 45,
        rangeM: 3000,
        depthM: 6,
        sourceProfileId: 'merchant-bulker-jomopans-echo',
        speedKn: 13.5,
        lengthM: 211,
        relativeVelocityMs: 2,
      },
    ]);
    expect(MERCHANT_PROFILE_CODES['merchant-bulker-jomopans-echo']).toBe(1);
    expect([...encoded]).toEqual([45, 3000, 6, 1, 13.5, 211, 2]);
  });

  it('v2에서 측정 톤 운항점을 명시적으로 인코딩한다', () => {
    const encoded = encodeProfiledTargetsV2([
      {
        bearingDeg: 45,
        rangeM: 10000,
        depthM: 6,
        sourceProfileId: 'merchant-bulker-jomopans-echo',
        speedKn: 16,
        lengthM: 172.9,
        relativeVelocityMs: 0,
        tonalOverlayId: 'overseas-harriette-140rpm',
        shaftRpm: 140,
        bladeCount: 4,
      },
    ]);
    expect(MERCHANT_TONAL_OVERLAY_CODES['overseas-harriette-140rpm']).toBe(1);
    expect([...encoded]).toEqual([45, 10000, 6, 1, 16, expect.closeTo(172.9), 0, 1, 140, 4]);
  });
});
