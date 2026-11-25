import { useCallback, useEffect, useMemo, useState } from "react";
import { Check, Code2, Crosshair, Moon, Plus, Sliders, Sparkles, Sun, Trash2, X } from "lucide-react";

import { clearThemeDraftPreview, setThemeDraftPreview } from "../../lib/appearanceSettings";
import type { ThemeDocumentV1 } from "../../lib/themeDocument";
import {
  createVividThemeColors,
  getStandardThemeColors,
  isReservedThemeId,
  resolveDocumentRoles,
  THEME_CSS_MAX_LENGTH,
  THEME_ROLE_GROUPS,
  themeColorsToFerryxTokens,
  themeColorToHex,
  themeIdFromName,
  type ThemeAppearance,
  type ThemeColorRole,
  type ThemeColors,
} from "../../lib/themePalettes";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { ThemeColorField } from "./ThemeColorPicker";
import { ThemeCssEditor } from "./ThemeCssEditor";
import {
  clearThemeInspectorOverlay,
  findRoleGroupId,
  inspectThemeRoleAtElement,
  showThemeInspectorOverlay,
} from "./themeInspector";
import { ThemePreviewCircles } from "./ThemePreviewCircles";

export type ThemeEditorPanelProps = {
  initialTheme?: ThemeDocumentV1 | null;
  defaultAppearance: ThemeAppearance;
  existingIds: ReadonlySet<string>;
  onSave: (doc: ThemeDocumentV1) => void;
  onCancel: () => void;
};

type EditorTab = "quick" | "advanced" | "css";

function buildDraftDocument(params: {
  id: string;
  name: string;
  base: ThemeAppearance;
  primaryColors: ThemeColors;
  oppositeColors: ThemeColors | null;
  seed: { canvas: string; accent: string };
  css: string;
}): ThemeDocumentV1 {
  const other: ThemeAppearance = params.base === "light" ? "dark" : "light";
  const tokens = themeColorsToFerryxTokens(params.primaryColors);
  const variants = params.oppositeColors
    ? { [other]: themeColorsToFerryxTokens(params.oppositeColors) }
    : undefined;
  const variantRoles = params.oppositeColors
    ? { [other]: { ...params.oppositeColors } }
    : undefined;
  return {
    version: 1,
    id: params.id,
    name: params.name.trim() || "Custom theme",
    base: params.base,
    tokens,
    roles: { ...params.primaryColors },
    ...(variants ? { variants } : {}),
    ...(variantRoles ? { variantRoles } : {}),
    seed: params.seed,
    ...(params.css.trim() ? { css: params.css, cssScope: "app" as const } : {}),
  };
}

