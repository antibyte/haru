// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import fixtures from '../ipc/contract.fixtures.json';
import { LANGUAGES, type UiText } from '../ipc/types';
import { TABLES } from './locales';
import {
  LANGUAGE_NAMES, S, errorText, fill, getLanguage, resolveLanguage, sameText, setLanguage, uiText,
} from './strings';

type Tree = { readonly [k: string]: string | Tree };

/** Every leaf as [dotted path, text]. */
function leaves(t: Tree, prefix = ''): [string, string][] {
  return Object.entries(t).flatMap(([k, v]): [string, string][] =>
    typeof v === 'string' ? [[prefix + k, v]] : leaves(v, `${prefix}${k}.`));
}

const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

const de = TABLES.de!;
const deLeaves = leaves(de);
const others = Object.entries(TABLES).filter(([lang]) => lang !== 'de') as [string, Tree][];

describe('locale tables', () => {
  it('German and English exist', () => {
    expect(TABLES.de).toBeDefined();
    expect(TABLES.en).toBeDefined();
  });

  it('every supported language has a table', () => {
    for (const lang of LANGUAGES) expect(TABLES[lang], lang).toBeDefined();
    expect(Object.keys(TABLES).sort()).toEqual([...LANGUAGES].sort());
  });

  it('every locale has exactly the German key set', () => {
    const keys = deLeaves.map(([k]) => k).sort();
    for (const [lang, table] of others) {
      expect(leaves(table).map(([k]) => k).sort(), lang).toEqual(keys);
    }
  });

  it('no locale has empty strings', () => {
    for (const [lang, table] of Object.entries(TABLES) as [string, Tree][]) {
      for (const [k, v] of leaves(table)) expect(v.trim(), `${lang}.${k}`).not.toBe('');
    }
  });

  it('placeholders match German in every locale', () => {
    const want = new Map(deLeaves.map(([k, v]) => [k, placeholders(v)]));
    for (const [lang, table] of others) {
      for (const [k, v] of leaves(table)) expect(placeholders(v), `${lang}.${k}`).toEqual(want.get(k));
    }
  });

  it('messages cover every Rust key plus unexpected', () => {
    expect(Object.keys(de.messages).sort()).toEqual([...fixtures.messageKeys, 'unexpected'].sort());
  });

  it('every language has a name in its own language', () => {
    expect(Object.keys(LANGUAGE_NAMES).sort()).toEqual([...LANGUAGES].sort());
  });
});

describe('language selection', () => {
  afterEach(() => { setLanguage('de'); });

  it('resolveLanguage maps tags and falls back to en', () => {
    expect(resolveLanguage('pt-BR')).toBe('pt');
    expect(resolveLanguage('zh-Hant-TW')).toBe('zh');
    expect(resolveLanguage('en_GB.UTF-8')).toBe('en');
    expect(resolveLanguage('de_DE@euro')).toBe('de');
    expect(resolveLanguage('JA')).toBe('ja');
    expect(resolveLanguage(' fr ')).toBe('fr');
    expect(resolveLanguage('nl-NL')).toBe('en');
    expect(resolveLanguage('C')).toBe('en');
    expect(resolveLanguage('')).toBe('en');
    expect(resolveLanguage(null)).toBe('en');
    expect(resolveLanguage(undefined)).toBe('en');
  });

  it('setLanguage sets document lang and the table', () => {
    expect(setLanguage('en-US')).toBe('en');
    expect(document.documentElement.lang).toBe('en');
    expect(getLanguage()).toBe('en');
    expect(S).toBe(TABLES.en);
    setLanguage('de-AT');
    expect(document.documentElement.lang).toBe('de');
    expect(S).toBe(TABLES.de);
    setLanguage('xx');
    expect(getLanguage()).toBe('en');
    expect(S).toBe(TABLES.en);
  });

  it('every supported language shows its own table', () => {
    for (const lang of LANGUAGES) {
      setLanguage(lang);
      expect(getLanguage()).toBe(lang);
      expect(S, lang).toBe(TABLES[lang]);
      expect(document.documentElement.lang, lang).toBe(lang);
    }
  });
});

describe('texts', () => {
  afterEach(() => { setLanguage('de'); });

  it('fill formats numeric params per language', () => {
    setLanguage('de');
    expect(fill('{x} GB', { x: 2.5 })).toBe('2,5 GB');
    expect(fill('{x} GB', { x: 7 })).toBe('7,0 GB');
    setLanguage('en');
    expect(fill('{x} GB', { x: 2.5 })).toBe('2.5 GB');
    expect(fill('„{name}“ und {name}', { name: 'USB' })).toBe('„USB“ und USB');
    expect(fill('{a} {missing}', { a: 'x' })).toBe('x {missing}');
    expect(fill('{a}')).toBe('{a}');
  });

  it('uiText uses the active table and falls back to message for an unknown key', () => {
    const t: UiText = { key: 'backendCrashedSwitched', params: { from: 'CUDA', to: 'Vulkan' }, message: 'Backend CUDA abgestürzt – gewechselt auf Vulkan.' };
    setLanguage('de');
    expect(uiText(t)).toBe(t.message);
    setLanguage('en');
    expect(uiText(t)).toBe('Backend CUDA crashed – switched to Vulkan.');
    expect(uiText({ key: 'fromTheFuture' as UiText['key'], message: 'Neu.' })).toBe('Neu.');
  });

  it('errorText falls back to message without key', () => {
    setLanguage('en');
    expect(errorText({ message: 'Kaputt.' })).toBe('Kaputt.');
    expect(errorText({ key: 'unexpected', message: 'Unerwarteter Fehler.' })).toBe('Unexpected error.');
    expect(errorText({ key: 'inputDeviceNotFound', params: { name: 'USB Mic' }, message: '' })).toBe('Microphone “USB Mic” was not found.');
  });

  it('sameText compares key and params', () => {
    const a: UiText = { key: 'smallGpu', params: { device: 'A' }, message: 'x' };
    expect(sameText(a, { key: 'smallGpu', params: { device: 'A' }, message: 'other text' })).toBe(true);
    expect(sameText(a, { key: 'smallGpu', params: { device: 'B' }, message: 'x' })).toBe(false);
    expect(sameText(a, { key: 'smallGpu', message: 'x' })).toBe(false);
    expect(sameText(a, { key: 'cpuSlow', params: { device: 'A' }, message: 'x' })).toBe(false);
    expect(sameText({ key: 'cpuSlow', message: 'x' }, { key: 'cpuSlow', params: {}, message: 'y' })).toBe(true);
    expect(sameText(a, null)).toBe(false);
    expect(sameText(null, undefined)).toBe(true);
  });
});
