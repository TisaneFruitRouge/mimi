import { useEffect, useMemo, useRef, useState } from "react";
import { Check, Loader2, Search, Wrench } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { Locality } from "@/bindings/Locality";
import type { ModelPrice } from "@/bindings/ModelPrice";
import type { ModelRef } from "@/bindings/ModelRef";
import { LocalityBadge } from "@/components/locality-badge";
import { Pill } from "@/components/page";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { api } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { sameModel, useAllModels, useModelInfo, useProviders, useSettings } from "@/lib/queries";

/** One model as the list shows it. */
export interface ModelEntry {
  ref: ModelRef;
  name: string;
  maker: string | null;
  sourceName: string;
  locality: Locality;
  sizeBytes: number | null;
  supportsTools: boolean | null;
  price: ModelPrice | null;
  recommended?: boolean;
}

/**
 * Roughly what one message costs, in cents: Mimi sends about 6,000 tokens (its
 * instructions, what it recalls, the conversation) and gets a few hundred back.
 */
export function centsPerMessage(price: ModelPrice) {
  return ((6000 * price.input + 300 * price.output) / 1e6) * 100;
}

export function costLabel(price: ModelPrice) {
  if (price.input === 0 && price.output === 0) return "Free";
  const cents = centsPerMessage(price);
  if (cents < 0.01) return "under 0.01¢ a message";
  if (cents < 0.1) return `about ${cents.toFixed(2)}¢ a message`;
  if (cents < 100) return `about ${cents.toFixed(1)}¢ a message`;
  return `about $${(cents / 100).toFixed(2)} a message`;
}

type Sort = "name" | "price";

/**
 * A searchable, filterable list of models to pick one from. Arrow keys move through it;
 * Enter or a double click chooses.
 */
export function ModelList({
  entries,
  selected,
  current,
  onSelect,
  onChoose,
  sources,
  initialSource = null,
  className,
}: {
  entries: ModelEntry[];
  selected: ModelRef | null;
  /** The model the assistant uses now. */
  current?: ModelRef | null;
  onSelect: (ref: ModelRef) => void;
  onChoose: (ref: ModelRef) => void;
  /** Offer a filter by source when models come from several. */
  sources?: { id: string; name: string }[];
  initialSource?: string | null;
  className?: string;
}) {
  const [query, setQuery] = useState("");
  const [source, setSource] = useState<string | null>(initialSource);
  const [toolsOnly, setToolsOnly] = useState(true);
  const [sort, setSort] = useState<Sort>("name");
  const listRef = useRef<HTMLDivElement>(null);

  const inSource = entries.filter((e) => !source || e.ref.provider_id === source);
  const hasToolInfo = inSource.some((e) => e.supportsTools === false);
  const hasPrices = inSource.some((e) => e.price);

  const shown = useMemo(() => {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const haystack = (e: ModelEntry) =>
      `${e.name} ${e.maker ?? ""} ${e.ref.model} ${e.sourceName}`.toLowerCase();
    const list = inSource.filter(
      (e) =>
        (!toolsOnly || !hasToolInfo || e.supportsTools !== false) &&
        words.every((w) => haystack(e).includes(w)),
    );
    const cost = (e: ModelEntry) => (e.price ? centsPerMessage(e.price) : Infinity);
    const byName = (a: ModelEntry, b: ModelEntry) => a.name.localeCompare(b.name);
    // Recommended first, then names that start with what was typed, then the chosen order.
    const rank = (e: ModelEntry) =>
      (e.recommended && !words.length ? 0 : 2) -
      (words[0] && e.name.toLowerCase().startsWith(words[0]) ? 1 : 0);
    return list.sort(
      (a, b) => rank(a) - rank(b) || (sort === "price" ? cost(a) - cost(b) || byName(a, b) : byName(a, b)),
    );
  }, [inSource, query, toolsOnly, hasToolInfo, sort]);

  const selectedIndex = shown.findIndex((e) => sameModel(e.ref, selected));

  // Keep the selection visible while moving with the keyboard.
  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>(`[data-index="${selectedIndex}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [selectedIndex]);

  const move = (by: number) => {
    if (shown.length === 0) return;
    const next = selectedIndex < 0 ? 0 : Math.min(shown.length - 1, Math.max(0, selectedIndex + by));
    onSelect(shown[next].ref);
  };

  const hidden = inSource.length - shown.length;

  return (
    <div
      className={cn("flex min-h-0 flex-col gap-3", className)}
      // Arrows move through the list wherever focus is (search field, filters, list);
      // Enter chooses from the search field or the list, not from a filter button.
      onKeyDown={(e) => {
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          e.preventDefault();
          move(e.key === "ArrowDown" ? 1 : -1);
        } else if (e.key === "Enter" && selectedIndex >= 0 && !(e.target instanceof HTMLButtonElement)) {
          e.preventDefault();
          onChoose(shown[selectedIndex].ref);
        }
      }}
    >
      <div className="relative">
        <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-faint" />
        <Input
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search by name or maker"
          aria-label="Search models"
          aria-controls="model-list"
          className="h-10 pl-9"
        />
      </div>

      <div className="flex flex-wrap items-center gap-1.5">
        {sources && sources.length > 1 && (
          <>
            <Chip active={source === null} onClick={() => setSource(null)}>
              All
            </Chip>
            {sources.map((s) => (
              <Chip key={s.id} active={source === s.id} onClick={() => setSource(s.id)}>
                {s.name}
              </Chip>
            ))}
            <span className="mx-1 h-4 w-px bg-separator" />
          </>
        )}
        {hasToolInfo && (
          <Chip active={toolsOnly} onClick={() => setToolsOnly((t) => !t)}>
            <Wrench className="size-3" /> Works with your calendar, mail and reminders
          </Chip>
        )}
        {hasPrices && (
          <Chip active={sort === "price"} onClick={() => setSort((s) => (s === "price" ? "name" : "price"))}>
            Cheapest first
          </Chip>
        )}
      </div>

      <div
        ref={listRef}
        id="model-list"
        role="listbox"
        aria-label="Models"
        tabIndex={-1}
        className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto rounded-[14px] bg-subtle p-1"
      >
        {shown.map((e, i) => (
          <ModelRow
            key={`${e.ref.provider_id}/${e.ref.model}`}
            entry={e}
            index={i}
            selected={i === selectedIndex}
            current={sameModel(e.ref, current)}
            showSource={!source && (sources?.length ?? 0) > 1}
            onSelect={() => onSelect(e.ref)}
            onChoose={() => onChoose(e.ref)}
          />
        ))}
        {shown.length === 0 && (
          <p className="px-3 py-10 text-center type-subhead text-muted-foreground">
            {query.trim() ? `No model matches “${query.trim()}”.` : "No models here."}
          </p>
        )}
      </div>
      <p className="px-1 type-footnote text-faint">
        {shown.length} model{shown.length === 1 ? "" : "s"}
        {hidden > 0 && toolsOnly && hasToolInfo && !query.trim()
          ? ` · ${hidden} that can't use your calendar, mail or reminders are hidden`
          : ""}
      </p>
    </div>
  );
}

