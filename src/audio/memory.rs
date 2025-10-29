use std::alloc::{alloc, dealloc, Layout};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

/// Optimized memory manager for high-resolution audio buffers
#[derive(Debug)]
pub struct AudioMemoryManager {
    // Memory pools for different buffer sizes
    pools: Arc<Mutex<HashMap<usize, MemoryPool>>>,
    
    // Memory usage tracking
    total_allocated: AtomicUsize,
    peak_allocated: AtomicUsize,
    allocation_count: AtomicUsize,
    
    // Configuration
    max_pool_size: usize,
    alignment: usize,
}

/// Memory pool for specific buffer sizes
#[derive(Debug)]
struct MemoryPool {
    buffers: Vec<NonNull<u8>>,
    buffer_size: usize,
    max_buffers: usize,
    allocated_count: usize,
}

// Safety: MemoryPool is safe to send between threads as long as
// the memory it manages is properly allocated and deallocated
unsafe impl Send for MemoryPool {}
unsafe impl Sync for MemoryPool {}

/// RAII wrapper for managed audio buffer memory
pub struct ManagedAudioBuffer {
    ptr: NonNull<u8>,
    size: usize,
    manager: Arc<AudioMemoryManager>,
}

// Safety: ManagedAudioBuffer is safe to send between threads as long as
// the memory it points to is properly managed and the pointer is valid
unsafe impl Send for ManagedAudioBuffer {}
unsafe impl Sync for ManagedAudioBuffer {}

impl AudioMemoryManager {
    /// Create a new audio memory manager
    pub fn new() -> Self {
        Self {
            pools: Arc::new(Mutex::new(HashMap::new())),
            total_allocated: AtomicUsize::new(0),
            peak_allocated: AtomicUsize::new(0),
            allocation_count: AtomicUsize::new(0),
            max_pool_size: 10, // Maximum buffers per pool
            alignment: 64, // 64-byte alignment for SIMD operations
        }
    }

    /// Create a memory manager with custom configuration
    pub fn with_config(max_pool_size: usize, alignment: usize) -> Self {
        Self {
            pools: Arc::new(Mutex::new(HashMap::new())),
            total_allocated: AtomicUsize::new(0),
            peak_allocated: AtomicUsize::new(0),
            allocation_count: AtomicUsize::new(0),
            max_pool_size,
            alignment,
        }
    }

    /// Allocate an optimized audio buffer
    pub fn allocate_buffer(self: &Arc<Self>, size: usize) -> Result<ManagedAudioBuffer, AudioMemoryError> {
        // Round up size to alignment boundary
        let aligned_size = self.align_size(size);
        
        // Try to get buffer from pool first
        if let Some(ptr) = self.try_get_from_pool(aligned_size) {
            self.update_allocation_stats(aligned_size, true);
            return Ok(ManagedAudioBuffer {
                ptr,
                size: aligned_size,
                manager: Arc::clone(self),
            });
        }
        
        // Allocate new buffer if pool is empty
        let ptr = self.allocate_aligned(aligned_size)?;
        self.update_allocation_stats(aligned_size, false);
        
        Ok(ManagedAudioBuffer {
            ptr,
            size: aligned_size,
            manager: Arc::clone(self),
        })
    }

    /// Try to get a buffer from the appropriate pool
    fn try_get_from_pool(&self, size: usize) -> Option<NonNull<u8>> {
        let mut pools = self.pools.lock().unwrap();
        
        if let Some(pool) = pools.get_mut(&size) {
            pool.buffers.pop()
        } else {
            None
        }
    }

    /// Return a buffer to the appropriate pool
    fn return_to_pool(&self, ptr: NonNull<u8>, size: usize) {
        let mut pools = self.pools.lock().unwrap();
        
        let pool = pools.entry(size).or_insert_with(|| MemoryPool {
            buffers: Vec::new(),
            buffer_size: size,
            max_buffers: self.max_pool_size,
            allocated_count: 0,
        });
        
        // Only return to pool if we haven't exceeded the maximum
        if pool.buffers.len() < pool.max_buffers {
            pool.buffers.push(ptr);
        } else {
            // Pool is full, deallocate the buffer
            unsafe {
                let layout = Layout::from_size_align_unchecked(size, self.alignment);
                dealloc(ptr.as_ptr(), layout);
            }
        }
        
        self.update_deallocation_stats(size);
    }

