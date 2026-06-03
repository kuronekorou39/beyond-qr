// beyond-qr 復号アルゴリズムの JavaScript 実装。
//
// Python (sender/beyond_qr_sender/geometry.py, color.py, render.py) からの移植。
// 入力: RGB ピクセル (Uint8ClampedArray of [r,g,b,a,...] や [r,g,b,...] 等)
// 出力: セル列 (Uint8Array, 各セルが palette index 0..=7)
// 最終的な RS 復号は core-wasm に渡す。

// ---------- パレット (sRGB キューブ 8 頂点) ----------

export const PALETTE_RGB = [
  [0, 0, 0],       // 0 Black
  [0, 0, 255],     // 1 Blue
  [0, 255, 0],     // 2 Green
  [0, 255, 255],   // 3 Cyan
  [255, 0, 0],     // 4 Red
  [255, 0, 255],   // 5 Magenta
  [255, 255, 0],   // 6 Yellow
  [255, 255, 255], // 7 White
];

// ---------- sRGB → OKLab (Björn Ottosson 2020) ----------

const M1 = [
  [0.4122214708, 0.5363325363, 0.0514459929],
  [0.2119034982, 0.6806995451, 0.1073969566],
  [0.0883024619, 0.2817188376, 0.6299787005],
];

const M2 = [
  [0.2104542553, 0.7936177850, -0.0040720468],
  [1.9779984951, -2.4285922050, 0.4505937099],
  [0.0259040371, 0.7827717662, -0.8086757660],
];

function srgbChannelToLinear(c) {
  const x = Math.min(1, Math.max(0, c / 255));
  return x <= 0.04045 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
}

function cbrt(x) {
  return Math.sign(x) * Math.pow(Math.abs(x), 1 / 3);
}

export function srgbToOklab(r, g, b) {
  const rl = srgbChannelToLinear(r);
  const gl = srgbChannelToLinear(g);
  const bl = srgbChannelToLinear(b);
  const l = M1[0][0] * rl + M1[0][1] * gl + M1[0][2] * bl;
  const m = M1[1][0] * rl + M1[1][1] * gl + M1[1][2] * bl;
  const s = M1[2][0] * rl + M1[2][1] * gl + M1[2][2] * bl;
  const lp = cbrt(l), mp = cbrt(m), sp = cbrt(s);
  return [
    M2[0][0] * lp + M2[0][1] * mp + M2[0][2] * sp,
    M2[1][0] * lp + M2[1][1] * mp + M2[1][2] * sp,
    M2[2][0] * lp + M2[2][1] * mp + M2[2][2] * sp,
  ];
}

// パレット OKLab を事前計算
const PALETTE_OKLAB = PALETTE_RGB.map(([r, g, b]) => srgbToOklab(r, g, b));

// ---------- ファインダー検出 (中心暗+リング明スコア) ----------

/**
 * RGB Uint8Array (h*w*3) と画像寸法から積分画像 (暗 / 明) を作る。
 * 返り値: { darkInt: Int32Array (h+1)*(w+1), lightInt: Int32Array, w, h }
 */
function buildIntegralImages(rgb, w, h, darkThreshold = 64, lightThreshold = 160) {
  const stride = w + 1;
  const darkInt = new Int32Array((h + 1) * stride);
  const lightInt = new Int32Array((h + 1) * stride);
  for (let y = 0; y < h; y++) {
    let rowDark = 0;
    let rowLight = 0;
    for (let x = 0; x < w; x++) {
      const idx = (y * w + x) * 3;
      const gray = (rgb[idx] + rgb[idx + 1] + rgb[idx + 2]) / 3;
      rowDark += gray < darkThreshold ? 1 : 0;
      rowLight += gray > lightThreshold ? 1 : 0;
      darkInt[(y + 1) * stride + x + 1] = darkInt[y * stride + x + 1] + rowDark;
      lightInt[(y + 1) * stride + x + 1] = lightInt[y * stride + x + 1] + rowLight;
    }
  }
  return { darkInt, lightInt, w, h };
}

