<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import type { Phase, CheckResult, LauncherUpdate, Settings } from "./types";
  import { normLang, t, type Lang } from "./i18n";
  import Home from "./screens/Home.svelte";

  const win = getCurrentWindow();

  let phase = $state<Phase>("boot");
  let check = $state<CheckResult | null>(null);
  let errorMsg = $state<string | null>(null);
  let lang = $state<Lang>("fr");

  async function changeLang(code: Lang) {
    lang = code;
    try {
      const cur = await invoke<Settings>("get_settings");
      await invoke("save_settings", { args: { ...cur, language: code } });
    } catch {
    }
  }

  let launcherUpdate = $state<LauncherUpdate | null>(null);
  let updatingLauncher = $state(false);

  async function checkLauncherUpdate() {
    try {
      const u = await invoke<LauncherUpdate>("launcher_selfupdate_check");
      if (u.available) {
        launcherUpdate = u;
      }
    } catch {
      launcherUpdate = null;
    }
  }

  async function runSelfUpdate(u: LauncherUpdate) {
    if (updatingLauncher) return;
    updatingLauncher = true;
    launcherUpdate = u;
    try {
      await invoke("launcher_selfupdate_run", { info: u });
      // le process est relancé par le helper ; on ne revient pas ici.
    } catch (e) {
      updatingLauncher = false;
      launcherUpdate = null;
      showError(String(e));
    }
  }

  function refresh(c: CheckResult) {
    check = c;
    phase = c.up_to_date ? "ready" : "install";
  }

  function setPhase(p: Phase) {
    phase = p;
  }

  function showError(msg: string) {
    phase = "error";
    errorMsg = msg;
  }

  async function minimize() {
    await win.minimize();
  }
  async function close() {
    await win.close();
  }

  onMount(async () => {
    try {
      const s = await invoke<Settings>("get_settings");
      lang = normLang(s.language);
    } catch {
    }
    checkLauncherUpdate();
    phase = "checking";
    try {
      const c = await invoke<CheckResult>("check_state");
      refresh(c);
    } catch (e) {
      showError(String(e));
    }
  });
</script>

<div class="winctl">
  <button class="min" title={t(lang, "win_minimize")} aria-label={t(lang, "win_minimize")} onclick={minimize}>
    <svg width="12" height="12" viewBox="0 0 12 12"><path d="M2 6 H10" stroke="currentColor" stroke-width="1.4" fill="none"/></svg>
  </button>
  <button class="close" title={t(lang, "win_close")} aria-label={t(lang, "win_close")} onclick={close}>
    <svg width="12" height="12" viewBox="0 0 12 12"><path d="M2 2 L10 10 M10 2 L2 10" stroke="currentColor" stroke-width="1.4" fill="none"/></svg>
  </button>
</div>

{#if launcherUpdate}
  <div class="updater-banner" role="status" aria-live="polite">
    <div class="updater-text">
      <div class="updater-title">{#if updatingLauncher}{t(lang, "upd_in_progress")}{:else}{t(lang, "upd_available")}{/if}</div>
      <div class="updater-sub">{t(lang, "upd_sub", { v: launcherUpdate.version })}</div>
    </div>
    <div class="updater-actions">
      <button
        class="updater-cta"
        onclick={() => runSelfUpdate(launcherUpdate)}
        disabled={updatingLauncher}>
        {#if updatingLauncher}
          {t(lang, "upd_cta_busy")}
        {:else}
          {t(lang, "upd_cta")}
        {/if}
      </button>
    </div>
  </div>
{/if}

<main class="stage">
  <div class="grip" data-tauri-drag-region aria-hidden="true"></div>
  <Home {check} {phase} {lang} onChangeLang={changeLang} onRefresh={refresh} onError={showError} setPhase={setPhase} />
</main>

{#if phase === "error" && errorMsg}
  <div class="errtoast">{errorMsg}</div>
{/if}

<style>
  :global(html), :global(body) { margin: 0; height: 100vh; overflow: hidden; background: #0c1017; }
  :global(#app) { height: 100vh; }

  .stage { height: 100vh; position: relative; }

  .grip {
    position: fixed; top: 0; left: 0; right: 0; z-index: 900;
    height: 32px; background: transparent; cursor: default;
  }

  .winctl {
    position: fixed; top: 0; right: 0; z-index: 999;
    display: flex; gap: 2px; padding: 4px;
  }
  .winctl button {
    width: 42px; height: 34px; display: grid; place-items: center;
    background: rgba(12,16,23,.4); border: none; color: #c8cee0; cursor: pointer;
    transition: background 0.12s, color 0.12s;
  }
  .winctl .min:hover { background: #1a1e2e; color: #fff; }
  .winctl .close:hover { background: #b13b3b; color: #fff; }

  .errtoast {
    position: fixed; left: 14px; bottom: 14px; z-index: 998;
    background: rgba(60,15,20,.92); border: 1px solid #7a2d34; color: #ffb3b8;
    padding: 9px 15px; border-radius: 8px; font-size: 12.5px; max-width: 72%; line-height: 1.4;
  }

  .updater-banner {
    position: fixed; left: 0; right: 0; top: 0; z-index: 990;
    display: flex; align-items: center; justify-content: space-between; gap: 16px;
    padding: 12px 110px 12px 16px;
    background: linear-gradient(180deg, rgba(107,140,255,.18), rgba(107,140,255,.07));
    border-bottom: 1px solid rgba(107,140,255,.38);
    animation: bannerIn .18s ease-out;
  }
  @keyframes bannerIn { from { opacity: 0; transform: translateY(-6px); } }
  .updater-text { min-width: 0; }
  .updater-title { font-size: 13px; font-weight: 600; color: #dfe6ff; letter-spacing: .2px; }
  .updater-sub { font-size: 11px; color: #9aa4c8; margin-top: 2px; }
  .updater-actions { display: inline-flex; align-items: center; gap: 8px; flex-shrink: 0; }
  .updater-cta {
    padding: 7px 16px; font-size: 12.5px; font-weight: 700; color: #fff;
    background: linear-gradient(180deg, #3a5bd9 0%, #243a86 100%);
    border: 1px solid #6b8cff; border-radius: 6px; cursor: pointer;
  }
  .updater-cta:hover { filter: brightness(1.15); }
  .updater-cta:disabled { opacity: .6; cursor: wait; }
</style>
