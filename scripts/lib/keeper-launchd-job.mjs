// The keeper's LaunchAgent, as the daemon writes it, checked from the packaging side.
//
// The plan requires the keeper to be an **independently managed launchd service**: its own label,
// its own process group, and a binary that lives OUTSIDE the app bundle so replacing the app cannot
// take the keeper (and its PTY masters) down with it. That property belongs to
// `src-tauri/src/daemon/keeper_launchd.rs`, which renders the plist at install time; nothing in the
// packaging scripts writes it, so a packaging error can only break the property in the payload the
// daemon is given -- a generation directory name that disagrees with the app version, or an
// executable that resolves inside `Ferryx.app`.
//
// Two jobs for this module, both deterministic and both offline:
//
//  * `renderKeeperLaunchdPlist` reproduces `generate_keeper_launchd_plist` byte for byte, so
//    `scripts/release-keeper-launchd.test.mjs` can diff it against the Rust `format!` literal. If
//    either side drifts (a label, a restart rule, a descriptor limit), the test fails instead of the
//    contract quietly changing.
//  * `verifyKeeperLaunchdJob` judges a captured plist -- `plutil -convert xml1 -o - <plist>` output
//    from a real install, recorded as gate evidence -- against the properties independence depends
//    on. The combined release gate runs it on that capture; the assertions are not satisfiable by a
//    daemon-job plist, because the labels and the restart rule differ.
//
// The XML subset parser below is deliberate: this must run without `plutil` (Linux CI, a bare
// archive, no Xcode) and without a dependency, and the documents it reads are always the plist
// subset launchd writes. It reports unparsable input rather than silently returning an empty dict,
// because an empty dict would make every "must be present" check pass for the wrong reason.

/** `keeper_launchd::PLIST_LABEL` -- the production keeper job. */
export const KEEPER_PLIST_LABEL = "com.ferryx.ptykeeper";
/** `keeper_launchd::QA_PLIST_LABEL` -- an isolated fixture's job, which must never ship. */
export const KEEPER_QA_PLIST_LABEL = "com.ferryx.qa.ptykeeper";
/** `launchd::PLIST_LABEL` -- the daemon's own job. Sharing it would make the two jobs one job. */
export const DAEMON_PLIST_LABEL = "com.ferryx.daemon";
/** `keeper_launchd::BINARY_NAME`, also `ferryx_ptykeeper::install::BINARY_NAME`. */
export const KEEPER_BINARY_NAME = "ferryx-ptykeeper";
/** `keeper_launchd::RETAINED_GENERATIONS` / `ferryx_ptykeeper::install::RETAINED_GENERATIONS`. */
export const KEEPER_RETAINED_GENERATIONS = 3;
/** `SoftResourceLimits.NumberOfFiles` in `generate_keeper_launchd_plist`. */
export const KEEPER_SOFT_FILE_LIMIT = 4096;

/**
 * Arguments the keeper job must never carry.
 *
 * `--socket` is refused because the endpoint has to stay derived from the runtime
 * (`crash_supervisor::KEEPER_SOCKET_NAME`): a plist that pinned a path would let a stale job talk to
 * a different socket than the daemon probes. The two `--daemon*` flags are the daemon's own modes;
 * a keeper job started with one of them is not a keeper at all.
 */
export const FORBIDDEN_KEEPER_ARGUMENTS = ["--socket", "--daemon", "--daemon-crash-supervisor"];

/** Typed refusal for callers that want an exception instead of a problem list. */
export class KeeperLaunchdError extends Error {
  constructor(code, message) {
    super(`${code}: ${message}`);
    this.name = "KeeperLaunchdError";
    this.code = code;
  }
}

/**
 * Renders the keeper's LaunchAgent exactly as `generate_keeper_launchd_plist` does.
 *
 * Kept textually identical on purpose -- including the four-space indentation and the trailing
 * newline -- so the parity test compares two whole documents rather than a summary of each.
 */