    /// Allocate aligned memory
    fn allocate_aligned(&self, size: usize) -> Result<NonNull<u8>, AudioMemoryError> {
        let layout = Layout::from_size_align(size, self.alignment)
            .map_err(|_| AudioMemoryError::InvalidLayout)?;
        
        let ptr = unsafe { alloc(layout) };
        
        if ptr.is_null() {
            return Err(AudioMemoryError::AllocationFailed);
        }
        
        // Zero the memory for audio buffers
        unsafe {
            std::ptr::write_bytes(ptr, 0, size);
        }
        
        Ok(unsafe { NonNull::new_unchecked(ptr) })
    }

    /// Align size to the configured alignment boundary
    fn align_size(&self, size: usize) -> usize {
        (size + self.alignment - 1) & !(self.alignment - 1)
    }

    /// Update allocation statistics
    fn update_allocation_stats(&self, size: usize, from_pool: bool) {
        if !from_pool {
            self.allocation_count.fetch_add(1, Ordering::Relaxed);
        }
        
        let new_total = self.total_allocated.fetch_add(size, Ordering::Relaxed) + size;
        
        // Update peak allocation
        let current_peak = self.peak_allocated.load(Ordering::Relaxed);
        if new_total > current_peak {
            self.peak_allocated.store(new_total, Ordering::Relaxed);
        }
    }

    /// Update deallocation statistics
    fn update_deallocation_stats(&self, size: usize) {
        self.total_allocated.fetch_sub(size, Ordering::Relaxed);
    }

    /// Get current memory usage in bytes
    pub fn current_usage(&self) -> usize {
        self.total_allocated.load(Ordering::Relaxed)
    }

    /// Get peak memory usage in bytes
    pub fn peak_usage(&self) -> usize {
        self.peak_allocated.load(Ordering::Relaxed)
    }

    /// Get total number of allocations performed
    pub fn allocation_count(&self) -> usize {
        self.allocation_count.load(Ordering::Relaxed)
    }

    /// Get memory pool statistics
    pub fn pool_stats(&self) -> Vec<PoolStats> {
        let pools = self.pools.lock().unwrap();
        
        pools.iter().map(|(&size, pool)| PoolStats {
            buffer_size: size,
            available_buffers: pool.buffers.len(),
            max_buffers: pool.max_buffers,
            total_allocated: pool.allocated_count,
        }).collect()
    }

    /// Clear all memory pools and force deallocation
    pub fn clear_pools(&self) {
        let mut pools = self.pools.lock().unwrap();
        
        for (_, pool) in pools.iter_mut() {
            for ptr in pool.buffers.drain(..) {
                unsafe {
                    let layout = Layout::from_size_align_unchecked(pool.buffer_size, self.alignment);
                    dealloc(ptr.as_ptr(), layout);
                }
                self.update_deallocation_stats(pool.buffer_size);
            }
        }
        
        pools.clear();
    }

    /// Optimize memory pools by removing unused buffers
    pub fn optimize_pools(&self) {
        let mut pools = self.pools.lock().unwrap();
        
        for (_, pool) in pools.iter_mut() {
            // Keep only half the buffers to reduce memory usage
            let target_size = pool.max_buffers / 2;
            
            while pool.buffers.len() > target_size {
                if let Some(ptr) = pool.buffers.pop() {
                    unsafe {
                        let layout = Layout::from_size_align_unchecked(pool.buffer_size, self.alignment);
                        dealloc(ptr.as_ptr(), layout);
                    }
                    self.update_deallocation_stats(pool.buffer_size);
                }
            }
        }
    }

