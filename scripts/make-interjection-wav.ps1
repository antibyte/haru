# Renders a 60 s interjection test WAV (24 kHz mono 16-bit) with Windows SAPI (System.Speech):
#   0 s  "Erzähl mir bitte ganz ausführlich, was du von den Menschen hältst, und hör nicht auf."
#   20 s "Stopp! Hör sofort auf zu reden. Ich will jetzt etwas sagen."
#   32 s "Hallo? Hörst du mich? Sei bitte still!"
# silence in between and until 60 s. Prints the RMS per second so the placement can be checked.
# Run with Windows PowerShell 5.1 (System.Speech is missing in PowerShell 7):
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\make-interjection-wav.ps1 [-Out FILE]
param(
    [string]$Out = "build\test\interject-60s.wav",
    [string]$Culture = "de-DE",
    [int]$TotalSeconds = 60
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Speech

$full = if ([System.IO.Path]::IsPathRooted($Out)) { $Out } else { Join-Path (Get-Location) $Out }
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $full) | Out-Null

$rate = 24000
$parts = @(
    @{ At = 0;  Text = "Erzähl mir bitte ganz ausführlich, was du von den Menschen hältst, und hör nicht auf." },
    @{ At = 20; Text = "Stopp! Hör sofort auf zu reden. Ich will jetzt etwas sagen." },
    @{ At = 32; Text = "Hallo? Hörst du mich? Sei bitte still!" }
)

$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
$mix = New-Object 'int16[]' ($TotalSeconds * $rate)
try {
    $installed = @($synth.GetInstalledVoices() | Where-Object { $_.Enabled })
    Write-Host ("Installed voices: " + (($installed | ForEach-Object { $_.VoiceInfo.Name + " (" + $_.VoiceInfo.Culture.Name + ")" }) -join ", "))
    $voice = $installed | Where-Object { $_.VoiceInfo.Culture.Name -eq $Culture } | Select-Object -First 1
    if ($voice) {
        $synth.SelectVoice($voice.VoiceInfo.Name)
        Write-Host "Using German voice: $($voice.VoiceInfo.Name)"
    } else {
        Write-Warning "No installed voice for culture $Culture. Falling back to the default voice ($($synth.Voice.Name), $($synth.Voice.Culture.Name)); the German text will sound wrong."
    }
    $fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo $rate, ([System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen), ([System.Speech.AudioFormat.AudioChannel]::Mono)
    foreach ($p in $parts) {
        $tmp = [System.IO.Path]::GetTempFileName() + ".wav"
        try {
            $synth.SetOutputToWaveFile($tmp, $fmt)
            $synth.Speak($p.Text)
            $synth.SetOutputToNull()
            $bytes = [System.IO.File]::ReadAllBytes($tmp)
        } finally {
            Remove-Item $tmp -ErrorAction SilentlyContinue
        }
        # Canonical 44-byte header written by SAPI; locate the "data" chunk to be safe.
        $pos = 12
        $dataStart = -1
        while ($pos + 8 -le $bytes.Length) {
            $id = [System.Text.Encoding]::ASCII.GetString($bytes, $pos, 4)
            $len = [BitConverter]::ToInt32($bytes, $pos + 4)
            if ($id -eq "data") { $dataStart = $pos + 8; break }
            $pos += 8 + $len
        }
        if ($dataStart -lt 0) { throw "no data chunk in SAPI output" }
        $n = [int](($bytes.Length - $dataStart) / 2)
        $off = $p.At * $rate
        Write-Host ("Rendered at {0,2} s: {1:N1} s of speech" -f $p.At, ($n / $rate))
        if ($off + $n -gt $mix.Length) { throw "part at $($p.At) s does not fit into $TotalSeconds s" }
        for ($i = 0; $i -lt $n; $i++) { $mix[$off + $i] = [BitConverter]::ToInt16($bytes, $dataStart + 2 * $i) }
        if ($p.At -lt 32 -and $off + $n -gt 32 * $rate) { Write-Warning "Part at $($p.At) s overlaps the next one." }
    }
} finally {
    $synth.Dispose()
}

# Write the WAV (44-byte PCM header).
$ms = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter $ms
$dataLen = $mix.Length * 2
$bw.Write([System.Text.Encoding]::ASCII.GetBytes("RIFF")); $bw.Write([int](36 + $dataLen))
$bw.Write([System.Text.Encoding]::ASCII.GetBytes("WAVEfmt ")); $bw.Write([int]16)
$bw.Write([int16]1); $bw.Write([int16]1); $bw.Write([int]$rate); $bw.Write([int]($rate * 2)); $bw.Write([int16]2); $bw.Write([int16]16)
$bw.Write([System.Text.Encoding]::ASCII.GetBytes("data")); $bw.Write([int]$dataLen)
$raw = New-Object 'byte[]' $dataLen
[Buffer]::BlockCopy($mix, 0, $raw, 0, $dataLen)
$bw.Write($raw)
$bw.Flush()
[System.IO.File]::WriteAllBytes($full, $ms.ToArray())
Write-Host "Wrote $full ($($mix.Length / $rate) s, $rate Hz mono 16-bit)"

# Verification: RMS per second.
Write-Host "RMS per second:"
$line = @()
for ($s = 0; $s -lt $TotalSeconds; $s++) {
    $sum = 0.0
    for ($i = 0; $i -lt $rate; $i++) { $v = $mix[$s * $rate + $i] / 32768.0; $sum += $v * $v }
    $r = [Math]::Sqrt($sum / $rate)
    $line += ("{0,2}:{1:N3}" -f $s, $r)
    if (($s + 1) % 10 -eq 0) { Write-Host ($line -join "  "); $line = @() }
}
