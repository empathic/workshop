<script lang="ts">
  import { onDestroy } from 'svelte';
  import { onLobbyMessage } from '$lib/stores/websocket';

  // Tiny grid rendered inside the monitor icon
  const GW = 30;
  const GH = 20;
  const AI_TICK = 140;
  const STALE_MS = 3000;

  // Read theme colors from CSS variables at runtime
  function cssVar(name: string, fallback: string): string {
    if (typeof document === 'undefined') return fallback;
    return getComputedStyle(document.body).getPropertyValue(name).trim() || fallback;
  }
  function getFoodColor(): string {
    return cssVar('--status-red', '#ef4444');
  }
  function getPlayerColors(): string[] {
    return [
      cssVar('--accent-500', '#fb923c'),
      cssVar('--thinking-500', '#8b5cf6'),
      cssVar('--status-green', '#22c55e'),
      cssVar('--status-blue', '#60a5fa'),
      cssVar('--status-blue', '#60a5fa'),
      cssVar('--status-yellow', '#fbbf24'),
      cssVar('--thinking-400', '#a78bfa'),
      cssVar('--accent-400', '#fdba74')
    ];
  }

  let FOOD_COLOR = getFoodColor();
  let PLAYER_COLORS = getPlayerColors();

  type Pt = [number, number];
  type Dir = 'up' | 'down' | 'left' | 'right';

  interface RemoteSnake {
    body: Pt[];
    color: string;
    alive: boolean;
    lastSeen: number;
  }

  // --- State ---
  let canvas = $state<HTMLCanvasElement>(undefined!);
  let frame: number | null = null;
  let aiTimer: ReturnType<typeof setInterval> | null = null;

  // AI snake (plays when no one else is)
  let aiBody: Pt[] = [
    [15, 10],
    [14, 10],
    [13, 10]
  ];
  let aiDir: Dir = 'right';
  let aiFood: Pt = spawnFood();

  // Remote players from lobby
  let remotes = new Map<string, RemoteSnake>();
  let remoteFood: Pt[] = [];
  let hasRemotes = false;

  // --- Lobby subscription ---
  const unsub = onLobbyMessage('snake', (senderId, payload) => {
    const msg = payload as {
      kind: string;
      body?: Pt[];
      color?: string;
      alive?: boolean;
      food?: { pos: Pt }[];
      newFood?: { pos: Pt };
      foodId?: number;
    };
    if (!msg?.kind) return;
    const now = Date.now();

    switch (msg.kind) {
      case 'state':
        if (msg.body) {
          const existing = remotes.get(senderId);
          remotes.set(senderId, {
            body: msg.body,
            color: existing?.color ?? PLAYER_COLORS[remotes.size % PLAYER_COLORS.length],
            alive: msg.alive ?? true,
            lastSeen: now
          });
          hasRemotes = true;
        }
        break;
      case 'join':
        remotes.set(senderId, {
          body: [],
          color: msg.color ?? PLAYER_COLORS[remotes.size % PLAYER_COLORS.length],
          alive: true,
          lastSeen: now
        });
        hasRemotes = true;
        break;
      case 'leave':
        remotes.delete(senderId);
        hasRemotes = remotes.size > 0;
        break;
      case 'food_sync':
        if (msg.food) remoteFood = msg.food.map((f) => f.pos);
        break;
      case 'food_eaten':
        if (msg.newFood) {
          // Simple: just accept the new food list will come via food_sync or accumulate
          // For the teaser we just track positions loosely
          if (msg.foodId !== undefined) {
            remoteFood = remoteFood.filter((_, i) => i !== msg.foodId);
          }
          remoteFood = [...remoteFood, msg.newFood.pos];
        }
        break;
    }
  });

  // --- Lifecycle ---
  $effect(() => {
    if (canvas) {
      canvas.width = 120;
      canvas.height = 80;
      startAI();
      renderLoop();
    }
    return () => {
      if (frame) cancelAnimationFrame(frame);
      if (aiTimer) clearInterval(aiTimer);
    };
  });

  onDestroy(() => {
    unsub();
    if (frame) cancelAnimationFrame(frame);
    if (aiTimer) clearInterval(aiTimer);
  });

  // --- AI snake ---
  function startAI() {
    aiTimer = setInterval(tickAI, AI_TICK);
  }

  function tickAI() {
    // Prune stale remotes
    const now = Date.now();
    let changed = false;
    for (const [id, r] of remotes) {
      if (now - r.lastSeen > STALE_MS) {
        remotes.delete(id);
        changed = true;
      }
    }
    if (changed) hasRemotes = remotes.size > 0;

    // Don't tick AI if we're showing real players
    if (hasRemotes) return;

    // Simple AI: chase food, avoid self
    aiDir = aiChooseDir();
    const head = aiBody[0];
    let [nx, ny] = step(head, aiDir);
    nx = ((nx % GW) + GW) % GW;
    ny = ((ny % GH) + GH) % GH;

    // Self collision → respawn
    if (aiBody.some((s) => s[0] === nx && s[1] === ny)) {
      aiBody = [
        [15, 10],
        [14, 10],
        [13, 10]
      ];
      aiDir = 'right';
      aiFood = spawnFood();
      return;
    }

    aiBody = [[nx, ny], ...aiBody];

    if (nx === aiFood[0] && ny === aiFood[1]) {
      aiFood = spawnFood();
      // keep tail (grow)
    } else {
      aiBody.pop();
    }
  }

  function aiChooseDir(): Dir {
    const head = aiBody[0];
    const dirs: Dir[] = ['up', 'down', 'left', 'right'];
    const opposite: Record<Dir, Dir> = { up: 'down', down: 'up', left: 'right', right: 'left' };

    // Filter: no 180, no self-collision
    const safe = dirs.filter((d) => {
      if (d === opposite[aiDir]) return false;
      const [nx, ny] = step(head, d);
      const wx = ((nx % GW) + GW) % GW;
      const wy = ((ny % GH) + GH) % GH;
      return !aiBody.some((s) => s[0] === wx && s[1] === wy);
    });

    if (safe.length === 0) return aiDir;

    // Prefer direction toward food
    const dx = aiFood[0] - head[0];
    const dy = aiFood[1] - head[1];

    const scored = safe.map((d) => {
      const [sx, sy] = step([0, 0], d);
      return { d, score: sx * Math.sign(dx) + sy * Math.sign(dy) };
    });
    scored.sort((a, b) => b.score - a.score);

    // Small random chance to not go optimal (looks more natural)
    if (Math.random() < 0.15 && scored.length > 1) return scored[1].d;
    return scored[0].d;
  }

  function step(pt: Pt, dir: Dir): Pt {
    switch (dir) {
      case 'up':
        return [pt[0], pt[1] - 1];
      case 'down':
        return [pt[0], pt[1] + 1];
      case 'left':
        return [pt[0] - 1, pt[1]];
      case 'right':
        return [pt[0] + 1, pt[1]];
    }
  }

  function spawnFood(): Pt {
    for (let i = 0; i < 100; i++) {
      const p: Pt = [Math.floor(Math.random() * GW), Math.floor(Math.random() * GH)];
      if (!aiBody.some((s) => s[0] === p[0] && s[1] === p[1])) return p;
    }
    return [0, 0];
  }

  // --- Rendering ---
  function renderLoop() {
    render();
    frame = requestAnimationFrame(renderLoop);
  }

  function render() {
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const w = canvas.width;
    const h = canvas.height;

    // The monitor "screen" area within the icon (roughly the inner rectangle)
    // We draw the game inside this region
    const sx = 2,
      sy = 2,
      sw = w - 4,
      sh = h - 4;
    const cw = sw / GW;
    const ch = sh / GH;

    // Background
    ctx.fillStyle = cssVar('--surface-900', '#0a0806');
    ctx.fillRect(0, 0, w, h);

    if (hasRemotes) {
      // Show real players
      for (const [, r] of remotes) {
        if (r.body.length === 0 || !r.alive) continue;
        drawBody(ctx, r.body, r.color, sx, sy, cw, ch);
      }
      // Remote food
      for (const f of remoteFood) {
        ctx.fillStyle = FOOD_COLOR;
        ctx.fillRect(sx + f[0] * cw, sy + f[1] * ch, Math.max(cw, 1.5), Math.max(ch, 1.5));
      }
    } else {
      // AI snake
      drawBody(ctx, aiBody, cssVar('--accent-500', '#fb923c'), sx, sy, cw, ch);
      // Food
      ctx.fillStyle = FOOD_COLOR;
      ctx.fillRect(sx + aiFood[0] * cw, sy + aiFood[1] * ch, Math.max(cw, 1.5), Math.max(ch, 1.5));
    }

    // Scanline overlay
    ctx.fillStyle = cssVar('--ambient-tint', 'rgba(0, 0, 0, 0.06)');
    for (let y = 0; y < h; y += 3) {
      ctx.fillRect(0, y, w, 1);
    }
  }

  function drawBody(
    ctx: CanvasRenderingContext2D,
    body: Pt[],
    color: string,
    ox: number,
    oy: number,
    cw: number,
    ch: number
  ) {
    for (let i = body.length - 1; i >= 0; i--) {
      const [bx, by] = body[i];
      const alpha = 1 - (i / Math.max(body.length, 1)) * 0.6;
      ctx.globalAlpha = alpha;
      ctx.fillStyle = color;
      if (i === 0) {
        // Head — slightly brighter
        ctx.fillRect(ox + bx * cw, oy + by * ch, Math.max(cw, 1.5), Math.max(ch, 1.5));
      } else {
        ctx.fillRect(ox + bx * cw + 0.25, oy + by * ch + 0.25, Math.max(cw - 0.5, 1), Math.max(ch - 0.5, 1));
      }
    }
    ctx.globalAlpha = 1;
  }
</script>

<canvas bind:this={canvas} class="teaser-canvas"></canvas>

<style>
  .teaser-canvas {
    width: 100%;
    height: 100%;
    display: block;
    image-rendering: pixelated;
    border-radius: 4px;
  }
</style>
