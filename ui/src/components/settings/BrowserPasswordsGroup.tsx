import { useCallback, useEffect, useRef, useState } from "react";
import { Copy, Eye, EyeOff, KeyRound, Pencil, Trash2 } from "lucide-react";

import { extractBrowserErrorCode } from "../../lib/browserTauri";
import {
  deleteVaultItem,
  generateVaultPassword,
  getVaultStatus,
  listVaultItems,
  revealVaultPassword,
  updateVaultItem,
  type GeneratedPassword,
  type VaultItemSummary,
  type VaultStatus,
} from "../../lib/browserVault";
import { copyTextToClipboard } from "../../lib/clipboard";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { SettingsGroup } from "./primitives";

const MIN_LENGTH = 8;
const MAX_LENGTH = 128;
const DEFAULT_LENGTH = 20;

/** Lists, edits and deletes saved browser passwords and generates new ones. */
export function BrowserPasswordsGroup() {
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [items, setItems] = useState<VaultItemSummary[]>([]);
  const [query, setQuery] = useState("");
  const [revealed, setRevealed] = useState<Record<string, string>>({});
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftUsername, setDraftUsername] = useState("");
  const [draftPassword, setDraftPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [length, setLength] = useState(String(DEFAULT_LENGTH));
  const [generated, setGenerated] = useState<GeneratedPassword | null>(null);
  const listSeq = useRef(0);

  const fail = useCallback((failure: unknown) => setError(extractBrowserErrorCode(failure)), []);

  const refresh = useCallback(
    (text: string) => {
      const seq = ++listSeq.current;
      const trimmed = text.trim();
      void listVaultItems(trimmed || undefined).then(
        (next) => {
          if (seq !== listSeq.current) return;
          setItems(next);
          setError(null);
        },
        (failure: unknown) => {
          if (seq === listSeq.current) fail(failure);
        },
      );
    },
    [fail],
  );

  useEffect(() => {
    let cancelled = false;
    void getVaultStatus().then(
      (next) => {
        if (cancelled) return;
        setStatus(next);
        if (next.available) refresh("");
      },
      (failure: unknown) => {
        if (!cancelled) setStatus({ available: false, backend: "unsupported", error: extractBrowserErrorCode(failure) });
      },
    );
    return () => {
      cancelled = true;
      listSeq.current += 1;
    };
  }, [refresh]);

  if (status === null) return null;

  if (!status.available) {
    return (
      <SettingsGroup title="Passwords">
        <div role="alert" data-testid="vault-unavailable" className="text-[11px] text-destructive">
          {status.error}
        </div>
      </SettingsGroup>
    );
  }

  const toggleReveal = (id: string) => {
    if (id in revealed) {
      setRevealed(({ [id]: _hidden, ...rest }) => rest);
      return;
    }
    setError(null);
    void revealVaultPassword(id).then((password) => setRevealed((prev) => ({ ...prev, [id]: password })), fail);
  };

  const copyPassword = (id: string) => {
    setError(null);
    void revealVaultPassword(id).then(async (password) => {
      if (!(await copyTextToClipboard(password))) setError("CLIPBOARD_UNAVAILABLE");
    }, fail);
  };

  const startEdit = (item: VaultItemSummary) => {
    setEditingId(item.id);
    setDraftUsername(item.username);
    setDraftPassword("");
    setError(null);
  };

  const saveEdit = (id: string) => {
    setError(null);
    void updateVaultItem(id, draftUsername, draftPassword || null).then((updated) => {
      setItems((prev) => prev.map((item) => (item.id === id ? updated : item)));
      setRevealed(({ [id]: _stale, ...rest }) => rest);
      setEditingId(null);
    }, fail);
  };

  const remove = (id: string) => {
    setError(null);
    void deleteVaultItem(id).then(() => {
      setItems((prev) => prev.filter((item) => item.id !== id));
      setRevealed(({ [id]: _removed, ...rest }) => rest);
      if (editingId === id) setEditingId(null);
    }, fail);
  };

  const generate = () => {
    const parsed = Number.parseInt(length, 10);
    const clamped = Number.isFinite(parsed) ? Math.min(MAX_LENGTH, Math.max(MIN_LENGTH, parsed)) : DEFAULT_LENGTH;
    setLength(String(clamped));
    setError(null);
    void generateVaultPassword({ length: clamped }).then(setGenerated, fail);
  };

  return (
    <SettingsGroup title="Passwords">
      <div className="flex flex-col gap-3">
        <Input
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            refresh(event.target.value);
          }}
          placeholder="Search saved passwords"
          aria-label="Search saved passwords"
          data-testid="vault-search"
          className="h-8 text-[11px]"
        />

        {items.length === 0 ? (
          <div data-testid="vault-empty" className="py-3 text-center text-[11px] text-muted-foreground">
            No saved passwords.
          </div>
        ) : (
          <div className="divide-y divide-border">
            {items.map((item) => {
              const editing = editingId === item.id;
              return (
                <div key={item.id} data-testid="vault-row" className="flex flex-col gap-1.5 py-2.5">
                  <div className="flex items-center gap-3">
                    <KeyRound className="size-4 shrink-0 text-muted-foreground" />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-[13px] font-medium">{item.origin}</div>
                      <div className="truncate text-[11px] text-muted-foreground">{item.username}</div>
                      {item.id in revealed ? (
                        <div data-testid="vault-password" className="truncate font-mono text-[11px]">
                          {revealed[item.id]}
                        </div>
                      ) : null}
                    </div>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => toggleReveal(item.id)}
                      className="h-7 gap-1 px-2 text-[11px]"
                      data-testid="vault-reveal"
                    >
                      {item.id in revealed ? <EyeOff className="size-3" /> : <Eye className="size-3" />}
                      {item.id in revealed ? "Hide" : "Show"}
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => copyPassword(item.id)}
                      className="h-7 gap-1 px-2 text-[11px]"
                      data-testid="vault-copy"
                    >
                      <Copy className="size-3" /> Copy
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => startEdit(item)}
                      className="h-7 gap-1 px-2 text-[11px]"
                      data-testid="vault-edit"
                    >
                      <Pencil className="size-3" /> Edit
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => remove(item.id)}
                      className="h-7 gap-1 px-2 text-[11px]"
                      data-testid="vault-delete"
                    >
                      <Trash2 className="size-3" /> Delete
                    </Button>
                  </div>
                  {editing ? (
                    <div className="flex items-center gap-1.5 pl-7">
                      <Input
                        value={draftUsername}
                        onChange={(event) => setDraftUsername(event.target.value)}
                        aria-label="Username"
                        placeholder="Username"
                        data-testid="vault-edit-username"
                        className="h-7 min-w-0 flex-1 text-[11px]"
                      />
                      <Input
                        type="password"
                        value={draftPassword}
                        onChange={(event) => setDraftPassword(event.target.value)}
                        aria-label="New password"
                        placeholder="New password (blank keeps current)"
                        autoComplete="new-password"
                        data-testid="vault-edit-password"
                        className="h-7 min-w-0 flex-1 text-[11px]"
                      />
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={() => saveEdit(item.id)}
                        className="h-7 px-2 text-[11px]"
                        data-testid="vault-edit-save"
                      >
                        Save
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        onClick={() => setEditingId(null)}
                        className="h-7 px-2 text-[11px]"
                        data-testid="vault-edit-cancel"
                      >
                        Cancel
                      </Button>
                    </div>
                  ) : null}
                </div>
              );
            })}
          </div>
        )}

        <div className="flex items-center gap-1.5 border-t border-border pt-3">
          <Input
            type="number"
            min={MIN_LENGTH}
            max={MAX_LENGTH}
            value={length}
            onChange={(event) => setLength(event.target.value)}
            aria-label="Generated password length"
            data-testid="vault-generate-length"
            className="h-7 w-20 text-[11px]"
          />
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={generate}
            className="h-7 px-2 text-[11px]"
            data-testid="vault-generate"
          >
            Generate
          </Button>
          {generated ? (
            <>
              <span data-testid="vault-generated" className="min-w-0 flex-1 truncate font-mono text-[11px]">
                {generated.password}
              </span>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => {
                  void copyTextToClipboard(generated.password).then((ok) => {
                    if (!ok) setError("CLIPBOARD_UNAVAILABLE");
                  });
                }}
                className="h-7 gap-1 px-2 text-[11px]"
                data-testid="vault-generated-copy"
              >
                <Copy className="size-3" /> Copy
              </Button>
            </>
          ) : null}
        </div>

        {error ? (
          <span role="alert" className="text-[11px] text-destructive">
            {error}
          </span>
        ) : null}
      </div>
    </SettingsGroup>
  );
}
