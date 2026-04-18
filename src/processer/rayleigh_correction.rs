//! 瑞利散射校正模組
//!
//! 實現完整的大氣校正公式:
//! ρ_TOA = T_g × [ρ_path + T_s × ρ_surf / (1 - S × ρ_surf)]
//!
//! 反演地表反射率:
//! ρ_surf = (ρ_TOA / T_g - ρ_path) / (T_s + S × (ρ_TOA / T_g - ρ_path))

use anyhow::Result;
use memmap2::MmapOptions;
use rayon::prelude::*;
use std::fs::File;
use std::path::Path;
use std::time::Instant;

// ==========================================
// 1. LUT 數據結構 (內存優化版)
// ==========================================

/// 瑞利散射查找表
///
/// 維度順序: (sza, vza, elev, raa)
/// 所有數據使用 f32 以節省內存
#[derive(Debug)]
pub struct RayleighLUT {
    /// 高程網格 (km): [0.0, 0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 7.0, 10.0, 15.0]
    elevations: Vec<f32>,
    /// 太陽天頂角網格 (度): 0-80°
    sza_grid: Vec<f32>,
    /// 觀測天頂角網格 (度): 0-80°
    vza_grid: Vec<f32>,
    /// 相對方位角網格 (度): 0-180°
    raa_grid: Vec<f32>,

    /// 瑞利路徑反射率 (sza, vza, elev, raa)
    path_refl: Vec<f32>,
    /// 氣體透過率
    trans_gas: Vec<f32>,
    /// 散射透過率
    trans_scat: Vec<f32>,
    /// 球面反照率
    albedo: Vec<f32>,

    // 維度大小
    n_sza: usize,
    n_vza: usize,
    n_elev: usize,
    n_raa: usize,
}

impl RayleighLUT {
    /// 從預處理的二進制文件加載 LUT
    ///
    /// 二進制格式:
    /// - Header: [n_elev, n_sza, n_vza, n_raa] (4 x u32, big-endian)
    /// - Reserved: 24 bytes
    /// - 坐標網格: elev, sza, vza, raa (各 n x f32)
    /// - 數據: path_refl, trans_gas, trans_scat, albedo (各 total x f32)
    pub fn from_binary<P: AsRef<Path>>(path: P) -> Result<Self> {
        let start = Instant::now();
        let file = File::open(&path)?;
        let mmap = unsafe { MmapOptions::new().map(&file)? };
        let data: &[u8] = &mmap;

        // 解析 Header
        if data.len() < 40 {
            anyhow::bail!("LUT 文件太小: {} bytes", data.len());
        }

        let n_elev = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let n_sza = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
        let n_vza = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;
        let n_raa = u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize;

        let total = n_sza * n_vza * n_elev * n_raa;
        let expected_size = 40 + (n_elev + n_sza + n_vza + n_raa) * 4 + total * 4 * 4;

        if data.len() != expected_size {
            anyhow::bail!(
                "LUT 文件大小不匹配: 預期 {} bytes, 實際 {} bytes",
                expected_size,
                data.len()
            );
        }

        let mut offset = 40; // 跳過 header

        // 讀取坐標網格
        let elevations = bytemuck::cast_slice::<u8, f32>(&data[offset..offset + n_elev * 4]).to_vec();
        offset += n_elev * 4;

        let sza_grid = bytemuck::cast_slice::<u8, f32>(&data[offset..offset + n_sza * 4]).to_vec();
        offset += n_sza * 4;

        let vza_grid = bytemuck::cast_slice::<u8, f32>(&data[offset..offset + n_vza * 4]).to_vec();
        offset += n_vza * 4;

        let raa_grid = bytemuck::cast_slice::<u8, f32>(&data[offset..offset + n_raa * 4]).to_vec();
        offset += n_raa * 4;

        // 讀取數據
        let path_refl = bytemuck::cast_slice::<u8, f32>(&data[offset..offset + total * 4]).to_vec();
        offset += total * 4;

        let trans_gas = bytemuck::cast_slice::<u8, f32>(&data[offset..offset + total * 4]).to_vec();
        offset += total * 4;

        let trans_scat = bytemuck::cast_slice::<u8, f32>(&data[offset..offset + total * 4]).to_vec();
        offset += total * 4;

        let albedo = bytemuck::cast_slice::<u8, f32>(&data[offset..offset + total * 4]).to_vec();

        println!(
            "  LUT 加載完成: {}x{}x{}x{} = {:.2} MB (耗時: {:?})",
            n_sza, n_vza, n_elev, n_raa,
            (total * 4 * 4) as f64 / 1024.0 / 1024.0,
            start.elapsed()
        );

        Ok(RayleighLUT {
            elevations,
            sza_grid,
            vza_grid,
            raa_grid,
            path_refl,
            trans_gas,
            trans_scat,
            albedo,
            n_sza,
            n_vza,
            n_elev,
            n_raa,
        })
    }

