import { describe, expect, it } from 'vitest';
import { encodeProfiledTargets, MERCHANT_PROFILE_CODES } from '../src/dsp/sourceProfiles.ts';

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
});
