// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IDLE_MS } from './hud';
import { getLanguage, onLanguageChange, setLanguage } from './strings';
import { TitleBar, type WindowControls } from './titlebar';

class FakeControls implements WindowControls {
  maximized = false;
  calls: string[] = [];
  resized: (() => void) | null = null;
  async minimize() { this.calls.push('minimize'); }
  async toggleMaximize() { this.calls.push('toggleMaximize'); }
  async close() { this.calls.push('close'); }
  async isMaximized() { return this.maximized; }
  onResized(cb: () => void) { this.resized = cb; }
}

const flush = () => vi.advanceTimersByTimeAsync(0);

describe('TitleBar', () => {
  let root: HTMLElement;
  let fake: FakeControls;
  let bars: TitleBar[];

  const btn = (name: string) => root.querySelector<HTMLButtonElement>(`.titlebar-${name}`)!;
  const bar = () => root.querySelector<HTMLElement>('.titlebar')!;

  beforeEach(() => {
    vi.useFakeTimers();
    setLanguage('en');
    root = document.createElement('div');
    document.body.append(root);
    fake = new FakeControls();
    bars = [new TitleBar(root, fake)];
  });

  afterEach(() => {
    vi.useRealTimers();
    root.remove();
    setLanguage('en');
  });

  it('has a drag strip and stable class names', () => {
    const strip = root.querySelector('.drag-strip')!;
    expect(strip.hasAttribute('data-tauri-drag-region')).toBe(true);
    expect(bar()).not.toBeNull();
    expect(bars).toHaveLength(1);
  });

  it('each button calls its adapter method', async () => {
    btn('minimize').click();
    btn('maximize').click();
    btn('close').click();
    await flush();
    expect(fake.calls).toEqual(['minimize', 'toggleMaximize', 'close']);
  });

  it('flips icon and label with the maximized state', async () => {
    await flush();
    const pathsOf = () => btn('maximize').querySelectorAll('path').length;
    expect(btn('maximize').getAttribute('aria-label')).toBe('Maximize');
    expect(pathsOf()).toBe(1);
    fake.maximized = true;
    fake.resized!();
    await flush();
    expect(btn('maximize').getAttribute('aria-label')).toBe('Restore');
    expect(btn('maximize').title).toBe('Restore');
    expect(pathsOf()).toBe(2);
    expect(bar().classList.contains('is-maximized')).toBe(true);
    fake.maximized = false;
    fake.resized!();
    await flush();
    expect(btn('maximize').getAttribute('aria-label')).toBe('Maximize');
    expect(pathsOf()).toBe(1);
  });

  it('starts in the maximized look when the window already is', async () => {
    root.replaceChildren();
    fake.maximized = true;
    bars.push(new TitleBar(root, fake));
    await flush();
    expect(btn('maximize').getAttribute('aria-label')).toBe('Restore');
  });

  it('dims after the idle time and wakes on pointermove', () => {
    expect(bar().classList.contains('is-dim')).toBe(false);
    vi.advanceTimersByTime(IDLE_MS - 1);
    expect(bar().classList.contains('is-dim')).toBe(false);
    vi.advanceTimersByTime(1);
    expect(bar().classList.contains('is-dim')).toBe(true);
    window.dispatchEvent(new Event('pointermove'));
    expect(bar().classList.contains('is-dim')).toBe(false);
    vi.advanceTimersByTime(IDLE_MS);
    expect(bar().classList.contains('is-dim')).toBe(true);
    window.dispatchEvent(new Event('keydown'));
    expect(bar().classList.contains('is-dim')).toBe(false);
  });

  it('relabel switches the language', async () => {
    await flush();
    expect(btn('close').getAttribute('aria-label')).toBe('Close');
    setLanguage('de');
    bars[0].relabel();
    expect(btn('close').getAttribute('aria-label')).toBe('Schließen');
    expect(btn('minimize').title).toBe('Minimieren');
    expect(btn('maximize').getAttribute('aria-label')).toBe('Maximieren');
  });

  it('onLanguageChange fires after setLanguage', () => {
    const cb = vi.fn();
    onLanguageChange(cb);
    setLanguage('fr');
    expect(cb).toHaveBeenCalled();
    expect(getLanguage()).toBe('fr');
  });
});
