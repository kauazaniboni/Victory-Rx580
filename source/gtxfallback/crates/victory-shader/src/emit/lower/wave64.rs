//! Exact guest-wave masks for an opt-in, acyclic mesh-compute schedule.
//!
//! Every physical thread visits the same topological sequence of native
//! blocks. Native control flow is data: one participation flag per edge,
//! uniform within a guest wave. Memory and ordinary instructions are guarded;
//! comparisons and lane elections reconverge before shared-memory exchange.
//! No host-wave layout, spin protocol, or half-wave substitute is used.

use super::*;
use crate::ir::VCC;

fn collective(i: &Inst) -> bool {
    let n = i.name();
    n == "v_readfirstlane_b32" || n.starts_with("v_cmp_") || n.starts_with("v_cmpx_")
        || matches!(n, "v_add_co_u32" | "v_sub_co_u32" | "v_subrev_co_u32"
            | "v_add_co_ci_u32" | "v_sub_co_ci_u32" | "v_subrev_co_ci_u32")
}

/// RDNA2 ISA section 5.8, table 16: hardware IDs 20/21 hold the
/// FLAT_SCRATCH address, not floating-point MODE. This exact literal setup
/// is dead only after dag_plan has excluded every reachable scratch access
/// and GETREG observer. Partial or SGPR-sourced writes remain unsupported.
fn dead_flat_scratch_setter(i: &Inst) -> bool {
    i.name() == "s_setreg_imm32_b32"
        && matches!(&i.fields, Fields::Sop { dst: None, src, imm: Some(field) }
            if matches!(*field, 0xf814 | 0xf815) && matches!(src.as_slice(), [Operand::Literal(_)]))
}

/// Reachable topological order. An actual cycle, not instruction count,
/// rejects the mode before any bytecode can be dispatched.
fn dag_order(p: &Program) -> Result<Vec<usize>, Why> {
    let mut reachable = vec![false; p.blocks.len()];
    let mut todo = vec![0];
    while let Some(b) = todo.pop() {
        if b >= p.blocks.len() { return Err("a guest-wave branch outside the program"); }
        if std::mem::replace(&mut reachable[b], true) { continue; }
        if p.blocks[b].exit == Exit::Indirect { return Err("an indirect guest-wave branch"); }
        todo.extend(p.blocks[b].exit.successors());
    }
    let mut incoming = vec![0usize; p.blocks.len()];
    for (b, block) in p.blocks.iter().enumerate().filter(|(b, _)| reachable[*b]) {
        let _ = b;
        for q in block.exit.successors() { incoming[q] += 1; }
    }
    let mut ready: std::collections::BTreeSet<usize> = (0..p.blocks.len())
        .filter(|&b| reachable[b] && incoming[b] == 0).collect();
    let mut order = Vec::new();
    while let Some(b) = ready.pop_first() {
        order.push(b);
        for q in p.blocks[b].exit.successors() {
            incoming[q] -= 1;
            if incoming[q] == 0 { ready.insert(q); }
        }
    }
    if order.len() != reachable.iter().filter(|&&b| b).count() {
        return Err("a cyclic mesh program in guest-wave DAG mode");
    }
    Ok(order)
}