    /// 計算線性索引
    #[inline(always)]
    fn idx(&self, i_sza: usize, i_vza: usize, i_elev: usize, i_raa: usize) -> usize {
        ((i_sza * self.n_vza + i_vza) * self.n_elev + i_elev) * self.n_raa + i_raa
    }

    /// 四線性插值獲取所有大氣參數
    ///
    /// 返回: (path_refl, trans_gas, trans_scat, albedo)
    #[inline(always)]
    fn interpolate(&self, sza: f32, vza: f32, elev: f32, raa: f32) -> (f32, f32, f32, f32) {
        // 查找各維度的索引和權重
        let (i_sza0, i_sza1, w_sza) = match self.find_index(&self.sza_grid, sza) {
            Some(v) => v,
            None => return (f32::NAN, f32::NAN, f32::NAN, f32::NAN),
        };

        let (i_vza0, i_vza1, w_vza) = match self.find_index(&self.vza_grid, vza) {
            Some(v) => v,
            None => return (f32::NAN, f32::NAN, f32::NAN, f32::NAN),
        };

        let (i_elev0, i_elev1, w_elev) = match self.find_index(&self.elevations, elev) {
            Some(v) => v,
            None => return (f32::NAN, f32::NAN, f32::NAN, f32::NAN),
        };

        let (i_raa0, i_raa1, w_raa) = match self.find_index(&self.raa_grid, raa) {
            Some(v) => v,
            None => return (f32::NAN, f32::NAN, f32::NAN, f32::NAN),
        };

        // 四線性插值宏
        macro_rules! interp_var {
            ($data:expr) => {{
                // 獲取 16 個頂點值
                let v0000 = $data[self.idx(i_sza0, i_vza0, i_elev0, i_raa0)];
                let v0001 = $data[self.idx(i_sza0, i_vza0, i_elev0, i_raa1)];
                let v0010 = $data[self.idx(i_sza0, i_vza0, i_elev1, i_raa0)];
                let v0011 = $data[self.idx(i_sza0, i_vza0, i_elev1, i_raa1)];
                let v0100 = $data[self.idx(i_sza0, i_vza1, i_elev0, i_raa0)];
                let v0101 = $data[self.idx(i_sza0, i_vza1, i_elev0, i_raa1)];
                let v0110 = $data[self.idx(i_sza0, i_vza1, i_elev1, i_raa0)];
                let v0111 = $data[self.idx(i_sza0, i_vza1, i_elev1, i_raa1)];
                let v1000 = $data[self.idx(i_sza1, i_vza0, i_elev0, i_raa0)];
                let v1001 = $data[self.idx(i_sza1, i_vza0, i_elev0, i_raa1)];
                let v1010 = $data[self.idx(i_sza1, i_vza0, i_elev1, i_raa0)];
                let v1011 = $data[self.idx(i_sza1, i_vza0, i_elev1, i_raa1)];
                let v1100 = $data[self.idx(i_sza1, i_vza1, i_elev0, i_raa0)];
                let v1101 = $data[self.idx(i_sza1, i_vza1, i_elev0, i_raa1)];
                let v1110 = $data[self.idx(i_sza1, i_vza1, i_elev1, i_raa0)];
                let v1111 = $data[self.idx(i_sza1, i_vza1, i_elev1, i_raa1)];

                // RAA 插值
                let v000 = v0000 * (1.0 - w_raa) + v0001 * w_raa;
                let v001 = v0010 * (1.0 - w_raa) + v0011 * w_raa;
                let v010 = v0100 * (1.0 - w_raa) + v0101 * w_raa;
                let v011 = v0110 * (1.0 - w_raa) + v0111 * w_raa;
                let v100 = v1000 * (1.0 - w_raa) + v1001 * w_raa;
                let v101 = v1010 * (1.0 - w_raa) + v1011 * w_raa;
                let v110 = v1100 * (1.0 - w_raa) + v1101 * w_raa;
                let v111 = v1110 * (1.0 - w_raa) + v1111 * w_raa;

                // Elevation 插值
                let v00 = v000 * (1.0 - w_elev) + v001 * w_elev;
                let v01 = v010 * (1.0 - w_elev) + v011 * w_elev;
                let v10 = v100 * (1.0 - w_elev) + v101 * w_elev;
                let v11 = v110 * (1.0 - w_elev) + v111 * w_elev;

                // VZA 插值
                let v0 = v00 * (1.0 - w_vza) + v01 * w_vza;
                let v1 = v10 * (1.0 - w_vza) + v11 * w_vza;

                // SZA 插值
                v0 * (1.0 - w_sza) + v1 * w_sza
            }};
        }

        (
            interp_var!(self.path_refl),
            interp_var!(self.trans_gas),
            interp_var!(self.trans_scat),
            interp_var!(self.albedo),
        )
    }

