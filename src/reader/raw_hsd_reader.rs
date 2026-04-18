use crate::file_struct::file_struct::*;
use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use bzip2::bufread::MultiBzDecoder;
use std::error::Error;
// 1. 引入 bzip2 解压器
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::time::Instant;
// #################################################################################
// # 公共读取函数
// #################################################################################

impl HsdFile {
    pub fn build(file_path: &str) -> Result<HsdFile, Box<dyn Error>> {
        read_hsd_file(file_path)
    }
}

/// 读取 bz2 压缩的 HSD 文件，解压并解析 11 个元数据块和数据块。
///
/// 此函数首先将整个 .bz2 文件读入内存，然后进行解压，最后解析数据。
/// 这种方式通过最小化 I/O 和在内存中处理所有操作来优化性能。
///
/// # 参数
/// * `file_path`: bz2 压缩的 HSD 数据文件的路径。
///
/// # 返回
/// * `Result<(HsdMetadata, Vec<u16>), Box<dyn Error>>`:
///   如果成功，返回一个包含元数据和像素数据的元组。
///   如果失败，返回一个描述错误的动态错误类型。
fn read_hsd_file(file_path: &str) -> Result<HsdFile, Box<dyn Error>> {
    let start = Instant::now();
    let file = File::open(file_path)?;
    let buf_reader = BufReader::new(file); // 减少系统调用

    let mut decoder = MultiBzDecoder::new(buf_reader);

    let mut buffer = Vec::new();

    decoder.read_to_end(&mut buffer)?;
    let end = start.elapsed();
    println!("解压耗时：{:?}", end);
    println!("解压后文件大小：{} bytes", buffer.len());

    let mut cursor = Cursor::new(buffer.as_slice());

    // 解析报头块
    let basic_info = parse_block_1(&mut cursor)?;

    // 根据块 #1 确定字节序
    let endianness = if basic_info.byte_order == "Big Endian" {
        Endian::Big
    } else {
        Endian::Little
    };

    let data_info = parse_block_2(&mut cursor, &endianness)?;
    let projection_info = parse_block_3(&mut cursor, &endianness)?;
    let navigation_info = parse_block_4(&mut cursor, &endianness)?;
    let calibration_info = parse_block_5(&mut cursor, &endianness)?;
    let inter_calibration_info = parse_block_6(&mut cursor, &endianness)?;
    let segment_info = parse_block_7(&mut cursor, &endianness)?;
    let nav_correction_info = parse_block_8(&mut cursor, &endianness)?;
    let observation_time_info = parse_block_9(&mut cursor, &endianness)?;
    let error_info = parse_block_10(&mut cursor, &endianness)?;

    // 跳过块 #11 (备用块)
    skip_block_11(&mut cursor)?;

    // 组装元数据结构
    let metadata = HsdMetadata {
        basic_info,
        data_info,
        projection_info,
        navigation_info,
        calibration_info,
        inter_calibration_info,
        segment_info,
        nav_correction_info,
        observation_time_info,
        error_info,
    };

    // 解析块 #12 (数据块)
    let data_block = parse_block_12(&mut cursor, &endianness, &metadata.data_info)?;

    // 组装 HSD 文件结构
    let data = HsdData {
        size: (metadata.data_info.columns, metadata.data_info.lines),
        data: data_block,
    };
    let hsd_file = HsdFile { metadata, data };

    Ok(hsd_file)
}

// #################################################################################
// # 内部解析逻辑 (这部分代码无需任何修改)
// #################################################################################

// 用于处理字节序的枚举
enum Endian {
    Little,
    Big,
}

/// 从字节切片中读取一个固定长度、经过 null 填充的 ASCII 字符串。
fn read_c_string<R: Read>(reader: &mut R, len: usize) -> Result<String, Box<dyn Error>> {
    let mut buf = vec![0; len];
    reader.read_exact(&mut buf)?;
    // 找到第一个 null 字符的位置，如果不存在则使用整个长度
    let end = buf.iter().position(|&b| b == 0).unwrap_or(len);
    // 从 UTF-8 转换，忽略无效字符，并去除首尾空白
    Ok(String::from_utf8_lossy(&buf[..end]).trim().to_string())
}

