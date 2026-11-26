//! Synthetic GPU fixture: no title shaders, assets, or window.
use super::*;
use crate::host::{Host, HostList};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::{Interface, PCSTR};

const PRODUCER: &str = r#"
#define RS "RootConstants(num32BitConstants=3,b0), UAV(u0), UAV(u1), UAV(u2)"
cbuffer Values : register(b0) { uint x; uint y; uint z; }
RWByteAddressBuffer input : register(u0);
RWByteAddressBuffer output : register(u1);
RWByteAddressBuffer records : register(u2);
[RootSignature(RS)] [numthreads(1,1,1)] void main() {
 input.Store3(4,uint3(x,y,z));
 // A missing converter clear must fail the oracle without issuing an
 // unbounded GPU command: these sentinels draw only the two safe records.
 output.Store4(0,uint4(2,1,1,3)); output.Store4(16,uint4(2,0,0,77));
 output.Store4(32,uint4(77,77,77,77));
 records.Store4(0,uint4(99,99,99,99)); records.Store4(16,uint4(99,99,99,99));
 records.Store4(32,uint4(99,99,99,99)); records.Store4(48,uint4(99,99,99,99));
}"#;
const RECORDS: &str = r#"
#define RS "RootConstants(num32BitConstants=1,b0), UAV(u0)"
cbuffer Values : register(b0) { uint tag; }
RWByteAddressBuffer records : register(u0);
[RootSignature(RS)] [numthreads(1,1,1)] void main(uint3 g: SV_GroupID) {
 records.Store4(g.x*16,uint4(tag+g.x,11,22,33));
}"#;
const VERTEX: &str = r#"
#define RS "SRV(t0,visibility=SHADER_VISIBILITY_VERTEX)"
ByteAddressBuffer records : register(t0);
struct Out { float4 p:SV_Position; float4 color:COLOR; };
[RootSignature(RS)] Out main(uint v:SV_VertexID,uint i:SV_InstanceID) {
 float2 p=v==0?float2(-1,-1):(v==1?float2(-1,1):float2(1,-1));
 Out o; o.p=float4(p.x*0.5+(i==0?-0.5:0.5),p.y,0,1);
 o.color=float4(records.Load(i*16)/255.0,0,0,1); return o;
}"#;
const PIXEL: &str = "float4 main(float4 p:SV_Position,float4 color:COLOR):SV_Target { return color; }";

fn code(source: &str, profile: PCSTR) -> Vec<u8> {
    let mut blob=None;
    let mut errors: Option<ID3DBlob>=None;
    // SAFETY: owned source, profiles and outputs.
    let result=unsafe { D3DCompile(source.as_ptr().cast(),source.len(),s!("synthetic-mesh"),None,None,
        s!("main"),profile,0,0,&mut blob,Some(&mut errors)) };
    assert!(result.is_ok(),"{result:?}");
    let blob=blob.unwrap();
    // SAFETY: live compiled blob; copy its declared byte range before dropping it.
    unsafe { std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(),blob.GetBufferSize()).to_vec() }
}

fn bytecode(bytes: &[u8]) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE { pShaderBytecode:bytes.as_ptr().cast(),BytecodeLength:bytes.len() }
}

fn compute(device:&ID3D12Device,source:&str)->(ID3D12RootSignature,ID3D12PipelineState) {
    let bytes=code(source,s!("cs_5_1"));
    // SAFETY: owned shader bytes and root signature outlive pipeline creation.
    unsafe {
        let root:ID3D12RootSignature=device.CreateRootSignature(0,&bytes).unwrap();
        let pso=device.CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature:std::mem::transmute_copy(&root),CS:bytecode(&bytes),..Default::default()
        }).unwrap();
        (root,pso)
    }
}

fn signature(device:&ID3D12Device,kind:D3D12_INDIRECT_ARGUMENT_TYPE,stride:u32)->ID3D12CommandSignature {
    let arg=D3D12_INDIRECT_ARGUMENT_DESC { Type:kind,..Default::default() };
    let mut result=None;
    // SAFETY: one live argument description; dispatch/draw signatures change no roots.
    unsafe { device.CreateCommandSignature(&D3D12_COMMAND_SIGNATURE_DESC {
        ByteStride:stride,NumArgumentDescs:1,pArgumentDescs:&arg,NodeMask:0
    },None::<&ID3D12RootSignature>,&mut result).unwrap(); }
    result.unwrap()
}