function boxSum(integral, stride, y0, x0, y1, x1) {
  return integral[y1 * stride + x1]
    - integral[y0 * stride + x1]
    - integral[y1 * stride + x0]
    + integral[y0 * stride + x0];
}

/**
 * 歪み画像から 4 隅 (TL/TR/BL/BR) のファインダー中心 (x, y) px を検出する。
 *
 * QR-style の 1:1:3:1:1 比率検出: ファインダーパターンの中心を水平/垂直に走査すると
 * B-W-B-W-B が 1:1:3:1:1 の比で並ぶ (これはセルピッチに関係なく成立する scale-invariant な特徴)。
 *
 * アルゴリズム:
 * 1. 各 row, col で run-length encoding
 * 2. 5 連続 run (B-W-B-W-B) で 1:1:3:1:1 比率に合うものを候補にする
 * 3. row 候補と col 候補が交差する位置 = ファインダー中心
 * 4. 各象限で 1 つの最有力中心を選ぶ
 *
 * 返り値: [[x,y],[x,y],[x,y],[x,y]] 順は TL, TR, BL, BR
 */
export function findFinderCenters(rgb, w, h, finderSizeCells, cellPx, darkThreshold = 64) {
  // 2 値化 (luminance < threshold が 1, else 0)
  const binary = new Uint8Array(w * h);
  for (let i = 0; i < w * h; i++) {
    const j = i * 3;
    binary[i] = (rgb[j] + rgb[j + 1] + rgb[j + 2]) / 3 < darkThreshold ? 1 : 0;
  }

  // 1 列のラン列に対して 1:1:3:1:1 を探し、3-unit B の中心位置と推定 unit を返す
  function findRatioInLine(getVal, length, tol = 0.5, minUnit = 3) {
    const hits = []; // { center, unit }
    let i = 0;
    const runs = []; // [value, start, length]
    while (i < length) {
      const v = getVal(i);
      const start = i;
      let n = 0;
      while (i < length && getVal(i) === v) {
        n++;
        i++;
      }
      runs.push([v, start, n]);
    }
    for (let k = 0; k + 4 < runs.length; k++) {
      const r = [runs[k], runs[k + 1], runs[k + 2], runs[k + 3], runs[k + 4]];
      if (r[0][0] !== 1 || r[1][0] !== 0 || r[2][0] !== 1 || r[3][0] !== 0 || r[4][0] !== 1) continue;
      const lens = [r[0][2], r[1][2], r[2][2], r[3][2], r[4][2]];
      const unit = (lens[0] + lens[1] + lens[3] + lens[4]) / 4;
      // モアレ (1-2 px 周期) を除外。本物のセルは ≥3 px ある想定。
      if (unit < minUnit) continue;
      const okU = (n) => Math.abs(n - unit) <= unit * tol;
      const ok3 = (n) => Math.abs(n - 3 * unit) <= 3 * unit * tol;
      if (okU(lens[0]) && okU(lens[1]) && ok3(lens[2]) && okU(lens[3]) && okU(lens[4])) {
        const center = r[2][1] + Math.floor(r[2][2] / 2);
        hits.push({ center, unit });
      }
    }
    return hits;
  }

  // Row 候補: 各 row で BWBWB を探す → (x_center, y, unit)
  const rowHits = [];
  for (let y = 0; y < h; y++) {
    const hits = findRatioInLine((i) => binary[y * w + i], w);
    for (const hit of hits) {
      rowHits.push({ x: hit.center, y, unit: hit.unit });
    }
  }
  // Col 候補: 各 col で BWBWB を探す → (x, y_center, unit)
  const colHits = [];
  for (let x = 0; x < w; x++) {
    const hits = findRatioInLine((i) => binary[i * w + x], h);
    for (const hit of hits) {
      colHits.push({ x, y: hit.center, unit: hit.unit });
    }
  }

  // 象限分割 (出力順: TL, TR, BL, BR)
  const halfW = w / 2;
  const halfH = h / 2;
  const quadrants = [
    [0, 0, halfW, halfH],
    [halfW, 0, w, halfH],
    [0, halfH, halfW, h],
    [halfW, halfH, w, h],
  ];

  const detected = [];
  for (const [xs, ys, xe, ye] of quadrants) {
    const rh = rowHits.filter((c) => c.x >= xs && c.x < xe && c.y >= ys && c.y < ye);
    const ch = colHits.filter((c) => c.x >= xs && c.x < xe && c.y >= ys && c.y < ye);

    if (rh.length === 0 || ch.length === 0) {
      detected.push([(xs + xe) / 2, (ys + ye) / 2]);
      continue;
    }

    // 交差点を取る (row hit と col hit が同位置 + 同 unit)
    const intersections = [];
    for (const r of rh) {
      for (const c of ch) {
        const unitAvg = (r.unit + c.unit) / 2;
        const posTol = unitAvg * 3.5;
        const unitTol = unitAvg * 0.5;
        if (Math.abs(r.x - c.x) > posTol) continue;
        if (Math.abs(r.y - c.y) > posTol) continue;
        if (Math.abs(r.unit - c.unit) > unitTol) continue;
        intersections.push({
          x: (r.x + c.x) / 2,
          y: (r.y + c.y) / 2,
          unit: unitAvg,
        });
      }
    }

    if (intersections.length === 0) {
      const sortedX = rh.map((c) => c.x).sort((a, b) => a - b);
      const sortedY = ch.map((c) => c.y).sort((a, b) => a - b);
      detected.push([
        sortedX[Math.floor(sortedX.length / 2)],
        sortedY[Math.floor(sortedY.length / 2)],
      ]);
      continue;
    }

    // 単純な重心ではなく、最密クラスターを採用する。
    // 各交差点で「近傍にある他の交差点の数」をカウントし、最多の点を中心と見なす。
    // モアレ等で生じた孤立 (or 散らばった) 偽陽性は近傍数が少なくなり弾かれる。
    let bestIdx = 0;
    let bestCount = -1;
    for (let i = 0; i < intersections.length; i++) {
      const tol = intersections[i].unit * 4;
      let count = 0;
      for (let j = 0; j < intersections.length; j++) {
        const dx = intersections[i].x - intersections[j].x;
        const dy = intersections[i].y - intersections[j].y;
        if (dx * dx + dy * dy <= tol * tol) count++;
      }
      if (count > bestCount) {
        bestCount = count;
        bestIdx = i;
      }
    }
    // 最密クラスター内の点の重心を取る
    const seed = intersections[bestIdx];
    const tol = seed.unit * 4;
    let sumX = 0, sumY = 0, n = 0;
    for (const p of intersections) {
      const dx = p.x - seed.x;
      const dy = p.y - seed.y;
      if (dx * dx + dy * dy <= tol * tol) {
        sumX += p.x;
        sumY += p.y;
        n++;
      }
    }
    detected.push([sumX / n, sumY / n]);
  }
  return detected;
}

