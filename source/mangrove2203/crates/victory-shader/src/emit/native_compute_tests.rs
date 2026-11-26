//! Synthetic native-compute election oracles. No title data. GPU tests use
//! bounded output buffers and run explicitly, serialized with game trials.
use crate::native::{ComputeIo, StageIo};
use crate::{Container, HwStage, NativeProgram, ResourceType, RootParam, RootType, UsedResource};
use crate::{emit, io, ir, resource, structure};
use emit::host::{Dispatch, Host, Root, View};

const THREADS: u32 = 1024;
const fn mov(s: u8, v: u8) -> u32 {
    0xbe80_0000 | (s as u32) << 16 | 3 << 8 | v as u32
}
const fn vop1(op: u16, d: u8, s: u16) -> u32 {
    0x7e00_0000 | (d as u32) << 17 | (op as u32) << 9 | s as u32
}

fn shader(wave64: bool) -> emit::HostShader {
    let words = [
        mov(1, 128), // s1=0; table pointer already in s0
        0x3d << 26 | 2 << 18 | 4 << 6,
        125 << 25,          // load output descriptor into s4:s7
        vop1(2, 8, 256),    // s8 = first active lane's v0 (local thread ID)
        vop1(1, 1, 8),      // v1 = s8
        (0x1a << 25) | 130, // v0 = v0 << 2, for bounded byte-address stores
        0x38 << 26 | 0x1c << 18 | 1 << 12,
        0x80 << 24 | 1 << 16 | 1 << 8, // store v1 at v0 through s4:s7
        0xbf81_0000,
    ];
    let n = NativeProgram {
        stage: HwStage::Cs,
        wave64,
        vgprs: 4,
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
        io: StageIo::Cs(ComputeIo {
            rsrc2: 1 << 1,
            threads: [THREADS, 1, 1],
        }),
        code: words.into_iter().flat_map(u32::to_le_bytes).collect(),
    };
    let p = ir::build(&n.code, n.wave64, n.vgprs).expect("synthetic IR");
    let names: Vec<_> = p
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .map(|i| i.name())
        .collect();
    assert_eq!(
        names
            .iter()
            .filter(|&&s| s == "v_readfirstlane_b32")
            .count(),
        1
    );
    assert_eq!(names.iter().filter(|&&s| s == "v_lshlrev_b32").count(), 1);
    assert_eq!(
        names.iter().filter(|&&s| s == "buffer_store_dword").count(),
        1
    );
    let st = structure::structure(&p).expect("structure");
    let r = resource::track(&p, &n.layout, n.stage, Some(&st)).expect("resource tracking");
    assert!(
        r.waterfall.reads.is_empty(),
        "election is not a descriptor waterfall"
    );
    let io = io::raise(&p, &n).expect("IO");
    // An empty signature container suffices for an original synthetic CS.
    let mut header = [0u8; 32];
    header[..4].copy_from_slice(b"DXBC");
    header[20..24].copy_from_slice(&1u32.to_le_bytes());
    header[24..28].copy_from_slice(&32u32.to_le_bytes());
    let c = Container::parse(&header).expect("empty signature container");
    let mut shader = emit::emit(&c, &n, &p, &st, &io, &r).expect("emit");
    let mut validator =
        emit::validator::Validator::load(emit::validator::Validator::default_path().as_deref())
            .expect("DXIL validator");
    validator
        .validate(&mut shader.dxil)
        .expect("DXIL validation");
    shader
}

fn run(host: &Host, wave64: bool) -> usize {
    let shader = shader(wave64);
    let (root, pipeline) = host.compute_pipeline(&shader).expect("compute pipeline");
    let results = host
        .dispatch(
            &root,
            &pipeline,
            &Dispatch {
                roots: vec![(
                    0,
                    Root::Constants(vec![0; shader.user_data_dwords().max(1) as usize]),
                )],
                views: &[View::RawUav(THREADS)],
                samplers: &[],
                groups: [1, 1, 1],
            },
        )
        .expect("bounded dispatch");
    let words = &results[0];
    assert_eq!(words.len(), THREADS as usize);
    let width = if wave64 { 64 } else { 32 };
    let wrong = words
        .iter()
        .enumerate()
        .filter(|(i, v)| **v != (*i as u32 / width) * width)
        .count();
    let samples: Vec<_> = [0, 31, 32, 63, 64, 95, 96, 127, 1023]
        .into_iter()
        .map(|i| (i, words[i], (i as u32 / width) * width))
        .collect();
    println!(
        "NATIVE_COMPUTE wave64={wave64} threads={THREADS} mismatches={wrong} samples_lane_actual_expected={samples:?}"
    );
    wrong
}

#[test]
#[ignore = "hardware GPU execution; serialize with game trials"]
fn first_lane_election_spans_a_console_wave() {
    let host = Host::new(false).expect("hardware D3D12 device");
    assert_eq!(run(&host, false), 0, "wave32 fixture control");
    assert_eq!(
        run(&host, true),
        0,
        "native wave64 compute must elect across both host-wave halves"
    );
}

