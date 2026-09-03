import { readFileSync } from 'node:fs';
import Ajv2020 from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';
import { describe, expect, it } from 'vitest';

type Confidence = 'A' | 'B' | 'C';

interface Parameter {
  id: string;
  value: number | { min: number; max: number };
  unit: string;
  reference_conditions: string;
  confidence: Confidence;
  uncertainty: string;
  evidence_refs: string[];
}

interface Catalog {
  schema_version: string;
  level_convention: Record<string, string>;
  layer_contract: { source_includes: string[]; source_excludes: string[] };
  references: Array<{ id: string; url: string; accessed_on: string }>;
  sources: Array<{
    id: string;
    priority: 'P0' | 'P1' | 'P2' | 'P3';
    status: string;
    confidence: Confidence;
    components: Array<{ parameters: Parameter[] }>;
    evidence_refs: string[];
    limitations: string[];
  }>;
}

const catalog = JSON.parse(
  readFileSync(new URL('../data/acoustics/catalog.json', import.meta.url), 'utf8'),
) as Catalog;
const schema = JSON.parse(
  readFileSync(new URL('../data/acoustics/schema/catalog.schema.json', import.meta.url), 'utf8'),
) as object;

describe('음향 소스 카탈로그', () => {
  it('Draft 2020-12 JSON Schema를 통과한다', () => {
    const validator = new Ajv2020({ allErrors: true });
    addFormats(validator);
    const validate = validator.compile(schema);
    const valid = validate(catalog);
    expect(validate.errors, JSON.stringify(validate.errors, null, 2)).toBeNull();
    expect(valid).toBe(true);
  });

  it('고유한 소스/출처 ID와 고정 레벨 규약을 가진다', () => {
    expect(catalog.schema_version).toBe('1.0.0');
    expect(catalog.level_convention).toEqual({
      source_level: 'dB re 1 µPa @ 1 m',
      spectral_density: 'dB re 1 µPa²/Hz',
      full_scale: '1 Pa = 1,000,000 µPa = 1.0 FS (120 dB re 1 µPa)',
    });

    const sourceIds = catalog.sources.map((source) => source.id);
    const referenceIds = catalog.references.map((reference) => reference.id);
    expect(new Set(sourceIds).size).toBe(sourceIds.length);
    expect(new Set(referenceIds).size).toBe(referenceIds.length);
  });

  it('발생원 계층에서 전파와 수신기 효과를 제외한다', () => {
    expect(catalog.layer_contract.source_includes).toContain('emission_at_reference_distance');
    expect(catalog.layer_contract.source_excludes).toEqual(
      expect.arrayContaining(['transmission_loss', 'doppler', 'receiver_array_response']),
    );
  });

  it('모든 수치 파라미터에 단위·조건·불확실성·유효한 출처가 있다', () => {
    const referenceIds = new Set(catalog.references.map((reference) => reference.id));

    for (const source of catalog.sources) {
      for (const ref of source.evidence_refs) expect(referenceIds.has(ref)).toBe(true);
      expect(source.limitations.length).toBeGreaterThan(0);

      for (const component of source.components) {
        for (const parameter of component.parameters) {
          expect(parameter.unit.length).toBeGreaterThan(0);
          expect(parameter.reference_conditions.length).toBeGreaterThan(0);
          expect(parameter.uncertainty.length).toBeGreaterThan(0);
          expect(['A', 'B', 'C']).toContain(parameter.confidence);
          expect(parameter.evidence_refs.length).toBeGreaterThan(0);
          for (const ref of parameter.evidence_refs) expect(referenceIds.has(ref)).toBe(true);

          if (typeof parameter.value !== 'number') {
            expect(parameter.value.max).toBeGreaterThanOrEqual(parameter.value.min);
          }
        }
      }
    }
  });

  it('P0 목록과 현재 근거 부족 상태를 명시한다', () => {
    const p0Ids = catalog.sources
      .filter((source) => source.priority === 'P0')
      .map((source) => source.id);
    expect(p0Ids).toEqual(
      expect.arrayContaining([
        'large-merchant-vessel',
        'submarine-generic',
        'torpedo-generic',
        'ocean-surface-ambient',
      ]),
    );

    for (const id of ['submarine-generic', 'torpedo-generic']) {
      const source = catalog.sources.find((candidate) => candidate.id === id)!;
      expect(source.status).toBe('research_required');
      expect(source.confidence).toBe('C');
    }
  });
});
