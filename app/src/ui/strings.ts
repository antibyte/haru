import { LANGUAGES, type HaruError, type Language, type MessageParams, type UiText } from '../ipc/types';
import { TABLES } from './locales';
import { de, type Strings } from './locales/de';
import { en } from './locales/en';

export type { Strings };

/** Not translated. */
export const APP_NAME = 'Haru';

/** Every language in its own name (not translated). */
export const LANGUAGE_NAMES: Record<Language, string> = {
  de: 'Deutsch', en: 'English', es: 'Español', fr: 'Français', ja: '日本語', ko: '한국어', pt: 'Português', ru: 'Русский', zh: '中文',
};

/** Shown for every language Haru does not support, and for supported languages without a table yet. */
export const FALLBACK_LANGUAGE: Language = 'en';

/** The selected language (settings.language, normalized). */
let language: Language = 'de';
/** The language of the active table and of number formatting (differs from `language` only while a table is missing). */
let locale: Language = 'de';

/**
 * The active UI strings. A live binding: always read `S.x` at the moment of use; never destructure `S` or keep
 * `S.x` in module constants or long-lived values, or a language switch will not reach that text.
 */
export let S: Strings = de;

const formats = new Map<string, Intl.NumberFormat>();

export function getLanguage(): Language { return language; }

const languageListeners: (() => void)[] = [];

/** Calls `cb` after every `setLanguage` (for components that live outside App, e.g. the title bar). */
export function onLanguageChange(cb: () => void): void { languageListeners.push(cb); }

/** Primary subtag of a language tag ("pt-BR", "zh-Hant-TW", "en_GB.UTF-8", "de_DE@euro"), like Rust; otherwise English. */
export function resolveLanguage(code: string | null | undefined): Language {
  const primary = (code ?? '').trim().split(/[-_.@]/)[0].toLowerCase();
  return (LANGUAGES as readonly string[]).includes(primary) ? (primary as Language) : FALLBACK_LANGUAGE;
}

/** Switches `S`, number formatting and `<html lang>`. Returns the resolved language. */
export function setLanguage(code: string | null | undefined): Language {
  language = resolveLanguage(code);
  const table = TABLES[language];
  locale = table ? language : FALLBACK_LANGUAGE;
  S = table ?? TABLES[FALLBACK_LANGUAGE] ?? en;
  formats.clear();
  if (typeof document !== 'undefined') document.documentElement.lang = locale;
  for (const cb of languageListeners) cb();
  return language;
}

function numberFormat(cacheKey: string, options: Intl.NumberFormatOptions): Intl.NumberFormat {
  let f = formats.get(cacheKey);
  if (!f) {
    f = new Intl.NumberFormat(locale, options);
    formats.set(cacheKey, f);
  }
  return f;
}

/** Fixed decimals in the active language, without grouping: de "6,62", en "6.62". */
export function formatNumber(n: number, digits: number): string {
  return numberFormat(`n${digits}`, { minimumFractionDigits: digits, maximumFractionDigits: digits, useGrouping: false }).format(n);
}

/** `pct` in 0..100 as a whole percentage in the active language: de "42 %" (narrow no-break space), en "42%". */
export function formatPercent(pct: number): string {
  return numberFormat('pct', { style: 'percent', maximumFractionDigits: 0 }).format(pct / 100);
}

/** Replaces `{name}` placeholders; numbers get one decimal in the active language. Unknown placeholders stay. */
export function fill(template: string, params?: MessageParams): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) => {
    if (!Object.prototype.hasOwnProperty.call(params, name)) return whole;
    const v = params[name];
    return typeof v === 'number' ? formatNumber(v, 1) : v;
  });
}

function message(key: string | undefined, params: MessageParams | undefined, fallback: string): string {
  const table: Readonly<Record<string, string>> = S.messages;
  return key !== undefined && Object.prototype.hasOwnProperty.call(table, key) ? fill(table[key], params) : fallback;
}

/** A backend text in the active language; the German `message` when the key is unknown. */
export function uiText(t: UiText): string {
  return message(t.key, t.params, t.message);
}

/** An error in the active language; the German `message` when there is no (known) key. */
export function errorText(e: Pick<HaruError, 'key' | 'params' | 'message'>): string {
  return message(e.key, e.params, e.message);
}

/** Same key and same params (the German `message` is ignored). */
export function sameText(a?: UiText | null, b?: UiText | null): boolean {
  if (!a || !b) return !a && !b;
  if (a.key !== b.key) return false;
  const pa = a.params ?? {};
  const pb = b.params ?? {};
  const ka = Object.keys(pa);
  return ka.length === Object.keys(pb).length && ka.every((k) => Object.prototype.hasOwnProperty.call(pb, k) && pa[k] === pb[k]);
}

/**
 * Static labels of one component: each registration sets the text now and again on every `apply()`, so `relabel()`
 * is a single call. Getters read `S` when they run.
 */
export class Labels {
  private readonly appliers: (() => void)[] = [];

  text<E extends HTMLElement>(el: E, get: () => string): E {
    return this.add(el, () => { el.textContent = get(); });
  }

  attr<E extends Element>(el: E, name: 'title' | 'aria-label' | 'placeholder', get: () => string): E {
    return this.add(el, () => el.setAttribute(name, get()));
  }

  apply(): void {
    for (const a of this.appliers) a();
  }

  private add<E>(el: E, applier: () => void): E {
    applier();
    this.appliers.push(applier);
    return el;
  }
}