/// 解析块 #1: 基本信息块
fn parse_block_1(cursor: &mut Cursor<&[u8]>) -> Result<BasicInfoBlock, Box<dyn Error>> {
    cursor.seek(SeekFrom::Start(0))?; // 确保从文件头开始

    let _header_block_num = cursor.read_u8()?; // 1
    let _block_length = cursor.read_u16::<LittleEndian>()?; // 2, 文档规定此块长度固定
    let _total_header_blocks = cursor.read_u16::<LittleEndian>()?; // 3

    let byte_order_flag = cursor.read_u8()?; // 4
    let byte_order = if byte_order_flag == 1 {
        "Big Endian"
    } else {
        "Little Endian"
    }
    .to_string();
    let endianness = if byte_order_flag == 1 {
        Endian::Big
    } else {
        Endian::Little
    };

    let satellite_name = read_c_string(cursor, 16)?; // 5
    let processing_center_name = read_c_string(cursor, 16)?; // 6
    let observation_area = read_c_string(cursor, 4)?; // 7
    cursor.seek(SeekFrom::Current(2))?; // 8, 跳过"其他观测信息"

    let observation_timeline = match endianness {
        Endian::Big => cursor.read_u16::<BigEndian>()?,
        Endian::Little => cursor.read_u16::<LittleEndian>()?,
    }; // 9

    let observation_start_time_mjd = match endianness {
        Endian::Big => cursor.read_f64::<BigEndian>()?,
        Endian::Little => cursor.read_f64::<LittleEndian>()?,
    }; // 10

    let observation_end_time_mjd = match endianness {
        Endian::Big => cursor.read_f64::<BigEndian>()?,
        Endian::Little => cursor.read_f64::<LittleEndian>()?,
    }; // 11

    let file_creation_time_mjd = match endianness {
        Endian::Big => cursor.read_f64::<BigEndian>()?,
        Endian::Little => cursor.read_f64::<LittleEndian>()?,
    }; // 12

    let total_header_length = match endianness {
        Endian::Big => cursor.read_u32::<BigEndian>()?,
        Endian::Little => cursor.read_u32::<LittleEndian>()?,
    }; // 13

    let total_data_length = match endianness {
        Endian::Big => cursor.read_u32::<BigEndian>()?,
        Endian::Little => cursor.read_u32::<LittleEndian>()?,
    }; // 14

    // 解析 Quality Flag 1
    let qf1 = cursor.read_u8()?; // 15
    let quality_flags = QualityFlags {
        flag1_valid: (qf1 & 0b10000000) == 0,
        sun_data_degradation: (qf1 & 0b01000000) != 0,
        moon_data_degradation: (qf1 & 0b00100000) != 0,
        is_test_observation: (qf1 & 0b00010000) != 0,
        is_maneuvering: (qf1 & 0b00001000) != 0,
        is_unloading: (qf1 & 0b00000100) != 0,
        is_in_solar_calibration: (qf1 & 0b00000010) != 0,
        is_in_solar_eclipse: (qf1 & 0b00000001) != 0,
    };

    cursor.seek(SeekFrom::Current(3))?; // 16, 17, 18, 跳过其他质量标志

    let file_format_version = read_c_string(cursor, 32)?; // 19
    let file_name = read_c_string(cursor, 128)?; // 20

    cursor.seek(SeekFrom::Current(40))?; // 21, 跳过备用区

    Ok(BasicInfoBlock {
        byte_order,
        satellite_name,
        processing_center_name,
        observation_area,
        observation_timeline,
        observation_start_time_mjd,
        observation_end_time_mjd,
        file_creation_time_mjd,
        total_header_length,
        total_data_length,
        quality_flags,
        file_format_version,
        file_name,
    })
}

