/** "7:00", "20:30": the same style as the daemon's descriptions ("Every weekday at 7:00"). */
export function clock(ms: number) {
  const d = new Date(ms);
  return `${d.getHours()}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** "Today at 9:00", "Tomorrow at 9:00", "Friday at 7:00", "Mon 20 Oct at 9:00". */
export function when(ms: number) {
  const d = new Date(ms);
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((start(d) - start(new Date())) / 86_400_000);
  const at = `at ${clock(ms)}`;
  if (days === 0) return `Today ${at}`;
  if (days === 1) return `Tomorrow ${at}`;
  if (days === -1) return `Yesterday ${at}`;
  if (days > 1 && days < 7) return `${d.toLocaleDateString([], { weekday: "long" })} ${at}`;
  const sameYear = d.getFullYear() === new Date().getFullYear();
  const date = d.toLocaleDateString([], {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: sameYear ? undefined : "numeric",
  });
  return `${date} ${at}`;
}