const fn sop2(op: u8, d: u8, a: u8, b: u8) -> u32 {
    0x8000_0000 | (op as u32) << 23 | (d as u32) << 16 | (b as u32) << 8 | a as u32
}
const fn vop2(op: u32, d: u32, a: u32, b: u32) -> u32 {
    op << 25 | d << 17 | b << 9 | a
}
fn branch(words: &mut [u32], at: usize, to: usize) {
    let relative = i16::try_from(to as isize - at as isize - 1).unwrap();
    words[at] = (words[at] & 0xffff_0000) | u32::from(relative as u16);
}

fn native(words: &[u32], dims: [u32; 3]) -> NativeProgram {
    NativeProgram {
        stage: HwStage::Cs,
        wave64: true,
        vgprs: 8,
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
        io: StageIo::Cs(ComputeIo {
            rsrc2: (1 << 1) | (1 << 7) | (2 << 11),
            threads: dims,
        }),
        code: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
    }
}

fn analyze(
    n: &NativeProgram,
) -> (
    ir::Program,
    structure::Structured,
    resource::Resources,
    io::Io,
    bool,
) {
    let p = ir::build(&n.code, n.wave64, n.vgprs).unwrap();
    let st = structure::structure(&p).unwrap();
    let res = resource::track(&p, &n.layout, n.stage, Some(&st)).unwrap();
    let io = io::raise(&p, n).unwrap();
    let proved = super::lower::native_compute::full_group_elections(&super::lower::Parts {
        native: n,
        p: &p,
        st: &st,
        io: &io,
        res: &res,
        inputs: vec![],
        outputs: vec![],
        prims: vec![],
        mesh_stride: 0,
        mesh_compute: false,
        wave64_dag: false, view_marks: false,
    });
    (p, st, res, io, proved)
}

fn compile(n: &NativeProgram) -> emit::HostShader {
    let (p, st, res, io, proved) = analyze(n);
    assert!(
        proved,
        "whole-group control flow must be proven before dispatch"
    );
    let mut header = [0u8; 32];
    header[..4].copy_from_slice(b"DXBC");
    header[20..24].copy_from_slice(&1u32.to_le_bytes());
    header[24..28].copy_from_slice(&32u32.to_le_bytes());
    let c = Container::parse(&header).unwrap();
    let mut out = emit::emit(&c, n, &p, &st, &io, &res).unwrap();
    emit::validator::Validator::load(emit::validator::Validator::default_path().as_deref())
        .unwrap()
        .validate(&mut out.dxil)
        .unwrap();
    out
}

fn loop_fixture(dims: [u32; 3], iterations: u8) -> Vec<u32> {
    assert!((1..=64).contains(&iterations));
    assert!(dims[0].is_power_of_two() && dims[1].is_power_of_two());
    let mut w = vec![
        mov(12, 1), // save group ID x before zeroing the descriptor high word
        mov(1, 128),
        0x3d << 26 | 2 << 18 | 4 << 6,
        125 << 25,
        // Reconstruct flattened group ID from x/y/z; no host-wave mapping.
        vop2(0x1a, 1, 128 + dims[0].trailing_zeros(), 1),
        vop2(0x1a, 2, 128 + (dims[0] * dims[1]).trailing_zeros(), 2),
        vop2(0x25, 0, 257, 0),
        vop2(0x25, 0, 258, 0),
        mov(9, 128),
        vop1(1, 3, 128),
        sop2(0, 13, 12, 128 + iterations),
    ];
    let head = w.len();
    w.extend([
        sop2(0x1e, 10, 9, 138), // scalar iteration * 1024
        vop2(0x25, 1, 10, 0),
        vop1(2, 8, 257), // election #1
        vop2(0x25, 3, 8, 3),
        vop2(0x25, 1, 145, 1), // source now has a different value at same site
        vop1(2, 11, 257),      // election #2 must not see old scratch
        vop2(0x25, 3, 11, 3),
        0xbf8a_0000,        // native s_barrier: reached uniformly by the entire group
        sop2(0, 9, 9, 129), // iteration++
        0xbf00_0000 | 0xa << 16 | 13 << 8 | 9,
    ]);
    let back = w.len();
    w.push(0xbf85_0000);
    branch(&mut w, back, head);
    // Disjoint outputs for two groups, with two guard words at each end.
    w.extend([
        sop2(
            0x1e,
            12,
            12,
            128 + (dims.into_iter().product::<u32>()).trailing_zeros() as u8,
        ),
        vop2(0x25, 0, 12, 0),
        vop2(0x25, 0, 130, 0),
        vop2(0x1a, 0, 130, 0),
        0x38 << 26 | 0x1c << 18 | 1 << 12,
        0x80 << 24 | 1 << 16 | 3 << 8,
        0xbf81_0000,
    ]);
    w
}

