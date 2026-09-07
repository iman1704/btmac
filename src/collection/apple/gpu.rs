// Apple Silicon GPU sampler — IOReport GPU Performance States + pmgr voltage-states9

use std::{ffi::CString, os::raw::c_void, ptr::null, time::Instant};

use core_foundation::{
    array::{
        CFArrayAppendValue, CFArrayCreateMutable, CFArrayGetCount, CFArrayGetValueAtIndex,
        CFArrayRef, CFMutableArrayRef, kCFTypeArrayCallBacks,
    },
    base::{
        CFAllocatorRef, CFRelease, CFTypeRef, kCFAllocatorDefault, kCFAllocatorNull, mach_port_t,
    },
    data::{CFDataGetBytes, CFDataGetLength, CFDataRef},
    dictionary::{
        CFDictionaryCreateMutableCopy, CFDictionaryGetCount, CFDictionaryGetValue, CFDictionaryRef,
        CFDictionarySetValue, CFMutableDictionaryRef,
    },
    string::{
        CFStringCreateWithBytesNoCopy, CFStringGetCString, CFStringRef, kCFStringEncodingUTF8,
    },
};
use mach2::kern_return::kern_return_t;

use anyhow::anyhow;

use crate::collection::error::{CollectionError, CollectionResult};

// ── FFI ────────────────────────────────────────────────────────────────────
#[allow(non_camel_case_types)]
type io_object_t = mach_port_t;
#[allow(non_camel_case_types)]
type io_iterator_t = io_object_t;
#[allow(non_camel_case_types)]
type io_registry_entry_t = io_object_t;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const i8) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingServices(
        mainPort: mach_port_t, matching: CFMutableDictionaryRef, existing: *mut io_iterator_t,
    ) -> kern_return_t;
    fn IOIteratorNext(iterator: io_iterator_t) -> io_object_t;
    fn IORegistryEntryGetName(entry: io_registry_entry_t, name: *mut i8) -> kern_return_t;
    fn IORegistryEntryCreateCFProperties(
        entry: io_registry_entry_t, properties: *mut CFMutableDictionaryRef,
        allocator: CFAllocatorRef, options: u32,
    ) -> kern_return_t;
    fn IOObjectRelease(obj: io_object_t) -> kern_return_t;
}

#[repr(C)]
struct IOReportSubscription {
    _data: [u8; 0],
    _marker: std::marker::PhantomData<(*mut u8, std::marker::PhantomPinned)>,
}
type IOReportSubRef = *const IOReportSubscription;
type CVoidRef = *const c_void;