fn readback(device:&ID3D12Device,bytes:u64)->ID3D12Resource {
    let mut out=None;
    // SAFETY: valid buffer description and owned output; caller bounds all copies.
    unsafe { device.CreateCommittedResource(&D3D12_HEAP_PROPERTIES {
        Type:D3D12_HEAP_TYPE_READBACK,..Default::default()
    },D3D12_HEAP_FLAG_NONE,&D3D12_RESOURCE_DESC {
        Dimension:D3D12_RESOURCE_DIMENSION_BUFFER,Width:bytes,Height:1,DepthOrArraySize:1,MipLevels:1,
        SampleDesc:DXGI_SAMPLE_DESC {Count:1,Quality:0},Layout:D3D12_TEXTURE_LAYOUT_ROW_MAJOR,..Default::default()
    },D3D12_RESOURCE_STATE_COPY_DEST,None,&mut out).unwrap(); }
    out.unwrap()
}

struct Target {
    buffer:ID3D12Resource,
    _heap:ID3D12DescriptorHeap,
    rtv:D3D12_CPU_DESCRIPTOR_HANDLE,
    root:ID3D12RootSignature,
    pso:ID3D12PipelineState,
}

impl Target {
    fn new(device:&ID3D12Device)->Self {
        let vs=code(VERTEX,s!("vs_5_1"));
        let ps=code(PIXEL,s!("ps_5_1"));
        // SAFETY: owned shaders/descriptors; the target and RTV heap live through execution.
        unsafe {
            let root:ID3D12RootSignature=device.CreateRootSignature(0,&vs).unwrap();
            let mut desc=D3D12_GRAPHICS_PIPELINE_STATE_DESC {
                pRootSignature:std::mem::transmute_copy(&root),VS:bytecode(&vs),PS:bytecode(&ps),
                SampleMask:u32::MAX,PrimitiveTopologyType:D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
                NumRenderTargets:1,SampleDesc:DXGI_SAMPLE_DESC {Count:1,Quality:0},
                RasterizerState:D3D12_RASTERIZER_DESC {FillMode:D3D12_FILL_MODE_SOLID,CullMode:D3D12_CULL_MODE_NONE,
                    DepthClipEnable:true.into(),..Default::default()},..Default::default()
            };
            desc.RTVFormats[0]=DXGI_FORMAT_R8G8B8A8_UNORM;
            desc.BlendState.RenderTarget[0].RenderTargetWriteMask=D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8;
            let pso=device.CreateGraphicsPipelineState(&desc).unwrap();
            let mut buffer=None;
            device.CreateCommittedResource(&D3D12_HEAP_PROPERTIES {Type:D3D12_HEAP_TYPE_DEFAULT,..Default::default()},
                D3D12_HEAP_FLAG_NONE,&D3D12_RESOURCE_DESC {
                    Dimension:D3D12_RESOURCE_DIMENSION_TEXTURE2D,Width:32,Height:16,DepthOrArraySize:1,MipLevels:1,
                    Format:DXGI_FORMAT_R8G8B8A8_UNORM,SampleDesc:DXGI_SAMPLE_DESC {Count:1,Quality:0},
                    Flags:D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,..Default::default()
                },D3D12_RESOURCE_STATE_RENDER_TARGET,None,&mut buffer).unwrap();
            let buffer=buffer.unwrap();
            let heap:ID3D12DescriptorHeap=device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type:D3D12_DESCRIPTOR_HEAP_TYPE_RTV,NumDescriptors:1,..Default::default()
            }).unwrap();
            let rtv=heap.GetCPUDescriptorHandleForHeapStart();
            device.CreateRenderTargetView(&buffer,None,rtv);
            Self {buffer,_heap:heap,rtv,root,pso}
        }
    }

    fn bind(&self,list:&HostList,records:&ID3D12Resource) {
        // SAFETY: open list and live target, PSO, root and geometry buffer.
        unsafe {
            let l=&list.list;
            l.SetGraphicsRootSignature(&self.root);l.SetPipelineState(&self.pso);
            l.SetGraphicsRootShaderResourceView(0,records.GetGPUVirtualAddress());
            l.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            l.RSSetViewports(&[D3D12_VIEWPORT {Width:32.0,Height:16.0,MaxDepth:1.0,..Default::default()}]);
            l.RSSetScissorRects(&[RECT {left:0,top:0,right:32,bottom:16}]);
            l.OMSetRenderTargets(1,Some(&self.rtv),false,None);
            l.ClearRenderTargetView(self.rtv,&[0.0;4],None);
        }
    }

    fn copy(&self,list:&HostList,back:&ID3D12Resource) {
        // SAFETY: the 32-by-16 RGBA8 target fits the 4096-byte, 256-byte-pitch region.
        unsafe {
            list.list.ResourceBarrier(&[transition(&self.buffer,D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_COPY_SOURCE)]);
            let src=D3D12_TEXTURE_COPY_LOCATION {pResource:std::mem::transmute_copy(&self.buffer),
                Type:D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,..Default::default()};
            let dst=D3D12_TEXTURE_COPY_LOCATION {pResource:std::mem::transmute_copy(back),
                Type:D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,Anonymous:D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint:D3D12_PLACED_SUBRESOURCE_FOOTPRINT {Offset:512,Footprint:D3D12_SUBRESOURCE_FOOTPRINT {
                        Format:DXGI_FORMAT_R8G8B8A8_UNORM,Width:32,Height:16,Depth:1,RowPitch:256}}
                }};
            list.list.CopyTextureRegion(&dst,0,0,0,&src,None);
            list.list.ResourceBarrier(&[transition(&self.buffer,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_RENDER_TARGET)]);
        }
    }
}