// ---------- 透視変換 (4 点で 3x3 homography) ----------

/**
 * src (4×{x,y}) → dst (4×{x,y}) の 3x3 ホモグラフィーを求める。
 * h[2][2] = 1 に正規化し、8x8 線形系を Gauss elimination で解く。
 * src, dst: [[x,y], ...] (length 4)
 * 返り値: 3x3 配列
 */
export function perspectiveMatrix(src, dst) {
  // 8 equations, 8 unknowns [a, b, c, d, e, f, g, h] (with i=1 fixed)
  const A = [];
  const b = [];
  for (let i = 0; i < 4; i++) {
    const [sx, sy] = src[i];
    const [dx, dy] = dst[i];
    A.push([sx, sy, 1, 0, 0, 0, -sx * dx, -sy * dx]);
    b.push(dx);
    A.push([0, 0, 0, sx, sy, 1, -sx * dy, -sy * dy]);
    b.push(dy);
  }
  const x = solveLinearSystem(A, b);
  return [
    [x[0], x[1], x[2]],
    [x[3], x[4], x[5]],
    [x[6], x[7], 1],
  ];
}

/** N×N の線形系 A x = b を Gauss elimination + back substitution で解く。 */
function solveLinearSystem(A, b) {
  const n = b.length;
  // Augmented matrix
  const M = A.map((row, i) => [...row, b[i]]);
  // Forward elimination with partial pivoting
  for (let i = 0; i < n; i++) {
    let pivot = i;
    for (let k = i + 1; k < n; k++) {
      if (Math.abs(M[k][i]) > Math.abs(M[pivot][i])) pivot = k;
    }
    if (pivot !== i) [M[i], M[pivot]] = [M[pivot], M[i]];
    if (Math.abs(M[i][i]) < 1e-12) throw new Error("singular matrix in solveLinearSystem");
    for (let k = i + 1; k < n; k++) {
      const factor = M[k][i] / M[i][i];
      for (let j = i; j <= n; j++) M[k][j] -= factor * M[i][j];
    }
  }
  const x = new Array(n).fill(0);
  for (let i = n - 1; i >= 0; i--) {
    let sum = M[i][n];
    for (let j = i + 1; j < n; j++) sum -= M[i][j] * x[j];
    x[i] = sum / M[i][i];
  }
  return x;
}