impl<'a> Lower<'a> {
    fn dag_plan(&self) -> Result<Vec<usize>, Stop> {
        let StageIo::Ms(ms) = &self.x.native.io else {
            return Err(unsupported(0, "guest-wave DAG mode requires a mesh program"));
        };
        if !self.x.mesh_compute || !self.x.native.wave64 || !self.x.p.wave64
            || !matches!(ms.threads, 64 | 128) || ms.lds_bytes != 0 {
            return Err(unsupported(0, "guest-wave DAG mode requires wave64 mesh compute with 64/128 threads and no LDS"));
        }
        let order = dag_order(self.x.p).map_err(|w| unsupported(0, w))?;
        for &b in &order {
            for (index, i) in self.x.p.blocks[b].insts.iter().enumerate() {
                let n = i.name();
                let fail = |w| unsupported(i.machine.offset, w);
                if n == "s_getreg_b32"
                    && matches!(i.fields, Fields::Sop { imm: Some(field), .. } if matches!(field & 63, 20 | 21)) {
                    return Err(fail("a scratch register read in guest-wave DAG mode"));
                }
                if n.starts_with("s_scratch_") {
                    return Err(fail("scalar scratch memory in guest-wave DAG mode"));
                }
                if matches!(n, "s_round_mode" | "s_denorm_mode") {
                    return Err(fail("an unmodelled MODE update in guest-wave DAG mode"));
                }
                if matches!(n, "s_setreg_b32" | "s_setreg_imm32_b32") && !dead_flat_scratch_setter(i) {
                    let mode = matches!(i.fields, Fields::Sop { imm: Some(field), .. } if field & 63 == 1);
                    return Err(fail(if mode { "an unmodelled MODE update in guest-wave DAG mode" }
                        else { "an unmodelled hardware-register update in guest-wave DAG mode" }));
                }
                if matches!(n, "s_gl1_inv" | "s_dcache_inv" | "s_dcache_wb" | "buffer_gl1_inv" | "s_swappc_b64") {
                    return Err(fail("an unmodelled cache operation or indirect call in guest-wave DAG mode"));
                }
                if matches!(i.fields, Fields::Sopp { .. }) && !matches!(n,
                    "s_endpgm" | "s_endpgm_saved" | "s_endpgm_ordered_ps_done" | "s_code_end"
                    | "s_branch" | "s_cbranch_scc0" | "s_cbranch_scc1" | "s_cbranch_execz"
                    | "s_cbranch_execnz" | "s_cbranch_vccz" | "s_cbranch_vccnz" | "s_barrier" | "s_sendmsg") {
                    return Err(fail("an unsupported control instruction in guest-wave DAG mode"));
                }
                // Read-only image accesses with an explicit (or zero) level
                // are plain per-lane loads, guarded like buffer loads; ones
                // that write, or sample with implicit derivatives (which a
                // compute program has none of), are not.
                let read_only_image = matches!(i.fields, Fields::Image { .. })
                    && (n.starts_with("image_load") || n == "image_get_resinfo"
                        || ["image_sample", "image_gather4"].iter().any(|k| n.strip_prefix(k).is_some_and(|rest| {
                            rest.starts_with("_lz") || rest.starts_with("_l") || rest.starts_with("_d")
                                || rest.starts_with("_c_lz") || rest.starts_with("_c_l") || rest.starts_with("_c_d")
                        })));
                if !read_only_image && (matches!(i.fields, Fields::Ds { .. } | Fields::Flat { .. } | Fields::Vintrp { .. } | Fields::Image { .. })
                    || n.contains("atomic") || n.starts_with("s_store") || n.starts_with("s_buffer_store")) {
                    return Err(fail("shared, flat or atomic memory in guest-wave DAG mode"));
                }
                if matches!(n, "v_readlane_b32" | "v_writelane_b32")
                    || matches!(&i.fields, Fields::Valu(v) if v.dpp.is_some()) {
                    return Err(fail("an unsupported lane operation in guest-wave DAG mode"));
                }
                if matches!(i.fields, Fields::Buffer { lds: true, .. }) {
                    return Err(fail("a buffer access to LDS in guest-wave DAG mode"));
                }
                if matches!(i.fields, Fields::Smem { cache, .. } if cache.glc || cache.dlc) {
                    return Err(fail("a coherent scalar load in guest-wave DAG mode"));
                }
                if n.contains("load")
                    && let Some(src) = self.descriptors.get(&((b, index), false))
                    && src.leaves().iter().any(|s| self.used_kind(s).is_some_and(|u| u.kind == ResourceType::Uav))
                {
                    return Err(fail("a mutable UAV load in guest-wave DAG mode"));
                }
                if n == "s_barrier" && self.writes_uavs {
                    return Err(fail("native barriers with UAV writes in guest-wave DAG mode"));
                }
                if n.starts_with("s_sendmsg")
                    && !matches!(i.fields, Fields::Sopp { imm } if n == "s_sendmsg" && imm & 0xF == GS_ALLOC_REQ) {
                    return Err(fail("a non-allocation message in guest-wave DAG mode"));
                }
            }
        }
        Ok(order)
    }

