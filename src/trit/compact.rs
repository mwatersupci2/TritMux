use crate::trit::block::PTryteBlock320;
use crate::trit::tryte::STryte;
use std::error::Error;
use std::fmt;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

pub const SETTLE_BUFFER_STRYTES: usize = 2;
pub const ACTIVE_GUARD_STRYTES: usize = 5;
pub const LOOKAHEAD_STRYTES: usize = ACTIVE_GUARD_STRYTES - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockWindow {
    active_index: usize,
    start: usize,
    end_exclusive: usize,
    total: usize,
}

#[derive(Debug)]
pub struct SlidingMutexWindow {
    locked: Vec<AtomicBool>,
}

#[derive(Debug)]
pub struct SlidingWindowGuard<'a> {
    locks: &'a SlidingMutexWindow,
    window: LockWindow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedCompaction {
    blocks: Vec<PTryteBlock320>,
    windows: Vec<LockWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowLockError {
    IndexOutOfBounds { index: usize, total: usize },
    AlreadyLocked(usize),
}

impl LockWindow {
    pub fn for_active(active_index: usize, total: usize) -> Result<Self, WindowLockError> {
        if active_index >= total {
            return Err(WindowLockError::IndexOutOfBounds {
                index: active_index,
                total,
            });
        }

        let start = active_index.saturating_sub(SETTLE_BUFFER_STRYTES);
        let end_exclusive = total.min(active_index + LOOKAHEAD_STRYTES + 1);

        Ok(Self {
            active_index,
            start,
            end_exclusive,
            total,
        })
    }

    pub fn active_index(self) -> usize {
        self.active_index
    }

    pub fn range(self) -> Range<usize> {
        self.start..self.end_exclusive
    }

    pub fn active_guard_range(self) -> Range<usize> {
        self.active_index..self.end_exclusive
    }

    pub fn settle_range(self) -> Range<usize> {
        self.start..self.active_index
    }

    pub fn is_locked(self, index: usize) -> bool {
        self.range().contains(&index)
    }

    pub fn total(self) -> usize {
        self.total
    }
}

impl SlidingMutexWindow {
    pub fn new(total_strytes: usize) -> Self {
        Self {
            locked: (0..total_strytes).map(|_| AtomicBool::new(false)).collect(),
        }
    }

    pub fn total_strytes(&self) -> usize {
        self.locked.len()
    }

    pub fn can_access(&self, index: usize) -> bool {
        self.locked
            .get(index)
            .map(|flag| !flag.load(Ordering::Acquire))
            .unwrap_or(false)
    }

    pub fn can_write(&self, index: usize) -> bool {
        self.can_access(index)
    }

    pub fn lock_at(&self, active_index: usize) -> Result<SlidingWindowGuard<'_>, WindowLockError> {
        let window = LockWindow::for_active(active_index, self.total_strytes())?;
        self.lock_range(window.range())?;
        Ok(SlidingWindowGuard {
            locks: self,
            window,
        })
    }

    fn lock_range(&self, range: Range<usize>) -> Result<(), WindowLockError> {
        let mut locked_now = Vec::new();
        for index in range {
            match self.lock_index(index) {
                Ok(()) => locked_now.push(index),
                Err(err) => {
                    self.unlock_indices(&locked_now);
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    fn lock_index(&self, index: usize) -> Result<(), WindowLockError> {
        let Some(flag) = self.locked.get(index) else {
            return Err(WindowLockError::IndexOutOfBounds {
                index,
                total: self.total_strytes(),
            });
        };

        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| WindowLockError::AlreadyLocked(index))
    }

    fn unlock_indices(&self, indices: &[usize]) {
        for index in indices {
            self.locked[*index].store(false, Ordering::Release);
        }
    }
}

impl SlidingWindowGuard<'_> {
    pub fn window(&self) -> LockWindow {
        self.window
    }

    pub fn slide_to(&mut self, active_index: usize) -> Result<LockWindow, WindowLockError> {
        let next = LockWindow::for_active(active_index, self.locks.total_strytes())?;
        let mut locked_now = Vec::new();

        for index in next.range() {
            if !self.window.is_locked(index) {
                match self.locks.lock_index(index) {
                    Ok(()) => locked_now.push(index),
                    Err(err) => {
                        self.locks.unlock_indices(&locked_now);
                        return Err(err);
                    }
                }
            }
        }

        let to_unlock: Vec<usize> = self
            .window
            .range()
            .filter(|index| !next.is_locked(*index))
            .collect();
        self.locks.unlock_indices(&to_unlock);
        self.window = next;
        Ok(next)
    }
}

impl Drop for SlidingWindowGuard<'_> {
    fn drop(&mut self) {
        let indices: Vec<usize> = self.window.range().collect();
        self.locks.unlock_indices(&indices);
    }
}

impl GuardedCompaction {
    pub fn blocks(&self) -> &[PTryteBlock320] {
        &self.blocks
    }

    pub fn windows(&self) -> &[LockWindow] {
        &self.windows
    }
}

pub fn compact_strytes_guarded(strytes: &[STryte]) -> Result<GuardedCompaction, WindowLockError> {
    if strytes.is_empty() {
        return Ok(GuardedCompaction {
            blocks: Vec::new(),
            windows: Vec::new(),
        });
    }

    let locks = SlidingMutexWindow::new(strytes.len());
    let mut guard = locks.lock_at(0)?;
    let mut windows = vec![guard.window()];

    for active_index in 1..strytes.len() {
        windows.push(guard.slide_to(active_index)?);
    }

    Ok(GuardedCompaction {
        blocks: PTryteBlock320::pack_many(strytes),
        windows,
    })
}

impl fmt::Display for WindowLockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WindowLockError::IndexOutOfBounds { index, total } => {
                write!(f, "sTryte index {index} is outside total {total}")
            }
            WindowLockError::AlreadyLocked(index) => {
                write!(f, "sTryte index {index} is already locked")
            }
        }
    }
}

