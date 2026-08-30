export type DiagnosticAnalyticsEvent =
  | {
      readonly name: "diagnostic_start";
      readonly lang: "ko" | "en";
    }
  | {
      readonly name: "question_answered";
      readonly questionIndex: number;
      readonly lang: "ko" | "en";
    }
  | {
      readonly name: "diagnostic_complete";
      readonly resultType: string;
      readonly primaryWinnerId: string;
      readonly isTie: boolean;
      readonly lang: "ko" | "en";
    }
  | {
      readonly name: "result_view";
      readonly resultType: string;
      readonly lang: "ko" | "en";
    }
  | {
      readonly name: "compare_view";
      readonly pair: string;
      readonly lang: "ko" | "en";
    };

export interface AnalyticsConfig {
  readonly endpoint?: string;
  readonly transport?: (url: string, payload: DiagnosticAnalyticsEvent) => void;
}

declare global {
  interface Window {
    readonly FERRYX_ANALYTICS_ENDPOINT?: string;
  }
}

function resolveEndpoint(): string | undefined {
  if (typeof window !== "undefined" && typeof window.FERRYX_ANALYTICS_ENDPOINT === "string") {
    return window.FERRYX_ANALYTICS_ENDPOINT;
  }
  return undefined;
}

export function sendDiagnosticEvent(
  event: DiagnosticAnalyticsEvent,
  config?: AnalyticsConfig
): boolean {
  const endpoint = config?.endpoint ?? resolveEndpoint();
  if (endpoint === undefined || endpoint.length === 0) {
    return false;
  }

  if (config?.transport !== undefined) {
    config.transport(endpoint, event);
    return true;
  }

  if (typeof navigator !== "undefined" && typeof navigator.sendBeacon === "function") {
    try {
      const blob = new Blob([JSON.stringify(event)], { type: "application/json" });
      return navigator.sendBeacon(endpoint, blob);
    } catch {
      return false;
    }
  }

  return false;
}
