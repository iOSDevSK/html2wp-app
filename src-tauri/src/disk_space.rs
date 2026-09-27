//! Capacity for an explicit conversion on the volume holding project data.
//! macOS can reclaim purgeable space; df only reports immediately free blocks.
use std::path::Path;

const GB: u64 = 1_000_000_000;

pub struct Capacity {
    bytes: u64,
    reclaimable_included: bool,
}

impl Capacity {
    pub fn parallel_error(&self, required_gb: u64) -> Option<String> {
        if self.bytes >= required_gb.saturating_mul(GB) { return None; }
        let basis = if self.reclaimable_included {
            "macOS available capacity, including space the system can reclaim"
        } else { "immediately free space; reclaimable space could not be determined" };
        let tenths = self.bytes / (GB / 10);
        Some(format!("Only {}.{} GB of disk space is available on the volume storing your projects ({basis}). Another conversion needs at least {required_gb} GB available; wait for a running conversion to finish or free space.", tenths / 10, tenths % 10))
    }
}

pub fn available(path: &Path) -> Option<Capacity> {
    capacity_with(|| important_capacity(path), || immediate_capacity(path))
}

fn capacity_with(important: impl FnOnce() -> Option<u64>, immediate: impl FnOnce() -> Option<u64>) -> Option<Capacity> {
    match important() {
        Some(bytes) => Some(Capacity { bytes, reclaimable_included: true }),
        None => immediate().map(|bytes| Capacity { bytes, reclaimable_included: false }),
    }
}

#[cfg(target_os = "macos")]
fn important_capacity(path: &Path) -> Option<u64> {
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{NSNumber, NSString, NSURL, NSURLVolumeAvailableCapacityForImportantUsageKey};
    autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
        let mut value = None;
        // Foundation declares this key as a read-only NSNumber (macOS 10.13+).
        // The out slot owns the returned object; validate its class before use.
        unsafe { url.getResourceValue_forKey_error(&mut value, NSURLVolumeAvailableCapacityForImportantUsageKey) }.ok()?;
        let value = value?;
        u64::try_from(value.downcast_ref::<NSNumber>()?.longLongValue()).ok()
    })
}

#[cfg(not(target_os = "macos"))]
fn important_capacity(_: &Path) -> Option<u64> { None }

fn immediate_capacity(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        let out = std::process::Command::new("df").arg("-Pk").arg(path).output().ok()?;
        if !out.status.success() { return None; }
        parse_df(&String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(not(unix))]
    { let _ = path; None }
}

#[cfg(any(unix, test))]
fn parse_df(text: &str) -> Option<u64> {
    text.lines().nth(1)?.split_whitespace().nth(3)?.parse::<u64>().ok()?.checked_mul(1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_available_capacity_does_not_false_block_on_low_raw_free_space() {
        let capacity = capacity_with(|| Some(51_519_665_210), || panic!("do not replace macOS capacity with df" )).unwrap();
        assert!(capacity.parallel_error(15).is_none());
        let raw = capacity_with(|| None, || Some(13 * GB)).unwrap();
        assert!(raw.parallel_error(15).unwrap().contains("immediately free"));
    }

    #[test]
    fn low_capacity_is_still_blocked_and_threshold_uses_decimal_bytes() {
        for bytes in [0, 14_900_000_000, 15 * GB - 1] {
            assert!(capacity_with(|| Some(bytes), || Some(99 * GB)).unwrap().parallel_error(15).is_some());
        }
        assert!(capacity_with(|| Some(15 * GB - 1), || None).unwrap().parallel_error(15).unwrap().contains("14.9 GB"));
        assert!(capacity_with(|| Some(15 * GB), || None).unwrap().parallel_error(15).is_none());
        assert!(capacity_with(|| None, || None).is_none());
    }

    #[test]
    fn df_fallback_converts_blocks_to_bytes_and_rejects_malformed_output() {
        assert_eq!(parse_df("Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk 999 100 13000000 1% /volume with spaces\n"), Some(13_312_000_000));
        assert_eq!(parse_df("error"), None);
        assert_eq!(parse_df("header\n/dev/disk 1 2 -1"), None);
    }

    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "live macOS Foundation capacity probe"]
    fn native_capacity_is_readable_on_the_actual_project_volume() {
        let path = std::env::var("H2WP_CAPACITY_PATH").unwrap();
        let bytes = important_capacity(Path::new(&path)).expect("native volume capacity");
        println!("macOS important capacity: {bytes} bytes");
        assert!(available(Path::new(&path)).unwrap().reclaimable_included);
    }
}
