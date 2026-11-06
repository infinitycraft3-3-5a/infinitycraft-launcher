export type Phase =
  | "boot"
  | "checking"
  | "install"
  | "updating"
  | "ready"
  | "repairing"
  | "launching"
  | "paused"
  | "error";

export interface Part {
  file: string;
  size: number;
  sha256: string;
}

export interface Skeleton {
  sha256: string;
  parts: Part[];
}

export interface Patch {
  file: string;
  version?: number;
  target: string;
  sha256: string;
  parts: Part[];
}

export interface VersionJson {
  release: string;
  latestVersion: number;
  minClientBuild?: number;
  skeleton: Skeleton;
  patches: Patch[];
}

export interface CheckResult {
  install_dir: string;
  up_to_date: boolean;
  skeleton_ok: boolean;
  mpqs_ok: number;
  mpqs_total: number;
  missing_mpqs: string[];
  to_download_bytes: number;
  local_build: number | null;
  server_build: number;
}

export interface ProgressEvent {
  file: string;
  file_index: number;
  file_count: number;
  percent: number;
  bytes_file: number;
  bytes_file_total: number;
  bytes_global: number;
  bytes_total: number;
  speed_bps: number;
  done: boolean;
  paused: boolean;
  phase: string;
}

export interface InstallResult {
  installed: boolean;
  skeleton_downloaded: boolean;
  mpqs_downloaded: number;
  mpqs_skipped: number;
  failed: number;
}

export interface Settings {
  install_dir: string;
  clear_cache: boolean;
  auto_update: boolean;
  language: string;
  owner: string;
  repo: string;
  tag_hint: string;
  realmline: string;
  server_host: string;
  server_port: number;
  announce_url: string;
  announce_board: string;
  version_json_url: string;
  github_token: string;
}

export interface Announcement {
  id: number;
  title: string;
  link: string;
  date: string;
  excerpt: string;
}

export interface ServerStatus {
  online: boolean;
  host: string;
  port: number;
  latency_ms: number | null;
  error: string | null;
}

export interface LaunchResult {
  pid: number;
  realmline: string;
  cwd: string;
}

export interface LauncherUpdate {
  available: boolean;
  version: string;
  url: string;
  sha256: string;
}
