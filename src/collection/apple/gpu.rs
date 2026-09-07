use crate::collection::error::CollectionResult;

#[derive(Debug)]
pub struct AppleGpuSampler {
    // baseline_sample: CFDictionaryRef,
    // baseline_time: Instant,
    // SocInfo, subscription...
}

imple AppleGpuSampler {
    pub fn new() -> CollectionResult<Self> {
        // SocInfo::new(), IORerport::with_filter(gpu), SMC::new()
        // return Err on missing freq tables / subsription fail
        todo!()
    }

    // Returns GPU usage as percent
    pub fn get_gpu_usage(&mut self) -> Option<f32> {
    // delta = CreateSamplesDelta(baseline, next)
    // gpu_scaled_ratio from 'GPU Performance States' residency -> *100.0
    None
}    

}

impl Drop for AppleGpuSampler {
    fn drop(&mut self) {
        // CFRelease samples/subscription
    }
}

#[cfg(target_os = "macos")]
pub fn get_apple_gpu_usage(_sample: &mut AppleGpuSampler) -> Option<f32> {
    None
}
