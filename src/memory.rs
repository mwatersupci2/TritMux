use crate::trit::payload::TritPayload;
use crate::trit::tryte::{STryte, TRITS_PER_TRYTE};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::mem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MemoryRegionId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StackFrameId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    Heap,
    Stack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Residency {
    Pinned,
    Hot,
    Warm,
    Cold,
    Flushable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPinState {
    Unpinned,
    Pinned,
    Unsupported,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStats {
    pub regions: usize,
    pub heap_regions: usize,
    pub stack_regions: usize,
    pub pinned_regions: usize,
    pub total_strytes: usize,
    pub total_trits: usize,
    pub host_pinned_bytes: usize,
}

#[derive(Debug)]
pub struct MemoryRegion {
    id: MemoryRegionId,
    name: String,
    kind: MemoryKind,
    residency: Residency,
    strytes: Vec<STryte>,
    used_trits: usize,
    logical_pin_count: usize,
    pre_pin_residency: Option<Residency>,
    host_pin: Option<HostPinGuard>,
    stack_frame: Option<StackFrameId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    id: StackFrameId,
    name: String,
    regions: Vec<MemoryRegionId>,
}

#[derive(Debug, Default)]
pub struct TrinaryMemoryManager {
    next_region: u64,
    next_frame: u64,
    regions: HashMap<MemoryRegionId, MemoryRegion>,
    stack_frames: Vec<StackFrame>,
}

#[derive(Debug)]
pub enum MemoryError {
    RegionNotFound(MemoryRegionId),
    StackFrameNotFound(StackFrameId),
    RegionStillPinned(MemoryRegionId),
    ResizePinnedRegion(MemoryRegionId),
    TritLengthExceedsCapacity {
        trits: usize,
        strytes: usize,
    },
}

#[derive(Debug)]
struct HostPinGuard {
    ptr: *const u8,
    len: usize,
}

#[derive(Debug)]
enum HostPinError {
    Failed(i32),
}

impl MemoryRegionId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl StackFrameId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl MemoryRegion {
    pub fn id(&self) -> MemoryRegionId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> MemoryKind {
        self.kind
    }

    pub fn residency(&self) -> Residency {
        self.residency
    }

    pub fn set_residency(&mut self, residency: Residency) {
        self.residency = residency;
    }

    pub fn strytes(&self) -> &[STryte] {
        &self.strytes
    }

    pub fn strytes_mut(&mut self) -> &mut [STryte] {
        &mut self.strytes
    }

    pub fn used_trits(&self) -> usize {
        self.used_trits
    }

    pub fn capacity_trits(&self) -> usize {
        self.strytes.len() * TRITS_PER_TRYTE
    }

    pub fn len_strytes(&self) -> usize {
        self.strytes.len()
    }

    pub fn is_pinned(&self) -> bool {
        self.logical_pin_count > 0 || matches!(self.residency, Residency::Pinned)
    }

    pub fn logical_pin_count(&self) -> usize {
        self.logical_pin_count
    }

    pub fn host_pin_state(&self) -> HostPinState {
        if self.host_pin.is_some() {
            HostPinState::Pinned
        } else {
            HostPinState::Unpinned
        }
    }

    pub fn stack_frame(&self) -> Option<StackFrameId> {
        self.stack_frame
    }

    pub fn to_payload(&self) -> Result<TritPayload, MemoryError> {
        TritPayload::from_strytes(self.strytes.clone(), self.used_trits).map_err(|_| {
            MemoryError::TritLengthExceedsCapacity {
                trits: self.used_trits,
                strytes: self.strytes.len(),
            }
        })
    }
}

impl StackFrame {
    pub fn id(&self) -> StackFrameId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn regions(&self) -> &[MemoryRegionId] {
        &self.regions
    }
}

impl TrinaryMemoryManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allocate_heap(
        &mut self,
        name: impl Into<String>,
        stryte_count: usize,
        residency: Residency,
    ) -> MemoryRegionId {
        let id = self.alloc_region_id();
        self.regions.insert(
            id,
            MemoryRegion {
                id,
                name: name.into(),
                kind: MemoryKind::Heap,
                residency,
                strytes: vec![STryte::zero(); stryte_count],
                used_trits: stryte_count * TRITS_PER_TRYTE,
                logical_pin_count: 0,
                pre_pin_residency: None,
                host_pin: None,
                stack_frame: None,
            },
        );
        id
    }

    pub fn allocate_heap_payload(
        &mut self,
        name: impl Into<String>,
        payload: &TritPayload,
        residency: Residency,
    ) -> MemoryRegionId {
        let id = self.alloc_region_id();
        self.regions.insert(
            id,
            MemoryRegion {
                id,
                name: name.into(),
                kind: MemoryKind::Heap,
                residency,
                strytes: payload.strytes().to_vec(),
                used_trits: payload.len_trits(),
                logical_pin_count: 0,
                pre_pin_residency: None,
                host_pin: None,
                stack_frame: None,
            },
        );
        id
    }

    pub fn duplicate_as_heap(
        &mut self,
        source_id: MemoryRegionId,
        name: impl Into<String>,
    ) -> Result<MemoryRegionId, MemoryError> {
        let (strytes, used_trits, residency) = {
            let source = self.region(source_id)?;
            let residency = if matches!(source.residency, Residency::Pinned) {
                Residency::Hot
            } else {
                source.residency
            };
            (source.strytes.clone(), source.used_trits, residency)
        };

        let id = self.alloc_region_id();
        self.regions.insert(
            id,
            MemoryRegion {
                id,
                name: name.into(),
                kind: MemoryKind::Heap,
                residency,
                strytes,
                used_trits,
                logical_pin_count: 0,
                pre_pin_residency: None,
                host_pin: None,
                stack_frame: None,
            },
        );
        Ok(id)
    }

    pub fn push_stack_frame(&mut self, name: impl Into<String>) -> StackFrameId {
        let id = self.alloc_frame_id();
        self.stack_frames.push(StackFrame {
            id,
            name: name.into(),
            regions: Vec::new(),
        });
        id
    }

    pub fn allocate_stack(
        &mut self,
        frame_id: StackFrameId,
        name: impl Into<String>,
        stryte_count: usize,
    ) -> Result<MemoryRegionId, MemoryError> {
        let frame_index = self
            .stack_frames
            .iter()
            .position(|frame| frame.id == frame_id)
            .ok_or(MemoryError::StackFrameNotFound(frame_id))?;
        let id = self.alloc_region_id();

        self.regions.insert(
            id,
            MemoryRegion {
                id,
                name: name.into(),
                kind: MemoryKind::Stack,
                residency: Residency::Pinned,
                strytes: vec![STryte::zero(); stryte_count],
                used_trits: stryte_count * TRITS_PER_TRYTE,
                logical_pin_count: 0,
                pre_pin_residency: None,
                host_pin: None,
                stack_frame: Some(frame_id),
            },
        );
        self.stack_frames[frame_index].regions.push(id);
        Ok(id)
    }

    pub fn pop_stack_frame(&mut self) -> Result<Option<StackFrame>, MemoryError> {
        let Some(frame) = self.stack_frames.pop() else {
            return Ok(None);
        };

        let pinned_region = frame.regions.iter().copied().find(|region_id| {
            self.regions
                .get(region_id)
                .map(MemoryRegion::logical_pin_count)
                .unwrap_or(0)
                > 0
        });

        if let Some(region_id) = pinned_region {
            self.stack_frames.push(frame);
            return Err(MemoryError::RegionStillPinned(region_id));
        }

        for region_id in &frame.regions {
            self.regions.remove(region_id);
        }

        Ok(Some(frame))
    }

    pub fn pin_region(&mut self, id: MemoryRegionId) -> Result<HostPinState, MemoryError> {
        let region = self.region_mut(id)?;
        if region.logical_pin_count == 0 {
            region.pre_pin_residency = Some(region.residency);
        }
        region.logical_pin_count = region.logical_pin_count.saturating_add(1);
        region.residency = Residency::Pinned;

        if region.host_pin.is_some() {
            return Ok(HostPinState::Pinned);
        }

        match HostPinGuard::pin_strytes(&region.strytes) {
            Ok(Some(guard)) => {
                region.host_pin = Some(guard);
                Ok(HostPinState::Pinned)
            }
            Ok(None) => Ok(HostPinState::Unsupported),
            Err(HostPinError::Failed(code)) => Ok(HostPinState::Failed(format!(
                "mlock failed with code {code}"
            ))),
        }
    }

    pub fn unpin_region(&mut self, id: MemoryRegionId) -> Result<(), MemoryError> {
        let region = self.region_mut(id)?;
        region.logical_pin_count = region.logical_pin_count.saturating_sub(1);
        if region.logical_pin_count == 0 {
            region.host_pin = None;
            if let Some(previous) = region.pre_pin_residency.take() {
                region.residency = previous;
            }
        }
        Ok(())
    }

    pub fn resize_region(
        &mut self,
        id: MemoryRegionId,
        stryte_count: usize,
    ) -> Result<(), MemoryError> {
        let region = self.region_mut(id)?;
        if region.host_pin.is_some() || region.logical_pin_count > 0 {
            return Err(MemoryError::ResizePinnedRegion(id));
        }

        region.strytes.resize(stryte_count, STryte::zero());
        region.used_trits = region.used_trits.min(stryte_count * TRITS_PER_TRYTE);
        Ok(())
    }

    pub fn region(&self, id: MemoryRegionId) -> Result<&MemoryRegion, MemoryError> {
        self.regions.get(&id).ok_or(MemoryError::RegionNotFound(id))
    }

    pub fn region_mut(&mut self, id: MemoryRegionId) -> Result<&mut MemoryRegion, MemoryError> {
        self.regions
            .get_mut(&id)
            .ok_or(MemoryError::RegionNotFound(id))
    }

    pub fn stack_frames(&self) -> &[StackFrame] {
        &self.stack_frames
    }

    pub fn eviction_candidates(&self) -> Vec<MemoryRegionId> {
        let mut candidates: Vec<_> = self
            .regions
            .values()
            .filter(|region| {
                !region.is_pinned()
                    && matches!(region.residency, Residency::Cold | Residency::Flushable)
            })
            .map(MemoryRegion::id)
            .collect();
        candidates.sort();
        candidates
    }

    pub fn stats(&self) -> MemoryStats {
        let regions = self.regions.values();
        let mut stats = MemoryStats {
            regions: 0,
            heap_regions: 0,
            stack_regions: 0,
            pinned_regions: 0,
            total_strytes: 0,
            total_trits: 0,
            host_pinned_bytes: 0,
        };

        for region in regions {
            stats.regions += 1;
            match region.kind {
                MemoryKind::Heap => stats.heap_regions += 1,
                MemoryKind::Stack => stats.stack_regions += 1,
            }
            if region.is_pinned() {
                stats.pinned_regions += 1;
            }
            stats.total_strytes += region.len_strytes();
            stats.total_trits += region.used_trits();
            if region.host_pin.is_some() {
                stats.host_pinned_bytes += region.strytes.len() * mem::size_of::<STryte>();
            }
        }

        stats
    }

    fn alloc_region_id(&mut self) -> MemoryRegionId {
        self.next_region = self.next_region.saturating_add(1);
        MemoryRegionId(self.next_region)
    }

    fn alloc_frame_id(&mut self) -> StackFrameId {
        self.next_frame = self.next_frame.saturating_add(1);
        StackFrameId(self.next_frame)
    }
}

impl HostPinGuard {
    fn pin_strytes(strytes: &[STryte]) -> Result<Option<Self>, HostPinError> {
        if strytes.is_empty() {
            return Ok(None);
        }

        host_pin(strytes.as_ptr().cast::<u8>(), mem::size_of_val(strytes))
    }
}

impl Drop for HostPinGuard {
    fn drop(&mut self) {
        host_unpin(self.ptr, self.len);
    }
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn host_pin(ptr: *const u8, len: usize) -> Result<Option<HostPinGuard>, HostPinError> {
    use std::ffi::c_void;
    use std::os::raw::c_int;

    unsafe extern "C" {
        fn mlock(addr: *const c_void, len: usize) -> c_int;
    }

    let rc = unsafe { mlock(ptr.cast::<c_void>(), len) };
    if rc == 0 {
        Ok(Some(HostPinGuard { ptr, len }))
    } else {
        Err(HostPinError::Failed(rc))
    }
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
fn host_pin(_ptr: *const u8, _len: usize) -> Result<Option<HostPinGuard>, HostPinError> {
    Ok(None)
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn host_unpin(ptr: *const u8, len: usize) {
    use std::ffi::c_void;
    use std::os::raw::c_int;

    unsafe extern "C" {
        fn munlock(addr: *const c_void, len: usize) -> c_int;
    }

    let _ = unsafe { munlock(ptr.cast::<c_void>(), len) };
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
fn host_unpin(_ptr: *const u8, _len: usize) {}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryError::RegionNotFound(id) => write!(f, "memory region {} was not found", id.0),
            MemoryError::StackFrameNotFound(id) => write!(f, "stack frame {} was not found", id.0),
            MemoryError::RegionStillPinned(id) => {
                write!(f, "memory region {} is still pinned", id.0)
            }
            MemoryError::ResizePinnedRegion(id) => {
                write!(f, "cannot resize pinned memory region {}", id.0)
            }
            MemoryError::TritLengthExceedsCapacity { trits, strytes } => write!(
                f,
                "trit length {trits} exceeds capacity of {strytes} sTrytes"
            ),
        }
    }
}

impl Error for MemoryError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trit::payload::TritPayload;

    #[test]
    fn heap_regions_track_stats_and_eviction_candidates() {
        let mut manager = TrinaryMemoryManager::new();
        let hot = manager.allocate_heap("hot", 2, Residency::Hot);
        let cold = manager.allocate_heap("cold", 3, Residency::Cold);
        let flushable = manager.allocate_heap("flushable", 4, Residency::Flushable);

        assert_eq!(manager.eviction_candidates(), vec![cold, flushable]);
        let stats = manager.stats();
        assert_eq!(stats.regions, 3);
        assert_eq!(stats.heap_regions, 3);
        assert_eq!(stats.stack_regions, 0);
        assert_eq!(stats.total_strytes, 9);
        assert_eq!(stats.total_trits, 9 * TRITS_PER_TRYTE);

        manager.pin_region(cold).unwrap();
        assert_eq!(manager.eviction_candidates(), vec![flushable]);
        assert_eq!(manager.region(cold).unwrap().logical_pin_count(), 1);
        manager.unpin_region(cold).unwrap();
        assert_eq!(manager.region(cold).unwrap().logical_pin_count(), 0);
        assert!(manager.eviction_candidates().contains(&cold));

        assert!(manager.resize_region(hot, 5).is_ok());
    }

    #[test]
    fn pinned_regions_cannot_be_resized() {
        let mut manager = TrinaryMemoryManager::new();
        let id = manager.allocate_heap("critical", 8, Residency::Hot);
        let _ = manager.pin_region(id).unwrap();
        assert!(matches!(
            manager.resize_region(id, 16),
            Err(MemoryError::ResizePinnedRegion(_))
        ));
    }

    #[test]
    fn stack_frames_own_regions_and_pop_them() {
        let mut manager = TrinaryMemoryManager::new();
        let frame = manager.push_stack_frame("app call");
        let region = manager.allocate_stack(frame, "locals", 2).unwrap();
        assert_eq!(manager.region(region).unwrap().kind(), MemoryKind::Stack);
        assert_eq!(manager.stack_frames()[0].regions(), &[region]);

        let popped = manager.pop_stack_frame().unwrap().unwrap();
        assert_eq!(popped.id(), frame);
        assert!(matches!(
            manager.region(region),
            Err(MemoryError::RegionNotFound(_))
        ));
    }

    #[test]
    fn stack_frame_pop_rejects_logically_pinned_region() {
        let mut manager = TrinaryMemoryManager::new();
        let frame = manager.push_stack_frame("protected call");
        let region = manager.allocate_stack(frame, "locals", 2).unwrap();
        let _ = manager.pin_region(region).unwrap();
        assert!(matches!(
            manager.pop_stack_frame(),
            Err(MemoryError::RegionStillPinned(_))
        ));
        assert_eq!(manager.stack_frames().len(), 1);
    }

    #[test]
    fn payloads_can_live_in_heap_regions() {
        let payload = TritPayload::from_utf8("trinary RAM");
        let mut manager = TrinaryMemoryManager::new();
        let region = manager.allocate_heap_payload("payload", &payload, Residency::Hot);
        assert_eq!(manager.region(region).unwrap().to_payload().unwrap(), payload);
    }
}