function Chip({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "pressable inline-flex h-7 items-center gap-1.5 rounded-full px-3 text-[12px] font-medium",
        active ? "bg-foreground text-background" : "bg-fill text-muted-foreground hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

function ModelRow({
  entry,
  index,
  selected,
  current,
  showSource,
  onSelect,
  onChoose,
}: {
  entry: ModelEntry;
  index: number;
  selected: boolean;
  current: boolean;
  showSource: boolean;
  onSelect: () => void;
  onChoose: () => void;
}) {
  const details = [
    entry.maker,
    showSource ? entry.sourceName : null,
    entry.price ? costLabel(entry.price) : null,
    entry.sizeBytes ? formatBytes(entry.sizeBytes) : null,
  ].filter(Boolean);
  return (
    <div
      role="option"
      aria-selected={selected}
      data-index={index}
      onClick={onSelect}
      onDoubleClick={onChoose}
      className={cn(
        "flex cursor-default items-center gap-3 rounded-[10px] px-3 py-2 transition-[background-color,box-shadow]",
        selected ? "bg-background shadow-[var(--shadow-card)]" : "hover:bg-[rgb(118_118_128/0.08)]",
      )}
    >
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate type-callout font-medium">{entry.name}</span>
          {entry.recommended && (
            <Pill className="h-[20px] bg-lime-soft px-2 text-[11px] text-lime-deep">Recommended</Pill>
          )}
          {current && <Pill className="h-[20px] px-2 text-[11px]">In use</Pill>}
        </div>
        <div className="truncate type-footnote text-muted-foreground" title={priceTitle(entry.price)}>
          {details.join(" · ") || entry.ref.model}
          {entry.supportsTools === false && (
            <span className="text-cloud"> · can't use your calendar, mail or reminders</span>
          )}
        </div>
      </div>
      <LocalityBadge locality={entry.locality} />
      <span className="flex w-5 shrink-0 justify-center">
        {selected ? (
          <Check className="size-[18px] text-lime-deep" strokeWidth={2.6} />
        ) : (
          <span className="size-4 rounded-full shadow-[inset_0_0_0_1.5px_rgb(0_0_0/0.18)]" />
        )}
      </span>
    </div>
  );
}

function priceTitle(price: ModelPrice | null) {
  if (!price) return undefined;
  return `$${price.input} per million tokens sent, $${price.output} per million received`;
}

/** Every model from every source, as list entries. */
export function useModelEntries(recommended: (ref: ModelRef) => boolean = () => false): ModelEntry[] {
  const { options } = useAllModels();
  const info = useModelInfo();
  return options.map((o) => {
    const { name, maker } = info(o.ref.model, o.name);
    return {
      ref: o.ref,
      name,
      maker,
      sourceName: o.providerName,
      locality: o.locality,
      sizeBytes: o.sizeBytes,
      supportsTools: o.supportsTools,
      price: o.price,
      recommended: recommended(o.ref),
    };
  });
}

/**
 * A window for choosing the assistant's model among everything the sources offer, or
 * (`forPhotos`) the model for photos among those that can see them.
 */
export function ModelPickerDialog({
  open,
  onOpenChange,
  initialSource = null,
  recommended,
  forPhotos = false,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Start filtered to this source. */
  initialSource?: string | null;
  recommended?: (ref: ModelRef) => boolean;
  forPhotos?: boolean;
}) {
  const settings = useSettings().data;
  const providers = useProviders().data ?? [];
  const { options, loading } = useAllModels();
  const all = useModelEntries(recommended);
  const entries = forPhotos
    ? all.filter((e) => options.find((o) => sameModel(o.ref, e.ref))?.seesImages === true)
    : all;
  const current = (forPhotos ? settings?.photo_model : settings?.default_model) ?? null;
  const [selected, setSelected] = useState<ModelRef | null>(null);
  const [busy, setBusy] = useState(false);
  const sources = providers
    .filter((p) => entries.some((e) => e.ref.provider_id === p.id))
    .map((p) => ({ id: p.id, name: p.name }));
  const chosen = entries.find((e) => sameModel(e.ref, selected ?? current)) ?? null;

  const choose = async (ref: ModelRef) => {
    if (!settings) return;
    if (sameModel(ref, current)) {
      onOpenChange(false);
      return;
    }
    setBusy(true);
    try {
      await api.putSettings(forPhotos ? { ...settings, photo_model: ref } : { ...settings, default_model: ref });
      const name = entries.find((e) => sameModel(e.ref, ref))?.name ?? ref.model;
      toast.success(forPhotos ? `Photos now go to ${name}` : `Your assistant now uses ${name}`);
      onOpenChange(false);
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        onOpenChange(o);
        if (!o) setSelected(null);
      }}
    >
      <DialogContent className="flex h-[min(680px,86vh)] flex-col gap-4 sm:max-w-[620px]">
        <DialogHeader>
          <DialogTitle>{forPhotos ? "Choose a model for photos" : "Choose a model"}</DialogTitle>
          <DialogDescription>
            {forPhotos
              ? "It answers messages with photos, and the message right after. Only models that can see photos are listed."
              : "What your assistant thinks with. You can change it any time."}
          </DialogDescription>
        </DialogHeader>
        {loading && entries.length === 0 ? (
          <div className="flex flex-1 items-center justify-center">
            <Loader2 className="size-5 animate-spin text-faint" />
          </div>
        ) : forPhotos && entries.length === 0 ? (
          <p className="flex flex-1 items-center justify-center px-6 text-center type-callout text-muted-foreground">
            None of your models can see photos. Add a cloud service in Model sources to get some that can.
          </p>
        ) : (
          <ModelList
            key={`${open}-${initialSource}`}
            entries={entries}
            selected={selected ?? current}
            current={current}
            onSelect={setSelected}
            onChoose={choose}
            sources={sources}
            initialSource={initialSource}
            className="flex-1"
          />
        )}
        <div className="flex items-center justify-end gap-2">
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button disabled={busy || !chosen} onClick={() => chosen && choose(chosen.ref)}>
            {busy && <Loader2 className="animate-spin" />}
            {chosen && !sameModel(chosen.ref, current) ? `Use ${chosen.name}` : "Done"}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
