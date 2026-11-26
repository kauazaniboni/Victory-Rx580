//! A program outside the pixel and compute stages whose module declares a
//! UAV declares DXIL's "UAVs at every stage" (shader flag bit 16, feature
//! 0x4), as the validator recomputes it from the module's UAVs. In the
//! title's archive census (2026-11-25), 867 vertex programs (the vertex
//! buffer transforms, storing through heap UAVs with write marks: the marks
//! UAV `u1, space2` is the module's) failed validation with "Flags must
//! match usage" (declared 0x40100010, actual 0x40110010). Compute programs
//! must not declare it. Hand-written GFX10 assembly.

use super::lower;
use super::validator::Validator;
use crate::native::{
    ComputeIo, HwStage, NativeProgram, ResourceType, RootParam, RootType, StageIo, UsedResource,
};
use crate::{io, ir, resource, structure};

/// Stores 1.0 to word 0 of the UAV at table entry 0 (table address in s0).
const CODE: &[u32] = &[
    0xBE82_0300, // s_mov_b32 s2, s0
    0xBE83_0380, // s_mov_b32 s3, 0
    0xF408_0201,
    0xFA00_0000, // s_load_dwordx4 s[8:11], s[2:3], 0x0 (UAV)
    0xBF8C_C07F, // s_waitcnt lgkmcnt(0)
    0x7E02_02F2, // v_mov_b32 v1, 1.0
    0x7E10_0280, // v_mov_b32 v8, 0
    0xE070_1000,
    0x8002_0108, // buffer_store_dword v1, v8, s[8:11], 0 offen
    0xBF81_0000, // s_endpgm
];

fn program(stage: HwStage) -> NativeProgram {
    NativeProgram {
        stage,
        wave64: false,
        vgprs: 16,
        layout: vec![RootParam {
            root: RootType::Table,
            table_offset_dwords: 0,
            sgprs: 1,
            base_register: 0,
            user_data: 0,
            slot: 0,
        }],
        used: vec![UsedResource {
            kind: ResourceType::Uav,
            root: RootType::Table,
            table_offset_dwords: 0,
            lower_bound: 0,
            stride: 0,
            dwords: 4,
            user_data: 0,
            slot: 0,
        }],
        io: match stage {
            HwStage::Cs => StageIo::Cs(ComputeIo {
                rsrc2: 1 << 1,
                threads: [64, 1, 1],
            }),
            _ => StageIo::Vs {
                inputs: vec![],
                outputs: vec![],
            },
        },
        code: CODE.iter().flat_map(|w| w.to_le_bytes()).collect(),
    }
}

/// The program lowered (with write marks: `marks`), its DXIL container
/// validated.
fn validate(stage: HwStage, marks: bool) -> Result<(), String> {
    let native = program(stage);
    let p = ir::build(&native.code, native.wave64, native.vgprs).expect("ir::build");
    let st = structure::structure(&p).expect("structure");
    let res = resource::track(&p, &native.layout, native.stage, Some(&st)).expect("track");
    let io = io::raise(&p, &native).expect("io::raise");
    let mut l = lower::lower(&lower::Parts {
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
        view_marks: marks,
    })
    .expect("lower");
    assert_eq!(l.view_marks, marks, "write marks: the module's marks UAV");
    let Some(path) = Validator::default_path() else {
        eprintln!("no dxil.dll; validation skipped");
        return Ok(());
    };
    Validator::load(Some(&path))
        .expect("validator")
        .validate(&mut l.dxil)
}

#[test]
fn a_vertex_program_with_write_marks_validates() {
    validate(HwStage::Vs, true).expect("a vertex program's marks UAV declares UAVs at every stage");
}

#[test]
fn a_vertex_program_writing_heap_uavs_only_validates() {
    validate(HwStage::Vs, false).expect("heap UAVs alone are no UAVs of the module");
}

#[test]
fn a_compute_program_with_write_marks_validates() {
    validate(HwStage::Cs, true).expect("compute programs do not declare UAVs at every stage");
}