#[derive(Clone,Copy)]
enum Mode { Direct, Indirect, OldDrop }

struct Observation { words:Vec<u32>, pixels:Vec<u8> }

fn run(host:&Host,cases:&[([u32;3],u32)],mode:Mode,initial_budget:u32)->(Vec<Observation>,u32) {
    let device=&host.device;
    let list=host.new_list().unwrap();
    let records=list.mesh_buffer(64).unwrap();
    let output=list.mesh_arguments().unwrap();
    assert_eq!(output.as_raw(),list.mesh_arguments().unwrap().as_raw(),"one reusable argument allocation per list");
    let (input,input_ptr)=host.shared_buffer(64).unwrap();
    let (budget,budget_ptr)=host.shared_buffer(256).unwrap();
    // SAFETY: mapped allocations hold 16 input and 64 budget dwords; the GPU is idle.
    unsafe {
        std::slice::from_raw_parts_mut(input_ptr.cast::<u32>(),16).fill(0xfeed_beef);
        std::slice::from_raw_parts_mut(input_ptr.cast::<u32>().add(1),3).fill(0);
        budget_ptr.cast::<u32>().write(initial_budget);
    }
    let producer=compute(device,PRODUCER);
    let mesh=compute(device,RECORDS);
    let convert=make(device).unwrap();
    let dispatch=signature(device,D3D12_INDIRECT_ARGUMENT_TYPE_DISPATCH,12);
    let draw=signature(device,D3D12_INDIRECT_ARGUMENT_TYPE_DRAW,16);
    let target=Target::new(device);
    let mut backs=Vec::new();
    for &(groups,tag) in cases {
        let back=readback(device,4608);
        // Upstream shared buffers stay in UAV state. The converter's UAV
        // barrier orders the producer before its argument reads.
        // SAFETY: open list, live resources and bounded synthetic dispatch/draw commands.
        unsafe {
            let l=&list.list;
            l.SetComputeRootSignature(&producer.0);l.SetPipelineState(&producer.1);
            l.SetComputeRoot32BitConstants(0,3,groups.as_ptr().cast(),0);
            l.SetComputeRootUnorderedAccessView(1,input.GetGPUVirtualAddress());
            l.SetComputeRootUnorderedAccessView(2,output.GetGPUVirtualAddress());
            l.SetComputeRootUnorderedAccessView(3,records.GetGPUVirtualAddress());
            l.Dispatch(1,1,1);
            // This input was initially zero on the CPU. The converter must
            // consume the dimensions produced above at GPU execution time.
            convert.record(&list,input.GetGPUVirtualAddress(),4,&output,budget.GetGPUVirtualAddress(),2,3);
            l.SetComputeRootSignature(&mesh.0);l.SetPipelineState(&mesh.1);
            l.SetComputeRoot32BitConstant(0,tag,0);
            l.SetComputeRootUnorderedAccessView(1,records.GetGPUVirtualAddress());
            match mode {
                Mode::Direct=>l.Dispatch(groups[0],1,1),
                Mode::Indirect=>l.ExecuteIndirect(&dispatch,1,&output,0,None,0),
                Mode::OldDrop=>{},
            }
            l.ResourceBarrier(&[transition(&records,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE)]);
            target.bind(&list,&records);
            match mode {
                Mode::Direct=>l.DrawInstanced(3,groups[0],0,0),
                Mode::Indirect=>l.ExecuteIndirect(&draw,1,&output,DRAW_OFFSET,None,0),
                Mode::OldDrop=>{},
            }
            l.ResourceBarrier(&[transition(&records,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_COPY_SOURCE),
                transition(&output,D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT,D3D12_RESOURCE_STATE_COPY_SOURCE)]);
            l.CopyBufferRegion(&back,0,&records,0,64);
            l.CopyBufferRegion(&back,64,&output,0,ARGUMENT_BYTES);
            l.ResourceBarrier(&[transition(&records,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_UNORDERED_ACCESS),
                transition(&output,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT)]);
        }
        done(&list,&output);
        target.copy(&list,&back);
        backs.push(back);
    }
    list.close();
    host.execute(&[&list]);
    // SAFETY: execute waited for completion; every read is inside this 4608-byte mapping.
    let result=backs.iter().map(|back| unsafe {
        let mut p=std::ptr::null_mut();back.Map(0,None,Some(&mut p)).unwrap();
        let out=Observation {words:std::slice::from_raw_parts(p.cast::<u32>(),28).to_vec(),
            pixels:std::slice::from_raw_parts(p.cast::<u8>().add(512),4096).to_vec()};
        back.Unmap(0,None);out
    }).collect();
    // SAFETY: GPU completion precedes reads inside the still-live mapped allocations.
    let (guards,budget)=unsafe {(std::slice::from_raw_parts(input_ptr.cast::<u32>(),16),budget_ptr.cast::<u32>().read())};
    assert!(guards[..1].iter().chain(&guards[4..]).all(|&w|w==0xfeed_beef),"input guards");
    (result,budget)
}