    /// 在有序網格中查找插值位置
    #[inline(always)]
    fn find_index(&self, grid: &[f32], val: f32) -> Option<(usize, usize, f32)> {
        if val.is_nan() || val < grid[0] || val > *grid.last().unwrap() {
            return None;
        }

        match grid.binary_search_by(|probe| {
            probe
                .partial_cmp(&val)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            Ok(idx) => Some((idx, idx, 0.0)),
            Err(idx) => {
                if idx == 0 || idx >= grid.len() {
                    return None;
                }
                let x0 = grid[idx - 1];
                let x1 = grid[idx];
                let w = (val - x0) / (x1 - x0);
                Some((idx - 1, idx, w))
            }
        }
    }
}

// ==========================================
// 2. 高程數據管理
// ==========================================

/// 高程數據加載器
///
/// 支持從二進制文件加載或使用默認值
pub struct ElevationData {
    data: Vec<f32>,
    width: usize,
}

impl ElevationData {
    /// 從二進制文件加載 (單位: 米)
    pub fn from_binary<P: AsRef<Path>>(path: P, width: usize, height: usize) -> Result<Self> {
        let start = Instant::now();
        let file = File::open(&path)?;
        let mmap = unsafe { MmapOptions::new().map(&file)? };
        let raw: &[f32] = bytemuck::cast_slice(&mmap);

        if raw.len() != width * height {
            anyhow::bail!(
                "高程數據大小不匹配: 預期 {}, 實際 {}",
                width * height,
                raw.len()
            );
        }

        // 轉換為公里並處理無效值
        let data: Vec<f32> = raw
            .iter()
            .map(|&x| if x.is_finite() && x >= -500.0 { x / 1000.0 } else { 0.0 })
            .collect();

        println!(
            "  高程數據加載完成: {}x{} (耗時: {:?})",
            width, height,
            start.elapsed()
        );

        Ok(ElevationData { data, width })
    }

