<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";

  interface Props {
    onNavigate: () => void;
  }
  let { onNavigate }: Props = $props();

  let install_dir = $state<string>("");
  let server_base = $state<string>("");
  let realmline = $state<string>("");
  let saved = $state(false);
  let errMsg = $state<string | null>(null);

  async function load() {
    try {
      const s = await invoke<Record<string, unknown>>("get_settings");
      install_dir = (s.install_dir as string) ?? "";
      server_base = (s.server_base as string) ?? "";
      realmline = (s.realmline as string) ?? "";
    } catch (e) {
      errMsg = String(e);
    }
  }
  load();

  async function pickDir() {
    const d = await open({ directory: true, title: "Dossier d'installation du client" });
    if (d) install_dir = d as string;
  }

  async function save() {
    saved = false;
    errMsg = null;
    try {
      await invoke("save_install_dir", { dir: install_dir });
      saved = true;
    } catch (e) {
      errMsg = String(e);
    }
  }
</script>

<section class="panel">
  <h2>Réglages</h2>
  <p class="hint">Seul le dossier d'installation est modifiable. Les autres valeurs sont fixées par le serveur.</p>

  <label class="field">
    <span>Chemin d'installation du client</span>
    <div class="row">
      <input type="text" bind:value={install_dir} placeholder="C:\Program Files\InfinityCraft 3.3.5a" />
      <button class="btn-small" onclick={pickDir}>Parcourir…</button>
    </div>
  </label>

  <div class="fixed">
    <div class="fixed__row"><span>Serveur de fichiers</span><code>{server_base || "—"}</code></div>
    <div class="fixed__row"><span>Realm (realmline)</span><code>{realmline || "—"}</code></div>
    <div class="fixed__row"><span>Expension</span><code>3.3.5a · WotLK</code></div>
    <div class="fixed__row"><span>Cache au lancement</span><code>Cache/WDB (WTF / Interface préservés)</code></div>
  </div>

  {#if saved}<p class="ok">Réglages enregistrés.</p>{/if}
  {#if errMsg}<p class="err">{errMsg}</p>{/if}

  <div class="actions">
    <button class="btn-primary" onclick={save}>Enregistrer</button>
    <button class="btn-ghost" onclick={onNavigate}>Retour</button>
  </div>
</section>

<style>
  .panel { max-width: 560px; margin: 0 auto; background: #12161f; border: 1px solid #1f2536; border-radius: 12px; padding: 28px; }
  h2 { margin: 0 0 4px; color: #cdd6ff; font-size: 18px; }
  .hint { color: #6b7188; font-size: 12px; margin: 0 0 22px; }

  .field { display: block; margin-bottom: 18px; font-size: 13px; color: #b8c0d8; }
  .field span { display: block; margin-bottom: 6px; }
  .row { display: flex; gap: 8px; }

  input[type="text"] {
    width: 100%; padding: 9px 11px; background: #0f1320; border: 1px solid #2a3252;
    border-radius: 6px; color: #e6e8ee; font-size: 13px;
  }
  input[type="text"]:focus { outline: none; border-color: #3a5bd9; }
  .btn-small {
    padding: 9px 14px; background: #1e2540; border: 1px solid #2e3a6a; color: #cdd6ff;
    border-radius: 6px; cursor: pointer; white-space: nowrap; font-size: 13px;
  }
  .btn-small:hover { background: #283055; }

  .fixed { border: 1px solid #1f2536; border-radius: 8px; overflow: hidden; margin-bottom: 6px; }
  .fixed__row { display: flex; justify-content: space-between; align-items: center; gap: 12px; padding: 10px 14px; border-bottom: 1px solid #1a1f2e; font-size: 13px; }
  .fixed__row:last-child { border-bottom: none; }
  .fixed__row span { color: #8a90a6; }
  .fixed__row code { font-family: ui-monospace, Consolas, monospace; color: #cdd6ff; font-size: 12px; }

  .ok { color: #37d67a; font-size: 13px; margin: 12px 0; }
  .err { color: #ff6b7a; font-size: 13px; margin: 12px 0; }

  .actions { display: flex; gap: 10px; margin-top: 20px; }
  .btn-primary { padding: 12px 22px; background: linear-gradient(180deg,#3a5bd9,#2d47a8); border: none; color: #fff; font-weight: 600; border-radius: 8px; cursor: pointer; font-size: 14px; }
  .btn-primary:hover { filter: brightness(1.15); }
  .btn-ghost { padding: 12px 18px; background: transparent; border: 1px solid #2e3a6a; color: #8a90a6; border-radius: 8px; cursor: pointer; font-size: 13px; }
  .btn-ghost:hover { color: #cdd6ff; border-color: #4a5aa0; }
</style>