fn pixel(o:&Observation,x:usize)->[u8;4] { o.pixels[12*256+x*4..12*256+x*4+4].try_into().unwrap() }

fn test_host()->Option<&'static Host> {
    static CHECK:std::sync::Once=std::sync::Once::new();
    CHECK.call_once(|| {
        let mut debug:Option<ID3D12Debug>=None;
        // SAFETY: plain interface query into an owned optional COM reference.
        if unsafe {D3D12GetDebugInterface(&mut debug)}.is_err() {
            eprintln!("SKIPPED mesh_indirect D3D12 state validation: Graphics Tools debug layer unavailable; GPU output oracles still run");
        } else if std::env::var_os("VICTORY_STATE_TEST_DEBUG").is_none() {
            eprintln!("SKIPPED mesh_indirect D3D12 state validation: rerun isolated with VICTORY_STATE_TEST_DEBUG=1 before device creation");
        }
    });
    host()
}

#[test]
fn mesh_indirect_gpu_records_pixels_reuse_and_old_drop_control() {
    let Some(host)=test_host() else { eprintln!("SKIPPED mesh_indirect_gpu_records_pixels_reuse_and_old_drop_control: no D3D12 device");return; };
    let cases=[([2,1,1],61),([1,1,1],103)];
    let (direct,_)=run(host,&cases,Mode::Direct,0);
    let (indirect,budget)=run(host,&cases,Mode::Indirect,0);
    assert_eq!(budget,3*GROUP_COST,"both GPU-produced dispatches charged the shared budget");
    for (i,(a,b)) in direct.iter().zip(&indirect).enumerate() {
        assert_eq!(&a.words[..cases[i].0[0] as usize*4],&b.words[..cases[i].0[0] as usize*4]);
        assert_eq!(a.pixels,b.pixels,"direct/indirect pixel equality round {i}");
        assert_eq!(&b.words[8..16],&[99;8],"record guards");
        let groups=cases[i].0;
        assert_eq!(&b.words[16..24],&[groups[0],1,1,3,groups[0],0,0,0],"accepted command layout");
        assert_eq!(&b.words[24..28],&[groups[0],groups[1],groups[2],2],"raw GPU arguments and capacity");
        assert_eq!(pixel(b,4),[cases[i].1 as u8,0,0,255]);
        assert_eq!(pixel(b,20),if i==0 {[62,0,0,255]}else{[0;4]});
    }
    let (old,_)=run(host,&cases[..1],Mode::OldDrop,0);
    assert_ne!(old[0].pixels,indirect[0].pixels,"old missing execution must fail the same pixel oracle");
    assert_eq!(pixel(&old[0],4),[0;4]);
}

