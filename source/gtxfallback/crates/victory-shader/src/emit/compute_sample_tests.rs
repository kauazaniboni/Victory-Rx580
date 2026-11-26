//! A compute program sampling with implicit derivatives (`image_sample`,
//! shader model 6.6 derivatives in compute) declares validator 1.8:
//! validators before it take such operations in pixel shaders only ("Entry
//! function performs some operation that is incompatible with the shader
//! stage", the title's archive census 2026-11-25: 735 compute programs;
//! DXC's own output fails the same way with `-validator-version 1.7`).
//! Hand-written GFX10 assembly.

use super::host::{Dispatch, Host, Root, View};
use super::validator::Validator;
use super::{HostShader, StaticSampler, lower};
use crate::native::{
    ComputeIo, HwStage, NativeProgram, ResourceType, RootParam, RootType, StageIo, UsedResource,
};
use crate::{io, ir, resource, structure};

/// User data: s0 a table with a texture, s2 a table with a raw UAV. Samples
/// the texture (implicit level, an immediate sampler) and stores red to
/// dword 0 of the UAV. `op`: the MIMG opcode word.
fn code(op: u32) -> Vec<u32> {
    vec![
        0xBE84_0300, // s_mov_b32 s4, s0
        0xBE85_0380, // s_mov_b32 s5, 0
        0xF40C_0202,
        0xFA00_0000, // s_load_dwordx8 s[8:15], s[4:5], 0x0
        0xBE86_0302, // s_mov_b32 s6, s2
        0xBE87_0380, // s_mov_b32 s7, 0
        0xF408_0403,
        0xFA00_0000, // s_load_dwordx4 s[16:19], s[6:7], 0x0
        0xBE94_0380, // s_mov_b32 s20, 0 (the sampler: wrap, point)
        0xBE95_0380, // s_mov_b32 s21, 0
        0xBE96_0380, // s_mov_b32 s22, 0
        0xBE97_0380, // s_mov_b32 s23, 0
        0x7E00_02FF,
        0x3FA0_0000, // v_mov_b32 v0, 1.25
        0x7E02_02FF,
        0x3F00_0000, // v_mov_b32 v1, 0.5
        0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
        op,
        0x00A2_0200, // image_sample(_lz) v2, v[0:1], s[8:15], s[20:23] dmask:0x1 dim:2D
        0xBF8C_3F70, // s_waitcnt vmcnt(0)
        0x7E06_0280, // v_mov_b32 v3, 0
        0xE070_1000,
        0x8004_0203, // buffer_store_dword v2, v3, s[16:19], 0 offen
        0xBF81_0000, // s_endpgm
    ]
}

/// `image_sample` (implicit derivatives) and `image_sample_lz` (level 0).
const IMAGE_SAMPLE: u32 = 0xF080_0108;
const IMAGE_SAMPLE_LZ: u32 = 0xF09C_0108;

fn lowered(op: u32) -> lower::Lowered {
    let param = |root, user_data| RootParam {
        root,
        table_offset_dwords: 0,
        sgprs: 1,
        base_register: 0,
        user_data,
        slot: user_data,
    };
    let entry = |kind, dwords, user_data| UsedResource {
        kind,
        root: RootType::Table,
        table_offset_dwords: 0,
        lower_bound: 0,
        stride: 0,
        dwords,
        user_data,
        slot: user_data,
    };
    let native = NativeProgram {
        stage: HwStage::Cs,
        wave64: false,
        vgprs: 8,
        layout: vec![param(RootType::Table, 0), param(RootType::Table, 2)],
        used: vec![
            entry(ResourceType::Srv, 8, 0),
            entry(ResourceType::Uav, 4, 2),
        ],
        io: StageIo::Cs(ComputeIo {
            rsrc2: 3 << 1,
            threads: [64, 1, 1],
        }),
        code: code(op).iter().flat_map(|w| w.to_le_bytes()).collect(),
    };
    let p = ir::build(&native.code, native.wave64, native.vgprs).expect("ir::build");
    let st = structure::structure(&p).expect("structure");
    let res = resource::track(&p, &native.layout, native.stage, Some(&st)).expect("track");
    let io = io::raise(&p, &native).expect("io::raise");
    lower::lower(&lower::Parts {
        native: &native,
        p: &p,
        st: &st,
        io: &io,
        res: &res,
        inputs: Vec::new(),
        outputs: Vec::new(),
        prims: Vec::new(),
        mesh_stride: 0,
        mesh_compute: false,
        wave64_dag: false,
        view_marks: false,
    })
    .expect("lower")
}

fn validate(op: u32) -> Result<(), String> {
    let mut l = lowered(op);
    let Some(path) = Validator::default_path() else {
        eprintln!("no dxil.dll; validation skipped");
        return Ok(());
    };
    Validator::load(Some(&path))
        .expect("validator")
        .validate(&mut l.dxil)
}

#[test]
fn a_compute_program_sampling_with_derivatives_validates() {
    validate(IMAGE_SAMPLE).expect("implicit derivatives in compute declare validator 1.8");
}

#[test]
fn a_compute_program_sampling_at_level_zero_validates() {
    validate(IMAGE_SAMPLE_LZ).expect("level-zero samples need no derivatives");
}

/// The sampling program runs on the host GPU and WARP: a pipeline of the
/// validator-1.8 container (runtime info 3) is made and samples as
/// expected (point, wrap: u = 1.25 reads texel 0).
#[test]
#[ignore = "requires hardware and WARP; run explicitly"]
fn a_compute_program_sampling_with_derivatives_runs() {
    let l = lowered(IMAGE_SAMPLE);
    let shader = HostShader {
        stage: HwStage::Cs,
        dxil: l.dxil,
        user_data: l.user_data,
        static_samplers: l
            .samplers
            .iter()
            .map(|&w| StaticSampler::from_words(w))
            .collect(),
        samplers: l.samplers,
        root_cbvs: l.root_cbvs,
        root_srvs: l.root_srvs,
        root_uavs: l.root_uavs,
        memory: l.memory,
        memory_writes: l.memory_writes,
        view_addresses: l.view_addresses,
        heap: l.heap,
        heap_writes: l.heap_writes,
        view_marks: l.view_marks,
    };
    let texels = [[1.0f32, 0.0, 0.0, 1.0], [2.0, 0.0, 0.0, 1.0]];
    let views = [View::Texture(2, 1, &texels), View::RawUav(1)];
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host
            .compute_pipeline(&shader)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        let mut ud = vec![0u32; shader.user_data_dwords().max(1) as usize];
        for u in &shader.user_data {
            // The texture at heap index 0, the UAV at 1 (32 bytes in).
            ud[u.index as usize] = if u.slot == 2 { 32 } else { 0 };
        }
        let words = host
            .dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![(0, Root::Constants(ud))],
                    views: &views,
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            )
            .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
        assert_eq!(f32::from_bits(words[0][0]), 1.0, "{label}");
        eprintln!(
            "{label}: a compute sample with derivatives reads {}",
            f32::from_bits(words[0][0])
        );
        ran += 1;
    }
    assert!(ran > 0, "neither hardware nor WARP SM 6.6 device available");
}
