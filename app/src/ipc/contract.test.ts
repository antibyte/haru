import { describe, expect, it, vi } from 'vitest';
import fixtures from './contract.fixtures.json';
import { DEFAULT_SETTINGS, MESSAGE_KEYS } from './types';
import { TauriClient, toHaruError } from './tauri-client';
import type { DownloadEvent, HaruError, Persona, Settings, StateEvent, VizFrame, WordPiece } from './types';
import { errorText, setLanguage, uiText } from '../ui/strings';

const invoke = vi.hoisted(() => vi.fn(async () => undefined));
vi.mock('@tauri-apps/api/core', () => ({
  invoke,
  Channel: class { onmessage: unknown = null; },
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

describe('IPC contract fixtures', () => {
  it('state events match StateEvent', () => {
    const a = fixtures.stateEvent as StateEvent;
    const b = fixtures.stateEventError as StateEvent;
    expect(a.state).toBe('talking');
    expect(b.error?.code).toBe('noBackend');
    expect(b.error?.key).toBe('noBackend');
    expect(b.error && 'params' in b.error).toBe(false); // omitted when empty
    expect(a.notice).toBeUndefined();
  });

  it('notice event carries the notice', () => {
    const n = fixtures.stateEventNotice as StateEvent;
    expect(n.state).toBe('talking');
    expect(n.notice?.key).toBe('segmentRenewed');
    expect(n.notice?.message).toBe('Neuer Gesprächsabschnitt – Haru fängt frisch an.');
  });

  it('warning event carries key and params, rendered like the German message', () => {
    const w = (fixtures.stateEventWarning as StateEvent).warning!;
    expect(w.key).toBe('smallGpu');
    expect(w.params).toEqual({ device: 'Intel(R) UHD Graphics' });
    setLanguage('de');
    expect(uiText(w)).toBe(w.message);
  });

  it('error params are numbers, rendered with a German comma like the message', () => {
    const e = fixtures.errorDiskSpace as HaruError;
    expect(e.key).toBe('diskSpace');
    expect(e.params).toEqual({ need: 7.5, free: 2.5 });
    setLanguage('de');
    expect(errorText(e)).toBe(e.message);
    setLanguage('en');
    expect(errorText(e)).toBe('Not enough disk space: 7.5 GB needed, 2.5 GB free.');
  });

  it('MESSAGE_KEYS equals fixtures.messageKeys (Rust MsgKey::ALL, same order)', () => {
    expect([...MESSAGE_KEYS]).toEqual(fixtures.messageKeys);
    expect(MESSAGE_KEYS).toHaveLength(64);
  });

  it('download events match DownloadEvent', () => {
    const all: DownloadEvent[] = [
      fixtures.downloadProgress as DownloadEvent,
      fixtures.downloadVerifying as DownloadEvent,
      fixtures.downloadDone as DownloadEvent,
      fixtures.downloadError as DownloadEvent,
    ];
    expect(all.map((e) => e.event)).toEqual(['progress', 'verifying', 'done', 'error']);
    const err = all[3];
    expect(err.event === 'error' && err.data.key).toBe('downloadFailed');
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

  it('start_conversation passes the persona as an argument next to the viz channel', async () => {
    const fx = fixtures.startConversation as { persona: Persona };
    expect(fx.persona).toBe('daemon');
    await new TauriClient().startConversation(fx.persona, () => {});
    expect(invoke).toHaveBeenCalledWith('start_conversation', expect.objectContaining({ persona: 'daemon', onViz: expect.anything() }));
  });

  it('settingsDefault no longer has a persona field', () => {
    expect('persona' in fixtures.settingsDefault).toBe(false);
    expect('persona' in DEFAULT_SETTINGS).toBe(false);
  });

  it('start_battle passes the topic next to the viz channel', async () => {
    await new TauriClient().startBattle('Darf man lügen?', () => {});
    expect(invoke).toHaveBeenCalledWith('start_battle', expect.objectContaining({ topic: 'Darf man lügen?', onViz: expect.anything() }));
  });

  it('battle frames carry per-speaker levels, phase and prepared; word pieces a speaker', () => {
    const bands = new Array<number>(16).fill(0);
    const piece: WordPiece = { seq: 3, text: ' Nein', speaker: 'daemon' };
    const f = {
      t: 2, model: { rms: 0.2, bands }, mic: { rms: 0, bands: new Array<number>(8).fill(0) }, latencyMs: 0, words: [piece],
      battle: { angel: { rms: 0.01, bands }, daemon: { rms: 0.2, bands }, phase: 'playing', prepared: 1 },
    } satisfies VizFrame;
    expect(f.battle.daemon.bands).toHaveLength(16);
    expect(f.words[0].speaker).toBe('daemon');
  });

  it('vizFrameBattle and bootInfo match the battle types', () => {
    const f = fixtures.vizFrameBattle as VizFrame;
    expect(f.battle!.daemon.bands).toHaveLength(16);
    expect(f.words[0]).toEqual({ seq: 3, text: ' nein', speaker: 'daemon' });
    expect(typeof fixtures.bootInfo.battle).toBe('boolean');
  });
});

describe('toHaruError', () => {
  it('keeps key/params and drops unknown key', () => {
    expect(toHaruError(fixtures.errorDiskSpace)).toEqual({
      code: 'diskSpace', key: 'diskSpace', params: { need: 7.5, free: 2.5 },
      message: 'Nicht genug Speicherplatz: 7,5 GB benötigt, 2,5 GB frei.', detail: null,
    });
    const unknown = toHaruError({ code: 'worker', key: 'somethingNew', params: { a: 1 }, message: 'Neu.', detail: 'x' });
    expect(unknown).toEqual({ code: 'worker', params: { a: 1 }, message: 'Neu.', detail: 'x' });
    expect('key' in unknown).toBe(false);
  });

  it('accepts params only as a plain object of strings and numbers', () => {
    expect(toHaruError({ code: 'io', key: 'ioError', params: ['x'], message: 'm', detail: null }).params).toBeUndefined();
    expect(toHaruError({ code: 'io', key: 'ioError', params: 'x', message: 'm', detail: null }).params).toBeUndefined();
    expect(toHaruError({ code: 'io', key: 'ioError', params: { a: 'x', b: 2, c: { d: 1 }, e: null }, message: 'm', detail: null }).params)
      .toEqual({ a: 'x', b: 2 });
  });

  it("fallback uses 'unexpected' and stays 'unexpected' when converted again", () => {
    const e = toHaruError(new Error('boom'));
    expect(e).toEqual({ code: 'internal', key: 'unexpected', message: 'Unerwarteter Fehler.', detail: 'boom' });
    expect(toHaruError(e)).toEqual(e);
    expect(toHaruError('kaputt').detail).toBe('kaputt');
  });
});
