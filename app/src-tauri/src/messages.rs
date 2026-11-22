//! Fine-grained message keys for every user-visible backend text.
//! The German template is the fallback (`message`); the frontend renders `S.messages[key]` with the params.
use std::collections::BTreeMap;

use serde::Serialize;

macro_rules! msg_keys {
    ($($variant:ident => $german:expr),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub enum MsgKey { $($variant),+ }

        impl MsgKey {
            pub const ALL: &'static [MsgKey] = &[$(MsgKey::$variant),+];
            pub fn german(self) -> &'static str {
                match self { $(MsgKey::$variant => $german),+ }
            }
        }
    };
}

msg_keys! {
    Internal => "Interner Fehler.",
    Busy => "Gerade nicht möglich.",
    IoError => "Dateifehler",
    SettingsSaveFailed => "Einstellungen konnten nicht gespeichert werden.",
    LanguageUnsupported => "Diese Sprache wird nicht unterstützt.",
    TemperatureRange => "Die Temperatur muss zwischen 0,3 und 1,0 liegen.",
    TopKRange => "Top-K muss zwischen 0 und 500 liegen.",
    LagLimitRange => "Die Verzögerungsgrenze muss zwischen 0,5 und 10 Sekunden liegen.",
    BackendUnknown => "Unbekanntes Backend.",
    QualityUnknown => "Unbekannte Qualitätsstufe.",
    GenderUnknown => "Unbekanntes Geschlecht.",
    BackchannelUnknown => "Unbekannte Rückmeldungsstärke.",
    VoiceFileMissing => "Die Stimmdatei wurde nicht gefunden.",
    ModelDirRelative => "SPEECH_MODEL_DIR muss ein absoluter Pfad sein.",
    ModelDirUnknown => "Der Modellordner konnte nicht bestimmt werden.",
    AudioDevicesUnreadable => "Audiogeräte konnten nicht gelesen werden.",
    NoMicrophone => "Kein Mikrofon gefunden.",
    InputDeviceNotFound => "Mikrofon „{name}“ wurde nicht gefunden.",
    NoOutputDevice => "Kein Ausgabegerät gefunden.",
    OutputDeviceNotFound => "Ausgabegerät „{name}“ wurde nicht gefunden.",
    MicOpenFailed => "Das Mikrofon konnte nicht geöffnet werden.",
    MicFormatUnsupported => "Das Mikrofon-Format wird nicht unterstützt.",
    MicStartFailed => "Das Mikrofon konnte nicht gestartet werden.",
    OutputOpenFailed => "Das Ausgabegerät konnte nicht geöffnet werden.",
    OutputFormatUnsupported => "Das Ausgabeformat wird nicht unterstützt.",
    OutputStartFailed => "Das Ausgabegerät konnte nicht gestartet werden.",
    AudioStartFailed => "Audio konnte nicht gestartet werden.",
    AudioConvertFailed => "Audio konnte nicht umgewandelt werden.",
    AudioDeviceLost => "Audiogerät nicht mehr verfügbar.",
    MicNoData => "Das Mikrofon liefert keine Daten.",
    NoBackendBuild => "Keine Backend-Variante gefunden. Baue sie mit scripts/build-backends.ps1 oder setze HARU_BACKENDS_DIR.",
    NoBackend => "Keine Backend-Variante gefunden.",
    BackendDevicesInvalid => "Die Geräteliste des Backends ist ungültig.",
    BackendDevicesFailed => "Die Geräte des Backends konnten nicht abgefragt werden.",
    NoBackendStarted => "Kein Backend konnte gestartet werden.",
    DownloadStartFailed => "Der Download konnte nicht gestartet werden.",
    DownloadFailed => "Der Download ist fehlgeschlagen. Bitte Internetverbindung prüfen.",
    DiskSpace => "Nicht genug Speicherplatz: {need} GB benötigt, {free} GB frei.",
    DownloadRejected => "Der Download-Server hat die Anfrage abgelehnt.",
    DownloadCorrupt => "Die heruntergeladene Datei ist beschädigt. Bitte erneut versuchen.",
    WorkerConnectionLost => "Die Verbindung zum Sprachmodell-Worker ist abgebrochen.",
    WorkerStartFailed => "Der Sprachmodell-Worker konnte nicht gestartet werden.",
    WorkerExited => "Der Sprachmodell-Worker wurde unerwartet beendet.",
    WorkerNotReady => "Der Sprachmodell-Worker wurde nicht rechtzeitig bereit.",
    WorkerBadReply => "Unerwartete Antwort vom Sprachmodell-Worker",
    WorkerIncompatible => "Das Modell oder der Worker ist nicht kompatibel.",
    WorkerError => "Der Sprachmodell-Worker meldet einen Fehler.",
    WorkerBadAudio => "Ungültige Audiodaten vom Worker",
    WorkerBadFrame => "Der Worker lieferte ein ungültiges Audio-Frame.",
    ConversationAborted => "Das Gespräch wurde abgebrochen.",
    WorkerNoAnswer => "Der Sprachmodell-Worker antwortet nicht.",
    WorkerUnresponsive => "Der Sprachmodell-Worker reagiert nicht mehr.",
    LagLimitBacklog => "Haru kommt nicht hinterher (Rückstand {backlog} s). Die GPU ist zu langsam oder ausgelastet.",
    LagLimit => "Haru kommt nicht hinterher. Die GPU ist zu langsam oder ausgelastet.",
    CpuSlow => "Läuft auf der CPU – das Gespräch kann zu langsam sein.",
    NoCapableGpu => "Keine Grafikkarte mit mindestens 8 GB gefunden – Haru läuft auf der CPU und kann zu langsam sein.",
    SmallGpu => "{device} hat weniger als 8 GB Grafikspeicher – Haru braucht mindestens 8 GB, das Gespräch kann abbrechen.",
    BackendCrashedSwitched => "Backend {from} abgestürzt – gewechselt auf {to}.",
    BackendFailedSwitched => "Backend {from} fehlgeschlagen – gewechselt auf {to}.",
    SegmentRenewed => "Neuer Gesprächsabschnitt – Haru fängt frisch an.",
    AecUnavailable => "Echounterdrückung nicht verfügbar – bitte Kopfhörer verwenden.",
    AecFailed => "Echounterdrückung ausgefallen – bitte Kopfhörer verwenden.",
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ParamValue {
    Text(String),
    Number(f64),
}

impl From<&str> for ParamValue { fn from(s: &str) -> Self { ParamValue::Text(s.to_string()) } }
impl From<String> for ParamValue { fn from(s: String) -> Self { ParamValue::Text(s) } }
impl From<f64> for ParamValue { fn from(n: f64) -> Self { ParamValue::Number(n) } }

pub type Params = BTreeMap<&'static str, ParamValue>;

/// Fills `{name}` placeholders; numbers get one decimal place with a German comma.
pub fn render(template: &str, p: &Params) -> String {
    let mut out = template.to_string();
    for (name, v) in p {
        let text = match v {
            ParamValue::Text(s) => s.clone(),
            ParamValue::Number(n) => format!("{n:.1}").replace('.', ","),
        };
        out = out.replace(&format!("{{{name}}}"), &text);
    }
    out
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiText {
    pub key: MsgKey,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub params: Params,
    pub message: String,
}

impl UiText {
    pub fn new(key: MsgKey) -> Self {
        UiText { key, params: Params::new(), message: key.german().to_string() }
    }
    pub fn with_param(mut self, name: &'static str, v: impl Into<ParamValue>) -> Self {
        self.params.insert(name, v.into());
        self.message = render(self.key.german(), &self.params);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_keys_are_unique() {
        let set: HashSet<String> = MsgKey::ALL.iter().map(|k| serde_json::to_string(k).unwrap()).collect();
        assert_eq!(set.len(), MsgKey::ALL.len());
        assert_eq!(MsgKey::ALL.len(), 62);
    }

    #[test]
    fn german_message_renders_params() {
        let t = UiText::new(MsgKey::DiskSpace).with_param("need", 7.5).with_param("free", 2.5);
        assert_eq!(t.message, "Nicht genug Speicherplatz: 7,5 GB benötigt, 2,5 GB frei.");
        let t = UiText::new(MsgKey::InputDeviceNotFound).with_param("name", "USB Mic");
        assert_eq!(t.message, "Mikrofon „USB Mic“ wurde nicht gefunden.");
    }

    #[test]
    fn params_omitted_when_empty() {
        let v = serde_json::to_value(UiText::new(MsgKey::SegmentRenewed)).unwrap();
        assert!(v.get("params").is_none());
        assert_eq!(v["key"], "segmentRenewed");
    }

    #[test]
    fn with_param_rerenders_message() {
        let t = UiText::new(MsgKey::BackendCrashedSwitched);
        assert!(t.message.contains("{from}"));
        let t = t.with_param("from", "cuda").with_param("to", "cpu");
        assert_eq!(t.message, "Backend cuda abgestürzt – gewechselt auf cpu.");
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["params"]["from"], "cuda");
    }
}