export function renderKeeperLaunchdPlist({ label, executablePath, logDir, runAtLoad }) {
  const stdoutPath = joinPath(logDir, "keeper.stdout.log");
  const stderrPath = joinPath(logDir, "keeper.stderr.log");
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>${label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>${executablePath}</string>
    </array>
    <key>RunAtLoad</key>
    ${runAtLoad ? "<true/>" : "<false/>"}
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>AbandonProcessGroup</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>SoftResourceLimits</key>
    <dict>
        <key>NumberOfFiles</key>
        <integer>${KEEPER_SOFT_FILE_LIMIT}</integer>
    </dict>
    <key>StandardOutPath</key>
    <string>${stdoutPath}</string>
    <key>StandardErrorPath</key>
    <string>${stderrPath}</string>
</dict>
</plist>
`;
}

/** `join` for POSIX paths without importing `node:path` semantics for Windows hosts. */
function joinPath(parent, name) {
  const base = String(parent ?? "").replace(/[/\\]+$/, "");
  return base === "" ? name : `${base}/${name}`;
}

/**
 * The generation directory the keeper job must exec, `~/.ferryx/keeper/<version>/`.
 *
 * Mirrors `keeper_launchd::default_keeper_install_root_for` with `HOME` given explicitly, so the
 * check never depends on the machine it runs on.
 */
export function keeperGenerationDir({ home, version }) {
  if (typeof home !== "string" || home === "") {
    throw new KeeperLaunchdError("KEEPER_JOB_NO_HOME", "no home directory was given, so no generation path can be stated");
  }
  if (typeof version !== "string" || version === "") {
    throw new KeeperLaunchdError("KEEPER_JOB_NO_VERSION", "no generation version was given");
  }
  return `${String(home).replace(/[/\\]+$/, "")}/.ferryx/keeper/${version}`;
}

/** Expected `ProgramArguments[0]` for a generation. */
export function keeperGenerationExecutable({ home, version }) {
  return joinPath(keeperGenerationDir({ home, version }), KEEPER_BINARY_NAME);
}

// -------------------------------------------------------------------------------------------
// minimal plist XML reader
// -------------------------------------------------------------------------------------------

class PlistParseError extends Error {}

function decodeEntities(text) {
  return text
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&amp;/g, "&");
}

/**
 * Parses the plist XML subset launchd emits: `dict`, `array`, `string`, `integer`, `true`, `false`.
 *
 * Returns the root value (normally a dict). Throws `PlistParseError` on anything it cannot read
 * with certainty; callers turn that into a typed problem rather than an empty document.
 */
export function parsePlistXml(text) {
  const source = String(text ?? "");
  let at = 0;

  const skip = () => {
    for (;;) {
      while (at < source.length && /\s/.test(source[at])) at += 1;
      if (source.startsWith("<!--", at)) {
        const end = source.indexOf("-->", at + 4);
        if (end < 0) throw new PlistParseError("unterminated comment");
        at = end + 3;
        continue;
      }
      if (source.startsWith("<?", at)) {
        const end = source.indexOf("?>", at + 2);
        if (end < 0) throw new PlistParseError("unterminated processing instruction");
        at = end + 2;
        continue;
      }
      if (source.startsWith("<!", at)) {
        // DOCTYPE, possibly with an internal subset in brackets.
        const end = source.indexOf(">", at);
        if (end < 0) throw new PlistParseError("unterminated declaration");
        at = end + 1;
        continue;
      }
      return;
    }
  };

  const openTag = () => {
    skip();
    if (source[at] !== "<") return null;
    const end = source.indexOf(">", at);
    if (end < 0) throw new PlistParseError("unterminated tag");
    const tag = source.slice(at + 1, end).trim();
    at = end + 1;
    return tag;
  };

  const textUntil = (name) => {
    const close = `</${name}>`;
    const end = source.indexOf(close, at);
    if (end < 0) throw new PlistParseError(`unterminated <${name}>`);
    const body = source.slice(at, end);
    at = end + close.length;
    return decodeEntities(body);
  };

  const parseValue = () => {
    const tag = openTag();
    if (tag === null) throw new PlistParseError("document ended where a value was expected");
    if (tag === "true/" || tag === "true") {
      // Non-self-closing form has an empty body; both spellings mean the same value.
      if (tag === "true") textUntil("true");
      return true;
    }
    if (tag === "false/" || tag === "false") {
      if (tag === "false") textUntil("false");
      return false;
    }
    if (tag === "string") return textUntil("string");
    if (tag === "integer") {
      const body = textUntil("integer").trim();
      const parsed = Number.parseInt(body, 10);
      if (!Number.isFinite(parsed)) throw new PlistParseError(`unreadable integer ${JSON.stringify(body)}`);
      return parsed;
    }
    if (tag === "dict") {
      const dict = {};
      for (;;) {
        const next = openTag();
        if (next === "/dict") return dict;
        if (next !== "key") throw new PlistParseError(`expected <key> in dict, found <${next ?? "end of document"}>`);
        const key = textUntil("key");
        dict[key] = parseValue();
      }
    }
    if (tag === "array") {
      const array = [];
      for (;;) {
        skip();
        if (source.startsWith("</array>", at)) {
          at += "</array>".length;
          return array;
        }
        array.push(parseValue());
      }
    }
    throw new PlistParseError(`unexpected element <${tag}>`);
  };

  // The document root is always `<plist version="1.0"> <value> </plist>`; consume the wrapper so
  // the value grammar stays one function and a stray root element is still an error.
  skip();
  let root;
  if (source.startsWith("<plist", at)) {
    const tag = openTag();
    if (!/^plist(\s[^>]*)?$/.test(tag)) throw new PlistParseError(`unreadable <${tag}> element`);
    root = parseValue();
    skip();
    if (!source.startsWith("</plist>", at)) throw new PlistParseError("missing </plist>");
    at += "</plist>".length;
  } else {
    root = parseValue();
  }
  skip();
  return root;
}

// -------------------------------------------------------------------------------------------
// the independence contract
// -------------------------------------------------------------------------------------------

function problem(code, detail) {
  return { code, detail };
}

/**
 * Judges a keeper LaunchAgent against the independence contract.
 *
 * `home` and `version` bind the job to the generation this release ships; `expectedLabel`
 * defaults to the production label, and a test may pass the QA label to model a fixture. Every
 * finding is returned rather than thrown, so a capture with three defects shows all three.
 */
export function verifyKeeperLaunchdJob({
  plistText,
  home,
  version,
  expectedLabel = KEEPER_PLIST_LABEL,
  requiredRunAtLoad = null,
}) {
  const problems = [];
  let job = null;
  try {
    job = parsePlistXml(plistText);
  } catch (error) {
    if (!(error instanceof PlistParseError)) throw error;
    problems.push(problem("KEEPER_JOB_PLIST_UNREADABLE", `the captured plist is not readable: ${error.message}`));
    return { ok: false, job: null, problems, executable: null };
  }
  if (!job || typeof job !== "object" || Array.isArray(job)) {
    problems.push(problem("KEEPER_JOB_PLIST_UNREADABLE", "the captured plist has no root dictionary"));
    return { ok: false, job: null, problems, executable: null };
  }

  if (job.Label !== expectedLabel) {
    const detail =
      job.Label === DAEMON_PLIST_LABEL
        ? `the keeper is installed under the daemon's own label ${JSON.stringify(DAEMON_PLIST_LABEL)}; one job cannot be both, and a shared job dies with the app`
        : job.Label === KEEPER_QA_PLIST_LABEL && expectedLabel !== KEEPER_QA_PLIST_LABEL
          ? `the shipped keeper job uses the QA label ${JSON.stringify(KEEPER_QA_PLIST_LABEL)}, so a real install would never manage it`
          : `Label is ${JSON.stringify(job.Label ?? null)}, expected ${JSON.stringify(expectedLabel)}`;
    problems.push(problem("KEEPER_JOB_LABEL", detail));
  }

  const args = Array.isArray(job.ProgramArguments) ? job.ProgramArguments : null;
  let executable = null;
  if (args === null) {
    problems.push(problem("KEEPER_JOB_ARGUMENTS", "ProgramArguments is not an array, so no executable is named"));
  } else if (args.length === 0) {
    problems.push(problem("KEEPER_JOB_ARGUMENTS", "ProgramArguments is empty, so the job starts nothing"));
  } else if (typeof args[0] !== "string" || args[0] === "") {
    problems.push(problem("KEEPER_JOB_ARGUMENTS", "ProgramArguments[0] is not a path"));
  } else {
    executable = args[0];
    // The executable is judged first: a job that names the wrong program is broken whatever else
    // it does, and reporting the path makes the finding actionable.
    if (executable.startsWith("-")) {
      problems.push(
        problem("KEEPER_JOB_ARGUMENT_FLAG", `ProgramArguments[0] is the flag ${JSON.stringify(executable)}, not a binary path`),
      );
    }
    const forbidden = FORBIDDEN_KEEPER_ARGUMENTS.find(
      (flag) => executable === flag || executable.startsWith(`${flag}=`),
    );
    if (forbidden) {
      problems.push(
        problem(
          "KEEPER_JOB_ARGUMENT_FLAG",
          `ProgramArguments[0] carries ${JSON.stringify(forbidden)}; a keeper job must run the keeper with no arguments`,
        ),
      );
    }
    const expected = home && version ? keeperGenerationExecutable({ home, version }) : null;
    if (expected !== null && executable !== expected) {
      const reasons = [];
      if (/\.app\//.test(executable)) {
        reasons.push(
          "it points inside an app bundle, so replacing the app replaces the process that owns the PTY masters",
        );
      }
      reasons.push(`the generation this archive installs is ${expected}`);
      problems.push(
        problem(
          "KEEPER_JOB_EXECUTABLE",
          `ProgramArguments[0] is ${JSON.stringify(executable)}: ${reasons.join("; ")}`,
        ),
      );
    }
    if (args.length !== 1) {
      problems.push(
        problem(
          "KEEPER_JOB_ARGUMENTS_NOT_SINGLE",
          `ProgramArguments has ${args.length} element(s); the keeper takes its endpoint from the runtime and needs no arguments, ` +
            "so any additional element is a mode flag the keeper must not run in",
        ),
      );
    }
  }

  if (job.RunAtLoad !== true && job.RunAtLoad !== false) {
    problems.push(problem("KEEPER_JOB_RUN_AT_LOAD", "RunAtLoad is not a boolean, so the job's start policy is unstated"));
  } else if (requiredRunAtLoad !== null && job.RunAtLoad !== requiredRunAtLoad) {
    problems.push(
      problem("KEEPER_JOB_RUN_AT_LOAD", `RunAtLoad is ${job.RunAtLoad}, expected ${requiredRunAtLoad}`),
    );
  }

  const keepAlive = job.KeepAlive;
  if (!keepAlive || typeof keepAlive !== "object" || Array.isArray(keepAlive)) {
    problems.push(
      problem(
        "KEEPER_JOB_KEEP_ALIVE",
        "KeepAlive is not a dictionary; an uncontrolled KeepAlive restarts the keeper after its deliberate exit(0) " +
          "(a second keeper that found the instance lock held), which fights the running one",
      ),
    );
  } else if (keepAlive.SuccessfulExit !== false) {
    problems.push(
      problem(
        "KEEPER_JOB_RESTARTS_ON_EXIT_ZERO",
        `KeepAlive.SuccessfulExit is ${JSON.stringify(keepAlive.SuccessfulExit ?? null)}; it must be false so only an abnormal ` +
          "death restarts the keeper",
      ),
    );
  }

  if (job.AbandonProcessGroup !== true) {
    problems.push(
      problem(
        "KEEPER_JOB_PROCESS_GROUP",
        "AbandonProcessGroup is not true, so launchd would signal the PTY children when the job's main process exits",
      ),
    );
  }

  if (job.ProcessType !== "Interactive") {
    problems.push(
      problem(
        "KEEPER_JOB_PROCESS_TYPE",
        `ProcessType is ${JSON.stringify(job.ProcessType ?? null)}, expected "Interactive"; a terminal host throttled as a ` +
          "background job stutters every pane it serves",
      ),
    );
  }

  const soft = job.SoftResourceLimits;
  if (!soft || typeof soft !== "object" || Array.isArray(soft)) {
    problems.push(problem("KEEPER_JOB_DESCRIPTOR_LIMIT", "SoftResourceLimits is absent, so the keeper starts at the default descriptor ceiling"));
  } else if (soft.NumberOfFiles !== KEEPER_SOFT_FILE_LIMIT) {
    problems.push(
      problem(
        "KEEPER_JOB_DESCRIPTOR_LIMIT",
        `SoftResourceLimits.NumberOfFiles is ${JSON.stringify(soft.NumberOfFiles ?? null)}, expected ${KEEPER_SOFT_FILE_LIMIT}; ` +
          "a keeper holds several descriptors per session and dies of EMFILE at the default",
      ),
    );
  }

  for (const [key, expectedName] of [
    ["StandardOutPath", "keeper.stdout.log"],
    ["StandardErrorPath", "keeper.stderr.log"],
  ]) {
    const path = job[key];
    if (typeof path !== "string" || path === "") {
      problems.push(problem("KEEPER_JOB_LOG_PATH", `${key} is absent, so the job's output cannot be read after a crash`));
      continue;
    }
    if (!path.endsWith(`/Library/Logs/Ferryx/${expectedName}`)) {
      problems.push(
        problem(
          "KEEPER_JOB_LOG_PATH",
          `${key} is ${JSON.stringify(path)}, expected a path under <home>/Library/Logs/Ferryx named ${expectedName}; ` +
            "the daemon's own capture files have different names so the two processes never interleave",
        ),
      );
    }
  }
  if (typeof job.StandardOutPath === "string" && job.StandardOutPath === job.StandardErrorPath) {
    problems.push(
      problem(
        "KEEPER_JOB_LOG_PATH",
        "StandardOutPath and StandardErrorPath are the same file, so a crash and its context interleave in one stream",
      ),
    );
  }

  return { ok: problems.length === 0, job, problems, executable };
}

/** Throws `KeeperLaunchdError` carrying the first finding, for callers that must not continue. */
export function assertKeeperLaunchdJob(options) {
  const result = verifyKeeperLaunchdJob(options);
  if (!result.ok) {
    throw new KeeperLaunchdError(result.problems[0].code, result.problems.map((entry) => entry.detail).join("; "));
  }
  return result;
}
