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

# Apply patches/ggml to the pinned ggml submodule, then patches/speech.cpp to speech.cpp itself. The two sets touch
# different repositories, so their order matters for the log only; ggml goes first as the layer speech.cpp builds on.
# A patch already applied is skipped; one that neither applies nor is applied stops the build.
apply_patches() {
  local name="$1" folder="$2" repo="$3" patch
  for patch in "$ROOT/$folder"/*.patch; do
    if git -C "$repo" apply --reverse --check "$patch" >/dev/null 2>&1; then
      echo "$name patch already applied: $(basename "$patch")"
    else
      git -C "$repo" apply --check "$patch" || { echo "$name patch neither applies nor is applied: $patch" >&2; exit 1; }
      git -C "$repo" apply "$patch"
      echo "$name patch applied: $(basename "$patch")"
    fi
  done
}
shopt -s nullglob
apply_patches ggml patches/ggml extern/speech.cpp/ggml
apply_patches speech.cpp patches/speech.cpp extern/speech.cpp
shopt -u nullglob

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
  cmake --build "$build_dir" --target speech-cli worker-battle-check worker-inbox-check -j "$JOBS"

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
  # The battle's reading and lockstep, and the inbox's handling of its lines, which need no model (speech.cpp's
  # checks/worker/worker-battle-check.cpp and worker-inbox-check.cpp).
  for check in worker-battle-check worker-inbox-check; do
    echo "--- $build_dir/$check"
    "$build_dir/$check"
  done
done
