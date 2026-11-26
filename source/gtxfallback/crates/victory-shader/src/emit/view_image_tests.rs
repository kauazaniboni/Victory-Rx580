//! A heap image descriptor loaded whole (8 dwords) and read only as data:
//! its width, height, levels, type and depth come from the host's view
//! addresses (`emit` "View addresses"), as the host view has them. In the
//! title's archive census (2026-11-25) "a descriptor read only as data" is
//! the top refusal; 872 `CS_RaytracingResolve` programs read a heap image's
//! width with `v_alignbit_b32` of dwords 1 and 2 and nothing else of it
//! (current upstream finds another load of theirs that a sample uses), and
//! 35 pixel programs with scalar shifts (dword 1 right by 30, dword 2 left
//! by 2) and its base level, with no other load of it.
//! Bits the view addresses do not hold (swizzle, tiling, metadata), dwords
//! read whole, and a 4-dword load (a buffer's record count) stay refused.
//! Hand-written GFX10 assembly.

use super::host::{Dispatch, Host, Root, View};
use super::{HostShader, lower, validator};
use crate::native::{
    ComputeIo, HwStage, NativeProgram, ResourceType, RootParam, RootType, StageIo, UsedResource,
};
use crate::{ShaderError, io, ir, resource, structure};

const S_ENDPGM: u32 = 0xBF81_0000;
const S_WAITCNT_LGKM: u32 = 0xBF8C_C07F;
const LITERAL: u8 = 0xFF;

const fn s_mov_b32(sdst: u8, src0: u8) -> u32 {
    0xBE80_0000 | (sdst as u32) << 16 | 0x03 << 8 | src0 as u32
}

/// SOP2 `op sdst, src0, src1`.
const fn sop2(op: u32, sdst: u8, src0: u8, src1: u8) -> u32 {
    0x8000_0000 | op << 23 | (sdst as u32) << 16 | (src1 as u32) << 8 | src0 as u32
}

/// VOP1 `v_mov_b32 vdst, src0`.
const fn v_mov_b32(vdst: u8, src0: u16) -> u32 {
    0x7E00_0000 | (vdst as u32) << 17 | 0x01 << 9 | (src0 as u32 & 0x1FF)
}

/// MUBUF `buffer_store_dword vdata, off, s[srsrc:srsrc+3], 0 offset:byte_off`.
const fn buffer_store_dword(vdata: u8, srsrc: u8, byte_off: u16) -> [u32; 2] {
    [
        0x38 << 26 | 0x1C << 18 | (byte_off as u32 & 0xFFF),
        0x80 << 24 | ((srsrc as u32 >> 2) & 0x1F) << 16 | (vdata as u32) << 8,
    ]
}

/// The descriptor's dwords land in s8 (dword 0) up.
const D: u8 = 8;

/// A read of the descriptor: its instruction words, the VGPR holding the
/// result (`v_mov`'d from `sdst` for scalar reads).
struct Probe {
    words: Vec<u32>,
    sdst: Option<u8>,
}

/// `v_alignbit_b32 v2, s10, s9, 30`: width - 1 in bits 0-13 (dword 1 bits
/// 30-31, dword 2 bits 0-11), as the title's resolve reads it.
fn width() -> Probe {
    let (hi, lo) = (u32::from(D + 2), u32::from(D + 1));
    Probe { words: vec![0xD54E_0002, (128 + 30) << 18 | lo << 9 | hi], sdst: None }
}

/// `s_bfe_u32 sdst, s(8 + dword), offset/width`.
fn bfe(sdst: u8, dword: u8, offset: u32, width: u32) -> Probe {
    Probe { words: vec![sop2(39, sdst, D + dword, LITERAL), width << 16 | offset], sdst: Some(sdst) }
}

/// `s_lshr_b32 sdst, s(8 + dword), by`.
fn lshr(sdst: u8, dword: u8, by: u8) -> Probe {
    Probe { words: vec![sop2(32, sdst, D + dword, 128 + by)], sdst: Some(sdst) }
}

