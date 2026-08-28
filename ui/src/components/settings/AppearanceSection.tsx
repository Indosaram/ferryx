import { Palette, RotateCcw } from "lucide-react";

import {
  useAppearanceSettings,
  type AppearanceSettingsState,
} from "../../lib/appearanceSettings";
import { Button } from "../ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";
import { SettingRow, SettingsHeading } from "./primitives";

export function AppearanceSection() {
  const { settings, updateSettings, resetSettings } = useAppearanceSettings();

  return (
    <section aria-labelledby="settings-appearance-heading">
      <SettingsHeading
        icon={<Palette />}
        title="Appearance"
        description="Customize the desktop theme palette, accent colors, and interface scale density."
      />
      <h2 id="settings-appearance-heading" className="sr-only">
        Appearance
      </h2>
      <div className="mb-5 flex items-center justify-between border-b border-border pb-3">
        <h3 className="text-[12px] font-semibold">Display & Styling</h3>
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
      </div>

      <div className="border-y border-border">
        <SettingRow
          label="Theme mode"
          description="Select the base color theme for the desktop interface."
        >
          <Select
            value={settings.theme}
            onValueChange={(val) =>
              updateSettings({
                theme: val as AppearanceSettingsState["theme"],
              })
            }
          >
            <SelectTrigger
              id="appearance-theme-mode"
              aria-label="Theme mode"
              className="h-8 w-[180px] text-xs"
            >
              <SelectValue placeholder="Theme mode" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="charcoal">Charcoal (Default)</SelectItem>
              <SelectItem value="dark">Dark</SelectItem>
              <SelectItem value="light">Light</SelectItem>
              <SelectItem value="system">System</SelectItem>
            </SelectContent>
          </Select>
        </SettingRow>

        <SettingRow
          label="Accent color"
          description="Accent highlight color used for active badges, selection rings, and indicators."
        >
          <Select
            value={settings.accentColor}
            onValueChange={(val) =>
              updateSettings({
                accentColor: val as AppearanceSettingsState["accentColor"],
              })
            }
          >
            <SelectTrigger
              id="appearance-accent-color"
              aria-label="Accent color"
              className="h-8 w-[180px] text-xs"
            >
              <SelectValue placeholder="Accent color" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="default">Slate (Default)</SelectItem>
              <SelectItem value="blue">Ocean Blue</SelectItem>
              <SelectItem value="emerald">Emerald</SelectItem>
              <SelectItem value="purple">Violet</SelectItem>
              <SelectItem value="amber">Amber</SelectItem>
              <SelectItem value="rose">Rose</SelectItem>
            </SelectContent>
          </Select>
        </SettingRow>

        <SettingRow
          label="Interface density"
          description="Adjust spacing and padding across tabs, sidebars, and dialog chrome."
        >
          <Select
            value={settings.density}
            onValueChange={(val) =>
              updateSettings({
                density: val as AppearanceSettingsState["density"],
              })
            }
          >
            <SelectTrigger
              id="appearance-density"
              aria-label="Interface density"
              className="h-8 w-[180px] text-xs"
            >
              <SelectValue placeholder="Interface density" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="compact">Compact (Default)</SelectItem>
              <SelectItem value="comfortable">Comfortable</SelectItem>
            </SelectContent>
          </Select>
        </SettingRow>
      </div>
    </section>
  );
}

export { AppearanceSection as AppearanceSettings };
export default AppearanceSection;