    /// 使用默認高程 (海平面)
    pub fn default_elevation(width: usize, height: usize) -> Self {
        ElevationData {
            data: vec![0.0; width * height],
            width,
        }
    }

    /// 獲取指定區域的高程切片 (公里)
    pub fn get_segment(&self, start_line: usize, num_lines: usize) -> &[f32] {
        let start = start_line * self.width;
        let end = (start_line + num_lines) * self.width;
        &self.data[start..end.min(self.data.len())]
    }
}

// ==========================================
// 3. 瑞利散射校正器
// ==========================================

/// 瑞利散射校正器
///
/// 實現基於 LUT 的大氣校正
pub struct RayleighCorrector {
    luts: [RayleighLUT; 4], // B01, B02, B03, B04
    elevation: ElevationData,
    width: usize,
}

/// 校正結果
pub struct CorrectionResult {
    pub refl: Vec<f64>,
    pub path_refl: Vec<f32>,
    pub trans_total: Vec<f32>,
}

impl RayleighCorrector {
    /// 創建校正器
    ///
    /// # 參數
    /// - `lut_dir`: LUT 二進制文件目錄
    /// - `elev_path`: 高程數據路徑 (可選，None 則使用海平面)
    /// - `width`, `height`: 圖像尺寸
    pub fn new<P: AsRef<Path>>(
        lut_dir: P,
        elev_path: Option<P>,
        width: usize,
        height: usize,
    ) -> Result<Self> {
        println!("正在加載瑞利散射 LUT...");

        let lut_dir = lut_dir.as_ref();

        // 並行加載 4 個波段的 LUT
        let (b01, (b02, (b03, b04))) = rayon::join(
            || RayleighLUT::from_binary(lut_dir.join("H09_LUT_B01.bin")),
            || {
                rayon::join(
                    || RayleighLUT::from_binary(lut_dir.join("H09_LUT_B02.bin")),
                    || {
                        rayon::join(
                            || RayleighLUT::from_binary(lut_dir.join("H09_LUT_B03.bin")),
                            || RayleighLUT::from_binary(lut_dir.join("H09_LUT_B04.bin")),
                        )
                    },
                )
            },
        );

        let luts = [b01?, b02?, b03?, b04?];

        // 加載高程數據
        let elevation = match elev_path {
            Some(path) => ElevationData::from_binary(path, width, height)?,
            None => {
                println!("  使用默認高程 (海平面)");
                ElevationData::default_elevation(width, height)
            }
        };

        Ok(RayleighCorrector {
            luts,
            elevation,
            width,
        })
    }