/// `s_lshl_b32 sdst, s(8 + dword), by`.
fn lshl(sdst: u8, dword: u8, by: u8) -> Probe {
    Probe { words: vec![sop2(30, sdst, D + dword, 128 + by)], sdst: Some(sdst) }
}

/// `s_lshl_b32 sdst, s(8 + dword), s(8 + by)`: by a register.
fn lshl_by(sdst: u8, dword: u8, by: u8) -> Probe {
    Probe { words: vec![sop2(30, sdst, D + dword, D + by)], sdst: Some(sdst) }
}

/// `s_and_b32 sdst, s(8 + dword), mask`.
fn and(sdst: u8, dword: u8, mask: u32) -> Probe {
    Probe { words: vec![sop2(14, sdst, D + dword, LITERAL), mask], sdst: Some(sdst) }
}

/// `v_mov_b32 v(3+..), s(8 + dword)`: the dword whole.
fn whole(dword: u8) -> Probe {
    Probe { words: vec![], sdst: Some(D + dword) }
}

/// What the full program reads, in output order: width - 1 (with the rest
/// of dword 2 above it), height - 1, base level, last level, type, depth,
/// then the width's parts by scalar shifts (dword 1 right by 30, dword 2
/// left by 2).
fn all() -> Vec<Probe> {
    vec![
        width(),
        bfe(24, 2, 14, 14),
        bfe(25, 3, 12, 4),
        bfe(26, 3, 16, 4),
        lshr(27, 3, 28),
        and(28, 4, 0x1FFF),
        lshr(29, 1, 30),
        lshl(30, 2, 2),
    ]
}

/// Loads the descriptor at heap index s0 (8 dwords, or 4 without `x8`),
/// runs `probes` and stores their results at words 0 up of the UAV at
/// table entry 0 (table at s2).
fn native(x8: bool, probes: &[Probe]) -> NativeProgram {
    let mut w = vec![
        sop2(30, 4, 0, 128 + 5), // s_lshl_b32 s4, s0, 5 (heap index * 32 bytes)
        s_mov_b32(6, 1),         // s6: the heap address
        s_mov_b32(7, 128),
        if x8 { 0xF40C_0203 } else { 0xF408_0203 }, // s_load_dwordx8/x4 s[8:..], s[6:7], s4
        0x0800_0000,
        s_mov_b32(16, 2), // s16: the table address
        s_mov_b32(17, 128),
        0xF408_0508, // s_load_dwordx4 s[20:23], s[16:17], 0x0 (the UAV)
        0xFA00_0000,
        S_WAITCNT_LGKM,
    ];
    for p in probes {
        w.extend(&p.words);
    }
    for (n, p) in probes.iter().enumerate() {
        let v = 3 + n as u8;
        match p.sdst {
            Some(s) => w.push(v_mov_b32(v, u16::from(s))),
            None => w.push(0x7E00_0302 | u32::from(v) << 17), // v_mov_b32 v, v2
        }
        w.extend(buffer_store_dword(v, 20, 4 * n as u16));
    }
    w.push(S_ENDPGM);
    let param = |root, user_data| RootParam {
        root,
        table_offset_dwords: 0,
        sgprs: 1,
        base_register: 0,
        user_data,
        slot: user_data,
    };
    NativeProgram {
        stage: HwStage::Cs,
        wave64: false,
        vgprs: 16,
        layout: vec![
            param(RootType::Constants, 0),
            param(RootType::ResourceHeap, 1),
            param(RootType::Table, 2),
        ],
        used: vec![UsedResource {
            kind: ResourceType::Uav,
            root: RootType::Table,
            table_offset_dwords: 0,
            lower_bound: 0,
            stride: 0,
            dwords: 4,
            user_data: 2,
            slot: 2,
        }],
        io: StageIo::Cs(ComputeIo { rsrc2: 3 << 1, threads: [1, 1, 1] }),
        code: w.iter().flat_map(|w| w.to_le_bytes()).collect(),
    }
}

