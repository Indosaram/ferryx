import { cn } from "./cn";

/**
 * The floating surface: dialogs, sheets, and menus that sit above a scrim.
 *
 * Composition, not a CSS component class. This bundle has no `@apply` or
 * `@layer components` precedent, and utilities would outrank a component class
 * anyway; routing through `cn` (twMerge) lets a caller's own utility still win.
 *
 * --popover is one step above the content surface and clearly above chrome
 * (--card). Every site must drop its own bg-card/shadow-* when it uses this, or
 * tailwind-merge keeps the caller's class and the fill silently reverts.
 */
export function floatingSurface(...classes: Array<string | undefined>) {
  return cn("border border-border bg-popover text-popover-foreground shadow-elevation-4", ...classes);
}
