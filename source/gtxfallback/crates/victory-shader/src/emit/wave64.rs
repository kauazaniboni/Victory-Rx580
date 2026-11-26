//! Guest-wave collectives for a common, acyclic execution schedule.
//!
//! Every group invocation must reach each site exactly once. `guest_flat`
//! is a bijection onto 0..threads, independent of the host's wave layout.
//! A bounded workspace holds predicates and values. Every site synchronizes
//! after its reads, so later sites cannot overwrite data still being read by
//! another host wave. No helper changes the caller's block.

use super::bitcode::{Attr, Bin, Cast, V, pred};
use super::shader::Shader;

const SHARED_LIMIT_BYTES: u32 = 32 * 1024;

impl Shader {
    /// Full-EXEC native compute only. The caller proves every thread follows
    /// the same control flow, including loops and exits. Unlike DAG sites,
    /// these may repeat, so the second barrier protects each completed read
    /// before any thread overwrites the reusable scratch for the next call.
    pub(crate) fn wave64_full_readfirst(
        &mut self, block: usize, flat: V, value: V, threads: u32,
    ) -> Result<V, &'static str> {
        let declared = self.threads.and_then(|t| t.into_iter().try_fold(1u32, u32::checked_mul));
        if threads == 0 || threads > 1024 || !threads.is_multiple_of(64) || declared != Some(threads)
            || self.m.type_of(flat) != self.t.i32 || self.m.type_of(value) != self.t.i32
        {
            return Err("full-EXEC election requires complete native wave64 groups");
        }
        let scratch = match self.native_election_scratch {
            Some(scratch) => scratch,
            None => {
                let scratch = self.wave64_scratch(threads)?;
                self.native_election_scratch = Some(scratch);
                scratch
            }
        };
        let at = self.m.element(block, scratch, flat);
        self.m.store(block, at, value);
        self.wave64_sync(block);
        let mask = self.m.i32(!63);
        let base = self.m.bin(block, Bin::And, flat, mask);
        let at = self.m.element(block, scratch, base);
        let elected = self.m.load(block, at);
        self.wave64_sync(block);
        Ok(elected)
    }

    fn wave64_inputs(
        &self,
        guest_flat: V,
        predicate: V,
        threads: u32,
    ) -> Result<(), &'static str> {
        let declared = self.threads.and_then(|dims| {
            dims.into_iter().try_fold(1u32, u32::checked_mul)
        });
        if !matches!(threads, 64 | 128) || declared != Some(threads)
        {
            return Err("guest wave64 collective requires a complete 64- or 128-thread group");
        }
        if self.m.type_of(guest_flat) != self.t.i32
            || self.m.type_of(predicate) != self.t.i1
        {
            return Err("guest wave64 collective requires an i32 thread index and i1 predicate");
        }
        Ok(())
    }

    fn wave64_scratch(&mut self, words: u32) -> Result<V, &'static str> {
        let bytes = words.checked_mul(4).ok_or("guest wave64 shared-memory size overflow")?;
        let next = self.collective_shared_bytes.checked_add(bytes)
            .ok_or("guest wave64 shared-memory size overflow")?;
        let total = self.lds_dwords.checked_mul(4).and_then(|lds| lds.checked_add(next))
            .ok_or("guest wave64 shared-memory size overflow")?;
        if total > SHARED_LIMIT_BYTES {
            return Err("guest wave64 collectives exceed the 32 KiB group shared-memory limit");
        }
        let name = format!("wave64_{}", self.collective_shared_bytes);
        let array = self.m.array(words, self.t.i32);
        let scratch = self.m.global(&name, array, 3);
        self.collective_shared_bytes = next;
        Ok(scratch)
    }

    fn wave64_sync(&mut self, block: usize) {
        let mode = self.m.i32(1 | 8);
        self.op(block, "barrier", Attr::NoDuplicate, self.t.void, 80, &[mode]);
    }

    /// Ballots and elections share one predicate/value array. Both helpers
    /// synchronize after reading it: a write at the next site cannot race a
    /// lagging host wave's reads at the current site. Their existing contract
    /// still requires every group invocation to visit every collective site.
    fn wave64_workspace(&mut self, threads: u32) -> Result<V, &'static str> {
        if let Some((reserved_threads, scratch)) = self.guest_collective_scratch {
            if reserved_threads != threads {
                return Err("guest wave64 workspace thread count changed");
            }
            return Ok(scratch);
        }
        let scratch = self.wave64_scratch(threads * 2)?;
        self.guest_collective_scratch = Some((threads, scratch));
        Ok(scratch)
    }

    /// Reads a completed predicate array in logical guest-lane order.
    fn wave64_mask(&mut self, block: usize, scratch: V, guest_flat: V) -> (V, V) {
        let mask = self.m.i32(!63);
        let base = self.m.bin(block, Bin::And, guest_flat, mask);
        let mut halves = [self.m.i32(0); 2];
        for lane in 0..64u32 {
            let k = self.m.i32(lane);
            let index = self.m.bin(block, Bin::Add, base, k);
            let at = self.m.element(block, scratch, index);
            let bit = self.m.load(block, at);
            let shift = self.m.i32(lane & 31);
            let bit = self.m.bin(block, Bin::Shl, bit, shift);
            let half = (lane / 32) as usize;
            halves[half] = self.m.bin(block, Bin::Or, halves[half], bit);
        }
        (halves[0], halves[1])
    }

    /// Ballots a guest predicate over each independent group of 64 threads.
    /// All threads must reach this site, even when their predicate is false.
    pub(crate) fn wave64_ballot(
        &mut self,
        block: usize,
        guest_flat: V,
        predicate: V,
        threads: u32,
    ) -> Result<(V, V), &'static str> {
        self.wave64_inputs(guest_flat, predicate, threads)?;
        let scratch = self.wave64_workspace(threads)?;
        let bit = self.m.cast(block, Cast::ZExt, predicate, self.t.i32);
        let at = self.m.element(block, scratch, guest_flat);
        self.m.store(block, at, bit);
        self.wave64_sync(block);
        let result = self.wave64_mask(block, scratch, guest_flat);
        self.wave64_sync(block);
        Ok(result)
    }

    /// Elects the lowest guest lane whose EXEC bit is set, or lane zero
    /// when EXEC is empty. Inactive lanes must supply their preserved VGPR
    /// values: RDNA's empty-EXEC case still reads lane zero's register.
    pub(crate) fn wave64_readfirst(
        &mut self,
        block: usize,
        guest_flat: V,
        value: V,
        exec_predicate: V,
        threads: u32,
    ) -> Result<V, &'static str> {
        self.wave64_inputs(guest_flat, exec_predicate, threads)?;
        if self.m.type_of(value) != self.t.i32 {
            return Err("guest wave64 readfirst requires an i32 source value");
        }
        let scratch = self.wave64_workspace(threads)?;
        let bit = self.m.cast(block, Cast::ZExt, exec_predicate, self.t.i32);
        let at = self.m.element(block, scratch, guest_flat);
        self.m.store(block, at, bit);
        let values = self.m.i32(threads);
        let index = self.m.bin(block, Bin::Add, values, guest_flat);
        let at = self.m.element(block, scratch, index);
        self.m.store(block, at, value);
        self.wave64_sync(block);

        let (lo, hi) = self.wave64_mask(block, scratch, guest_flat);
        let first_lo = self.op(block, "unaryBits.i32", Attr::ReadNone, self.t.i32, 32, &[lo]);
        let first_hi = self.op(block, "unaryBits.i32", Attr::ReadNone, self.t.i32, 32, &[hi]);
        let zero = self.m.i32(0);
        let k32 = self.m.i32(32);
        let high_lane = self.m.bin(block, Bin::Add, first_hi, k32);
        let has_hi = self.m.cmp(block, pred::NE, hi, zero);
        let high_or_zero = self.m.select(block, has_hi, high_lane, zero);
        let has_lo = self.m.cmp(block, pred::NE, lo, zero);
        let lane = self.m.select(block, has_lo, first_lo, high_or_zero);
        let mask = self.m.i32(!63);
        let base = self.m.bin(block, Bin::And, guest_flat, mask);
        let index = self.m.bin(block, Bin::Add, base, lane);
        let index = self.m.bin(block, Bin::Add, values, index);
        let at = self.m.element(block, scratch, index);
        let elected = self.m.load(block, at);
        self.wave64_sync(block);
        Ok(elected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collective_budget_counts_guest_lds_and_reuses_one_workspace() {
        let mut s = Shader::new(false);
        s.threads = Some([128, 1, 1]);
        s.lds_dwords = (SHARED_LIMIT_BYTES - 1024) / 4;
        let block = s.entry;
        let flat = s.m.i32(0);
        let value = s.m.i32(123);
        let exec = s.m.bool(false);
        for _ in 0..64 {
            s.wave64_readfirst(block, flat, value, exec, 128).unwrap();
            s.wave64_ballot(block, flat, exec, 128).unwrap();
        }
        assert_eq!(s.lds_dwords * 4 + s.collective_shared_bytes, SHARED_LIMIT_BYTES);
        assert_eq!(s.collective_shared_bytes, 1024);
        // The reusable workspace does not waive the physical memory limit.
        let mut full = Shader::new(false);
        full.threads = Some([128, 1, 1]);
        full.lds_dwords = (SHARED_LIMIT_BYTES - 1024 + 4) / 4;
        let block = full.entry;
        let flat = full.m.i32(0);
        let exec = full.m.bool(false);
        assert!(full.wave64_ballot(block, flat, exec, 128).is_err());
        assert_eq!(full.collective_shared_bytes, 0);
        assert!(full.guest_collective_scratch.is_none());
    }

    #[test]
    fn collective_budget_overflows_fail_without_reserving_memory() {
        let mut s = Shader::new(false);
        assert!(s.wave64_scratch(u32::MAX).is_err());
        assert_eq!(s.collective_shared_bytes, 0);
        s.lds_dwords = u32::MAX;
        assert!(s.wave64_scratch(64).is_err());
        assert_eq!(s.collective_shared_bytes, 0);
        s.lds_dwords = 0;
        s.collective_shared_bytes = u32::MAX;
        assert!(s.wave64_scratch(64).is_err());
        assert_eq!(s.collective_shared_bytes, u32::MAX);
    }

    #[test]
    fn incomplete_or_mismatched_groups_are_rejected() {
        let mut s = Shader::new(false);
        let block = s.entry;
        let flat = s.m.i32(0);
        let predicate = s.m.bool(true);
        for threads in [0, 32, 96, 256] {
            s.threads = Some([threads, 1, 1]);
            assert!(s.wave64_ballot(block, flat, predicate, threads).is_err());
        }
        s.threads = Some([64, 1, 1]);
        assert!(s.wave64_ballot(block, flat, predicate, 128).is_err());
        s.threads = Some([u32::MAX, u32::MAX, u32::MAX]);
        assert!(s.wave64_ballot(block, flat, predicate, 64).is_err());
        s.threads = Some([64, 1, 1]);
        assert!(s.wave64_ballot(block, flat, flat, 64).is_err());
        assert!(s.wave64_readfirst(block, flat, predicate, predicate, 64).is_err());
        assert_eq!(s.collective_shared_bytes, 0);
    }

    /// CPU validation only: never creates a D3D12 device or dispatches work.
    #[cfg(windows)]
    #[test]
    fn shared_collectives_validate_for_64_and_128_threads() {
        use super::super::bitcode::Term;
        use super::super::validator::Validator;

        let path = Validator::default_path().expect("DXIL validator installed");
        let mut validator = Validator::load(Some(&path)).expect("DXIL validator loads");
        for threads in [64, 128] {
            let mut s = Shader::new(false);
            s.threads = Some([threads, 1, 1]);
            let block = s.entry;
            let flat = s.op(block, "flattenedThreadIdInGroup.i32", Attr::ReadNone, s.t.i32, 96, &[]);
            let mask = s.m.i32(63);
            let lane = s.m.bin(block, Bin::And, flat, mask);
            let k40 = s.m.i32(40);
            let high_only = s.m.cmp(block, pred::IUGE, lane, k40);
            let k1000 = s.m.i32(1000);
            let value = s.m.bin(block, Bin::Add, flat, k1000);
            s.wave64_ballot(block, flat, high_only, threads).unwrap();
            s.wave64_readfirst(block, flat, value, high_only, threads).unwrap();
            let none = s.m.bool(false);
            s.wave64_readfirst(block, flat, value, none, threads).unwrap();
            assert_eq!(s.collective_shared_bytes, threads * 8);
            assert_eq!(s.flags & super::super::shader::flags::WAVE_OPS, 0);
            s.m.terminate(block, Term::Ret);
            let mut dxil = s.finish();
            validator.validate(&mut dxil).unwrap_or_else(|why| {
                panic!("{threads}-thread guest wave64 collectives: {why}");
            });
        }
    }
}
