//! Experimental mesh lowering: compute writes bounded records; a vertex
//! shader pulls triangle corners from them. No mesh hardware is used.
//! The native program, resource bindings and LDS lowering stay the same.

use super::bitcode::{Attr, Bin, Cast, Term, V};
use super::container::SigElement;
use super::shader::{Shader, flags};
use super::HostShader;
use crate::{Container, FourCC, HwStage, ShaderError};

/// Compute output is root UAV u0 in this reserved space.
pub const SPACE: u32 = 3;
/// Vertex input is root SRV t254 in ROOT_CBV_SPACE (outside title slots).
pub const VERTEX_SRV: u8 = 254;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub vertices: u32,
    pub primitives: u32,
    pub vertex_stride: u32,
    pub primitive_stride: u32,
}

impl Layout {
    pub fn vertex_base(self) -> u32 { self.primitives * 12 }
    pub fn primitive_base(self) -> u32 { self.vertex_base() + self.vertices * self.vertex_stride }
    pub fn bytes(self) -> u32 { self.primitive_base() + self.primitives * self.primitive_stride }
    /// Bounded scratch for one dispatch. Reject before allocating or recording work.
    pub fn allocation(self, groups: u32) -> Option<u64> {
        let bytes = u64::from(self.bytes()).checked_mul(u64::from(groups))?;
        (groups > 0 && groups <= 65535 && bytes <= 16 * 1024 * 1024).then_some(bytes)
    }
}

pub struct Fallback {
    pub compute: HostShader,
    pub vertex: HostShader,
    pub layout: Layout,
}

pub fn translate(bytes: &[u8]) -> Result<Fallback, ShaderError> {
    translate_with(bytes, false, super::Options::default())
}

/// Experimental, checked common-schedule lowering for eligible wave64 mesh
/// programs. Unsupported programs return an error instead of half-wave values.
pub fn translate_wave64_dag(bytes: &[u8]) -> Result<Fallback, ShaderError> {
    translate_with(bytes, true, super::Options::default())
}

pub fn translate_with(bytes: &[u8], wave64_dag: bool, options: super::Options) -> Result<Fallback, ShaderError> {
    let c = Container::parse(bytes)?;
    let part = c.part(FourCC::DXIL).ok_or(ShaderError::Malformed("no DXIL part"))?;
    let native = crate::native_programs(part.data)?.into_iter().find(|n| n.stage == HwStage::Ms)
        .ok_or(ShaderError::Malformed("no mesh program"))?;
    let crate::native::StageIo::Ms(ms) = &native.io else { unreachable!() };
    if ms.threads == 0 || ms.max_vertices == 0 || ms.max_primitives == 0 {
        return Err(ShaderError::Malformed("empty mesh limits"));
    }
    let (p, st) = crate::range::build(&native.patched_code(), native.wave64, native.vgprs)?;
    let res = crate::resource::track(&p, &native.layout, native.stage, st.as_ref().ok())?;
    let io = crate::io::raise(&p, &native)?;
    let st = st?;
    let original = super::emit(&c, &native, &p, &st, &io, &res)?;
    let out = Container::parse(&original.dxil)?;
    let outputs = super::signature(out.part(FourCC::OSG1).unwrap().data)?;
    let prims = out.part(FourCC::PSG1).map(|p| super::signature(p.data)).transpose()?.unwrap_or_default();
    let layout = Layout { vertices: ms.max_vertices, primitives: ms.max_primitives,
        vertex_stride: outputs.len() as u32 * 16, primitive_stride: prims.len() as u32 * 16 };
    if ms.threads == 0 || layout.vertices == 0 || layout.primitives == 0 || prims.iter().any(|e| e.kind != 0) {
        return Err(ShaderError::Unsupported { offset: 0, what: "mesh fallback limits or primitive system values" });
    }
    let vertex = vertex_shader(layout, outputs, prims);
    let compute = super::emit_mode(&c, &native, &p, &st, &io, &res, options, true, wave64_dag && native.wave64)?;
    Ok(Fallback { compute, vertex, layout })
}

impl Shader {
    fn mesh_layout(&self) -> Layout {
        let m = self.mesh.expect("mesh lowering");
        Layout { vertices: m.max_vertices, primitives: m.max_primitives,
            vertex_stride: self.outputs.len() as u32 * 16, primitive_stride: self.prims.len() as u32 * 16 }
    }

    fn mesh_output_handle(&mut self) -> V {
        if let Some(h) = self.mesh_uav { return h; }
        let args = [self.m.i32(0), self.m.i32(0), self.m.i32(SPACE), self.m.i8(1)];
        let bind = self.m.aggregate(self.t.bind, &args);
        let z = self.m.i32(0);
        let nonuniform = self.m.bool(false);
        let h = self.op(self.entry, "createHandleFromBinding", Attr::ReadNone, self.t.handle, 217, &[bind, z, nonuniform]);
        let h = self.annotate(self.entry, h, 11 | 0x1000, 0);
        self.mesh_uav = Some(h);
        self.flags |= flags::RAW_BUFFERS;
        h
    }

    fn mesh_address(&mut self, block: usize, index: V, stride: u32, offset: u32) -> V {
        let zero = self.m.i32(0);
        let group = self.op(block, "groupId.i32", Attr::ReadNone, self.t.i32, 94, &[zero]);
        let size = self.m.i32(self.mesh_layout().bytes());
        let group = self.m.bin(block, Bin::Mul, group, size);
        let stride = self.m.i32(stride);
        let index = self.m.bin(block, Bin::Mul, index, stride);
        let offset = self.m.i32(offset);
        let at = self.m.bin(block, Bin::Add, group, index);
        self.m.bin(block, Bin::Add, at, offset)
    }

    fn mesh_word(&mut self, block: usize, address: V, value: V) {
        let h = self.mesh_output_handle();
        let undef = self.m.undef(self.t.i32);
        let mask = self.m.i8(1);
        let align = self.m.i32(4);
        self.op(block, "rawBufferStore.i32", Attr::NoUnwind, self.t.void, 140,
            &[h, address, undef, value, undef, undef, undef, mask, align]);
    }

    pub(super) fn mesh_indices(&mut self, block: usize, primitive: V, indices: [V; 3]) {
        for (corner, value) in indices.into_iter().enumerate() {
            let at = self.mesh_address(block, primitive, 12, corner as u32 * 4);
            self.mesh_word(block, at, value);
        }
    }

