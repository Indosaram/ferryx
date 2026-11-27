import { useCallback, useEffect, useState } from "react";
import { CheckCircle2, Mic, Trash2 } from "lucide-react";

import { invoke } from "@tauri-apps/api/core";

import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { SettingRow, SettingsGroup, SettingsHeading } from "./primitives";

interface SpeechKeyState {
  readonly saved: boolean;
  readonly source: "file" | "environment" | null;
}

interface VoiceStatus {
  readonly available: boolean;
  readonly sttSource: "key" | "codex" | null;
  readonly speakAvailable: boolean;
  readonly speakReason: string | null;
}

type SaveState =
  | { readonly kind: "idle" }
  | { readonly kind: "saving" }
  | { readonly kind: "saved"; readonly source: string }
  | { readonly kind: "error"; readonly message: string };

export function VoiceSection() {
  const [keyState, setKeyState] = useState<SpeechKeyState | null>(null);
  const [voice, setVoice] = useState<VoiceStatus | null>(null);
  const [draft, setDraft] = useState("");
  const [save, setSave] = useState<SaveState>({ kind: "idle" });

  const readKeyState = useCallback(async () => {
    try {
      setKeyState(await invoke<SpeechKeyState>("cmd_voice_call_key_state"));
    } catch {
      setKeyState(null);
    }
  }, []);

  const readStatus = useCallback(async () => {
    try {
      setVoice(await invoke<VoiceStatus>("cmd_voice_call_status"));
    } catch {
      setVoice(null);
    }
  }, []);

  useEffect(() => {
    void readKeyState();
    void readStatus();
  }, [readKeyState, readStatus]);

  const submit = useCallback(async (value: string) => {
    setSave({ kind: "saving" });
    try {
      const next = await invoke<SpeechKeyState>("cmd_voice_call_set_key", { apiKey: value });
      setKeyState(next);
      setDraft("");
      setSave(next.saved ? { kind: "saved", source: "your key" } : { kind: "idle" });
      // A saved key starts paying for speech, so the capability line has to be re-read. The key
      // state itself comes from the save response: re-reading it here would race the write.
      await readStatus();
    } catch (error) {
      setSave({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    }
  }, [readStatus]);

  const saved = keyState?.saved === true;
  const codex = voice?.sttSource === "codex";

  return (
    <>
      <SettingsHeading
        icon={<Mic />}
        title="Voice"
        description="Speak to a conversation instead of typing. Recording and playback stay on this machine."
      />
      <SettingsGroup title="Speech provider">
        <SettingRow
          label="API key"
          description={
            saved
              ? keyState.source === "environment"
                ? "Using a key from this process's environment."
                : "Your key is saved on this machine and is never shown again."
              : codex
                ? "Not needed: your Codex sign-in covers dictation and the realtime voice that speaks replies. A key only replaces the dictation provider."
                : "Run `codex login` on this machine: dictation and spoken replies (realtime voice) both use that sign-in. A key only replaces the dictation provider."
          }
        >
          {saved ? (
            <div className="flex items-center gap-2">
              <span className="flex items-center gap-1.5 text-[12px] text-muted-foreground">
                <CheckCircle2 className="size-3.5 text-status-success" />
                Saved
              </span>
              {keyState.source === "file" ? (
                <Button variant="ghost" size="sm" onClick={() => void submit("")} aria-label="Remove the saved key">
                  <Trash2 className="size-3.5" />
                  Remove
                </Button>
              ) : null}
            </div>
          ) : (
            <form
              className="flex items-center gap-2"
              onSubmit={(event) => {
                event.preventDefault();
                if (draft.trim() !== "") void submit(draft);
              }}
            >
              <Input
                type="password"
                autoComplete="off"
                spellCheck={false}
                placeholder="sk-…"
                aria-label="Speech provider API key"
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
                className="h-8 w-[260px] font-mono text-[12px]"
              />
              <Button type="submit" size="sm" disabled={draft.trim() === "" || save.kind === "saving"}>
                {save.kind === "saving" ? "Saving…" : "Save"}
              </Button>
            </form>
          )}
        </SettingRow>
        <SettingRow
          label="Billing"
          description={
            voice?.speakAvailable
              ? "Dictation and spoken replies are covered by your Codex sign-in. Replies are spoken by the OpenAI realtime voice."
              : "Spoken replies use the realtime voice on your Codex sign-in. Until you sign in, replies arrive as text."
          }
        >
          <span className="text-[12px] text-muted-foreground">
            {voice?.speakAvailable ? "Codex sign-in" : "Text replies only"}
          </span>
        </SettingRow>
        {save.kind === "error" ? (
          <p role="alert" className="px-4 py-2 text-[12px] text-destructive">
            {save.message}
          </p>
        ) : null}
      </SettingsGroup>
    </>
  );
}
