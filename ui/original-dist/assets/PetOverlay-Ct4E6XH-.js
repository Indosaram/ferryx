import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import "./button-DszXJEV6.js";
import { Gr as readCustomPetBlobUrl, Hr as detectedSpriteCache, Kr as retainCustomPetBlobCacheEntry, Ur as loadCustomBlobUrl, Wr as peekCustomPetBlobUrl, Xr as isBundledPetId, Yr as findBundledPet, qr as BUNDLED_PET, t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import { a as isExplicitAgentStatusFresh, u as AGENT_STATUS_STALE_AFTER_MS } from "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./useMountedRef-1omUd-IV.js";
import { t as usePrefersReducedMotion } from "./usePrefersReducedMotion-TEdW-TWP.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
const CODEX_PET_FRAME = {
	width: 192,
	height: 208
};
function appStateDurations(frames, frameMs, finalMs) {
	return Array.from({ length: frames }, (_, i) => i === frames - 1 ? finalMs : frameMs);
}
const CODEX_PET_ANIMATIONS = {
	idle: {
		row: 0,
		frames: 6,
		frameDurationsMs: [
			1680,
			660,
			660,
			840,
			840,
			1920
		]
	},
	"running-right": {
		row: 1,
		frames: 8,
		frameDurationsMs: appStateDurations(8, 120, 220)
	},
	"running-left": {
		row: 2,
		frames: 8,
		frameDurationsMs: appStateDurations(8, 120, 220)
	},
	waving: {
		row: 3,
		frames: 4,
		frameDurationsMs: appStateDurations(4, 140, 280)
	},
	jumping: {
		row: 4,
		frames: 5,
		frameDurationsMs: appStateDurations(5, 140, 280)
	},
	failed: {
		row: 5,
		frames: 8,
		frameDurationsMs: appStateDurations(8, 140, 240)
	},
	waiting: {
		row: 6,
		frames: 6,
		frameDurationsMs: appStateDurations(6, 150, 260)
	},
	running: {
		row: 7,
		frames: 6,
		frameDurationsMs: appStateDurations(6, 120, 220)
	},
	review: {
		row: 8,
		frames: 6,
		frameDurationsMs: appStateDurations(6, 150, 280)
	}
};
function isLegacyCodexSprite(sprite) {
	const animations = sprite.animations;
	if (!animations || sprite.fps !== 8 || sprite.frameWidth !== CODEX_PET_FRAME.width || sprite.frameHeight !== CODEX_PET_FRAME.height || sprite.columns !== 8 || sprite.defaultAnimation !== "idle") return false;
	const names = Object.keys(animations);
	if (names.length !== Object.keys(CODEX_PET_ANIMATIONS).length) return false;
	return names.every((name) => {
		const anim = animations[name];
		const preset = CODEX_PET_ANIMATIONS[name];
		return !!preset && !!anim && anim.row === preset.row && anim.frames === preset.frames && anim.frameDurationsMs === void 0;
	});
}
function applyCodexSpriteTimingDefaults(sprite) {
	if (!isLegacyCodexSprite(sprite)) return sprite;
	return {
		...sprite,
		animations: { ...CODEX_PET_ANIMATIONS }
	};
}
function usePetUrl() {
	const petId = useAppStore((s) => s.petId);
	const customPets = useAppStore((s) => s.customPets);
	const bundled = isBundledPetId(petId);
	const customMeta = bundled ? null : customPets.find((m) => m.id === petId);
	const [customUrl, setCustomUrl] = (0, import_react.useState)(() => customMeta ? peekCustomPetBlobUrl(customMeta.id) : null);
	const pendingRef = (0, import_react.useRef)(null);
	const customId = customMeta?.id ?? null;
	const customFileName = customMeta?.fileName ?? null;
	const customMime = customMeta?.mimeType ?? "image/png";
	const customKind = customMeta?.kind ?? "image";
	const customSpriteFps = customMeta?.sprite?.fps ?? customMeta?.spriteFps;
	const customHasManifestSprite = !!customMeta?.sprite && customMeta.sprite.frameWidth > 0 && customMeta.sprite.frameHeight > 0 && customMeta.sprite.fps > 0;
	(0, import_react.useLayoutEffect)(() => {
		if (!customId) return;
		return retainCustomPetBlobCacheEntry(customId);
	}, [customId]);
	(0, import_react.useEffect)(() => {
		if (!customId || !customFileName) {
			setCustomUrl(null);
			return;
		}
		const cached = readCustomPetBlobUrl(customId);
		if (cached) {
			setCustomUrl(cached);
			return;
		}
		setCustomUrl(null);
		pendingRef.current = customId;
		let cancelled = false;
		loadCustomBlobUrl(customId, customFileName, customMime, customKind, customSpriteFps, customHasManifestSprite).then((url) => {
			if (cancelled || pendingRef.current !== customId) return;
			setCustomUrl(url);
		});
		return () => {
			cancelled = true;
		};
	}, [
		customId,
		customFileName,
		customMime,
		customKind,
		customSpriteFps,
		customHasManifestSprite
	]);
	if (bundled) return {
		url: (findBundledPet(petId) ?? BUNDLED_PET).url,
		ready: true,
		sprite: null,
		detected: null
	};
	if (customMeta && customUrl) {
		if (customMeta.sprite && customMeta.sprite.frameWidth > 0 && customMeta.sprite.frameHeight > 0 && customMeta.sprite.fps > 0) return {
			url: customUrl,
			ready: true,
			sprite: applyCodexSpriteTimingDefaults(customMeta.sprite),
			detected: null
		};
		const detected = detectedSpriteCache.get(customMeta.id);
		if (detected) return {
			url: customUrl,
			ready: true,
			sprite: null,
			detected
		};
		return {
			url: customUrl,
			ready: true,
			sprite: null,
			detected: null
		};
	}
	return {
		url: BUNDLED_PET.url,
		ready: false,
		sprite: null,
		detected: null
	};
}
function nextPetDragAnimation(current, deltaX) {
	if (deltaX >= 4) return {
		animation: "running-right",
		accepted: true
	};
	if (deltaX <= -4) return {
		animation: "running-left",
		accepted: true
	};
	return {
		animation: current,
		accepted: false
	};
}
function agentStateAnimation(entries, retainedCount, now, staleAfterMs) {
	let hasWorking = false;
	let hasDone = false;
	for (const entry of entries) {
		if (!isExplicitAgentStatusFresh(entry, now, staleAfterMs)) continue;
		if (entry.state === "blocked" || entry.state === "waiting") return "waiting";
		if (entry.state === "working") hasWorking = true;
		else if (entry.state === "done") hasDone = true;
	}
	if (hasWorking) return "running";
	if (hasDone || retainedCount > 0) return "review";
	return "idle";
}
function selectPetAnimationName({ entries, retainedCount, dragging, dragAnimation, hovering, now, staleAfterMs }) {
	const base = agentStateAnimation(entries, retainedCount, now, staleAfterMs);
	if (dragging) return dragAnimation ?? base;
	if (hovering) return "jumping";
	return base;
}
function usePetPointerInteraction(position, moveTo) {
	const [dragging, setDragging] = (0, import_react.useState)(false);
	const [dragAnimation, setDragAnimation] = (0, import_react.useState)(null);
	const [hovering, setHovering] = (0, import_react.useState)(false);
	const [dragGeneration, setDragGeneration] = (0, import_react.useState)(0);
	const dragOffsetRef = (0, import_react.useRef)({
		x: 0,
		y: 0
	});
	const dragBaselineXRef = (0, import_react.useRef)(0);
	const dragDirectionRef = (0, import_react.useRef)(null);
	const activePointerRef = (0, import_react.useRef)(null);
	const onPointerDown = (event) => {
		if (event.button !== 0 || activePointerRef.current !== null) return;
		activePointerRef.current = event.pointerId;
		dragOffsetRef.current = {
			x: event.clientX - position.x,
			y: event.clientY - position.y
		};
		dragBaselineXRef.current = event.clientX;
		dragDirectionRef.current = null;
		event.currentTarget.setPointerCapture(event.pointerId);
		setDragging(true);
		setDragAnimation(null);
		setDragGeneration((generation) => generation + 1);
		event.preventDefault();
	};
	const onPointerMove = (event) => {
		if (event.pointerId !== activePointerRef.current) return;
		const next = nextPetDragAnimation(dragDirectionRef.current, event.clientX - dragBaselineXRef.current);
		if (next.accepted) {
			dragBaselineXRef.current = event.clientX;
			if (next.animation !== dragDirectionRef.current) {
				dragDirectionRef.current = next.animation;
				setDragAnimation(next.animation);
			}
		}
		moveTo({
			x: event.clientX - dragOffsetRef.current.x,
			y: event.clientY - dragOffsetRef.current.y
		});
	};
	const endDrag = (event) => {
		if (event.pointerId !== activePointerRef.current) return;
		activePointerRef.current = null;
		dragDirectionRef.current = null;
		if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
		setDragging(false);
		setDragAnimation(null);
	};
	return {
		dragging,
		dragAnimation,
		hovering,
		dragGeneration,
		handlers: {
			onPointerDown,
			onPointerMove,
			onPointerUp: endDrag,
			onPointerCancel: endDrag,
			onLostPointerCapture: endDrag,
			onPointerEnter: () => setHovering(true),
			onPointerLeave: () => setHovering(false)
		}
	};
}
var MAX_FRAME_DURATION_MS = 6e4;
function buildSpriteAnimationCss({ keyframesId, frames, fps, frameWidth, scale, rowOffsetY, frameDurationsMs }) {
	const name = `pet-${keyframesId}`;
	const durations = validFrameDurations(frameDurationsMs, frames);
	if (durations) {
		const totalMs = durations.reduce((sum, ms) => sum + ms, 0);
		const stops = stepEndStops(durations, totalMs, frameWidth, scale, rowOffsetY);
		if (stops) return {
			keyframesCss: `@keyframes ${name} { ${stops.join(" ")} }`,
			animationCss: `${name} ${totalMs / 1e3}s step-end infinite`
		};
	}
	const duration = Math.max(.1, frames / Math.max(.1, fps));
	return {
		keyframesCss: `@keyframes ${name} { from { background-position: 0px ${rowOffsetY}px; } to { background-position: ${-(frames * frameWidth * scale)}px ${rowOffsetY}px; } }`,
		animationCss: `${name} ${duration}s steps(${frames}) infinite`
	};
}
function validFrameDurations(frameDurationsMs, frames) {
	if (Array.isArray(frameDurationsMs) && frameDurationsMs.length === frames && frameDurationsMs.every((ms) => Number.isFinite(ms) && ms > 0 && ms <= MAX_FRAME_DURATION_MS)) return frameDurationsMs;
	return null;
}
function stepEndStops(durations, totalMs, frameWidth, scale, rowOffsetY) {
	const stops = [];
	let elapsedMs = 0;
	let previousPct = -1;
	for (let index = 0; index < durations.length; index++) {
		const pct = +(elapsedMs / totalMs * 100).toFixed(4);
		if (pct <= previousPct || pct >= 100) return null;
		previousPct = pct;
		const x = -(index * frameWidth * scale);
		stops.push(`${pct}% { background-position: ${x}px ${rowOffsetY}px; }`);
		elapsedMs += durations[index];
	}
	return stops;
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function usePetAnimationName(dragging, dragAnimation, hovering) {
	const agentStatusByPaneKey = useAppStore((s) => s.agentStatusByPaneKey);
	useAppStore((s) => s.agentStatusEpoch);
	const retainedAgentsByPaneKey = useAppStore((s) => s.retainedAgentsByPaneKey);
	return selectPetAnimationName({
		entries: Object.values(agentStatusByPaneKey),
		retainedCount: Object.keys(retainedAgentsByPaneKey).length,
		dragging,
		dragAnimation,
		hovering,
		now: Date.now(),
		staleAfterMs: AGENT_STATUS_STALE_AFTER_MS
	});
}
function SpriteFrame({ url, sprite, animate, maxSize, animationName, restartKey }) {
	const baseId = (0, import_react.useId)().replace(/[^a-zA-Z0-9_-]/g, "");
	const anim = sprite.animations?.[animationName] || sprite.defaultAnimation && sprite.animations?.[sprite.defaultAnimation] || (sprite.animations ? Object.values(sprite.animations)[0] : void 0);
	const row = anim?.row ?? 0;
	const frames = Math.max(1, anim?.frames ?? sprite.columns ?? 1);
	const animKeyframesId = `${baseId}-${row}-${frames}-${restartKey}`;
	const scale = Math.min(maxSize / sprite.frameWidth, maxSize / sprite.frameHeight);
	const renderedW = sprite.frameWidth * scale;
	const renderedH = sprite.frameHeight * scale;
	const bgW = sprite.sheetWidth * scale;
	const bgH = sprite.sheetHeight * scale;
	const startX = 0;
	const startY = -(row * sprite.frameHeight * scale);
	const { keyframesCss, animationCss } = buildSpriteAnimationCss({
		keyframesId: animKeyframesId,
		frames,
		fps: sprite.fps,
		frameWidth: sprite.frameWidth,
		scale,
		rowOffsetY: startY,
		frameDurationsMs: anim?.frameDurationsMs
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("style", { children: keyframesCss }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { style: {
		width: renderedW,
		height: renderedH,
		backgroundImage: `url(${url})`,
		backgroundRepeat: "no-repeat",
		backgroundSize: `${bgW}px ${bgH}px`,
		backgroundPosition: `${startX}px ${startY}px`,
		imageRendering: "pixelated",
		animation: animationCss,
		animationPlayState: animate ? "running" : "paused"
	} })] });
}
function DetectedSpriteFrame({ detected, animate, maxSize }) {
	const canvasRef = (0, import_react.useRef)(null);
	const frameIndexRef = (0, import_react.useRef)(0);
	const lastTimeRef = (0, import_react.useRef)(0);
	const fps = detected.fps > 0 ? detected.fps : 8;
	const { footprintW, footprintH } = (0, import_react.useMemo)(() => {
		let w = 0;
		let h = 0;
		for (const f of detected.frames) {
			const s = Math.min(maxSize / f.w, maxSize / f.h);
			w = Math.max(w, f.w * s);
			h = Math.max(h, f.h * s);
		}
		return {
			footprintW: Math.max(1, Math.round(w)),
			footprintH: Math.max(1, Math.round(h))
		};
	}, [detected, maxSize]);
	(0, import_react.useEffect)(() => {
		const canvas = canvasRef.current;
		if (!canvas) return;
		const ctx = canvas.getContext("2d");
		if (!ctx) return;
		canvas.width = footprintW;
		canvas.height = footprintH;
		frameIndexRef.current = 0;
		lastTimeRef.current = 0;
		if (detected.frames.length === 0) {
			ctx.clearRect(0, 0, canvas.width, canvas.height);
			return;
		}
		let raf = 0;
		const draw = () => {
			const f = detected.frames[frameIndexRef.current % detected.frames.length];
			const bmp = detected.bitmaps[frameIndexRef.current % detected.bitmaps.length];
			if (!f || !bmp) return;
			ctx.imageSmoothingEnabled = false;
			ctx.clearRect(0, 0, canvas.width, canvas.height);
			const scale = Math.min(maxSize / f.w, maxSize / f.h);
			const w = f.w * scale;
			const h = f.h * scale;
			ctx.drawImage(bmp, (footprintW - w) / 2, (footprintH - h) / 2, w, h);
		};
		const tick = (now) => {
			if (now - lastTimeRef.current >= 1e3 / fps) {
				lastTimeRef.current = now;
				frameIndexRef.current = (frameIndexRef.current + 1) % detected.frames.length;
				draw();
			}
			if (animate) raf = requestAnimationFrame(tick);
		};
		draw();
		if (animate) {
			lastTimeRef.current = performance.now();
			raf = requestAnimationFrame(tick);
		}
		return () => {
			if (raf) cancelAnimationFrame(raf);
		};
	}, [
		detected,
		animate,
		footprintW,
		footprintH,
		maxSize,
		fps
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("canvas", {
		ref: canvasRef,
		style: {
			width: footprintW,
			height: footprintH,
			imageRendering: "pixelated"
		}
	});
}
function useDocumentVisible() {
	const [visible, setVisible] = (0, import_react.useState)(() => typeof document === "undefined" ? true : document.visibilityState === "visible");
	(0, import_react.useEffect)(() => {
		const onChange = () => {
			setVisible(document.visibilityState === "visible");
		};
		document.addEventListener("visibilitychange", onChange);
		return () => document.removeEventListener("visibilitychange", onChange);
	}, []);
	return visible;
}
var SIZE = 180;
var POSITION_STORAGE_KEY = "pet-overlay-position";
var LEGACY_POSITION_STORAGE_KEY = "sidekick-overlay-position";
function clampPositionToViewport(pos, size, viewport) {
	const maxX = Math.max(0, viewport.width - size);
	const maxY = Math.max(0, viewport.height - size);
	return {
		x: Math.min(Math.max(0, pos.x), maxX),
		y: Math.min(Math.max(0, pos.y), maxY)
	};
}
function clampToViewport(pos, size = SIZE) {
	if (typeof window === "undefined") return pos;
	return clampPositionToViewport(pos, size, {
		width: window.innerWidth,
		height: window.innerHeight
	});
}
function loadStoredPosition(size = SIZE) {
	if (typeof window === "undefined") return null;
	try {
		let raw = window.localStorage.getItem(POSITION_STORAGE_KEY);
		let migratedFromLegacy = false;
		if (!raw) {
			raw = window.localStorage.getItem(LEGACY_POSITION_STORAGE_KEY);
			if (!raw) return null;
			migratedFromLegacy = true;
		}
		const parsed = JSON.parse(raw);
		if (typeof parsed.x !== "number" || typeof parsed.y !== "number") return null;
		if (migratedFromLegacy) try {
			window.localStorage.setItem(POSITION_STORAGE_KEY, raw);
		} catch {}
		return clampToViewport({
			x: parsed.x,
			y: parsed.y
		}, size);
	} catch {
		return null;
	}
}
function defaultPosition(size = SIZE) {
	if (typeof window === "undefined") return {
		x: 0,
		y: 0
	};
	return clampToViewport({
		x: window.innerWidth - size - 64,
		y: window.innerHeight - size - 16
	}, size);
}
var PET_BOB_KEYFRAMES_CSS = "@keyframes pet-bob { 0%,100% { transform: translateY(0); } 50% { transform: translateY(-4px); } }";
function PetOverlay() {
	const documentVisible = useDocumentVisible();
	const reducedMotion = usePrefersReducedMotion();
	const { url, sprite, detected } = usePetUrl();
	const size = useAppStore((s) => s.petSize);
	const [positionState, setPositionState] = (0, import_react.useState)(() => {
		const currentSize = useAppStore.getState().petSize ?? SIZE;
		return {
			size: currentSize,
			position: loadStoredPosition(currentSize) ?? defaultPosition(currentSize)
		};
	});
	let position = positionState.position;
	if (positionState.size !== size) {
		position = clampToViewport(positionState.position, size);
		setPositionState({
			size,
			position
		});
	}
	const setPosition = (0, import_react.useCallback)((nextPosition) => {
		setPositionState((current) => {
			const currentPosition = current.size === size ? current.position : clampToViewport(current.position, size);
			return {
				size,
				position: typeof nextPosition === "function" ? nextPosition(currentPosition) : nextPosition
			};
		});
	}, [size]);
	const { dragging, dragAnimation, hovering, dragGeneration, handlers } = usePetPointerInteraction(position, (next) => setPosition(clampToViewport(next, size)));
	(0, import_react.useEffect)(() => {
		const onResize = () => setPosition((prev) => clampToViewport(prev, size));
		window.addEventListener("resize", onResize);
		return () => window.removeEventListener("resize", onResize);
	}, [setPosition, size]);
	(0, import_react.useEffect)(() => {
		if (dragging) return;
		try {
			window.localStorage.setItem(POSITION_STORAGE_KEY, JSON.stringify(position));
		} catch {}
	}, [dragging, position]);
	const motionAllowed = documentVisible && !reducedMotion;
	const spriteAnimate = motionAllowed && (!dragging || dragAnimation !== null);
	const bobAnimate = motionAllowed && !dragging;
	const animationName = usePetAnimationName(dragging, dragAnimation, hovering);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		"aria-hidden": true,
		className: "pointer-events-none fixed z-40",
		style: {
			left: position.x,
			top: position.y,
			width: size,
			height: size
		},
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "pointer-events-none flex size-full items-center justify-end",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				...handlers,
				className: "pointer-events-auto flex h-fit w-fit select-none",
				style: {
					cursor: dragging ? "grabbing" : "grab",
					animation: "pet-bob 1.2s ease-in-out infinite",
					animationPlayState: bobAnimate ? "running" : "paused",
					touchAction: "none",
					minWidth: 24,
					minHeight: 24
				},
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("style", { children: PET_BOB_KEYFRAMES_CSS }), sprite ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SpriteFrame, {
					url,
					sprite,
					animate: spriteAnimate,
					maxSize: size,
					animationName,
					restartKey: dragGeneration
				}, url) : detected ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DetectedSpriteFrame, {
					detected,
					animate: spriteAnimate,
					maxSize: size
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
					src: url,
					alt: "",
					className: "max-h-full max-w-full object-contain",
					style: {
						maxWidth: size,
						maxHeight: size
					},
					draggable: false
				})]
			})
		})
	});
}
var PetOverlay_default = PetOverlay;
export { PetOverlay, clampPositionToViewport, PetOverlay_default as default };
