import { describe, expect, test } from 'bun:test';
import { getLocale } from './locale';

describe('landing locales', () => {
  test('uses root English and a reciprocal Chinese path under configured base', () => {
    const en = getLocale('en', '/preview/');
    const zh = getLocale('zh-cn', '/preview/');
    expect(en.basePath).toBe('/preview/');
    expect(en.switchPath).toBe('/preview/zh-cn/');
    expect(zh.basePath).toBe('/preview/zh-cn/');
    expect(zh.switchPath).toBe('/preview/');
    expect(zh.lang).toBe('zh-CN');
  });

  test('normalizes base paths without a trailing slash', () => {
    const en = getLocale('en', '/preview');
    const zh = getLocale('zh-cn', '/preview');
    expect(en.switchPath).toBe('/preview/zh-cn/');
    expect(zh.switchPath).toBe('/preview/');
    expect(en.lang).toBe('en');
    expect(zh.lang).toBe('zh-CN');
  });
});