#[link(name = "IOReport", kind = "dylib")]
unsafe extern "C" {
    fn IOReportCopyAllChannels(a: u64, b: u64) -> CFDictionaryRef;
    fn IOReportCreateSubscription(
        a: CVoidRef, b: CFMutableDictionaryRef, c: *mut CFMutableDictionaryRef, d: u64,
        b2: CFTypeRef,
    ) -> IOReportSubRef;
    fn IOReportCreateSamples(
        a: IOReportSubRef, b: CFMutableDictionaryRef, c: CFTypeRef,
    ) -> CFDictionaryRef;
    fn IOReportCreateSamplesDelta(
        a: CFDictionaryRef, b: CFDictionaryRef, c: CFTypeRef,
    ) -> CFDictionaryRef;
    fn IOReportChannelGetGroup(a: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetSubGroup(a: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetChannelName(a: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetUnitLabel(a: CFDictionaryRef) -> CFStringRef;
    fn IOReportStateGetCount(a: CFDictionaryRef) -> i32;
    fn IOReportStateGetNameForIndex(a: CFDictionaryRef, b: i32) -> CFStringRef;
    fn IOReportStateGetResidency(a: CFDictionaryRef, b: i32) -> i64;
}

// ── CF helpers ─────────────────────────────────────────────────────────────
fn cfstr(val: &str) -> CFStringRef {
    unsafe {
        CFStringCreateWithBytesNoCopy(
            kCFAllocatorDefault,
            val.as_ptr(),
            val.len() as isize,
            kCFStringEncodingUTF8,
            0,
            kCFAllocatorNull,
        )
    }
}

fn from_cfstr(val: CFStringRef) -> String {
    unsafe {
        let mut buf = vec![0u8; 128];
        if CFStringGetCString(val, buf.as_mut_ptr() as *mut i8, 128, kCFStringEncodingUTF8) == 0 {
            return String::new();
        }
        std::ffi::CStr::from_ptr(buf.as_ptr() as *const i8)
            .to_string_lossy()
            .into_owned()
    }
}

fn cfdict_get_val(dict: CFDictionaryRef, key: &str) -> Option<CFTypeRef> {
    unsafe {
        let k = cfstr(key);
        let v = CFDictionaryGetValue(dict, k as *const c_void);
        CFRelease(k as *const c_void);
        if v.is_null() { None } else { Some(v) }
    }
}

fn cfio_get_residencies(item: CFDictionaryRef) -> Vec<(String, i64)> {
    let count = unsafe { IOReportStateGetCount(item) };
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let name_ref = unsafe { IOReportStateGetNameForIndex(item, i) };
        let val = unsafe { IOReportStateGetResidency(item, i) };
        let name = if name_ref.is_null() {
            std::format!("S{i}")
        } else {
            from_cfstr(name_ref)
        };
        out.push((name, val));
    }
    out
}

fn cfio_get_props(entry: io_registry_entry_t, name: &str) -> CollectionResult<CFDictionaryRef> {
    unsafe {
        let mut props: std::mem::MaybeUninit<CFMutableDictionaryRef> =
            std::mem::MaybeUninit::uninit();
        let ret =
            IORegistryEntryCreateCFProperties(entry, props.as_mut_ptr(), kCFAllocatorDefault, 0);
        if ret != 0 {
            return Err(CollectionError::General(anyhow!(
                "IORegistryEntryCreateCFProperties failed for {name}"
            )));
        }
        Ok(props.assume_init() as CFDictionaryRef)
    }
}

fn get_dvfs_mhz(dict: CFDictionaryRef, key: &str) -> Option<(Vec<u32>, Vec<u32>)> {
    unsafe {
        let obj = cfdict_get_val(dict, key)? as CFDataRef;
        let len = CFDataGetLength(obj);
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        CFDataGetBytes(
            obj,
            core_foundation::base::CFRange::init(0, len),
            buf.as_mut_ptr(),
        );
        let count = (len / 8) as usize;
        let mut freqs = vec![0u32; count];
        let mut volts = vec![0u32; count];
        for (i, chunk) in buf.chunks_exact(8).enumerate() {
            freqs[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            volts[i] = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        }
        Some((volts, freqs))
    }
}

fn zero_div<T>(a: T, b: T) -> T
where
    T: std::ops::Div<Output = T> + Default + PartialEq,
{
    let z = T::default();
    if b == z { z } else { a / b }
}

fn calc_freq_from_residencies(items: &[(String, i64)], freqs: &[u32]) -> (u32, f32, f32) {
    let offset = items
        .iter()
        .position(|(n, _)| n != "IDLE" && n != "DOWN" && n != "OFF")
        .unwrap_or(0);
    let usage: f64 = items
        .iter()
        .skip(offset)
        .take(freqs.len())
        .map(|(_, v)| *v as f64)
        .sum();
    let total: f64 = items.iter().map(|(_, v)| *v as f64).sum();
    let mut avg = 0f64;
    for i in 0..freqs.len() {
        let residency = items.get(i + offset).map(|(_, v)| *v as f64).unwrap_or(0.0);
        avg += zero_div(residency, usage) * freqs[i] as f64;
    }
    let active = zero_div(usage, total);
    let min_freq = freqs.first().copied().unwrap_or(0) as f64;
    let max_freq = freqs.last().copied().unwrap_or(1) as f64;
    let scaled = if max_freq == 0.0 {
        0.0
    } else {
        (avg.max(min_freq) * active) / max_freq
    };
    (avg as u32, scaled as f32, active as f32)
}

// ── SoC: gpu freqs from pmgr (always Hz -> MHz) ───────────────────────────
fn load_gpu_freqs() -> CollectionResult<Vec<u32>> {
    let service = CString::new("AppleARMIODevice")
        .map_err(|_| CollectionError::from("CString AppleARMIODevice"))?;
    let mut iter: io_iterator_t = 0;
    unsafe {
        let matching = IOServiceMatching(service.as_ptr());
        if IOServiceGetMatchingServices(0, matching, &mut iter) != 0 {
            return Err(CollectionError::from(
                "IOServiceGetMatchingServices AppleARMIODevice failed",
            ));
        }
    }
    let mut result: Option<Vec<u32>> = None;
    loop {
        let entry = unsafe { IOIteratorNext(iter) };
        if entry == 0 {
            break;
        }
        let mut name_buf = [0i8; 128];
        let name_ok = unsafe { IORegistryEntryGetName(entry, name_buf.as_mut_ptr()) } == 0;
        let name = if name_ok {
            unsafe {
                std::ffi::CStr::from_ptr(name_buf.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            }
        } else {
            String::new()
        };
        if name == "pmgr" {
            if let Ok(dict) = cfio_get_props(entry, &name) {
                if let Some((_, freqs)) = get_dvfs_mhz(dict, "voltage-states9") {
                    // GPU always Hz -> MHz per macmon §2.2
                    let mhz: Vec<u32> = freqs.into_iter().map(|v| v / 1_000_000).collect();
                    if !mhz.is_empty() {
                        result = Some(mhz);
                    }
                }
                unsafe { CFRelease(dict as *const c_void) };
            }
        }
        unsafe {
            IOObjectRelease(entry);
        }
        if result.is_some() {
            break;
        }
    }
    unsafe {
        IOObjectRelease(iter);
    }
    result.ok_or_else(|| CollectionError::from("No GPU frequencies (voltage-states9)"))
}

// ── IOReport (GPU only) ────────────────────────────────────────────────────
#[derive(Debug)]
struct IOReport {
    sub: IOReportSubRef,
    chan: CFMutableDictionaryRef,
    src: Option<CFDictionaryRef>,
    sel: Option<CFMutableArrayRef>,
    meta: Vec<(String, String, String, String)>,
    prev: Option<(CFDictionaryRef, Instant)>,
}

impl IOReport {
    fn with_gpu_filter() -> CollectionResult<Self> {
        let all = unsafe { IOReportCopyAllChannels(0, 0) };
        if all.is_null() {
            return Err(CollectionError::from("IOReportCopyAllChannels failed"));
        }
        let count = unsafe { CFDictionaryGetCount(all) };
        let chan = unsafe { CFDictionaryCreateMutableCopy(kCFAllocatorDefault, count, all) };
        if chan.is_null() {
            unsafe { CFRelease(all as *const c_void) };
            return Err(CollectionError::from(
                "CFDictionaryCreateMutableCopy failed",
            ));
        }
        // filter: GPU Stats / GPU Performance States
        let arr = cfdict_get_val(all, "IOReportChannels")
            .ok_or_else(|| CollectionError::from("IOReportChannels missing"))?
            as CFArrayRef;
        let n = unsafe { CFArrayGetCount(arr) };
        let sel = unsafe { CFArrayCreateMutable(kCFAllocatorDefault, n, &kCFTypeArrayCallBacks) };
        if sel.is_null() {
            unsafe {
                CFRelease(chan as *const c_void);
                CFRelease(all as *const c_void);
            }
            return Err(CollectionError::from("CFArrayCreateMutable failed"));
        }
        for i in 0..n {
            let item = unsafe { CFArrayGetValueAtIndex(arr, i) } as CFDictionaryRef;
            let group = {
                let r = unsafe { IOReportChannelGetGroup(item) };
                if r.is_null() {
                    String::new()
                } else {
                    from_cfstr(r)
                }
            };
            let subgroup = {
                let r = unsafe { IOReportChannelGetSubGroup(item) };
                if r.is_null() {
                    String::new()
                } else {
                    from_cfstr(r)
                }
            };
            if group == "GPU Stats" && subgroup == "GPU Performance States" {
                unsafe { CFArrayAppendValue(sel, item as *const c_void) };
            }
        }
        let key = cfstr("IOReportChannels");
        unsafe {
            CFDictionarySetValue(chan, key as *const c_void, sel as *const c_void);
            CFRelease(key as *const c_void);
        }
        // metadata from filtered chan
        let mut meta = Vec::new();
        if let Some(filtered) = cfdict_get_val(chan as CFDictionaryRef, "IOReportChannels") {
            let farr = filtered as CFArrayRef;
            let fc = unsafe { CFArrayGetCount(farr) };
            for i in 0..fc {
                let item = unsafe { CFArrayGetValueAtIndex(farr, i) } as CFDictionaryRef;
                let g = {
                    let r = unsafe { IOReportChannelGetGroup(item) };
                    if r.is_null() {
                        String::new()
                    } else {
                        from_cfstr(r)
                    }
                };
                let sg = {
                    let r = unsafe { IOReportChannelGetSubGroup(item) };
                    if r.is_null() {
                        String::new()
                    } else {
                        from_cfstr(r)
                    }
                };
                let ch = {
                    let r = unsafe { IOReportChannelGetChannelName(item) };
                    if r.is_null() {
                        String::new()
                    } else {
                        from_cfstr(r)
                    }
                };
                let un = {
                    let r = unsafe { IOReportChannelGetUnitLabel(item) };
                    if r.is_null() {
                        String::new()
                    } else {
                        from_cfstr(r).trim().to_string()
                    }
                };
                meta.push((g, sg, ch, un));
            }
        }
        if meta.is_empty() {
            unsafe {
                CFRelease(sel as *const c_void);
                CFRelease(chan as *const c_void);
                CFRelease(all as *const c_void);
            }
            return Err(CollectionError::from(
                "No GPU Performance States channels found",
            ));
        }
        let mut sub_out: std::mem::MaybeUninit<CFMutableDictionaryRef> =
            std::mem::MaybeUninit::uninit();
        let sub =
            unsafe { IOReportCreateSubscription(null(), chan, sub_out.as_mut_ptr(), 0, null()) };
        if sub.is_null() {
            unsafe {
                CFRelease(sel as *const c_void);
                CFRelease(chan as *const c_void);
                CFRelease(all as *const c_void);
            }
            return Err(CollectionError::from("IOReportCreateSubscription failed"));
        }
        Ok(Self {
            sub,
            chan,
            src: Some(all),
            sel: Some(sel),
            meta,
            prev: None,
        })
    }

    fn raw_sample(&self) -> (CFDictionaryRef, Instant) {
        let s = unsafe { IOReportCreateSamples(self.sub, self.chan, null()) };
        (s, Instant::now())
    }
}

impl Drop for IOReport {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.chan as *const c_void);
            CFRelease(self.sub as *const c_void);
            if let Some(s) = self.sel {
                CFRelease(s as *const c_void);
            }
            if let Some(s) = self.src {
                CFRelease(s as *const c_void);
            }
            if let Some((p, _)) = self.prev {
                CFRelease(p as *const c_void);
            }
        }
    }
}

// ── Public sampler ───────────────────────────────────
#[derive(Debug)]
pub struct AppleGpuSampler {
    gpu_freqs: Vec<u32>,
    ior: IOReport,
}

impl AppleGpuSampler {
    pub fn new() -> CollectionResult<Self> {
        let gpu_freqs = load_gpu_freqs()?;
        if gpu_freqs.len() < 2 {
            return Err(CollectionError::from("gpu freq table too short"));
        }
        let ior = IOReport::with_gpu_filter()?;
        Ok(Self { gpu_freqs, ior })
    }

    /// Returns GPU utilisation as `0.0..100.0`. First call primes the baseline and returns `None`.
    /// Subsequent calls compute `scaled_ratio*100` from `GPUPH` residency (§4.1, `gpu_freqs[1..]`).
    pub fn get_gpu_usage(&mut self) -> Option<f32> {
        let cur = self.ior.raw_sample();
        let prev = self.ior.prev.take();
        if let Some((prev_dict, _prev_time)) = prev {
            let delta = unsafe { IOReportCreateSamplesDelta(prev_dict, cur.0, null()) };
            unsafe { CFRelease(prev_dict as *const c_void) };
            if delta.is_null() {
                // store cur for next attempt, no data this tick
                self.ior.prev = Some(cur);
                return None;
            }
            // walk delta channels; metadata order matches delta array
            let arr = cfdict_get_val(delta, "IOReportChannels").map(|v| v as CFArrayRef);
            let mut result: Option<f32> = None;
            if let Some(arr) = arr {
                let n = unsafe { CFArrayGetCount(arr) } as usize;
                for i in 0..n {
                    if i >= self.ior.meta.len() {
                        break;
                    }
                    let (ref g, ref sg, ref ch, _) = self.ior.meta[i];
                    if g == "GPU Stats" && sg == "GPU Performance States" && ch == "GPUPH" {
                        let item =
                            unsafe { CFArrayGetValueAtIndex(arr, i as isize) } as CFDictionaryRef;
                        let residencies = cfio_get_residencies(item);
                        // macmon uses gpu_freqs[1..] (skip first entry)
                        let freq_slice = if self.gpu_freqs.len() > 1 {
                            &self.gpu_freqs[1..]
                        } else {
                            &self.gpu_freqs[..]
                        };
                        if residencies.len() > freq_slice.len() {
                            let (_, scaled, _) =
                                calc_freq_from_residencies(&residencies, freq_slice);
                            result = Some((scaled * 100.0).clamp(0.0, 100.0));
                        }
                        break;
                    }
                }
            }
            unsafe { CFRelease(delta as *const c_void) };
            self.ior.prev = Some(cur);
            result
        } else {
            self.ior.prev = Some(cur);
            None
        }
    }
}

impl Drop for AppleGpuSampler {
    fn drop(&mut self) {}
}
