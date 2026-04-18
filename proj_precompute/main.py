"""
HSD Data Process - 几何预计算脚本

此脚本用于预计算Himawari-9卫星的几何数据，包括：
- 纬度/经度网格
- 太阳天顶角和方位角
- 卫星观测几何参数（VZA, VAA等）
- 大气校正所需的查找表

这些预计算数据用于加速主Rust程序中的几何校正和大气校正处理。

使用方法：
    python main.py

输出文件将保存在 h09_geometry_data_v2/ 目录中。
"""

import os
import numpy as np
import pyproj
from tqdm import tqdm
import time

# ================= 官方文档常量矫正 (Page 15, 16) =================

SAT_LON = 140.7
R_EQ = 6378137.0         # WGS84 赤道半径
R_POL = 6356752.314245   # WGS84 极半径
SAT_DIST = 42164000.0    # 卫星地心距离 Rs
SAT_H = SAT_DIST - R_EQ  # 卫星高度

# 基于 Page 15, Block #3 的精确参数
RES_1KM = {
    'name': '1km',
    'cols': 11000, 'rows': 11000,
    'cfac': 40932513, 'lfac': 40932513,
    'coff': 5500.5,   'loff': 5500.5
}

RES_05KM = {
    'name': '05km',
    'cols': 22000, 'rows': 22000,
    'cfac': 81865026, 'lfac': 81865026,
    'coff': 11000.5,  'loff': 11000.5
}

OUTPUT_DIR = 'h09_geometry_data_v2'
if not os.path.exists(OUTPUT_DIR):
    os.makedirs(OUTPUT_DIR)

# ================= 科学严谨的几何引擎 =================

def calculate_geometry_ecef(lat_deg, lon_deg):
    """
    使用 WGS84 向量法计算 VZA 和 VAA
    """
    lat = np.radians(lat_deg)
    lon = np.radians(lon_deg)
    sat_lon = np.radians(SAT_LON)

    e2 = 1 - (R_POL**2 / R_EQ**2)
    N_phi = R_EQ / np.sqrt(1 - e2 * np.sin(lat)**2)

    px = N_phi * np.cos(lat) * np.cos(lon)
    py = N_phi * np.cos(lat) * np.sin(lon)
    pz = N_phi * (1 - e2) * np.sin(lat)

    sx = SAT_DIST * np.cos(sat_lon)
    sy = SAT_DIST * np.sin(sat_lon)
    sz = 0.0

    P = np.stack([px, py, pz], axis=-1)
    S = np.array([sx, sy, sz], dtype=np.float64)

    G = S - P
    g_norm = np.linalg.norm(G, axis=-1, keepdims=True)
    G_unit = G / g_norm

    nx = np.cos(lat) * np.cos(lon)
    ny = np.cos(lat) * np.sin(lon)
    nz = np.sin(lat)
    N_unit = np.stack([nx, ny, nz], axis=-1)

    dot_gn = np.sum(G_unit * N_unit, axis=-1)
    dot_gn = np.clip(dot_gn, -1.0, 1.0)
    vza = np.degrees(np.arccos(dot_gn))

    up = N_unit
    east = np.stack([-np.sin(lon), np.cos(lon), np.zeros_like(lon)], axis=-1)
    north = np.cross(east, up)

    g_east = np.sum(G_unit * east, axis=-1)
    g_north = np.sum(G_unit * north, axis=-1)

    vaa = np.degrees(np.arctan2(g_east, g_north))
    vaa = (vaa + 360.0) % 360.0

    return vza, vaa

class H09GeneratorV2:
    def __init__(self, cfg):
        self.cfg = cfg
        self.transformer = pyproj.Transformer.from_crs(
            f"+proj=geos +h={SAT_H} +lon_0={SAT_LON} +a={R_EQ} +b={R_POL} +sweep=x +units=m +no_defs",
            "EPSG:4326",
            always_xy=True
        )

    def process(self):
        rows, cols = self.cfg['rows'], self.cfg['cols']
        print(f"\n任务: {self.cfg['name']} ({rows}x{cols})")

        paths = {k: os.path.join(OUTPUT_DIR, f"H09_{self.cfg['name']}_{k}.dat")
                 for k in ['Lat', 'Lon', 'VZA', 'VAA']}
        mm = {k: np.memmap(v, dtype='float32', mode='w+', shape=(rows, cols))
              for k, v in paths.items()}

        chunk_size = 500 if rows > 15000 else 1000
        col_idx = np.arange(cols)

        for r0 in tqdm(range(0, rows, chunk_size)):
            r1 = min(r0 + chunk_size, rows)
            C, L = np.meshgrid(col_idx, np.arange(r0, r1))

            scale_x = (2**16) / self.cfg['cfac']
            scale_y = (2**16) / self.cfg['lfac']

            x = (C + 1 - self.cfg['coff']) * scale_x
            y = (self.cfg['loff'] - (L + 1)) * scale_y

            lon, lat = self.transformer.transform(x, y)

            mask = np.isfinite(lon) & np.isfinite(lat) & (lat > -89) & (lat < 89)

            vza_chunk = np.full(lon.shape, np.nan, dtype='float32')
            vaa_chunk = np.full(lon.shape, np.nan, dtype='float32')

            if np.any(mask):
                vza_val, vaa_val = calculate_geometry_ecef(lat[mask], lon[mask])
                vza_chunk[mask] = vza_val
                vaa_chunk[mask] = vaa_val

            mm['Lat'][r0:r1, :] = lat.astype('float32')
            mm['Lon'][r0:r1, :] = lon.astype('float32')
            mm['VZA'][r0:r1, :] = vza_chunk
            mm['VAA'][r0:r1, :] = vaa_chunk

        for m in mm.values():
            m.flush()
        print(f"完成 {self.cfg['name']}")

if __name__ == "__main__":
    # 建议先跑 1km 验证速度
    H09GeneratorV2(RES_1KM).process()
    H09GeneratorV2(RES_05KM).process()