    pub(super) fn run_wave64_dag(mut self) -> Result<Lowered, Stop> {
        let order = self.dag_plan()?;
        let p = self.x.p;
        self.cur = self.s.entry;
        self.inputs()?;
        self.mesh_prologue(self.s.mesh.ok_or_else(|| unsupported(0, "missing mesh output layout"))?)
            .map_err(|w| unsupported(0, w))?;
        let mut edges: HashMap<(usize, usize), V> = HashMap::new();
        for &b in &order {
            self.cur_ir = b;
            self.block[b] = Some(self.cur);
            let mut live = self.s.m.bool(b == 0);
            for &pred in &p.blocks[b].preds {
                if let Some(&edge) = edges.get(&(pred, b)) {
                    live = self.s.m.bin(self.cur, Bin::Or, live, edge);
                }
            }
            self.dag_live = Some(live);
            // A native SSA phi selects the predecessor followed by this
            // guest wave, not the physical predecessor of our linear schedule.
            for phi in &p.blocks[b].phis {
                let bit = phi.reg == Reg::Scc;
                let mut result = if bit { self.s.m.bool(false) } else { self.s.m.i32(0) };
                let mut missing = None;
                for (n, &pred) in p.blocks[b].preds.iter().enumerate() {
                    let Some(&edge) = edges.get(&(pred, b)) else { continue };
                    let val = match self.get(phi.args[n]) {
                        Ok(Val::Bit(v)) if bit => v,
                        Ok(Val::I32(v)) if !bit => v,
                        Ok(Val::Bit(v)) => self.s.m.cast(self.cur, Cast::ZExt, v, self.s.t.i32),
                        Ok(Val::I32(v)) => {
                            let z = self.s.m.i32(0);
                            self.s.m.cmp(self.cur, pred::NE, v, z)
                        }
                        Ok(Val::Missing(w)) | Err(w) => { missing = Some(w); continue; }
                    };
                    result = self.s.m.select(self.cur, edge, val, result);
                }
                let val = match missing {
                    Some(w) => Val::Missing(w),
                    None if bit => Val::Bit(result),
                    None => Val::I32(result),
                };
                self.vals.insert(phi.value, val);
            }
            for i in 0..p.blocks[b].insts.len() { self.dag_inst(b, i)?; }
            match p.blocks[b].exit {
                Exit::Jump(to) => { self.dag_edge(&mut edges, b, to, live); }
                Exit::Branch { taken, not_taken } => {
                    let i = p.blocks[b].insts.last().ok_or_else(|| unsupported(p.blocks[b].offset, "a guest branch without an instruction"))?;
                    let cond = self.dag_branch(i).map_err(|w| unsupported(i.machine.offset, w))?;
                    let yes = self.s.m.bin(self.cur, Bin::And, live, cond);
                    let truth = self.s.m.bool(true);
                    let no = self.s.m.bin(self.cur, Bin::Xor, cond, truth);
                    let no = self.s.m.bin(self.cur, Bin::And, live, no);
                    self.dag_edge(&mut edges, b, taken, yes);
                    self.dag_edge(&mut edges, b, not_taken, no);
                }
                Exit::End => {}
                Exit::Indirect => return Err(unsupported(p.blocks[b].offset, "an indirect guest-wave branch")),
            }
            self.tail[b] = Some(self.cur);
        }
        self.s.m.terminate(self.cur, Term::Ret);
        Ok(self.finish_lowered())
    }

    fn dag_edge(&mut self, edges: &mut HashMap<(usize, usize), V>, from: usize, to: usize, value: V) {
        let value = match edges.get(&(from, to)) {
            Some(&old) => self.s.m.bin(self.cur, Bin::Or, old, value),
            None => value,
        };
        edges.insert((from, to), value);
        self.dag_edges.insert((from, to), value);
    }