    /// 角度退化权重:
    /// - <= 75°: 完全使用瑞利校正结果
    /// - >= 80°: 完全退回原始 TOA
    /// - 75°~80°: 使用 smoothstep 平滑过渡
    #[inline(always)]
    fn correction_blend_weight(sza: f32, vza: f32) -> f64 {
        const DEG_START: f64 = 78.0;
        const DEG_END: f64 = 88.0;

        let angle_metric = sza.max(vza) as f64;

        if angle_metric <= DEG_START {
            return 1.0;
        }
        if angle_metric >= DEG_END {
            return 0.0;
        }

        let t = ((angle_metric - DEG_START) / (DEG_END - DEG_START)).clamp(0.0, 1.0);
        let smooth = t * t * (3.0 - 2.0 * t);
        1.0 - smooth
    }
    /// 對單個波段進行瑞利散射校正
    ///
    /// # 大氣校正公式
    /// ρ_TOA = T_g × [ρ_path + T_s × ρ_surf / (1 - S × ρ_surf)]
    ///
    /// 反演:
    /// ρ_surf = (ρ_TOA / T_g - ρ_path) × T_s / (1 + S × (ρ_TOA / T_g - ρ_path))
    ///
    /// 簡化版 (忽略多次散射):
    /// ρ_surf = (ρ_TOA / T_g - ρ_path) / (T_s + S × (ρ_TOA / T_g - ρ_path))
    ///
    /// 本實現增加高角度平滑退化:
    /// - <= 75° 完全使用 ρ_surf
    /// - >= 80° 完全退回 ρ_TOA
    /// - 75°~80° 在兩者之間平滑混合
    pub fn correct_band(
        &self,
        band_idx: usize,
        refl_toa: &[f64],
        sza: &[f32],
        vza: &[f32],
        raa: &[f32],
        elev: &[f32],
        bt_data: &[f64],
    ) -> CorrectionResult {
        let start = Instant::now();
        let lut = &self.luts[band_idx];
        let n = refl_toa.len();

        assert_eq!(sza.len(), n, "sza 长度不匹配");
        assert_eq!(vza.len(), n, "vza 长度不匹配");
        assert_eq!(raa.len(), n, "raa 长度不匹配");
        assert_eq!(elev.len(), n, "elev 长度不匹配");
        assert_eq!(bt_data.len(), n, "bt_data 长度不匹配");

        let result: Vec<(f64, f32, f32)> = refl_toa
            .par_iter()
            .zip(sza.par_iter())
            .zip(vza.par_iter())
            .zip(raa.par_iter())
            .zip(elev.par_iter())
            .zip(bt_data.par_iter())
            .map(|(((((&rho_toa, &sza), &vza), &raa), &elev), &bt)| {
                if rho_toa.is_nan() || sza.is_nan() || vza.is_nan() || raa.is_nan() || elev.is_nan() || bt.is_nan() {
                    return (f64::NAN, f32::NAN, f32::NAN);
                }

                let blend_weight = Self::correction_blend_weight(sza, vza);

                if blend_weight <= 0.0 {
                    return (rho_toa.clamp(0.0, 1.0), f32::NAN, f32::NAN);
                }

                let (path_refl, trans_gas, trans_scat, albedo) =
                    lut.interpolate(sza, vza, elev, raa);

                if path_refl.is_nan() || trans_gas.is_nan() || trans_scat.is_nan() || albedo.is_nan() {
                    return (rho_toa.clamp(0.0, 1.0), f32::NAN, f32::NAN);
                }

                // let psf = crate::processer::processer::calculate_scaling_factor(bt);
                // let path_refl_scaled = (path_refl as f64) * psf;
                
                let tg = trans_gas.max(1e-6) as f64;
                let ts = trans_scat.max(1e-6) as f64;
                let s = albedo.max(0.0) as f64;

                let x = rho_toa / tg - path_refl as f64;


                if !x.is_finite() {
                    return (rho_toa.clamp(0.0, 1.0), path_refl, trans_gas * trans_scat);
                }

                let rho_corr = if x <= 0.0 {
                    0.0
                } else {
                    let denom = ts + s * x;
                    if denom <= 1e-12 || !denom.is_finite() {
                        rho_toa.clamp(0.0, 1.0)
                    } else {
                        (x / denom).clamp(0.0, 1.0)
                    }
                };

                let rho_final = (blend_weight * rho_corr + (1.0 - blend_weight) * rho_toa)
                    .clamp(0.0, 1.0);

                (rho_final, path_refl, (trans_gas * trans_scat))
            })
            .collect();

        let refl: Vec<f64> = result.iter().map(|(r, _, _)| *r).collect();
        let path_refl: Vec<f32> = result.iter().map(|(_, p, _)| *p).collect();
        let trans_total: Vec<f32> = result.iter().map(|(_, _, t)| *t).collect();

        println!(
            "  Band {} 校正完成: {} 像素 (耗時: {:?})",
            band_idx + 1,
            n,
            start.elapsed()
        );

        CorrectionResult {
            refl,
            path_refl,
            trans_total,
        }
    }

