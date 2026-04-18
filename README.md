# HSD Data Process
这是一个开源项目，用于处理Himawari卫星的HSD格式数据。该项目使用Rust语言实现高效的数据处理管道，包括数据校正、瑞利大气校正、Lanczos缩放，并输出为TIFF图像格式。

## 项目结构
```
hsd_data_process/
├── Cargo.toml
├── README.md
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── processer/
│   │   ├── mod.rs
│   │   ├── processer.rs
│   │   └── rayleigh_correction.rs
│   ├── reader/
│   │   ├── mod.rs
│   │   ├── hsd_organizer.rs
│   │   └── raw_hsd_reader.rs
│   └── writer/
│       ├── mod.rs
│       ├── writer.rs
│       └── writer_testing.rs
├── proj_precompute/
│   ├── main.py
│   ├── pyproject.toml
│   └── README.md
├── lut_binary/
├── 02/
└── target/
```

## 功能特性
- 高效数据处理：使用Rust实现高性能并行处理
- 多波段支持：支持Himawari卫星的多个可见光和红外波段
- 大气校正：内置瑞利大气散射校正算法
- 几何校正：支持卫星几何参数计算和校正
- 图像输出：生成标准TIFF格式的真彩色图像
- 预计算优化：Python脚本预计算几何数据以加速处理

## 依赖要求
### Rust依赖
- Rust 1.70+ (支持2024 edition)
- 系统依赖：需要支持bzip2静态链接

### Python依赖 (仅预计算)
- Python 3.12+ (推荐使用 [uv](https://github.com/astral-sh/uv) 包管理器)
- numpy
- pyproj
- tqdm

## 安装
1. 克隆项目：
   ```bash
   git clone 
   cd hsd_data_process
   ```

2. 安装Rust依赖：
   ```bash
   cargo build --release
   ```

3. 安装Python依赖（可选，用于预计算）：
   ```bash
   cd proj_precompute
   uv sync
   ```

## 使用方法
1. 预计算几何数据
   首先运行Python脚本生成几何数据文件：
   ```bash
   uv run main.py
   ```
   这将生成Himawari-9卫星的纬度、经度、太阳天顶角等几何数据文件。

2. 准备数据
   将HSD数据文件放置在`02`目录下，确保文件结构符合预期格式。
   (即同一时间块的所有160个FLDK文件包含B01-B16)

3. 运行数据处理
   ```bash
   cd ..
   cargo run --release
   ```
   程序将自动处理所有时间序列的数据，生成对应的TIFF图像文件。

## 数据处理流程
1. 数据读取：解析HSD文件格式，提取原始观测数据
2. 校正处理：
   - 数据定标校正
   - 瑞利大气校正
   - 几何参数计算
3. 图像合成：
   - 多波段数据融合
   - 颜色空间转换（线性sRGB）
   - Lanczos插值缩放
4. 输出：生成全盘真彩色TIFF图像

## 主要模块说明
### processer模块
- `processer.rs`：核心数据处理逻辑，包括定标校正和颜色转换
- `rayleigh_correction.rs`：瑞利大气校正实现

### reader模块
- `hsd_organizer.rs`：组织和管理HSD文件
- `raw_hsd_reader.rs`：低级HSD文件解析

### writer模块
- `writer.rs`：TIFF文件写入和编码
- `writer_testing.rs`：写入模块的测试代码

## 配置参数
主要参数在`main.rs`中定义：
- `width/height`：图像分辨率 (22000x22000 for 0.5km)
- 几何数据路径：指向预计算的.dat文件
- LUT路径：瑞利查找表目录

## 输出格式
- 生成的TIFF文件包含RGB真彩色图像
- 分辨率：22000x22000像素 (0.5km)
- 压缩：无压缩 (Uncompressed)
- 颜色深度：16位

## 贡献
欢迎贡献！请遵循以下步骤：
1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/AmazingFeature`)
3. 提交更改 (`git commit -m 'Add some AmazingFeature'`)
4. 推送到分支 (`git push origin feature/AmazingFeature`)
5. 创建 Pull Request
