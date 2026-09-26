import { cn } from "cn";

import { useScrollEdge } from "@/lib/scroll-edge";

/**
 * A full-height scrolling page under the translucent top bar. Every non-chat screen
 * (Connections, Models, and any new one) is built from these pieces so spacing,
 * type and surfaces stay consistent. See AGENTS.md, "Look and feel".
 */
export function Page({ children, className }: { children: React.ReactNode; className?: string }) {
  const onScroll = useScrollEdge();
  return (
    <div className="h-full overflow-y-auto" onScroll={onScroll}>
      <div className={cn("mx-auto flex w-full max-w-[880px] flex-col gap-10 px-6 pt-[92px] pb-20", className)}>
        {children}
      </div>
    </div>
  );
}

/** Large title and one line of plain-language explanation. */
export function PageHeader({
  title,
  subtitle,
  action,
}: {
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    <header className="flex items-end justify-between gap-6">
      <div className="flex flex-col gap-1.5">
        <h1 className="type-large-title">{title}</h1>
        {subtitle && <p className="max-w-[560px] type-body text-muted-foreground">{subtitle}</p>}
      </div>
      {action}
    </header>
  );
}

/** A titled group of content. */
export function Section({
  title,
  action,
  children,
  className,
}: {
  title?: React.ReactNode;
  action?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section className={cn("flex flex-col gap-2.5", className)}>
      {(title || action) && (
        <div className="flex min-h-7 items-end justify-between gap-4">
          {title ? <h2 className="section-label">{title}</h2> : <span />}
          {action}
        </div>
      )}
      {children}
    </section>
  );
}

/** iOS-style grouped list: white rounded container with hairline separators. */
export function Grouped({ children, className }: { children: React.ReactNode; className?: string }) {
  return <div className={cn("grouped flex flex-col", className)}>{children}</div>;
}

/** One row of a grouped list: optional leading icon, title, detail, trailing controls. */
export function Row({
  icon,
  title,
  detail,
  trailing,
  onClick,
  className,
}: {
  icon?: React.ReactNode;
  title: React.ReactNode;
  detail?: React.ReactNode;
  trailing?: React.ReactNode;
  onClick?: () => void;
  className?: string;
}) {
  const body = (
    <>
      {icon}
      <div className="min-w-0 flex-1">
        <div className="truncate type-body font-medium">{title}</div>
        {detail && <div className="truncate type-subhead text-muted-foreground">{detail}</div>}
      </div>
      {trailing && <div className="flex shrink-0 items-center gap-1.5">{trailing}</div>}
    </>
  );
  const base = cn("flex min-h-[60px] items-center gap-3.5 px-4 py-2.5 text-left", className);
  return onClick ? (
    <button onClick={onClick} className={cn(base, "w-full transition-colors hover:bg-[rgb(118_118_128/0.06)]")}>
      {body}
    </button>
  ) : (
    <div className={base}>{body}</div>
  );
}

/** A rounded square with an icon, tinted like iOS Settings icons. */
export function IconTile({
  children,
  className,
  size = "md",
}: {
  children: React.ReactNode;
  className?: string;
  size?: "sm" | "md" | "lg";
}) {
  return (
    <div
      className={cn(
        "flex shrink-0 items-center justify-center",
        size === "sm" && "size-7 rounded-[8px] [&_svg]:size-4",
        size === "md" && "size-9 rounded-[10px] [&_svg]:size-[18px]",
        size === "lg" && "size-12 rounded-[14px] [&_svg]:size-6",
        className,
      )}
    >
      {children}
    </div>
  );
}

/** A small capsule label, e.g. "Recommended" or "Soon". */
export function Pill({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <span
      className={cn(
        "inline-flex h-[22px] shrink-0 items-center gap-1 rounded-full bg-fill px-2.5 text-[12px] font-medium text-muted-foreground",
        className,
      )}
    >
      {children}
    </span>
  );
}
