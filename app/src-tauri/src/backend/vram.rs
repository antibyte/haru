//! System-wide dedicated VRAM of a GPU on Windows.
//!
//! The Vulkan memory budget is per process, so `speech devices --json` (a separate probe process) never
//! sees VRAM used by the worker or other apps. Windows' `\GPU Adapter Memory(*)\Dedicated Usage` counter is
//! system-wide; DXGI gives the adapter's total dedicated memory and its LUID to pick the right instances.

/// PDH instance prefix of an adapter: `luid_0x{high:08X}_0x{low:08X}_` (compared case-insensitively).
pub fn luid_prefix(high: i32, low: u32) -> String {
    format!("luid_0x{:08X}_0x{:08X}_", high as u32, low)
}

/// Sum of all counter instances (`luid_..._phys_N`) belonging to the adapter with the given prefix.
pub fn sum_usage<'a>(instances: impl IntoIterator<Item = (&'a str, u64)>, prefix: &str) -> u64 {
    let prefix = prefix.to_ascii_lowercase();
    instances
        .into_iter()
        .filter(|(name, _)| name.to_ascii_lowercase().starts_with(&prefix))
        .fold(0u64, |acc, (_, v)| acc.saturating_add(v))
}

/// A DXGI adapter as far as matching needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adapter {
    pub description: String,
    pub dedicated: u64,
    pub luid_high: i32,
    pub luid_low: u32,
    pub software: bool,
}

/// Picks the adapter for a device description: trimmed case-insensitive equality first, then containment
/// either way. Software adapters are skipped; among several matches the largest dedicated memory wins.
pub fn match_adapter<'a>(adapters: &'a [Adapter], description: &str) -> Option<&'a Adapter> {
    let want = description.trim().to_lowercase();
    if want.is_empty() {
        return None;
    }
    let hw = || adapters.iter().filter(|a| !a.software);
    let exact = hw().filter(|a| a.description.trim().to_lowercase() == want).max_by_key(|a| a.dedicated);
    if exact.is_some() {
        return exact;
    }
    hw().filter(|a| {
        let d = a.description.trim().to_lowercase();
        !d.is_empty() && (d.contains(&want) || want.contains(&d))
    })
    .max_by_key(|a| a.dedicated)
}

/// free = total - used, never below zero.
pub fn free_bytes(total: u64, used: u64) -> u64 {
    total.saturating_sub(used)
}

/// Free dedicated VRAM of the GPU with this description, system-wide. `None` on any failure.
#[cfg(windows)]
pub fn system_free(description: &str) -> Option<u64> {
    let usage = win::dedicated_usage()?;
    // Virtual adapters (e.g. remote-display sessions) can repeat the real GPU's description with a LUID the
    // counter does not know; only adapters that have PDH instances are candidates.
    let adapters: Vec<Adapter> = win::adapters()?
        .into_iter()
        .filter(|a| {
            let p = luid_prefix(a.luid_high, a.luid_low).to_ascii_lowercase();
            usage.iter().any(|(n, _)| n.to_ascii_lowercase().starts_with(&p))
        })
        .collect();
    let a = match_adapter(&adapters, description)?;
    let used = sum_usage(usage.iter().map(|(n, v)| (n.as_str(), *v)), &luid_prefix(a.luid_high, a.luid_low));
    Some(free_bytes(a.dedicated, used))
}

#[cfg(not(windows))]
pub fn system_free(_description: &str) -> Option<u64> {
    None
}

