// The real logo, extruded. Plain 2D canvas with a small projection, no libs.
// The two halves fly in and lock, it looks at the mouse, the selection box
// from the viewport hops between them, and clicking knocks them apart.
(() => {
  const canvas = document.getElementById("logo3d");
  const ctx = canvas.getContext("2d");
  const moving = document.documentElement.classList.contains("motion");

  const UNIT = 0.44;
  const DEPTH = 0.62;
  const CAMERA = 19;
  const FLOOR = -4.5;
  const LIGHT = [0.3, 0.55, -0.78];

  // Outline of one half from the <path> in the page, curves flattened.
  function outline(id) {
    const d = document.getElementById(id).getAttribute("d");
    const points = [];
    let x = 0, y = 0;
    for (const [, cmd, args] of d.matchAll(/([MLQ])([^MLQZ]*)/g)) {
      const n = args.trim().split(/[\s,]+/).map(Number);
      if (cmd === "Q") {
        for (let i = 1; i <= 8; i++) {
          const t = i / 8, u = 1 - t;
          points.push([u * u * x + 2 * u * t * n[0] + t * t * n[2], u * u * y + 2 * u * t * n[1] + t * t * n[3]]);
        }
        x = n[2]; y = n[3];
      } else {
        x = n[0]; y = n[1];
        points.push([x, y]);
      }
    }
    return points.map(([px, py]) => [(px - 11.9) * UNIT, (8.5 - py) * UNIT]);
  }

  function makeHalf(id, dir, spinSign) {
    const pts = outline(id);
    const n = pts.length;
    let area = 0, cx = 0, cy = 0;
    pts.forEach((p, i) => {
      const q = pts[(i + 1) % n];
      area += p[0] * q[1] - q[0] * p[1];
      cx += p[0] / n;
      cy += p[1] / n;
    });
    const walls = pts.map((p, i) => {
      const q = pts[(i + 1) % n];
      let nx = q[1] - p[1], ny = p[0] - q[0];
      if (area < 0) { nx = -nx; ny = -ny; }
      const len = Math.hypot(nx, ny) || 1;
      return { a: p, b: q, n: [nx / len, ny / len, 0] };
    });
    const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
    return {
      pts, walls, dir, spinSign, center: [cx, cy],
      box: [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)],
    };
  }

  const halves = [
    makeHalf("half-a", [-1, -0.43], -1),
    makeHalf("half-b", [1, 0.43], 1),
  ];

  const backOut = (t) => {
    const c = 1.5;
    return 1 + (c + 1) * Math.pow(t - 1, 3) + c * Math.pow(t - 1, 2);
  };
  const clamp01 = (v) => Math.min(1, Math.max(0, v));

  let width = 0, height = 0;
  function fit() {
    const ratio = Math.min(devicePixelRatio || 1, 2);
    width = canvas.clientWidth;
    height = canvas.clientHeight;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  }

  function view(yaw, pitch) {
    const cy = Math.cos(yaw), sy = Math.sin(yaw), cp = Math.cos(pitch), sp = Math.sin(pitch);
    return ([x, y, z]) => {
      const rx = x * cy + z * sy;
      const rz = -x * sy + z * cy;
      return [rx, y * cp - rz * sp, y * sp + rz * cp];
    };
  }
  function project([x, y, z]) {
    const scale = (Math.min(width, height) * 1.16) / (CAMERA + z);
    return [width / 2 + x * scale, height * 0.5 - y * scale];
  }
  function shade(normal) {
    const lit = Math.max(0, normal[0] * LIGHT[0] + normal[1] * LIGHT[1] + normal[2] * LIGHT[2]);
    const b = 0.34 + lit * 0.82;
    return `rgb(${(108 * b) | 0},${(127 * b) | 0},${(219 * b) | 0})`;
  }
  function polygon(points, fill) {
    ctx.beginPath();
    points.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
    ctx.closePath();
    ctx.fillStyle = fill;
    ctx.strokeStyle = fill;
    ctx.lineWidth = 0.7;
    ctx.fill();
    ctx.stroke();
  }

  // How far the halves are apart: 0 is locked, 1 is thrown.
  let clock = 0, enteredAt = 0.1, knockedAt = -9, selected = 1, nextHop = 2.2;
  function apart(now) {
    let d = 0;
    if (moving) {
      const entering = clamp01((now - enteredAt) / 1.15);
      d = (1 - backOut(entering)) * 1.05;
      const t = now - knockedAt;
      if (t < 1.5) {
        if (t < 0.32) d += 1 - Math.pow(1 - t / 0.32, 3);
        else if (t < 0.62) d += 1;
        else d += 1 - backOut(clamp01((t - 0.62) / 0.85));
      }
      d += Math.min(scrollY / innerHeight, 1) * 0.2;
    }
    return d;
  }

  function placer(half, d) {
    const spin = d * 0.5 * half.spinSign;
    const c = Math.cos(spin), s = Math.sin(spin);
    const dx = half.dir[0] * d * 2.3, dy = half.dir[1] * d * 2.3;
    return {
      point: ([x, y]) => {
        const ox = x - half.center[0], oy = y - half.center[1];
        return [half.center[0] + ox * c - oy * s + dx, half.center[1] + ox * s + oy * c + dy];
      },
      turn: ([x, y, z]) => [x * c - y * s, x * s + y * c, z],
    };
  }

  let yawOffset = 0, pitchOffset = 0, wantYaw = 0, wantPitch = 0;

  function draw(now) {
    ctx.clearRect(0, 0, width, height);
    const yaw = -0.34 + (moving ? Math.sin(now * 0.34) * 0.2 : 0) + yawOffset;
    const pitch = 0.2 + pitchOffset;
    const rotate = view(yaw, pitch);

    ctx.strokeStyle = "rgba(255,255,255,.08)";
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let i = -8; i <= 8; i += 2) {
      for (const [a, b] of [[[i, FLOOR, -8], [i, FLOOR, 8]], [[-8, FLOOR, i], [8, FLOOR, i]]]) {
        const p = project(rotate(a)), q = project(rotate(b));
        ctx.moveTo(p[0], p[1]);
        ctx.lineTo(q[0], q[1]);
      }
    }
    ctx.stroke();

    const d = apart(now);
    const order = halves
      .map((half) => ({ half, place: placer(half, d) }))
      .map((o) => ({ ...o, depth: rotate([...o.place.point(o.half.center), 0])[2] }))
      .sort((a, b) => b.depth - a.depth);

    for (const { half, place } of order) {
      // Walls, furthest first, then the face on top of them.
      const walls = [];
      for (const wall of half.walls) {
        const a = place.point(wall.a), b = place.point(wall.b);
        const corners = [[a[0], a[1], -DEPTH], [b[0], b[1], -DEPTH], [b[0], b[1], DEPTH], [a[0], a[1], DEPTH]].map(rotate);
        const normal = place.turn(wall.n);
        const seen = rotate(normal);
        const mid = corners.reduce((s, v) => [s[0] + v[0] / 4, s[1] + v[1] / 4, s[2] + v[2] / 4], [0, 0, 0]);
        if (seen[0] * mid[0] + seen[1] * mid[1] + seen[2] * (mid[2] + CAMERA) >= 0) continue;
        walls.push({ depth: mid[2], pts: corners.map(project), fill: shade(normal) });
      }
      walls.sort((a, b) => b.depth - a.depth);
      walls.forEach((w) => polygon(w.pts, w.fill));
      const face = half.pts.map((p) => {
        const [x, y] = place.point(p);
        return project(rotate([x, y, -DEPTH]));
      });
      polygon(face, shade([0, 0, -1]));
    }

    // Selection box, same blue as the viewport's.
    if (moving && d < 0.4) {
      const half = halves[selected];
      const place = placer(half, d);
      const pad = 0.2;
      const [x0, x1, y0, y1] = half.box;
      const offset = place.point(half.center);
      const ox = offset[0] - half.center[0], oy = offset[1] - half.center[1];
      const c = [];
      for (const z of [-DEPTH - pad, DEPTH + pad]) {
        for (const [x, y] of [[x0 - pad, y0 - pad], [x1 + pad, y0 - pad], [x1 + pad, y1 + pad], [x0 - pad, y1 + pad]]) {
          c.push(project(rotate([x + ox, y + oy, z])));
        }
      }
      ctx.strokeStyle = "#5cb5f5";
      ctx.lineWidth = 2;
      ctx.lineJoin = "round";
      ctx.beginPath();
      for (const [a, b] of [[0, 1], [1, 2], [2, 3], [3, 0], [4, 5], [5, 6], [6, 7], [7, 4], [0, 4], [1, 5], [2, 6], [3, 7]]) {
        ctx.moveTo(c[a][0], c[a][1]);
        ctx.lineTo(c[b][0], c[b][1]);
      }
      ctx.stroke();
    }
  }

  canvas.addEventListener("click", () => {
    if (moving && clock - knockedAt > 1.5) knockedAt = clock;
  });

  if (moving) {
    addEventListener("pointermove", (e) => {
      wantYaw = (e.clientX / innerWidth - 0.5) * 0.7;
      wantPitch = (e.clientY / innerHeight - 0.5) * 0.28;
    }, { passive: true });
  }

  let before = 0, running = false;
  function tick(ms) {
    if (!running) return;
    const dt = Math.min((ms - before) / 1000, 0.05);
    before = ms;
    clock += dt;
    yawOffset += (wantYaw - yawOffset) * Math.min(dt * 5, 1);
    pitchOffset += (wantPitch - pitchOffset) * Math.min(dt * 5, 1);
    if (clock > nextHop) {
      selected = 1 - selected;
      nextHop = clock + 1.6;
    }
    draw(clock);
    requestAnimationFrame(tick);
  }
  function run(on) {
    if (on === running) return;
    running = on;
    if (on) {
      before = performance.now();
      requestAnimationFrame(tick);
    }
  }

  new ResizeObserver(() => {
    fit();
    if (!running) draw(clock);
  }).observe(canvas);
  fit();
  if (moving) {
    new IntersectionObserver(([e]) => run(e.isIntersecting && !document.hidden)).observe(canvas);
    document.addEventListener("visibilitychange", () => run(!document.hidden && canvas.getBoundingClientRect().bottom > 0));
  } else {
    draw(0);
  }
})();
