use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::core::download::{self, ProgressEvent, ProgressSender};
use crate::core::extract;
use crate::core::github;
use crate::core::hash;
use crate::core::recombine;
use crate::core::settings::Settings;
use crate::core::state::Store;
use crate::core::transfer::TransferState;
use crate::core::utils;
use crate::core::version::{self, VersionJson};

// ─── Résultats serialisés vers le frontend ───

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub install_dir: String,
    pub up_to_date: bool,
    pub skeleton_ok: bool,
    pub mpqs_ok: u32,
    pub mpqs_total: u32,
    pub missing_mpqs: Vec<String>,
    pub to_download_bytes: u64,
    pub local_build: Option<u32>,
    pub server_build: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallResult {
    pub installed: bool,
    pub skeleton_downloaded: bool,
    pub mpqs_downloaded: u32,
    pub mpqs_skipped: u32,
    /// MPQs vérifiés par hachage (réparation) et validés sans ré-telechargement.
    pub mpqs_verified: u32,
    pub failed: u32,
}

// ─── Helpers ───

fn dl_dir(install_dir: &Path) -> PathBuf {
    install_dir.join(".dl")
}

/// Détecte un fichier du squelette (manifeste `skeleton.files`) absent ou
/// tronqué dans install_dir. Retourne le nom relatif du premier fautif,
/// ou None si tout est présent à la bonne taille (manifeste vide = None).
fn skeleton_missing_file(install_dir: &Path, files: &[version::SkeletonFile]) -> Option<String> {
    for f in files {
        let p = install_dir.join(f.name.replace('/', "\\"));
        match std::fs::metadata(&p) {
            Ok(md) if md.is_file() && md.len() == f.size => {}
            _ => return Some(f.name.clone()),
        }
    }
    None
}

// ─── helpers téléchargement ──────────────────────────────────────────────────
async fn fetch_manifest(
    settings: &Settings,
    tok: &str,
    client: &reqwest::Client,
) -> Result<VersionJson, String> {
    let custom = settings.version_json_url();
    if !custom.is_empty() {
        return version::fetch(&custom, Some(tok)).await;
    }
    let ref_name = if settings.tag_hint.trim().is_empty() {
        "HEAD".to_string()
    } else {
        settings.tag_hint.trim().to_string()
    };
    github::fetch_version_json(
        client,
        &settings.owner(),
        &settings.repo(),
        &ref_name,
        Some(tok),
    )
    .await
    .map_err(|e| format!("manifest version.json : {e}"))
}

fn asset_tag(settings: &Settings, vj: &VersionJson) -> String {
    let t = if settings.tag_hint.trim().is_empty() {
        vj.release.clone()
    } else {
        settings.tag_hint.trim().to_string()
    };
    let t = t.trim();
    if t.is_empty() {
        "latest".to_string()
    } else {
        t.to_string()
    }
}

fn resource_dir(app: &AppHandle) -> Option<PathBuf> {
    // Résoudre la ressource par son nom (Tauri 2) : fiable même si resource_dir() varie.
    if let Ok(p) = app.path().resolve("7za.exe", tauri::path::BaseDirectory::Resource) {
        return p.parent().map(|d| d.to_path_buf());
    }
    app.path().resource_dir().ok()
}

fn emit_phase(app: &AppHandle, phase: &str, msg: &str) {
    let _ = app.emit("lc://launcher-log", format!("[{phase}] {msg}"));
}

fn make_tx(app: &AppHandle, file_index: usize, file_count: usize) -> ProgressSender {
    let app = app.clone();
    Arc::new(move |ev: ProgressEvent| {
        let _ = app.emit(
            "update-progress",
            ProgressEvent {
                file_index,
                file_count,
                ..ev
            },
        );
    })
}

// ─── check_state ───