    pub(super) fn mesh_store(&mut self, block: usize, primitive: bool, element: u32, column: u32, index: V, value: V) {
        let layout = self.mesh_layout();
        let (base, stride) = if primitive { (layout.primitive_base(), layout.primitive_stride) }
            else { (layout.vertex_base(), layout.vertex_stride) };
        let at = self.mesh_address(block, index, stride, base + element * 16 + column * 4);
        self.mesh_word(block, at, value);
    }
}

fn load(s: &mut Shader, handle: V, address: V) -> V {
    let result = s.m.named_struct("dx.types.ResRet.i32", vec![s.t.i32; 5]);
    let undef = s.m.undef(s.t.i32);
    let mask = s.m.i8(1);
    let align = s.m.i32(4);
    let r = s.op(s.entry, "rawBufferLoad.i32", Attr::ReadOnly, result, 139, &[handle, address, undef, mask, align]);
    s.m.extract(s.entry, r, 0)
}

fn vertex_shader(layout: Layout, outputs: Vec<SigElement>, prims: Vec<SigElement>) -> HostShader {
    let mut s = Shader::new(false);
    s.flags |= flags::RAW_BUFFERS;
    let b = s.entry;
    let input = |name: &str, kind, row| SigElement { name: name.into(), index: 0, kind, comp: 5,
        interp: 0, row: Some(row), col: 0, cols: 1, usage: 1, half: false };
    s.inputs = vec![input("SV_VertexID", 1, 0), input("SV_InstanceID", 2, 1)];
    let mut ids = Vec::new();
    for id in 0..2 {
        let args = [s.m.i32(id), s.m.i32(0), s.m.i8(0), s.m.undef(s.t.i32)];
        ids.push(s.op(b, "loadInput.i32", Attr::ReadNone, s.t.i32, 4, &args));
    }
    let three = s.m.i32(3);
    let primitive = s.m.bin(b, Bin::UDiv, ids[0], three);
    let record_size = s.m.i32(layout.bytes());
    let record = s.m.bin(b, Bin::Mul, ids[1], record_size);
    let handle = s.root_srv_handle(VERTEX_SRV);
    let twelve = s.m.i32(12);
    let at = s.m.bin(b, Bin::Mul, primitive, twelve);
    let at = s.m.bin(b, Bin::Add, record, at);
    let max = s.m.i32(layout.vertices);
    let zero = s.m.i32(0);
    let mut valid = s.m.bool(true);
    let mut corners = Vec::new();
    for k in 0..3 {
        let k = s.m.i32(k * 4);
        let addr = s.m.bin(b, Bin::Add, at, k);
        let v = load(&mut s, handle, addr);
        let in_range = s.m.cmp(b, super::bitcode::pred::IULT, v, max);
        valid = s.m.bin(b, Bin::And, valid, in_range);
        for &previous in &corners {
            let distinct = s.m.cmp(b, super::bitcode::pred::NE, v, previous);
            valid = s.m.bin(b, Bin::And, valid, distinct);
        }
        corners.push(v);
    }
    let corner = s.m.bin(b, Bin::URem, ids[0], three);
    let one = s.m.i32(1);
    let first = s.m.cmp(b, super::bitcode::pred::EQ, corner, zero);
    let second = s.m.cmp(b, super::bitcode::pred::EQ, corner, one);
    let vertex = s.m.select(b, second, corners[1], corners[2]);
    let vertex = s.m.select(b, first, corners[0], vertex);
    // Validate the whole primitive, not just this corner, before pulling vertices.
    let vertex = s.m.select(b, valid, vertex, zero);
    let nvertex = outputs.len();
    let primitive_row = outputs.iter().filter_map(|e| e.row).max().map_or(0, |r| r + 1);
    for (id, mut e) in outputs.into_iter().chain(prims).enumerate() {
        let is_primitive = id >= nvertex;
        let (idx, stride, offset) = if is_primitive {
            (primitive, layout.primitive_stride, layout.primitive_base() + (id - nvertex) as u32 * 16)
        } else { (vertex, layout.vertex_stride, layout.vertex_base() + id as u32 * 16) };
        // Keep the vertex signature's register and component packing:
        // D3D12 links it to the original PS at those locations. Record
        // columns and storeOutput operands stay relative to the element.
        // Mesh primitive outputs have a separate register namespace; put
        // them after the vertex rows, preserving their own packing.
        if is_primitive {
            e.row = e.row.map(|r| primitive_row + r);
        }
        e.interp = if e.kind == 3 { 4 } else if is_primitive || e.comp != 9 { 1 } else { 2 };
        e.usage = (1 << e.cols) - 1;
        for col in 0..e.cols {
            let k = s.m.i32(stride);
            let addr = s.m.bin(b, Bin::Mul, idx, k);
            let addr = s.m.bin(b, Bin::Add, record, addr);
            let k = s.m.i32(offset + u32::from(col) * 4);
            let addr = s.m.bin(b, Bin::Add, addr, k);
            let bits = load(&mut s, handle, addr);
            let empty = s.m.i32(if e.kind == 3 && col == 3 { 1.0f32.to_bits() } else { 0 });
            let bits = s.m.select(b, valid, bits, empty);
            let (suffix, value) = if e.comp == 9 { ("f32", s.m.cast(b, Cast::Bitcast, bits, s.t.f32)) } else { ("i32", bits) };
            let args = [s.m.i32(id as u32), s.m.i32(0), s.m.i8(col), value];
            s.op(b, &format!("storeOutput.{suffix}"), Attr::NoUnwind, s.t.void, 5, &args);
        }
        s.outputs.push(e);
    }
    s.m.terminate(b, Term::Ret);
    HostShader { stage: HwStage::Vs, dxil: s.finish(), user_data: vec![], samplers: vec![], static_samplers: vec![], root_cbvs: vec![], root_srvs: vec![VERTEX_SRV], root_uavs: vec![], memory: false, memory_writes: false, view_addresses: false, heap: false, heap_writes: false, view_marks: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position() -> SigElement {
        SigElement { name: "SV_Position".into(), index: 0, kind: 3, comp: 9, interp: 4,
            row: Some(0), col: 0, cols: 4, usage: 15, half: false }
    }

    /// Own synthetic RDNA2 program: lane 0/1/2 emits a full-screen
    /// triangle. Other lanes must not write outside the vertex record.
    fn native_triangle() -> HostShader {
        use crate::native::{NativeProgram, StageIo, MeshIo};
        let vop1 = |op: u32, dst: u32, src: u32| 0x7e00_0000 | dst << 17 | op << 9 | src;
        let vop2 = |op: u32, dst: u32, src: u32, vsrc: u32| op << 25 | dst << 17 | vsrc << 9 | src;
        let mut words = vec![0xd765_0000, 193 | 128 << 9]; // mbcnt_lo v0, -1, 0
        words.push(vop2(0x16, 1, 129, 0)); // x = lane >> 1
        words.push(vop2(0x1b, 2, 129, 0)); // y = lane & 1
        for dst in [1, 2] {
            words.push(vop1(6, dst, 256 + dst)); // cvt_f32_u32
            words.extend([vop2(8, dst, 255, dst), 4.0f32.to_bits()]);
            words.extend([vop2(3, dst, 255, dst), (-1.0f32).to_bits()]);
        }
        words.extend([vop1(1, 3, 255), 0.5f32.to_bits()]);
        words.extend([vop1(1, 4, 255), 1.0f32.to_bits()]);
        words.extend([vop1(1, 6, 255), (1 << 10) | (2 << 20)]);
        words.extend([0xf800_00cf, 0x0403_0201]); // exp pos0, v1, v2, v3, v4
        words.extend([0xf800_0941, 6]); // exp prim, v6 done
        words.push(0xbf81_0000);
        let native = NativeProgram { stage: HwStage::Ms, wave64: false, vgprs: 8,
            layout: vec![], used: vec![], io: StageIo::Ms(MeshIo { outputs: vec![],
                clip_cntl: 0, threads: 32, max_vertices: 3, max_primitives: 1,
                topology: 2, lds_bytes: 0 }),
            code: words.into_iter().flat_map(u32::to_le_bytes).collect() };
        let p = crate::ir::build(&native.code, native.wave64, native.vgprs).unwrap();
        let st = crate::structure::structure(&p).unwrap();
        let res = crate::resource::track(&p, &native.layout, native.stage, Some(&st)).unwrap();
        let io = crate::io::raise(&p, &native).unwrap();
        let lowered = super::super::lower::lower(&super::super::lower::Parts {
            native: &native, p: &p, st: &st, io: &io, res: &res,
            inputs: vec![], outputs: vec![position()], prims: vec![],
            mesh_stride: 0, view_marks: false, mesh_compute: true, wave64_dag: false,
        }).unwrap();
        HostShader { stage: HwStage::Cs, dxil: lowered.dxil, user_data: lowered.user_data,
            samplers: lowered.samplers, static_samplers: vec![],
            root_cbvs: lowered.root_cbvs, root_srvs: lowered.root_srvs, root_uavs: lowered.root_uavs, memory: lowered.memory, memory_writes: lowered.memory_writes,
            view_addresses: lowered.view_addresses, heap: lowered.heap, heap_writes: lowered.heap_writes, view_marks: lowered.view_marks }
    }

    /// A high-half guest-wave election determines the local vertex IDs.
    /// Outputs remain indexed by the physical group thread, so the records
    /// exercise upper lanes rather than silently reusing vertices 0..2.
    fn native_wave64_triangle(threads: u32, wrong_election: bool) -> (HostShader, Layout, u32) {
        native_wave64_triangle_with_scratch(threads, wrong_election, false)
    }

    fn native_wave64_triangle_with_scratch(threads: u32, wrong_election: bool, dead_scratch: bool) -> (HostShader, Layout, u32) {
        native_wave64_triangle_with_flags(threads, wrong_election, dead_scratch, 0)
    }

    fn native_wave64_triangle_with_flags(threads: u32, wrong_election: bool, dead_scratch: bool, primitive_flags: u32) -> (HostShader, Layout, u32) {
        use crate::native::{NativeProgram, StageIo, MeshIo};
        assert!(matches!(threads, 64 | 128));
        let base = threads - 64 + 40;
        let layout = Layout {
            vertices: base + 3, primitives: base + 1,
            vertex_stride: 16, primitive_stride: 0,
        };
        let vop1 = |op: u32, dst: u32, src: u32| 0x7e00_0000 | dst << 17 | op << 9 | src;
        let vop2 = |op: u32, dst: u32, src: u32, vsrc: u32| op << 25 | dst << 17 | vsrc << 9 | src;
        let sop2 = |op: u32, dst: u32, a: u32, b: u32| 0x8000_0000 | op << 23 | dst << 16 | b << 8 | a;
        let mut words = vec![
            0xbefe_04c1, // full guest EXEC before defining all thread IDs
            0xd765_0000, 193 | 128 << 9 | 128 << 18,
            0xd766_0000, 193 | 256 << 9 | 128 << 18,
            sop2(0x27, 4, 3, 255), 0x0004_0018, // guest wave index
            sop2(0x1e, 4, 4, 134),
            vop2(0x25, 0, 4, 0), // group thread, including wave 1
        ];
        words.extend([0x7c00_0000 | 0xd3 << 17 | 255, base]); // base <= v0
        words.extend([0x7c00_0000 | 0xd6 << 17 | 255, base + 2]); // base+2 >= v0
        words.push(vop1(2, 0, 256)); // s0 = first active group thread
        if wrong_election {
            // Negative control: choose the start of the physical 32-lane
            // half instead of guest lane 40. All three vertices move offscreen.
            words.push(sop2(0x01, 0, 0, 136)); // s0 -= 8
        }
        words.push(vop2(0x27, 0, 0, 0)); // local vertex = group thread - s0
        words.push(vop2(0x16, 1, 129, 0));
        words.push(vop2(0x1b, 2, 129, 0));
        for dst in [1, 2] {
            words.push(vop1(6, dst, 256 + dst));
            words.extend([vop2(8, dst, 255, dst), 4.0f32.to_bits()]);
            words.extend([vop2(3, dst, 255, dst), (-1.0f32).to_bits()]);
        }
        words.extend([vop1(1, 3, 255), 0.5f32.to_bits()]);
        words.extend([vop1(1, 4, 255), 1.0f32.to_bits()]);
        words.extend([vop1(1, 6, 255), base | (base + 1) << 10 | (base + 2) << 20 | primitive_flags]);
        words.extend([0xf800_00cf, 0x0403_0201]); // exp pos0
        words.extend([0xf800_0941, 6]); // only thread base is a bounded primitive
        words.push(0xbf81_0000);
        if dead_scratch {
            // Own synthetic full-width FLAT_SCRATCH_LO/HI literal setup.
            // The geometry program has no scratch access or GETREG observer.
            let setreg = |id: u32| 0xb000_0000u32 | 0x15 << 23 | 31 << 11 | id;
            let mut setup = vec![setreg(20), 0, setreg(21), 1024];
            setup.extend(words);
            words = setup;
        }
        let native = NativeProgram {
            stage: HwStage::Ms, wave64: true, vgprs: 8, layout: vec![], used: vec![],
            io: StageIo::Ms(MeshIo {
                outputs: vec![], clip_cntl: 0, threads,
                max_vertices: layout.vertices, max_primitives: layout.primitives,
                topology: 2, lds_bytes: 0,
            }),
            code: words.into_iter().flat_map(u32::to_le_bytes).collect(),
        };
        let p = crate::ir::build(&native.code, native.wave64, native.vgprs).unwrap();
        let st = crate::structure::structure(&p).unwrap();
        let res = crate::resource::track(&p, &native.layout, native.stage, Some(&st)).unwrap();
        let io = crate::io::raise(&p, &native).unwrap();
        let lowered = super::super::lower::lower(&super::super::lower::Parts {
            native: &native, p: &p, st: &st, io: &io, res: &res,
            inputs: vec![], outputs: vec![position()], prims: vec![],
            mesh_stride: 0, view_marks: false, mesh_compute: true, wave64_dag: true,
        }).expect("high-half triangle through guest-wave DAG");
        let shader = HostShader {
            stage: HwStage::Cs, dxil: lowered.dxil, user_data: lowered.user_data,
            samplers: lowered.samplers, static_samplers: vec![],
            root_cbvs: lowered.root_cbvs, root_srvs: lowered.root_srvs,
            root_uavs: lowered.root_uavs, memory: lowered.memory, memory_writes: lowered.memory_writes,
            view_addresses: lowered.view_addresses, heap: lowered.heap, heap_writes: lowered.heap_writes, view_marks: lowered.view_marks,
        };
        (shader, layout, base)
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "opt-in guest-wave mesh records and pixels hardware diagnostic; run explicitly"]
    fn mesh_compute_wave64_dag_high_half_records_and_pixels() {
        check_wave64_triangle_records_and_pixels(false);
    }

    #[test]
    fn mesh_compute_wave64_dead_scratch_preserves_lowered_geometry() {
        // CPU-only bytecode equality proves the admitted setup does not
        // change either the working geometry or its offscreen negative control.
        for threads in [64, 128] {
            for wrong in [false, true] {
                let (base, layout, base_id) = native_wave64_triangle(threads, wrong);
                let (admitted, after, after_id) = native_wave64_triangle_with_scratch(threads, wrong, true);
                assert_eq!(base.dxil, admitted.dxil, "threads={threads}, wrong-election={wrong}");
                assert_eq!(layout.bytes(), after.bytes());
                assert_eq!(base_id, after_id);
            }
        }
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "opt-in dead scratch setup records and pixels diagnostic; run explicitly"]
    fn mesh_compute_wave64_dead_scratch_records_and_pixels() {
        check_wave64_triangle_records_and_pixels(true);
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "opt-in primitive edge flags and null primitive GPU diagnostic; run explicitly"]
    fn mesh_compute_wave64_primitive_edge_flags_records_and_pixels() {
        // Own generated primitive exports. Mesa's build_export_prim documents
        // 9-bit indices and edge bits 9, 19, 29; bit 31 culls the primitive.
        // Candidate identified by the read-only Claude Opus 5.5 review.
        for flags in [1 << 9, 1 << 19, 1 << 29, (1 << 9) | (1 << 19) | (1 << 29)] {
            for null in [0, 1 << 31] {
                check_wave64_triangle_records_and_pixels_with_flags(false, flags | null);
            }
        }
    }

    #[cfg(windows)]
    fn check_wave64_triangle_records_and_pixels(dead_scratch: bool) {
        check_wave64_triangle_records_and_pixels_with_flags(dead_scratch, 0);
    }

    #[cfg(windows)]
    fn check_wave64_triangle_records_and_pixels_with_flags(dead_scratch: bool, primitive_flags: u32) {
        use super::super::host::{Draw, Host, MeshComputeDraw};
        use windows::Win32::Graphics::Direct3D12::D3D12_SHADER_VISIBILITY_ALL;

        let mut validator = super::super::validator::Validator::load(
            super::super::validator::Validator::default_path().as_deref(),
        ).expect("DXIL validator");
        let mut pixel = Shader::new(true);
        pixel.outputs = vec![SigElement { name: "SV_Target".into(), kind: 16, interp: 0, ..position() }];
        let color = [1.0f32, 0.25, 0.5, 1.0];
        for (col, value) in color.into_iter().enumerate() {
            let args = [pixel.m.i32(0), pixel.m.i32(0), pixel.m.i8(col as u8), pixel.m.f32(value.to_bits())];
            pixel.op(pixel.entry, "storeOutput.f32", Attr::NoUnwind, pixel.t.void, 5, &args);
        }
        pixel.m.terminate(pixel.entry, Term::Ret);
        let mut ps = super::super::host::stub_ps();
        ps.dxil = pixel.finish();
        validator.validate(&mut ps.dxil).expect("fixture pixel shader");
        let gpu = Host::new(false).expect("SM 6.6 GPU");
        for threads in [64, 128] {
            for wrong_election in [false, true] {
                let (mut cs, layout, base) = native_wave64_triangle_with_flags(threads, wrong_election, dead_scratch, primitive_flags);
                assert!(layout.allocation(2).unwrap() <= 6 * 1024);
                let mut vs = vertex_shader(layout, vec![position()], vec![]);
                validator.validate(&mut cs.dxil).expect("DAG triangle compute shader");
                validator.validate(&mut vs.dxil).expect("DAG triangle pulling vertex shader");
                let (root, pipeline) = gpu.pipeline(&vs, &ps).expect("DAG triangle VS/PS linkage");
                let cs_root = gpu.root_signature_for_with_mesh_output(
                    &[(&cs, D3D12_SHADER_VISIBILITY_ALL)], true,
                ).unwrap();
                let cs_pipeline = gpu.compute_pipeline_with(&cs_root, &cs.dxil).unwrap();
                let draw = Draw {
                    roots: vec![], textures: &[], samplers: &[], vertices: &[], stride: 0,
                    size: (8, 8), mesh_groups: 0,
                };
                let fallback = MeshComputeDraw {
                    root: &cs_root, pipeline: &cs_pipeline, layout, groups: 2,
                    uav_param: 1, srv_param: 2,
                };
                let (pixels, records) = gpu.draw_with_mesh_compute_readback(
                    &root, &pipeline, &draw, &fallback,
                ).unwrap();
                let record_words = layout.bytes() as usize / 4;
                assert_eq!(records.len(), record_words * 2);
                for group in 0..2usize {
                    let record = &records[group * record_words..(group + 1) * record_words];
                    for primitive in 0..layout.primitives {
                        let offset = primitive as usize * 3;
                        let expected = if primitive == base && primitive_flags & (1 << 31) == 0 { [base, base + 1, base + 2] } else { [0; 3] };
                        assert_eq!(&record[offset..offset + 3], &expected,
                            "threads={threads}, wrong-election={wrong_election}, group={group}, primitive={primitive}");
                    }
                    let positions = if wrong_election {
                        [[15.0f32, -1.0, 0.5, 1.0], [15.0, 3.0, 0.5, 1.0], [19.0, -1.0, 0.5, 1.0]]
                    } else {
                        [[-1.0f32, -1.0, 0.5, 1.0], [-1.0, 3.0, 0.5, 1.0], [3.0, -1.0, 0.5, 1.0]]
                    };
                    for (vertex, expected) in positions.into_iter().enumerate() {
                        let offset = layout.vertex_base() as usize / 4 + (base as usize + vertex) * 4;
                        assert_eq!(&record[offset..offset + 4], &expected.map(f32::to_bits),
                            "threads={threads}, wrong-election={wrong_election}, group={group}, vertex={vertex}");
                    }
                    // Other vertex rows were never exported and are intentionally
                    // not asserted. All primitive rows are initialized by the prologue.
                }
                let expected_pixel = if wrong_election || primitive_flags & (1 << 31) != 0 { [0.0; 4] } else { color };
                assert_eq!(pixels.len(), 64);
                assert!(pixels.iter().all(|p| *p == expected_pixel),
                    "threads={threads}, wrong-election={wrong_election}: {pixels:?}");
            }
        }
    }

    /// Own synthetic wave64 mesh program for the guest-wave DAG: guest lane
    /// `t` (group thread) reads texel (t & 7, (t >> 3) & 7) of the texture
    /// in view-heap slot 0 (table in s8) with `image_load`, or with
    /// `image_sample_lz` through the sampler in slot 0 (table in s9) at the
    /// texel centre, and exports the four words as its position. With
    /// `partial`, only lanes t < 40 read (a guest EXEC crossing the 32-lane
    /// half NVIDIA's wave32 splits it at); the others keep -1.0.
    fn native_wave64_image(threads: u32, sample: bool, partial: bool) -> (HostShader, Layout) {
        use crate::native::{MeshIo, NativeProgram, ResourceType, RootParam, RootType, StageIo, UsedResource};
        let layout = Layout { vertices: threads, primitives: threads, vertex_stride: 16, primitive_stride: 0 };
        let vop1 = |op: u32, dst: u32, src: u32| 0x7e00_0000 | dst << 17 | op << 9 | src;
        let vop2 = |op: u32, dst: u32, src: u32, vsrc: u32| op << 25 | dst << 17 | vsrc << 9 | src;
        let sop2 = |op: u32, dst: u32, a: u32, b: u32| 0x8000_0000 | op << 23 | dst << 16 | b << 8 | a;
        let mut words = vec![
            0xbefe_04c1, // full guest EXEC
            0xd765_0000, 193 | 128 << 9 | 128 << 18,
            0xd766_0000, 193 | 256 << 9 | 128 << 18,
            sop2(0x27, 4, 3, 255), 0x0004_0018, // guest wave index
            sop2(0x1e, 4, 4, 134),
            vop2(0x25, 0, 4, 0), // v0: group thread
            0xbe8a_0308, 0xbe8b_0380, // s[10:11] = view table (s8)
            0xf40c_0405, 0xfa00_0000, // s_load_dwordx8 s[16:23], s[10:11], 0
            0xbe8c_0309, 0xbe8d_0380, // s[12:13] = sampler table (s9)
            0xf408_0606, 0xfa00_0000, // s_load_dwordx4 s[24:27], s[12:13], 0
            0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
            vop2(0x1b, 1, 135, 0), // v1 = t & 7
            vop2(0x16, 2, 131, 0), // v2 = t >> 3
            vop2(0x1b, 2, 135, 2), // v2 &= 7
        ];
        if sample {
            for v in [1, 2] {
                words.push(vop1(6, v, 256 + v)); // v_cvt_f32_u32
                words.push(vop2(3, v, 240, v)); // + 0.5
                words.extend([vop2(8, v, 255, v), 0.125f32.to_bits()]); // / 8
            }
        }
        for v in 4..8 {
            words.push(vop1(1, v, 243)); // -1.0: what an inactive lane keeps
        }
        if partial {
            words.extend([0x7c00_0000 | 0xd4 << 17 | 255, 40]); // v_cmpx_gt_u32 40, v0
        }
        if sample {
            // image_sample_lz v[4:7], v[1:2], s[16:23], s[24:27] dmask 0xf 2D
            words.extend([0xf000_0008 | 0x27 << 18 | 0xf << 8, 1 | 4 << 8 | 4 << 16 | 6 << 21]);
        } else {
            // image_load v[4:7], v[1:2], s[16:23] dmask 0xf 2D unorm
            words.extend([0xf000_0008 | 0xf << 8 | 1 << 12, 1 | 4 << 8 | 4 << 16]);
        }
        words.extend([
            0xbf8c_3f70, // s_waitcnt vmcnt(0)
            0xbefe_04c1, // full guest EXEC again
            vop1(1, 3, 128), // v3 = 0: a degenerate primitive per lane
            0xf800_00cf, 0x0706_0504, // exp pos0 v4, v5, v6, v7
            0xf800_0941, 3, // exp prim v3 done
            0xbf81_0000,
        ]);
        let table = |user_data| RootParam { root: RootType::Table, table_offset_dwords: 0, sgprs: 1,
            base_register: 0, user_data, slot: user_data };
        let used = |kind, dwords, user_data| UsedResource { kind, root: RootType::Table, table_offset_dwords: 0,
            lower_bound: 0, stride: 0, dwords, user_data, slot: user_data };
        let mut used_resources = vec![used(ResourceType::Srv, 8, 0)];
        if sample {
            used_resources.push(used(ResourceType::Sampler, 4, 1));
        }
        let native = NativeProgram {
            stage: HwStage::Ms, wave64: true, vgprs: 8,
            layout: vec![table(0), table(1)], used: used_resources,
            io: StageIo::Ms(MeshIo {
                outputs: vec![], clip_cntl: 0, threads,
                max_vertices: layout.vertices, max_primitives: layout.primitives,
                topology: 2, lds_bytes: 0,
            }),
            code: words.into_iter().flat_map(u32::to_le_bytes).collect(),
        };
        let p = crate::ir::build(&native.code, native.wave64, native.vgprs).unwrap();
        let st = crate::structure::structure(&p).unwrap();
        let res = crate::resource::track(&p, &native.layout, native.stage, Some(&st)).unwrap();
        let io = crate::io::raise(&p, &native).unwrap();
        let lowered = super::super::lower::lower(&super::super::lower::Parts {
            native: &native, p: &p, st: &st, io: &io, res: &res,
            inputs: vec![], outputs: vec![position()], prims: vec![],
            mesh_stride: 0, view_marks: false, mesh_compute: true, wave64_dag: true,
        }).expect("image reads through the guest-wave DAG");
        let shader = HostShader {
            stage: HwStage::Cs, dxil: lowered.dxil, user_data: lowered.user_data,
            samplers: lowered.samplers, static_samplers: vec![],
            root_cbvs: lowered.root_cbvs, root_srvs: lowered.root_srvs,
            root_uavs: lowered.root_uavs, memory: lowered.memory, memory_writes: lowered.memory_writes,
            view_addresses: lowered.view_addresses, heap: lowered.heap, heap_writes: lowered.heap_writes, view_marks: lowered.view_marks,
        };
        (shader, layout)
    }

    #[test]
    fn mesh_compute_wave64_dag_image_reads_translate() {
        for threads in [64, 128] {
            for sample in [false, true] {
                for partial in [false, true] {
                    let (cs, layout) = native_wave64_image(threads, sample, partial);
                    eprintln!("image bindings threads={threads} sample={sample} partial={partial}: user={:?} srv={:?} uav={:?} memory={} dirty={} addresses={} marks={}", cs.user_data, cs.root_srvs, cs.root_uavs, cs.memory, cs.memory_writes, cs.view_addresses, cs.view_marks);
                    assert!(!cs.dxil.is_empty());
                    assert!(layout.allocation(2).is_some());
                }
            }
        }
    }

    /// The texel words lane `t` reads, per [`native_wave64_image`]: every
    /// texel distinct in each channel.
    fn texel(t: u32) -> [f32; 4] {
        let i = ((t >> 3) & 7) * 8 + (t & 7);
        [100.0 + i as f32, -(100.0 + i as f32), 0.25 * i as f32, 1000.0 + i as f32]
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "requires hardware, WARP and the DXIL validator; run explicitly"]
    fn mesh_compute_wave64_dag_image_reads_return_texels() {
        use super::super::host::{Draw, Host, MeshComputeDraw, Root};
        use windows::Win32::Graphics::Direct3D12::*;
        let _one = super::super::valu_tests::ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let mut validator = super::super::validator::Validator::load(
            super::super::validator::Validator::default_path().as_deref(),
        ).expect("DXIL validator");
        // Row-major 8x8: texel (x, y) is lane y * 8 + x's.
        let texels: Vec<[f32; 4]> = (0..64).map(texel).collect();
        let sampler = D3D12_SAMPLER_DESC {
            Filter: D3D12_FILTER_MIN_MAG_MIP_POINT,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            MaxAnisotropy: 1,
            MaxLOD: 15.0,
            ..Default::default()
        };
        let mut ps = super::super::host::stub_ps();
        validator.validate(&mut ps.dxil).expect("stub pixel shader");
        for (label, warp) in [("hardware", false), ("warp", true)] {
            let gpu = Host::new(warp).expect("SM 6.6 device");
            for threads in [64, 128] {
                for sample in [false, true] {
                    for partial in [false, true] {
                        let what = format!("{label}: threads={threads}, sample={sample}, partial={partial}");
                        let (mut cs, layout) = native_wave64_image(threads, sample, partial);
                        let mut vs = vertex_shader(layout, vec![position()], vec![]);
                        validator.validate(&mut cs.dxil).expect("DAG image compute shader");
                        validator.validate(&mut vs.dxil).expect("pulling vertex shader");
                        let (root, pipeline) = gpu.pipeline(&vs, &ps).expect("VS/PS linkage");
                        let cs_root = gpu.root_signature_for_with_mesh_output(
                            &[(&cs, D3D12_SHADER_VISIBILITY_ALL)], true,
                        ).unwrap();
                        let cs_pipeline = gpu.compute_pipeline_with(&cs_root, &cs.dxil).unwrap();
                        let draw = Draw {
                            roots: vec![], textures: &[(8, 8, texels.as_slice())], samplers: &[sampler],
                            vertices: &[], stride: 0, size: (8, 8), mesh_groups: 0,
                        };
                        // Current upstream also declares the view-address table,
                        // including for descriptor dwords unused as ordinary data.
                        // Scratch follows that table in the root signature.
                        assert!(cs.root_cbvs.is_empty() && cs.root_srvs.is_empty() && cs.root_uavs.is_empty());
                        assert!(!cs.memory && !cs.memory_writes && !cs.view_marks);
                        let fallback = MeshComputeDraw {
                            root: &cs_root, pipeline: &cs_pipeline, layout, groups: 2,
                            uav_param: 1 + u32::from(cs.view_addresses), srv_param: 2,
                        };
                        let user = vec![0u32; cs.user_data_dwords().max(1) as usize];
                        let mut roots = vec![(0, Root::Constants(user))];
                        if cs.view_addresses {
                            // Textures/samplers have no title buffer address.
                            roots.push((1, Root::View { srv: true, data: vec![0; 8] }));
                        }
                        let (_, records) = gpu.draw_with_mesh_compute_readback_roots(
                            &root, &pipeline, &draw, &fallback, &roots,
                        ).expect(&what);
                        let words = layout.bytes() as usize / 4;
                        assert_eq!(records.len(), words * 2, "{what}");
                        for group in 0..2usize {
                            let record = &records[group * words..(group + 1) * words];
                            for t in 0..threads {
                                let expected = if partial && t >= 40 { [-1.0f32; 4] } else { texel(t) };
                                let at = layout.vertex_base() as usize / 4 + t as usize * 4;
                                assert_eq!(&record[at..at + 4], &expected.map(f32::to_bits),
                                    "{what}, group={group}, lane={t}");
                            }
                        }
                        eprintln!("{what}: {threads} lanes x 2 groups exact");
                    }
                }
            }
        }
    }

    #[test]
    fn mesh_compute_containers_validate() {
        let mut s = Shader::new(false);
        s.mesh_compute = true;
        s.mesh = Some(super::super::shader::Mesh { max_vertices: 3, max_primitives: 1, topology: 2 });
        s.outputs = vec![position()];
        s.threads = Some([1, 1, 1]);
        let b = s.entry;
        let zero = s.m.i32(0);
        let indices = [zero, s.m.i32(1), s.m.i32(2)];
        s.mesh_indices(b, zero, indices);
        for (v, xy) in [[-1.0f32, -1.0], [-1.0, 3.0], [3.0, -1.0]].iter().enumerate() {
            for (c, x) in [xy[0], xy[1], 0.5, 1.0].into_iter().enumerate() {
                let value = s.m.i32(x.to_bits());
                let index = s.m.i32(v as u32);
                s.mesh_store(b, false, 0, c as u32, index, value);
            }
        }
        s.m.terminate(b, Term::Ret);
        let cs = s.finish();
        let layout = Layout { vertices: 3, primitives: 1, vertex_stride: 16, primitive_stride: 0 };
        let vs = vertex_shader(layout, vec![position()], vec![]);
        #[cfg(windows)] {
            let mut validator = super::super::validator::Validator::load(super::super::validator::Validator::default_path().as_deref()).expect("DXIL validator");
            validator.validate(&mut cs.clone()).expect("compute output shader");
            validator.validate(&mut vs.dxil.clone()).expect("vertex pulling shader");
            let mut pixel = Shader::new(true);
            pixel.outputs = vec![SigElement { name: "SV_Target".into(), kind: 16, interp: 0, ..position() }];
            for (col, value) in [1.0f32, 0.25, 0.5, 1.0].into_iter().enumerate() {
                let args = [pixel.m.i32(0), pixel.m.i32(0), pixel.m.i8(col as u8), pixel.m.f32(value.to_bits())];
                pixel.op(pixel.entry, "storeOutput.f32", Attr::NoUnwind, pixel.t.void, 5, &args);
            }
            pixel.m.terminate(pixel.entry, Term::Ret);
            let mut ps = super::super::host::stub_ps();
            ps.dxil = pixel.finish();
            let mut cs_host = ps.clone();
            cs_host.stage = HwStage::Cs;
            cs_host.dxil = cs;
            use windows::Win32::Graphics::Direct3D12::D3D12_SHADER_VISIBILITY_ALL;
            let draw = super::super::host::Draw { roots: vec![], textures: &[], samplers: &[], vertices: &[], stride: 0, size: (8, 8), mesh_groups: 0 };
            let mut shaders = [("hand-written DXIL", cs_host), ("native RDNA2 lowered to compute", native_triangle())];
            validator.validate(&mut ps.dxil).expect("fallback pixel shader");
            for (name, shader) in &mut shaders {
                validator.validate(&mut shader.dxil).expect(name);
            }
            for (label, warp) in [("hardware", false), ("WARP", true)] {
                let Some(gpu) = available_test_gpu(warp) else { continue };
                let (root, pipeline) = gpu.pipeline(&vs, &ps).expect("fallback VS pipeline");
                for (name, shader) in &shaders {
                    let cs_root = gpu.root_signature_for_with_mesh_output(&[(shader, D3D12_SHADER_VISIBILITY_ALL)], true).unwrap();
                    let cs_pipeline = gpu.compute_pipeline_with(&cs_root, &shader.dxil).unwrap();
                    let fallback = super::super::host::MeshComputeDraw { root: &cs_root, pipeline: &cs_pipeline,
                        layout, groups: 2, uav_param: 1, srv_param: 2 };
                    let pixels = gpu.draw_with_mesh_compute(&root, &pipeline, &draw, Some(&fallback)).unwrap();
                    assert!(pixels.iter().all(|p| *p == [1.0, 0.25, 0.5, 1.0]), "{label}, {name}: {pixels:?}");
                    eprintln!("{label}, {name}: {} pixels exact", pixels.len());
                }
            }
        }
        assert_eq!(layout.bytes(), 60);
        assert_eq!(layout.allocation(2), Some(120));
        assert_eq!(layout.allocation(0), None);
        assert_eq!(layout.allocation(u32::MAX), None);
    }

    /// Hosted CI may have no hardware adapter, or a WARP device below SM 6.6.
    /// Validation must run before this probe, and available-device pipeline,
    /// draw and readback failures must still fail the test.
    #[cfg(windows)]
    fn available_test_gpu(warp: bool) -> Option<super::super::host::Host> {
        let label = if warp { "WARP" } else { "hardware" };
        match super::super::host::Host::new(warp) {
            Ok(gpu) => Some(gpu),
            Err(error) if error == "no hardware adapter" || error.starts_with("the device supports SM ") => {
                eprintln!("{label} mesh draw skipped: {error}; shader validation still ran");
                None
            }
            Err(error) => panic!("{label} device initialization failed: {error}"),
        }
    }

    fn packed_elements(reverse: bool, mixed: bool) -> Vec<SigElement> {
        let mut elements: Vec<_> = (0..4).map(|col| SigElement {
            name: "PACKED".into(), index: u32::from(col), kind: 0,
            comp: if mixed && col % 2 == 1 { 5 } else { 9 },
            interp: if mixed { 1 } else { 2 }, row: Some(1), col, cols: 1,
            usage: 1, half: false,
        }).collect();
        if reverse { elements.reverse(); }
        elements
    }

    #[cfg(windows)]
    fn validate_packed_fallback(reverse: bool, mixed: bool) {
        let mut outputs = vec![position()];
        outputs.extend(packed_elements(reverse, mixed));
        let layout = Layout { vertices: 3, primitives: 1,
            vertex_stride: outputs.len() as u32 * 16, primitive_stride: 0 };
        let mut vs = vertex_shader(layout, outputs, vec![]);
        let mut validator = super::super::validator::Validator::load(
            super::super::validator::Validator::default_path().as_deref(),
        ).expect("DXIL validator");
        validator.validate(&mut vs.dxil).expect("packed fallback VS must validate");
    }

    #[test]
    #[cfg(windows)]
    fn mesh_compute_reversed_packed_signature_validates() {
        validate_packed_fallback(true, false);
    }

    #[test]
    #[cfg(windows)]
    fn mesh_compute_mixed_packed_signature_validates() {
        validate_packed_fallback(false, true);
    }

    /// Four distinct semantics share one output register. The pulling VS
    /// must keep their component masks so the existing PS can link and
    /// read each value, including elements that begin at y, z and w.
    #[test]
    #[cfg(windows)]
    fn mesh_compute_packed_semantics_link_and_draw() {
        for warp in [false, true] {
            for reverse in [false, true] {
                for mixed in [false, true] {
                    check_packed_semantics(warp, reverse, mixed);
                }
            }
        }
    }

    #[cfg(windows)]
    fn check_packed_semantics(warp: bool, reverse: bool, mixed: bool) {
        let packed = packed_elements(reverse, mixed);
        let mut outputs = vec![position()];
        outputs.extend(packed.clone());
        let layout = Layout {
            vertices: 3, primitives: 1,
            vertex_stride: outputs.len() as u32 * 16, primitive_stride: 0,
        };
        let expected = if mixed { [0.125f32, 2.0, 0.5, 4.0] }
            else { [0.125f32, 0.25, 0.5, 1.0] };
        let mut cs = Shader::new(false);
        cs.mesh_compute = true;
        cs.mesh = Some(super::super::shader::Mesh {
            max_vertices: 3, max_primitives: 1, topology: 2,
        });
        cs.outputs = outputs.clone();
        cs.threads = Some([1, 1, 1]);
        let b = cs.entry;
        let zero = cs.m.i32(0);
        let indices = [zero, cs.m.i32(1), cs.m.i32(2)];
        cs.mesh_indices(b, zero, indices);
        for (vertex, xy) in [[-1.0f32, -1.0], [-1.0, 3.0], [3.0, -1.0]].iter().enumerate() {
            let vertex = cs.m.i32(vertex as u32);
            for (col, value) in [xy[0], xy[1], 0.5, 1.0].into_iter().enumerate() {
                let value = cs.m.i32(value.to_bits());
                cs.mesh_store(b, false, 0, col as u32, vertex, value);
            }
            for (id, element) in packed.iter().enumerate() {
                let value = expected[element.index as usize];
                let value = cs.m.i32(if element.comp == 5 { value as u32 } else { value.to_bits() });
                // Record columns are relative to each signature element;
                // packing into the signature's row happens in the VS.
                cs.mesh_store(b, false, id as u32 + 1, 0, vertex, value);
            }
        }
        cs.m.terminate(b, Term::Ret);
        let mut cs_host = super::super::host::stub_ps();
        cs_host.stage = HwStage::Cs;
        cs_host.dxil = cs.finish();
        let mut vs = vertex_shader(layout, outputs, vec![]);

        let mut pixel = Shader::new(true);
        pixel.inputs = packed;
        pixel.outputs = vec![SigElement {
            name: "SV_Target".into(), kind: 16, interp: 0, ..position()
        }];
        for id in 0..4 {
            let element = &pixel.inputs[id];
            let (col, integer) = (element.index as u8, element.comp == 5);
            let args = [pixel.m.i32(id as u32), pixel.m.i32(0), pixel.m.i8(0), pixel.m.undef(pixel.t.i32)];
            let value = if integer {
                let value = pixel.op(pixel.entry, "loadInput.i32", Attr::ReadNone, pixel.t.i32, 4, &args);
                pixel.m.cast(pixel.entry, Cast::UiToFp, value, pixel.t.f32)
            } else {
                pixel.op(pixel.entry, "loadInput.f32", Attr::ReadNone, pixel.t.f32, 4, &args)
            };
            let args = [pixel.m.i32(0), pixel.m.i32(0), pixel.m.i8(col), value];
            pixel.op(pixel.entry, "storeOutput.f32", Attr::NoUnwind, pixel.t.void, 5, &args);
        }
        pixel.m.terminate(pixel.entry, Term::Ret);
        let mut ps = super::super::host::stub_ps();
        ps.dxil = pixel.finish();
        let mut validator = super::super::validator::Validator::load(
            super::super::validator::Validator::default_path().as_deref(),
        ).expect("DXIL validator");
        validator.validate(&mut cs_host.dxil).expect("packed fixture compute shader");
        validator.validate(&mut vs.dxil).expect("packed fixture pulling vertex shader");
        validator.validate(&mut ps.dxil).expect("packed fixture pixel shader");
        let Some(gpu) = available_test_gpu(warp) else { return };
        let (root, pipeline) = gpu.pipeline(&vs, &ps).expect("packed fallback VS/PS linkage");
        use windows::Win32::Graphics::Direct3D12::D3D12_SHADER_VISIBILITY_ALL;
        let cs_root = gpu.root_signature_for_with_mesh_output(
            &[(&cs_host, D3D12_SHADER_VISIBILITY_ALL)], true,
        ).unwrap();
        let cs_pipeline = gpu.compute_pipeline_with(&cs_root, &cs_host.dxil).unwrap();
        let draw = super::super::host::Draw {
            roots: vec![], textures: &[], samplers: &[], vertices: &[], stride: 0,
            size: (8, 8), mesh_groups: 0,
        };
        let fallback = super::super::host::MeshComputeDraw {
            root: &cs_root, pipeline: &cs_pipeline, layout, groups: 2,
            uav_param: 1, srv_param: 2,
        };
        let pixels = gpu.draw_with_mesh_compute(&root, &pipeline, &draw, Some(&fallback)).unwrap();
        assert!(pixels.iter().all(|p| *p == expected),
            "warp={warp}, reverse={reverse}, mixed={mixed}: {pixels:?}");
        eprintln!("packed fallback pixels exact: warp={warp}, reverse={reverse}, mixed={mixed}, pixels={}", pixels.len());
    }
}
