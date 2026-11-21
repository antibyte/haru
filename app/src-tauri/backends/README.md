# Backend variants

Each variant of `speech` (speech.cpp v0.11.1) lives in its own folder, which is gitignored:

```
backends/
  vulkan/speech.exe (+ DLLs)
  cuda/speech.exe   (+ DLLs)
  cpu/speech.exe
  metal/speech
```

Build them with `scripts/build-backends.ps1` (Windows) or `scripts/build-backends.sh` (POSIX).
At runtime the app looks for `HARU_BACKENDS_DIR`, then the bundled resource folder, then (debug builds only) this folder.