#[tauri::command]
pub async fn check_state(state: State<'_, crate::AppState>) -> Result<CheckResult, String> {
    let settings = state.settings();
    let install_dir = settings.install_dir();
    let tok = settings.github_token();

    let client = reqwest::Client::new();
    let vj = fetch_manifest(&settings, &tok, &client).await?;

    let server_build = vj.latest_version;

    let wow_exists = utils::wow_exe(&install_dir).is_file();

    let mut mpqs_ok: u32 = 0;
    let mut missing: Vec<String> = Vec::new();
    let mut to_download: u64 = 0;

    let mut store = Store::new(&install_dir);
    store.load();

    // Si le manifeste porte `files` : la vérité est la présence/taille des
    // fichiers (pas le state, qui est une simple comptabilité). Le state reste
    // pertinent seulement pour les anciens manifests sans liste.
    let skeleton_ok = if vj.skeleton.files.is_empty() {
        wow_exists
            && store
                .data
                .skeleton_version
                .map(|v| v >= vj.min_client_build.unwrap_or(0))
                .unwrap_or(false)
    } else {
        skeleton_missing_file(&install_dir, &vj.skeleton.files).is_none()
    };

    for patch in &vj.patches {
        let target_dest = install_dir.join(patch.target.replace('/', "\\"));
        let local_sha = store.lookup_mpq(&patch.target);
        let ok = target_dest.is_file()
            && local_sha
                .as_ref()
                .map(|s| s.eq_ignore_ascii_case(&patch.sha256))
                .unwrap_or(false);
        if ok {
            mpqs_ok += 1;
        } else {
            missing.push(patch.file.clone());
            to_download += patch.parts.iter().map(|p| p.size).sum::<u64>();
        }
    }

    let up_to_date = skeleton_ok && missing.is_empty();
    let local_build = store.data.skeleton_version;
    let skel_bytes: u64 = vj.skeleton.parts.iter().map(|p| p.size).sum();
    let total_download = if skeleton_ok { to_download } else { to_download + skel_bytes };

    Ok(CheckResult {
        install_dir: install_dir.display().to_string(),
        up_to_date,
        skeleton_ok,
        mpqs_ok,
        mpqs_total: vj.patches.len() as u32,
        missing_mpqs: missing,
        to_download_bytes: total_download,
        local_build,
        server_build,
    })
}

// ─── install_or_update / repair_client ───

#[tauri::command]
pub async fn install_or_update(
    app: AppHandle,
    state: State<'_, crate::AppState>,
) -> Result<InstallResult, String> {
    run_install(app, state, false).await
}

#[tauri::command]
pub async fn repair_client(
    app: AppHandle,
    state: State<'_, crate::AppState>,
) -> Result<InstallResult, String> {
    run_install(app, state, true).await
}

