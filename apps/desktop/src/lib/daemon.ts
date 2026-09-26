import { invoke } from "@tauri-apps/api/core";
import type { Status } from "@/bindings/Status";

export type { Status };

// Mirrors CommandError in src-tauri/src/lib.rs.
export type DaemonError =
  | { kind: "not_running" }
  | { kind: "api"; code: string; message: string }
  | { kind: "other"; message: string };

export const daemonStatus = () => invoke<Status>("api", { method: "GET", path: "/status" });
