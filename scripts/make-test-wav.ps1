# Generates a 24 kHz mono 16-bit test question with System.Speech.
# Run with Windows PowerShell 5.1 (System.Speech is missing in PowerShell 7):
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\make-test-wav.ps1
param(
    [string]$Out = "build\test\question-de.wav",
    [string]$Text = "Hallo Haru! Wie geht es dir heute? Erzähl mir bitte etwas über dich.",
    [string]$Culture = "de-DE"
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Speech

$full = if ([System.IO.Path]::IsPathRooted($Out)) { $Out } else { Join-Path (Get-Location) $Out }
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $full) | Out-Null

$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
try {
    $voice = $synth.GetInstalledVoices() | Where-Object { $_.Enabled -and $_.VoiceInfo.Culture.Name -eq $Culture } | Select-Object -First 1
    if ($voice) {
        $synth.SelectVoice($voice.VoiceInfo.Name)
    } else {
        Write-Warning "No installed voice for culture $Culture. Using the default voice; use English text and --language en instead."
    }
    $fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo 24000, ([System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen), ([System.Speech.AudioFormat.AudioChannel]::Mono)
    $synth.SetOutputToWaveFile($full, $fmt)
    $synth.Speak($Text)
} finally {
    $synth.Dispose()
}
Write-Host "Wrote $full"
