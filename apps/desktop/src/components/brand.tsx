import { cn } from "cn";

export function FlameIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.1"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden
    >
      <path d="M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.072-2.143-.224-4.054 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.153.433-2.294 1-3a2.5 2.5 0 0 0 2.5 2.5z" />
    </svg>
  );
}

/** The lime tile with the flame. */
export function LogoMark({ className }: { className?: string }) {
  return (
    <div
      className={cn(
        "flex size-8 shrink-0 items-center justify-center rounded-[10px] bg-lime text-lime-ink shadow-[inset_0_0_0_1px_rgba(0,0,0,0.06)]",
        className,
      )}
    >
      <FlameIcon className="size-[55%]" />
    </div>
  );
}

/** The assistant's avatar next to its replies. */
export function AssistantMark() {
  return (
    <div className="flex size-7 shrink-0 items-center justify-center rounded-lg border bg-background text-[#6f8f14] shadow-xs">
      <FlameIcon className="size-4" />
    </div>
  );
}
