import {
  Baby,
  Briefcase,
  Calendar,
  Car,
  Code,
  Dumbbell,
  Folder,
  Gift,
  GraduationCap,
  Heart,
  House,
  Landmark,
  type LucideIcon,
  Music,
  Newspaper,
  PawPrint,
  PiggyBank,
  Plane,
  Receipt,
  ShoppingBag,
  Sparkles,
  Star,
  Stethoscope,
  Users,
  Utensils,
} from "lucide-react";
import { cn } from "cn";

/**
 * How smart folders can look. The names match the daemon's `mail::folders::ICONS` and
 * `COLORS` (it refuses anything else); unknown names fall back to the first of each.
 */
export const folderIcons: Record<string, LucideIcon> = {
  sparkles: Sparkles,
  folder: Folder,
  receipt: Receipt,
  plane: Plane,
  briefcase: Briefcase,
  heart: Heart,
  house: House,
  "shopping-bag": ShoppingBag,
  "graduation-cap": GraduationCap,
  baby: Baby,
  "paw-print": PawPrint,
  car: Car,
  stethoscope: Stethoscope,
  landmark: Landmark,
  newspaper: Newspaper,
  users: Users,
  star: Star,
  gift: Gift,
  utensils: Utensils,
  dumbbell: Dumbbell,
  music: Music,
  code: Code,
  "piggy-bank": PiggyBank,
  calendar: Calendar,
};

/** Each colour as a soft fill and a darker ink readable on it and on white. */
export const folderColors: Record<string, { soft: string; ink: string; label: string }> = {
  violet: { soft: "#efe9fb", ink: "#6146ad", label: "Violet" },
  blue: { soft: "#e6effd", ink: "#1f64c7", label: "Blue" },
  teal: { soft: "#e3f5ee", ink: "#0a7a5b", label: "Teal" },
  green: { soft: "#eaf6e1", ink: "#3f7a1e", label: "Green" },
  yellow: { soft: "#fcf3d4", ink: "#8a6a00", label: "Yellow" },
  orange: { soft: "#fdeedc", ink: "#b25a00", label: "Orange" },
  red: { soft: "#fdeaea", ink: "#c2311f", label: "Red" },
  pink: { soft: "#fce9f2", ink: "#b0306e", label: "Pink" },
  gray: { soft: "#efeff2", ink: "#5b5b62", label: "Gray" },
};

export const iconOf = (name: string) => folderIcons[name] ?? Sparkles;
export const colorOf = (name: string) => folderColors[name] ?? folderColors.violet;

/** A folder's icon in its colour, e.g. in the sidebar. */
export function FolderGlyph({ icon, color, className }: { icon: string; color: string; className?: string }) {
  const Icon = iconOf(icon);
  return <Icon className={cn("size-4 shrink-0", className)} style={{ color: colorOf(color).ink }} />;
}

/** Icon and colour choices for the folder dialog. */
export function FolderLooksPicker({
  icon,
  color,
  onIcon,
  onColor,
}: {
  icon: string;
  color: string;
  onIcon: (name: string) => void;
  onColor: (name: string) => void;
}) {
  const c = colorOf(color);
  return (
    <div className="flex flex-col gap-3">
      <div role="radiogroup" aria-label="Colour" className="flex flex-wrap gap-2">
        {Object.entries(folderColors).map(([name, v]) => (
          <button
            key={name}
            type="button"
            role="radio"
            aria-checked={name === color}
            aria-label={v.label}
            title={v.label}
            onClick={() => onColor(name)}
            className={cn(
              "pressable size-6 rounded-full shadow-[inset_0_0_0_0.5px_rgb(0_0_0/0.12)]",
              name === color && "ring-2 ring-offset-2 ring-offset-background",
            )}
            style={{ background: v.ink, ...(name === color ? { ["--tw-ring-color" as string]: v.ink } : {}) }}
          />
        ))}
      </div>
      <div role="radiogroup" aria-label="Icon" className="grid grid-cols-8 gap-1.5">
        {Object.entries(folderIcons).map(([name, Icon]) => {
          const on = name === icon;
          return (
            <button
              key={name}
              type="button"
              role="radio"
              aria-checked={on}
              aria-label={name.replace("-", " ")}
              onClick={() => onIcon(name)}
              className={cn(
                "pressable flex h-9 items-center justify-center rounded-[10px] transition-colors",
                on ? "" : "text-muted-foreground hover:bg-fill",
              )}
              style={on ? { background: c.soft, color: c.ink } : undefined}
            >
              <Icon className="size-4" />
            </button>
          );
        })}
      </div>
    </div>
  );
}
