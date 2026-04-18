use crate::file_struct::file_struct::{BandSpecificCalibration, HsdFile, HsdMetadata};
use anyhow::Result;
use memmap2::MmapOptions;
use rayon::prelude::*;
use std::f64::consts::PI;
use std::fs::File;
use std::path::Path;

pub fn calculate_scaling_factor(bt: f64) -> f64 {
    if bt >= 280.0 {
        1.0
    } else if bt <= 230.0 {
        0.3
    } else {
        0.3 + (bt - 230.0) * (0.7 / 50.0)
    }
}

pub struct DataCalibrationResult {
    pub refl_bright_temp_result: Vec<f64>,
    pub hsd_metadata: HsdMetadata,
}

pub fn data_calibration_correction(hsd_file: HsdFile) -> DataCalibrationResult {
    let metadata = hsd_file.metadata;
    let raw_data = hsd_file.data.data;

    // 获取通用定标参数（见 PDF 第 18 页 Block #5 No. 8 & 9）
    let slope = metadata.calibration_info.slope;
    let intercept = metadata.calibration_info.intercept;
    let central_lambda_um = metadata.calibration_info.central_wavelength;

    match metadata.calibration_info.band_specific_calibration {
        BandSpecificCalibration::VisibleNIR {
            albedo_conversion_coeff,
            calibrated_slope,
            calibrated_intercept,
            ..
        } => {
            let result: Vec<f64> = raw_data
                .par_iter()
                .map(|&px| {
                    if px >= 65534 {
                        f64::NAN
                    } else {
                        let dn = px as f64;
                        // 这里的逻辑沿用你原始代码的“双重斜率”逻辑（注意：通常直接用 metadata.slope 即可）
                        // 若按照文档 No. 10 修正，可见光通常直接转 Albedo
                        let radiance = calibrated_slope * dn + calibrated_intercept;
                        let albedo = albedo_conversion_coeff * radiance;
                        albedo.clamp(0.0, 1.0)
                    }
                })
                .collect();

            DataCalibrationResult {
                refl_bright_temp_result: result,
                hsd_metadata: metadata,
            }
        }
        BandSpecificCalibration::Infrared {
            tb_conversion_coeffs, // (c0, c1, c2)
            speed_of_light,       // c
            planck_constant,      // h
            boltzmann_constant,   // k
            ..
        } => {
            // 预计算物理常数项以提高并行效率
            // 注意单位换算：文档中 lambda 是微米(um)，辐射率 I 是 W/(m^2 sr um)
            // 为了匹配 SI 单位制计算，通常建议统一换算：
            let lambda_m = central_lambda_um * 1e-6;
            let h = planck_constant;
            let c = speed_of_light;
            let k = boltzmann_constant;

            // 分子项: (h * c) / (k * lambda)
            let hckl = (h * c) / (k * lambda_m);
            // 辐射率前面的系数: (2 * h * c^2) / lambda^5
            let two_h_c2_l5 = (2.0 * h * c * c) / lambda_m.powi(5);

            let (c0, c1, c2) = tb_conversion_coeffs;

            let result: Vec<f64> = raw_data
                .par_iter()
                .map(|&px| {
                    if px >= 65534 {
                        f64::NAN
                    } else {
                        let dn = px as f64;
                        // 1. 计算辐射率 I [W / (m^2 sr um)]
                        let radiance = slope * dn + intercept;

                        if radiance <= 0.0 {
                            return f64::NAN;
                        }

                        // 2. 换算辐射率到 SI 单位 [W / (m^2 sr m)] 以匹配物理常数
                        // 1 / um = 1e6 / m
                        let radiance_si = radiance * 1e6;

                        // 3. 计算有效亮度温度 Te (见 PDF 第 18 页公式 11)
                        let te = hckl / ((two_h_c2_l5 / radiance_si) + 1.0).ln();

                        // 4. 计算亮度温度 Tb (见 PDF 第 18 页公式 12)
                        let tb = c0 + c1 * te + c2 * te * te;

                        tb
                    }
                })
                .collect();

            DataCalibrationResult {
                refl_bright_temp_result: result,
                hsd_metadata: metadata,
            }
        }
    }
}

