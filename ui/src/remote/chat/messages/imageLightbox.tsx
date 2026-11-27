import React, { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { Copy, Download, Minus, Plus, RotateCcw, X } from "lucide-react";
import { cn } from "../../../lib/cn";

/**
 * Images in messages (t3code ExpandedImageDialog / ZoomableImage): a thumbnail
 * opens a lightbox dialog, zoom controls and Escape live there, and a
 * right-click or long-press opens the save/copy path.
 */

export interface ImageActions {
  /** Save or share the image at `src` (a data:, blob:, or resolved URL). */
  onSaveImage?: (src: string, name?: string) => void;
  /** Copy the image bytes, or on devices without that, its path or URL. */
  onCopyImage?: (src: string) => void;
}

const ImageActionsContext = createContext<ImageActions | null>(null);

export function ImageActionsProvider({
  actions,
  children,
}: {
  actions: ImageActions | null;
  children: React.ReactNode;
}) {
  return <ImageActionsContext.Provider value={actions}>{children}</ImageActionsContext.Provider>;
}

function useImageActions(): ImageActions | null {
  return useContext(ImageActionsContext);
}

/** Last resort save: a plain download anchor, which works for data: and blob: URLs. */
function saveViaAnchor(src: string, name?: string): void {
  const anchor = document.createElement("a");
  anchor.href = src;
  anchor.download = name ?? "image";
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
}

export const ExpandedImageDialog: React.FC<{
  src: string;
  alt?: string;
  onClose: () => void;
}> = ({ src, alt, onClose }) => {
  const [zoom, setZoom] = useState(1);
  const [menuOpen, setMenuOpen] = useState(false);
  const longPressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const actions = useImageActions();

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        // An open action menu is dismissed first, the way the reference layers them.
        if (menuOpen) {
          setMenuOpen(false);
          return;
        }
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, menuOpen]);

  useEffect(() => {
    return () => {
      if (longPressTimer.current) clearTimeout(longPressTimer.current);
    };
  }, []);

  const openMenu = useCallback(() => setMenuOpen(true), []);
  const closeMenu = useCallback(() => setMenuOpen(false), []);

  const handleSave = () => {
    if (actions?.onSaveImage) actions.onSaveImage(src, alt);
    else saveViaAnchor(src, alt);
    closeMenu();
  };
  const handleCopy = () => {
    actions?.onCopyImage?.(src);
    closeMenu();
  };

  return (
    <div
      data-testid="expanded-image-dialog"
      role="dialog"
      aria-modal="true"
      aria-label={alt ?? "Image preview"}
      onClick={onClose}
      onContextMenu={(event) => {
        event.preventDefault();
        openMenu();
      }}
      className="fixed inset-0 z-50 flex flex-col items-center justify-center bg-black/80 p-4"
    >
      <div className="absolute inset-x-0 top-0 flex items-center justify-between gap-2 p-2">
        <span className="flex items-center gap-1">
          <button
            type="button"
            data-testid="image-zoom-out"
            aria-label="Zoom out"
            onClick={(event) => {
              event.stopPropagation();
              setZoom((prev) => Math.max(0.25, prev - 0.5));
            }}
            className="flex size-11 items-center justify-center rounded text-white transition-colors hover:bg-white/10"
          >
            <Minus aria-hidden="true" className="size-4" />
          </button>
          <button
            type="button"
            data-testid="image-zoom-reset"
            aria-label="Reset zoom"
            onClick={(event) => {
              event.stopPropagation();
              setZoom(1);
            }}
            className="flex size-11 items-center justify-center rounded text-white transition-colors hover:bg-white/10"
          >
            <RotateCcw aria-hidden="true" className="size-4" />
          </button>
          <button
            type="button"
            data-testid="image-zoom-in"
            aria-label="Zoom in"
            onClick={(event) => {
              event.stopPropagation();
              setZoom((prev) => Math.min(4, prev + 0.5));
            }}
            className="flex size-11 items-center justify-center rounded text-white transition-colors hover:bg-white/10"
          >
            <Plus aria-hidden="true" className="size-4" />
          </button>
          <span data-testid="image-zoom-level" className="font-mono text-xs text-white/80">
            {Math.round(zoom * 100)}%
          </span>
        </span>
        <button
          type="button"
          data-testid="expanded-image-close"
          aria-label="Close image preview"
          onClick={onClose}
          className="flex size-11 items-center justify-center rounded text-white transition-colors hover:bg-white/10"
        >
          <X aria-hidden="true" className="size-4" />
        </button>
      </div>
      <img
        data-testid="expanded-image"
        src={src}
        alt={alt ?? ""}
        onClick={(event) => event.stopPropagation()}
        style={{ transform: `scale(${zoom})` }}
        className="max-h-[80%] max-w-full select-none object-contain transition-transform duration-150 motion-reduce:transition-none"
      />
      {menuOpen && (
        <div
          data-testid="image-action-menu"
          role="menu"
          onClick={(event) => event.stopPropagation()}
          className="absolute bottom-6 flex flex-col overflow-hidden rounded-xl border border-chat-border bg-chat-surface shadow-xl"
        >
          <button
            type="button"
            role="menuitem"
            data-testid="image-save"
            onClick={handleSave}
            className="flex min-h-11 items-center gap-2 px-4 text-sm text-chat-foreground transition-colors hover:bg-chat-surface-hover"
          >
            <Download aria-hidden="true" className="size-4" />
            Save or share
          </button>
          <button
            type="button"
            role="menuitem"
            data-testid="image-copy"
            onClick={handleCopy}
            className="flex min-h-11 items-center gap-2 px-4 text-sm text-chat-foreground transition-colors hover:bg-chat-surface-hover"
          >
            <Copy aria-hidden="true" className="size-4" />
            Copy image
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={closeMenu}
            className="flex min-h-11 items-center px-4 text-sm text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover"
          >
            Cancel
          </button>
        </div>
      )}
    </div>
  );
};

