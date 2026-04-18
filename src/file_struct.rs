pub mod file_struct {
    #[derive(Debug)]
    pub struct HsdFile {
        pub metadata: HsdMetadata,
        pub data: HsdData,
    }

    #[derive(Debug)]
    pub struct HsdData {
        pub size: (u16, u16),
        pub data: Vec<u16>,
    }

    #[derive(Debug)]
    /// 包含从 11 个报头块中解析出的所有元数据。
    pub struct HsdMetadata {
        pub basic_info: BasicInfoBlock,
        pub data_info: DataInfoBlock,
        pub projection_info: ProjectionInfoBlock,
        pub navigation_info: NavigationInfoBlock,
        pub calibration_info: CalibrationInfoBlock,
        pub inter_calibration_info: InterCalibrationInfoBlock,
        pub segment_info: SegmentInfoBlock,
        pub nav_correction_info: NavCorrectionInfoBlock,
        pub observation_time_info: ObservationTimeInfoBlock,
        pub error_info: ErrorInfoBlock,
        // pub spare_block: SpareBlock,
    }

    /// 块 #1: 基本信息块
    #[derive(Debug)]
    pub struct BasicInfoBlock {
        pub byte_order: String,
        pub satellite_name: String,
        pub processing_center_name: String,
        pub observation_area: String,
        pub observation_timeline: u16,
        pub observation_start_time_mjd: f64,
        pub observation_end_time_mjd: f64,
        pub file_creation_time_mjd: f64,
        pub total_header_length: u32,
        pub total_data_length: u32,
        pub quality_flags: QualityFlags,
        pub file_format_version: String,
        pub file_name: String,
    }

    /// 块 #1 中的质量标志位 [cite: 74]
    #[derive(Debug)]
    pub struct QualityFlags {
        pub flag1_valid: bool,
        pub sun_data_degradation: bool,
        pub moon_data_degradation: bool,
        pub is_test_observation: bool,
        pub is_maneuvering: bool,
        pub is_unloading: bool,
        pub is_in_solar_calibration: bool,
        pub is_in_solar_eclipse: bool,
    }

    /// 块 #2: 数据信息块 [cite: 101]
    #[derive(Debug)]
    pub struct DataInfoBlock {
        pub bits_per_pixel: u16,
        pub columns: u16, // 参考表 3 [cite: 43]
        pub lines: u16,   // 参考表 3 [cite: 43]
        pub compression: String,
    }

    /// 块 #3: 投影信息块 [cite: 78]
    #[derive(Debug)]
    pub struct ProjectionInfoBlock {
        pub sub_lon: f64,
        pub cfac: u32,
        pub lfac: u32,
        pub coff: f32,
        pub loff: f32,
        pub rs: f64,
        pub req: f64,
        pub rpol: f64,
    }

    /// 块 #4: 导航信息块 [cite: 81]
    #[derive(Debug)]
    pub struct NavigationInfoBlock {
        pub navigation_time_mjd: f64,
        pub ssp_longitude: Option<f64>,
        pub ssp_latitude: Option<f64>,
        pub distance_from_center: Option<f64>,
        pub nadir_longitude: Option<f64>,
        pub nadir_latitude: Option<f64>,
        pub sun_position_j2000: [f64; 3],
        pub moon_position_j2000: Option<[f64; 3]>,
    }

    /// 块 #5: 定标信息块 [cite: 83]
    #[derive(Debug)]
    pub struct CalibrationInfoBlock {
        pub band_number: u16,
        pub central_wavelength: f64,
        pub valid_bits_per_pixel: u16,
        pub count_error_pixel: u16,
        pub count_outside_scan_area: u16,
        pub slope: f64,
        pub intercept: f64,
        pub band_specific_calibration: BandSpecificCalibration,
    }

    /// 包含针对红外或可见光/近红外波段的特定定标数据 [cite: 85, 87]
    #[derive(Debug)]
    pub enum BandSpecificCalibration {
        Infrared {
            // 波段 7-16
            // 辐射率到亮度温度的转换系数
            tb_conversion_coeffs: (f64, f64, f64),
            // 亮度温度到辐射率的转换系数
            radiance_conversion_coeffs: (f64, f64, f64),
            speed_of_light: f64,
            planck_constant: f64,
            boltzmann_constant: f64,
        },
        VisibleNIR {
            // 波段 1-6
            albedo_conversion_coeff: f64,
            calibrated_slope: f64,
            calibrated_intercept: f64,
            update_time_mjd: f64,
        },
    }

    /// 块 #6: GSICS 交叉定标信息块 [cite: 89]
    #[derive(Debug)]
    pub struct InterCalibrationInfoBlock {
        pub intercept: Option<f64>,
        pub slope: Option<f64>,
        pub quadratic_term: Option<f64>,
        pub radiance_bias_std_scene: Option<f64>,
        pub uncertainty_radiance_bias: Option<f64>,
        pub radiance_std_scene: Option<f64>,
        pub gsics_correction_start_mjd: Option<f64>,
        pub gsics_correction_end_mjd: Option<f64>,
        pub validity_range_upper: Option<f32>,
        pub validity_range_lower: Option<f32>,
        pub gsics_correction_filename: Option<String>,
    }

    /// 块 #7: 分段信息块
    #[derive(Debug)]
    pub struct SegmentInfoBlock {
        pub total_segments: u8,
        pub segment_sequence_number: u8,
        pub first_line_number: u16,
    }

    /// 块 #8: 导航修正信息块
    #[derive(Debug)]
    pub struct NavCorrectionInfoBlock {
        pub center_column_rotation: f32,
        pub center_line_rotation: f32,
        pub rotation_correction_amount: f64,
        pub corrections: Vec<TranslationCorrection>,
    }

    #[derive(Debug)]
    pub struct TranslationCorrection {
        pub line_number_after_rotation: u16,
        pub shift_amount_column: f32,
        pub shift_amount_line: f32,
    }

    /// 块 #9: 观测时间信息块
    #[derive(Debug)]
    pub struct ObservationTimeInfoBlock {
        pub times: Vec<LineObservationTime>,
    }

    #[derive(Debug)]
    pub struct LineObservationTime {
        pub line_number: u16,
        pub observation_time_mjd: f64,
    }

    /// 块 #10: 错误信息块
    #[derive(Debug)]
    pub struct ErrorInfoBlock {
        pub errors: Vec<LineErrorInfo>,
    }

    #[derive(Debug)]
    pub struct LineErrorInfo {
        pub line_number: u16,
        pub error_pixels_per_line: u16,
    }

    /// 块 #11: 备用块
    // #[derive(Debug)]
    // pub struct SpareBlock {
    //     pub content: Vec<u8>,
    // }

    // #################################################################################
    // # 解析逻辑
    // #################################################################################

    // 用于处理字节序的枚举
    pub enum Endian {
        Little,
        Big,
    }
}
