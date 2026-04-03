<script lang="ts">
  /**
   * VoiceVisualizer — Phosphor decay spectrum analyzer
   *
   * Modeled after hardware audio gear: discrete phosphor segments stacked
   * vertically per frequency band. Each segment is independently energized
   * and decays at its own rate — hot white-amber when first lit, cooling
   * through warm amber to dim orange as the phosphor dies. Segments have
   * visible gaps, like a real segmented VU display.
   */

  interface Props {
    /** Shared frequency data buffer — written by voice analyser at ~60fps */
    data: Uint8Array;
  }

  let { data }: Props = $props();

  let canvas: HTMLCanvasElement;
  let container: HTMLDivElement;

  const BAR_COUNT = 64;
  const BAR_GAP = 1;
  const ROWS = 12; // discrete phosphor segments per column
  const ROW_GAP = 1; // gap between segments for hardware look
  // Reverse-log decay: holds bright, then accelerates into darkness
  // Rate interpolates from DECAY_LO (dim → fast fade) to DECAY_HI (bright → slow fade)
  const DECAY_HI = 0.995;
  const DECAY_LO = 0.94;

  // Read a CSS variable from the active theme, with fallback
  function cssVar(name: string, fallback: string): string {
    if (typeof document === 'undefined') return fallback;
    return getComputedStyle(document.body).getPropertyValue(name).trim() || fallback;
  }

  // Parse a hex color (#rrggbb or #rgb) into [r, g, b]
  function hexToRgb(hex: string): [number, number, number] {
    const h = hex.replace('#', '');
    const full = h.length === 3 ? h.split('').map((c) => c + c).join('') : h;
    return [parseInt(full.slice(0, 2), 16), parseInt(full.slice(2, 4), 16), parseInt(full.slice(4, 6), 16)];
  }

  // Theme-derived phosphor colors: dim ember (bottom) and hot amber (top)
  const [loR, loG, loB] = hexToRgb(cssVar('--accent-600', '#d97706'));
  const [hiR, hiG, hiB] = hexToRgb(cssVar('--accent-500', '#fb923c'));

  // 2D phosphor grid: each cell has its own brightness 0–1
  const phosphor: Float32Array[] = Array.from({ length: BAR_COUNT }, () => new Float32Array(ROWS));

  $effect(() => {
    if (!canvas || !container) return;
    const ctx = canvas.getContext('2d', { alpha: true })!;
    let alive = true;
    const dpr = window.devicePixelRatio || 1;

    function resize() {
      const rect = container.getBoundingClientRect();
      canvas.width = rect.width * dpr;
      canvas.height = rect.height * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }

    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(container);

    function render() {
      if (!alive) return;

      const w = canvas.width / dpr;
      const h = canvas.height / dpr;

      ctx.clearRect(0, 0, w, h);

      const binCount = data.length || 256;
      const usableBins = Math.floor(binCount * 0.4);
      const cellH = (h - (ROWS - 1) * ROW_GAP) / ROWS;

      for (let i = 0; i < BAR_COUNT; i++) {
        const x = (i / BAR_COUNT) * w;
        const xNext = ((i + 1) / BAR_COUNT) * w;
        const lineW = xNext - x - BAR_GAP;

        // Compute target level from frequency data
        const startBin = Math.floor((i / BAR_COUNT) * usableBins);
        const endBin = Math.max(startBin + 1, Math.floor(((i + 1) / BAR_COUNT) * usableBins));
        let sum = 0;
        for (let b = startBin; b < endBin && b < binCount; b++) {
          sum += data[b] ?? 0;
        }
        const raw = sum / (endBin - startBin) / 255;
        // Boost + compress: sqrt gives more headroom at low levels
        const target = Math.min(1, Math.sqrt(raw) * 1.15);
        const litRows = Math.floor(target * ROWS);
        const tipRow = litRows > 0 ? ROWS - litRows : -1;

        const col = phosphor[i];

        for (let r = 0; r < ROWS; r++) {
          const isLit = r >= ROWS - litRows;

          if (isLit) {
            col[r] = Math.max(col[r], 0.3 + target * 0.7);
          } else {
            // Reverse-log: slow when bright, accelerates as it dims
            const rate = DECAY_LO + (DECAY_HI - DECAY_LO) * col[r];
            col[r] *= rate;
          }

          const brightness = col[r];
          if (brightness < 0.01) {
            col[r] = 0;
            continue;
          }

          const cy = r * (cellH + ROW_GAP);
          const isTip = r === tipRow;

          // Position in column: 0 = bottom, 1 = top
          const pos = 1 - r / (ROWS - 1);

          // Gradient by position: bottom=dim ember, mid=warm orange, top=hot amber
          // Lerp between theme-derived low (--accent-600) and high (--accent-500)
          const baseR = Math.floor(loR + pos * (hiR - loR));
          const baseG = Math.floor(loG + pos * (hiG - loG));
          const baseB = Math.floor(loB + pos * (hiB - loB));

          if (isTip) {
            // Hot tip — extra bright leading edge
            ctx.shadowBlur = 8 + brightness * 14;
            ctx.shadowColor = `rgba(${baseR}, ${Math.min(baseG + 30, 210)}, ${Math.min(baseB + 20, 130)}, ${brightness * 0.7})`;
            ctx.fillStyle = `rgba(${Math.min(baseR + 10, 255)}, ${Math.min(baseG + 40, 220)}, ${Math.min(baseB + 30, 140)}, ${0.8 + brightness * 0.2})`;
          } else {
            // Phosphor glow scales with brightness
            const glow = Math.min(brightness * 8, 6);
            ctx.shadowBlur = glow;
            ctx.shadowColor = `rgba(${baseR}, ${Math.floor(baseG * 0.7)}, ${Math.floor(baseB * 0.5)}, ${brightness * 0.4})`;
            ctx.fillStyle = `rgba(${baseR}, ${baseG}, ${baseB}, ${brightness * 0.9})`;
          }

          ctx.fillRect(x, cy, lineW, cellH);
          ctx.shadowBlur = 0;
        }
      }

      requestAnimationFrame(render);
    }

    requestAnimationFrame(render);

    return () => {
      alive = false;
      ro.disconnect();
    };
  });
</script>

<div class="visualizer" bind:this={container}>
  <canvas bind:this={canvas}></canvas>
</div>

<style>
  .visualizer {
    width: 100%;
    min-width: 0;
    height: 32px;
    align-self: stretch;
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
