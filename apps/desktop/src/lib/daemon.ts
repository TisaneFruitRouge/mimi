import { invoke } from "@tauri-apps/api/core";

// Mirrors hearth_protocol::Status.
export interface Status {
  version: string;
  pid: number;
  uptime_secs: number;
  data_dir: string;
}

// Mirrors CommandError in src-tauri/src/lib.rs.
export type DaemonError = { kind: "not_running" } | { kind: "other"; message: string };

export const daemonStatus = () => invoke<Status>("daemon_status");
