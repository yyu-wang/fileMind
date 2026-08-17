import { describe, it, expect } from 'vitest';
import { FEATURE_FLAGS, MAX_FILE_SIZE_MB, SUPPORTED_FILE_TYPES } from '../src/lib/constants/flags';

describe('Feature Flags', () => {
  it('should have RAG_ENABLED as boolean', () => {
    expect(typeof FEATURE_FLAGS.RAG_ENABLED).toBe('boolean');
  });

  it('should have CLOUD_INFERENCE as boolean', () => {
    expect(typeof FEATURE_FLAGS.CLOUD_INFERENCE).toBe('boolean');
  });

  it('should have RULE_EDITOR as boolean', () => {
    expect(typeof FEATURE_FLAGS.RULE_EDITOR).toBe('boolean');
  });
});

describe('Constants', () => {
  it('should have reasonable max file size', () => {
    expect(MAX_FILE_SIZE_MB).toBeGreaterThan(0);
    expect(MAX_FILE_SIZE_MB).toBeLessThan(10000);
  });

  it('should have supported file types', () => {
    expect(SUPPORTED_FILE_TYPES.length).toBeGreaterThan(0);
    expect(SUPPORTED_FILE_TYPES).toContain('.txt');
    expect(SUPPORTED_FILE_TYPES).toContain('.pdf');
  });
});
