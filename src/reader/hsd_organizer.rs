use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::fs::DirEntry;
use std::path::{Path, PathBuf};
use std::process::exit;

// --- 数据结构 ---
#[derive(Debug, Clone)]
pub struct SegmentBlock {
    pub segment_index: usize,
    pub files: Vec<Option<PathBuf>>, // 索引与 band_names 对应
}

#[derive(Debug)]
pub struct FullDiskRawDataStructure {
    pub band_names: Vec<String>,     // 例如 ["B01", "B02", ...]
    pub segments: Vec<SegmentBlock>, // 1-10 段
}

#[derive(Debug, Clone)]
struct HsdFileInfo {
    path: PathBuf,
    band: String,      // B01-B16
    segment: usize,    // 1-10
    timestamp: String, // YYYYMMDD_HHMM
}

/// 主入口：获取目录下所有合规的全盘 HSD 数据
pub fn get_full_process_file(dir_path: &Path) -> BTreeMap<String, FullDiskRawDataStructure> {
    // 1. 基础检查
    if !dir_path.exists() {
        eprintln!("错误: 路径 '{}' 不存在。", dir_path.display());
        exit(1);
    }
    if !dir_path.is_dir() {
        eprintln!("错误: '{}' 不是一个目录。", dir_path.display());
        exit(1);
    }

    // 2. 扫描目录
    let entries_res = fs::read_dir(dir_path);
    let entries: Vec<DirEntry> = match entries_res {
        Ok(iter) => iter
            .filter_map(|res| match res {
                Ok(entry) => Some(entry),
                Err(e) => {
                    eprintln!("警告: 忽略目录中的无效项: {}", e);
                    None
                }
            })
            .collect(),
        Err(e) => {
            eprintln!("无法打开目录 '{}': {}", dir_path.display(), e);
            exit(1);
        }
    };

    if entries.is_empty() {
        println!("提示: 目录 '{}' 为空。", dir_path.display());
        return BTreeMap::new();
    }

    // 3. 过滤并组织数据
    let time_series_map = match organize_hsd_files_by_time(entries) {
        Ok(map) => {
            println!("扫描完成。共发现 {} 个有效时间点。数据详情：", map.len());
            for (ts, data) in &map {
                let file_count: usize = data
                    .segments
                    .iter()
                    .map(|s| s.files.iter().flatten().count())
                    .sum();
                println!(
                    " - [{}]: 波段数={}, 已就绪分段文件={}/{}",
                    ts,
                    data.band_names.len(),
                    file_count,
                    data.band_names.len() * 10
                );
            }
            map
        }
        Err(e) => {
            eprintln!("整理失败: {}", e);
            exit(1);
        }
    };

    time_series_map
}

impl HsdFileInfo {
    /// 核心解析函数：严格匹配 HSD 全盘 压缩/非压缩 文件名格式
    /// 示例: HS_H08_20231001_0000_B01_FLDK_R10_S0110.DAT.bz2
    fn try_from_path(path: PathBuf) -> Option<Self> {
        // 1. 获取文件名并转大写，处理各种后缀情况
        let file_name = path.file_name()?.to_string_lossy().to_uppercase();

        // 2. 严格前置过滤
        // - 必须以 HS_ 开头
        // - 必须包含 _FLDK_ (全盘)，从而直接过滤掉 _JP_ 或 _Rxx_ (区域文件)
        // - 排除 .PNG, .JPG, .XML 等非数据文件
        if !file_name.starts_with("HS_") || !file_name.contains("_FLDK_") {
            return None;
        }

        // 检查扩展名是否包含 .DAT (支持 .DAT, .DAT.BZ2, .DAT.GZ 等)
        if !file_name.contains(".DAT") {
            return None;
        }

        // 3. 基于 HSD 标准协议的下划线切分
        // 0:HS, 1:卫星, 2:日期, 3:时间, 4:波段, 5:FLDK, 6:分辨率, 7:分段信息
        let parts: Vec<&str> = file_name.split('_').collect();
        if parts.len() < 8 {
            return None;
        }

        // 4. 提取时间戳 (索引 2 和 3) -> YYYYMMDD_HHMM
        let date_part = parts[2];
        let time_part = parts[3];
        if date_part.len() != 8 || time_part.len() != 4 {
            return None;
        }
        let timestamp = format!("{}_{}", date_part, time_part);

        // 5. 提取波段 (索引 4) -> B01-B16
        let band_raw = parts[4]; // 例如 "B01"
        if !band_raw.starts_with('B') || band_raw.len() != 3 {
            return None;
        }
        let band = band_raw.to_string();

        // 6. 提取分段号 (索引 7) -> S0110...
        // 格式通常为 S0110，前两位是当前段，后两位是总段数
        let seg_raw = parts[7];
        if !seg_raw.starts_with('S') || seg_raw.len() < 3 {
            return None;
        }
        let segment_num = seg_raw[1..3].parse::<usize>().ok()?;
        if !(1..=10).contains(&segment_num) {
            return None;
        }

        Some(HsdFileInfo {
            path,
            band,
            segment: segment_num,
            timestamp,
        })
    }
}

impl FullDiskRawDataStructure {
    /// 将零散的文件信息合并为结构化数据
    fn build_from_files(files: Vec<HsdFileInfo>) -> Self {
        // 提取并排序所有出现的波段
        let mut bands: Vec<String> = files
            .iter()
            .map(|f| f.band.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        bands.sort();

        // 映射波段名称到索引，方便快速填充
        let band_map: HashMap<&str, usize> = bands
            .iter()
            .enumerate()
            .map(|(i, b)| (b.as_str(), i))
            .collect();
        let num_bands = bands.len();

        // 初始化 10 个 SegmentBlock (HSD 全盘标准为 10 段)
        let mut segments: Vec<SegmentBlock> = (1..=10)
            .map(|i| SegmentBlock {
                segment_index: i,
                files: vec![None; num_bands],
            })
            .collect();

        // 填充路径
        for file in files {
            if let Some(&col_idx) = band_map.get(file.band.as_str()) {
                let row_idx = file.segment - 1;
                if let Some(block) = segments.get_mut(row_idx) {
                    block.files[col_idx] = Some(file.path);
                }
            }
        }

        FullDiskRawDataStructure {
            band_names: bands,
            segments,
        }
    }
}

/// 核心逻辑：遍历目录项并按时间戳分组
fn organize_hsd_files_by_time(
    entries: Vec<DirEntry>,
) -> Result<BTreeMap<String, FullDiskRawDataStructure>, String> {
    let valid_files: Vec<HsdFileInfo> = entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            // 只处理文件，跳过子目录
            if !path.is_file() {
                return None;
            }
            HsdFileInfo::try_from_path(path)
        })
        .collect();

    if valid_files.is_empty() {
        return Err(
            "目录下未发现任何符合 HSD 全盘格式 (.DAT / .BZ2 / .GZ) 的数据文件。".to_string(),
        );
    }

    // 按时间戳 (YYYYMMDD_HHMM) 分组
    let mut time_groups: HashMap<String, Vec<HsdFileInfo>> = HashMap::new();
    for info in valid_files {
        time_groups
            .entry(info.timestamp.clone())
            .or_default()
            .push(info);
    }

    // 转换为 BTreeMap (自动按时间顺序排序)
    let mut result_map = BTreeMap::new();
    for (timestamp, files) in time_groups {
        result_map.insert(timestamp, FullDiskRawDataStructure::build_from_files(files));
    }

    Ok(result_map)
}
