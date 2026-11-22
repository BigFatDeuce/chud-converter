<script lang="ts">
  // Fond de l'application : nébuleuses, champ d'étoiles et orbites discrètes.
  // Les étoiles sont dessinées une seule fois sur un canevas (et redessinées au changement de
  // taille ou de thème) : aucune boucle d'animation. Le seul mouvement est confié au
  // compositeur (opacité, transformation), coupé par le réglage Animations.
  import { onMount } from "svelte";
  import { ui } from "../../ui.svelte";

  let dim = $state<HTMLCanvasElement>();
  let bright = $state<HTMLCanvasElement>();

  // Générateur pseudo-aléatoire à graine fixe : le ciel ne change pas d'un démarrage à l'autre.
  function rng(seed: number) {
    return () => {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      return seed / 4294967296;
    };
  }

  function draw() {
    if (!dim || !bright) return;
    const css = getComputedStyle(document.documentElement);
    const rgb = css.getPropertyValue("--star-rgb").trim().split(/\s+/).join(",") || "225,232,255";
    const alpha = Number(css.getPropertyValue("--star-alpha")) || 0.8;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const w = window.innerWidth;
    const h = window.innerHeight;
    const area = w * h;

    for (const c of [dim, bright]) {
      c.width = Math.round(w * dpr);
      c.height = Math.round(h * dpr);
    }

    const a = dim.getContext("2d");
    const b = bright.getContext("2d");
    if (!a || !b) return;
    a.setTransform(dpr, 0, 0, dpr, 0, 0);
    b.setTransform(dpr, 0, 0, dpr, 0, 0);

    const r = rng(7);
    // Poussière d'étoiles : nombreuses, petites, pâles.
    const n = Math.round(area / 4200);
    for (let i = 0; i < n; i++) {
      const x = r() * w;
      const y = r() * h;
      const s = 0.35 + r() * r() * 0.9;
      a.fillStyle = `rgba(${rgb},${(alpha * (0.18 + r() * 0.55)).toFixed(3)})`;
      a.beginPath();
      a.arc(x, y, s, 0, Math.PI * 2);
      a.fill();
    }

    // Quelques étoiles brillantes, légèrement teintées, avec un halo.
    const tints = [rgb, "160,200,255", "200,170,255", "130,230,255"];
    const m = Math.max(8, Math.round(area / 52000));
    for (let i = 0; i < m; i++) {
      const x = r() * w;
      const y = r() * h;
      const s = 0.8 + r() * 0.9;
      const t = tints[Math.floor(r() * tints.length)];
      const g = b.createRadialGradient(x, y, 0, x, y, s * 5);
      g.addColorStop(0, `rgba(${t},${(alpha * 0.9).toFixed(3)})`);
      g.addColorStop(0.25, `rgba(${t},${(alpha * 0.25).toFixed(3)})`);
      g.addColorStop(1, `rgba(${t},0)`);
      b.fillStyle = g;
      b.beginPath();
      b.arc(x, y, s * 5, 0, Math.PI * 2);
      b.fill();
    }
  }

  // Redessin au changement de thème (couleur des étoiles).
  $effect(() => {
    void ui.dark;
    void ui.theme;
    requestAnimationFrame(draw);
  });

  onMount(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const resize = () => {
      clearTimeout(timer);
      timer = setTimeout(draw, 160);
    };
    window.addEventListener("resize", resize);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("resize", resize);
    };
  });
</script>

<div class="cosmos" aria-hidden="true">
  <div class="nebula ambient"></div>
  <canvas class="stars" bind:this={dim}></canvas>
  <canvas class="stars twinkle ambient" bind:this={bright}></canvas>
  <svg class="orbits ambient" viewBox="0 0 1000 1000" preserveAspectRatio="xMidYMid slice">
    <g>
      <ellipse cx="500" cy="500" rx="470" ry="170" />
      <ellipse cx="500" cy="500" rx="360" ry="128" transform="rotate(-14 500 500)" />
      <ellipse cx="500" cy="500" rx="250" ry="88" transform="rotate(9 500 500)" />
      <circle class="moon" cx="970" cy="500" r="3.2" />
      <circle class="moon b" cx="250" cy="500" r="2.2" transform="rotate(9 500 500)" />
    </g>
  </svg>
  <svg class="constellation" viewBox="0 0 300 200">
    <path d="M18 160 L70 120 L118 136 L170 82 L236 64 L270 96" />
    <path d="M170 82 L150 30" />
    <circle cx="18" cy="160" r="2" /><circle cx="70" cy="120" r="2.4" /><circle cx="118" cy="136" r="1.8" />
    <circle cx="170" cy="82" r="2.8" /><circle cx="236" cy="64" r="2" /><circle cx="270" cy="96" r="1.6" /><circle cx="150" cy="30" r="1.8" />
  </svg>
  <div class="vignette"></div>
</div>

<style>
  .cosmos {
    position: fixed;
    inset: 0;
    z-index: -1;
    overflow: hidden;
    pointer-events: none;
    background: radial-gradient(120% 90% at 50% 0%, var(--bg) 40%, var(--bg-deep) 100%);
  }

  .nebula {
    position: absolute;
    inset: -25%;
    background:
      radial-gradient(38% 32% at 22% 18%, var(--nebula-1), transparent 70%),
      radial-gradient(34% 30% at 82% 78%, var(--nebula-3), transparent 70%),
      radial-gradient(30% 26% at 72% 22%, var(--nebula-2), transparent 70%),
      radial-gradient(26% 22% at 30% 82%, var(--nebula-2), transparent 70%);
    animation: drift 90s ease-in-out infinite alternate;
    will-change: transform;
  }

  @keyframes drift {
    to {
      transform: translate3d(3%, -2%, 0) scale(1.06) rotate(2deg);
    }
  }

  .stars {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .twinkle {
    animation: twinkle 7s ease-in-out infinite alternate;
  }

  @keyframes twinkle {
    from {
      opacity: 1;
    }
    to {
      opacity: 0.45;
    }
  }

  .orbits {
    position: absolute;
    width: 120vmax;
    height: 120vmax;
    right: -48vmax;
    top: -58vmax;
    opacity: 0.9;
    animation: turn 360s linear infinite;
    will-change: transform;
  }

  @keyframes turn {
    to {
      transform: rotate(360deg);
    }
  }

  .orbits ellipse {
    fill: none;
    stroke: var(--orbit);
    stroke-width: 1.2;
    vector-effect: non-scaling-stroke;
  }

  .moon {
    fill: var(--accent-3);
    opacity: 0.55;
  }

  .moon.b {
    fill: var(--accent-2);
  }

  .constellation {
    position: absolute;
    left: 2%;
    bottom: 3%;
    width: 300px;
    opacity: 0.55;
  }

  .constellation path {
    fill: none;
    stroke: var(--orbit);
    stroke-width: 1;
  }

  .constellation circle {
    fill: rgb(var(--star-rgb) / var(--star-alpha));
  }

  .vignette {
    position: absolute;
    inset: 0;
    background: radial-gradient(140% 120% at 50% 40%, transparent 55%, var(--bg-deep) 100%);
    opacity: 0.6;
  }
</style>
