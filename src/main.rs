//! # HSD Data Process 主程序
//!
//! 这个程序是HSD数据处理管道的主入口点。
//! 它读取Himawari卫星的HSD格式数据，进行大气校正和几何校正，
//! 然后生成真彩色TIFF图像输出。
//!
//! ## 处理流程
//!
//! 1. 加载预计算的几何数据
//! 2. 初始化瑞利校正器和Lanczos缩放器
//! 3. 扫描02目录中的HSD文件
//! 4. 对每个时间序列进行并行处理
//! 5. 生成TIFF输出文件
//!
//! ## 配置
//!
//! 主要配置参数：
//! - 图像分辨率: 22000x22000 (0.5km)
//! - 几何数据路径: proj_precompute/h09_geometry_data_v2/
//! - LUT路径: lut_binary/
//! - 输入目录: 02/
//!
//! ## 输出
//!
//! 生成的TIFF文件包含16位RGB真彩色数据，无压缩。

use hsd_data_process::file_struct::file_struct::HsdFile;
use hsd_data_process::processer::processer::{
    LanczosScaler, SolarEngine, convert_raw_rgb_to_linear_srgb_color_space,
    data_calibration_correction,
};
use hsd_data_process::processer::rayleigh_correction::{
    GeometryLoader, RayleighCorrector,
};
use hsd_data_process::reader::hsd_organizer::get_full_process_file;
use hsd_data_process::writer::writer::write_rgb_chunk_to_tiff;
use rayon::prelude::*;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use tiff::encoder::{Compression, TiffEncoder, colortype};

// fn stream_write(path: &str, data: &[f32]) -> anyhow::Result<()> {
//     let file = OpenOptions::new().create(true).append(true).open(path)?;
//     let mut writer = BufWriter::new(file);
//     writer.write_all(bytemuck::cast_slice(data))?;
//     Ok(())
// }

fn main() {
    // 1. 全局静态初始化 (只执行一次)
    let width = 22000;
    let height = 22000;
    let lat_path = "proj_precompute/h09_geometry_data_v2/H09_05km_Lat.dat";
    let lon_path = "proj_precompute/h09_geometry_data_v2/H09_05km_Lon.dat";
    let vaa_path = "proj_precompute/h09_geometry_data_v2/H09_05km_VAA.dat";
    let vza_path = "proj_precompute/h09_geometry_data_v2/H09_05km_VZA.dat";

    println!("正在加载几何数据、瑞利 LUT 并预计算 Lanczos 权重...");
    let geometry_loader =
        GeometryLoader::build(lat_path, lon_path, vaa_path, vza_path, width, height).unwrap();
    let rayleigh_corrector =
        RayleighCorrector::new("lut_binary", Option::<&str>::None, width, height).unwrap();

    let scaler_4x = LanczosScaler::new(5500, 550);
    let scaler = LanczosScaler::new(11000, 1100);

    // let outputs = ["SZA.dat", "SAA.dat", "RAA.dat"];
    // for out in outputs {
    //     if Path::new(out).exists() {
    //         std::fs::remove_file(out).unwrap();
    //     }
    // }

    let time_series_map = get_full_process_file(Path::new("02"));

    for (timestamp, full_disk_data) in time_series_map {
        println!("\n=== 处理时间点: {} ===", timestamp);

        let tiff_file = File::create(format!("output{}.tif", timestamp)).unwrap();
        let mut writer = BufWriter::new(tiff_file);
        let mut tiff = TiffEncoder::new(&mut writer)
            .unwrap()
            .with_compression(Compression::Uncompressed);

        let mut image_encoder = tiff.new_image::<colortype::RGB16>(22000, 22000).unwrap();
        image_encoder.rows_per_strip(2200).unwrap();

        for (seg_idx, item) in full_disk_data.segments.iter().enumerate() {
            println!("Processing Segment {}/10", seg_idx + 1);

            let processed_bands: Vec<_> = [1, 2, 3, 4, 13]
                .into_par_iter()
                .map(|band_num| {
                    // Update to locate the correct band file based on band number
                    let path = item
                        .files
                        .get(band_num - 1)
                        .expect("Missing Band File")
                        .as_ref()
                        .unwrap();
                    let hsd = HsdFile::build(path.to_str().unwrap()).expect("HSD Parse Error");
                    data_calibration_correction(hsd)
                })
                .collect();

            let [p_b01, p_b02, p_b03, p_b04, p_b13] = processed_bands.try_into().ok().unwrap();

            let start_line = seg_idx * 2200;
            let seg_height = 2200;
            let (lats, lons, vaas, vzas) =
                geometry_loader.get_segment_slices(start_line, seg_height);

            let first_mjd = p_b03.hsd_metadata.observation_time_info.times[0].observation_time_mjd;
            let last_mjd = p_b03
                .hsd_metadata
                .observation_time_info
                .times
                .last()
                .unwrap()
                .observation_time_mjd;

            let sol = SolarEngine::calculate_segment(
                lats, lons, vaas, width, seg_height, first_mjd, last_mjd, 1,
            );

            // stream_write("SZA.dat", &sol.sza).unwrap();
            // stream_write("SAA.dat", &sol.saa).unwrap();
            // stream_write("RAA.dat", &sol.raa).unwrap();

            let b01_up_toa = scaler.resize(&p_b01.refl_bright_temp_result);
            let b02_up_toa = scaler.resize(&p_b02.refl_bright_temp_result);
            let b04_up_toa = scaler.resize(&p_b04.refl_bright_temp_result);
            let b13_up_toa = scaler_4x.resize(&p_b13.refl_bright_temp_result);
            let b13_up_toa = scaler.resize(&b13_up_toa);
            let b03_toa = p_b03.refl_bright_temp_result;

            assert_eq!(b01_up_toa.len(), sol.sza.len(), "B01 与几何长度不一致");
            assert_eq!(b02_up_toa.len(), sol.sza.len(), "B02 与几何长度不一致");
            assert_eq!(b03_toa.len(), sol.sza.len(), "B03 与几何长度不一致");
            assert_eq!(b04_up_toa.len(), sol.sza.len(), "B04 与几何长度不一致");
            assert_eq!(b13_up_toa.len(), sol.sza.len(), "B13 与几何长度不一致");

            let [corr_b01, corr_b02, corr_b03, corr_b04] = rayleigh_corrector
                .correct_all_bands(
                    &b01_up_toa,
                    &b02_up_toa,
                    &b03_toa,
                    &b04_up_toa,
                    &b13_up_toa,
                    &sol.sza,
                    vzas,
                    &sol.raa,
                    start_line,
                    seg_height,
                )
                .unwrap();

            let (out_r, out_g, out_b) = convert_raw_rgb_to_linear_srgb_color_space(
                corr_b01.refl,
                corr_b02.refl,
                corr_b03.refl,
                corr_b04.refl,
            );

            write_rgb_chunk_to_tiff(
                &mut image_encoder,
                &out_r,
                &out_g,
                &out_b,
                (1.0 / 2.2, false),
            )
            .unwrap();
        }
    }
    println!("Mission Accomplished.");
}
