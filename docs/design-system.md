# Design system

How Mimi looks, feels and talks: a calm, precise, macOS/iOS-system-app feel on a light
canvas. The tokens and utilities live in `apps/desktop/src/index.css`, the shared pieces
in `apps/desktop/src/components/`. New screens compose these rather than inventing their
own. How the frontend is put together (transport, routes, panels) is in
[Frontend](frontend.md).

## Rules

- Compose the existing tokens, utilities and components (`index.css`,
  `src/components/`); don't invent new ones per screen. Light theme only for now.
- Copy is plain language for non-technical people: "model source" not "provider", no
  URLs unless the user typed one, no model names outside the Models page, no tool names
  or technical labels in chat.
- Every place a model is chosen or used shows its locality (`LocalityBadge`,
  `components/locality-badge.tsx`). Cloud use must always be visible to the user (see
  [Models](models.md)).
- Use the `type-*` utilities for sizes, never ad-hoc sizes, and never `text-*` names for
  sizes (the `cn()` merger would drop them; see Type).
- Section headings use `section-label`, never mono uppercase labels. Mono is only for
  things the user literally types.
- Lime is used sparingly. Data-flow colours (teal, blue, amber) keep their fixed
  meanings. No hard black borders.
- Elevation comes from shadows, never from borders.
- The top bar's material fades in by opacity only: never transition `backdrop-filter`,
  and keep scroll state out of the shell's React state (`lib/scroll-edge.ts`).
- Destructive and rarely used actions go in a "…" `DropdownMenu`, not in the row.
  Destructive actions are confirmed with the centred, macOS-style `AlertDialog`
  (`AlertDialogAction variant="destructive"`).
- List selection is grey fill, not lime.
- Anything read from a calendar (titles, places, notes, attendees) is plain text: never
  linkified, never markdown.
- Every right-click opens one of Mimi's menus, never the webview's (Back, Reload…). Text
  fields keep the system's editing menu.
- Motion uses springs, not linear easing. Don't wrap streaming content (message text) in
  layout animations. Everything respects `prefers-reduced-motion`.
- Chat has no avatars. Model switching lives in Models, not in the composer.
- The assistant's character is never named or lettered, and is used sparingly. The app
  icon is the same drawing: change them together.

## Copy

User-facing text is written for non-technical people, with no jargon in the default
path:

- "model source", not "provider";
- no URLs unless the user typed one;
- no model names outside the Models page (onboarding may name models like the Models
  page does; see [Onboarding](onboarding.md));