/// 解析块 #2: 数据信息块
fn parse_block_2(
    cursor: &mut Cursor<&[u8]>,
    endian: &Endian,
) -> Result<DataInfoBlock, Box<dyn Error>> {
    let _header_block_num = cursor.read_u8()?;
    let _block_length = cursor.read_u16::<LittleEndian>()?;

    let bits_per_pixel = match endian {
        Endian::Big => cursor.read_u16::<BigEndian>()?,
        Endian::Little => cursor.read_u16::<LittleEndian>()?,
    };
    let columns = match endian {
        Endian::Big => cursor.read_u16::<BigEndian>()?,
        Endian::Little => cursor.read_u16::<LittleEndian>()?,
    };
    let lines = match endian {
        Endian::Big => cursor.read_u16::<BigEndian>()?,
        Endian::Little => cursor.read_u16::<LittleEndian>()?,
    };
    let compression_flag = cursor.read_u8()?;
    let compression = match compression_flag {
        0 => "no compression".to_string(),
        1 => "gzip".to_string(),
        2 => "bzip2".to_string(),
        _ => "unknown".to_string(),
    };

    cursor.seek(SeekFrom::Current(40))?; // 跳过备用区

    Ok(DataInfoBlock {
        bits_per_pixel,
        columns,
        lines,
        compression,
    })
}

/// 解析块 #3: 投影信息块
fn parse_block_3(
    cursor: &mut Cursor<&[u8]>,
    endian: &Endian,
) -> Result<ProjectionInfoBlock, Box<dyn Error>> {
    let _header_block_num = cursor.read_u8()?;
    let _block_length = cursor.read_u16::<LittleEndian>()?;

    let sub_lon = match endian {
        Endian::Big => cursor.read_f64::<BigEndian>()?,
        Endian::Little => cursor.read_f64::<LittleEndian>()?,
    };
    let cfac = match endian {
        Endian::Big => cursor.read_u32::<BigEndian>()?,
        Endian::Little => cursor.read_u32::<LittleEndian>()?,
    };
    let lfac = match endian {
        Endian::Big => cursor.read_u32::<BigEndian>()?,
        Endian::Little => cursor.read_u32::<LittleEndian>()?,
    };
    let coff = match endian {
        Endian::Big => cursor.read_f32::<BigEndian>()?,
        Endian::Little => cursor.read_f32::<LittleEndian>()?,
    };
    let loff = match endian {
        Endian::Big => cursor.read_f32::<BigEndian>()?,
        Endian::Little => cursor.read_f32::<LittleEndian>()?,
    };
    let rs = match endian {
        Endian::Big => cursor.read_f64::<BigEndian>()?,
        Endian::Little => cursor.read_f64::<LittleEndian>()?,
    };
    let req = match endian {
        Endian::Big => cursor.read_f64::<BigEndian>()?,
        Endian::Little => cursor.read_f64::<LittleEndian>()?,
    };
    let rpol = match endian {
        Endian::Big => cursor.read_f64::<BigEndian>()?,
        Endian::Little => cursor.read_f64::<LittleEndian>()?,
    };

    // 跳过基于 WGS84 的固定值和处理中心使用的值
    cursor.seek(SeekFrom::Current(8 * 4 + 2 + 2))?;
    cursor.seek(SeekFrom::Current(40))?; // 跳过备用区

    Ok(ProjectionInfoBlock {
        sub_lon,
        cfac,
        lfac,
        coff,
        loff,
        rs,
        req,
        rpol,
    })
}

/// 解析块 #4: 导航信息块
fn parse_block_4(
    cursor: &mut Cursor<&[u8]>,
    endian: &Endian,
) -> Result<NavigationInfoBlock, Box<dyn Error>> {
    const UNDEFINED_F64: f64 = -1010.0;

    let read_f64 = |cursor: &mut Cursor<&[u8]>| -> Result<f64, Box<dyn Error>> {
        match endian {
            Endian::Big => Ok(cursor.read_f64::<BigEndian>()?),
            Endian::Little => Ok(cursor.read_f64::<LittleEndian>()?),
        }
    };

    let read_opt_f64 = |cursor: &mut Cursor<&[u8]>| -> Result<Option<f64>, Box<dyn Error>> {
        let val = read_f64(cursor)?;
        Ok(if val == UNDEFINED_F64 {
            None
        } else {
            Some(val)
        })
    };

    let _header_block_num = cursor.read_u8()?;
    let _block_length = cursor.read_u16::<LittleEndian>()?;

    let navigation_time_mjd = read_f64(cursor)?;
    let ssp_longitude = read_opt_f64(cursor)?;
    let ssp_latitude = read_opt_f64(cursor)?;
    let distance_from_center = read_opt_f64(cursor)?;
    let nadir_longitude = read_opt_f64(cursor)?;
    let nadir_latitude = read_opt_f64(cursor)?;

    let sun_position_j2000 = [read_f64(cursor)?, read_f64(cursor)?, read_f64(cursor)?];

    let moon_x = read_f64(cursor)?;
    let moon_position_j2000 = if moon_x == UNDEFINED_F64 {
        cursor.seek(SeekFrom::Current(16))?; // 跳过 y 和 z
        None
    } else {
        Some([moon_x, read_f64(cursor)?, read_f64(cursor)?])
    };

    cursor.seek(SeekFrom::Current(40))?; // 跳过备用区

    Ok(NavigationInfoBlock {
        navigation_time_mjd,
        ssp_longitude,
        ssp_latitude,
        distance_from_center,
        nadir_longitude,
        nadir_latitude,
        sun_position_j2000,
        moon_position_j2000,
    })
}