/**
 * A thumbnail that opens the lightbox. A long press (500ms) opens the save/copy
 * menu straight from the thumbnail, the way the reference's mobile flow does;
 * a plain tap or click opens the dialog.
 */
export const ZoomableImage: React.FC<{
  src: string;
  alt?: string;
  className?: string;
  thumbnailTestId?: string;
}> = ({ src, alt, className, thumbnailTestId }) => {
  const [expanded, setExpanded] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  // The menu is drawn fixed at the thumbnail, because the chat clips and sizes the box it
  // sits in: an absolute menu inside it is cut off and unreachable.
  const [menuAt, setMenuAt] = useState<{ top: number; left: number } | null>(null);
  const longPressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // A long press opens the menu; the click that follows the finger lifting must not also
  // open the full-screen dialog.
  const longPressOpened = useRef(false);
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const actions = useImageActions();

  useEffect(() => {
    return () => {
      if (longPressTimer.current) clearTimeout(longPressTimer.current);
    };
  }, []);

  // A menu that cannot be dismissed is a trap: Escape and a tap outside close it.
  useEffect(() => {
    if (!menuOpen) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        setMenuOpen(false);
      }
    };
    const onPointerDown = (event: PointerEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) setMenuOpen(false);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointerDown);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointerDown);
    };
  }, [menuOpen]);

  const openMenu = () => {
    const box = buttonRef.current?.getBoundingClientRect();
    if (!box) {
      setMenuAt(null);
      setMenuOpen(true);
      return;
    }
    // Keep the menu on screen: it opens below the thumbnail unless that overflows the viewport.
    const width = 176;
    const height = 132;
    const left = Math.max(8, Math.min(box.left, window.innerWidth - width - 8));
    const below = box.bottom + 4;
    const top = below + height > window.innerHeight ? Math.max(8, box.top - height - 4) : below;
    setMenuAt({ top, left });
    setMenuOpen(true);
  };

  const startLongPress = () => {
    longPressOpened.current = false;
    longPressTimer.current = setTimeout(() => {
      longPressOpened.current = true;
      openMenu();
    }, 500);
  };
  const cancelLongPress = () => {
    if (longPressTimer.current) {
      clearTimeout(longPressTimer.current);
      longPressTimer.current = null;
    }
  };

  const handleSave = () => {
    if (actions?.onSaveImage) actions.onSaveImage(src, alt);
    else saveViaAnchor(src, alt);
    setMenuOpen(false);
  };

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        data-testid={`${thumbnailTestId ?? "zoomable-image"}-button`}
        aria-label={alt ? `Preview image: ${alt}` : "Preview image"}
        onClick={() => {
          if (longPressOpened.current) {
            longPressOpened.current = false;
            return;
          }
          setExpanded(true);
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          openMenu();
        }}
        onTouchStart={startLongPress}
        onTouchEnd={cancelLongPress}
        onTouchCancel={cancelLongPress}
        onTouchMove={cancelLongPress}
        className={cn(
          "block cursor-zoom-in overflow-hidden rounded-md focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
          className,
        )}
      >
        {/* The testid stays on the image itself: callers and tests key on the picture. */}
        <img
          data-testid={thumbnailTestId ?? "zoomable-image"}
          src={src}
          alt={alt ?? ""}
          loading="lazy"
          className="my-1 max-w-full rounded-md"
        />
      </button>
      {menuOpen && (
        <div
          ref={menuRef}
          data-testid="image-action-menu"
          role="menu"
          style={menuAt ? { top: menuAt.top, left: menuAt.left } : undefined}
          className={cn(
            "z-40 flex flex-col overflow-hidden rounded-xl border border-chat-border bg-chat-surface shadow-xl",
            menuAt ? "fixed w-44" : "absolute",
          )}
        >
          <button
            type="button"
            role="menuitem"
            data-testid="image-save"
            onClick={handleSave}
            className="flex min-h-11 items-center gap-2 px-4 text-sm text-chat-foreground transition-colors hover:bg-chat-surface-hover"
          >
            <Download aria-hidden="true" className="size-4" />
            Save or share
          </button>
          <button
            type="button"
            role="menuitem"
            data-testid="image-copy"
            onClick={() => {
              actions?.onCopyImage?.(src);
              setMenuOpen(false);
            }}
            className="flex min-h-11 items-center gap-2 px-4 text-sm text-chat-foreground transition-colors hover:bg-chat-surface-hover"
          >
            <Copy aria-hidden="true" className="size-4" />
            Copy image
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => setMenuOpen(false)}
            className="flex min-h-11 items-center px-4 text-sm text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover"
          >
            Cancel
          </button>
        </div>
      )}
      {expanded && (
        <ExpandedImageDialog src={src} alt={alt} onClose={() => setExpanded(false)} />
      )}
    </>
  );
};
