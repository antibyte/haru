<!-- Template for the release text. .github/workflows/release.yml replaces {{VERSION}} and appends GitHub's automatically generated changes. -->
**Haru** is a desktop app for a real-time conversation with the speech model Haru-4B: you speak into the microphone, Haru answers with voice, and every word appears as glowing ink in a fluid simulation. Everything runs locally with [speech.cpp](https://github.com/nyosegawa/speech.cpp).

## Download

| System | File | Backends |
|---|---|---|
| Windows 10/11, 64-bit | `Haru_{{VERSION}}_x64-setup.exe` | Vulkan (any graphics card with Vulkan 1.3, including NVIDIA) |
| Windows 10/11, 64-bit, NVIDIA | `Haru_{{VERSION}}_x64-cuda-setup.exe` (about 370 MB) | CUDA and Vulkan |
| Linux x64 | `Haru_{{VERSION}}_amd64.AppImage` (no installation needed) or `Haru_{{VERSION}}_amd64.deb` (Debian, Ubuntu) | Vulkan, CPU |
| macOS 13.3 or newer, Apple Silicon | `Haru_{{VERSION}}_aarch64.dmg` | Metal |

### Which Windows installer?

The regular installer works for every graphics card, including NVIDIA. The CUDA installer additionally bundles NVIDIA's CUDA runtime (cuBLAS), because the graphics driver only installs the driver interface (`nvcuda.dll`), not these libraries. That is why it is much larger. On NVIDIA cards, CUDA can be faster. The CUDA variant has not yet been tested on real NVIDIA hardware; if it fails, Haru switches to Vulkan automatically. Both installers install the same app; installing one over the other replaces the backends.

## Requirements

- **GPU with at least 8 GB of graphics memory** (Vulkan 1.3 or NVIDIA with CUDA). Without a suitable GPU, Haru runs on the CPU, far slower than real time. On a Mac the GPU shares system memory, where 16 GB is advisable.
- **Model download on first start: 6.6 GB** (about 7 GB of free space). An interrupted download resumes on the next start.
- Headphones sound best.

## Installers are not signed

On first launch, the operating system shows a warning:

- **Windows (SmartScreen):** "Windows protected your PC" → **More info** → **Run anyway**.
- **macOS (Gatekeeper):** Drag Haru from the dmg into "Applications", then run `xattr -dr com.apple.quarantine /Applications/Haru.app` in Terminal and open Haru. Alternatively, open it once, then go to **System Settings → Privacy & Security → Open Anyway**. Allow microphone access during the first conversation.
- **Linux (AppImage):** `chmod +x Haru_{{VERSION}}_amd64.AppImage`, then start it. The AppImage needs FUSE 2 (Ubuntu 24.04: `sudo apt install libfuse2t64`), and the Vulkan variant needs a Vulkan driver (Mesa or NVIDIA).

Checksums: `Haru_{{VERSION}}_SHA256SUMS.txt`.