/// 解析块 #5: 定标信息块
fn parse_block_5(
    cursor: &mut Cursor<&[u8]>,
    endian: &Endian,
) -> Result<CalibrationInfoBlock, Box<dyn Error>> {
    let read_f64 = |cursor: &mut Cursor<&[u8]>| -> Result<f64, Box<dyn Error>> {
        match endian {
            Endian::Big => Ok(cursor.read_f64::<BigEndian>()?),
            Endian::Little => Ok(cursor.read_f64::<LittleEndian>()?),
        }
    };
    let read_u16 = |cursor: &mut Cursor<&[u8]>| -> Result<u16, Box<dyn Error>> {
        match endian {
            Endian::Big => Ok(cursor.read_u16::<BigEndian>()?),
            Endian::Little => Ok(cursor.read_u16::<LittleEndian>()?),
        }
    };

    let _header_block_num = cursor.read_u8()?;
    let _block_length = cursor.read_u16::<LittleEndian>()?;

    let band_number = read_u16(cursor)?;
    let central_wavelength = read_f64(cursor)?;
    let valid_bits_per_pixel = read_u16(cursor)?;
    let count_error_pixel = read_u16(cursor)?;
    let count_outside_scan_area = read_u16(cursor)?;
    let slope = read_f64(cursor)?;
    let intercept = read_f64(cursor)?;

    let band_specific_calibration = if band_number <= 6 {
        // 可见光/近红外波段
        let albedo_conversion_coeff = read_f64(cursor)?;
        let update_time_mjd = read_f64(cursor)?;
        let calibrated_slope = read_f64(cursor)?;
        let calibrated_intercept = read_f64(cursor)?;
        cursor.seek(SeekFrom::Current(80))?; // 跳过备用区
        BandSpecificCalibration::VisibleNIR {
            albedo_conversion_coeff,
            update_time_mjd,
            calibrated_slope,
            calibrated_intercept,
        }
    } else {
        // 红外波段
        let c0 = read_f64(cursor)?;
        let c1 = read_f64(cursor)?;
        let c2 = read_f64(cursor)?;
        let c_big_0 = read_f64(cursor)?;
        let c_big_1 = read_f64(cursor)?;
        let c_big_2 = read_f64(cursor)?;
        let speed_of_light = read_f64(cursor)?;
        let planck_constant = read_f64(cursor)?;
        let boltzmann_constant = read_f64(cursor)?;
        cursor.seek(SeekFrom::Current(40))?; // 跳过备用区
        BandSpecificCalibration::Infrared {
            tb_conversion_coeffs: (c0, c1, c2),
            radiance_conversion_coeffs: (c_big_0, c_big_1, c_big_2),
            speed_of_light,
            planck_constant,
            boltzmann_constant,
        }
    };

    Ok(CalibrationInfoBlock {
        band_number,
        central_wavelength,
        valid_bits_per_pixel,
        count_error_pixel,
        count_outside_scan_area,
        slope,
        intercept,
        band_specific_calibration,
    })
}