// ==========================================
// 2. Lanczos 3 重采样 (权重缓存版)
// ==========================================

const KERNEL_RADIUS: f64 = 3.0;

#[derive(Clone)]
pub struct FilterWeights {
    pub indices: Vec<usize>,
    pub weights: Vec<f64>,
}

pub struct LanczosScaler {
    x_filters: Vec<FilterWeights>,
    y_filters: Vec<FilterWeights>,
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
}

impl LanczosScaler {
    pub fn new(src_w: usize, src_h: usize) -> Self {
        let dst_w = src_w * 2;
        let dst_h = src_h * 2;

        let x_filters = Self::compute_axis_weights(src_w, dst_w);
        let y_filters = Self::compute_axis_weights(src_h, dst_h);

        Self {
            x_filters,
            y_filters,
            src_w,
            src_h,
            dst_w,
            dst_h,
        }
    }

    fn lanczos3_kernel(x: f64) -> f64 {
        if x == 0.0 {
            return 1.0;
        }
        if x.abs() >= KERNEL_RADIUS {
            return 0.0;
        }
        let px = x * PI;
        let px3 = px / 3.0;
        (px.sin() / px) * (px3.sin() / px3)
    }

    fn compute_axis_weights(src_len: usize, dst_len: usize) -> Vec<FilterWeights> {
        let scale = dst_len as f64 / src_len as f64;
        (0..dst_len)
            .map(|i| {
                let center = (i as f64 + 0.5) / scale - 0.5;
                let start = (center - KERNEL_RADIUS).ceil() as isize;
                let end = (center + KERNEL_RADIUS).floor() as isize;

                let mut indices = Vec::new();
                let mut weights = Vec::new();
                let mut weight_sum = 0.0;

                for j in start..=end {
                    let idx = j.clamp(0, src_len as isize - 1) as usize;
                    let weight = Self::lanczos3_kernel(center - j as f64);
                    indices.push(idx);
                    weights.push(weight);
                    weight_sum += weight;
                }
                if weight_sum != 0.0 {
                    weights.iter_mut().for_each(|w| *w /= weight_sum);
                }
                FilterWeights { indices, weights }
            })
            .collect()
    }

    pub fn resize(&self, input: &[f64]) -> Vec<f64> {
        assert_eq!(
            input.len(),
            self.src_w * self.src_h,
            "LanczosScaler::resize 输入尺寸不匹配"
        );

        let mut output = vec![0.0; self.dst_w * self.dst_h];
        let chunk_height = 64;

        output
            .par_chunks_mut(self.dst_w * chunk_height)
            .enumerate()
            .for_each(|(chunk_idx, out_chunk)| {
                let start_y = chunk_idx * chunk_height;
                let rows_in_chunk = out_chunk.len() / self.dst_w;
                let end_y = (start_y + rows_in_chunk).min(self.dst_h);

                let mut min_src_y = usize::MAX;
                let mut max_src_y = 0usize;

                for y_dst in start_y..end_y {
                    for &idx in &self.y_filters[y_dst].indices {
                        min_src_y = min_src_y.min(idx);
                        max_src_y = max_src_y.max(idx);
                    }
                }

                if min_src_y > max_src_y {
                    return;
                }

                let src_h_needed = max_src_y - min_src_y + 1;
                let mut temp = vec![0.0f64; src_h_needed * self.dst_w];

                for src_y in min_src_y..=max_src_y {
                    let local_y = src_y - min_src_y;
                    let src_row = &input[src_y * self.src_w..(src_y + 1) * self.src_w];
                    let temp_row = &mut temp[local_y * self.dst_w..(local_y + 1) * self.dst_w];

                    for (x_dst, filter) in temp_row.iter_mut().zip(&self.x_filters) {
                        let mut val = 0.0;
                        for (&idx, &w) in filter.indices.iter().zip(&filter.weights) {
                            val += src_row[idx] * w;
                        }
                        *x_dst = val;
                    }
                }

                for y_local in 0..(end_y - start_y) {
                    let y_dst = start_y + y_local;
                    let filter = &self.y_filters[y_dst];
                    let out_row = &mut out_chunk[y_local * self.dst_w..(y_local + 1) * self.dst_w];

                    for x in out_row.iter_mut() {
                        *x = 0.0;
                    }

                    for (&src_y_global, &w) in filter.indices.iter().zip(&filter.weights) {
                        let src_y_local = src_y_global - min_src_y;
                        let temp_row =
                            &temp[src_y_local * self.dst_w..(src_y_local + 1) * self.dst_w];
                        for (out, &val) in out_row.iter_mut().zip(temp_row.iter()) {
                            *out += val * w;
                        }
                    }
                }
            });

        output
    }
}

