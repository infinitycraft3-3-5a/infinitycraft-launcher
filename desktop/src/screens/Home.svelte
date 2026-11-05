<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount, onDestroy } from "svelte";
  import type { CheckResult, ProgressEvent, ServerStatus, Settings } from "../types";

  interface Props {
    check: CheckResult | null;
    phase: string;
    setPhase: (p: string) => void;
    onRefresh: (c: CheckResult) => void;
    onError: (m: string) => void;
  }
  let { check, phase, setPhase, onRefresh, onError }: Props = $props();

  // ── Réglages ──
  let showSettings = $state(false);
  let installDir = $state("");
  let saved = $state(false);
  let settingsErr = $state("");

  async function openSettings() {
    settingsErr = "";
    saved = false;
    try {
      const s = await invoke<Settings>("get_settings");
      installDir = s.install_dir;
    } catch (e) {
      settingsErr = String(e);
    }
    showSettings = true;
  }

  function closeSettings() {
    showSettings = false;
    saved = false;
    settingsErr = "";
  }

  async function pickDir() {
    const d = await open({ directory: true, multiple: false, title: "Dossier d'installation du client" });
    if (typeof d === "string") installDir = d;
  }

  async function saveSettings() {
    settingsErr = "";
    saved = false;
    try {
      const cur = await invoke<Settings>("get_settings");
      await invoke("save_settings", { args: { ...cur, install_dir: installDir } });
      await refetchCheck();
      saved = true;
      setTimeout(closeSettings, 700);
    } catch (e) {
      settingsErr = String(e);
    }
  }

  let progress = $state<ProgressEvent | null>(null);
  let showDone = $state(false);
  let doneTimer: ReturnType<typeof setTimeout> | undefined;
  let serverStatus = $state<ServerStatus | null>(null);
  let busy = $derived(
    phase === "checking" ||
    phase === "updating" || phase === "repairing" || phase === "launching"
  );

  function flashDone() {
    showDone = true;
    if (doneTimer) clearTimeout(doneTimer);
    doneTimer = setTimeout(() => { showDone = false; }, 3000);
  }

  let unlisten: (() => void) | null = null;
  let serverTimer: ReturnType<typeof setInterval> | undefined;
  onMount(async () => {
    unlisten = await listen<ProgressEvent>("update-progress", (e) => {
      progress = e.payload;
    });
    async function pollServer() {
      try {
        serverStatus = await invoke<ServerStatus>("server_status");
      } catch (e) {
        serverStatus = { online: false, host: "—", port: 0, latency_ms: null, error: String(e) };
      }
    }
    await pollServer();
    serverTimer = setInterval(pollServer, 15000);
  });
  onDestroy(() => {
    if (unlisten) unlisten();
    if (serverTimer) clearInterval(serverTimer);
  });

  // ── Snow ──
  let snowCanvas: HTMLCanvasElement | undefined = $state();
  let raf = 0;
  type Flake = { x: number; y: number; r: number; vy: number; vx: number; tw: number };
  let flakes: Flake[] = [];

  function seedFlakes(w: number, h: number) {
    const n = Math.max(40, Math.round((w * h) / 11000));
    flakes = Array.from({ length: n }, () => ({
      x: Math.random() * w,
      y: Math.random() * h,
      r: 0.6 + Math.random() * 2.4,
      vy: 0.3 + Math.random() * 0.9,
      vx: -0.25 + Math.random() * 0.5,
      tw: Math.random() * Math.PI * 2,
    }));
  }

  function sizeCanvas() {
    const c = snowCanvas;
    if (!c) return;
    const rect = c.getBoundingClientRect();
    c.width = Math.max(1, Math.round(rect.width));
    c.height = Math.max(1, Math.round(rect.height));
    seedFlakes(c.width, c.height);
  }

  function startSnow() {
    const c = snowCanvas;
    if (!c) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    sizeCanvas();
    let last = performance.now();
    const loop = (t: number) => {
      const dt = Math.min(2.5, (t - last) / 16.7);
      last = t;
      const w = c.width, h = c.height;
      ctx.clearRect(0, 0, w, h);
      for (const f of flakes) {
        f.tw += 0.02 * dt;
        f.y += f.vy * dt;
        f.x += (f.vx + Math.sin(f.tw) * 0.25) * dt;
        if (f.y > h + 4) { f.y = -4; f.x = Math.random() * w; }
        if (f.x < -6) f.x = w + 4;
        if (f.x > w + 6) f.x = -4;
        const a = 0.35 + Math.abs(Math.sin(f.tw)) * 0.55;
        ctx.beginPath();
        ctx.fillStyle = `rgba(220,232,255,${a})`;
        ctx.arc(f.x, f.y, f.r, 0, Math.PI * 2);
        ctx.fill();
      }
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
  }

  onMount(() => {
    startSnow();
    window.addEventListener("resize", sizeCanvas);
  });
  onDestroy(() => {
    cancelAnimationFrame(raf);
    window.removeEventListener("resize", sizeCanvas);
  });

  function fmtBytes(n: number) {
    if (!n) return "0 B";
    const u = ["B", "Ko", "Mo", "Go"];
    const i = Math.min(u.length - 1, Math.floor(Math.log(n) / Math.log(1024)));
    return (n / Math.pow(1024, i)).toFixed(i ? 1 : 0) + " " + u[i];
  }

  function fmtSpeed(bps: number) {
    if (!bps) return "—";
    if (bps < 1024) return bps + " B/s";
    if (bps < 1024 * 1024) return (bps / 1024).toFixed(1) + " Ko/s";
    return (bps / (1024 * 1024)).toFixed(1) + " Mo/s";
  }

  let isPaused = $derived(progress?.paused === true);

  async function togglePause() {
    try {
      if (isPaused) await invoke("resume_download");
      else await invoke("pause_download");
    } catch (e) {
      onError(String(e));
    }
  }

  async function refetchCheck() {
    const c = await invoke<CheckResult>("check_state");
    progress = null;
    onRefresh(c);
  }

  async function primaryAction() {
    try {
      if (phase === "ready") {
        await invoke("launch_game");
      } else {
        progress = null;
        showDone = false;
        setPhase("updating");
        await invoke("install_or_update", {});
        flashDone();
        await refetchCheck();
      }
    } catch (e) {
      onError(String(e));
    }
  }

  async function repair() {
    try {
      progress = null;
      showDone = false;
      setPhase("repairing");
      await invoke("repair_client", {});
      flashDone();
      await refetchCheck();
    } catch (e) {
      onError(String(e));
    }
  }

  let label = $derived(
    phase === "ready" ? "Jouer"
    : phase === "launching" ? "Lancement…"
    : check && !check.up_to_date ? "Installer / Mettre à jour"
    : "Installer");
</script>

<section class="hero">
  <div class="hero-bg" aria-hidden="true"></div>
  <div class="hero-frost" aria-hidden="true"></div>
  <canvas bind:this={snowCanvas} class="snow"></canvas>

  <div class="hero__inner">
    <img class="hero__logo" src="/LOGO-InfinityCraft.png" alt="InfinityCraft"/>

    <h1 class="hero__title">The realm that never sleeps.</h1>

    <p class="hero__lede">
      InfinityCraft is a WoW private server, tuned to feel like the classic
      <span class="mono">3.3.5a</span> days with some custom content to update whats already good. No lag, no bots.
    </p>

  <div class="status">
    {#if phase === "checking" || phase === "boot"}
      <span class="dot check"></span> Vérification du client…
    {:else if phase === "install"}
      <span class="dot check"></span>
      Client non installé · {fmtBytes(check?.to_download_bytes ?? 0)} à télécharger
    {:else if phase === "updating" || phase === "repairing"}
         <span class="dot check"></span>
         {#if isPaused}
           En pause · {fmtBytes(progress?.bytes_global ?? 0)} / {fmtBytes(progress?.bytes_total ?? check?.to_download_bytes ?? 0)}
         {:else if progress?.phase === "mpqs"}
           Téléchargement des MPQs…
         {:else}
           Téléchargement…
         {/if}
      {:else if phase === "launching"}
        <span class="dot ok"></span> Lancement d'InfinityCraft…
      {:else if phase === "error"}
        <span class="dot off"></span> Vérification impossible
      {:else if check}
        <span class="dot ok"></span>
        {#if check.up_to_date}
          Client prêt à jouer
        {:else}
          {check.missing_mpqs.length} MPQ(s) manquants · {fmtBytes(check.to_download_bytes)} à télécharger
        {/if}
      {/if}
    </div>

    {#if serverStatus}
      <div class="status" class:server-off={!serverStatus.online}
        title={(serverStatus.online ? "" : (serverStatus.error ?? "")) + " (" + serverStatus.host + ":" + serverStatus.port + ")"}>
        {#if serverStatus.online}
          <span class="dot ok"></span> Serveur en ligne{#if serverStatus.latency_ms !== null} · {serverStatus.latency_ms} ms{/if}
        {:else}
          <span class="dot off"></span> Serveur hors ligne
        {/if}
      </div>
    {/if}

    <div class="panel-status">
      {#if showDone}
        <p class="done">Install terminé</p>
      {/if}
      {#if phase === "updating" || phase === "repairing"}
        <div class="progress-wrap">
          {#if progress}
            <div class="progress-track">
              <div class="progress-fill" style="width: {progress.percent}%"></div>
            </div>
            <div class="progress-meta">
              <span>#{progress.file_index + 1}/{progress.file_count} · {progress.file.split("/").slice(-1)[0]}</span>
              <span class="meta-right">
                <span>{fmtBytes(progress.bytes_global)} / {fmtBytes(progress.bytes_total)}</span>
                <span>{progress.percent}%</span>
                {#if !isPaused}
                  <span class="speed">▲ {fmtSpeed(progress.speed_bps)}</span>
                {/if}
              </span>
            </div>
            <div class="progress-actions">
              <button class="pause-btn" onclick={togglePause}>
                {#if isPaused}
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg> Reprendre
                {:else}
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M6 4h4v16H6zM14 4h4v16h-4z"/></svg> Pause
                {/if}
              </button>
            </div>
          {:else}
            <div class="progress-meta">Préparation du téléchargement…</div>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  <div class="hero__bottom">
    <div class="bottom-left">
      <button class="wow-btn" disabled={busy} onclick={primaryAction}>
        {#if phase === "ready"}
          <svg class="ico" viewBox="0 0 24 24" width="18" height="18"><path d="M6 4l14 8-14 8z" fill="currentColor"/></svg>
        {:else if phase !== "checking" && phase !== "boot"}
          <svg class="ico" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 3v13M6 11l6 6 6-6M4 21h16"/></svg>
        {/if}
        {label}
      </button>
      <button class="ghost-btn" disabled={busy || phase === "launching"} onclick={repair}>Réparer</button>
    </div>

    <div class="bottom-right">
      {#if check?.up_to_date}
        <span class="status-ico ok" title="Client à jour">
          <svg viewBox="0 0 24 24" width="22" height="22">
            <circle cx="12" cy="12" r="11" fill="currentColor" opacity=".12"/>
            <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-width="1.8"/>
            <path d="M7.5 12.3l2.8 2.8L16.5 9.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
          </svg>
        </span>
      {:else if check}
        <span class="status-ico up" title="Mise à jour disponible · Build {check.server_build}">
          <svg viewBox="0 0 24 24" width="22" height="22">
            <circle cx="12" cy="12" r="11" fill="currentColor" opacity=".12"/>
            <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-width="1.8"/>
            <path d="M16.8 9.2a5.5 5.5 0 1 0 .8 6.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>
            <path d="M19.8 11.4V7h-4.4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
          </svg>
        </span>
      {/if}
      <button class="gear-btn" class-active={showSettings} onclick={openSettings} title="Réglages">
        <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="3"/>
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
        </svg>
      </button>
    </div>

  </section>

  {#if showSettings}
    <div class="settings-overlay" onclick={closeSettings}>
      <div class="settings-panel" onclick={(e) => e.stopPropagation()}>
        <div class="settings-head">
          <h3>Réglages</h3>
          <button class="close-x" onclick={closeSettings} aria-label="Fermer">✕</button>
        </div>

        <div class="field">
          <label>Dossier d'installation du client</label>
          <div class="field-row">
            <input type="text" bind:value={installDir} spellcheck="false"/>
            <button class="btn-pick" onclick={pickDir}>Parcourir…</button>
          </div>
        </div>

        {#if settingsErr}
          <p class="err">{settingsErr}</p>
        {/if}
        {#if saved}
          <p class="saved-ok">Enregistré ✓</p>
        {/if}

        <div class="settings-actions">
          <button class="ghost-btn" onclick={closeSettings}>Annuler</button>
          <button class="wow-btn small" onclick={saveSettings}>Enregistrer</button>
        </div>
      </div>
    </div>
  {/if}

<style>
  .hero {
    position: relative; overflow: hidden;
    height: 100%; min-height: 520px;
    display: flex; flex-direction: column; justify-content: flex-end;
    background: #0c1017;
  }
  .hero-bg {
    position: absolute; inset: 0; z-index: 0;
    background-image: url("/hero.webp");
    background-size: cover; background-position: center 38%;
  }
  .hero-frost {
    position: absolute; inset: 0; z-index: 0;
    background:
      radial-gradient(ellipse 90% 60% at 50% 22%, rgba(59,127,212,.30), transparent 62%),
      radial-gradient(ellipse 60% 50% at 82% 88%, rgba(242,193,78,.10), transparent 60%),
      linear-gradient(180deg, rgba(10,14,21,.45) 0%, rgba(10,14,21,.30) 45%, rgba(10,14,21,.86) 100%);
  }

  .hero__inner {
    position: relative; z-index: 2;
    text-align: center; margin: 0 auto;
    padding: 14% 24px 18px; max-width: 720px;
    display: flex; flex-direction: column; align-items: center; gap: 14px;
  }
  .snow { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; z-index: 1; }
  .hero__logo { height: 90px; width: auto; filter: drop-shadow(0 8px 34px rgba(0,0,0,.55)); margin-bottom: 8px; }
  .hero__title {
    font-family: "Cinzel", "Palatino Linotype", "Book Antiqua", Palatino, Georgia, serif;
    font-size: clamp(2.2rem, 5.5vw, 4rem); line-height: 1.05; letter-spacing: 2px;
    background: linear-gradient(180deg, #f7f9fc 0%, #dce2ea 45%, #a4adba 100%);
    -webkit-background-clip: text; background-clip: text; color: transparent; margin: 0;
  }
  .hero__lede { text-align: center; color: #b4bdc9; margin: 0; font-size: 1.1rem; line-height: 1.55; }
  .mono { font-family: ui-monospace, Consolas, monospace; color: #6fb3ff; }

  .status {
    display: inline-flex; align-items: center; gap: 8px;
    padding: 6px 14px; border-radius: 999px;
    background: rgba(74,222,128,.06); border: 1px solid rgba(74,222,128,.22);
    color: #86efac; font-size: .88rem; font-weight: 500;
    margin-top: 2px;
  }
  .status.server-off {
    background: rgba(239,68,68,.07); border-color: rgba(239,68,68,.28);
    color: #fca5a5;
  }
  .dot { width: 9px; height: 9px; border-radius: 50%; }
  .dot.ok { background: #4ade80; box-shadow: 0 0 10px #4ade80; animation: pulse 2s ease-in-out infinite; }
  .dot.check { background: #f2c14e; box-shadow: 0 0 10px #f2c14e; animation: pulse 1.1s ease-in-out infinite; }
  .dot.off { background: #ef4444; box-shadow: 0 0 10px #ef4444; }
  @keyframes pulse { 50% { opacity: .45; } }

  .panel-status { width: 100%; margin-top: 4px; }
  .done {
    color: #37d67a; font-size: 13px; margin: 0 0 8px;
    animation: doneFade 3s ease-in-out forwards;
  }
  @keyframes doneFade {
    0% { opacity: 0; transform: translateY(4px); }
    12% { opacity: 1; transform: translateY(0); }
    85% { opacity: 1; }
    100% { opacity: 0; }
  }

  .progress-wrap { margin: 12px auto; max-width: 480px; }
  .progress-track { height: 8px; background: #1a1f2e; border: 1px solid #232a44; border-radius: 4px; overflow: hidden; }
  .progress-fill { height: 100%; background: linear-gradient(90deg, #3a5bd9, #6b8cff); transition: width 0.2s; }
  .progress-meta { display: flex; justify-content: space-between; font-size: 11px; color: #6b7188; margin-top: 6px; gap: 12px; }
  .progress-meta span:first-child { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 60%; }
  .progress-meta .meta-right { display: inline-flex; gap: 8px; align-items: baseline; margin-left: auto; }
  .progress-meta .speed { color: #86efac; font-family: ui-monospace, Consolas, monospace; }
  .progress-actions { display: flex; justify-content: flex-end; margin-top: 8px; }
  .pause-btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 5px 12px; font-size: 11px;
    background: rgba(255,255,255,.04); border: 1px solid #2c3644;
    color: #aab2bf; border-radius: 6px; cursor: pointer; transition: all .13s;
  }
  .pause-btn:hover { color: #e6e8ee; border-color: #5568c9; background: rgba(107,140,255,.08); }

  .hero__bottom {
    position: relative; z-index: 2;
    display: flex; align-items: flex-end; justify-content: space-between; gap: 16px;
    padding: 20px 80px 44px 80px; margin-top: auto;
  }
  .bottom-left { display: flex; align-items: center; gap: 12px; }

  .wow-btn {
    display: inline-flex; align-items: center; gap: 11px;
    padding: 14px 30px;
    background: linear-gradient(180deg, #2f47b0 0%, #243a86 48%, #1b2c66 100%);
    border: 1px solid #5568c9; color: #fff; font-size: 17px; font-weight: 700;
    letter-spacing: .4px; border-radius: 8px; cursor: pointer;
    box-shadow: 0 8px 26px rgba(40,63,150,.45), inset 0 1px 0 rgba(255,255,255,.25);
    transition: filter .15s, transform .05s;
  }
  .wow-btn:hover:not(:disabled) { filter: brightness(1.16); }
  .wow-btn:active:not(:disabled) { transform: translateY(1px); }
  .wow-btn:disabled { opacity: .55; cursor: default; }
  .wow-btn .ico { color: #ffe9a8; }

  .ghost-btn {
    padding: 14px 20px; background: rgba(255,255,255,.04); border: 1px solid #2c3644;
    color: #aab2bf; border-radius: 8px; cursor: pointer; font-size: 14px; transition: all .13s;
  }
  .ghost-btn:hover:not(:disabled) { color: #e6e8ee; border-color: #5568c9; background: rgba(107,140,255,.08); }
  .ghost-btn:disabled { opacity: .45; cursor: default; }

  .bottom-right { display: flex; align-items: center; gap: 9px; font-size: 12px; color: #7d8794; font-family: ui-monospace, Consolas, monospace; }
  .bottom-right .status-ico { display: inline-flex; align-items: center; justify-content: center; width: 26px; height: 26px; border-radius: 50%; }
  .bottom-right .status-ico.ok { color: #4ade80; }
  .bottom-right .status-ico.up { color: #f2c14e; animation: pulse 2s ease-in-out infinite; }

  .gear-btn {
    display: inline-flex; align-items: center; justify-content: center;
    width: 30px; height: 30px; margin-left: 6px;
    background: rgba(255,255,255,.04); border: 1px solid #2c3644;
    color: #aab2bf; border-radius: 7px; cursor: pointer; transition: all .13s;
  }
  .gear-btn:hover, .gear-btn:active { color: #e6e8ee; border-color: #5568c9; background: rgba(107,140,255,.10); }
  .gear-btn:active svg { transform: rotate(60deg); }
  .gear-btn svg { transition: transform .5s; }

  .settings-overlay {
    position: absolute; inset: 0; z-index: 10;
    background: rgba(5,8,14,.72); backdrop-filter: blur(3px);
    display: flex; align-items: center; justify-content: center;
    animation: fadeIn .15s ease-out;
  }
  @keyframes fadeIn { from { opacity: 0; } }

  .settings-panel {
    width: min(460px, 92%);
    background: #10141f; border: 1px solid #2c3644; border-radius: 12px;
    padding: 20px 22px;
    box-shadow: 0 20px 60px rgba(0,0,0,.6);
    animation: pop .16s ease-out;
  }
  @keyframes pop { from { transform: scale(.96); opacity: 0; } }

  .settings-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 14px; }
  .settings-head h3 { margin: 0; font-size: 17px; color: #e6e8ee; letter-spacing: .4px; }
  .close-x {
    background: none; border: none; color: #7d8794; font-size: 15px; cursor: pointer; padding: 2px 6px; border-radius: 5px;
  }
  .close-x:hover { color: #e6e8ee; background: rgba(255,255,255,.06); }

  .field { margin-bottom: 14px; }
  .field label { display: block; font-size: 12px; color: #8f98a8; margin-bottom: 6px; font-weight: 500; letter-spacing: .3px; }
  .field input[type="text"], .field input[type="password"] {
    width: 100%; box-sizing: border-box;
    background: #0c1017; border: 1px solid #2c3644; color: #e6e8ee;
    padding: 9px 11px; border-radius: 7px; font-size: 13px; font-family: ui-monospace, Consolas, monospace;
  }
  .field input:focus { outline: none; border-color: #5568c9; box-shadow: 0 0 0 2px rgba(85,104,201,.25); }
  .field-row { display: flex; gap: 8px; }
  .field-row input { flex: 1; }
  .btn-pick {
    padding: 9px 14px; white-space: nowrap;
    background: rgba(255,255,255,.04); border: 1px solid #2c3644;
    color: #aab2bf; border-radius: 7px; cursor: pointer; font-size: 13px; transition: all .13s;
  }
  .btn-pick:hover { color: #e6e8ee; border-color: #5568c9; background: rgba(107,140,255,.10); }
  .hint { font-size: 11px; color: #6b7188; margin: 6px 0 0; }

  .err { color: #ef4444; font-size: 12px; margin: 10px 0 0; }
  .saved-ok { color: #4ade80; font-size: 12px; margin: 10px 0 0; }

  .settings-actions { display: flex; justify-content: flex-end; gap: 10px; margin-top: 18px; }
  .wow-btn.small { padding: 10px 20px; font-size: 14px; }
</style>