/// 解析块 #6: 交叉定标信息块
fn parse_block_6(
    cursor: &mut Cursor<&[u8]>,
    endian: &Endian,
) -> Result<InterCalibrationInfoBlock, Box<dyn Error>> {
    const UNDEFINED_F64: f64 = -1010.0;

    let read_opt_f64 = |cursor: &mut Cursor<&[u8]>| -> Result<Option<f64>, Box<dyn Error>> {
        let val = match endian {
            Endian::Big => cursor.read_f64::<BigEndian>()?,
            Endian::Little => cursor.read_f64::<LittleEndian>()?,
        };
        Ok(if val == UNDEFINED_F64 {
            None
        } else {
            Some(val)
        })
    };

    let read_opt_f32 = |cursor: &mut Cursor<&[u8]>| -> Result<Option<f32>, Box<dyn Error>> {
        let val = match endian {
            Endian::Big => cursor.read_f32::<BigEndian>()?,
            Endian::Little => cursor.read_f32::<LittleEndian>()?,
        };
        // 用 f32 的 epsilon 比较浮点数
        Ok(if (val - UNDEFINED_F64 as f32).abs() < f32::EPSILON {
            None
        } else {
            Some(val)
        })
    };

    let _header_block_num = cursor.read_u8()?;
    let _block_length = cursor.read_u16::<LittleEndian>()?;

    let intercept = read_opt_f64(cursor)?;
    let slope = read_opt_f64(cursor)?;
    let quadratic_term = read_opt_f64(cursor)?;
    let radiance_bias_std_scene = read_opt_f64(cursor)?;
    let uncertainty_radiance_bias = read_opt_f64(cursor)?;
    let radiance_std_scene = read_opt_f64(cursor)?;
    let gsics_correction_start_mjd = read_opt_f64(cursor)?;
    let gsics_correction_end_mjd = read_opt_f64(cursor)?;
    let validity_range_upper = read_opt_f32(cursor)?;
    let validity_range_lower = read_opt_f32(cursor)?;
    let filename = read_c_string(cursor, 128)?;
    let gsics_correction_filename = if filename.is_empty() {
        None
    } else {
        Some(filename)
    };

    cursor.seek(SeekFrom::Current(56))?;

    Ok(InterCalibrationInfoBlock {
        intercept,
        slope,
        quadratic_term,
        radiance_bias_std_scene,
        uncertainty_radiance_bias,
        radiance_std_scene,
        gsics_correction_start_mjd,
        gsics_correction_end_mjd,
        validity_range_upper,
        validity_range_lower,
        gsics_correction_filename,
    })
}

/// 解析块 #7: 分段信息块
fn parse_block_7(
    cursor: &mut Cursor<&[u8]>,
    endian: &Endian,
) -> Result<SegmentInfoBlock, Box<dyn Error>> {
    let _header_block_num = cursor.read_u8()?;
    let _block_length = cursor.read_u16::<LittleEndian>()?;

    let total_segments = cursor.read_u8()?;
    let segment_sequence_number = cursor.read_u8()?;
    let first_line_number = match endian {
        Endian::Big => cursor.read_u16::<BigEndian>()?,
        Endian::Little => cursor.read_u16::<LittleEndian>()?,
    };

    cursor.seek(SeekFrom::Current(40))?;

    Ok(SegmentInfoBlock {
        total_segments,
        segment_sequence_number,
        first_line_number,
    })
}

/// 解析块 #8: 导航修正信息块
fn parse_block_8(
    cursor: &mut Cursor<&[u8]>,
    endian: &Endian,
) -> Result<NavCorrectionInfoBlock, Box<dyn Error>> {
    let _header_block_num = cursor.read_u8()?;
    let _block_length = cursor.read_u16::<LittleEndian>()?;

    // 为闭包添加显式的返回类型注解以解决之前的编译错误
    let read_f32 = |c: &mut Cursor<&[u8]>| -> Result<f32, Box<dyn Error>> {
        Ok(match endian {
            Endian::Big => c.read_f32::<BigEndian>()?,
            Endian::Little => c.read_f32::<LittleEndian>()?,
        })
    };
    let read_f64 = |c: &mut Cursor<&[u8]>| -> Result<f64, Box<dyn Error>> {
        Ok(match endian {
            Endian::Big => c.read_f64::<BigEndian>()?,
            Endian::Little => c.read_f64::<LittleEndian>()?,
        })
    };
    let read_u16 = |c: &mut Cursor<&[u8]>| -> Result<u16, Box<dyn Error>> {
        Ok(match endian {
            Endian::Big => c.read_u16::<BigEndian>()?,
            Endian::Little => c.read_u16::<LittleEndian>()?,
        })
    };

    let center_column_rotation = read_f32(cursor)?;
    let center_line_rotation = read_f32(cursor)?;
    let rotation_correction_amount = read_f64(cursor)?;
    let num_corrections = read_u16(cursor)? as usize;

    let mut corrections = Vec::with_capacity(num_corrections);
    for _ in 0..num_corrections {
        corrections.push(TranslationCorrection {
            line_number_after_rotation: read_u16(cursor)?,
            shift_amount_column: read_f32(cursor)?,
            shift_amount_line: read_f32(cursor)?,
        });
    }

    cursor.seek(SeekFrom::Current(40))?;

    Ok(NavCorrectionInfoBlock {
        center_column_rotation,
        center_line_rotation,
        rotation_correction_amount,
        corrections,
    })
}

