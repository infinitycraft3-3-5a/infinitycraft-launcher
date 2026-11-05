<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { AuditResult } from "../types";

  interface Props {
    audit: AuditResult | null;
    onRefresh: (a: AuditResult) => void;
  }
  let { audit, onRefresh }: Props = $props();

  let checking = $state(false);
  let err = $state<string | null>(null);

  async function runCheck() {
    checking = true;
    err = null;
    try {
      const a = await invoke<AuditResult>("deep_verify");
      onRefresh(a);
    } catch (e) {
      err = String(e);
    } finally {
      checking = false;
    }
  }
</script>

<section class="panel">
  <h2>Intégrité des fichiers</h2>
  <p class="hint">Recalcule le SHA-256 de chaque fichier installé et le compare au manifeste du serveur.</p>

  {#if audit}
    <div class="grid">
      <div class="stat ok"><b>{audit.ok}</b><span>OK</span></div>
      <div class="stat bad"><b>{audit.bad}</b><span>À réparer</span></div>
      <div class="stat miss"><b>{audit.missing}</b><span>Mancants</span></div>
      <div class="stat">
        <b>{(audit.to_download_bytes / 1048576).toFixed(1)} Mo</b><span>à télécharger</span>
      </div>
    </div>
  {/if}

  {#if err}<p class="err">{err}</p>{/if}

  <div class="actions">
    <button class="btn-primary" disabled={checking} onclick={runCheck}>
      {#if checking}Vérification…{:else}Vérifier maintenant{/if}
    </button>
  </div>
</section>

<style>
  .panel { max-width: 560px; margin: 0 auto; background: #12161f; border: 1px solid #1f2536; border-radius: 12px; padding: 28px; }
  h2 { margin: 0 0 4px; color: #cdd6ff; font-size: 18px; }
  .hint { color: #6b7188; font-size: 12px; margin: 0 0 22px; }

  .grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 12px; margin-bottom: 18px; }
  .stat { background: #0f1320; border: 1px solid #232a44; border-radius: 8px; padding: 14px; text-align: center; }
  .stat b { display: block; font-size: 22px; color: #e6e8ee; }
  .stat span { font-size: 11px; color: #6b7188; }
  .stat.ok b { color: #37d67a; }
  .stat.bad b { color: #e0b040; }
  .stat.miss b { color: #ff6b7a; }

  .err { color: #ff6b7a; font-size: 13px; margin: 12px 0; }
  .actions { margin-top: 18px; }
  .btn-primary { padding: 12px 22px; background: linear-gradient(180deg,#3a5bd9,#2d47a8); border: none; color: #fff; font-weight: 600; border-radius: 8px; cursor: pointer; font-size: 14px; }
  .btn-primary:hover:not(:disabled) { filter: brightness(1.15); }
  .btn-primary:disabled { opacity: 0.5; cursor: default; }
</style>
