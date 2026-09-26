import type { Locality } from "@/bindings/Locality";

export function formatBytes(bytes: number) {
  const gb = bytes / 1e9;
  return gb >= 10 ? `${Math.round(gb)} GB` : `${gb.toFixed(1)} GB`;
}

export const localityLabel: Record<Locality, string> = {
  device: "On this device",
  network: "On your network",
  cloud: "Cloud",
};

export const localityExplanation: Record<Locality, string> = {
  device: "Runs on this computer. Your messages never leave it.",
  network: "Runs on a machine in your own network. Messages stay within it.",
  cloud: "Runs on a third-party service. Your messages are sent to it over the internet.",
};