// ==========================================
// 3. 颜色转换逻辑
// ==========================================

pub fn convert_raw_rgb_to_linear_srgb_color_space(
    b01: Vec<f64>,
    b02: Vec<f64>,
    b03: Vec<f64>,
    b04: Vec<f64>,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let len = b01.len();

    // 预分配内存，提高性能
    let mut out_r = Vec::with_capacity(len);
    let mut out_g = Vec::with_capacity(len);
    let mut out_b = Vec::with_capacity(len);

    // 2. 迭代处理每一个像素
    for i in 0..len {
        let v_b01 = b01[i];
        let v_b02 = b02[i];
        let v_b03 = b03[i];
        let v_b04 = b04[i];

        // 3. NaN 检查：任意通道为 NaN，则输出全部为 NaN
        if v_b01.is_nan() || v_b02.is_nan() || v_b03.is_nan() || v_b04.is_nan() {
            out_r.push(f64::NAN);
            out_g.push(f64::NAN);
            out_b.push(f64::NAN);
            continue;
        }

        // // 4. 计算伪绿波段 (Pseudo Green) - 论文 Eq. 22
        let g_pseudo = 0.6321 * v_b02 + 0.2928 * v_b03 + 0.0751 * v_b04;
        // let g_pseudo = (1.0 - 0.07) * v_b02 + 0.07 * v_b04;
        // // // 5. 转换到 CIE 1931 XYZ - 论文 Eq. 20 / Table 5
        // // // 注意顺序: Red(B03), Pseudo-Green, Blue(B01)
        let x = 0.4677 * v_b03 + 0.3450 * g_pseudo + 0.1600 * v_b01;
        let y = 0.2210 * v_b03 + 0.6892 * g_pseudo + 0.0897 * v_b01;
        let z = 0.0001 * v_b03 + 0.0050 * g_pseudo + 1.0285 * v_b01;

        // 6. 转换到线性 sRGB (CIE XYZ to sRGB D65)
        let r_lin = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
        let g_lin = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
        let b_lin = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;

        // let r_lin = v_b03;
        // let g_lin = g_pseudo;
        // let b_lin = v_b01;

        out_r.push(r_lin);
        out_g.push(g_lin);
        out_b.push(b_lin);
    }
    (out_r, out_g, out_b)
}

// ==========================================
// 4. 几何与太阳位置逻辑 (优化切片转换)
// ==========================================

pub struct GeoLoader {
    lats: Vec<f32>,
    lons: Vec<f32>,
    vaas: Vec<f32>,
    pub width: usize,
}

impl GeoLoader {
    pub fn build<P: AsRef<Path>>(
        lat_p: P,
        lon_p: P,
        vaa_p: P,
        w: usize,
        _h: usize,
    ) -> Result<Self> {
        let read_f32 = |p: P| -> Result<Vec<f32>> {
            let f = File::open(p)?;
            let mmap = unsafe { MmapOptions::new().map(&f)? };
            Ok(bytemuck::cast_slice::<u8, f32>(&mmap).to_vec())
        };
        Ok(Self {
            lats: read_f32(lat_p)?,
            lons: read_f32(lon_p)?,
            vaas: read_f32(vaa_p)?,
            width: w,
        })
    }

