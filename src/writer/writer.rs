use rayon::prelude::*;
use std::io::{Seek, Write};
use tiff::encoder::{ImageEncoder, TiffKind, colortype};

/// 最终修复版签名
/// 泛型参数说明：
/// 'a: 生命周期
/// W: 写入器类型
/// K: TIFF 类型 (Standard TIFF 或 BigTIFF)
pub fn write_rgb_chunk_to_tiff<'a, W, K>(
    // 关键点：这里必须填满三个类型参数 W, colortype::RGB16, K
    encoder: &mut ImageEncoder<'a, W, colortype::RGB16, K>,
    band3: &[f64], // 改为切片，支持流式分块传入
    band2: &[f64],
    band1: &[f64],
    gamma_params: (f64, bool),
) -> Result<(), Box<dyn std::error::Error>>
where
    W: Write + Seek,
    K: TiffKind, // 必须添加这个约束
{
    let (inv_gamma, is_linear) = gamma_params;

    // 1. 验证长度（可选，但建议）
    let len = band3.len();
    if band2.len() != len || band1.len() != len {
        return Err("分块数据长度不一致".into());
    }
    println!("Processing done.");

    // 2. 并行处理并交错 RGB 数据
    let buffer: Vec<u16> = band3
        .par_iter()
        .zip(band2.par_iter())
        .zip(band1.par_iter())
        .flat_map(|((&r, &g), &b)| {
            let pr = apply_gamma_inline(r, inv_gamma, is_linear);
            let pg = apply_gamma_inline(g, inv_gamma, is_linear);
            let pb = apply_gamma_inline(b, inv_gamma, is_linear);
            [pr, pg, pb]
        })
        .collect();

    // 3. 写入当前分块
    encoder.write_strip(&buffer)?;

    Ok(())
}

#[inline(always)]
fn apply_gamma_inline(reflectance: f64, inv_gamma: f64, is_linear: bool) -> u16 {
    const MAX_U16: f64 = 65535.0;
    let r = reflectance.clamp(0.0, 1.0);
    if is_linear {
        (r * MAX_U16).round() as u16
    } else {
        r.powf(inv_gamma).mul_add(MAX_U16, 0.5) as u16
    }
}
