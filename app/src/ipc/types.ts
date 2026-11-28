export const STATE_EVENT = 'haru:state';
export type AppStateName = 'booting' | 'downloading' | 'loading' | 'ready' | 'talking' | 'error';
export type Backend = 'cuda' | 'vulkan' | 'metal' | 'cpu';
export type ErrorCode = 'download' | 'hashMismatch' | 'diskSpace' | 'noBackend' | 'worker' | 'noMicrophone'
  | 'audioDevice' | 'lagLimit' | 'settings' | 'io' | 'busy' | 'internal';
/**
 * Fine-grained message keys of every user-visible backend text (Rust `MsgKey::ALL`, same order as the
 * `messageKeys` contract fixture). The UI shows `S.messages[key]`; `message` stays German for logs and as fallback.
 */
export const MESSAGE_KEYS = [
  // errors
  'internal', 'busy', 'ioError', 'settingsSaveFailed', 'languageUnsupported', 'temperatureRange', 'topKRange',
  'lagLimitRange', 'backendUnknown', 'qualityUnknown', 'genderUnknown', 'backchannelUnknown', 'voiceFileMissing',
  'modelDirRelative', 'modelDirUnknown', 'audioDevicesUnreadable', 'noMicrophone', 'inputDeviceNotFound',
  'noOutputDevice', 'outputDeviceNotFound', 'micOpenFailed', 'micFormatUnsupported', 'micStartFailed',
  'outputOpenFailed', 'outputFormatUnsupported', 'outputStartFailed', 'audioStartFailed', 'audioConvertFailed',
  'audioDeviceLost', 'micNoData', 'noBackendBuild', 'noBackend', 'backendDevicesInvalid', 'backendDevicesFailed',
  'noBackendStarted', 'downloadStartFailed', 'downloadFailed', 'diskSpace', 'downloadRejected', 'downloadCorrupt',
  'workerConnectionLost', 'workerStartFailed', 'workerExited', 'workerNotReady', 'workerBadReply',
  'workerIncompatible', 'workerError', 'workerBadAudio', 'workerBadFrame', 'conversationAborted', 'workerNoAnswer',
  'workerUnresponsive', 'lagLimitBacklog', 'lagLimit',
  // warnings and notices
  'cpuSlow', 'noCapableGpu', 'smallGpu', 'backendCrashedSwitched', 'backendFailedSwitched', 'segmentRenewed',
  'aecUnavailable', 'aecFailed', 'battleUnsupported', 'battleTopic',
] as const;
export type MessageKey = typeof MESSAGE_KEYS[number];
/** Placeholder values; numbers are shown with one decimal in the active language. */
export type MessageParams = Record<string, string | number>;
/** A backend text: key plus params for the UI, the German rendering in `message`. */
export interface UiText { key: MessageKey; params?: MessageParams; message: string }
/** `key` is missing when an older backend or an unknown key arrives; `'unexpected'` marks frontend fallbacks. */
export interface HaruError { code: ErrorCode; key?: MessageKey | 'unexpected'; params?: MessageParams; message: string; detail: string | null }
export interface StateEvent { state: AppStateName; backend: Backend | null; device: string | null; error: HaruError | null; warning: UiText | null; notice?: UiText | null }
export type DownloadEvent =
  | { event: 'progress'; data: { downloaded: number; total: number; bytesPerSec: number } }
  | { event: 'verifying'; data: { done: number; total: number } }
  | { event: 'done'; data: { path: string } }
  | { event: 'error'; data: UiText };
/** Who speaks in a battle: the angel (Haru) or the daemon (Mephisto). */
export type Speaker = 'angel' | 'daemon';
export interface WordPiece { seq: number; text: string; speaker?: Speaker }
/** Played audio of one voice: RMS and 16 bands. */
export interface Levels { rms: number; bands: number[] }
export type BattlePhase = 'preparing' | 'playing' | 'paused';
/** Battle only: each voice's levels, the buffer phase and the start progress (0..1). */
export interface BattleViz { angel: Levels; daemon: Levels; phase: BattlePhase; prepared: number }
export interface VizFrame { t: number; model: Levels; mic: { rms: number; bands: number[] }; latencyMs: number; words: WordPiece[]; battle?: BattleViz }
export type Quality = 'auto' | 'low' | 'medium' | 'high' | 'ultra';
export type QualityLevel = Exclude<Quality, 'auto'>;
/** Which Haru the user picked on the start screen. */
export type Persona = 'angel' | 'daemon';
export const LANGUAGES = ['de', 'en', 'es', 'fr', 'ja', 'ko', 'pt', 'ru', 'zh'] as const;
export type Language = typeof LANGUAGES[number];
export interface Settings {
  language: string; name: string; gender: 'female' | 'male' | null; backchannel: 'low' | 'mid' | 'high' | null;
  speakFirst: boolean; temperature: number; topK: number; voicePath: string | null; inputDevice: string | null; outputDevice: string | null;
  backend: 'auto' | Backend; device: string; quality: Quality; subtitles: boolean; showFace: boolean; lagLimitSecs: number; headphoneHintSeen: boolean;
  echoCancellation: boolean;
}
export const DEFAULT_SETTINGS: Settings = { language: 'de', name: 'Haru', gender: null, backchannel: null, speakFirst: true,
  temperature: 0.6, topK: 25, voicePath: null, inputDevice: null, outputDevice: null, backend: 'auto', device: 'auto', quality: 'auto',
  subtitles: false, showFace: true, lagLimitSecs: 1.0, headphoneHintSeen: false,
  echoCancellation: true };
export interface SaveSettingsResult { settings: Settings; reloadRequired: boolean }
export interface AudioDevices { inputs: string[]; outputs: string[]; defaultInput: string | null; defaultOutput: string | null }
export interface DeviceInfo { variant: Backend; name: string; description: string; kind: 'cpu' | 'gpu' | 'igpu'; memoryTotal: number }
export interface BackendOptions { variants: Backend[]; devices: DeviceInfo[]; cudaAvailable: boolean; backendsDir: string | null }
export interface AttemptInfo { backend: Backend; device: string; description: string }
/** `battle`: the loaded worker advertised the battle feature. */
export interface BootInfo { backend: Backend; device: string; deviceName: string; modelPath: string; plan: AttemptInfo[]; warning: UiText | null; battle: boolean }

export interface HaruClient {
  getStatus(): Promise<StateEvent>;
  onState(cb: (e: StateEvent) => void): Promise<() => void>;
  getSettings(): Promise<Settings>;
  saveSettings(s: Settings): Promise<SaveSettingsResult>;
  listAudioDevices(): Promise<AudioDevices>;
  backendOptions(): Promise<BackendOptions>;
  boot(onDownload: (e: DownloadEvent) => void): Promise<BootInfo>;
  startConversation(persona: Persona, onViz: (f: VizFrame) => void): Promise<void>;
  /** A battle on `topic` (angel against daemon, no microphone); `stopConversation` ends it too. */
  startBattle(topic: string, onViz: (f: VizFrame) => void): Promise<void>;
  stopConversation(): Promise<void>;
  setMuted(muted: boolean): Promise<void>;
  playCue(): Promise<void>;
}
