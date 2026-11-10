import { useEffect, useMemo, useRef, useState } from "react";
import type { VoiceCallPort, VoiceCallStatus } from "./voiceCall";
import type { ConversationHost } from "./voiceConversation";
import { createReplyWaiter } from "./replyWaiter";
import type { MobileChatMessageProps } from "../remote/chat/MobileChatMessage";
import type { VoiceConversationProps } from "../components/supervisor/VoiceConversationButton";

export function lastAssistantMessage(messages: readonly MobileChatMessageProps[]): MobileChatMessageProps | null {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message && message.role === "assistant" && message.content.trim() !== "") return message;
  }
  return null;
}

export function useVoiceConversation(input: {
  readonly port: VoiceCallPort | null;
  readonly send: (text: string) => unknown;
  readonly messages?: readonly MobileChatMessageProps[];
}): VoiceConversationProps | undefined {
  const { port, messages } = input;
  const [status, setStatus] = useState<VoiceCallStatus | null>(null);
  const [settled, setSettled] = useState(false);
  const waiter = useRef(createReplyWaiter());
  const sendRef = useRef(input.send);
  sendRef.current = input.send;

  useEffect(() => {
    if (!port) return;
    let alive = true;
    void port
      .status()
      .then((next) => {
        if (alive) setStatus(next);
      })
      .catch(() => {
        if (alive) setStatus(null);
      })
      .finally(() => {
        if (alive) setSettled(true);
      });
    return () => {
      alive = false;
    };
  }, [port]);

  useEffect(() => {
    const last = messages ? lastAssistantMessage(messages) : null;
    if (last) waiter.current.observe(last.id, last.content);
  }, [messages]);

  const host = useMemo<ConversationHost>(() => {
    if (!messages) {
      return {
        send: (text: string) => {
          const result = sendRef.current(text);
          if (result instanceof Promise) return result as Promise<string>;
          return typeof result === "string" ? result : undefined;
        },
      };
    }
    return {
      send: async (text: string) => {
        const current = lastAssistantMessage(messages);
        const reply = waiter.current.arm(current?.id ?? "");
        const accepted = sendRef.current(text);
        if (accepted === false) {
          waiter.current.disarm();
          return "";
        }
        return reply;
      },
    };
  }, [messages]);

  if (!port || !settled) return undefined;
  return { port, host, status, speakReplies: true };
}
