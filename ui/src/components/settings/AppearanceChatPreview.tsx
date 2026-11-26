import { ChatProseMarkdown, ToolCallCard } from "../../remote/chat/MobileChatComponents";
import { MobileChatMessage } from "../../remote/chat/MobileChatMessage";
import { MobileChatComposer } from "../../remote/chat/MobileChatComposer";
import { SettingsGroup } from "./primitives";

const heldMessages = [{ id: "appearance-preview", text: "Compare the light palette next." }];
const refuseSend = () => false;

export function AppearanceChatPreview() {
  return <SettingsGroup title="Live chat preview" description="A read-only sample using your current chat appearance.">
    {/* These are the production components, not a second workspace or token resolver.
        Keep the scope on the sample only: theme recovery controls stay outside it. */}
    <div data-chat-scope="" data-testid="appearance-chat-preview"
      className="rounded-xl border border-chat-border bg-chat-screen p-3 text-chat-foreground">
      <style>{`
        [data-testid="appearance-chat-preview"], [data-testid="appearance-chat-preview"] * {
          animation: none !important;
          transition: none !important;
        }
        /* The real composer normally folds held messages. Expose its static sample
           without opening a disclosure or adding preview state. */
        [data-testid="appearance-chat-preview"] [data-testid="chat-composer-held"] {
          display: flex !important;
        }
      `}</style>
      <div role="group" aria-label="Chat appearance sample" {...{ inert: "" }}>
        <div role="group" aria-label="User bubble" data-preview-row="user">
          <MobileChatMessage id="appearance-user" role="user" content="Show me this chat in my chosen colors." />
        </div>
        <div role="group" aria-label="Assistant prose with math" data-preview-row="assistant">
          <MobileChatMessage id="appearance-assistant" role="assistant" content={"Your palette is live. The area of a circle is $A = \\pi r^2$."} />
        </div>
        <div role="group" aria-label="Code block" data-preview-row="code-block">
          <ChatProseMarkdown text={'```ts\nconst appearance = "yours";\n```'} />
        </div>
        <div role="group" aria-label="Inline code" data-preview-row="inline-code">
          <ChatProseMarkdown text="Use `--chat-primary` to highlight the conversation." />
        </div>
        <div role="group" aria-label="Tool and status" data-preview-row="tool-status">
          <ToolCallCard toolName="read" summary="Read appearance settings" output="Palette ready." status="success" />
          <MobileChatComposer value="" onSend={refuseSend} heldMessages={heldMessages}
            showAttachments={false} dictationSuspended placeholder="Ask the agent..." />
        </div>
      </div>
    </div>
  </SettingsGroup>;
}
