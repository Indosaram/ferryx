import { Command } from "lucide-react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/Card";

export function Shortcuts() {
  const shortcutGroups = [
    {
      category: "Navigation & Tabs",
      items: [
        { keys: ["⌘", "T"], action: "Open New Terminal Tab" },
        { keys: ["⌘", "⇧", "B"], action: "Open New Browser Tab" },
        { keys: ["⌘", "W"], action: "Close Focused Tab / Pane" },
        { keys: ["⌘", "1..9"], action: "Direct Tab Switch" },
        { keys: ["⌃", "Tab"], action: "Cycle Through Tabs" },
      ],
    },
    {
      category: "Split Pane & Layout",
      items: [
        { keys: ["⌘", "D"], action: "Split Terminal Horizontally" },
        { keys: ["⌘", "⇧", "D"], action: "Split Terminal Vertically" },
        { keys: ["⌘", "["], action: "Focus Previous Split Pane" },
        { keys: ["⌘", "]"], action: "Focus Next Split Pane" },
        { keys: ["⌘", "⇧", "Enter"], action: "Toggle Pane Maximize" },
      ],
    },
    {
      category: "Agent & Workspace",
      items: [
        { keys: ["⌘", "K"], action: "Command Palette / Quick Launch" },
        { keys: ["⌘", "⇧", "P"], action: "Switch Project Workspace" },
        { keys: ["⌘", "R"], action: "Reload Webview Companion" },
        { keys: ["⌘", "⇧", "C"], action: "Copy Terminal Output / Buffer" },
        { keys: ["⌘", ","], action: "Preferences & Daemon Settings" },
      ],
    },
  ];

  return (
    <section id="shortcuts" className="py-20 border-t border-zinc-800/60 bg-zinc-950 relative">
      <div className="mx-auto max-w-6xl px-4 sm:px-6">
        <div className="text-center max-w-2xl mx-auto mb-16">
          <h2 className="text-3xl sm:text-4xl font-bold tracking-tight text-zinc-100">
            Intuitive Keyboard Controls
          </h2>
          <p className="mt-4 text-zinc-400 text-base">
            Keep your hands on the keyboard. Ferryx features zero-friction shortcuts designed for power terminal and agent users.
          </p>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
          {shortcutGroups.map((group, idx) => (
            <Card key={idx} className="bg-zinc-900/30 border-zinc-800">
              <CardHeader className="p-6 pb-3 border-b border-zinc-800/60 bg-zinc-900/40">
                <CardTitle className="text-base font-semibold text-zinc-200 flex items-center justify-between">
                  <span>{group.category}</span>
                  <Command className="h-4 w-4 text-zinc-500" />
                </CardTitle>
              </CardHeader>
              <CardContent className="p-6 pt-4 space-y-4">
                {group.items.map((item, itemIdx) => (
                  <div key={itemIdx} className="flex items-center justify-between text-xs">
                    <span className="text-zinc-400">{item.action}</span>
                    <div className="flex items-center space-x-1">
                      {item.keys.map((key, kIdx) => (
                        <kbd
                          key={kIdx}
                          className="rounded border border-zinc-700 bg-zinc-800/80 px-2 py-1 text-[11px] font-semibold text-zinc-200 shadow-sm"
                        >
                          {key}
                        </kbd>
                      ))}
                    </div>
                  </div>
                ))}
              </CardContent>
            </Card>
          ))}
        </div>
      </div>
    </section>
  );
}
