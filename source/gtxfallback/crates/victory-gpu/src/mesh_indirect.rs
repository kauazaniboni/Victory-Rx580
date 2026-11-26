//! Opt-in compute/vertex execution of one GPU-produced mesh argument.
//! Uses upstream shared-buffer state tracking and the submission work budget.
use windows::Win32::Graphics::Direct3D::{Fxc::D3DCompile, ID3DBlob};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
use windows::core::s;
use crate::host::{HostList, host, transition};

pub(crate) const SCRATCH_LIMIT: u64 = 4 << 20;
pub(crate) const ARGUMENT_BYTES: u64 = 48;
pub(crate) const DRAW_OFFSET: u64 = 12;
pub(crate) const BUDGET: u32 = 1 << 27;
// D3D12's maximum compute threads per group; conservative until the
// translated program exposes its exact group size to this layer.
pub(crate) const GROUP_COST: u32 = 1024;

pub(crate) fn enabled() -> bool {
    cfg!(test) || std::env::var("VICTORY_MESH_INDIRECT").is_ok_and(|v| v == "1")
}

pub(crate) fn shape(kind: u32, stride: u32, max: u32, counted: bool) -> bool {
    kind == 10 && stride == 12 && max == 1 && !counted
}

pub(crate) fn limits(record_bytes: u64, vertices: u32) -> Option<u32> {
    if record_bytes == 0 || vertices == 0 || vertices > BUDGET { return None; }
    let groups = (SCRATCH_LIMIT / record_bytes).min(65535).min(u64::from(BUDGET / vertices.max(GROUP_COST)));
    (groups > 0).then_some(groups as u32)
}

// One invocation, no title data on the CPU. An invalid shape clears both
// commands; it never clamps a large request to partial geometry. Bytes 0..28
// are the two native arguments, followed by reason and raw x/y/z/capacity.
const SOURCE: &str = r#"
#define RS "RootConstants(num32BitConstants=4,b0), UAV(u0), UAV(u1), UAV(u2)"
cbuffer Constants : register(b0) { uint capacity; uint vertices; uint limit; uint input_offset; }
RWByteAddressBuffer input : register(u0);
RWByteAddressBuffer output : register(u1);
RWByteAddressBuffer budget : register(u2);
[RootSignature(RS)] [numthreads(1,1,1)]
void main() {
    output.Store3(0, uint3(0,1,1));
    output.Store4(12, uint4(0,0,0,0));
    uint3 g = input.Load3(input_offset);
    output.Store4(32, uint4(g,capacity));
    output.Store(28, 1); // empty work
    if (g.x == 0 || g.y == 0 || g.z == 0) return;
    output.Store(28, 2); // unsupported dimensions
    if (g.y != 1 || g.z != 1) return;
    output.Store(28, 3); // record capacity
    if (g.x > capacity) return;
    output.Store(28, 4); // budget/cost limit
    uint unit = max(vertices,1024u);
    if (unit > limit || g.x > limit / unit) return;
    uint cost = g.x * unit;
    uint before = budget.Load(0);
    [loop] for (uint attempt=0; attempt<8; ++attempt) {
        if (before > limit || cost > limit-before) return;
        uint observed;
        budget.InterlockedCompareExchange(0, before, before+cost, observed);
        if (observed == before) {
            output.Store3(0, uint3(g.x,1,1));
            output.Store4(12, uint4(vertices,g.x,0,0));
            output.Store(28, 0);
            return;
        }
        before=observed;
    }
    output.Store(28, 5); // bounded CAS contention
}
"#;

pub(crate) struct Converter {
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
}

pub(crate) fn converter() -> Option<&'static Converter> {
    static VALUE: std::sync::OnceLock<Option<Converter>> = std::sync::OnceLock::new();
    VALUE.get_or_init(|| make(&host()?.device).map_err(|e| {
        tracing::warn!(error=%e, "indirect mesh argument converter unavailable");
    }).ok()).as_ref()
}

fn make(device: &ID3D12Device) -> windows::core::Result<Converter> {
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: source and output pointers live through compilation.
    unsafe { D3DCompile(SOURCE.as_ptr().cast(), SOURCE.len(), s!("mesh-indirect"), None, None,
        s!("main"), s!("cs_5_1"), 0, 0, &mut code, Some(&mut errors))?; }
    let code = code.ok_or_else(windows::core::Error::empty)?;
    // SAFETY: live blob; its embedded root signature matches the shader.
    unsafe {
        let bytes = std::slice::from_raw_parts(code.GetBufferPointer().cast::<u8>(), code.GetBufferSize());
        let root: ID3D12RootSignature = device.CreateRootSignature(0, bytes)?;
        let pso = device.CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::transmute_copy(&root),
            CS: D3D12_SHADER_BYTECODE { pShaderBytecode: bytes.as_ptr().cast(), BytecodeLength: bytes.len() },
            ..Default::default()
        })?;
        Ok(Converter { root, pso })
    }
}

pub(crate) fn argument_buffer(device: &ID3D12Device) -> windows::core::Result<ID3D12Resource> {
    let mut buffer = None;
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER, Width: ARGUMENT_BYTES, Height: 1,
        DepthOrArraySize: 1, MipLevels: 1, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR, Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ..Default::default()
    };
    // SAFETY: owned descriptors and output; all accesses are within 48 bytes.
    unsafe { device.CreateCommittedResource(&D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default()
    }, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, None, &mut buffer)?; }
    buffer.ok_or_else(windows::core::Error::empty)
}

impl Converter {
    /// Input and budget are shared tracked buffers in UAV state. Output is
    /// owned by this list, enters UAV, and leaves INDIRECT_ARGUMENT.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record(&self, list: &HostList, input: u64, input_offset: u32, output: &ID3D12Resource,
        budget: u64, capacity: u32, vertices: u32) {
        let values = [capacity, vertices, BUDGET, input_offset];
        let l = &list.list;
        // SAFETY: validated addresses and live resources retained by caller;
        // one invocation only, with bounded output and work accounting.
        unsafe {
            l.ResourceBarrier(&[crate::indirect::uav_barrier()]);
            l.SetComputeRootSignature(&self.root);
            l.SetPipelineState(&self.pso);
            l.SetComputeRoot32BitConstants(0, 4, values.as_ptr().cast(), 0);
            l.SetComputeRootUnorderedAccessView(1, input);
            l.SetComputeRootUnorderedAccessView(2, output.GetGPUVirtualAddress());
            l.SetComputeRootUnorderedAccessView(3, budget);
            l.Dispatch(1,1,1);
            l.ResourceBarrier(&[transition(output, D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT)]);
        }
    }
}

pub(crate) fn done(list: &HostList, output: &ID3D12Resource) {
    // SAFETY: the recorded indirect passes consumed this owned buffer.
    unsafe { list.list.ResourceBarrier(&[transition(output, D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS)]); }
}

#[cfg(test)]
#[path="mesh_indirect_gpu.rs"]
mod gpu;

#[cfg(test)]
mod tests {
    #[test]
    fn mesh_indirect_shape_and_capacity_are_bounded() {
        assert!(super::shape(10,12,1,false));
        for bad in [(0,12,1,false),(10,16,1,false),(10,12,2,false),(10,12,1,true)] {
            assert!(!super::shape(bad.0,bad.1,bad.2,bad.3));
        }
        assert_eq!(super::limits(3584,192), Some(1170));
        assert_eq!(super::limits(0,3),None);
        assert_eq!(super::limits(super::SCRATCH_LIMIT+1,3),None);
        assert_eq!(super::limits(64,0),None);
        assert_eq!(super::limits(64,u32::MAX),None);
    }
}
