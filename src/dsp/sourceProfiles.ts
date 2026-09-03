/** Rust/WASM의 안정된 상선 profile_code와 데이터 파일 ID 사이의 경계. */
export const MERCHANT_PROFILE_CODES = {
  'merchant-bulker-jomopans-echo': 1,
  'merchant-containership-jomopans-echo': 2,
  'merchant-vehicle-carrier-jomopans-echo': 3,
  'merchant-tanker-jomopans-echo': 4,
} as const;

export type MerchantSourceProfileId = keyof typeof MERCHANT_PROFILE_CODES;
export type SourceProfileId = 'legacy-generic' | MerchantSourceProfileId;

export interface ProfiledTarget {
  bearingDeg: number;
  rangeM: number;
  depthM: number;
  sourceProfileId: MerchantSourceProfileId;
  speedKn: number;
  lengthM: number;
  relativeVelocityMs: number;
}

/**
 * profile target stride 7:
 * [bearing, range, depth, profile_code, speed_kn, length_m, rel_vel_ms]
 */
export function encodeProfiledTargets(targets: readonly ProfiledTarget[]): Float32Array {
  const encoded = new Float32Array(targets.length * 7);
  targets.forEach((target, index) => {
    encoded.set(
      [
        target.bearingDeg,
        target.rangeM,
        target.depthM,
        MERCHANT_PROFILE_CODES[target.sourceProfileId],
        target.speedKn,
        target.lengthM,
        target.relativeVelocityMs,
      ],
      index * 7,
    );
  });
  return encoded;
}