// ---------- 歪み画像から直接セルサンプリング ----------

/**
 * 各セルの中心領域 (samplesPerAxis × samplesPerAxis) を unwarped 座標系で取り、
 * forward perspective で歪み画像座標に変換、bilinear で 1 回だけ補間して平均する。
 *
 * 返り値: Float32Array (grid_h × grid_w × 3) の sRGB float (clip 前)。
 */
export function sampleCellsThroughPerspective(
  rgb, imgW, imgH,
  observedCorners, expectedCorners,
  gridW, gridH, cellPx,
  samplesPerAxis = 4
) {
  const forward = perspectiveMatrix(expectedCorners, observedCorners);
  const border = cellPx / 4;
  const offsets = new Array(samplesPerAxis);
  for (let i = 0; i < samplesPerAxis; i++) {
    offsets[i] = border + (i * (cellPx - 2 * border - 1)) / (samplesPerAxis - 1);
  }

  const result = new Float32Array(gridH * gridW * 3);
  for (let gy = 0; gy < gridH; gy++) {
    const cellOriginY = gy * cellPx;
    for (let gx = 0; gx < gridW; gx++) {
      const cellOriginX = gx * cellPx;
      let sumR = 0, sumG = 0, sumB = 0;
      const n = samplesPerAxis * samplesPerAxis;
      for (let oy = 0; oy < samplesPerAxis; oy++) {
        const sy = cellOriginY + offsets[oy];
        for (let ox = 0; ox < samplesPerAxis; ox++) {
          const sx = cellOriginX + offsets[ox];
          // forward 変換 (3x3 @ [sx, sy, 1])
          const dx = forward[0][0] * sx + forward[0][1] * sy + forward[0][2];
          const dy = forward[1][0] * sx + forward[1][1] * sy + forward[1][2];
          const dw = forward[2][0] * sx + forward[2][1] * sy + forward[2][2];
          const fx = dx / dw;
          const fy = dy / dw;
          const [pr, pg, pb] = bilinearSample(rgb, imgW, imgH, fx, fy);
          sumR += pr;
          sumG += pg;
          sumB += pb;
        }
      }
      const idx = (gy * gridW + gx) * 3;
      result[idx] = sumR / n;
      result[idx + 1] = sumG / n;
      result[idx + 2] = sumB / n;
    }
  }
  return result;
}