fn lower(native: &NativeProgram) -> Result<lower::Lowered, ShaderError> {
    let p = ir::build(&native.code, native.wave64, native.vgprs).expect("ir::build");
    let st = structure::structure(&p).expect("structure");
    let res = resource::track(&p, &native.layout, native.stage, Some(&st)).expect("track");
    let io = io::raise(&p, native).expect("io::raise");
    lower::lower(&lower::Parts {
        native,
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
}

#[track_caller]
fn refused(x8: bool, probes: &[Probe]) {
    match lower(&native(x8, probes)) {
        Err(ShaderError::Unsupported { what, .. }) => assert_eq!(what, "a descriptor read only as data"),
        Err(e) => panic!("unexpected error {e}"),
        Ok(_) => panic!("no host value for this read; it must stay refused"),
    }
}

#[test]
fn heap_image_size_levels_and_type_translate() {
    let l = lower(&native(true, &all())).expect("a heap image's size, levels and type read as data translate");
    assert!(l.view_addresses, "they come from the view addresses");
}

#[test]
fn heap_image_tiling_bits_stay_refused() {
    // Dword 3 bits 20-24: the swizzle mode, the console's tiling.
    refused(true, &[bfe(24, 3, 20, 5)]);
    // Dword 3 bits 0-11: the component swizzle.
    refused(true, &[and(24, 3, 0xFFF)]);
}

#[test]
fn heap_image_words_read_whole_stay_refused() {
    refused(true, &[whole(2)]);
    // Dword 2 bit 31, the resource level, by a shift that keeps it.
    refused(true, &[lshr(24, 2, 14)]);
    refused(true, &[lshl(24, 2, 0)]);
    // A shift by a register reads any bit.
    refused(true, &[lshl_by(24, 2, 0)]);
}

#[test]
fn heap_image_metadata_dwords_stay_refused() {
    refused(true, &[and(24, 5, 0xFF)]);
    refused(true, &[bfe(24, 6, 0, 8)]);
}

#[test]
fn four_dword_heap_loads_stay_refused() {
    // Dword 2 of a buffer descriptor is its record count, in its stride's
    // units, which the view addresses do not hold.
    refused(false, &[bfe(24, 2, 14, 14)]);
}

#[test]
#[ignore = "requires hardware, WARP and the DXIL validator; run explicitly"]
fn heap_image_words_read_from_the_view_addresses() {
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let l = lower(&native(true, &all())).expect("lower");
    let shader = HostShader {
        stage: HwStage::Cs,
        dxil: l.dxil,
        user_data: l.user_data,
        samplers: l.samplers,
        static_samplers: Vec::new(),
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
    validator::Validator::load(validator::Validator::default_path().as_deref())
        .expect("validator required")
        .validate(&mut shader.dxil.clone())
        .expect("must validate");
    assert!(!shader.memory && !shader.memory_writes && shader.view_addresses);
    // Heap index 1: a 1920x1080 2D array of 6 layers and 11 levels, as
    // `victory_gpu`'s `image_words` writes it; dword 0, dwords 5-7 and the
    // entries around it hold other values, which must not reach the program.
    let (w, h) = (1919u32, 1079u32);
    let d = [0x0FFF_FFFF, (w & 3) << 30, (w >> 2) | h << 14, 10 << 16 | 13 << 28, 5];
    let mut table = vec![0xDEAD_BEEFu32; 8 * 3];
    table[8..8 + 5].copy_from_slice(&d);
    let want = [d[1] >> 30 | d[2] << 2, h, 0, 10, 13, 5, d[1] >> 30, d[2] << 2];
    let mut ran = 0;
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        let (root, pso) = host.compute_pipeline(&shader).expect("pipeline");
        let mut ud = vec![0u32; shader.user_data_dwords().max(1) as usize];
        for u in &shader.user_data {
            // The heap index in s0; the heap and table at index 0.
            ud[u.index as usize] = u32::from(u.root == RootType::Constants);
        }
        let out = host
            .dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![(0, Root::Constants(ud)), (1, Root::View { srv: true, data: table.clone() })],
                    views: &[View::RawUav(8)],
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            )
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(out[0][..8], want, "{label}: width, height, base and last level, type, depth, width parts");
        eprintln!("{label}: heap image words {:?}", &out[0][..8]);
        ran += 1;
    }
    assert!(ran > 0, "no device ran the program");
}