- no tool names or technical labels in chat;
- buttons that name the assistant use the user's assistant name (`useAssistantName`),
  not "Mimi" (see [Frontend › Ask about this](frontend.md#ask-about-this)).

## Locality

Every place a model is chosen or used shows where it runs with `LocalityBadge`
(`components/locality-badge.tsx`), in the data-flow colours below. Cloud use must always
be visible. Locality itself comes from the model source; see [Models](models.md).

## Colours

Tokens are on `:root`, with Tailwind names in `@theme`:

| Token | Value | Use |
|---|---|---|
| `canvas` | #f5f5f7 | App background |
| `background` | white | Cards, popovers, composer |
| `foreground` | #1d1d1f | Text |
| `muted-foreground` / `faint` | #6e6e73 | Secondary text |
| `fill` | translucent grey | Segmented tracks, chips, hovers, secondary buttons |
| `subtle` | | Quiet insets (user bubbles use #e9e9ee) |
| `separator` | | Hairlines |

- **Lime** is the signature accent, used sparingly: send, Approve, best fit, progress,
  focus. Tokens: `lime`, `lime-soft`, and `lime-deep` for lime-family text on white.
- **Data-flow colours** have fixed meanings: `private*` teal, `network*` blue, `cloud*`
  amber.
- No hard black borders.

## Type

Utilities, used instead of ad-hoc sizes:

| Utility | Size | Use |
|---|---|---|
| `type-large-title` | 30 | Page titles |
| `type-title` | 22 | |
| `type-headline` | 17, semibold | |
| `type-body` | 15 | |
| `type-callout` | 14 | |
| `type-subhead` | 13 | |
| `type-footnote` | 12 | |

They're named `type-*`, not `text-*`, on purpose: the `cn()` merger treats unknown
`text-*` classes as colours and silently drops a size combined with a text colour.

Section headings use `section-label` (13, semibold, grey), never mono uppercase labels.
Mono is only for things the user literally types (e.g. `/newbot`).

## Surfaces and elevation

- `surface`: white, 18px radius, `--shadow-card`.
- `grouped`: an iOS grouped list: white, 16px radius, hairline separators between
  children.
- `material` / `material-thick`: translucent + blur, for bars and floating controls.
- The top bar's is `bar-material`, faded in by opacity only: never transition
  `backdrop-filter`, and keep scroll state out of the shell's React state
  (`lib/scroll-edge.ts`). The bar gains its material and hairline only once something is
  scrolled under it (`Page` reports the scroll edge).
- Elevation: `--shadow-card` < `--shadow-raised` < `--shadow-float` (popovers, sheets),
  never borders.
- Radii: controls 10, small cards 14, cards 18, sheets 22, composer 24.
- To override a utility's background or shadow on the same element, use the important
  suffix (`bg-subtle!`).

## Components

Page pieces (`src/components/page.tsx`):

- `Page`: the full-height scroll container under the top bar; reports the scroll edge so
  the bar gains its material and hairline.
- `PageHeader`: large title + one-line subtitle (plus an optional action).
- `Section`: label + optional action.
- `Grouped` + `Row`: icon tile, title, detail, trailing controls; pass `onClick` for a
  selectable row.
- `IconTile`: tinted rounded-square icon, sm/md/lg.
- `Pill`: small capsule label.

Buttons (`components/ui/button.tsx`): `default` (dark, primary), `lime` (signature
positive action), `secondary` (grey fill), `ghost`, `outline`, `destructive`. All press
with a slight scale.

Put destructive and rarely used actions in a "…" `DropdownMenu`, not in the row. Confirm
destructive actions with the centred, macOS-style `AlertDialog` (`AlertDialogAction
variant="destructive"`).

New shadcn/ui components are added with `pnpm dlx shadcn@latest add <name>` from
`apps/desktop`; icons are lucide.

## Spacing

4/8pt grid. Pages: max 880px wide, 24px side padding, 40px between sections, 10px
between a section label and its content, 12px grid gaps.

## Panels

- Panels are full-height screens under the top bar.
- Master–detail screens (People, Settings) use a left column on a translucent white
  (`bg-[rgb(255_255_255/0.45)]`, hairline on the right, starting 68–76px down so it
  clears the bar) and the detail as a normal `Page` beside it.
- Lists in a column are arrow-key navigable (`role="listbox"` / sidebar `nav`), with the
  selection in grey fill, not lime.
- **Calendar**: colours come from the daemon (`CalendarInfo.color`: the server's own,
  else a stable palette pick; see [Calendar](calendar.md)). Events are drawn as a tint of
  that colour with a 3px left bar; the "now" line and today's date are red (#ff3b30), as
  calendar apps do. Anything read from a calendar (titles, places, notes, attendees) is
  plain text: never linkified, never markdown.

What the panels do is in [Frontend › Panels](frontend.md#panels).

## Right-click menus

`components/app-context-menu.tsx`, `components/ui/context-menu.tsx`. Every right-click
opens one of Mimi's menus, never the webview's (Back, Reload…).

- Things with their own menu wrap themselves in `ContextMenu`: a mail conversation, a
  message, a smart folder, a chat message, a person. `ThingMenu` gives "Open / Ask about
  this".
- In Calendar:
  - an event: open, remind me before (upcoming only), remove its reminders, ask, copy
    details, then edit, send invitations (the user's own events with guests) and delete
    for calendars written from here, hide its calendar;
  - a reminder or routine: `ItemMenu`;
  - an empty slot of the week grid: new event at that time, new reminder, navigation,
    view.
- The innermost menu wins. `AppContextMenu` around the shell covers the rest: Copy / Ask
  about the selected text, New chat, Search, Settings.
- Text fields keep the system's editing menu.
- The email frame re-dispatches its right-clicks to the page (with the link or selection
  under them) so the message's menu opens there (see [Email](email.md)).

## Motion

The `motion` library (`motion/react`):

- Springs, not linear easing (typical stiffness 380–520, damping 30–38).
- Section changes fade and rise 6px; new messages and approval cards rise in; popovers
  and sheets scale in from 0.96–0.97.
- Don't wrap streaming content (message text) in layout animations.
- Everything respects `prefers-reduced-motion` (`MotionConfig reducedMotion="user"` plus
  a CSS override).

## Chat

- No avatars. User messages are grey bubbles on the right; replies are plain text.
- The composer floats over the thread; the thread pads itself by the composer's measured
  height.
- The composer's toolbar keeps round icon buttons on the left (`@` mentions, `+`
  more/actions, also opened by typing `/`) and the privacy chip plus the round send/stop
  button on the right.
- Model switching lives in Models, not in the composer.

## The assistant's character

`components/assistant-avatar.tsx`: a lime mochi with a sprout, never named or lettered.

- `AssistantAvatar` (`size`, `mood`: idle, thinking, happy, listening, sleepy) is used
  sparingly: the welcome (happy, then idle), the empty new chat, the "Thinking" status
  before a reply starts (gone once text arrives, so it's not an avatar on messages) and
  the offline screen (sleepy).
- `AssistantGlyph` is the still version inside `LogoMark`.
- The app icon is the same drawing: `apps/desktop/src-tauri/icons/source/` (master SVGs +
  `regenerate.sh`). Change them together.

## Desktop window

- **macOS**: the title bar is an overlay (hidden title). The top bar leaves room for the
  traffic lights (`macOverlayTitleBar` in `lib/platform.ts`) and is a
  `data-tauri-drag-region`.
- **Linux**: the window has no system title bar at all (`set_decorations(false)` at
  startup: GTK's would only say "Mimi" above our bar). The top bar draws
  minimize/maximize/close (`WindowControls`, from the `window_chrome` command), except
  under tiling window managers (Hyprland, Sway, i3, niri…), which manage windows
  themselves.