#[cfg(windows)]
mod win {
    use super::Adapter;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
        PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    };

    pub fn adapters() -> Option<Vec<Adapter>> {
        // SAFETY: plain DXGI enumeration; COM objects are released by their wrappers.
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            let mut out = Vec::new();
            let mut i = 0;
            while let Ok(adapter) = factory.EnumAdapters1(i) {
                i += 1;
                let Ok(d) = adapter.GetDesc1() else { continue };
                let len = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
                out.push(Adapter {
                    description: String::from_utf16_lossy(&d.Description[..len]),
                    dedicated: d.DedicatedVideoMemory as u64,
                    luid_high: d.AdapterLuid.HighPart,
                    luid_low: d.AdapterLuid.LowPart,
                    software: d.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0,
                });
            }
            Some(out)
        }
    }

    /// (instance name, bytes) for every `GPU Adapter Memory` instance.
    pub fn dedicated_usage() -> Option<Vec<(String, u64)>> {
        // SAFETY: the query handle is closed on every path; buffers are sized by PDH's own report.
        unsafe {
            let mut query = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return None;
            }
            let r = read_counters(query);
            PdhCloseQuery(query);
            r
        }
    }

    unsafe fn read_counters(query: PDH_HQUERY) -> Option<Vec<(String, u64)>> {
        let mut counter = PDH_HCOUNTER::default();
        if PdhAddEnglishCounterW(query, w!("\\GPU Adapter Memory(*)\\Dedicated Usage"), 0, &mut counter) != 0 {
            return None;
        }
        if PdhCollectQueryData(query) != 0 {
            return None;
        }
        let mut size = 0u32;
        let mut count = 0u32;
        let st = PdhGetFormattedCounterArrayW(counter, PDH_FMT_LARGE, &mut size, &mut count, None);
        if st != PDH_MORE_DATA || size == 0 {
            return None;
        }
        // u64 backing keeps the buffer 8-byte aligned for the item structs.
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        let st = PdhGetFormattedCounterArrayW(counter, PDH_FMT_LARGE, &mut size, &mut count, Some(items));
        if st != 0 {
            return None;
        }
        let mut out = Vec::with_capacity(count as usize);
        for k in 0..count as usize {
            let it = &*items.add(k);
            if it.FmtValue.CStatus != 0 || it.szName.is_null() {
                continue;
            }
            let Ok(name) = it.szName.to_string() else { continue };
            out.push((name, it.FmtValue.Anonymous.largeValue.max(0) as u64));
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ad(desc: &str, dedicated: u64, software: bool) -> Adapter {
        Adapter { description: desc.into(), dedicated, luid_high: 0, luid_low: 1, software }
    }

    #[test]
    fn prefix_is_zero_padded_uppercase_hex() {
        assert_eq!(luid_prefix(0, 0xD3CA), "luid_0x00000000_0x0000D3CA_");
        assert_eq!(luid_prefix(-1, 0xABCDEF12), "luid_0xFFFFFFFF_0xABCDEF12_");
    }

    #[test]
    fn sums_all_instances_of_the_adapter_case_insensitively() {
        let inst = [
            ("luid_0x00000000_0x0000D3CA_phys_0", 100u64),
            ("LUID_0x00000000_0x0000d3ca_phys_1", 50),
            ("luid_0x00000000_0x0000D3CB_phys_0", 7),
        ];
        let p = luid_prefix(0, 0xD3CA);
        assert_eq!(sum_usage(inst.iter().map(|(n, v)| (*n, *v)), &p), 150);
        assert_eq!(sum_usage(std::iter::empty(), &p), 0);
    }

    #[test]
    fn matches_exact_before_containment() {
        let list = [
            ad("Intel(R) Arc(TM) B580 Graphics", 12, false),
            ad("Intel(R) Arc(TM) B580 Graphics Extra", 20, false),
        ];
        let m = match_adapter(&list, "  intel(r) arc(tm) b580 graphics ").unwrap();
        assert_eq!(m.dedicated, 12);
    }

    #[test]
    fn matches_by_containment_either_way_and_picks_largest() {
        let list = [ad("NVIDIA GeForce RTX 4070", 5, false), ad("NVIDIA GeForce RTX 4070 Ti", 9, false)];
        assert_eq!(match_adapter(&list, "GeForce RTX 4070").unwrap().dedicated, 9);
        let list = [ad("Arc B580", 3, false)];
        assert_eq!(match_adapter(&list, "Intel Arc B580 Graphics").unwrap().dedicated, 3);
    }

    #[test]
    fn skips_software_adapters_and_misses() {
        let list = [ad("Microsoft Basic Render Driver", 100, true), ad("Other", 1, false)];
        assert!(match_adapter(&list, "Microsoft Basic Render Driver").is_none());
        assert!(match_adapter(&list, "").is_none());
        assert!(match_adapter(&list, "AMD").is_none());
    }

    #[test]
    fn free_saturates() {
        assert_eq!(free_bytes(10, 4), 6);
        assert_eq!(free_bytes(4, 10), 0);
    }

    /// Sanity check on a real machine: `cargo test vram -- --ignored --nocapture system_free_prints`.
    /// Set HARU_VRAM_DESC to the device description (default: Intel Arc B580).
    #[test]
    #[ignore]
    fn system_free_prints() {
        let d = std::env::var("HARU_VRAM_DESC").unwrap_or_else(|_| "Intel(R) Arc(TM) B580 Graphics".into());
        #[cfg(windows)]
        {
            let a = win::adapters().unwrap();
            let m = match_adapter(&a, &d).unwrap();
            let usage = win::dedicated_usage().unwrap();
            println!("adapter {:?}", m); println!("all {:?}", a);
            for (n, v) in &usage {
                println!("  {n} = {v}");
            }
        }
        println!("system_free({d}) = {:?}", system_free(&d));
    }
}