    fn dag_inst(&mut self, b: usize, i: usize) -> Result<(), Stop> {
        let inst = &self.x.p.blocks[b].insts[i];
        let offset = inst.machine.offset;
        // All native control flow is represented by edge predicates.
        // LDS-free allocation synchronization has no host-side operation.
        if matches!(inst.fields, Fields::Sopp { .. }) { return Ok(()); }
        // The whole reachable program has passed dag_plan, including its
        // no-observer proof. Do not rely on generic dead-result suppression.
        if dead_flat_scratch_setter(inst) { return Ok(()); }
        if collective(inst) {
            // Crucially outside every physical conditional region.
            return self.valu(inst, b, i).map_err(|w| unsupported(offset, w));
        }
        let live = self.dag_live.ok_or_else(|| unsupported(offset, "missing guest block participation"))?;
        let guard = if matches!(inst.fields, Fields::Buffer { .. } | Fields::Image { .. } | Fields::Exp { .. }) {
            let exec = self.dag_exec_bit(inst).map_err(|w| unsupported(offset, w))?;
            self.s.m.bin(self.cur, Bin::And, live, exec)
        } else { live };
        // Guard the whole operation, including constructing a resource
        // descriptor for a memory access. Scalar operations ignore EXEC.
        let at = self.begin_guarded(Some(guard));
        self.inst(b, i)?;
        let mut definitions = Vec::new();
        let mut values = Vec::new();
        for &(reg, id) in &inst.writes {
            match self.vals.get(&id).copied() {
                Some(Val::I32(v)) => {
                    // An EXEC-inactive vector load retains its old VGPR.
                    let old = if matches!(reg, Reg::V(_)) {
                        inst.read(reg).and_then(|p| self.vals.get(&p)).copied()
                    } else { None };
                    let outside = match old {
                        Some(Val::I32(o)) => o,
                        _ => self.s.m.i32(0),
                    };
                    definitions.push((id, false));
                    values.push((v, outside));
                }
                Some(Val::Bit(v)) => {
                    definitions.push((id, true));
                    values.push((v, self.s.m.bool(false)));
                }
                _ => {}
            }
            self.floats.remove(&id);
        }
        let merged = self.end_guarded(at, &values);
        for ((id, bit), value) in definitions.into_iter().zip(merged) {
            self.vals.insert(id, if bit { Val::Bit(value) } else { Val::I32(value) });
        }
        Ok(())
    }

    fn dag_pair(&mut self, inst: &Inst, code: u8) -> Result<(V, V), Why> {
        if code == NULL { let z = self.s.m.i32(0); return Ok((z, z)); }
        let lo = inst.read(Reg::S(code)).ok_or("a missing guest mask low dword")?;
        let hi = inst.read(Reg::S(code + 1)).ok_or("a missing guest mask high dword")?;
        Ok((self.i32_of(lo)?, self.i32_of(hi)?))
    }

    fn dag_operand_pair(&mut self, inst: &Inst, op: &Operand) -> Result<(V, V), Why> {
        match *op {
            Operand::Scalar { code, .. } => self.dag_pair(inst, code),
            Operand::Const(k) => {
                let v = crate::resource::inline(k).ok_or("a special scalar mask operand")?;
                Ok((self.s.m.i32(v), self.s.m.i32(if (v as i32) < 0 { u32::MAX } else { 0 })))
            }
            Operand::Literal(v) => Ok((self.s.m.i32(v), self.s.m.i32(0))),
            _ => Err("a non-scalar guest mask"),
        }
    }

    fn dag_bit(&mut self, lo: V, hi: V) -> V {
        let lane = self.lane_index();
        let (k32, k31, one, zero) = (self.s.m.i32(32), self.s.m.i32(31), self.s.m.i32(1), self.s.m.i32(0));
        let low = self.s.m.cmp(self.cur, pred::IULT, lane, k32);
        let word = self.s.m.select(self.cur, low, lo, hi);
        let shift = self.s.m.bin(self.cur, Bin::And, lane, k31);
        let bit = self.s.m.bin(self.cur, Bin::LShr, word, shift);
        let bit = self.s.m.bin(self.cur, Bin::And, bit, one);
        self.s.m.cmp(self.cur, pred::NE, bit, zero)
    }

    pub(super) fn dag_exec_bit(&mut self, inst: &Inst) -> Result<V, Why> {
        let (lo, hi) = self.dag_pair(inst, EXEC)?;
        Ok(self.dag_bit(lo, hi))
    }

    pub(super) fn dag_operand_bit(&mut self, inst: &Inst, op: &Operand) -> Result<V, Why> {
        let (lo, hi) = self.dag_operand_pair(inst, op)?;
        Ok(self.dag_bit(lo, hi))
    }

    fn dag_nonzero(&mut self, lo: V, hi: V) -> V {
        let v = self.s.m.bin(self.cur, Bin::Or, lo, hi);
        let z = self.s.m.i32(0);
        self.s.m.cmp(self.cur, pred::NE, v, z)
    }