    pub fn get_segment_slices(
        &self,
        start_line: usize,
        num_lines: usize,
    ) -> (&[f32], &[f32], &[f32]) {
        let start = start_line * self.width;
        let end = (start_line + num_lines) * self.width;
        (
            &self.lats[start..end],
            &self.lons[start..end],
            &self.vaas[start..end],
        )
    }
}

pub struct SolarResult {
    pub sza: Vec<f32>,
    pub saa: Vec<f32>,
    pub raa: Vec<f32>,
}
pub struct SolarEngine;

impl SolarEngine {
    pub fn calculate_segment(
        lats: &[f32],
        lons: &[f32],
        vaas: &[f32],
        full_width: usize,
        seg_height: usize,
        mjd_start: f64,
        mjd_end: f64,
        stride: usize,
    ) -> SolarResult {
        let out_w = (full_width + stride - 1) / stride;
        let out_h = (seg_height + stride - 1) / stride;

        let time_step = if seg_height > 1 {
            (mjd_end - mjd_start) / ((seg_height - 1) as f64)
        } else {
            0.0
        };

        let results: Vec<(f32, f32, f32)> = (0..out_h)
            .into_par_iter()
            .flat_map_iter(|r| {
                let src_y = r * stride;
                let mjd = mjd_start + (src_y as f64 * time_step);
                let jd = mjd + 2_400_000.5;
                let sun = SunEphemeris::from_jd(jd);

                (0..out_w).map(move |c| {
                    let idx = src_y * full_width + (c * stride);
                    let (lat, lon, vaa_static) = (lats[idx], lons[idx], vaas[idx]);

                    if !lat.is_finite() || !lon.is_finite() || !vaa_static.is_finite() {
                        return (f32::NAN, f32::NAN, f32::NAN);
                    }

                    let (sza, saa) = Self::pixel_sun_pos(lat, lon, &sun);

                    let mut raa = (saa - vaa_static).abs();
                    if raa > 180.0 {
                        raa = 360.0 - raa;
                    }

                    (sza, saa, raa)
                })
            })
            .collect();

        SolarResult {
            sza: results.iter().map(|x| x.0).collect(),
            saa: results.iter().map(|x| x.1).collect(),
            raa: results.iter().map(|x| x.2).collect(),
        }
    }

    #[inline(always)]
    fn pixel_sun_pos(lat_deg: f32, lon_deg: f32, sun: &SunEphemeris) -> (f32, f32) {
        let lat = (lat_deg as f64).to_radians();
        let lon = (lon_deg as f64).to_radians();

        let lha = sun.gha + lon;

        let cos_sza =
            (lat.sin() * sun.dec.sin() + lat.cos() * sun.dec.cos() * lha.cos()).clamp(-1.0, 1.0);
        let sza = cos_sza.acos().to_degrees();

        let y = -lha.sin();
        let x = sun.dec.tan() * lat.cos() - lat.sin() * lha.cos();
        let mut saa = y.atan2(x).to_degrees();

        if saa < 0.0 {
            saa += 360.0;
        }

        (sza as f32, saa as f32)
    }
}

struct SunEphemeris {
    dec: f64,
    gha: f64,
}

impl SunEphemeris {
    fn from_jd(jd: f64) -> Self {
        let d = jd - 2451545.0;

        let l = (280.460 + 0.9856474 * d).to_radians();
        let g = (357.528 + 0.9856003 * d).to_radians();

        let lambda =
            l + (1.915_f64.to_radians() * g.sin()) + (0.020_f64.to_radians() * (2.0 * g).sin());

        let epsilon = (23.439 - 0.0000004 * d).to_radians();

        let dec = (epsilon.sin() * lambda.sin()).asin();
        let ra = (epsilon.cos() * lambda.sin()).atan2(lambda.cos());

        let jd0 = (jd + 0.5).floor() - 0.5;
        let t = (jd0 - 2451545.0) / 36525.0;
        let gmst_deg = 280.46061837 + 360.98564736629 * (jd - 2451545.0) + 0.000387933 * t * t
            - (t * t * t) / 38710000.0;

        let gha = (gmst_deg.to_radians() - ra).rem_euclid(std::f64::consts::TAU);

        Self { dec, gha }
    }
}
