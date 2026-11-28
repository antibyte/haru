// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';
import { Hud } from './hud';

let root: HTMLElement; let hud: Hud;
beforeEach(() => { document.body.innerHTML = '<div id="r"></div>'; root = document.getElementById('r')!; hud = new Hud(root); });
const hidden = (sel: string) => root.querySelector<HTMLElement>(sel)!.hidden;

describe('Hud in a battle', () => {
  it('hides only the mic group and Mute, keeping Settings, Fullscreen and Stop', () => {
    hud.setBattle(true);
    expect(root.querySelector('.hud')!.classList.contains('is-battle')).toBe(true);
    expect(['.hud-left', '.hud-mute'].map(hidden)).toEqual([true, true]);
    expect(['.hud-settings', '.hud-fullscreen', '.hud-stop'].map(hidden)).toEqual([false, false, false]);
  });
  it('leaving battle mode restores every control', () => {
    hud.setBattle(true); hud.setBattle(false);
    expect(['.hud-left', '.hud-mute', '.hud-settings', '.hud-fullscreen', '.hud-stop'].map(hidden)).toEqual([false, false, false, false, false]);
    expect(root.querySelector('.hud')!.classList.contains('is-battle')).toBe(false);
  });
});
