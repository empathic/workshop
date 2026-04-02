<script lang="ts">
  import { onDestroy } from 'svelte';
  import { get } from 'svelte/store';
  import { sendLobbyMessage, onLobbyMessage, isConnected } from '$lib/stores/websocket';
  import { currentUser } from '$lib/stores/auth';

  interface Props {
    onexit?: () => void;
  }

  let { onexit }: Props = $props();

  // =========================================================================
  // Constants
  // =========================================================================

  const CHANNEL = 'snake';
  const GRID_W = 30;
  const GRID_H = 20;
  const TICK_MS = 120;
  const FOOD_COUNT = 3;

  // Read a CSS variable from the active theme, with fallback
  function cssVar(name: string, fallback: string): string {
    if (typeof document === 'undefined') return fallback;
    return getComputedStyle(document.body).getPropertyValue(name).trim() || fallback;
  }

  // Player colors — read from theme accent/status palette
  function getPlayerColors(): string[] {
    return [
      cssVar('--accent-500', '#fb923c'),
      cssVar('--thinking-500', '#8b5cf6'),
      cssVar('--status-green', '#22c55e'),
      cssVar('--status-blue', '#60a5fa'),
      cssVar('--status-yellow', '#fbbf24'),
      cssVar('--thinking-400', '#a78bfa'),
      cssVar('--accent-400', '#fdba74'),
      cssVar('--accent-600', '#d97706')
    ];
  }
  function getFoodColor(): string {
    return cssVar('--status-red', '#ef4444');
  }

  let PLAYER_COLORS = getPlayerColors();
  let FOOD_COLOR = getFoodColor();

  type Dir = 'up' | 'down' | 'left' | 'right';
  type Pt = [number, number];

  interface SnakeState {
    body: Pt[];
    dir: Dir;
    color: string;
    name: string;
    alive: boolean;
    score: number;
  }

  interface FoodItem {
    pos: Pt;
    id: number;
  }

  // Lobby message payloads
  type SnakePayload =
    | { kind: 'join'; name: string; color: string }
    | { kind: 'state'; body: Pt[]; dir: Dir; score: number; alive: boolean }
    | { kind: 'leave' }
    | { kind: 'food_sync'; food: FoodItem[] }
    | { kind: 'food_eaten'; foodId: number; newFood: FoodItem };

  // =========================================================================
  // State
  // =========================================================================

  let wrapper = $state<HTMLDivElement>(undefined!);
  let canvas = $state<HTMLCanvasElement>(undefined!);
  let gameRunning = $state(false);
  let myId = $state('');
  let mySnake: SnakeState | null = $state(null);
  let remoteSnakes = $state(new Map<string, SnakeState>());
  let food = $state<FoodItem[]>([]);
  let nextFoodId = 0;
  let tickTimer: ReturnType<typeof setInterval> | null = null;
  let renderFrame: number | null = null;
  let inputQueue: Dir[] = [];
  let unsubLobby: (() => void) | null = null;
  let broadcastTimer: ReturnType<typeof setInterval> | null = null;
  let staleTimer: ReturnType<typeof setInterval> | null = null;
  let lastRemoteUpdate = new Map<string, number>();
  let leaderboard = $derived(buildLeaderboard());
  let cellSize = $state(0);

  // =========================================================================
  // Lifecycle
  // =========================================================================

  $effect(() => {
    if (canvas) {
      sizeCanvas();
      renderLoop();
    }
    return () => {
      if (renderFrame) cancelAnimationFrame(renderFrame);
    };
  });

  onDestroy(() => {
    leaveGame();
  });

  function sizeCanvas() {
    if (!wrapper) return;
    const rect = wrapper.getBoundingClientRect();
    // Use most of the available space, leave some padding
    const maxW = rect.width - 48;
    const maxH = rect.height - 48;
    const cs = Math.max(8, Math.min(Math.floor(maxW / GRID_W), Math.floor(maxH / GRID_H)));
    cellSize = cs;
    canvas.width = GRID_W * cs;
    canvas.height = GRID_H * cs;
  }

  // =========================================================================
  // Game control
  // =========================================================================

  function joinGame() {
    if (gameRunning) return;
    if (!isConnected()) return;

    gameRunning = true;
    myId = crypto.randomUUID().slice(0, 8);

    const user = get(currentUser);
    const name = user?.display_name ?? 'anon';
    // Pick next unused color based on how many players are already in
    const usedColors = new Set([...remoteSnakes.values()].map((s) => s.color));
    const color =
      PLAYER_COLORS.find((c) => !usedColors.has(c)) ?? PLAYER_COLORS[remoteSnakes.size % PLAYER_COLORS.length];

    const startX = 5 + Math.floor(Math.random() * (GRID_W - 10));
    const startY = 5 + Math.floor(Math.random() * (GRID_H - 10));

    mySnake = {
      body: [
        [startX, startY],
        [startX - 1, startY],
        [startX - 2, startY]
      ],
      dir: 'right',
      color,
      name,
      alive: true,
      score: 0
    };
    inputQueue = [];

    // Spawn initial food (first joiner seeds it)
    if (food.length === 0) {
      food = [];
      for (let i = 0; i < FOOD_COUNT; i++) {
        food.push({ pos: randomFreeCell(), id: nextFoodId++ });
      }
      sendLobbyMessage(CHANNEL, { kind: 'food_sync', food } satisfies SnakePayload);
    }

    // Subscribe to lobby
    unsubLobby = onLobbyMessage(CHANNEL, handleLobbyMessage);

    // Announce join
    sendLobbyMessage(CHANNEL, { kind: 'join', name, color } satisfies SnakePayload);

    // Start game loop
    tickTimer = setInterval(tick, TICK_MS);

    // Broadcast state periodically
    broadcastTimer = setInterval(broadcastState, TICK_MS * 2);

    // Prune stale remote players every 3s
    staleTimer = setInterval(pruneStale, 3000);

    // Key listener
    window.addEventListener('keydown', onKeyDown);
  }

  function leaveGame() {
    if (!gameRunning) return;
    gameRunning = false;

    if (tickTimer) {
      clearInterval(tickTimer);
      tickTimer = null;
    }
    if (broadcastTimer) {
      clearInterval(broadcastTimer);
      broadcastTimer = null;
    }
    if (staleTimer) {
      clearInterval(staleTimer);
      staleTimer = null;
    }
    if (unsubLobby) {
      unsubLobby();
      unsubLobby = null;
    }

    window.removeEventListener('keydown', onKeyDown);

    try {
      sendLobbyMessage(CHANNEL, { kind: 'leave' } satisfies SnakePayload);
    } catch {}

    mySnake = null;
    remoteSnakes = new Map();
    food = [];
    lastRemoteUpdate = new Map();
  }

  function respawn() {
    if (!mySnake || !gameRunning) return;
    const startX = 5 + Math.floor(Math.random() * (GRID_W - 10));
    const startY = 5 + Math.floor(Math.random() * (GRID_H - 10));
    mySnake = {
      ...mySnake,
      body: [
        [startX, startY],
        [startX - 1, startY],
        [startX - 2, startY]
      ],
      dir: 'right',
      alive: true,
      score: 0
    };
    inputQueue = [];
  }

  // =========================================================================
  // Game tick
  // =========================================================================

  function tick() {
    if (!mySnake || !mySnake.alive) return;

    // Drain input queue (max one per tick for responsiveness)
    if (inputQueue.length > 0) {
      mySnake.dir = inputQueue.shift()!;
    }

    const head = mySnake.body[0];
    let [nx, ny] = move(head, mySnake.dir);

    // Wrap around
    nx = ((nx % GRID_W) + GRID_W) % GRID_W;
    ny = ((ny % GRID_H) + GRID_H) % GRID_H;

    // Self-collision (skip head)
    for (let i = 1; i < mySnake.body.length; i++) {
      if (mySnake.body[i][0] === nx && mySnake.body[i][1] === ny) {
        mySnake = { ...mySnake, alive: false };
        broadcastState();
        return;
      }
    }

    // Remote snake collision
    for (const [, rs] of remoteSnakes) {
      if (!rs.alive) continue;
      for (const seg of rs.body) {
        if (seg[0] === nx && seg[1] === ny) {
          mySnake = { ...mySnake, alive: false };
          broadcastState();
          return;
        }
      }
    }

    const newBody: Pt[] = [[nx, ny], ...mySnake.body];

    // Check food
    let ate = false;
    for (const f of food) {
      if (f.pos[0] === nx && f.pos[1] === ny) {
        ate = true;
        const newFood: FoodItem = { pos: randomFreeCell(), id: nextFoodId++ };
        food = food.map((fi) => (fi.id === f.id ? newFood : fi));
        mySnake = { ...mySnake, score: mySnake.score + 1 };
        sendLobbyMessage(CHANNEL, { kind: 'food_eaten', foodId: f.id, newFood } satisfies SnakePayload);
        break;
      }
    }

    if (!ate) {
      newBody.pop();
    }

    mySnake = { ...mySnake, body: newBody };
  }

  function move(pt: Pt, dir: Dir): Pt {
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

  function randomFreeCell(): Pt {
    for (let attempt = 0; attempt < 200; attempt++) {
      const x = Math.floor(Math.random() * GRID_W);
      const y = Math.floor(Math.random() * GRID_H);
      // Check no overlap with own snake
      if (mySnake) {
        let collision = false;
        for (const seg of mySnake.body) {
          if (seg[0] === x && seg[1] === y) {
            collision = true;
            break;
          }
        }
        if (collision) continue;
      }
      return [x, y];
    }
    return [Math.floor(Math.random() * GRID_W), Math.floor(Math.random() * GRID_H)];
  }

  // =========================================================================
  // Input
  // =========================================================================

  function onKeyDown(e: KeyboardEvent) {
    if (!mySnake) return;

    // Respawn on any key if dead
    if (!mySnake.alive) {
      if (e.key === ' ' || e.key === 'Enter') {
        e.preventDefault();
        respawn();
      }
      return;
    }

    const last = inputQueue.length > 0 ? inputQueue[inputQueue.length - 1] : mySnake.dir;
    let newDir: Dir | null = null;

    switch (e.key) {
      case 'ArrowUp':
      case 'w':
      case 'W':
        if (last !== 'down') newDir = 'up';
        break;
      case 'ArrowDown':
      case 's':
      case 'S':
        if (last !== 'up') newDir = 'down';
        break;
      case 'ArrowLeft':
      case 'a':
      case 'A':
        if (last !== 'right') newDir = 'left';
        break;
      case 'ArrowRight':
      case 'd':
      case 'D':
        if (last !== 'left') newDir = 'right';
        break;
      case 'Escape':
        leaveGame();
        onexit?.();
        return;
    }

    if (newDir) {
      e.preventDefault();
      // Buffer max 3 inputs to avoid flooding
      if (inputQueue.length < 3) {
        inputQueue.push(newDir);
      }
    }
  }

  // Touch controls
  let touchStart: { x: number; y: number } | null = null;

  function onTouchStart(e: TouchEvent) {
    if (!mySnake?.alive) {
      if (!mySnake?.alive && mySnake) respawn();
      return;
    }
    const t = e.touches[0];
    touchStart = { x: t.clientX, y: t.clientY };
  }

  function onTouchEnd(e: TouchEvent) {
    if (!touchStart || !mySnake?.alive) return;
    const t = e.changedTouches[0];
    const dx = t.clientX - touchStart.x;
    const dy = t.clientY - touchStart.y;
    touchStart = null;

    if (Math.abs(dx) < 20 && Math.abs(dy) < 20) return; // Too short, ignore

    const last = inputQueue.length > 0 ? inputQueue[inputQueue.length - 1] : mySnake.dir;
    let newDir: Dir | null = null;

    if (Math.abs(dx) > Math.abs(dy)) {
      newDir = dx > 0 ? 'right' : 'left';
    } else {
      newDir = dy > 0 ? 'down' : 'up';
    }

    // Prevent 180 reversal
    const opposites: Record<Dir, Dir> = { up: 'down', down: 'up', left: 'right', right: 'left' };
    if (newDir && newDir !== opposites[last] && inputQueue.length < 3) {
      inputQueue.push(newDir);
    }
  }

  // =========================================================================
  // Networking
  // =========================================================================

  function broadcastState() {
    if (!mySnake) return;
    sendLobbyMessage(CHANNEL, {
      kind: 'state',
      body: mySnake.body,
      dir: mySnake.dir,
      score: mySnake.score,
      alive: mySnake.alive
    } satisfies SnakePayload);
  }

  function handleLobbyMessage(senderId: string, payload: unknown) {
    const msg = payload as SnakePayload;
    if (!msg || !msg.kind) return;

    switch (msg.kind) {
      case 'join': {
        const s: SnakeState = {
          body: [],
          dir: 'right',
          color: msg.color,
          name: msg.name,
          alive: true,
          score: 0
        };
        remoteSnakes = new Map(remoteSnakes).set(senderId, s);
        lastRemoteUpdate.set(senderId, Date.now());
        // Sync food to new joiner
        if (food.length > 0) {
          sendLobbyMessage(CHANNEL, { kind: 'food_sync', food } satisfies SnakePayload);
        }
        break;
      }
      case 'state': {
        const existing = remoteSnakes.get(senderId);
        const updated: SnakeState = {
          body: msg.body,
          dir: msg.dir,
          color: existing?.color ?? PLAYER_COLORS[remoteSnakes.size % PLAYER_COLORS.length],
          name: existing?.name ?? senderId.slice(0, 6),
          alive: msg.alive,
          score: msg.score
        };
        remoteSnakes = new Map(remoteSnakes).set(senderId, updated);
        lastRemoteUpdate.set(senderId, Date.now());
        break;
      }
      case 'leave': {
        const m = new Map(remoteSnakes);
        m.delete(senderId);
        remoteSnakes = m;
        lastRemoteUpdate.delete(senderId);
        break;
      }
      case 'food_sync': {
        if (food.length === 0) {
          food = msg.food;
          nextFoodId = Math.max(...msg.food.map((f) => f.id), nextFoodId) + 1;
        }
        break;
      }
      case 'food_eaten': {
        food = food.map((f) => (f.id === msg.foodId ? msg.newFood : f));
        nextFoodId = Math.max(msg.newFood.id, nextFoodId) + 1;
        break;
      }
    }
  }

  function pruneStale() {
    const now = Date.now();
    let changed = false;
    const m = new Map(remoteSnakes);
    for (const [id, ts] of lastRemoteUpdate) {
      if (now - ts > 5000) {
        m.delete(id);
        lastRemoteUpdate.delete(id);
        changed = true;
      }
    }
    if (changed) remoteSnakes = m;
  }

  // =========================================================================
  // Leaderboard
  // =========================================================================

  function buildLeaderboard(): { name: string; score: number; color: string; alive: boolean }[] {
    const entries: { name: string; score: number; color: string; alive: boolean }[] = [];
    if (mySnake) {
      entries.push({ name: mySnake.name + ' (you)', score: mySnake.score, color: mySnake.color, alive: mySnake.alive });
    }
    for (const [, s] of remoteSnakes) {
      if (s.body.length > 0) {
        entries.push({ name: s.name, score: s.score, color: s.color, alive: s.alive });
      }
    }
    entries.sort((a, b) => b.score - a.score);
    return entries;
  }

  // =========================================================================
  // Rendering
  // =========================================================================

  function renderLoop() {
    render();
    renderFrame = requestAnimationFrame(renderLoop);
  }

  function render() {
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const cs = cellSize;
    if (cs === 0) return;

    const w = GRID_W * cs;
    const h = GRID_H * cs;

    // Background
    ctx.fillStyle = cssVar('--surface-900', '#0a0806');
    ctx.fillRect(0, 0, w, h);

    // Grid lines (very subtle)
    ctx.strokeStyle = cssVar('--tint-subtle', 'rgba(160, 128, 96, 0.06)');
    ctx.lineWidth = 1;
    for (let x = 0; x <= GRID_W; x++) {
      ctx.beginPath();
      ctx.moveTo(x * cs + 0.5, 0);
      ctx.lineTo(x * cs + 0.5, h);
      ctx.stroke();
    }
    for (let y = 0; y <= GRID_H; y++) {
      ctx.beginPath();
      ctx.moveTo(0, y * cs + 0.5);
      ctx.lineTo(w, y * cs + 0.5);
      ctx.stroke();
    }

    // Food
    for (const f of food) {
      ctx.fillStyle = FOOD_COLOR;
      ctx.shadowColor = FOOD_COLOR;
      ctx.shadowBlur = 8;
      ctx.fillRect(f.pos[0] * cs + 1, f.pos[1] * cs + 1, cs - 2, cs - 2);
    }
    ctx.shadowBlur = 0;

    // Remote snakes
    for (const [, s] of remoteSnakes) {
      if (s.body.length === 0) continue;
      drawSnake(ctx, s, cs);
    }

    // My snake
    if (mySnake) {
      drawSnake(ctx, mySnake, cs);
    }

    // Death overlay
    if (mySnake && !mySnake.alive) {
      ctx.fillStyle = cssVar('--backdrop', 'rgba(10, 8, 6, 0.65)');
      ctx.fillRect(0, 0, w, h);

      const redColor = getFoodColor();
      ctx.fillStyle = redColor;
      ctx.shadowColor = redColor;
      ctx.shadowBlur = 20;
      ctx.font = `bold ${Math.max(14, cs)}px "SF Mono", Consolas, monospace`;
      ctx.textAlign = 'center';
      ctx.fillText('GAME OVER', w / 2, h / 2 - cs);
      ctx.shadowBlur = 0;

      ctx.fillStyle = cssVar('--text-secondary', '#a08060');
      ctx.font = `${Math.max(10, cs * 0.7)}px "SF Mono", Consolas, monospace`;
      ctx.fillText(`Score: ${mySnake.score}`, w / 2, h / 2 + 4);
      ctx.fillText('Press SPACE to respawn', w / 2, h / 2 + cs + 8);
    }

    // Scanline overlay
    ctx.fillStyle = cssVar('--ambient-tint', 'rgba(0, 0, 0, 0.03)');
    for (let y = 0; y < h; y += 4) {
      ctx.fillRect(0, y, w, 2);
    }
  }

  function drawSnake(ctx: CanvasRenderingContext2D, s: SnakeState, cs: number) {
    const alpha = s.alive ? 1 : 0.3;
    for (let i = s.body.length - 1; i >= 0; i--) {
      const [sx, sy] = s.body[i];
      const t = i / Math.max(s.body.length - 1, 1);
      const fade = 1 - t * 0.5; // Tail fades to 50%
      if (i === 0) {
        // Head — brighter, with glow
        ctx.fillStyle = s.color;
        ctx.globalAlpha = alpha;
        ctx.shadowColor = s.color;
        ctx.shadowBlur = s.alive ? 10 : 0;
        ctx.fillRect(sx * cs, sy * cs, cs, cs);
        ctx.shadowBlur = 0;
      } else {
        ctx.fillStyle = s.color;
        ctx.globalAlpha = alpha * fade;
        ctx.fillRect(sx * cs + 1, sy * cs + 1, cs - 2, cs - 2);
      }
    }
    ctx.globalAlpha = 1;

    // Name above head
    if (s.body.length > 0) {
      const [hx, hy] = s.body[0];
      ctx.fillStyle = s.color;
      ctx.globalAlpha = s.alive ? 0.8 : 0.3;
      ctx.font = `bold ${Math.max(8, cs * 0.55)}px "SF Mono", Consolas, monospace`;
      ctx.textAlign = 'center';
      ctx.fillText(s.name, hx * cs + cs / 2, hy * cs - 3);
      ctx.globalAlpha = 1;
    }
  }
</script>

<div class="snake-wrapper" bind:this={wrapper}>
  {#if !gameRunning}
    <div class="lobby">
      <div class="lobby-title">SNAKE</div>
      <div class="lobby-sub">Multiplayer. All clients connected to this server.</div>
      <button class="play-btn" onclick={joinGame} disabled={!isConnected()}>
        {isConnected() ? 'PLAY' : 'CONNECTING...'}
      </button>
      <div class="controls-hint">
        <span>WASD / Arrows</span>
        <span>ESC to quit</span>
      </div>
      {#if onexit}
        <button class="back-btn" onclick={onexit}>Back</button>
      {/if}
    </div>
  {:else}
    <div
      class="game-area"
      ontouchstart={onTouchStart}
      ontouchend={onTouchEnd}
      role="application"
      aria-label="Snake game"
    >
      <canvas bind:this={canvas}></canvas>
      {#if leaderboard.length > 0}
        <div class="scoreboard">
          {#each leaderboard as entry}
            <div class="score-row" class:dead={!entry.alive}>
              <span class="score-dot" style="background: {entry.color}"></span>
              <span class="score-name">{entry.name}</span>
              <span class="score-val">{entry.score}</span>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .snake-wrapper {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    user-select: none;
    -webkit-user-select: none;
  }

  /* ---- Lobby ---- */

  .lobby {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
  }

  .lobby-title {
    font-size: 32px;
    font-weight: 800;
    letter-spacing: 0.3em;
    color: var(--chrome-accent-400);
  }

  .lobby-sub {
    font-size: 11px;
    letter-spacing: 0.08em;
    color: var(--text-muted);
  }

  .play-btn {
    margin-top: 8px;
    padding: 12px 48px;
    background: linear-gradient(180deg, var(--tint-focus) 0%, var(--tint-hover) 100%);
    border: 1px solid var(--chrome-accent-600);
    border-radius: 4px;
    color: var(--chrome-accent-400);
    font-family: inherit;
    font-size: 14px;
    font-weight: 700;
    letter-spacing: 0.15em;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .play-btn:hover:not(:disabled) {
    background: linear-gradient(180deg, var(--tint-selection) 0%, var(--tint-active-strong) 100%);
  }

  .play-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .controls-hint {
    display: flex;
    gap: 20px;
    font-size: 10px;
    letter-spacing: 0.1em;
    color: var(--text-muted);
    text-transform: uppercase;
  }

  .back-btn {
    margin-top: 4px;
    padding: 6px 20px;
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-muted);
    font-family: inherit;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .back-btn:hover {
    border-color: var(--text-secondary);
    color: var(--text-secondary);
  }

  /* ---- Game area ---- */

  .game-area {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    touch-action: none;
  }

  canvas {
    display: block;
    image-rendering: pixelated;
    border: 1px solid var(--surface-border);
    border-radius: 2px;
    box-shadow: var(--depth-down);
  }

  /* ---- Scoreboard ---- */

  .scoreboard {
    position: absolute;
    top: 12px;
    right: 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 120px;
    padding: 8px 10px;
    background: var(--backdrop);
    border: 1px solid var(--surface-border);
    border-radius: 2px;
    font-family: 'SF Mono', Consolas, monospace;
  }

  .score-row {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10px;
    letter-spacing: 0.05em;
    color: var(--text-secondary);
  }

  .score-row.dead {
    opacity: 0.35;
  }

  .score-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .score-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .score-val {
    font-weight: 700;
    color: var(--chrome-accent-400);
    min-width: 16px;
    text-align: right;
  }

  /* ---- Responsive ---- */

  @media (max-width: 639px) {
    .scoreboard {
      top: 8px;
      right: 8px;
      min-width: 90px;
      padding: 6px 8px;
      font-size: 9px;
    }
  }
</style>