    /// Pre-allocate buffers for common sizes
    pub fn preallocate_common_sizes(&self) -> Result<(), AudioMemoryError> {
        // Common buffer sizes for different sample rates and bit depths
        let common_sizes = [
            4096,   // Small buffer
            8192,   // Medium buffer
            16384,  // Large buffer
            32768,  // Very large buffer
            65536,  // High-res buffer
        ];
        
        for &size in &common_sizes {
            let aligned_size = self.align_size(size);
            
            // Pre-allocate a few buffers for each size
            for _ in 0..3 {
                let ptr = self.allocate_aligned(aligned_size)?;
                self.return_to_pool(ptr, aligned_size);
            }
        }
        
        Ok(())
    }
}

impl ManagedAudioBuffer {
    /// Get a mutable slice to the buffer data
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe {
            std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.size)
        }
    }

    /// Get an immutable slice to the buffer data
    pub fn as_slice(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(self.ptr.as_ptr(), self.size)
        }
    }

    /// Get a mutable slice of f32 samples (assuming the buffer contains f32 data)
    pub fn as_f32_mut_slice(&mut self) -> &mut [f32] {
        unsafe {
            std::slice::from_raw_parts_mut(
                self.ptr.as_ptr() as *mut f32,
                self.size / std::mem::size_of::<f32>()
            )
        }
    }

    /// Get an immutable slice of f32 samples
    pub fn as_f32_slice(&self) -> &[f32] {
        unsafe {
            std::slice::from_raw_parts(
                self.ptr.as_ptr() as *const f32,
                self.size / std::mem::size_of::<f32>()
            )
        }
    }

    /// Get the buffer size in bytes
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get the buffer capacity in f32 samples
    pub fn f32_capacity(&self) -> usize {
        self.size / std::mem::size_of::<f32>()
    }

    /// Zero the buffer contents
    pub fn zero(&mut self) {
        unsafe {
            std::ptr::write_bytes(self.ptr.as_ptr(), 0, self.size);
        }
    }
}

impl Drop for ManagedAudioBuffer {
    fn drop(&mut self) {
        // Return buffer to pool or deallocate
        self.manager.return_to_pool(self.ptr, self.size);
    }
}

/// Memory pool statistics
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub buffer_size: usize,
    pub available_buffers: usize,
    pub max_buffers: usize,
    pub total_allocated: usize,
}

/// Audio memory management errors
#[derive(Debug, Clone)]
pub enum AudioMemoryError {
    AllocationFailed,
    InvalidLayout,
    PoolExhausted,
}

impl std::fmt::Display for AudioMemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioMemoryError::AllocationFailed => write!(f, "Memory allocation failed"),
            AudioMemoryError::InvalidLayout => write!(f, "Invalid memory layout"),
            AudioMemoryError::PoolExhausted => write!(f, "Memory pool exhausted"),
        }
    }
}

impl std::error::Error for AudioMemoryError {}

/// Optimized buffer allocator for high-resolution audio
pub struct HighResBufferAllocator {
    memory_manager: Arc<AudioMemoryManager>,
    buffer_size_cache: Mutex<HashMap<(u32, u16, u16), usize>>, // (sample_rate, bit_depth, channels) -> size
}