    fn dag_branch(&mut self, inst: &Inst) -> Result<V, Why> {
        let name = inst.name();
        let (value, invert) = match name {
            "s_cbranch_scc0" | "s_cbranch_scc1" => (self.scc_in(inst)?, name.ends_with('0')),
            "s_cbranch_execz" | "s_cbranch_execnz" => {
                let (lo, hi) = self.dag_pair(inst, EXEC)?;
                (self.dag_nonzero(lo, hi), name == "s_cbranch_execz")
            }
            "s_cbranch_vccz" | "s_cbranch_vccnz" => {
                let (lo, hi) = self.dag_pair(inst, VCC)?;
                (self.dag_nonzero(lo, hi), name == "s_cbranch_vccz")
            }
            _ => return Err("an unsupported native guest-wave branch"),
        };
        Ok(if invert {
            let t = self.s.m.bool(true);
            self.s.m.bin(self.cur, Bin::Xor, value, t)
        } else { value })
    }

    pub(super) fn dag_mask_result(&mut self, inst: &Inst, code: u8, predicate: V) -> Result<(), Why> {
        let exec = self.dag_exec_bit(inst)?;
        let live = self.dag_live.ok_or("missing guest block participation")?;
        let enabled = self.s.m.bin(self.cur, Bin::And, exec, live);
        let bit = self.s.m.bin(self.cur, Bin::And, enabled, predicate);
        let flat = self.mesh_thread();
        let threads = self.s.threads.ok_or("a guest wave without threads")?[0];
        let (lo, hi) = self.s.wave64_ballot(self.cur, flat, bit, threads)?;
        if code != NULL {
            self.set(inst.write(Reg::S(code)), Val::I32(lo));
            self.set(inst.write(Reg::S(code + 1)), Val::I32(hi));
        }
        Ok(())
    }

