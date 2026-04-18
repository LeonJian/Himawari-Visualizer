[簡體中文](https://github.com/LeonJian/Himawari-Visualizer/blob/main/README_CN.md)
[繁體中文](https://github.com/LeonJian/Himawari-Visualizer/blob/main/README_TW.md)
# HSD Data Process

這是一個開源專案，用於處理 Himawari 衛星的 HSD 格式資料。該專案使用 Rust 語言實現高效的資料處理管線，包括資料校正、瑞利大氣校正、Lanczos 縮放，並輸出為 TIFF 圖像格式。目前此專案非常不穩定，請謹慎使用。

## 專案結構
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
- 高效資料處理：使用 Rust 實現高性能並行處理
- 多波段支援：支援 Himawari 衛星的多個可見光和紅外波段
- 大氣校正：內建瑞利大氣散射校正演算法
- 幾何校正：支援衛星幾何參數計算與校正
- 圖像輸出：生成標準 TIFF 格式真彩色影像
- 預計算優化：Python 腳本預先計算幾何資料以加速處理

## 依賴需求
### Rust 依賴
- Rust 1.70+（支援 2024 edition）
- 系統依賴：需要支援 bzip2 靜態連結

### Python 依賴（僅預計算）
- Python 3.12+（使用 [uv](https://github.com/astral-sh/uv) 套件管理器）
- numpy
- pyproj
- tqdm

## 安裝
1. 複製專案：
   ```bash
   git clone https://github.com/LeonJian/Himawari-Visualizer.git
   cd hsd_data_process
   ```

2. 安裝 Rust 依賴：
   ```bash
   cargo build --release
   ```

3. 安裝 Python 依賴：
   ```bash
   cd proj_precompute
   uv sync
   ```

## 使用方法
1. 預計算幾何資料
   先執行 Python 腳本生成幾何資料檔：
   ```bash
   uv run main.py
   ```
   這會生成 Himawari-9 衛星的緯度、經度、太陽天頂角等幾何資料檔案。

2. 準備資料
   將 HSD 資料檔放置在 `02` 目錄中，確保檔案結構符合預期格式。
   （即同一時間區間的所有 160 個 FLDK 檔案包含 B01-B16）

3. 執行資料處理
   ```bash
   cd ..
   cargo run --release
   ```
   程式會自動處理所有時間序列資料，並生成對應的 TIFF 影像檔案。

## 資料處理流程
1. 資料讀取：解析 HSD 檔案格式，擷取原始觀測資料
2. 校正處理：
   - 資料定標校正
   - 瑞利大氣校正
   - 幾何參數計算
3. 影像合成：
   - 多波段資料融合
   - 顏色空間轉換（線性 sRGB）
   - Lanczos 插值縮放
4. 輸出：生成全盤真彩色 TIFF 影像
