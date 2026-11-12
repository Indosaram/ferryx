import { Palette, RotateCcw } from "lucide-react";

import {
  useAppearanceSettings,
  useResolvedTheme,
  type AppearanceDensity,
} from "../../lib/appearanceSettings";
import { Button } from "../ui/button";
import { SegmentedControl } from "../ui/SegmentedControl";
import { SettingBlock, SettingsGroup, SettingsHeading } from "./primitives";
import { AccentSwatchRow, ThemePreviewStrip } from "./ThemePreviewCard";

const DENSITY_OPTIONS: ReadonlyArray<{ value: AppearanceDensity; label: string }> = [
  { value: "compact", label: "Compact" },
  { value: "comfortable", label: "Comfortable" },
];

export function AppearanceSection() {
  const { settings, updateSettings, resetSettings } = useAppearanceSettings();
  const resolvedTheme = useResolvedTheme(settings.theme);

  return (
    <section aria-labelledby="settings-appearance-heading" aria-label="Appearance">
      <SettingsHeading
        icon={<Palette />}
        title="Appearance"
        description="Pick a theme and accent for the desktop interface. Changes apply immediately and follow you into the terminal."
      />
      <h2 id="settings-appearance-heading" className="sr-only">
        Appearance
      </h2>
      <SettingsGroup
        title="Theme"
        action={
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={resetSettings}
            className="no-drag h-7 shrink-0 gap-1.5 px-2 text-[11px] text-muted-foreground hover:text-foreground"
          >
            <RotateCcw className="size-3" />
            Reset to defaults
          </Button>
        }
      >
        <SettingBlock
          label="Theme mode"
          description={
            settings.theme === "system"
              ? `Follows your OS appearance, terminal included. Currently ${resolvedTheme === "light" ? "Light" : "Dark"}.`
              : "The base palette for the whole app, settings included."
          }
        >
          <ThemePreviewStrip value={settings.theme} onChange={(theme) => updateSettings({ theme })} />
        </SettingBlock>

        <SettingBlock
          label="Accent color"
          description="Active badges, selection rings, and indicators draw from this colour."
        >
          <AccentSwatchRow
            value={settings.accentColor}
            onChange={(accentColor) => updateSettings({ accentColor })}
          />
        </SettingBlock>

        <SettingBlock
          label="Interface density"
          description="Spacing and padding across tabs, sidebars, and dialog chrome."
        >
          <SegmentedControl
            label="Interface density"
            value={settings.density}
            options={DENSITY_OPTIONS}
            onChange={(density) => updateSettings({ density })}
            className="w-[240px] max-w-full bg-background"
          />
        </SettingBlock>
      </SettingsGroup>
    </section>
  );
}

export { AppearanceSection as AppearanceSettings };
export default AppearanceSection;