async fn run_install(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    force: bool,
) -> Result<InstallResult, String> {
    let settings = state.settings();
    let install_dir = settings.install_dir();
    let transfer: std::sync::Arc<TransferState> = state.transfer.clone();
    transfer.set_paused(false);

    tokio::fs::create_dir_all(&install_dir)
        .await
        .map_err(|e| format!("création du dossier {:?} impossible : {e}", install_dir))?;

    let tok = settings.github_token();
    let client = reqwest::Client::new();
    let vj = fetch_manifest(&settings, &tok, &client).await?;
    let tag = asset_tag(&settings, &vj);
    let owner = settings.owner();
    let repo = settings.repo();

    let assets = github::fetch_release(&client, &owner, &repo, &tag, Some(tok.as_str()))
        .await
        .map_err(|e| format!("release {tag} : {e}"))?;
    let asset_ids: std::collections::HashMap<String, u64> =
        assets.iter().map(|a| (a.name.clone(), a.id)).collect();
    let asset_url_for = |file: &str| -> Result<String, String> {
        let id = asset_ids
            .get(file)
            .ok_or_else(|| format!("asset introuvable dans la release {tag} : {file}"))?;
        Ok(github::asset_url(&owner, &repo, *id))
    };
    let rd = resource_dir(&app);
    let dl = dl_dir(&install_dir);
    tokio::fs::create_dir_all(&dl).await.ok();

    let mut store = Store::new(&install_dir);
    store.load();

    // ── 1. Squelette ──────────────────────────────────────────────────────────
    let wow_exists = utils::wow_exe(&install_dir).is_file();
    let local_skel_ver = store.data.skeleton_version.unwrap_or(0);
    // Squelette : re-téléchargé si la version locale est inférieure, si le
    // manifeste liste un fichier absent ou tronqué (wow.exe, DLL, cinématique,
    //…), ou si wow.exe est absent (fallback quand le manifeste n'a pas `files`).
    // Sinon — tout le contenu est présent — le téléchargement est sauté, même
    // en mode réparation.
    let manifest_missing = skeleton_missing_file(&install_dir, &vj.skeleton.files);
    if let Some(name) = &manifest_missing {
        emit_phase(&app, "skeleton", &format!("Fichier manquant/tronqué : {name}"));
    }
    // Manifeste avec `files` : la présence des fichiers EST la validité —
    // le squelette ne se re-télécharge que pour un fichier absent/tronqué.
    // (Le state est alors simplement synchronisé plus bas : bootstrap des
    // installations manuelles/copiées, sans re-téléchargement.)
    // Manifests anciens sans `files` : règle historique state/wow.exe.
    let skel_needed = match manifest_missing {
        Some(_) => true,
        None => {
            if vj.skeleton.files.is_empty() {
                (!wow_exists) || local_skel_ver < vj.latest_version
            } else {
                false
            }
        }
    };

    let mut skeleton_downloaded = false;

    // Pré-calcul du nombre total d'octets à télécharger pour la session (squelette + MPQs)
    let mut total_bytes: u64 = 0;
    if skel_needed {
        for part in &vj.skeleton.parts {
            let dest = dl.join(&part.file);
            total_bytes += if dest.is_file() {
                let len = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
                part.size.saturating_sub(len)
            } else {
                part.size
            };
        }
    }
    for patch in &vj.patches {
        let dest = install_dir.join(patch.target.replace('/', "\\"));
        let ok = dest.is_file()
            && store
                .lookup_mpq(&patch.target)
                .map(|s| s.eq_ignore_ascii_case(&patch.sha256))
                .unwrap_or(false);
        if !ok {
            for part in &patch.parts {
                let dest = dl.join(&part.file);
                total_bytes += if dest.is_file() {
                    let len = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
                    part.size.saturating_sub(len)
                } else {
                    part.size
                };
            }
        }
    }

    let mut global_done: u64 = 0;

    if skel_needed {
        emit_phase(&app, "skeleton", "Téléchargement du squelette…");
        // compter les parties du squelette
    let sk_parts = &vj.skeleton.parts;
    let skel_count = sk_parts.len();
    let mpq_parts_total: usize = vj.patches.iter().map(|p| p.parts.len()).sum();
    let total_parts = skel_count + mpq_parts_total;
    let mut g_idx = 0;

    for part in sk_parts.iter() {
        let dest = dl.join(&part.file);
        let need = if dest.is_file() {
            let len = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
            len != part.size
        } else {
            true
        };
        if need {
            let tx = make_tx(&app, g_idx, total_parts);
            let url = asset_url_for(&part.file)?;
            let got = download::download_with_resume(
                &client,
                &url,
                &dest,
                part.size,
                Some(&tx),
                "skeleton",
                g_idx,
                total_parts,
                Some(&tok),
                global_done,
                total_bytes,
                &transfer,
            )
            .await
            .map_err(|e| format!("download {} : {e}", part.file))?;
            global_done = global_done.saturating_add(got);
            let (ok, actual) = hash::verify(&dest, &part.sha256).await
                .map_err(|e| format!("vérif sha256 {} : {e}", part.file))?;
            if !ok {
                return Err(format!(
                    "sha256 invalide pour {} : attendu {}… obtenu {}…",
                    part.file,
                    part.sha256.chars().take(12).collect::<String>(),
                    actual.chars().take(12).collect::<String>()
                ));
            }
        }
        g_idx += 1;
    }

        // Extraire la première partie (7za gère les volumes suivants automatiquement)
        emit_phase(&app, "extraction", "Extraction du squelette…");
        let part0 = dl.join(&sk_parts[0].file);
        let install_dir_c = install_dir.clone();
        let rd_c = rd.clone();
        let joined = tokio::task::spawn_blocking(move || {
            let res = extract::extract_7z(&part0, &install_dir_c, rd_c.as_deref());
            if res.is_ok() {
                recombine::quarantine(&install_dir_c);
            }
            res
        })
        .await
        .map_err(|e| format!("la tâche d'extraction a paniqué : {e}"))?;
        joined
            .map_err(|e| format!("extraction du squelette impossible : {e}"))?;
        emit_phase(&app, "extraction", "Squelette extrait.");

        store.set_skeleton(vj.latest_version, &vj.skeleton.sha256);
        skeleton_downloaded = true;
        emit_phase(&app, "skeleton", "Squelette à jour.");
    } else if !vj.skeleton.files.is_empty()
        && store.data.skeleton_version != Some(vj.latest_version)
    {
        // Bootstrap : squelette complet mais inconnu du state (install
        // copiée manuellement). Le synchroniser sans rien télécharger.
        store.set_skeleton(vj.latest_version, &vj.skeleton.sha256);
        emit_phase(
            &app,
            "skeleton",
            "Squelette complet — état synchronisé (aucun téléchargement).",
        );
    }

    // ── 2. MPQs ───────────────────────────────────────────────────────────────

    // Déjà validés par le state (le lanceur a posé le SHA lui-même) : jamais
    // re-hachés ni re-téléchargés, même en mode réparation.
    let mut clean_targets: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for p in &vj.patches {
        let dest = install_dir.join(p.target.replace('/', "\\"));
        if dest.is_file()
            && store
                .lookup_mpq(&p.target)
                .is_some_and(|s| s.eq_ignore_ascii_case(&p.sha256))
        {
            clean_targets.insert(p.target.clone());
        }
    }

    // MPQs à vérifier par hachage en parallèle (mode réparation uniquement) :
    // présents sur disque, non validés par le state, taille conforme au
    // manifeste. Les absents ou à taille faussée vont directement au
    // téléchargement (une ré-écriture a la même probabilité d'échec que
    // le téléchargement du fichier).
    let mpq_need_verify: Vec<(version::Patch, PathBuf, u64)> = if force {
        vj.patches
            .iter()
            .filter_map(|patch| {
                if clean_targets.contains(&patch.target) {
                    return None;
                }
                let dest = install_dir.join(patch.target.replace('/', "\\"));
                if !dest.is_file() {
                    return None;
                }
                let expected: u64 = patch.parts.iter().map(|p| p.size).sum();
                if dest.metadata().is_ok_and(|m| m.len() != expected) {
                    return None;
                }
                Some((patch.clone(), dest, expected))
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut mpqs_verified: u32 = 0;
    if force && !mpq_need_verify.is_empty() {
        let mpq_need_verify_total = mpq_need_verify.len();
        emit_phase(
            &app,
            "mpqs",
            &format!(
                "Vérification des {} MPQs (SHA256, en parallèle)…",
                mpq_need_verify.len()
            ),
        );
        let semaphore = Arc::new(tokio::sync::Semaphore::new(4));
        let mut handles = Vec::with_capacity(mpq_need_verify.len());
        for (patch, dest, expected) in mpq_need_verify.into_iter() {
            let sem = Arc::clone(&semaphore);
            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire_owned().await;
                match hash::verify(&dest, &patch.sha256).await {
                    Ok((true, _)) => Ok((patch, expected)),
                    Ok((false, actual)) => Err(format!(
                        "MPQ corrompu : {} (SHA256 {}… ≠ {}…)",
                        patch.file,
                        actual.chars().take(12).collect::<String>(),
                        patch.sha256.chars().take(12).collect::<String>()
                    )),
                    Err(e) => Err(format!("hachage impossible pour {} : {e}", patch.file)),
                }
            }));
        }
        for h in handles {
            match h.await {
                Ok(Ok((patch, expected))) => {
                    clean_targets.insert(patch.target.clone());
                    // Enregistrement dans le state : cette install (peut-être
                    // copiée manuellement) devient désormais connue du lanceur.
                    store.record_mpq(&patch.target, &patch.sha256, expected);
                    mpqs_verified += 1;
                    emit_phase(&app, "mpqs", &format!("{} ✓ vérifié", patch.file));
                }
                Ok(Err(e)) => {
                    emit_phase(&app, "mpqs", &format!("→ ré-écriture : {e}"));
                }
                Err(e) => {
                    emit_phase(&app, "mpqs", &format!("tâche de vérification a échoué : {e}"));
                }
            }
        }
        emit_phase(
            &app,
            "mpqs",
            &format!(
                "{} MPQ validé(s) par hachage sur {} à vérifier.",
                mpqs_verified,
                mpq_need_verify_total
            ),
        );
    }

    emit_phase(&app, "mpqs", "Téléchargement des MPQs manquants…");
    let mpq_parts_total: usize = vj.patches.iter().map(|p| p.parts.len()).sum();
    let sk_parts_total = vj.skeleton.parts.len();
    let total_all = sk_parts_total + mpq_parts_total;
    let mut g_idx = sk_parts_total; // on démarre après le squelette

    let mut downloaded: u32 = 0;
    let mut skipped: u32 = 0;
    let failed: u32 = 0;

    for patch in vj.patches.iter() {
        let target_rel = patch.target.replace('/', "\\");
        let data_dir = install_dir.join("Data");
        tokio::fs::create_dir_all(&data_dir).await.ok();

        let is_ok = clean_targets.contains(&patch.target);

        if is_ok {
            skipped += 1;
            g_idx += patch.parts.len();
            continue;
        }

        emit_phase(
            &app,
            "mpqs",
            &format!("Téléchargement de {}…", patch.file),
        );

        let mut part_paths: Vec<String> = Vec::with_capacity(patch.parts.len());

        for part in patch.parts.iter() {
            let part_dest = dl.join(&part.file);
            let need = if part_dest.is_file() {
                let len = std::fs::metadata(&part_dest).map(|m| m.len()).unwrap_or(0);
                len != part.size
            } else {
                true
            };
            if need {
                let tx = make_tx(&app, g_idx, total_all);
                let url = asset_url_for(&part.file)?;
                let got = download::download_with_resume(
                    &client,
                    &url,
                    &part_dest,
                    part.size,
                    Some(&tx),
                    "mpqs",
                    g_idx,
                    total_all,
                    Some(&tok),
                    global_done,
                    total_bytes,
                    &transfer,
                )
                .await
                .map_err(|e| format!("download {} : {e}", part.file))?;
                global_done = global_done.saturating_add(got);

                let (ok, actual) = hash::verify(&part_dest, &part.sha256).await
                    .map_err(|e| format!("vérif sha256 {} : {e}", part.file))?;
                if !ok {
                    return Err(format!(
                        "sha256 invalide pour {} : attendu {}… obtenu {}…",
                        part.file,
                        part.sha256.chars().take(12).collect::<String>(),
                        actual.chars().take(12).collect::<String>()
                    ));
                }
            }
            part_paths.push(part_dest.display().to_string());
            g_idx += 1;
        }

        // Concaténer les parties → MPQ final
        let final_dest = install_dir.join(&target_rel);
        let tmp_final = dl.join(format!("{}.full", patch.file));
        let (size, sha) = recombine::concat_parts(&part_paths, &tmp_final)
            .map_err(|e| format!("concatenation {} : {e}", patch.file))?;

        let ok = sha.eq_ignore_ascii_case(&patch.sha256);
        if !ok {
            let _ = std::fs::remove_file(&tmp_final);
            return Err(format!(
                "sha256 final {} : attendu {}… obtenu {}…  (taille {size} octets)",
                patch.file,
                patch.sha256.chars().take(12).collect::<String>(),
                sha.chars().take(12).collect::<String>()
            ));
        }

        // Réinstaller le MPQ
        let _ = std::fs::remove_file(&final_dest);
        std::fs::rename(&tmp_final, &final_dest)
            .map_err(|e| format!("déplacement {} → {} : {e}", patch.file, target_rel))?;
        recombine::quarantine(&final_dest);

        store.record_mpq(&patch.target, &patch.sha256, size);
        downloaded += 1;
        emit_phase(&app, "mpqs", &format!("{} ✓", patch.file));
    }

    let installed = failed == 0 && (skeleton_downloaded || skipped > 0 || downloaded > 0);

    // suppression du dossier temp .dl en fin de tâche
    let _ = tokio::fs::remove_dir_all(&dl).await;

    // sauvegarder state
    store
        .save()
        .map_err(|e| format!("enregistrement du state impossible : {e}"))?;

    if failed > 0 {
        return Err(format!("{failed} fichiers ont échoué."));
    }

    Ok(InstallResult {
        installed,
        skeleton_downloaded,
        mpqs_downloaded: downloaded,
        mpqs_skipped: skipped,
        mpqs_verified,
        failed: 0,
    })
}
