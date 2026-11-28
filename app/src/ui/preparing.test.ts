// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';
import { TABLES } from './locales';
import { PreparingOverlay } from './preparing';
import { setLanguage } from './strings';

let root: HTMLElement; let ov: PreparingOverlay;
beforeEach(() => { setLanguage('de'); document.body.innerHTML = '<div id="r"></div>'; root = document.getElementById('r')!; ov = new PreparingOverlay(root); });
const el = () => root.querySelector('.preparing')!;
const fill = () => root.querySelector<HTMLElement>('.preparing-fill')!.style.width;

describe('PreparingOverlay', () => {
  it('is visible while preparing or paused, with the progress', () => {
    ov.update('preparing', 0.4);
    expect(el().classList.contains('is-visible')).toBe(true);
    expect(fill()).toBe('40%');
    ov.update('paused', 0.123);
    expect(ov.isVisible).toBe(true);
    expect(fill()).toBe('12.3%');
    ov.update('paused', 7);
    expect(fill()).toBe('100%');
  });
  it('hides while playing and on hide()', () => {
    ov.update('preparing', 0.5); ov.update('playing', 1);
    expect(ov.isVisible).toBe(false);
    ov.update('preparing', 0); ov.hide();
    expect(ov.isVisible).toBe(false);
  });
  it('shows the text in the active language', () => {
    expect(root.querySelector('.preparing-text')!.textContent).toBe('Haru und Mephisto bereiten sich vor …');
    setLanguage('en'); ov.relabel();
    expect(root.querySelector('.preparing-text')!.textContent).toBe(TABLES.en!.battle.preparing);
  });
});