impl Error for WindowLockError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trit::core::Trit;
    use crate::trit::tryte::{STryte, TRITS_PER_TRYTE};

    #[test]
    fn lock_window_has_two_settle_and_four_lookahead_strytes() {
        let window = LockWindow::for_active(3, 10).unwrap();
        assert_eq!(window.settle_range(), 1..3);
        assert_eq!(window.active_guard_range(), 3..8);
        assert_eq!(window.range(), 1..8);
    }

    #[test]
    fn sliding_guard_moves_without_unlocking_overlap_first() {
        let locks = SlidingMutexWindow::new(8);
        {
            let mut guard = locks.lock_at(0).unwrap();
            assert!(!locks.can_write(0));
            assert!(!locks.can_write(4));
            assert!(locks.can_write(5));

            guard.slide_to(3).unwrap();
            assert!(locks.can_write(0));
            assert!(!locks.can_write(1));
            assert!(!locks.can_write(7));
        }

        for index in 0..8 {
            assert!(locks.can_write(index));
        }
    }

    #[test]
    fn overlapping_compactor_windows_are_rejected() {
        let locks = SlidingMutexWindow::new(8);
        let _guard = locks.lock_at(0).unwrap();
        assert!(matches!(
            locks.lock_at(2),
            Err(WindowLockError::AlreadyLocked(_))
        ));
    }

    #[test]
    fn guarded_compaction_records_a_window_per_stryte() {
        let strytes: Vec<STryte> = (0..18)
            .map(|index| {
                let trit = if index % 2 == 0 {
                    Trit::Neutral
                } else {
                    Trit::Positive
                };
                STryte::from_trits([trit; TRITS_PER_TRYTE])
            })
            .collect();

        let result = compact_strytes_guarded(&strytes).unwrap();
        assert_eq!(result.windows().len(), strytes.len());
        assert_eq!(result.blocks().len(), 2);
        assert_eq!(
            result.blocks()[0].ptrytes_as_strytes().unwrap()[0],
            strytes[0]
        );
    }
}
