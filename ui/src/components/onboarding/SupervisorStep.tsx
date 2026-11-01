import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { Loader2 } from "lucide-react";

import { getSupervisorSnapshot, subscribeSupervisorSnapshot } from "../../lib/supervisor";
import {
  actionFor,
  adoptAll,
  adoptSummary,
  getSetupStatus,
  readiness,
  runSetup,
  setupFinished,
  type SetupProject,
  type SetupStatus,
} from "../../lib/supervisorSetup";
import { MEMORY_IMPORT_LABEL } from "../supervisor/SupervisorSetupBanner";
import { Button } from "../ui/button";
import { WizardRow, WizardRowList, WizardStatus } from "./WizardPrimitives";

export type SupervisorStepProps = {
  /** Registered local projects; every one is mapped unless unchecked. */
  projects: readonly SetupProject[];
  /** Called once setup reports the supervisor running on its own. */
  onReady: () => void;
};

type Notice = { tone: "error" | "info"; text: string };

/**
 * Optional setup step: turn the bundled supervisor on, map projects, and (opt-in, default on)
 * learn from the person's own recent session turns. After "Turn on" it follows the live snapshot
 * until the supervisor is ready or blocked; there is no open-ended spinner, only the current step.
 */
export function SupervisorStep({ projects, onReady }: SupervisorStepProps): JSX.Element {
  const snapshot = useSyncExternalStore(subscribeSupervisorSnapshot, getSupervisorSnapshot, getSupervisorSnapshot);
  const [fetched, setFetched] = useState<SetupStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [importMemory, setImportMemory] = useState(true);
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set(projects.map((project) => project.path)));
  const [running, setRunning] = useState(false);
  const [started, setStarted] = useState(false);
  const [adopting, setAdopting] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const onReadyRef = useRef(onReady);
  onReadyRef.current = onReady;
  const reported = useRef(false);

  useEffect(() => {
    let live = true;
    void getSetupStatus().then((result) => {
      if (!live) return;
      if (result.kind === "ok") setFetched(result.status);
      else setLoadError(`${result.code}: ${result.message}`);
    });
    return () => {
      live = false;
    };
  }, []);

  // The snapshot follows every change the daemon announces; the first read covers older feeds.
  const status = snapshot.setup ?? fetched;
  const state = readiness(status);
  const finished = setupFinished(status);

  useEffect(() => {
    if (!finished || reported.current) return;
    reported.current = true;
    onReadyRef.current();
  }, [finished]);

  const turnOn = async () => {
    setRunning(true);
    setNotice(null);
    const result = await runSetup({ importMemory, projects: projects.filter((project) => selected.has(project.path)) });
    setRunning(false);
    if (result.kind === "ok") {
      setFetched(result.status);
      setStarted(true);
    } else {
      setNotice({ tone: "error", text: `${result.code}: ${result.message}` });
    }
  };

  const adopt = async () => {
    setAdopting(true);
    setNotice(null);
    const result = await adoptAll();
    setAdopting(false);
    setNotice(result.kind === "ok" ? { tone: "info", text: adoptSummary(result) } : { tone: "error", text: `${result.code}: ${result.message}` });
  };

  if (!status) {
    return (
      <div data-testid="onboarding-supervisor-unknown" className="space-y-2">
        <p className="text-[12px] text-muted-foreground">
          {loadError
            ? "This version of the background service does not report supervisor setup yet. You can set it up later from the Supervisor panel."
            : "Checking the supervisor..."}
        </p>
        {loadError ? <p className="font-mono text-[11px] text-muted-foreground">{loadError}</p> : null}
      </div>
    );
  }

  const action = actionFor(status);
  // Earlier jobs waiting for adoption get their own row; the status box says the supervisor runs.
  const shown = finished ? actionFor({ ...status, nextAction: "none" }) : action;
  // A step that still needs the person keeps its button: a stalled setup is retried, never spun on.
  const canRun = action.primary === "run_setup";
  const showProgress = state === "working";
  const toggleProject = (path: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });

  return (
    <div data-testid="onboarding-supervisor" data-kind={status.nextAction} data-readiness={state} className="space-y-4">
      <p className="text-[13px] leading-relaxed text-foreground">
        The supervisor takes requests from you (here, on your phone, or in Slack), runs them in your projects, and asks you
        when something needs a decision. It never types while you do.
      </p>

      <div
        data-testid="onboarding-supervisor-status"
        role={state === "blocked" ? "alert" : "status"}
        className={
          state === "blocked"
            ? "rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2"
            : "rounded-md border border-border bg-background/40 px-3 py-2"
        }
      >
        <div className="flex min-w-0 items-center gap-2">
          {showProgress ? <Loader2 aria-hidden="true" className="size-3.5 shrink-0 animate-spin text-muted-foreground" /> : null}
          {finished ? (
            <WizardStatus tone="success" label={shown.label} />
          ) : state === "blocked" ? (
            <WizardStatus tone="warning" label={action.label} />
          ) : (
            <span data-testid="onboarding-supervisor-label" className="min-w-0 text-[12px] font-medium text-foreground">
              {action.label}
            </span>
          )}
        </div>
        <p data-testid="onboarding-supervisor-description" className="mt-1 break-words text-[12px] leading-relaxed text-muted-foreground">
          {shown.description}
        </p>
      </div>

      {canRun ? (
        <>
          <label className="flex items-start gap-2 text-[12px] text-foreground">
            <input
              type="checkbox"
              data-testid="onboarding-supervisor-import-memory"
              checked={importMemory}
              onChange={(event) => setImportMemory(event.target.checked)}
              className="mt-0.5 size-3.5 shrink-0 accent-primary"
            />
            <span className="min-w-0">{MEMORY_IMPORT_LABEL}</span>
          </label>

          {projects.length > 0 ? (
            <div>
              <div className="mb-1 text-[11px] font-medium text-muted-foreground">Projects it can work in</div>
              <WizardRowList className="max-h-40 overflow-y-auto">
                {projects.map((project) => (
                  <li key={project.path} className="py-1.5">
                    <label className="flex min-w-0 items-center gap-2 text-[12px]">
                      <input
                        type="checkbox"
                        data-testid="onboarding-supervisor-project"
                        checked={selected.has(project.path)}
                        onChange={() => toggleProject(project.path)}
                        className="size-3.5 shrink-0 accent-primary"
                      />
                      <span className="min-w-0 max-w-[60%] shrink-0 truncate font-medium text-foreground" title={project.name}>
                        {project.name}
                      </span>
                      <span className="min-w-0 truncate font-mono text-[11px] text-muted-foreground" title={project.path}>
                        {project.path}
                      </span>
                    </label>
                  </li>
                ))}
              </WizardRowList>
            </div>
          ) : (
            <p className="text-[12px] text-muted-foreground">No projects yet. Projects you open later are added automatically.</p>
          )}

          <Button type="button" size="sm" data-testid="onboarding-supervisor-run" disabled={running} onClick={() => void turnOn()}>
            {running ? <Loader2 aria-hidden="true" className="animate-spin" /> : null}
            {started ? "Finish setup" : "Turn on supervisor"}
          </Button>
        </>
      ) : null}

      {status.adoptable > 0 && finished ? (
        <WizardRowList>
          <WizardRow
            testId="onboarding-supervisor-adopt-row"
            title={`${status.adoptable} earlier job${status.adoptable === 1 ? "" : "s"} can be handed to the supervisor`}
            description="It keeps their goals and tracks them with the rest of your work."
            action={
              <Button type="button" size="sm" variant="outline" data-testid="onboarding-supervisor-adopt" disabled={adopting} onClick={() => void adopt()}>
                Adopt {status.adoptable} earlier job{status.adoptable === 1 ? "" : "s"}
              </Button>
            }
          />
        </WizardRowList>
      ) : null}

      {notice ? (
        <p
          data-testid="onboarding-supervisor-notice"
          role={notice.tone === "error" ? "alert" : undefined}
          className={notice.tone === "error" ? "break-words text-[12px] text-destructive" : "text-[12px] text-muted-foreground"}
        >
          {notice.text}
        </p>
      ) : null}
    </div>
  );
}
