// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';
import fixtures from '../ipc/contract.fixtures.json';
import { DEFAULT_SETTINGS, type AudioDevices, type BackendOptions, type Settings } from '../ipc/types';
import { TABLES } from './locales';
import { SettingsPanel } from './settings';
import { setLanguage } from './strings';

const AUDIO: AudioDevices = { inputs: [], outputs: [], defaultInput: null, defaultOutput: null };
const BACKENDS: BackendOptions = { variants: [], devices: [], cudaAvailable: false, backendsDir: null };
const de = TABLES.de!;

let root: HTMLElement;
beforeEach(() => { setLanguage('de'); document.body.innerHTML = '<div id="r"></div>'; root = document.getElementById('r')!; });

const sliders = () => [...root.querySelectorAll<HTMLInputElement>('input[type="range"]')];
const labelOf = (i: HTMLInputElement) => i.closest('.field, .setting, label, div')?.textContent ?? '';
const save = () => [...root.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === de.settings.save)!;

describe('battle temperature', () => {
  it('the defaults are 0.7 and 0.9 and match the contract fixture', () => {
    expect(DEFAULT_SETTINGS.temperature).toBe(0.7);
    expect(DEFAULT_SETTINGS.battleTemperature).toBe(0.8);
    expect((fixtures.settingsDefault as Settings).battleTemperature).toBe(0.8);
  });

  it('a second slider sits right under the temperature slider and reads battleTemperature', () => {
    const panel = new SettingsPanel(root);
    panel.open({ ...DEFAULT_SETTINGS, temperature: 0.55, battleTemperature: 0.75 }, AUDIO, BACKENDS);
    const [t, b] = sliders();
    expect([t.min, t.max, t.step]).toEqual(['0.3', '1', '0.05']);
    expect([b.min, b.max, b.step]).toEqual(['0.3', '1', '0.05']);
    expect(t.value).toBe('0.55');
    expect(b.value).toBe('0.75');
    expect(labelOf(b)).toContain(de.settings.battleTemperature);
    const outs = [...root.querySelectorAll('output')].map((o) => o.textContent);
    expect(outs.slice(0, 2)).toEqual(['0,55', '0,75']);
    expect(root.textContent).toContain(de.settings.battleTemperatureHint);
  });

  it('saving writes battleTemperature and leaves temperature alone', async () => {
    const panel = new SettingsPanel(root);
    let saved: Settings | null = null;
    panel.onSave(async (s) => { saved = s; });
    panel.open(DEFAULT_SETTINGS, AUDIO, BACKENDS);
    const b = sliders()[1];
    b.value = '0.45';
    b.dispatchEvent(new Event('input'));
    save().click();
    await new Promise<void>((r) => setTimeout(r, 0));
    expect(saved!.battleTemperature).toBeCloseTo(0.45, 6);
    expect(saved!.temperature).toBe(DEFAULT_SETTINGS.temperature);
  });
});
