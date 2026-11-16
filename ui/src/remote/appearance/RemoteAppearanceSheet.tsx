import { Palette } from "lucide-react";
import { useState } from "react";
import { cycleAppearanceTheme, openAppearanceSurface } from "../../lib/appearanceCommands";
import { useAppearanceSettings, type AppearanceAccent, type AppearanceTheme } from "../../lib/appearanceSettings";
import { scopeCustomCss, type CustomCssError } from "../../lib/customThemeCss";
import { MobileSheet } from "../chat/MobileSheet";

const CONTROL = "min-h-11 w-full rounded-lg border border-chat-border bg-chat-surface px-3 text-base text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";
const THEMES: AppearanceTheme[] = ["charcoal", "dark", "light", "system"];
const ACCENTS: AppearanceAccent[] = ["default", "blue", "emerald", "purple", "amber", "rose"];

export function RemoteAppearanceButton({ open, onOpen }: { open: boolean; onOpen: () => void }) {
  return (
    <button
      type="button"
      aria-label="Appearance"
      title="Appearance"
      aria-haspopup="dialog"
      aria-expanded={open}
      onClick={() => openAppearanceSurface(onOpen)}
      className="flex size-11 shrink-0 items-center justify-center rounded-lg text-chat-foreground-secondary hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
    >
      <Palette aria-hidden="true" className="size-4" />
    </button>
  );
}

function AppearanceControls() {
  const { settings, invalidThemes, updateSettings } = useAppearanceSettings();
  const [css, setCss] = useState(settings.customCss);
  const [errors, setErrors] = useState<CustomCssError[]>([]);
  return (
    <div className="space-y-4 px-2 pb-3">
      <label className="block space-y-1 text-sm text-chat-foreground">
        <span>Theme</span>
        <select
          aria-label="Theme"
          className={CONTROL}
          value={settings.activeCustomThemeId ? `custom:${settings.activeCustomThemeId}` : settings.theme}
          onChange={(event) => {
            const value = event.target.value;
            updateSettings(value.startsWith("custom:")
              ? { activeCustomThemeId: value.slice(7) }
              : { theme: value as AppearanceTheme, activeCustomThemeId: null });
          }}
        >
          <optgroup label="Built-in themes">
            {THEMES.map((theme) => <option key={theme} value={theme}>{theme[0].toUpperCase() + theme.slice(1)}</option>)}
          </optgroup>
          {settings.customThemes.length > 0 ? (
            <optgroup label="Saved themes">
              {settings.customThemes.map((theme) => <option key={theme.id} value={`custom:${theme.id}`}>{theme.name}</option>)}
            </optgroup>
          ) : null}
        </select>
      </label>
      <button type="button" className={CONTROL} onClick={() => cycleAppearanceTheme(settings, updateSettings)}>Cycle theme</button>
      <label className="block space-y-1 text-sm text-chat-foreground">
        <span>Accent</span>
        <select aria-label="Accent" className={CONTROL} value={settings.accentColor} onChange={(event) => updateSettings({ accentColor: event.target.value as AppearanceAccent })}>
          {ACCENTS.map((accent) => <option key={accent} value={accent}>{accent[0].toUpperCase() + accent.slice(1)}</option>)}
        </select>
      </label>
      {invalidThemes.map((row, index) => (
        <div key={index} role="alert" className="rounded-lg border border-chat-border p-3 text-sm text-chat-foreground">
          <p>Saved appearance could not be loaded. The built-in theme remains available.</p>
          <ul>{row.errors.map((error, errorIndex) => <li key={errorIndex}>{error.path}: {error.message}</li>)}</ul>
        </div>
      ))}
      <label className="flex min-h-11 cursor-pointer items-center gap-3 text-sm text-chat-foreground">
        <input type="checkbox" className="size-5" checked={settings.renderMath} onChange={(event) => updateSettings({ renderMath: event.target.checked })} />
        Render math in chat
      </label>
      <form className="space-y-2" onSubmit={(event) => {
        event.preventDefault();
        const result = scopeCustomCss(css, settings.activeCustomThemeId ?? "custom-css");
        if (!result.ok) { setErrors(result.errors); return; }
        setErrors([]);
        updateSettings({ customCss: css });
      }}>
        <label className="block space-y-1 text-sm text-chat-foreground">
          <span>Custom CSS</span>
          <textarea aria-label="Custom CSS" aria-describedby="remote-custom-css-hint" aria-invalid={errors.length > 0} className={`${CONTROL} min-h-28 py-3 font-mono`} value={css} onChange={(event) => setCss(event.target.value)} spellCheck={false} autoCapitalize="none" />
        </label>
        <p id="remote-custom-css-hint" className="text-sm text-chat-foreground-secondary">Applies only inside chat, after the saved theme's CSS. Appearance controls stay outside chat.</p>
        {errors.length > 0 ? <ul role="alert" className="text-sm text-chat-foreground">{errors.map((error, index) => <li key={index}>{error.path}: {error.message}</li>)}</ul> : null}
        <button type="submit" className={CONTROL}>Apply custom CSS</button>
        <button type="button" className={CONTROL} onClick={() => { setCss(""); setErrors([]); updateSettings({ customCss: "" }); }}>Remove custom CSS</button>
      </form>
    </div>
  );
}

export function RemoteAppearanceSheet({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <MobileSheet open={open} onOpenChange={onOpenChange} title="Appearance" ariaLabel="Appearance" testId="remote-appearance-sheet">
      {open ? <AppearanceControls /> : null}
    </MobileSheet>
  );
}
