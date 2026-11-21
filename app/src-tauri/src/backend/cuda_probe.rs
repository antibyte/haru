//! CUDA driver probe via libloading (Task 1.3).

/// True when the NVIDIA driver library loads and reports at least one device.
pub fn probe_cuda() -> bool {
    probe_impl()
}

#[cfg(any(windows, target_os = "linux"))]
fn probe_impl() -> bool {
    #[cfg(windows)]
    const LIB: &str = "nvcuda.dll";
    #[cfg(not(windows))]
    const LIB: &str = "libcuda.so.1";

    // SAFETY: loading the vendor driver runs its initialisers; the symbols are called with the
    // documented CUDA driver API signatures and the library outlives the calls.
    unsafe {
        let lib = match libloading::Library::new(LIB) {
            Ok(l) => l,
            Err(_) => return false,
        };
        let init: libloading::Symbol<unsafe extern "C" fn(u32) -> i32> = match lib.get(b"cuInit\0") {
            Ok(s) => s,
            Err(_) => return false,
        };
        let count_fn: libloading::Symbol<unsafe extern "C" fn(*mut i32) -> i32> =
            match lib.get(b"cuDeviceGetCount\0") {
                Ok(s) => s,
                Err(_) => return false,
            };
        if init(0) != 0 {
            return false;
        }
        let mut count: i32 = 0;
        count_fn(&mut count) == 0 && count > 0
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn probe_impl() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_does_not_panic() {
        println!("probe_cuda() = {}", probe_cuda());
    }
}