    /// 對所有四個波段進行校正
    pub fn correct_all_bands(
        &self,
        refl_b01: &[f64],
        refl_b02: &[f64],
        refl_b03: &[f64],
        refl_b04: &[f64],
        refl_b13: &[f64],
        sza: &[f32],
        vza: &[f32],
        raa: &[f32],
        start_line: usize,
        seg_height: usize,
    ) -> Result<[CorrectionResult; 4]> {
        let elev = self.elevation.get_segment(start_line, seg_height);

        // 並行校正四個波段
        let (r1, (r2, (r3, r4))) = rayon::join(
            || self.correct_band(0, refl_b01, sza, vza, raa, elev, refl_b13),
            || {
                rayon::join(
                    || self.correct_band(1, refl_b02, sza, vza, raa, elev, refl_b13),
                    || {
                        rayon::join(
                            || self.correct_band(2, refl_b03, sza, vza, raa, elev, refl_b13),
                            || self.correct_band(3, refl_b04, sza, vza, raa, elev, refl_b13),
                        )
                    },
                )
            },
        );


        Ok([r1, r2, r3, r4])
    }

    /// 獲取高程數據切片
    pub fn get_elevation(&self, start_line: usize, seg_height: usize) -> &[f32] {
        self.elevation.get_segment(start_line, seg_height)
    }
}

// ==========================================
// 4. 幾何數據加載器 (含 VZA)
// ==========================================

/// 幾何數據加載器
pub struct GeometryLoader {
    lats: Vec<f32>,
    lons: Vec<f32>,
    vaas: Vec<f32>,
    vzas: Vec<f32>,
    pub width: usize,
}

impl GeometryLoader {
    /// 從二進制文件加載幾何數據
    pub fn build<P: AsRef<Path>>(
        lat_path: P,
        lon_path: P,
        vaa_path: P,
        vza_path: P,
        width: usize,
        _height: usize,
    ) -> Result<Self> {
        let start = Instant::now();

        let load_f32 = |p: &Path| -> Result<Vec<f32>> {
            let f = File::open(p)?;
            let mmap = unsafe { MmapOptions::new().map(&f)? };
            Ok(bytemuck::cast_slice(&mmap).to_vec())
        };

        let lat_path = lat_path.as_ref();
        let lon_path = lon_path.as_ref();
        let vaa_path = vaa_path.as_ref();
        let vza_path = vza_path.as_ref();

        let (lats, (lons, (vaas, vzas))) = rayon::join(
            || load_f32(lat_path),
            || {
                rayon::join(
                    || load_f32(lon_path),
                    || {
                        rayon::join(
                            || load_f32(vaa_path),
                            || load_f32(vza_path),
                        )
                    },
                )
            },
        );

        let lats = lats?;
        let lons = lons?;
        let vaas = vaas?;
        let vzas = vzas?;

        if lats.len() != lons.len() || lats.len() != vaas.len() || lats.len() != vzas.len() {
            anyhow::bail!("几何数据长度不一致");
        }
        if lats.len() % width != 0 {
            anyhow::bail!("几何数据长度不是 width 的整数倍");
        }

        println!("  几何数据加载完成 (耗时: {:?})", start.elapsed());

        Ok(GeometryLoader {
            lats,
            lons,
            vaas,
            vzas,
            width,
        })
    }

    /// 獲取指定區域的幾何數據切片
    pub fn get_segment_slices(
        &self,
        start_line: usize,
        num_lines: usize,
    ) -> (&[f32], &[f32], &[f32], &[f32]) {
        let start = start_line * self.width;
        let end = (start_line + num_lines) * self.width;
        (
            &self.lats[start..end],
            &self.lons[start..end],
            &self.vaas[start..end],
            &self.vzas[start..end],
        )
    }
}

// ==========================================
// 5. 測試與工具函數
// ==========================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lut_interpolation() {
        // 測試插值邏輯的正確性
        // 需要實際的 LUT 文件才能運行
    }
}