#[test]
fn mesh_indirect_gpu_rejects_dimensions_capacity_and_exhausted_budget() {
    let Some(host)=test_host() else { eprintln!("SKIPPED mesh_indirect_gpu_rejects_dimensions_capacity_and_exhausted_budget: no D3D12 device");return; };
    for (groups,reason) in [([0,1,1],1),([3,1,1],3),([u32::MAX,1,1],3),
        ([1,0,1],1),([1,2,1],2),([1,1,0],1),([1,1,u32::MAX],2)] {
        let (out,budget)=run(host,&[(groups,61)],Mode::Indirect,0);
        assert_eq!(&out[0].words[16..24],&[0,1,1,0,0,0,0,reason],"dimensions {groups:?}");
        assert_eq!(&out[0].words[24..28],&[groups[0],groups[1],groups[2],2],"rejected raw GPU arguments");
        assert_eq!(&out[0].words[..16],&[99;16]);
        assert_eq!(pixel(&out[0],4),[0;4]);assert_eq!(budget,0);
    }
    for initial in [BUDGET-GROUP_COST+1,BUDGET,u32::MAX] {
        let (out,budget)=run(host,&[([1,1,1],61)],Mode::Indirect,initial);
        assert_eq!(&out[0].words[16..24],&[0,1,1,0,0,0,0,4]);
        assert_eq!(&out[0].words[24..28],&[1,1,1,2]);
        assert_eq!(pixel(&out[0],4),[0;4]);assert_eq!(budget,initial);
    }
    let (out,budget)=run(host,&[([1,1,1],61),([1,1,1],62)],Mode::Indirect,BUDGET-GROUP_COST);
    assert_eq!(budget,BUDGET);assert_eq!(pixel(&out[0],4),[61,0,0,255]);
    assert_eq!(pixel(&out[1],4),[0;4]);assert_eq!(out[1].words[23],4);
    assert_eq!(&out[1].words[24..28],&[1,1,1,2]);
}

#[test]
fn mesh_indirect_gpu_legacy_clamp_cannot_wrap_shared_budget() {
    let Some(host)=test_host() else {eprintln!("SKIPPED mesh_indirect_gpu_legacy_clamp_cannot_wrap_shared_budget: no D3D12 device");return;};
    let list=host.new_list().unwrap();
    let (legacy,data)=host.shared_buffer(64*16+16).unwrap();
    let (input,input_data)=host.shared_buffer(16).unwrap();
    // SAFETY: mapped input ranges cover these synthetic records before submission.
    unsafe {
        for record in 0..64 {
            let words=data.cast::<u32>().add(1+record*4);
            // Each record costs2^26; the old 64-record InterlockedAdd
            // wrapped2^32 back to zero and admitted the later mesh draw.
            std::ptr::copy_nonoverlapping([1<<24,4,0,0].as_ptr(),words,4);
        }
        std::ptr::copy_nonoverlapping([1,1,1].as_ptr(),input_data.cast::<u32>(),3);
        assert!(crate::indirect::bound(&list,&legacy,legacy.GetGPUVirtualAddress()+4,(0,16,64)));
    }
    crate::indirect::done(&list,&legacy);
    let budget=list.budget().unwrap();
    let output=list.mesh_arguments().unwrap();
    let converter=make(&host.device).unwrap();
    // SAFETY: the input resource remains alive through the recorded execution.
    converter.record(&list,unsafe {input.GetGPUVirtualAddress()},0,&output,budget,2,3);
    let back=readback(&host.device,ARGUMENT_BYTES);
    // SAFETY: live argument/readback resources and copies bounded by ARGUMENT_BYTES.
    unsafe {
        list.list.ResourceBarrier(&[transition(&output,D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT,D3D12_RESOURCE_STATE_COPY_SOURCE)]);
        list.list.CopyBufferRegion(&back,0,&output,0,ARGUMENT_BYTES);
        list.list.ResourceBarrier(&[transition(&output,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_UNORDERED_ACCESS)]);
    }
    list.close();
    // Repeat the same recorded list: its shared budget must reset for each
    // submission and must still reject the mesh dispatch after the clamp.
    for _ in 0..2 {
        // SAFETY: the prior execution has completed; these fields lie in the live mapping.
        unsafe {
            for record in 0..64 {data.cast::<u32>().add(1+record*4).write(1<<24);}
        }
        host.execute(&[&list]);
        let mut p=std::ptr::null_mut();
        // SAFETY: GPU is finished; the 12 dwords fit the mapped readback resource.
        unsafe {
            back.Map(0,None,Some(&mut p)).unwrap();
            assert_eq!(std::slice::from_raw_parts(p.cast::<u32>(),12),&[0,1,1,0,0,0,0,4,1,1,1,2]);
            back.Unmap(0,None);
        }
    }
}