function bilinearSample(rgb, w, h, x, y) {
  // 範囲外はクリップ
  const xc = Math.min(w - 1, Math.max(0, x));
  const yc = Math.min(h - 1, Math.max(0, y));
  const x0 = Math.floor(xc);
  const y0 = Math.floor(yc);
  const x1 = Math.min(w - 1, x0 + 1);
  const y1 = Math.min(h - 1, y0 + 1);
  const wx = xc - x0;
  const wy = yc - y0;
  const get = (yy, xx) => {
    const i = (yy * w + xx) * 3;
    return [rgb[i], rgb[i + 1], rgb[i + 2]];
  };
  const v00 = get(y0, x0);
  const v01 = get(y0, x1);
  const v10 = get(y1, x0);
  const v11 = get(y1, x1);
  const r = (1 - wx) * (1 - wy) * v00[0] + wx * (1 - wy) * v01[0] + (1 - wx) * wy * v10[0] + wx * wy * v11[0];
  const g = (1 - wx) * (1 - wy) * v00[1] + wx * (1 - wy) * v01[1] + (1 - wx) * wy * v10[1] + wx * wy * v11[1];
  const b = (1 - wx) * (1 - wy) * v00[2] + wx * (1 - wy) * v01[2] + (1 - wx) * wy * v10[2] + wx * wy * v11[2];
  return [r, g, b];
}

// ---------- キャリブレーション + OKLab 量子化 ----------

/**
 * spec.calibration_rows > 0 を前提に、サンプル済みセル中心色から
 * キャリブレーションパッチ 8 個を取り出して 3x3 線形補正行列を最小二乗で求め、
 * データセルに逆適用してから OKLab 距離で最近傍パレット色に量子化する。
 *
 * centers: Float32Array (gridH × gridW × 3) — sampleCellsThroughPerspective の出力
 * 返り値: Uint8Array (gridH × gridW) — palette index
 */
export function quantizeWithCalibration(centers, spec) {
  const { gridWidth, gridHeight, calibrationRowStart, calibrationRows } = spec;

  // キャリブレーションパッチ観測色 (8 × 3)
  const observed = sampleCalibrationPatches(centers, spec);
  const truePalette = PALETTE_RGB.map((c) => [c[0], c[1], c[2]]);

  // observed @ C = truePalette を満たす C (3x3) を normal equations で求める
  // C = (O^T O)^-1 O^T T
  const cMatrix = lstsq8x3(observed, truePalette);

  // データセルに適用しながら最近傍量子化
  const out = new Uint8Array(gridHeight * gridWidth);
  for (let gy = 0; gy < gridHeight; gy++) {
    for (let gx = 0; gx < gridWidth; gx++) {
      const idx = (gy * gridWidth + gx) * 3;
      const r = centers[idx], g = centers[idx + 1], b = centers[idx + 2];
      const cr = cMatrix[0][0] * r + cMatrix[1][0] * g + cMatrix[2][0] * b;
      const cg = cMatrix[0][1] * r + cMatrix[1][1] * g + cMatrix[2][1] * b;
      const cb = cMatrix[0][2] * r + cMatrix[1][2] * g + cMatrix[2][2] * b;
      const lab = srgbToOklab(cr, cg, cb);
      out[gy * gridWidth + gx] = nearestPaletteOklab(lab);
    }
  }
  return out;
}

function sampleCalibrationPatches(centers, spec) {
  const { gridWidth, calibrationRowStart, calibrationRows } = spec;
  const samples = [];
  for (let i = 0; i < 8; i++) {
    const colStart = Math.floor((i * gridWidth) / 8);
    const colEnd = Math.floor(((i + 1) * gridWidth) / 8);
    let sumR = 0, sumG = 0, sumB = 0;
    let count = 0;
    for (let r = calibrationRowStart; r < calibrationRowStart + calibrationRows; r++) {
      for (let c = colStart; c < colEnd; c++) {
        const idx = (r * gridWidth + c) * 3;
        sumR += centers[idx];
        sumG += centers[idx + 1];
        sumB += centers[idx + 2];
        count++;
      }
    }
    samples.push([sumR / count, sumG / count, sumB / count]);
  }
  return samples;
}