#[test]
#[ignore = "GPU execution on hardware and WARP; serialize with game trials"]
fn repeated_elections_and_uniform_loops_preserve_each_guest_wave() {
    for warp in [false, true] {
        let host = Host::new(warp).expect("D3D12 device");
        for dims in [
            [64, 1, 1],
            [128, 1, 1],
            [256, 1, 1],
            [1024, 1, 1],
            [32, 8, 4],
        ] {
            let iterations = 47u8;
            let n = native(&loop_fixture(dims, iterations), dims);
            let shader = compile(&n);
            let (root, pipeline) = host.compute_pipeline(&shader).unwrap();
            let threads = dims.into_iter().product::<u32>();
            for repeat in 0..3 {
                let results = host
                    .dispatch(
                        &root,
                        &pipeline,
                        &Dispatch {
                            roots: vec![(
                                0,
                                Root::Constants(vec![0; shader.user_data_dwords().max(1) as usize]),
                            )],
                            views: &[View::RawUav(threads * 2 + 4)],
                            samplers: &[],
                            groups: [2, 1, 1],
                        },
                    )
                    .unwrap();
                let words = &results[0];
                assert_eq!(words.len(), (threads * 2 + 4) as usize);
                assert_eq!(&words[..2], &[0, 0]);
                assert_eq!(&words[words.len() - 2..], &[0, 0]);
                for (index, &actual) in words[2..words.len() - 2].iter().enumerate() {
                    let base = (index as u32 % threads) / 64 * 64;
                    let n = u32::from(iterations) + index as u32 / threads;
                    let expected = 2 * n * base + 1024 * n * (n - 1) + 17 * n;
                    assert_eq!(
                        actual, expected,
                        "warp={warp} dims={dims:?} repeat={repeat} word={index}"
                    );
                }
            }
            println!(
                "NATIVE_LOOP warp={warp} dims={dims:?} groups=2 iterations=47/48 repeats=3 all words and guards correct"
            );
        }
    }
}

#[test]
fn unsafe_native_control_flow_does_not_gain_group_barriers() {
    let dims = [1024, 1, 1];
    let words = loop_fixture(dims, 4);
    assert!(analyze(&native(&words, dims)).4);
    // A scalar branch based on a first-lane read can differ between guest
    // waves even though it is uniform within each wave. Never dispatch it
    // through a whole-group exchange, including on a loop back edge.
    let mut wave_branch = words.clone();
    let compare = wave_branch
        .iter()
        .position(|w| w & 0xffff_0000 == 0xbf0a_0000)
        .unwrap();
    wave_branch[compare] = (wave_branch[compare] & !255) | 8;
    assert!(!analyze(&native(&wave_branch, dims)).4);
    let mut loaded_branch = words.clone();
    loaded_branch[compare] = (loaded_branch[compare] & !255) | 4;
    assert!(
        !analyze(&native(&loaded_branch, dims)).4,
        "memory loads are not uniformity evidence"
    );
    // An early exit decided by that wave-specific scalar is also unsafe.
    let mut early = vec![vop1(2, 8, 256), 0xbf06_8008, 0xbf85_0001, 0xbf81_0000];
    early.extend(&words);
    assert!(!analyze(&native(&early, dims)).4);
    // Explicit EXEC updates require a different proof, even a full-mask reset.
    let mut masked = vec![0xbefe_04c1];
    masked.extend(&words);
    assert!(!analyze(&native(&masked, dims)).4);
    assert!(!analyze(&native(&words, [96, 1, 1])).4);
    // Guest LDS plus reusable scratch must fit the shared-memory budget.
    let mut too_much = native(&words, dims);
    if let StageIo::Cs(ref mut cs) = too_much.io {
        cs.rsrc2 |= 64 << 15;
    }
    assert!(!analyze(&too_much).4);
}

#[test]
fn many_election_sites_fit_within_the_shared_memory_budget() {
    let mut words = vec![
        mov(1, 128),
        0x3d << 26 | 2 << 18 | 4 << 6,
        125 << 25,
        vop1(1, 2, 256),
    ];
    for _ in 0..40 {
        words.extend([vop1(2, 8, 256), vop2(0x25, 0, 8, 0)]);
    }
    words.extend([
        vop1(1, 1, 8),
        vop2(0x1a, 0, 130, 2),
        0x38 << 26 | 0x1c << 18 | 1 << 12,
        0x80 << 24 | 1 << 16 | 1 << 8,
        0xbf81_0000,
    ]);
    // Validation only. Forty distinct sites would exceed 32 KiB if each
    // reserved scratch; the output index remains the original bounded ID.
    let shader = compile(&native(&words, [1024, 1, 1]));
    assert!(!shader.dxil.is_empty());
}
