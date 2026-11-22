<!-- Vorlage für den Release-Text. .github/workflows/release.yml ersetzt {{VERSION}} und hängt GitHubs automatisch erzeugte Änderungen an. -->
**Haru** ist eine Desktop-App für ein Gespräch in Echtzeit mit dem Sprachmodell Haru-4B: Du sprichst ins Mikrofon, Haru antwortet mit Stimme, und jedes Wort erscheint als glühende Tinte in einer Fluidsimulation. Alles läuft lokal mit [speech.cpp](https://github.com/nyosegawa/speech.cpp).

## Download

| System | Datei | Backends |
|---|---|---|
| Windows 10/11, 64 Bit | `Haru_{{VERSION}}_x64-setup.exe` | Vulkan, CUDA |
| Linux x64 | `Haru_{{VERSION}}_amd64.AppImage` (ohne Installation) oder `Haru_{{VERSION}}_amd64.deb` (Debian, Ubuntu) | Vulkan, CPU |
| macOS 13.3 oder neuer, Apple Silicon | `Haru_{{VERSION}}_aarch64.dmg` | Metal |

## Voraussetzungen

- **GPU mit mindestens 8 GB Grafikspeicher** (Vulkan 1.3 oder NVIDIA mit CUDA). Ohne passende GPU läuft Haru auf der CPU, deutlich langsamer als Echtzeit. Auf dem Mac teilt sich die GPU den Arbeitsspeicher, dort sind 16 GB sinnvoll.
- **Modell-Download beim ersten Start: 6,6 GB** (etwa 7 GB freier Speicher). Ein abgebrochener Download setzt beim nächsten Start fort.
- Kopfhörer klingen am besten.

## Installer sind nicht signiert

Beim ersten Öffnen warnt das Betriebssystem:

- **Windows (SmartScreen):** „Der Computer wurde durch Windows geschützt“ → **Weitere Informationen** → **Trotzdem ausführen**.
- **macOS (Gatekeeper):** Haru aus dem dmg in „Programme“ ziehen, dann im Terminal `xattr -dr com.apple.quarantine /Applications/Haru.app` ausführen und Haru öffnen. Alternativ einmal öffnen, dann **Systemeinstellungen → Datenschutz & Sicherheit → Dennoch öffnen**. Den Mikrofonzugriff beim ersten Gespräch erlauben.
- **Linux (AppImage):** `chmod +x Haru_{{VERSION}}_amd64.AppImage`, dann starten. Das AppImage braucht FUSE 2 (Ubuntu 24.04: `sudo apt install libfuse2t64`), die Vulkan-Variante einen Vulkan-Treiber (Mesa oder NVIDIA).

Prüfsummen: `Haru_{{VERSION}}_SHA256SUMS.txt`.
