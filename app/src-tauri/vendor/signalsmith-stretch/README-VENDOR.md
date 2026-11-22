Vendored from crates.io `signalsmith-stretch` 0.1.3 (MIT, Colin Marc) which bundles the MIT-licensed
Signalsmith Stretch and Signalsmith Linear C++ libraries (Geraint Luff / Signalsmith Audio).
Changes: build.rs without bindgen (libclang is not required), hand-written `sys` FFI module in
src/lib.rs, no `dasp` dependency, tests/web/cmd directories stripped. Licences: LICENSE-wrapper.md,
signalsmith-stretch/LICENSE.txt, signalsmith-linear/LICENSE.txt.
