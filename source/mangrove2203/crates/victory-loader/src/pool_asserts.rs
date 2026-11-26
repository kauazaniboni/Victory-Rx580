//! `VICTORY_POOL_NO_ASSERT=1`: the title's fixed-size pool allocators stop the
//! game with an `int3` when a pool has no free slot. In the opening cinematic
//! the `INSTANCE_LIST` pool fills (45000/45000) at a fixed point about 6-8
//! minutes in, and every worker thread stops at once (VIctory issue #698).
//! The code right after each of those `int3`s is `xor reg, reg` on the way to
//! the allocator's own null return, so writing a NOP over the `int3` makes the
//! allocation fail with null instead.
//!
//! Approach by WalliceCariasPerussio (VIctory #698, gist 3f344d52): 17+
//! minutes in the world on an RTX 5060 with every such site disarmed. This
//! version finds the sites itself in the mapped image (no list derived from
//! the title is shipped): every allocator copy pops its free list with a
//! 17-bit index (`and r32, 0x1ffff`) next to a `lock cmpxchg`; every jump
//! near it that lands on `int3` + `xor r, r` (same register) is a site.
//!
//! A diagnostic workaround, off by default: a caller that does not expect
//! null may fault later.

use victory_pe::ImageInfo;
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Memory::{PAGE_EXECUTE_READWRITE, VirtualProtect};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

pub(crate) const ENV: &str = "VICTORY_POOL_NO_ASSERT";

/// The build the scan was checked on (1.0.2.1; BLAKE3 prefix).
const BUILD: &str = "b99f876b74d4a77f";

/// `int3` followed by `xor r32, r32` / `xor r64, r64` of one register.
fn null_return_int3(code: &[u8], at: usize) -> bool {
    let Some(&[cc, mut op, mut modrm, next]) = code.get(at..at + 4) else {
        return false;
    };
    if cc != 0xCC {
        return false;
    }
    if (0x40..=0x4F).contains(&op) {
        op = modrm;
        modrm = next;
    }
    (op == 0x31 || op == 0x33) && modrm >> 6 == 3 && (modrm >> 3) & 7 == modrm & 7
}

/// Offsets (in `code`) of the pool-empty `int3`s.
pub(crate) fn sites(code: &[u8]) -> Vec<usize> {
    const IMM: [u8; 4] = [0xFF, 0xFF, 0x01, 0x00];
    let mut out = std::collections::BTreeSet::new();
    for i in 2..code.len().saturating_sub(24) {
        if code[i..i + 4] != IMM || code[i - 2] != 0x81 || (code[i - 1] >> 3) & 7 != 4 || code[i - 1] >> 6 != 3 {
            continue;
        }
        let w = &code[i..i + 24];
        if !w.contains(&0xF0) || !w.windows(2).any(|p| p == [0x0F, 0xB1]) {
            continue;
        }
        let (lo, hi) = (i.saturating_sub(0xA0), (i + 0x1C0).min(code.len().saturating_sub(8)));
        for p in lo..hi {
            let b = code[p];
            let target = match b {
                0x70..=0x7F | 0xEB => (p + 2).checked_add_signed(code[p + 1] as i8 as isize),
                0x0F if (0x80..=0x8F).contains(&code[p + 1]) => {
                    (p + 6).checked_add_signed(i32::from_le_bytes(code[p + 2..p + 6].try_into().unwrap()) as isize)
                }
                0xE9 => (p + 5).checked_add_signed(i32::from_le_bytes(code[p + 1..p + 5].try_into().unwrap()) as isize),
                _ => None,
            };
            if let Some(t) = target
                && null_return_int3(code, t)
            {
                out.insert(t);
            }
        }
    }
    out.into_iter().collect()
}

/// Writes a NOP over each pool-empty `int3` of the mapped image's `.text`.
/// Returns how many were disarmed (0 for another build).
///
/// # Safety
/// `base` is the just-mapped image described by `info`, and no title code
/// has run yet.
pub(crate) unsafe fn disarm(base: usize, info: &ImageInfo, hash: &blake3::Hash) -> usize {
    if !hash.to_hex().starts_with(BUILD) {
        tracing::warn!("{ENV}: not the build this was checked on; nothing changed");
        return 0;
    }
    let Some(text) = info.sections.iter().find(|s| s.name == ".text") else { return 0 };
    let start = base + text.rva as usize;
    // SAFETY: the section is mapped and readable for its virtual size.
    let code = unsafe { std::slice::from_raw_parts(start as *const u8, text.virtual_size as usize) };
    let found = sites(code);
    let mut done = 0;
    for off in found.iter().copied() {
        let at = (start + off) as *mut u8;
        let mut old = 0;
        // SAFETY: one byte inside the mapped `.text`; protection restored below.
        if unsafe { VirtualProtect(at.cast(), 1, PAGE_EXECUTE_READWRITE, &mut old) } == 0 {
            continue;
        }
        // SAFETY: writable now, and the byte is the int3 checked by `sites`.
        unsafe {
            *at = 0x90;
            VirtualProtect(at.cast(), 1, old, &mut old);
            FlushInstructionCache(GetCurrentProcess(), at.cast(), 1);
        }
        done += 1;
    }
    tracing::info!(found = found.len(), disarmed = done, "{ENV}: pool-empty int3s now return null");
    done
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_the_jump_target_after_a_17_bit_pop() {
        // and eax, 0x1ffff; lock cmpxchg [rcx], edx; jz +2; ret; ret; int3; xor edi, edi
        let mut code = vec![0x90; 0xC0];
        code.extend([0x81, 0xE0, 0xFF, 0xFF, 0x01, 0x00, 0xF0, 0x0F, 0xB1, 0x11, 0x74, 0x02, 0xC3, 0xC3, 0xCC, 0x31, 0xFF, 0xC3]);
        code.extend(vec![0x90; 0x40]);
        assert_eq!(super::sites(&code), vec![0xC0 + 14]);
    }

    #[test]
    fn ignores_int3_without_a_null_return() {
        let mut code = vec![0x90; 0xC0];
        code.extend([0x81, 0xE0, 0xFF, 0xFF, 0x01, 0x00, 0xF0, 0x0F, 0xB1, 0x11, 0x74, 0x02, 0xC3, 0xC3, 0xCC, 0x31, 0xC7, 0xC3]);
        code.extend(vec![0x90; 0x40]);
        assert!(super::sites(&code).is_empty());
    }
}