export function ThemeEditorPanel({
  initialTheme,
  defaultAppearance,
  existingIds,
  onSave,
  onCancel,
}: ThemeEditorPanelProps) {
  const isEditingExisting = Boolean(initialTheme);
  const initialBase: ThemeAppearance = initialTheme?.base ?? defaultAppearance;
  const initialPrimaryRoles = useMemo(
    () =>
      initialTheme
        ? resolveDocumentRoles(initialTheme, initialBase)
        : createVividThemeColors(
            initialBase,
            initialBase === "dark" ? "#161821" : "#faf8fb",
            initialBase === "dark" ? "#7aa2f7" : "#4f46e5",
          ),
    [initialTheme, initialBase],
  );
  const initialOtherMode: ThemeAppearance = initialBase === "light" ? "dark" : "light";
  const initialHasOpposite = Boolean(
    initialTheme?.variantRoles?.[initialOtherMode] || initialTheme?.variants?.[initialOtherMode],
  );
  const initialOppositeRoles = useMemo(
    () =>
      initialTheme && initialHasOpposite
        ? resolveDocumentRoles(initialTheme, initialOtherMode)
        : null,
    [initialTheme, initialHasOpposite, initialOtherMode],
  );

  const [name, setName] = useState(initialTheme?.name ?? "My custom theme");
  const [base, setBase] = useState<ThemeAppearance>(initialBase);
  const [editingMode, setEditingMode] = useState<ThemeAppearance>(initialBase);
  const [primaryColors, setPrimaryColors] = useState<ThemeColors>(initialPrimaryRoles);
  const [oppositeColors, setOppositeColors] = useState<ThemeColors | null>(initialOppositeRoles);
  const [css, setCss] = useState(initialTheme?.css ?? "");
  const [tab, setTab] = useState<EditorTab>("quick");
  const [activeGroup, setActiveGroup] = useState<string>("surfaces");
  const [highlightedRole, setHighlightedRole] = useState<ThemeColorRole | null>(null);
  const [inspecting, setInspecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const otherMode: ThemeAppearance = base === "light" ? "dark" : "light";
  const activeColors = editingMode === base ? primaryColors : (oppositeColors ?? primaryColors);

  const resolvedId = useMemo(() => {
    if (isEditingExisting && initialTheme) return initialTheme.id;
    let candidate = themeIdFromName(name || "custom-theme");
    if (isReservedThemeId(candidate)) candidate = `${candidate}-custom`;
    let unique = candidate;
    let counter = 2;
    while (existingIds.has(unique) || isReservedThemeId(unique)) {
      unique = `${candidate.slice(0, 58)}-${counter++}`;
    }
    return unique;
  }, [existingIds, initialTheme, isEditingExisting, name]);

  const draftDoc = useMemo(
    () =>
      buildDraftDocument({
        id: resolvedId,
        name,
        base,
        primaryColors,
        oppositeColors,
        seed: {
          canvas: themeColorToHex(primaryColors.canvas) ?? "#161821",
          accent: themeColorToHex(primaryColors.accent) ?? "#7aa2f7",
        },
        css,
      }),
    [resolvedId, name, base, primaryColors, oppositeColors, css],
  );

  useEffect(() => {
    setThemeDraftPreview(draftDoc, editingMode);
  }, [draftDoc, editingMode]);

  useEffect(() => {
    return () => {
      clearThemeInspectorOverlay();
      clearThemeDraftPreview();
    };
  }, []);

  useEffect(() => {
    if (!inspecting) {
      clearThemeInspectorOverlay();
      return;
    }
    const handleMove = (event: MouseEvent) => {
      const target = document.elementFromPoint(event.clientX, event.clientY);
      if (!target) {
        clearThemeInspectorOverlay();
        return;
      }
      const inspection = inspectThemeRoleAtElement(target);
      if (inspection) showThemeInspectorOverlay(inspection);
      else clearThemeInspectorOverlay();
    };
    const handleClick = (event: MouseEvent) => {
      const target = document.elementFromPoint(event.clientX, event.clientY);
      if (!target || target.closest("[data-theme-inspector-ignore]")) return;
      event.preventDefault();
      event.stopPropagation();
      const inspection = inspectThemeRoleAtElement(target);
      clearThemeInspectorOverlay();
      setInspecting(false);
      if (inspection) {
        setTab("advanced");
        setActiveGroup(findRoleGroupId(inspection.primaryRole));
        setHighlightedRole(inspection.primaryRole);
      }
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        clearThemeInspectorOverlay();
        setInspecting(false);
      }
    };
    window.addEventListener("mousemove", handleMove, true);
    window.addEventListener("click", handleClick, true);
    window.addEventListener("keydown", handleKeyDown, true);
    return () => {
      clearThemeInspectorOverlay();
      window.removeEventListener("mousemove", handleMove, true);
      window.removeEventListener("click", handleClick, true);
      window.removeEventListener("keydown", handleKeyDown, true);
    };
  }, [inspecting]);

  const setModeColors = useCallback(
    (next: ThemeColors) => {
      if (editingMode === base) setPrimaryColors(next);
      else setOppositeColors(next);
    },
    [base, editingMode],
  );

  const updateSeed = useCallback(
    (kind: "canvas" | "accent", value: string) => {
      const nextCanvas = kind === "canvas" ? value : activeColors.canvas;
      const nextAccent = kind === "accent" ? value : activeColors.accent;
      setModeColors(createVividThemeColors(editingMode, nextCanvas, nextAccent));
    },
    [activeColors.accent, activeColors.canvas, editingMode, setModeColors],
  );

  const updateRole = useCallback(
    (role: ThemeColorRole, value: string) => {
      setModeColors({ ...activeColors, [role]: value });
    },
    [activeColors, setModeColors],
  );

  const derivedBaseline = useMemo(
    () => createVividThemeColors(editingMode, activeColors.canvas, activeColors.accent),
    [editingMode, activeColors.canvas, activeColors.accent],
  );

  const handleSave = () => {
    const trimmed = name.trim();
    if (!trimmed) {
      setError("Theme name cannot be blank.");
      return;
    }
    if (trimmed.length > 64) {
      setError("Theme name must be 64 characters or fewer.");
      return;
    }
    if (css.length > THEME_CSS_MAX_LENGTH) {
      setError(`Theme CSS must be ${THEME_CSS_MAX_LENGTH.toLocaleString()} characters or fewer.`);
      return;
    }
    setError(null);
    clearThemeDraftPreview();
    onSave(draftDoc);
  };

  const handleCancel = () => {
    clearThemeDraftPreview();
    onCancel();
  };

  const lightColors = base === "light" ? primaryColors : oppositeColors ?? undefined;
  const darkColors = base === "dark" ? primaryColors : oppositeColors ?? undefined;

  return (
    <div
      data-theme-inspector-ignore="true"
      data-testid="theme-editor-panel"
      className="space-y-4 rounded-xl border border-border bg-panel p-4"
    >
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-3">
        <div className="flex items-center gap-3">
          <ThemePreviewCircles
            label={name || "Draft theme"}
            lightColors={lightColors}
            darkColors={darkColors}
            selectedLight={editingMode === "light"}
            selectedDark={editingMode === "dark"}
            onSelectAppearance={(mode) => {
              if (mode === base || oppositeColors) setEditingMode(mode);
            }}
          />
          <div>
            <h3 className="text-[13px] font-semibold text-foreground">
              {isEditingExisting ? `Edit theme: ${initialTheme?.name}` : "Create custom theme"}
            </h3>
            <p className="text-[11px] text-muted-foreground">
              Live preview active · ID: <code className="font-mono">{resolvedId}</code>
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant={inspecting ? "default" : "outline"}
            size="sm"
            onClick={() => setInspecting((prev) => !prev)}
            className="h-7 gap-1.5 px-2.5 text-[11px]"
          >
            <Crosshair className="size-3.5" aria-hidden="true" />
            {inspecting ? "Click any element (Esc)" : "Pick from app"}
          </Button>
          <Button type="button" variant="ghost" size="sm" onClick={handleCancel} className="h-7 gap-1 px-2 text-[11px]">
            <X className="size-3.5" aria-hidden="true" />
            Cancel
          </Button>
          <Button type="button" size="sm" onClick={handleSave} className="h-7 gap-1 px-2.5 text-[11px]">
            <Check className="size-3.5" aria-hidden="true" />
            Save theme
          </Button>
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-1">
          <label htmlFor="theme-editor-name" className="block text-[11px] font-medium text-muted-foreground">
            Theme name
          </label>
          <Input
            id="theme-editor-name"
            aria-label="Theme name"
            value={name}
            maxLength={64}
            onChange={(event) => setName(event.target.value)}
            className="h-8 text-[12px]"
          />
        </div>
        <div className="space-y-1">
          <span className="block text-[11px] font-medium text-muted-foreground">Appearance variants</span>
          <div className="flex flex-wrap items-center gap-1.5">
            <Button
              type="button"
              size="sm"
              variant={editingMode === base ? "default" : "outline"}
              onClick={() => setEditingMode(base)}
              className="h-8 gap-1 px-2.5 text-[11px]"
            >
              {base === "light" ? <Sun className="size-3" aria-hidden="true" /> : <Moon className="size-3" aria-hidden="true" />}
              {base === "light" ? "Light" : "Dark"} (Base)
            </Button>
            {oppositeColors ? (
              <>
                <Button
                  type="button"
                  size="sm"
                  variant={editingMode === otherMode ? "default" : "outline"}
                  onClick={() => setEditingMode(otherMode)}
                  className="h-8 gap-1 px-2.5 text-[11px]"
                >
                  {otherMode === "light" ? <Sun className="size-3" aria-hidden="true" /> : <Moon className="size-3" aria-hidden="true" />}
                  {otherMode === "light" ? "Light" : "Dark"}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  aria-label={`Remove ${otherMode} variant`}
                  onClick={() => {
                    setOppositeColors(null);
                    setEditingMode(base);
                  }}
                  className="h-8 px-2 text-[11px] text-muted-foreground hover:text-destructive"
                >
                  <Trash2 className="size-3.5" aria-hidden="true" />
                </Button>
              </>
            ) : (
              <>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    const nextBase: ThemeAppearance = base === "dark" ? "light" : "dark";
                    setBase(nextBase);
                    setEditingMode(nextBase);
                    setPrimaryColors(
                      createVividThemeColors(
                        nextBase,
                        nextBase === "dark" ? "#161821" : "#faf8fb",
                        primaryColors.accent,
                      ),
                    );
                  }}
                  className="h-8 px-2 text-[11px]"
                >
                  Switch base to {otherMode}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    setOppositeColors(
                      createVividThemeColors(
                        otherMode,
                        otherMode === "light" ? "#faf8fb" : "#161821",
                        activeColors.accent,
                      ),
                    );
                    setEditingMode(otherMode);
                  }}
                  className="h-8 gap-1 px-2 text-[11px]"
                >
                  <Plus className="size-3" aria-hidden="true" />
                  Add {otherMode} variant
                </Button>
              </>
            )}
          </div>
        </div>
      </div>

      <div role="tablist" aria-label="Theme editor tabs" className="flex gap-1 border-b border-border pb-2">
        <Button
          type="button"
          role="tab"
          aria-selected={tab === "quick"}
          variant={tab === "quick" ? "secondary" : "ghost"}
          size="sm"
          onClick={() => setTab("quick")}
          className="h-7 gap-1.5 px-2.5 text-[11px]"
        >
          <Sparkles className="size-3" aria-hidden="true" />
          Palette (OKLCH)
        </Button>
        <Button
          type="button"
          role="tab"
          aria-selected={tab === "advanced"}
          variant={tab === "advanced" ? "secondary" : "ghost"}
          size="sm"
          onClick={() => setTab("advanced")}
          className="h-7 gap-1.5 px-2.5 text-[11px]"
        >
          <Sliders className="size-3" aria-hidden="true" />
          Advanced tokens
        </Button>
        <Button
          type="button"
          role="tab"
          aria-selected={tab === "css"}
          variant={tab === "css" ? "secondary" : "ghost"}
          size="sm"
          onClick={() => setTab("css")}
          className="h-7 gap-1.5 px-2.5 text-[11px]"
        >
          <Code2 className="size-3" aria-hidden="true" />
          Custom CSS{css.trim() ? " •" : ""}
        </Button>
      </div>

      {tab === "quick" ? (
        <div className="space-y-3">
          <p className="text-[11px] text-muted-foreground">
            Pick Canvas and Accent colors. Ferryx derives all 61 surface, border, text, sidebar, chat, and terminal roles in OKLCH with WCAG contrast guarantees.
          </p>
          <div className="grid gap-2 sm:grid-cols-2">
            <ThemeColorField
              id="theme-editor-seed-canvas"
              label="Canvas"
              value={activeColors.canvas}
              defaultValue={getStandardThemeColors(editingMode).canvas}
              onChange={(value) => updateSeed("canvas", value)}
              onReset={() => updateSeed("canvas", getStandardThemeColors(editingMode).canvas)}
            />
            <ThemeColorField
              id="theme-editor-seed-accent"
              label="Accent"
              value={activeColors.accent}
              defaultValue={getStandardThemeColors(editingMode).accent}
              onChange={(value) => updateSeed("accent", value)}
              onReset={() => updateSeed("accent", getStandardThemeColors(editingMode).accent)}
            />
          </div>
          <div className="grid grid-cols-3 gap-2 pt-1 sm:grid-cols-6">
            {(
              [
                ["Canvas", activeColors.canvas],
                ["Surface", activeColors.surface],
                ["Sidebar", activeColors.sidebar],
                ["Accent", activeColors.accent],
                ["Action", activeColors.messageAction],
                ["Terminal", activeColors.terminalBackground],
              ] as const
            ).map(([swatchLabel, color]) => (
              <div key={swatchLabel} className="rounded-md border border-border bg-background p-1.5 text-center">
                <div
                  className="mb-1 h-5 w-full rounded border border-border"
                  style={{ backgroundColor: themeColorToHex(color) ?? color }}
                />
                <span className="block truncate text-[11px] text-muted-foreground">{swatchLabel}</span>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      {tab === "advanced" ? (
        <div className="space-y-3">
          <div className="flex flex-wrap gap-1">
            {THEME_ROLE_GROUPS.map((group) => (
              <Button
                key={group.id}
                type="button"
                size="sm"
                variant={activeGroup === group.id ? "secondary" : "ghost"}
                onClick={() => setActiveGroup(group.id)}
                className="h-7 px-2 text-[11px]"
              >
                {group.label}
              </Button>
            ))}
          </div>
          <div className="grid gap-2 sm:grid-cols-2">
            {(THEME_ROLE_GROUPS.find((g) => g.id === activeGroup) ?? THEME_ROLE_GROUPS[0]).roles.map((item) => (
              <ThemeColorField
                key={item.role}
                id={`theme-role-${item.role}`}
                label={item.label}
                value={activeColors[item.role]}
                defaultValue={derivedBaseline[item.role]}
                highlighted={highlightedRole === item.role}
                onChange={(value) => updateRole(item.role, value)}
                onReset={() => updateRole(item.role, derivedBaseline[item.role])}
              />
            ))}
          </div>
        </div>
      ) : null}

      {tab === "css" ? <ThemeCssEditor value={css} onChange={setCss} /> : null}

      {error ? (
        <p role="alert" className="text-[12px] text-destructive">
          {error}
        </p>
      ) : null}
    </div>
  );
}
