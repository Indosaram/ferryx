import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn } from "./button-DszXJEV6.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { t as CircleCheck } from "./circle-check-CmH3uVJy.js";
import { t as Workflow } from "./workflow-DL6naYZy.js";
import { t as Wrench } from "./wrench-DbYIAUyT.js";
import { t as AgentStateDot } from "./AgentStateDot-DFt63YGw.js";
import { a as OpenAIIcon, t as ClaudeIcon } from "./icons-jFAuHbv9.js";
import { n as getAgentCatalog, t as AgentIcon } from "./agent-catalog-CBF2CV5Q.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var CLAUDE_ACTIVITIES = [
	{
		kind: "tool",
		tool: "Edit",
		arg: "auth/withSession.ts"
	},
	{
		kind: "msg",
		text: "Looking at the session middleware now…"
	},
	{
		kind: "tool",
		tool: "Bash",
		arg: "pnpm run typecheck:node"
	},
	{
		kind: "msg",
		text: "Typecheck passes. Pulling on the login route next."
	},
	{
		kind: "tool",
		tool: "Read",
		arg: "routes/login.ts"
	},
	{
		kind: "tool",
		tool: "Edit",
		arg: "middleware/session.ts"
	},
	{
		kind: "msg",
		text: "Adding tests for the redirect path."
	},
	{
		kind: "tool",
		tool: "Bash",
		arg: "pnpm test auth"
	},
	{
		kind: "tool",
		tool: "Edit",
		arg: "auth.test.ts"
	}
];
function StatusesPage(props) {
	const { active, reducedMotion } = props;
	const [revealed, setRevealed] = (0, import_react.useState)({
		claude: false,
		opencode: false,
		codex: false
	});
	const [claudeIdx, setClaudeIdx] = (0, import_react.useState)(0);
	const [claudeFading, setClaudeFading] = (0, import_react.useState)(false);
	(0, import_react.useEffect)(() => {
		if (!active) {
			setRevealed({
				claude: false,
				opencode: false,
				codex: false
			});
			setClaudeIdx(0);
			setClaudeFading(false);
			return;
		}
		if (reducedMotion) {
			setRevealed({
				claude: true,
				opencode: true,
				codex: true
			});
			return;
		}
		const timeouts = [];
		const schedule = (fn, delay) => {
			timeouts.push(window.setTimeout(fn, delay));
		};
		schedule(() => setRevealed((r) => ({
			...r,
			claude: true
		})), 700);
		schedule(() => setRevealed((r) => ({
			...r,
			opencode: true
		})), 1200);
		schedule(() => setRevealed((r) => ({
			...r,
			codex: true
		})), 1900);
		let idx = 0;
		const cycleId = window.setInterval(() => {
			setClaudeFading(true);
			const swap = window.setTimeout(() => {
				idx = (idx + 1) % CLAUDE_ACTIVITIES.length;
				setClaudeIdx(idx);
				setClaudeFading(false);
			}, 280);
			timeouts.push(swap);
		}, 2400);
		return () => {
			timeouts.forEach((id) => window.clearTimeout(id));
			window.clearInterval(cycleId);
		};
	}, [active, reducedMotion]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex h-full flex-col gap-5",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SupportedAgentsMarquee, { reducedMotion }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "rounded-[10px] bg-foreground/[0.05] px-2 py-2.5 shadow-[inset_0_0_0_1px_rgba(24,24,27,0.06)]",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "grid grid-cols-[14px_minmax(0,1fr)] items-center gap-3 px-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "inline-block size-[9px] rounded-full bg-emerald-500" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "truncate text-[15.5px] font-semibold leading-[1.2]",
					children: translate("auto.components.feature.wall.agents.orchestration.StatusesPage.79971d1539", "redesign auth flow")
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-col gap-3 pl-[30px] pr-2 pt-2.5 pb-1",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentRow$1, {
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(OpenAIIcon, { size: 18 }),
						name: "Codex",
						state: revealed.codex ? "permission" : "working",
						permission: revealed.codex,
						children: revealed.codex ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [translate("auto.components.feature.wall.agents.orchestration.StatusesPage.78f0318ac1", "Wants to run"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CodeChip, { children: translate("auto.components.feature.wall.agents.orchestration.StatusesPage.7b26349cb2", "pnpm migrate latest") })] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Skel, { widthPct: 64 })
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentRow$1, {
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ClaudeIcon, { size: 18 }),
						name: "Claude",
						state: "working",
						children: revealed.claude ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: cn("block transition-opacity duration-[280ms]", claudeFading ? "opacity-0" : "opacity-100"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ClaudeActivityLine, { activity: CLAUDE_ACTIVITIES[claudeIdx] })
						}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Skel, { widthPct: 78 })
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentRow$1, {
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
							agent: "opencode",
							size: 18
						}),
						name: "OpenCode",
						state: "done",
						children: revealed.opencode ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [translate("auto.components.feature.wall.agents.orchestration.StatusesPage.139e3d7458", "Updated"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CodeChip, { children: translate("auto.components.feature.wall.agents.orchestration.StatusesPage.2f549fc0ba", "src/auth/session.test.ts") })] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Skel, { widthPct: 56 })
					})
				]
			})]
		})]
	});
}
function AgentRow$1(props) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "grid grid-cols-[18px_20px_minmax(0,1fr)] items-center gap-3",
		"aria-label": props.name,
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "inline-flex size-[18px] items-center justify-center",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentStateDot, {
					state: props.state,
					size: "md"
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "inline-flex size-5 items-center justify-center",
				children: props.icon
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: cn("truncate text-[13px] leading-[1.3]", props.permission ? "text-red-700 dark:text-red-300" : "text-muted-foreground"),
				children: props.children
			})
		]
	});
}
function ClaudeActivityLine(props) {
	const a = props.activity;
	if (a.kind === "msg") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: a.text });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
		className: "inline-flex max-w-full items-center gap-1.5 truncate",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			className: "inline-flex items-center gap-1 font-semibold text-muted-foreground",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Wrench, {
				className: "size-2.5",
				"aria-hidden": true
			}), a.tool]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CodeChip, { children: a.arg })]
	});
}
function CodeChip(props) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", {
		className: "rounded-[3px] bg-foreground/[0.06] px-1 py-px font-mono text-[12px] text-foreground",
		children: props.children
	});
}
function Skel(props) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: "inline-block h-2 rounded-[5px] bg-foreground/[0.16] align-[-1px]",
		style: { width: `${props.widthPct}%` }
	});
}
function SupportedAgentsMarquee(props) {
	const trackRef = (0, import_react.useRef)(null);
	const agentCatalog = getAgentCatalog();
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "relative -mx-1 mb-1 border-b border-border pb-2 pt-1 overflow-hidden",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "overflow-hidden",
			style: {
				WebkitMaskImage: "linear-gradient(to right, transparent 0, #000 32px, #000 calc(100% - 32px), transparent 100%)",
				maskImage: "linear-gradient(to right, transparent 0, #000 32px, #000 calc(100% - 32px), transparent 100%)"
			},
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				ref: trackRef,
				className: cn("inline-flex gap-2 whitespace-nowrap", props.reducedMotion ? "" : "feature-wall-marquee-track"),
				children: [agentCatalog.map((agent) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MarqueePill, {
					agentId: agent.id,
					label: agent.label
				}, `a-${agent.id}`)), agentCatalog.map((agent) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MarqueePill, {
					agentId: agent.id,
					label: agent.label
				}, `b-${agent.id}`))]
			})
		})
	});
}
function MarqueePill(props) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
		className: "inline-flex shrink-0 items-center gap-1.5 rounded-full bg-foreground/[0.05] px-2.5 py-1 text-[11px] leading-none shadow-[inset_0_0_0_1px_rgba(24,24,27,0.06)]",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "inline-flex size-3.5 items-center justify-center",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
				agent: props.agentId,
				size: 14
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: props.label })]
	});
}
var EXPAND_DELAY = 2200;
var TARGET_DELAY = 3400;
var SWAP_DELAY = 4400;
var LOOP_DELAY = 7800;
function UsagePage(props) {
	const { active, reducedMotion } = props;
	const [phase, setPhase] = (0, import_react.useState)("reset");
	const [pulseKey, setPulseKey] = (0, import_react.useState)(0);
	(0, import_react.useEffect)(() => {
		if (!active) {
			setPhase("reset");
			return;
		}
		if (reducedMotion) {
			setPhase("swapped");
			return;
		}
		let cancelled = false;
		const timeouts = [];
		const at = (delay, fn) => {
			timeouts.push(window.setTimeout(() => {
				if (!cancelled) fn();
			}, delay));
		};
		const loop = () => {
			setPhase("reset");
			at(EXPAND_DELAY, () => setPhase("expanded"));
			at(TARGET_DELAY, () => setPhase("targeted"));
			at(SWAP_DELAY, () => {
				setPhase("swapped");
				setPulseKey((k) => k + 1);
			});
			at(LOOP_DELAY, () => {
				if (!cancelled) loop();
			});
		};
		loop();
		return () => {
			cancelled = true;
			timeouts.forEach((id) => window.clearTimeout(id));
		};
	}, [active, reducedMotion]);
	const swapped = phase === "swapped";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "relative h-full w-full",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Popover, {
			expanded: phase === "expanded" || phase === "targeted" || phase === "swapped",
			targeted: phase === "targeted" || phase === "swapped",
			swapped,
			pulseKey
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(BottomBar, { swapped })]
	});
}
function Popover(props) {
	const { expanded, targeted, swapped, pulseKey } = props;
	const sessionPctText = swapped ? "0% used" : "96% used";
	const sessionResetText = swapped ? "Resets in 5h" : "Resets in 47m";
	const sessionFillWidth = swapped ? "0%" : "96%";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("absolute z-10 flex flex-col gap-[7px] rounded-[10px] border bg-card px-3 py-2.5 not-italic", "border-border text-foreground", "shadow-[0_16px_38px_rgba(24,24,27,0.18),0_2px_6px_rgba(24,24,27,0.08)]"),
		style: {
			left: 40,
			bottom: 70,
			width: 320
		},
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "grid grid-cols-[24px_minmax(0,1fr)] items-center gap-2.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "inline-flex size-6 items-center justify-center rounded-[5px] bg-foreground/[0.06] text-foreground",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "inline-flex",
						style: { color: "#111" },
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(OpenAIIcon, { size: 14 })
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "text-[13.5px] font-bold leading-[1.1]",
					children: translate("auto.components.feature.wall.agents.orchestration.UsagePage.6a4b1d3c38", "Codex")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "text-[11px] text-muted-foreground",
					children: translate("auto.components.feature.wall.agents.orchestration.UsagePage.5e45fb1238", "Updated 1m ago")
				})] })]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageBar, {
				title: translate("auto.components.feature.wall.agents.orchestration.UsagePage.f421abf962", "Session"),
				fillWidth: sessionFillWidth,
				warn: !swapped,
				metaLeft: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: cn("font-semibold", swapped ? "feature-wall-meta-pulse" : "text-[rgb(220_38_38)]"),
					children: sessionPctText
				}, `pct-${pulseKey}-${swapped ? "on" : "off"}`),
				metaRight: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: sessionResetText })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageBar, {
				title: translate("auto.components.feature.wall.agents.orchestration.UsagePage.0470aaed99", "Weekly"),
				fillWidth: "38%",
				warn: false,
				metaLeft: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.feature.wall.agents.orchestration.UsagePage.05ce4ecdd3", "38% used") }),
				metaRight: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.feature.wall.agents.orchestration.UsagePage.4dce5ca3aa", "Resets in 4d 3h") })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "h-px bg-border" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "text-[11px] font-semibold",
				children: translate("auto.components.feature.wall.agents.orchestration.UsagePage.277a9c65a9", "Codex Account")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center justify-between text-[11px]",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AccountNameSkeleton, { widthClassName: swapped ? "w-24" : "w-28" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: cn("inline-flex items-center justify-center text-muted-foreground transition-transform duration-[240ms] ease-[cubic-bezier(.2,.8,.2,1)]", expanded ? "rotate-90" : "rotate-0"),
					"aria-hidden": true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
						width: "10",
						height: "10",
						viewBox: "0 0 10 10",
						fill: "none",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", {
							d: "M3 2 L7 5 L3 8",
							stroke: "currentColor",
							strokeWidth: "1.4",
							strokeLinecap: "round",
							strokeLinejoin: "round"
						})
					})
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: cn("-mt-px grid transition-[grid-template-rows] duration-[280ms] ease-[cubic-bezier(.2,.8,.2,1)]", expanded ? "grid-rows-[1fr]" : "grid-rows-[0fr]"),
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "overflow-hidden min-h-0",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "pb-1 pt-1 text-[10px] font-semibold uppercase tracking-[0.06em] text-muted-foreground",
						children: translate("auto.components.feature.wall.agents.orchestration.UsagePage.be5a165875", "Switch to")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-col gap-0.5 rounded-lg border border-border bg-foreground/[0.025] p-[3px]",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SwitchAccount, {
							accountWidthClassName: "w-24",
							tag: "Team",
							fillPct: 0,
							metaText: "0%",
							highlighted: targeted
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SwitchAccount, {
							accountWidthClassName: "w-32",
							tag: null,
							fillPct: 22,
							metaText: "22%",
							highlighted: false
						})]
					})]
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "absolute bottom-[-5px] left-1/2 size-[10px] -ml-[5px] rotate-45 border-b border-r border-border bg-card",
				"aria-hidden": true
			})
		]
	});
}
function AccountNameSkeleton(props) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: cn("block h-2.5 rounded-full bg-foreground/[0.14]", props.widthClassName) });
}
function UsageBar(props) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col gap-1",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "text-[12px] font-semibold",
				children: props.title
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "h-1.5 w-full overflow-hidden rounded-full bg-foreground/[0.08]",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: cn("block h-full rounded-full transition-[width,background] duration-[800ms] ease-[cubic-bezier(.2,.8,.2,1)]", props.warn ? "bg-[rgb(239_68_68)]" : "bg-[rgb(34_197_94)]"),
					style: { width: props.fillWidth }
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex justify-between font-mono text-[11px] text-muted-foreground",
				children: [props.metaLeft, props.metaRight]
			})
		]
	});
}
function SwitchAccount(props) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("flex items-center justify-between gap-2.5 rounded-md px-1.5 py-1 text-[11.5px] transition-colors duration-[160ms]", props.highlighted ? "bg-emerald-500/10" : "bg-transparent"),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex min-w-0 items-center gap-1.5",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AccountNameSkeleton, { widthClassName: props.accountWidthClassName }), props.tag ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "shrink-0 rounded-full bg-foreground/[0.06] px-1.5 py-px text-[9.5px] font-semibold uppercase tracking-[0.06em] text-muted-foreground",
				children: props.tag
			}) : null]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex shrink-0 items-center gap-1.5",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "block h-1 w-11 overflow-hidden rounded-full bg-foreground/[0.10]",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "block h-full rounded-full bg-emerald-500",
					style: { width: `${props.fillPct}%` }
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "min-w-7 text-right font-mono text-[10px] text-muted-foreground",
				children: props.metaText
			})]
		})]
	});
}
function BottomBar(props) {
	const codexFillWidth = props.swapped ? "0%" : "96%";
	const codexFillColor = props.swapped ? "rgb(34 197 94)" : "rgb(239 68 68)";
	const codexMeta = props.swapped ? "0% used 5h · 4% used wk" : "96% used 47m";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "absolute bottom-[22px] left-1/2 flex -translate-x-1/2 items-center gap-3.5 rounded-lg border border-border bg-muted/60 px-3.5 py-1.5 text-[11px] shadow-[0_1px_2px_rgba(24,24,27,0.04)]",
		style: { width: 340 },
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "inline-flex items-center gap-1.5 font-mono text-[10.5px] text-muted-foreground",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ClaudeIcon, { size: 12 }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "block h-1 w-9 overflow-hidden rounded-full bg-foreground/[0.12]",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "block h-full rounded-full bg-emerald-500",
						style: { width: "29%" }
					})
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.feature.wall.agents.orchestration.UsagePage.64265cb295", "29% used 5h") })
			]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "-my-0.5 inline-flex items-center gap-1.5 rounded-md bg-foreground/[0.06] px-1.5 py-0.5 font-mono text-[10.5px] text-foreground",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					style: { color: "#111" },
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(OpenAIIcon, { size: 12 })
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "block h-1 w-9 overflow-hidden rounded-full bg-foreground/[0.12]",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "block h-full rounded-full transition-[width,background] duration-[600ms] ease-out",
						style: {
							width: codexFillWidth,
							background: codexFillColor
						}
					})
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: codexMeta })
			]
		})]
	});
}
const BUBBLE_FLIGHT_MS = 1600;
const BUBBLE_LAND_MS = BUBBLE_FLIGHT_MS + 360;
const BUBBLE_GAP_MS = 3400;
const ORCHESTRATION_CLI_COMMAND_TIMINGS_MS = [
	250,
	2500,
	5200,
	8600
];
const ORCHESTRATION_CLI_COMMAND_LOOP_MS = 12800;
const PHASE1_BEATS = [
	{
		from: "coord-claude",
		to: "child-codex",
		recipientMsg: "Adding the email_verified column…"
	},
	{
		from: "coord-claude",
		to: "child-claude",
		recipientMsg: "Wiring withSession middleware…"
	},
	{
		from: "child-codex",
		to: "coord-claude",
		coordMsg: "PR 1/2 ready",
		senderFinishes: true
	},
	{
		from: "child-claude",
		to: "coord-claude",
		coordMsg: "PR 2/2 ready",
		senderFinishes: true
	}
];
const COORD_INITIAL_MSG = "Splitting auth rewrite into 2 PRs…";
const CHILD_CODEX_INITIAL_MSG = "Writing the users table migration…";
const CHILD_CLAUDE_INITIAL_MSG = "Sketching withSession middleware…";
const INITIAL_ROW_STATE = {
	"coord-claude": "working",
	"child-codex": "working",
	"child-claude": "working"
};
const INITIAL_ROW_MESSAGES = {
	"coord-claude": COORD_INITIAL_MSG,
	"child-codex": CHILD_CODEX_INITIAL_MSG,
	"child-claude": CHILD_CLAUDE_INITIAL_MSG
};
function rectIn(el, stageRect) {
	const r = el.getBoundingClientRect();
	return {
		left: r.left - stageRect.left,
		right: r.right - stageRect.left,
		top: r.top - stageRect.top,
		bottom: r.bottom - stageRect.top,
		cx: r.left - stageRect.left + r.width / 2,
		cy: r.top - stageRect.top + r.height / 2
	};
}
function arrowPathFromCoordTo(coordEl, targetEl, stageRect) {
	const c = rectIn(coordEl, stageRect);
	const t = rectIn(targetEl, stageRect);
	if (t.top >= c.bottom - 4) {
		const x1$1 = c.right + 4;
		const y1$1 = c.cy;
		const x2$1 = t.right + 4;
		const y2$1 = t.cy;
		const apexX = Math.min(Math.max(x1$1, x2$1) + 36, stageRect.width - 8);
		return `M${x1$1} ${y1$1} C${apexX} ${y1$1}, ${apexX} ${y2$1}, ${x2$1} ${y2$1}`;
	}
	const x1 = c.right + 4;
	const y1 = c.cy;
	const x2 = t.left - 8;
	const y2 = t.cy;
	const dx = (x2 - x1) * .55;
	return `M${x1} ${y1} C${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
}
function bubblePathBetweenRows(stage, fromRow, toRow) {
	const stageRect = stage.getBoundingClientRect();
	const fromCard = fromRow.closest("[data-feature-wall-card]");
	const toCard = toRow.closest("[data-feature-wall-card]");
	const fromR = rectIn(fromRow, stageRect);
	const toR = rectIn(toRow, stageRect);
	const fromC = fromCard instanceof HTMLElement ? rectIn(fromCard, stageRect) : fromR;
	const toC = toCard instanceof HTMLElement ? rectIn(toCard, stageRect) : toR;
	if (toC.top >= fromC.bottom - 4 || fromC.top >= toC.bottom - 4) {
		const x1$1 = fromC.right + 4;
		const y1$1 = fromR.cy;
		const x2$1 = toC.right + 4;
		const y2$1 = toR.cy;
		const apexX = Math.min(Math.max(x1$1, x2$1) + 36, stageRect.width - 8);
		return `M ${x1$1} ${y1$1} C ${apexX} ${y1$1}, ${apexX} ${y2$1}, ${x2$1} ${y2$1}`;
	}
	const x1 = fromC.right + 4;
	const y1 = fromR.cy;
	const x2 = toC.left - 8;
	const y2 = toR.cy;
	const dx = (x2 - x1) * .55;
	return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
}
function WorkspaceCard(props) {
	const { variant, name, dataCard, rows, childPadding, dimName, amberDot } = props;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		"data-feature-wall-card": dataCard,
		className: cn("relative flex flex-col gap-[7px] rounded-[9px] px-[13px] py-[11px]", variant === "coordinator" ? "bg-foreground/[0.04] shadow-[inset_0_0_0_1px_rgba(24,24,27,0.14)]" : "bg-card shadow-[inset_0_0_0_1px_rgba(24,24,27,0.10),0_1px_2px_rgba(24,24,27,0.03)]"),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "grid grid-cols-[12px_minmax(0,1fr)] items-center gap-[9px]",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "block size-[9px] rounded-full",
				style: {
					background: amberDot ? "rgb(245 158 11)" : "rgb(16 185 129)",
					margin: "0 auto"
				},
				"aria-hidden": true
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: cn("truncate font-semibold leading-[1.2] text-foreground", dimName && "opacity-55"),
				style: {
					fontSize: "var(--feature-wall-workspace-title-size, 14.5px)",
					...dimName ? { opacity: .55 } : {}
				},
				children: name
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: cn("flex flex-col gap-[6px]", childPadding && "pl-0.5"),
			children: rows
		})]
	});
}
function AgentRow(props) {
	const { icon, state, message, flashKey, pending, spawnRow, registerRef } = props;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: registerRef,
		className: cn("feature-wall-agent-row grid items-center pl-1", spawnRow && "feature-wall-spawn-row"),
		style: {
			columnGap: "var(--feature-wall-agent-row-gap, 9px)",
			gridTemplateColumns: "var(--feature-wall-agent-status-col, 16px) var(--feature-wall-agent-icon-col, 16px) minmax(0, 1fr)"
		},
		"data-pending": pending ? "true" : void 0,
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "feature-wall-agent-status inline-flex items-center justify-center",
				style: {
					height: "var(--feature-wall-agent-status-box, 16px)",
					width: "var(--feature-wall-agent-status-box, 16px)"
				},
				children: state === "working" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentStateDot, {
					state: "working",
					size: "md"
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "inline-flex items-center justify-center text-emerald-500",
					style: {
						height: "var(--feature-wall-agent-status-icon, 12px)",
						width: "var(--feature-wall-agent-status-icon, 12px)"
					},
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleCheck, {
						"aria-hidden": true,
						style: {
							height: "var(--feature-wall-agent-status-icon, 12px)",
							width: "var(--feature-wall-agent-status-icon, 12px)"
						}
					})
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "feature-wall-agent-icon inline-flex items-center justify-center",
				style: {
					height: "var(--feature-wall-agent-icon-box, 16px)",
					width: "var(--feature-wall-agent-icon-box, 16px)"
				},
				children: icon
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: cn("truncate leading-[1.3] text-foreground", flashKey > 0 && "feature-wall-msg-received"),
				style: { fontSize: "var(--feature-wall-agent-message-size, 13px)" },
				children: message
			}, flashKey)
		]
	});
}
var INITIAL_CHILD_PENDING = {
	"child-codex": true,
	"child-claude": true
};
var CHILD_ONE_CREATE_MS = ORCHESTRATION_CLI_COMMAND_TIMINGS_MS[0];
var CHILD_TWO_CREATE_MS = ORCHESTRATION_CLI_COMMAND_TIMINGS_MS[1];
var FIRST_DISPATCH_MS = ORCHESTRATION_CLI_COMMAND_TIMINGS_MS[2];
function OrchestrationPage(props) {
	const { active, reducedMotion, onCycleComplete, controlledCreatedChildCount, loopMs, showResponseBeats = true } = props;
	const stageRef = (0, import_react.useRef)(null);
	const arrowsRef = (0, import_react.useRef)(null);
	const bubbleLayerRef = (0, import_react.useRef)(null);
	const rowRefs = (0, import_react.useRef)({});
	const childCountControlledRef = (0, import_react.useRef)(controlledCreatedChildCount !== void 0);
	const [rowState, setRowState] = (0, import_react.useState)(INITIAL_ROW_STATE);
	const [rowMessages, setRowMessages] = (0, import_react.useState)(INITIAL_ROW_MESSAGES);
	const [rowFlash, setRowFlash] = (0, import_react.useState)({});
	const [rowPending, setRowPending] = (0, import_react.useState)(INITIAL_CHILD_PENDING);
	const [createdChildCount, setCreatedChildCount] = (0, import_react.useState)(0);
	const displayedChildCount = controlledCreatedChildCount ?? createdChildCount;
	const pendingMirror = (0, import_react.useRef)({ ...INITIAL_CHILD_PENDING });
	childCountControlledRef.current = controlledCreatedChildCount !== void 0;
	const drawArrow = (0, import_react.useCallback)(() => {
		const arrows = arrowsRef.current;
		const stage = stageRef.current;
		if (!arrows || !stage) return;
		arrows.removeAttribute("data-fading");
		const stageRect = stage.getBoundingClientRect();
		arrows.setAttribute("viewBox", `0 0 ${stageRect.width} ${stageRect.height}`);
		arrows.setAttribute("width", String(stageRect.width));
		arrows.setAttribute("height", String(stageRect.height));
		const coordEl = stage.querySelector("[data-feature-wall-card=\"coord\"]");
		if (!(coordEl instanceof HTMLElement)) {
			arrows.innerHTML = "";
			return;
		}
		const codexEl = stage.querySelector("[data-feature-wall-card=\"child\"]");
		const claudeEl = stage.querySelector("[data-feature-wall-card=\"child-claude\"]");
		const paths = [];
		if (codexEl instanceof HTMLElement) paths.push(arrowPathFromCoordTo(coordEl, codexEl, stageRect));
		if (claudeEl instanceof HTMLElement) paths.push(arrowPathFromCoordTo(coordEl, claudeEl, stageRect));
		arrows.innerHTML = paths.map((d) => `<path d="${d}"/>`).join("");
	}, []);
	(0, import_react.useEffect)(() => {
		if (active && displayedChildCount >= 2) {
			const frameId = requestAnimationFrame(() => drawArrow());
			return () => cancelAnimationFrame(frameId);
		}
	}, [
		active,
		displayedChildCount,
		drawArrow
	]);
	(0, import_react.useEffect)(() => {
		if (!active) {
			setRowState(INITIAL_ROW_STATE);
			setRowMessages(INITIAL_ROW_MESSAGES);
			setRowFlash({});
			setRowPending(INITIAL_CHILD_PENDING);
			setCreatedChildCount(0);
			pendingMirror.current = { ...INITIAL_CHILD_PENDING };
			const arrows = arrowsRef.current;
			if (arrows) arrows.innerHTML = "";
			const layer = bubbleLayerRef.current;
			if (layer) layer.innerHTML = "";
			return;
		}
		if (reducedMotion) {
			setRowState(INITIAL_ROW_STATE);
			setRowMessages(INITIAL_ROW_MESSAGES);
			setRowPending({});
			setCreatedChildCount(2);
			pendingMirror.current = {};
			const frameId = requestAnimationFrame(() => drawArrow());
			return () => cancelAnimationFrame(frameId);
		}
		let cancelled = false;
		const timeouts = [];
		const frames = /* @__PURE__ */ new Set();
		const later = (fn, ms) => {
			timeouts.push(window.setTimeout(() => !cancelled && fn(), ms));
		};
		const nextFrame = (fn) => {
			const frameId = requestAnimationFrame(() => {
				frames.delete(frameId);
				if (!cancelled) fn();
			});
			frames.add(frameId);
		};
		const clearArrows = () => {
			const arrows = arrowsRef.current;
			if (arrows) arrows.innerHTML = "";
		};
		const fireBubble = (beat) => {
			const fromRow = rowRefs.current[beat.from];
			const toRow = rowRefs.current[beat.to];
			const stage = stageRef.current;
			const layer = bubbleLayerRef.current;
			if (!fromRow || !toRow || !stage || !layer) return;
			if (beat.senderFinishes) setRowState((s) => ({
				...s,
				[beat.from]: "done"
			}));
			const wasPending = pendingMirror.current[beat.to] === true;
			const path = bubblePathBetweenRows(stage, fromRow, wasPending ? toRow.closest("[data-feature-wall-card]") ?? toRow : toRow);
			const bubble = document.createElement("div");
			bubble.className = "feature-wall-bubble";
			bubble.style.offsetPath = `path("${path}")`;
			bubble.innerHTML = "<svg viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden><rect x=\"3\" y=\"5\" width=\"18\" height=\"14\" rx=\"2\"/><path d=\"M3 7l9 6 9-6\"/></svg>";
			layer.appendChild(bubble);
			bubble.offsetWidth;
			nextFrame(() => bubble.classList.add("in-flight"));
			later(() => {
				if (wasPending) {
					pendingMirror.current = {
						...pendingMirror.current,
						[beat.to]: false
					};
					setRowPending((p) => ({
						...p,
						[beat.to]: false
					}));
				}
				const replacement = beat.to === "coord-claude" && beat.coordMsg ? beat.coordMsg : beat.recipientMsg ?? "";
				if (replacement) {
					setRowMessages((m) => ({
						...m,
						[beat.to]: replacement
					}));
					setRowFlash((f) => ({
						...f,
						[beat.to]: (f[beat.to] ?? 0) + 1
					}));
				}
				bubble.classList.remove("in-flight");
				bubble.classList.add("landed");
			}, BUBBLE_FLIGHT_MS);
			later(() => bubble.remove(), BUBBLE_LAND_MS);
		};
		const runOnce = (done) => {
			clearArrows();
			setRowState(INITIAL_ROW_STATE);
			setRowMessages(INITIAL_ROW_MESSAGES);
			setRowPending(INITIAL_CHILD_PENDING);
			setCreatedChildCount(0);
			pendingMirror.current = { ...INITIAL_CHILD_PENDING };
			if (!childCountControlledRef.current) {
				later(() => {
					setCreatedChildCount(1);
				}, CHILD_ONE_CREATE_MS);
				later(() => {
					setCreatedChildCount(2);
					later(() => drawArrow(), 360);
				}, CHILD_TWO_CREATE_MS);
			}
			const beats = showResponseBeats ? PHASE1_BEATS : PHASE1_BEATS.slice(0, 2);
			let beatIdx = 0;
			const next = () => {
				if (beatIdx >= beats.length) {
					later(done, 800);
					return;
				}
				fireBubble(beats[beatIdx]);
				beatIdx += 1;
				later(next, BUBBLE_GAP_MS);
			};
			later(next, FIRST_DISPATCH_MS);
		};
		const loop = () => {
			runOnce(() => {
				onCycleComplete?.();
				const elapsedMs = FIRST_DISPATCH_MS + (showResponseBeats ? PHASE1_BEATS.length : 2) * BUBBLE_GAP_MS + 800;
				later(loop, loopMs ? Math.max(0, loopMs - elapsedMs) : 1400);
			});
		};
		loop();
		const onResize = () => drawArrow();
		window.addEventListener("resize", onResize);
		const cleanupLayer = bubbleLayerRef.current;
		return () => {
			cancelled = true;
			timeouts.forEach((id) => window.clearTimeout(id));
			frames.forEach((id) => cancelAnimationFrame(id));
			frames.clear();
			window.removeEventListener("resize", onResize);
			if (cleanupLayer) cleanupLayer.innerHTML = "";
		};
	}, [
		active,
		onCycleComplete,
		reducedMotion,
		drawArrow,
		loopMs,
		showResponseBeats
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: stageRef,
		className: "feature-wall-orch-stage relative grid",
		style: {
			gridTemplateColumns: "minmax(0, 1fr)",
			gridAutoRows: "min-content",
			rowGap: 28,
			paddingRight: 56,
			alignItems: "start",
			alignContent: "center",
			height: "100%"
		},
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "relative flex min-w-0 flex-col gap-1.5",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceCard, {
						variant: "coordinator",
						name: translate("auto.components.feature.wall.agents.orchestration.OrchestrationPage.coordinatorName", "redesign auth flow"),
						dataCard: "coord",
						rows: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentRow, {
							agentKey: "coord-claude",
							icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ClaudeIcon, { size: 13 }),
							state: rowState["coord-claude"],
							message: rowMessages["coord-claude"],
							flashKey: rowFlash["coord-claude"] ?? 0,
							registerRef: (node) => {
								rowRefs.current["coord-claude"] = node;
							}
						}, "coord-claude")]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex justify-start",
						style: {
							marginLeft: "var(--feature-wall-child-indent, 28px)",
							marginTop: 0,
							marginBottom: 0
						},
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
							className: "inline-flex items-center gap-1 rounded-md border border-border bg-card px-1.5 text-muted-foreground",
							style: {
								height: 18,
								fontSize: 10,
								fontWeight: 500
							},
							"aria-label": translate("auto.components.feature.wall.agents.orchestration.OrchestrationPage.862605d066", "2 child workspaces"),
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Workflow, {
									className: "size-2.5",
									"aria-hidden": true
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "truncate",
									children: translate("auto.components.feature.wall.agents.orchestration.OrchestrationPage.30b509a467", "2 children")
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, {
									className: "size-2.5",
									"aria-hidden": true
								})
							]
						})
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "feature-wall-children-wrapper",
						"data-visible": displayedChildCount > 0 ? "true" : void 0,
						style: {
							width: "calc(100% - var(--feature-wall-child-indent, 28px))",
							marginLeft: "auto",
							display: "flex",
							flexDirection: "column",
							gap: 8
						},
						children: [displayedChildCount >= 1 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "feature-wall-child-card-shell",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceCard, {
								variant: "default",
								name: translate("auto.components.feature.wall.agents.orchestration.OrchestrationPage.childPr1Name", "PR 1/2: migrate users.sql"),
								dataCard: "child",
								childPadding: true,
								rows: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentRow, {
									agentKey: "child-codex",
									icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(OpenAIIcon, { size: 13 }),
									state: rowState["child-codex"],
									message: rowMessages["child-codex"],
									flashKey: rowFlash["child-codex"] ?? 0,
									pending: rowPending["child-codex"],
									spawnRow: true,
									registerRef: (node) => {
										rowRefs.current["child-codex"] = node;
									}
								}, "child-codex")]
							})
						}) : null, displayedChildCount >= 2 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "feature-wall-child-card-shell",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceCard, {
								variant: "default",
								name: translate("auto.components.feature.wall.agents.orchestration.OrchestrationPage.childPr2Name", "PR 2/2: withSession middleware"),
								dataCard: "child-claude",
								childPadding: true,
								rows: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentRow, {
									agentKey: "child-claude",
									icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ClaudeIcon, { size: 13 }),
									state: rowState["child-claude"],
									message: rowMessages["child-claude"],
									flashKey: rowFlash["child-claude"] ?? 0,
									pending: rowPending["child-claude"],
									spawnRow: true,
									registerRef: (node) => {
										rowRefs.current["child-claude"] = node;
									}
								}, "child-claude")]
							})
						}) : null]
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
				ref: arrowsRef,
				className: "feature-wall-orch-arrows",
				"aria-hidden": true,
				preserveAspectRatio: "none"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				ref: bubbleLayerRef,
				"aria-hidden": true,
				style: {
					position: "absolute",
					inset: 0,
					pointerEvents: "none",
					zIndex: 3
				}
			})
		]
	});
}
var PANEL_HEIGHT_PX = 392;
var PANEL_WIDTH_PX = 520;
function AgentsOrchestrationVisual(props) {
	const { reducedMotion, activeStepId, widthPx, heightPx, onCycleComplete, orchestrationCreatedChildCount, orchestrationLoopMs, orchestrationShowResponseBeats } = props;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "relative flex flex-col text-foreground",
		style: {
			width: widthPx ?? PANEL_WIDTH_PX,
			height: heightPx ?? PANEL_HEIGHT_PX
		},
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Page, {
				active: activeStepId === "statuses",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatusesPage, {
					active: activeStepId === "statuses",
					reducedMotion
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Page, {
				active: activeStepId === "usage",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsagePage, {
					active: activeStepId === "usage",
					reducedMotion
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Page, {
				active: activeStepId === "orchestration",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(OrchestrationPage, {
					active: activeStepId === "orchestration",
					reducedMotion,
					onCycleComplete,
					controlledCreatedChildCount: orchestrationCreatedChildCount,
					loopMs: orchestrationLoopMs,
					showResponseBeats: orchestrationShowResponseBeats
				})
			})
		]
	});
}
function Page(props) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		"aria-hidden": !props.active,
		className: cn("absolute inset-0 overflow-hidden transition-[opacity,transform] duration-[360ms] ease-out", props.active ? "pointer-events-auto translate-y-0 opacity-100" : "pointer-events-none translate-y-1 opacity-0"),
		children: props.children
	});
}
function getFeatureWallOpenSource(modalData) {
	const source = modalData.source;
	return source === "help_menu" || source === "popup" || source === "onboarding" ? source : "unknown";
}
export { ORCHESTRATION_CLI_COMMAND_TIMINGS_MS as i, AgentsOrchestrationVisual as n, ORCHESTRATION_CLI_COMMAND_LOOP_MS as r, getFeatureWallOpenSource as t };
