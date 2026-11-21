import { describe, expect, it } from 'vitest';
import fixtures from './contract.fixtures.json';
import { DEFAULT_SETTINGS } from './types';
import type { DownloadEvent, Settings, StateEvent, VizFrame } from './types';

describe('IPC contract fixtures', () => {
  it('state events match StateEvent', () => {
    const a = fixtures.stateEvent as StateEvent;
    const b = fixtures.stateEventError as StateEvent;
    expect(a.state).toBe('talking');
    expect(b.error?.code).toBe('noBackend');
    expect(a.notice).toBeUndefined();
  });

  it('notice event carries the notice', () => {
    const n = fixtures.stateEventNotice as StateEvent;
    expect(n.state).toBe('talking');
    expect(n.notice).toBe('Neuer Gesprächsabschnitt – Haru fängt frisch an.');
  });

  it('download events match DownloadEvent', () => {
    const all: DownloadEvent[] = [
      fixtures.downloadProgress as DownloadEvent,
      fixtures.downloadVerifying as DownloadEvent,
      fixtures.downloadDone as DownloadEvent,
      fixtures.downloadError as DownloadEvent,
    ];
    expect(all.map((e) => e.event)).toEqual(['progress', 'verifying', 'done', 'error']);
  });

  it('vizFrame matches VizFrame with 16 model and 8 mic bands', () => {
    const f = fixtures.vizFrame satisfies VizFrame;
    expect(f.model.bands).toHaveLength(16);
    expect(f.mic.bands).toHaveLength(8);
    expect(f.words[0]).toEqual({ seq: 7, text: ' Hal' });
  });

  it('settingsDefault deep-equals DEFAULT_SETTINGS', () => {
    const s = fixtures.settingsDefault as Settings;
    expect(s).toEqual(DEFAULT_SETTINGS);
    expect(DEFAULT_SETTINGS.echoCancellation).toBe(true);
  });
});