    pub(super) fn dag_saveexec(&mut self, inst: &Inst) -> Result<(), Why> {
        let Fields::Sop { dst, src, .. } = &inst.fields else { return Err("a guest scalar mask operation"); };
        let name = inst.name();
        let op = name.strip_prefix("s_").and_then(|s| s.strip_suffix("_b64"))
            .ok_or("a non-wave64 saveexec operation")?;
        let (op, save_old) = if let Some(op) = op.strip_suffix("_saveexec") { (op, true) }
            else if let Some(op) = op.strip_suffix("_wrexec") { (op, false) }
            else { return Err("an unknown guest EXEC update"); };
        let (a, ah) = self.dag_pair(inst, EXEC)?;
        let (b, bh) = self.dag_operand_pair(inst, src.first().ok_or("a saveexec source")?)?;
        let mut result = Vec::new();
        for (mut a, mut b) in [(a, b), (ah, bh)] {
            let ones = self.s.m.i32(u32::MAX);
            // For SAVEEXEC, source0 is the explicit operand and source1
            // is old EXEC (as the ISA's ANDN1/ANDN2 forms distinguish).
            std::mem::swap(&mut a, &mut b);
            if matches!(op, "andn1" | "orn1") { a = self.s.m.bin(self.cur, Bin::Xor, a, ones); }
            if matches!(op, "andn2" | "orn2") { b = self.s.m.bin(self.cur, Bin::Xor, b, ones); }
            let bin = match op {
                "and" | "andn1" | "andn2" | "nand" => Bin::And,
                "or" | "orn1" | "orn2" | "nor" => Bin::Or,
                "xor" | "xnor" => Bin::Xor,
                _ => return Err("an unsupported guest EXEC combination"),
            };
            let mut r = self.s.m.bin(self.cur, bin, a, b);
            if matches!(op, "nand" | "nor" | "xnor") { r = self.s.m.bin(self.cur, Bin::Xor, r, ones); }
            result.push(r);
        }
        if let Some(Operand::Scalar { code, .. }) = dst {
            self.set(inst.write(Reg::S(*code)), Val::I32(if save_old { a } else { result[0] }));
            self.set(inst.write(Reg::S(*code + 1)), Val::I32(if save_old { ah } else { result[1] }));
        }
        self.set(inst.write(Reg::S(EXEC)), Val::I32(result[0]));
        self.set(inst.write(Reg::S(EXEC + 1)), Val::I32(result[1]));
        let scc = self.dag_nonzero(result[0], result[1]);
        self.set(inst.write(Reg::Scc), Val::Bit(scc));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::MeshIo;

    fn graph(exits: Vec<Exit>) -> Program {
        let mut blocks: Vec<_> = exits.into_iter().map(|exit| crate::ir::Block {
            offset: 0, preds: Vec::new(), phis: Vec::new(), insts: Vec::new(), exit,
        }).collect();
        for b in 0..blocks.len() {
            for to in blocks[b].exit.successors() { blocks[to].preds.push(b); }
        }
        Program { wave64: true, vgprs: 0, blocks, inputs: Vec::new() }
    }

    fn plan(p: &Program, threads: u32, lds: u32) -> Result<Vec<usize>, Stop> {
        let native = NativeProgram {
            stage: HwStage::Ms, wave64: true, vgprs: 0, layout: vec![], used: vec![], code: vec![],
            io: StageIo::Ms(MeshIo { threads, lds_bytes: lds, outputs: vec![], clip_cntl: 0,
                max_vertices: 128, max_primitives: 128, topology: 2 }),
        };
        let io = crate::io::raise(p, &native).unwrap();
        // These fixtures exercise the admission gate, not address tracking.
        // Rejected memory forms must reach the gate even with unknown bases.
        let res = crate::resource::Resources::default();
        let st = crate::structure::structure(p).unwrap();
        let parts = Parts { native: &native, p, st: &st, io: &io, res: &res,
            inputs: vec![], outputs: vec![], prims: vec![], mesh_stride: 128, view_marks: false,
            mesh_compute: true, wave64_dag: true,   };
        let bad = HashMap::new();
        Lower::new(&parts, &bad).dag_plan()
    }

    #[test]
    fn guest_schedule_orders_diamonds_and_rejects_cycles_and_indirect_exits() {
        let p = graph(vec![Exit::Branch { taken: 1, not_taken: 2 }, Exit::Jump(3), Exit::Jump(3), Exit::End]);
        let order = dag_order(&p).unwrap();
        assert_eq!(order.len(), 4);
        for (from, block) in p.blocks.iter().enumerate() {
            for to in block.exit.successors() {
                assert!(order.iter().position(|&b| b == from) < order.iter().position(|&b| b == to));
            }
        }
        assert!(dag_order(&graph(vec![Exit::Jump(0)])).is_err());
        assert!(dag_order(&graph(vec![Exit::Jump(1), Exit::Jump(0)])).is_err());
        assert!(dag_order(&graph(vec![Exit::Indirect])).is_err());
    }

    #[test]
    fn guest_schedule_rejects_native_lds_and_incomplete_guest_waves() {
        // Run the actual plan gate without emitting DXIL or creating a GPU.
        for (threads, lds, accepted) in [(64, 0, true), (128, 0, true), (96, 0, false), (128, 4, false)] {
            let p = graph(vec![Exit::End]);
            assert_eq!(plan(&p, threads, lds).is_ok(), accepted,
                "threads={threads}, native LDS={lds}");
        }
    }

    fn decoded(words: &[u32]) -> Program {
        let code: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        crate::ir::build(&code, true, 4).expect("synthetic instruction decoding")
    }

    fn hwfield(id: u32, offset: u32, width: u32) -> u32 {
        assert!(offset < 32 && (1..=32).contains(&width));
        id | offset << 6 | (width - 1) << 11
    }

    fn setreg(op: u32, field: u32) -> u32 { 0xb000_0000 | op << 23 | field }

    #[test]
    fn guest_schedule_accepts_dead_flat_scratch_literal_setters() {
        for threads in [64, 128] {
            for value in [0, 1024, u32::MAX] {
                let p = decoded(&[
                    setreg(0x15, hwfield(20, 0, 32)), value,
                    setreg(0x15, hwfield(21, 0, 32)), value,
                    0xbf81_0000,
                ]);
                assert!(plan(&p, threads, 0).is_ok(), "{threads} threads, literal {value}");
            }
        }
    }

    #[test]
    fn guest_schedule_rejects_observed_flat_scratch_even_for_dead_results() {
        // Put the observer in a successor block so eligibility must scan
        // beyond the setters before any instruction can be emitted.
        for id in [20, 21] {
            for (offset, width) in [(0, 32), (7, 1)] {
                let p = decoded(&[
                    setreg(0x15, hwfield(20, 0, 32)), 0,
                    setreg(0x15, hwfield(21, 0, 32)), 1024,
                    0xbf82_0000, // branch to next instruction
                    setreg(0x12, hwfield(id, offset, width)), // unused GETREG result
                    0xbf81_0000,
                ]);
                assert!(matches!(plan(&p, 128, 0), Err(Stop::Fail(
                    ShaderError::Unsupported { what, .. })) if what.contains("scratch register read")));
            }
        }
        // All three vector addressing forms share Fields::Flat; none is
        // admitted, even when the loaded result has no consumers.
        for segment in [0, 1, 2] {
            let p = decoded(&[
                setreg(0x15, hwfield(20, 0, 32)), 0,
                0xdc00_0000 | 0x0c << 18 | segment << 14, 0,
                0xbf81_0000,
            ]);
            assert!(matches!(plan(&p, 128, 0), Err(Stop::Fail(
                ShaderError::Unsupported { what, .. })) if what.contains("flat or atomic memory")));
        }
        let p = decoded(&[
            setreg(0x15, hwfield(20, 0, 32)), 0,
            0xf400_0000 | 0x05 << 18, 125 << 25, // scalar scratch load, unused
            0xbf81_0000,
        ]);
        assert!(matches!(plan(&p, 128, 0), Err(Stop::Fail(
            ShaderError::Unsupported { what, .. })) if what.contains("scalar scratch")));
    }

    #[test]
    fn guest_schedule_rejects_nonliteral_partial_and_other_hardware_setters() {
        for (op, field, literal) in [
            (0x13, hwfield(20, 0, 32), false), // SGPR source
            (0x15, hwfield(20, 0, 31), true),
            (0x15, hwfield(21, 1, 31), true),
            (0x15, hwfield(1, 0, 32), true), // actual MODE
            (0x15, hwfield(25, 0, 32), true),
        ] {
            let mut words = vec![setreg(op, field)];
            if literal { words.push(0); }
            words.push(0xbf81_0000);
            let p = decoded(&words);
            assert!(plan(&p, 128, 0).is_err(), "op={op}, field={field}");
        }
    }

    #[test]
    fn guest_schedule_rejects_unmodelled_hardware_updates_without_ssa_results() {
        use crate::gfx10::{Encoding, Instruction};
        for (encoding, opcode) in [(Encoding::Sopk, 0x13), (Encoding::Sopk, 0x15),
            (Encoding::Sopp, 0x24), (Encoding::Sopp, 0x25)] {
            let mut p = graph(vec![Exit::End]);
            p.blocks[0].insts.push(Inst {
                machine: Instruction::synthetic(encoding, opcode),
                fields: if encoding == Encoding::Sopp { Fields::Sopp { imm: 0 } }
                    else { Fields::Sop { dst: None, src: vec![], imm: Some(0) } },
                reads: vec![], writes: vec![],
            });
            let Err(error) = plan(&p, 128, 0) else { panic!("hardware update must fail before emission") };
            let expected = if encoding == Encoding::Sopp { "MODE update" } else { "hardware-register update" };
            assert!(matches!(error, Stop::Fail(ShaderError::Unsupported { what,
                .. }) if what.contains(expected)));
        }
    }

    #[test]
    fn guest_schedule_rejects_unmodelled_cache_effects_and_calls_without_ssa_results() {
        use crate::gfx10::{Encoding, Instruction};
        for (encoding, opcode) in [(Encoding::Smem, 0x1f), (Encoding::Smem, 0x20),
            (Encoding::Smem, 0x21), (Encoding::Mubuf, 0x72), (Encoding::Sop1, 0x21)] {
            let mut p = graph(vec![Exit::End]);
            // The admission check is instruction-name based and must run
            // before normal lowering can discard an error as a dead result.
            p.blocks[0].insts.push(Inst {
                machine: Instruction::synthetic(encoding, opcode),
                fields: Fields::Sop { dst: None, src: vec![], imm: None },
                reads: vec![], writes: vec![],
            });
            let Err(error) = plan(&p, 128, 0) else { panic!("unmodelled side effect must fail before emission") };
            assert!(matches!(error, Stop::Fail(ShaderError::Unsupported { what,
                .. }) if what.contains("cache operation or indirect call")));
        }
    }
}
