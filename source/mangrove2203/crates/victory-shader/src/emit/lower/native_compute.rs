//! Full-EXEC native compute elections with a group-uniform control-flow proof.
//!
//! A scalar is only wave-uniform on the console. In particular, a first-lane
//! read of the thread ID is NOT group-uniform. Barriers for an exchange are
//! legal here only after proving that every branch in the program is uniform
//! over the entire group, including loop back edges and early exits.

use super::*;

fn pure_scalar(i: &Inst) -> bool {
    matches!(i.fields, Fields::Sop { .. })
        && matches!(
            i.name(),
            "s_mov_b32"
                | "s_mov_b64"
                | "s_add_u32"
                | "s_sub_u32"
                | "s_add_i32"
                | "s_sub_i32"
                | "s_addc_u32"
                | "s_subb_u32"
                | "s_and_b32"
                | "s_and_b64"
                | "s_or_b32"
                | "s_or_b64"
                | "s_xor_b32"
                | "s_xor_b64"
                | "s_lshl_b32"
                | "s_lshr_b32"
                | "s_cmp_eq_u32"
                | "s_cmp_lg_u32"
                | "s_cmp_gt_u32"
                | "s_cmp_ge_u32"
                | "s_cmp_lt_u32"
                | "s_cmp_le_u32"
                | "s_cmp_eq_i32"
                | "s_cmp_lg_i32"
                | "s_cmp_gt_i32"
                | "s_cmp_ge_i32"
                | "s_cmp_lt_i32"
                | "s_cmp_le_i32"
        )
}

/// Deliberately narrow: no changing EXEC, no LDS accesses, no lane-layout
/// operations, and only group-uniform scalar control flow. Memory loads and
/// readfirstlane results are never evidence of group uniformity. This leaves
/// divergent native loops to a future common-schedule implementation.
pub(in crate::emit) fn full_group_elections(x: &Parts) -> bool {
    let StageIo::Cs(cs) = x.native.io else {
        return false;
    };
    let Some(threads) = cs.threads.into_iter().try_fold(1u32, u32::checked_mul) else {
        return false;
    };
    if !x.native.wave64
        || !x.p.wave64
        || threads == 0
        || threads > 1024
        || !threads.is_multiple_of(64)
        || cs
            .lds_bytes()
            .checked_add(threads * 4)
            .is_none_or(|n| n > 32 * 1024)
    {
        return false;
    }
    let blocks = &x.p.blocks;
    let instructions = || blocks.iter().flat_map(|b| &b.insts);
    if !instructions().any(|i| i.name() == "v_readfirstlane_b32")
        || instructions().any(|i| {
            i.writes
                .iter()
                .any(|&(r, _)| matches!(r, Reg::S(EXEC) | Reg::S(127)))
                || matches!(
                    i.fields,
                    Fields::Ds { .. } | Fields::Buffer { lds: true, .. }
                )
        })
        || x.st
            .guards
            .iter()
            .flatten()
            .flatten()
            .any(|&g| g != Bool::TRUE)
    {
        return false;
    }

    // Start optimistically for pure SALU definitions and their phis, then
    // remove every value reached by a non-uniform/unknown input. This handles
    // loop-carried scalar counters without declaring an SGPR uniform merely
    // because it is scalar. Uniform branch selection is proved separately
    // below, making the phis safe under this jointly checked invariant.
    let mut uniform: HashSet<Value> =
        x.p.inputs
            .iter()
            .filter_map(|&(reg, value)| {
                let root = match reg {
                    Reg::S(s) => x.native.layout.iter().any(|r| {
                        u32::from(s) >= u32::from(r.user_data)
                            && u32::from(s) < u32::from(r.user_data) + r.sgprs
                    }),
                    _ => false,
                };
                (root
                    || x.io
                        .inputs
                        .iter()
                        .any(|&(v, input)| v == value && matches!(input, Input::GroupId(_))))
                .then_some(value)
            })
            .collect();
    let mut definitions = Vec::new();
    for b in blocks {
        for phi in &b.phis {
            if matches!(phi.reg, Reg::S(_) | Reg::Scc) {
                uniform.insert(phi.value);
                definitions.push((phi.value, phi.args.clone()));
            }
        }
        for i in &b.insts {
            if pure_scalar(i) {
                for &(reg, value) in &i.writes {
                    if matches!(reg, Reg::S(_) | Reg::Scc) {
                        uniform.insert(value);
                        definitions.push((value, i.reads.iter().map(|r| r.1).collect()));
                    }
                }
            }
        }
    }
    loop {
        let mut changed = false;
        for (value, reads) in &definitions {
            if reads.iter().any(|v| !uniform.contains(v)) {
                changed |= uniform.remove(value);
            }
        }
        if !changed {
            break;
        }
    }
    x.st.exits.iter().all(|exit| match *exit {
        LaneExit::Branch { when, .. } => {
            x.st.bools
                .support(when)
                .iter()
                .all(|a| matches!(a, Atom::Uniform(v) if uniform.contains(v)))
        }
        LaneExit::Indirect => false,
        _ => true,
    })
}

impl Lower<'_> {
    pub(super) fn native_full_readfirst(&mut self, value: V) -> Result<V, Why> {
        let threads = self
            .s
            .threads
            .ok_or("native election without a thread group")?
            .into_iter()
            .product();
        let flat = self.s.op(
            self.s.entry,
            "flattenedThreadIdInGroup.i32",
            Attr::ReadNone,
            self.s.t.i32,
            96,
            &[],
        );
        self.s.wave64_full_readfirst(self.cur, flat, value, threads)
    }
}