/// 解析块 #9: 观测时间信息块
fn parse_block_9(
    cursor: &mut Cursor<&[u8]>,
    endian: &Endian,
) -> Result<ObservationTimeInfoBlock, Box<dyn Error>> {
    let _header_block_num = cursor.read_u8()?;
    let _block_length = cursor.read_u16::<LittleEndian>()?;

    let num_times = match endian {
        Endian::Big => cursor.read_u16::<BigEndian>()?,
        Endian::Little => cursor.read_u16::<LittleEndian>()?,
    } as usize;

    let mut times = Vec::with_capacity(num_times);
    for _ in 0..num_times {
        times.push(LineObservationTime {
            line_number: match endian {
                Endian::Big => cursor.read_u16::<BigEndian>()?,
                Endian::Little => cursor.read_u16::<LittleEndian>()?,
            },
            observation_time_mjd: match endian {
                Endian::Big => cursor.read_f64::<BigEndian>()?,
                Endian::Little => cursor.read_f64::<LittleEndian>()?,
            },
        });
    }

    cursor.seek(SeekFrom::Current(40))?;

    Ok(ObservationTimeInfoBlock { times })
}

/// 解析块 #10: 错误信息块
fn parse_block_10(
    cursor: &mut Cursor<&[u8]>,
    endian: &Endian,
) -> Result<ErrorInfoBlock, Box<dyn Error>> {
    let _header_block_num = cursor.read_u8()?;
    let _block_length = cursor.read_u32::<LittleEndian>()?; // 注意：此块长度是 I4

    let num_errors = match endian {
        Endian::Big => cursor.read_u16::<BigEndian>()?,
        Endian::Little => cursor.read_u16::<LittleEndian>()?,
    } as usize;

    let mut errors = Vec::with_capacity(num_errors);
    for _ in 0..num_errors {
        errors.push(LineErrorInfo {
            line_number: match endian {
                Endian::Big => cursor.read_u16::<BigEndian>()?,
                Endian::Little => cursor.read_u16::<LittleEndian>()?,
            },
            error_pixels_per_line: match endian {
                Endian::Big => cursor.read_u16::<BigEndian>()?,
                Endian::Little => cursor.read_u16::<LittleEndian>()?,
            },
        });
    }

    cursor.seek(SeekFrom::Current(40))?;

    Ok(ErrorInfoBlock { errors })
}

/// 跳过块 #11: 备用块
fn skip_block_11(cursor: &mut Cursor<&[u8]>) -> Result<(), Box<dyn Error>> {
    let _header_block_num = cursor.read_u8()?;
    let _block_length = cursor.read_u16::<LittleEndian>()?;
    cursor.seek(SeekFrom::Current(256))?;
    Ok(())
}

/// 解析块 #12: 数据块
fn parse_block_12(
    cursor: &mut Cursor<&[u8]>,
    endian: &Endian,
    data_info: &DataInfoBlock,
) -> Result<Vec<u16>, Box<dyn Error>> {
    let num_pixels = data_info.columns as usize * data_info.lines as usize;
    let mut pixel_data = Vec::with_capacity(num_pixels);

    match endian {
        Endian::Big => {
            for _ in 0..num_pixels {
                pixel_data.push(cursor.read_u16::<BigEndian>()?);
            }
        }
        Endian::Little => {
            for _ in 0..num_pixels {
                pixel_data.push(cursor.read_u16::<LittleEndian>()?);
            }
        }
    }

    Ok(pixel_data)
}
