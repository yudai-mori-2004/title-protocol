// wasm-brightness/src/lib.rs
//
// TEEから渡されるRGBAピクセルデータの平均輝度を計算する。
// wasm32-unknown-unknown ターゲットでコンパイルする。
#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// WASM線形メモリを拡張してポインタを返す
#[no_mangle]
pub extern "C" fn alloc(size: u32) -> u32 {
    let pages_needed = ((size as usize) + 65535) / 65536;
    let old_pages = core::arch::wasm32::memory_grow(0, pages_needed);
    if old_pages == usize::MAX {
        return 0;
    }
    (old_pages * 65536) as u32
}

/// RGBAピクセルデータから知覚輝度の平均を計算
/// 知覚輝度 = 0.299R + 0.587G + 0.114B
#[no_mangle]
pub extern "C" fn compute_brightness(ptr: u32, len: u32) -> f32 {
    let data = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let pixel_count = (len as usize) / 4; // RGBA
    if pixel_count == 0 {
        return 0.0;
    }

    let mut sum: u64 = 0;
    for i in 0..pixel_count {
        let base = i * 4;
        let r = data[base] as u64;
        let g = data[base + 1] as u64;
        let b = data[base + 2] as u64;
        sum += (299 * r + 587 * g + 114 * b) / 1000;
    }

    (sum as f64 / pixel_count as f64) as f32
}