/**
 * 8x3 観測 → 8x3 真値 を満たす 3x3 行列 C を最小二乗で求める。
 * Normal equations: C = (O^T O)^-1 O^T T
 * 返り値: 3x3 (cMatrix[k][col] = k 番目入力次元から col 番目出力次元への寄与)
 */
function lstsq8x3(observed, truePalette) {
  // OtO = O^T O (3x3)
  const otO = [[0, 0, 0], [0, 0, 0], [0, 0, 0]];
  for (let i = 0; i < 8; i++) {
    for (let r = 0; r < 3; r++) {
      for (let c = 0; c < 3; c++) {
        otO[r][c] += observed[i][r] * observed[i][c];
      }
    }
  }
  // OtT = O^T T (3x3)
  const otT = [[0, 0, 0], [0, 0, 0], [0, 0, 0]];
  for (let i = 0; i < 8; i++) {
    for (let r = 0; r < 3; r++) {
      for (let c = 0; c < 3; c++) {
        otT[r][c] += observed[i][r] * truePalette[i][c];
      }
    }
  }
  // C = otO^-1 @ otT
  const otOInv = invert3x3(otO);
  const c = [[0, 0, 0], [0, 0, 0], [0, 0, 0]];
  for (let r = 0; r < 3; r++) {
    for (let cc = 0; cc < 3; cc++) {
      let sum = 0;
      for (let k = 0; k < 3; k++) sum += otOInv[r][k] * otT[k][cc];
      c[r][cc] = sum;
    }
  }
  return c;
}

function invert3x3(m) {
  const a = m[0][0], b = m[0][1], c = m[0][2];
  const d = m[1][0], e = m[1][1], f = m[1][2];
  const g = m[2][0], h = m[2][1], i = m[2][2];
  const det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
  if (Math.abs(det) < 1e-12) throw new Error("singular 3x3 in invert3x3");
  const invDet = 1 / det;
  return [
    [(e * i - f * h) * invDet, (c * h - b * i) * invDet, (b * f - c * e) * invDet],
    [(f * g - d * i) * invDet, (a * i - c * g) * invDet, (c * d - a * f) * invDet],
    [(d * h - e * g) * invDet, (b * g - a * h) * invDet, (a * e - b * d) * invDet],
  ];
}

function nearestPaletteOklab(lab) {
  let bestIdx = 0;
  let bestDist = Infinity;
  for (let i = 0; i < 8; i++) {
    const [pl, pa, pb] = PALETTE_OKLAB[i];
    const dl = lab[0] - pl, da = lab[1] - pa, db = lab[2] - pb;
    const d = dl * dl + da * da + db * db;
    if (d < bestDist) {
      bestDist = d;
      bestIdx = i;
    }
  }
  return bestIdx;
}

// ---------- フルパイプライン ----------

/**
 * RGB 画像(任意サイズ)から 4 ファインダー検出 → 透視サンプリング → キャリブレーション
 * → OKLab 量子化 → セル列 (palette index) を返す。
 *
 * spec: { gridWidth, gridHeight, cellPx, finderSize, calibrationRowStart, calibrationRows }
 */
export function decodeImageToCells(rgb, w, h, spec) {
  const observed = findFinderCenters(rgb, w, h, spec.finderSize, spec.cellPx);
  const fpHalf = (spec.finderSize * spec.cellPx) / 2;
  const expected = [
    [fpHalf, fpHalf],
    [w - fpHalf, fpHalf],
    [fpHalf, h - fpHalf],
    [w - fpHalf, h - fpHalf],
  ];
  const centers = sampleCellsThroughPerspective(
    rgb, w, h, observed, expected,
    spec.gridWidth, spec.gridHeight, spec.cellPx
  );
  return quantizeWithCalibration(centers, spec);
}

// 既定の Phase 0 spec
export const PHASE_0_SPEC = {
  gridWidth: 128,
  gridHeight: 128,
  cellPx: 8,
  finderSize: 7,
  calibrationRowStart: 64,
  calibrationRows: 1,
};