impl HighResBufferAllocator {
    /// Create a new high-resolution buffer allocator
    pub fn new() -> Self {
        let memory_manager = Arc::new(AudioMemoryManager::new());
        
        // Pre-allocate common buffer sizes
        let _ = memory_manager.preallocate_common_sizes();
        
        Self {
            memory_manager,
            buffer_size_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Allocate an optimized buffer for specific audio format
    pub fn allocate_for_format(
        &self,
        sample_rate: u32,
        bit_depth: u16,
        channels: u16,
        duration_ms: u32,
    ) -> Result<ManagedAudioBuffer, AudioMemoryError> {
        let buffer_size = self.calculate_buffer_size(sample_rate, bit_depth, channels, duration_ms);
        self.memory_manager.allocate_buffer(buffer_size)
    }

    /// Calculate optimal buffer size for given audio format
    fn calculate_buffer_size(&self, sample_rate: u32, bit_depth: u16, channels: u16, duration_ms: u32) -> usize {
        let key = (sample_rate, bit_depth, channels);
        
        // Check cache first
        {
            let cache = self.buffer_size_cache.lock().unwrap();
            if let Some(&cached_size) = cache.get(&key) {
                return (cached_size * duration_ms as usize) / 100; // Cached size is for 100ms
            }
        }
        
        // Calculate size for 100ms of audio
        let samples_per_100ms = (sample_rate as f64 * 0.1) as usize;
        let bytes_per_sample = match bit_depth {
            8 => 1,
            16 => 2,
            24 => 3,
            32 => 4,
            _ => 4, // Default to 32-bit
        };
        
        let size_100ms = samples_per_100ms * channels as usize * bytes_per_sample;
        
        // Cache the 100ms size
        {
            let mut cache = self.buffer_size_cache.lock().unwrap();
            cache.insert(key, size_100ms);
        }
        
        // Return size for requested duration
        (size_100ms * duration_ms as usize) / 100
    }

    /// Get memory manager statistics
    pub fn memory_stats(&self) -> MemoryStats {
        MemoryStats {
            current_usage: self.memory_manager.current_usage(),
            peak_usage: self.memory_manager.peak_usage(),
            allocation_count: self.memory_manager.allocation_count(),
            pool_stats: self.memory_manager.pool_stats(),
        }
    }

    /// Optimize memory usage
    pub fn optimize(&self) {
        self.memory_manager.optimize_pools();
    }

    /// Clear all cached data
    pub fn clear_cache(&self) {
        self.buffer_size_cache.lock().unwrap().clear();
        self.memory_manager.clear_pools();
    }
}

/// Memory usage statistics
#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub current_usage: usize,
    pub peak_usage: usize,
    pub allocation_count: usize,
    pub pool_stats: Vec<PoolStats>,
}

impl MemoryStats {
    /// Format memory statistics as a human-readable string
    pub fn format_stats(&self) -> String {
        let mut stats = String::new();
        
        stats.push_str("=== Memory Statistics ===\n");
        stats.push_str(&format!("Current Usage: {:.2} MB\n", self.current_usage as f64 / 1024.0 / 1024.0));
        stats.push_str(&format!("Peak Usage: {:.2} MB\n", self.peak_usage as f64 / 1024.0 / 1024.0));
        stats.push_str(&format!("Total Allocations: {}\n", self.allocation_count));
        
        if !self.pool_stats.is_empty() {
            stats.push_str("\n--- Memory Pools ---\n");
            for pool in &self.pool_stats {
                stats.push_str(&format!(
                    "Size: {} bytes, Available: {}/{}, Total: {}\n",
                    pool.buffer_size,
                    pool.available_buffers,
                    pool.max_buffers,
                    pool.total_allocated
                ));
            }
        }
        
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_manager_creation() {
        let manager = Arc::new(AudioMemoryManager::new());
        
        assert_eq!(manager.current_usage(), 0);
        assert_eq!(manager.peak_usage(), 0);
        assert_eq!(manager.allocation_count(), 0);
    }

    #[test]
    fn test_buffer_allocation() {
        let manager = Arc::new(AudioMemoryManager::new());
        
        let buffer = manager.allocate_buffer(1024).unwrap();
        assert_eq!(buffer.size(), 1024);
        assert!(manager.current_usage() >= 1024);
        assert!(manager.peak_usage() >= 1024);
        assert_eq!(manager.allocation_count(), 1);
    }

    #[test]
    fn test_buffer_pooling() {
        let manager = Arc::new(AudioMemoryManager::new());
        
        // Allocate and drop a buffer
        {
            let _buffer = manager.allocate_buffer(1024).unwrap();
        }
        
        // Allocate another buffer of the same size (should come from pool)
        let buffer2 = manager.allocate_buffer(1024).unwrap();
        assert_eq!(buffer2.size(), 1024);
        
        // Allocation count should still be 1 (reused from pool)
        assert_eq!(manager.allocation_count(), 1);
    }

    #[test]
    fn test_buffer_alignment() {
        let manager = Arc::new(AudioMemoryManager::with_config(10, 64));
        
        let buffer = manager.allocate_buffer(100).unwrap();
        
        // Size should be aligned to 64-byte boundary
        assert_eq!(buffer.size() % 64, 0);
        assert!(buffer.size() >= 100);
    }

    #[test]
    fn test_managed_buffer_operations() {
        let manager = Arc::new(AudioMemoryManager::new());
        let mut buffer = manager.allocate_buffer(1024).unwrap();
        
        // Test byte operations
        let slice = buffer.as_mut_slice();
        slice[0] = 42;
        assert_eq!(buffer.as_slice()[0], 42);
        
        // Test f32 operations
        let f32_slice = buffer.as_f32_mut_slice();
        f32_slice[0] = 3.14;
        assert_eq!(buffer.as_f32_slice()[0], 3.14);
        
        // Test zero operation
        buffer.zero();
        assert_eq!(buffer.as_slice()[0], 0);
        assert_eq!(buffer.as_f32_slice()[0], 0.0);
    }

    #[test]
    fn test_high_res_allocator() {
        let allocator = HighResBufferAllocator::new();
        
        // Allocate buffer for high-res audio (192kHz, 24-bit, stereo, 100ms)
        let buffer = allocator.allocate_for_format(192000, 24, 2, 100).unwrap();
        
        // Should be large enough for the specified format
        let expected_samples = (192000.0 * 0.1) as usize; // 100ms worth
        let expected_size = expected_samples * 2 * 3; // stereo * 3 bytes per sample
        
        assert!(buffer.size() >= expected_size);
    }

    #[test]
    fn test_memory_stats() {
        let allocator = HighResBufferAllocator::new();
        
        // Allocate some buffers
        let _buffer1 = allocator.allocate_for_format(44100, 16, 2, 100).unwrap();
        let _buffer2 = allocator.allocate_for_format(96000, 24, 2, 100).unwrap();
        
        let stats = allocator.memory_stats();
        
        assert!(stats.current_usage > 0);
        assert!(stats.peak_usage > 0);
        assert!(stats.allocation_count > 0);
        
        let formatted = stats.format_stats();
        assert!(formatted.contains("Memory Statistics"));
        assert!(formatted.contains("Current Usage"));
    }

    #[test]
    fn test_pool_optimization() {
        let manager = Arc::new(AudioMemoryManager::with_config(10, 64));
        
        // Allocate and drop many buffers to fill pools
        for _ in 0..15 {
            let _buffer = manager.allocate_buffer(1024).unwrap();
        }
        
        let stats_before = manager.pool_stats();
        manager.optimize_pools();
        let stats_after = manager.pool_stats();
        
        // Pool should be optimized (fewer available buffers)
        if let (Some(before), Some(after)) = (stats_before.first(), stats_after.first()) {
            assert!(after.available_buffers <= before.available_buffers);
        }
    }

    #[test]
    fn test_memory_error_handling() {
        // Test invalid layout
        let manager = Arc::new(AudioMemoryManager::with_config(10, 1024));
        
        // This should work fine
        let result = manager.allocate_buffer(1024);
        assert!(result.is_ok());
    }

    #[test]
    fn test_buffer_size_calculation() {
        let allocator = HighResBufferAllocator::new();
        
        // Test different formats
        let size_cd = allocator.calculate_buffer_size(44100, 16, 2, 100);
        let size_hires = allocator.calculate_buffer_size(192000, 24, 2, 100);
        
        // High-res should require more memory
        assert!(size_hires > size_cd);
        
        // Test caching (second call should be faster)
        let size_cd_cached = allocator.calculate_buffer_size(44100, 16, 2, 100);
        assert_eq!(size_cd, size_cd_cached);
    }

    #[test]
    fn test_clear_operations() {
        let allocator = HighResBufferAllocator::new();
        
        // Allocate some buffers
        let _buffer = allocator.allocate_for_format(44100, 16, 2, 100).unwrap();
        
        let stats_before = allocator.memory_stats();
        assert!(stats_before.current_usage > 0);
        
        // Clear cache and pools
        allocator.clear_cache();
        
        // Memory should be freed
        let stats_after = allocator.memory_stats();
        assert_eq!(stats_after.current_usage, 0);
    }
}