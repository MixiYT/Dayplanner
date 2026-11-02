/* Анимированные фоны на canvas. Цвета берутся из акцентного цвета.
   Режимы: none, aurora, bubbles, stars, waves, gradient. */
'use strict';

const Background = (() => {
  const canvas = document.getElementById('bg');
  const ctx = canvas.getContext('2d');
  let mode = 'none';
  let intensity = 0.6;
  let accent = '#6750a4';
  let dark = false;
  let raf = 0;
  let w = 0, h = 0, dpr = 1;
  let t0 = performance.now();
  let particles = [];
  const reduce = window.matchMedia('(prefers-reduced-motion: reduce)');

  function hexToRgb(hex) {
    const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
    const n = m ? parseInt(m[1], 16) : 0x6750a4;
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }
  function rgbToHsl([r, g, b]) {
    r /= 255; g /= 255; b /= 255;
    const max = Math.max(r, g, b), min = Math.min(r, g, b);
    let hh = 0, s = 0; const l = (max + min) / 2;
    if (max !== min) {
      const d = max - min;
      s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
      if (max === r) hh = (g - b) / d + (g < b ? 6 : 0);
      else if (max === g) hh = (b - r) / d + 2;
      else hh = (r - g) / d + 4;
      hh *= 60;
    }
    return [hh, s * 100, l * 100];
  }
  /* Три близких оттенка вокруг акцента */
  function palette() {
    const [hu, s] = rgbToHsl(hexToRgb(accent));
    const sat = Math.max(55, Math.min(95, s));
    const l = dark ? 55 : 62;
    return [
      `hsl(${hu}, ${sat}%, ${l}%)`,
      `hsl(${(hu + 40) % 360}, ${sat}%, ${l}%)`,
      `hsl(${(hu + 320) % 360}, ${sat}%, ${l + 4}%)`,
    ];
  }

  function resize() {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    w = window.innerWidth; h = window.innerHeight;
    canvas.width = Math.round(w * dpr); canvas.height = Math.round(h * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    seed();
  }

  function seed() {
    const area = w * h;
    particles = [];
    if (mode === 'bubbles') {
      const n = Math.round(area / 26000 * (0.5 + intensity));
      for (let i = 0; i < n; i++) particles.push({
        x: Math.random() * w, y: Math.random() * h, r: 8 + Math.random() * 46,
        v: 6 + Math.random() * 18, ph: Math.random() * 6.28, c: i % 3,
      });
    } else if (mode === 'stars') {
      const n = Math.round(area / 5200 * (0.4 + intensity));
      for (let i = 0; i < n; i++) particles.push({
        x: Math.random() * w, y: Math.random() * h, r: 0.5 + Math.random() * 1.6,
        ph: Math.random() * 6.28, sp: 0.6 + Math.random() * 1.8, d: 2 + Math.random() * 10,
      });
    }
  }

  function drawAurora(t) {
    const cols = palette();
    ctx.save();
    ctx.globalCompositeOperation = dark ? 'lighter' : 'source-over';
    const a = 0.16 + 0.36 * intensity;
    const blobs = [
      { cx: 0.2, cy: 0.18, r: 0.62, sp: 0.07, c: 0 },
      { cx: 0.85, cy: 0.35, r: 0.55, sp: 0.09, c: 1 },
      { cx: 0.45, cy: 0.88, r: 0.7, sp: 0.06, c: 2 },
      { cx: 0.9, cy: 0.95, r: 0.45, sp: 0.1, c: 0 },
    ];
    blobs.forEach((b, i) => {
      const x = (b.cx + Math.sin(t * b.sp + i * 1.7) * 0.16) * w;
      const y = (b.cy + Math.cos(t * b.sp * 1.2 + i) * 0.13) * h;
      const r = b.r * Math.max(w, h) * (0.85 + 0.15 * Math.sin(t * 0.2 + i));
      const g = ctx.createRadialGradient(x, y, 0, x, y, r);
      g.addColorStop(0, cols[b.c].replace('hsl', 'hsla').replace(')', `, ${a})`));
      g.addColorStop(1, cols[b.c].replace('hsl', 'hsla').replace(')', ', 0)'));
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, w, h);
    });
    ctx.restore();
  }

  function drawBubbles(t, dt) {
    const cols = palette();
    const a = 0.1 + 0.3 * intensity;
    for (const p of particles) {
      p.y -= p.v * dt; p.x += Math.sin(t * 0.5 + p.ph) * 4 * dt;
      if (p.y + p.r < 0) { p.y = h + p.r; p.x = Math.random() * w; }
      const g = ctx.createRadialGradient(p.x - p.r * 0.3, p.y - p.r * 0.3, p.r * 0.1, p.x, p.y, p.r);
      const c = cols[p.c];
      g.addColorStop(0, c.replace('hsl', 'hsla').replace(')', `, ${a * 0.6})`));
      g.addColorStop(1, c.replace('hsl', 'hsla').replace(')', `, ${a * 1.4})`));
      ctx.beginPath(); ctx.arc(p.x, p.y, p.r, 0, 6.283);
      ctx.fillStyle = g; ctx.fill();
      ctx.lineWidth = 1; ctx.strokeStyle = `rgba(255,255,255,${0.16 * intensity + 0.04})`; ctx.stroke();
    }
  }

  function drawStars(t) {
    const cols = palette();
    ctx.fillStyle = cols[0];
    for (const p of particles) {
      const tw = 0.35 + 0.65 * (0.5 + 0.5 * Math.sin(t * p.sp + p.ph));
      const y = (p.y + t * p.d) % h;
      ctx.globalAlpha = Math.min(1, tw * (0.25 + 0.75 * intensity));
      ctx.beginPath(); ctx.arc(p.x, y, p.r, 0, 6.283);
      ctx.fillStyle = p.r > 1.5 ? cols[1] : (dark ? '#fff' : cols[0]);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }

  function drawWaves(t) {
    const cols = palette();
    const layers = 4;
    for (let i = 0; i < layers; i++) {
      const base = h * (0.62 + i * 0.1);
      const amp = 14 + i * 9;
      const k = 0.004 - i * 0.0004;
      ctx.beginPath(); ctx.moveTo(0, h);
      for (let x = 0; x <= w + 8; x += 8) {
        ctx.lineTo(x, base + Math.sin(x * k + t * (0.5 + i * 0.18) + i) * amp + Math.sin(x * k * 2.1 - t * 0.4) * amp * 0.4);
      }
      ctx.lineTo(w, h); ctx.closePath();
      ctx.fillStyle = cols[i % 3].replace('hsl', 'hsla').replace(')', `, ${(0.08 + 0.2 * intensity) * (1 - i * 0.16)})`);
      ctx.fill();
    }
  }

  function drawGradient(t) {
    const [hu, s] = rgbToHsl(hexToRgb(accent));
    const sat = Math.max(50, Math.min(90, s));
    const sh = Math.sin(t * 0.08) * 40;
    const g = ctx.createLinearGradient(w * (0.5 + 0.5 * Math.sin(t * 0.05)), 0, w * (0.5 - 0.5 * Math.cos(t * 0.06)), h);
    const a = 0.1 + 0.38 * intensity;
    g.addColorStop(0, `hsla(${hu + sh}, ${sat}%, ${dark ? 40 : 70}%, ${a})`);
    g.addColorStop(0.5, `hsla(${hu + 50 + sh}, ${sat}%, ${dark ? 35 : 75}%, ${a * 0.8})`);
    g.addColorStop(1, `hsla(${hu - 50 + sh}, ${sat}%, ${dark ? 30 : 68}%, ${a})`);
    ctx.fillStyle = g; ctx.fillRect(0, 0, w, h);
  }

  let last = 0;
  function frame(now) {
    raf = 0;
    const t = (now - t0) / 1000;
    const dt = Math.min(0.1, (now - last) / 1000 || 0.016);
    last = now;
    ctx.clearRect(0, 0, w, h);
    if (mode === 'aurora') drawAurora(t);
    else if (mode === 'bubbles') drawBubbles(t, dt);
    else if (mode === 'stars') drawStars(t);
    else if (mode === 'waves') drawWaves(t);
    else if (mode === 'gradient') drawGradient(t);
    schedule();
  }

  function schedule() {
    if (raf || mode === 'none' || document.hidden) return;
    // Для «меньше движения» рисуем один статичный кадр.
    if (reduce.matches) { raf = 0; frameStatic(); return; }
    raf = requestAnimationFrame(frame);
  }
  function frameStatic() {
    ctx.clearRect(0, 0, w, h);
    const t = 3;
    if (mode === 'aurora') drawAurora(t);
    else if (mode === 'bubbles') drawBubbles(t, 0);
    else if (mode === 'stars') drawStars(t);
    else if (mode === 'waves') drawWaves(t);
    else if (mode === 'gradient') drawGradient(t);
  }

  function restart() {
    if (raf) { cancelAnimationFrame(raf); raf = 0; }
    ctx.clearRect(0, 0, w, h);
    seed();
    schedule();
  }

  window.addEventListener('resize', () => { resize(); restart(); });
  document.addEventListener('visibilitychange', () => { if (!document.hidden) { last = performance.now(); schedule(); } });
  reduce.addEventListener?.('change', restart);
  resize();

  return {
    set(opts) {
      if (opts.mode !== undefined) mode = opts.mode;
      if (opts.intensity !== undefined) intensity = Math.max(0, Math.min(1, opts.intensity));
      if (opts.accent) accent = opts.accent;
      if (opts.dark !== undefined) dark = opts.dark;
      restart();
    },
    modes: [
      ['none', 'Без фона'], ['aurora', 'Северное сияние'], ['bubbles', 'Пузыри'],
      ['stars', 'Звёзды'], ['waves', 'Волны'], ['gradient', 'Переливы'],
    ],
  };
})();
