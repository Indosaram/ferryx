import { useCallback, useEffect, useMemo, useState } from "react";

export type DomElementBox = {
  id: string;
  tag: string;
  bounds: [number, number, number, number]; // [x, y, width, height]
  text?: string | null;
};

export type DesignModeSnapshot = {
  sessionId: string;
  timestampMs: number;
  screenshotPngBase64: string;
  domElements: DomElementBox[];
};

export type RemoteDesignOverlayProps = {
  /** Snapshot to render directly. If omitted, `fetchSnapshot` is used instead. */
  snapshot?: DesignModeSnapshot | null;
  /** Optional loader invoked when no snapshot prop is provided. */
  fetchSnapshot?: () => Promise<DesignModeSnapshot>;
  /** Called when the user selects (hover or click) a DOM element box. */
  onSelectElement?: (element: DomElementBox | null) => void;
  className?: string;
};

type SelectedElementInfo = {
  element: DomElementBox;
  source: "hover" | "click";
};

export function RemoteDesignOverlay({
  snapshot: snapshotProp,
  fetchSnapshot,
  onSelectElement,
  className,
}: RemoteDesignOverlayProps) {
  const [fetchedSnapshot, setFetchedSnapshot] = useState<DesignModeSnapshot | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<SelectedElementInfo | null>(null);

  useEffect(() => {
    if (snapshotProp !== undefined || !fetchSnapshot) {
      return;
    }
    let cancelled = false;
    setLoading(true);
    setLoadError(null);
    fetchSnapshot()
      .then((result) => {
        if (!cancelled) {
          setFetchedSnapshot(result);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setLoadError(err instanceof Error ? err.message : String(err));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [snapshotProp, fetchSnapshot]);

  const snapshot = snapshotProp !== undefined ? snapshotProp : fetchedSnapshot;

  const handleHover = useCallback(
    (element: DomElementBox | null) => {
      setSelected((prev) => {
        if (prev?.source === "click") {
          // A click-pinned selection takes priority over hover.
          return prev;
        }
        if (element === null) {
          return null;
        }
        return { element, source: "hover" };
      });
      if (!selected || selected.source !== "click") {
        onSelectElement?.(element);
      }
    },
    [onSelectElement, selected],
  );

  const handleClick = useCallback(
    (element: DomElementBox) => {
      setSelected((prev) => {
        if (prev?.source === "click" && prev.element.id === element.id) {
          return null;
        }
        return { element, source: "click" };
      });
      onSelectElement?.(element);
    },
    [onSelectElement],
  );

  const imageSrc = useMemo(() => {
    if (!snapshot?.screenshotPngBase64) {
      return null;
    }
    if (snapshot.screenshotPngBase64.startsWith("data:")) {
      return snapshot.screenshotPngBase64;
    }
    return `data:image/png;base64,${snapshot.screenshotPngBase64}`;
  }, [snapshot]);

  if (loading) {
    return (
      <div className={className} data-testid="remote-design-overlay-loading">
        Loading design snapshot...
      </div>
    );
  }

  if (loadError) {
    return (
      <div className={className} data-testid="remote-design-overlay-error">
        Failed to load design snapshot: {loadError}
      </div>
    );
  }

  if (!snapshot || !imageSrc) {
    return (
      <div className={className} data-testid="remote-design-overlay-empty">
        No design snapshot available.
      </div>
    );
  }

  const selectedElement = selected?.element ?? null;

  return (
    <div
      className={className}
      data-testid="remote-design-overlay"
      style={{ position: "relative", display: "inline-block", lineHeight: 0 }}
    >
      <img
        src={imageSrc}
        alt={`Design mode screenshot for session ${snapshot.sessionId}`}
        style={{ display: "block", maxWidth: "100%", height: "auto" }}
      />
      <svg
        data-testid="remote-design-overlay-svg"
        style={{ position: "absolute", top: 0, left: 0, width: "100%", height: "100%", pointerEvents: "none" }}
      >
        {snapshot.domElements.map((element) => {
          const [x, y, width, height] = element.bounds;
          const isSelected = selectedElement?.id === element.id;
          return (
            <rect
              key={element.id}
              data-testid={`design-overlay-box-${element.id}`}
              x={x}
              y={y}
              width={width}
              height={height}
              fill={isSelected ? "rgba(59, 130, 246, 0.25)" : "transparent"}
              stroke={isSelected ? "#3b82f6" : "rgba(59, 130, 246, 0.6)"}
              strokeWidth={isSelected ? 2 : 1}
              style={{ pointerEvents: "auto", cursor: "pointer" }}
              onMouseEnter={() => handleHover(element)}
              onMouseLeave={() => handleHover(null)}
              onClick={() => handleClick(element)}
            >
              <title>{`${element.tag}#${element.id}`}</title>
            </rect>
          );
        })}
      </svg>
      {selectedElement && (
        <div
          data-testid="remote-design-overlay-tooltip"
          style={{
            position: "absolute",
            left: selectedElement.bounds[0],
            top: Math.max(0, selectedElement.bounds[1] - 8),
            transform: "translateY(-100%)",
            background: "rgba(17, 24, 39, 0.92)",
            color: "#fff",
            padding: "4px 8px",
            borderRadius: 4,
            fontSize: 12,
            fontFamily: "monospace",
            pointerEvents: "none",
            whiteSpace: "nowrap",
            zIndex: 10,
          }}
        >
          <div>
            <strong>{selectedElement.tag}</strong> #{selectedElement.id}
          </div>
          {selectedElement.text ? <div>{selectedElement.text}</div> : null}
        </div>
      )}
    </div>
  );
}

export default RemoteDesignOverlay;
