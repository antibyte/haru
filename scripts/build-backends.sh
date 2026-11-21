#!/usr/bin/env bash
# Builds speech.cpp backend variants into app/src-tauri/backends/<variant>/.
# Usage: scripts/build-backends.sh [variant...]   (variants: metal, vulkan, cuda, cpu)
# Default: metal on Darwin, "vulkan cpu" on Linux.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

OS="$(uname -s)"
if [ "$#" -gt 0 ]; then
  VARIANTS=("$@")
elif [ "$OS" = "Darwin" ]; then
  VARIANTS=(metal)
else
  VARIANTS=(vulkan cpu)
fi

for v in "${VARIANTS[@]}"; do
  case "$v" in
    metal|vulkan|cuda|cpu) ;;
    *) echo "Unknown variant '$v' (metal, vulkan, cuda, cpu)" >&2; exit 2 ;;
  esac
done

JOBS="$(nproc 2>/dev/null || sysctl -n hw.ncpu)"

git submodule update --init --recursive extern/speech.cpp

for v in "${VARIANTS[@]}"; do
  echo "=== Backend variant: $v ==="
  build_dir="build/speech-$v"
  flags=(
    -DCMAKE_BUILD_TYPE=Release -DGGML_NATIVE=OFF -DBUILD_SHARED_LIBS=OFF
    -DGGML_BUILD_TESTS=OFF -DGGML_BUILD_EXAMPLES=OFF
  )
  if [ "$OS" = "Linux" ]; then flags+=(-DGGML_OPENMP=OFF); fi
  case "$v" in
    metal)  flags+=(-DGGML_METAL_EMBED_LIBRARY=ON) ;;
    vulkan) flags+=(-DGGML_VULKAN=ON) ;;
    cuda)
      if [ -z "${CUDA_PATH:-}" ]; then
        echo "CUDA_PATH is not set. Install the CUDA 12 toolkit to build the 'cuda' variant." >&2
        exit 1
      fi
      flags+=(-DGGML_CUDA=ON)
      ;;
    cpu) ;;
  esac

  cmake -S extern/speech.cpp -B "$build_dir" "${flags[@]}"
  cmake --build "$build_dir" --target speech-cli -j "$JOBS"

  dest="app/src-tauri/backends/$v"
  mkdir -p "$dest"
  cp "$build_dir/speech" "$dest/speech"

  if [ "$v" = "cuda" ]; then
    for lib in libcudart.so.12 libcublas.so.12 libcublasLt.so.12; do
      if [ ! -e "$CUDA_PATH/lib64/$lib" ]; then
        echo "$lib not found in $CUDA_PATH/lib64" >&2
        exit 1
      fi
      cp -L "$CUDA_PATH/lib64/$lib" "$dest/"
    done
  fi

  echo "--- $dest/speech --version"
  "$dest/speech" --version
  echo "--- $dest/speech devices --json"
  "$dest/speech" devices --json
done
