import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon } from "./button-DszXJEV6.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { Jr as BUNDLED_PETS, Mi as PET_SIZE_MIN, Xr as isBundledPetId, Yr as findBundledPet, ji as PET_SIZE_MAX, m_ as Trash2, qr as BUNDLED_PET, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as Upload } from "./upload-BpSdFZIC.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { a as DropdownMenuLabel, d as DropdownMenuSub, f as DropdownMenuSubContent, i as DropdownMenuItem, l as DropdownMenuSeparator, m as DropdownMenuTrigger, o as DropdownMenuPortal, p as DropdownMenuSubTrigger, r as DropdownMenuContent, t as DropdownMenu } from "./dropdown-menu-Dth6LPK-.js";
import "./useMountedRef-1omUd-IV.js";
var PackageOpen = createLucideIcon("package-open", [
	["path", {
		d: "M12 22v-9",
		key: "x3hkom"
	}],
	["path", {
		d: "M15.17 2.21a1.67 1.67 0 0 1 1.63 0L21 4.57a1.93 1.93 0 0 1 0 3.36L8.82 14.79a1.655 1.655 0 0 1-1.64 0L3 12.43a1.93 1.93 0 0 1 0-3.36z",
		key: "2ntwy6"
	}],
	["path", {
		d: "M20 13v3.87a2.06 2.06 0 0 1-1.11 1.83l-6 3.08a1.93 1.93 0 0 1-1.78 0l-6-3.08A2.06 2.06 0 0 1 4 16.87V13",
		key: "1pmm1c"
	}],
	["path", {
		d: "M21 12.43a1.93 1.93 0 0 0 0-3.36L8.83 2.2a1.64 1.64 0 0 0-1.63 0L3 4.57a1.93 1.93 0 0 0 0 3.36l12.18 6.86a1.636 1.636 0 0 0 1.63 0z",
		key: "12ttoo"
	}]
]);
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function PetStatusSegmentInner() {
	const petVisible = useAppStore((s) => s.petVisible);
	const setPetVisible = useAppStore((s) => s.setPetVisible);
	const petId = useAppStore((s) => s.petId);
	const setPetId = useAppStore((s) => s.setPetId);
	const customPets = useAppStore((s) => s.customPets);
	const addCustomPet = useAppStore((s) => s.addCustomPet);
	const removeCustomPet = useAppStore((s) => s.removeCustomPet);
	const petSize = useAppStore((s) => s.petSize);
	const setPetSize = useAppStore((s) => s.setPetSize);
	const openSettingsPage = useAppStore((s) => s.openSettingsPage);
	const openSettingsTarget = useAppStore((s) => s.openSettingsTarget);
	const bundled = isBundledPetId(petId);
	const activeBundled = bundled ? findBundledPet(petId) ?? BUNDLED_PET : null;
	const activeCustom = bundled ? null : customPets.find((m) => m.id === petId);
	const activeLabel = activeBundled ? activeBundled.label : activeCustom?.label ?? "Pet";
	const label = petVisible ? activeLabel : `${activeLabel} hidden`;
	const handleImport = async () => {
		console.log("[pet-overlay] upload: click");
		if (!window.api?.pet?.import) {
			console.warn("[pet-overlay] upload: window.api.pet.import missing — restart Orca");
			toast.error(translate("auto.components.status.bar.PetStatusSegment.e6234bcc17", "Custom pet upload needs a full app restart (not just reload)."));
			return;
		}
		try {
			const model = await window.api.pet.import();
			console.log("[pet-overlay] upload: result", model);
			if (!model) return;
			addCustomPet(model);
			if (!petVisible) setPetVisible(true);
			setPetId(model.id);
		} catch (error) {
			console.error("[pet-overlay] upload: error", error);
			toast.error(error instanceof Error ? error.message : translate("auto.components.status.bar.PetStatusSegment.f395c9a685", "Failed to import file"));
		}
	};
	const handleImportPetBundle = async () => {
		if (!window.api?.pet?.importPetBundle) {
			toast.error(translate("auto.components.status.bar.PetStatusSegment.2021d4f6db", "Pet bundle import needs a full app restart (not just reload)."));
			return;
		}
		try {
			const model = await window.api.pet.importPetBundle();
			if (!model) return;
			addCustomPet(model);
			if (!petVisible) setPetVisible(true);
			setPetId(model.id);
		} catch (error) {
			console.error("[pet-overlay] pet bundle: error", error);
			toast.error(error instanceof Error ? error.message : translate("auto.components.status.bar.PetStatusSegment.cef0ab4636", "Failed to import pet bundle"));
		}
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenu, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
			type: "button",
			className: "group inline-flex items-center cursor-pointer pl-1 pr-[6.5rem] py-0.5",
			"aria-label": translate("auto.components.status.bar.PetStatusSegment.aec479308a", "Pet menu"),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: `rounded px-1 py-0.5 text-[11px] font-medium text-muted-foreground group-hover:bg-accent/70 group-hover:text-foreground ${petVisible ? "" : "opacity-50"}`,
				children: label
			})
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuContent, {
		side: "top",
		align: "end",
		sideOffset: 8,
		className: "min-w-[220px]",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuLabel, { children: translate("auto.components.status.bar.PetStatusSegment.34c25dfe9c", "Pet") }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuItem, {
				onSelect: (event) => {
					event.preventDefault();
					setPetVisible(!petVisible);
				},
				children: petVisible ? translate("auto.components.status.bar.PetStatusSegment.1fbc51cc77", "Hide pet") : translate("auto.components.status.bar.PetStatusSegment.6d0a8cd179", "Show pet")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "px-2 py-1.5",
				onPointerDown: (e) => e.stopPropagation(),
				onClick: (e) => e.stopPropagation(),
				onKeyDown: (e) => e.stopPropagation(),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mb-1 flex items-center justify-between text-[11px] text-muted-foreground",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.status.bar.PetStatusSegment.2f7bbaa457", "Size") }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "tabular-nums",
						children: [petSize, translate("auto.components.status.bar.PetStatusSegment.c6aa805b1b", "px")]
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
					type: "range",
					min: 60,
					max: 360,
					step: 10,
					value: petSize,
					onChange: (e) => setPetSize(Number(e.target.value)),
					className: "w-full",
					"aria-label": translate("auto.components.status.bar.PetStatusSegment.b75484a01a", "Pet size")
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSub, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSubTrigger, { children: translate("auto.components.status.bar.PetStatusSegment.0608ad02a2", "Choose pet") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuPortal, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSubContent, {
				className: "min-w-[220px]",
				children: [
					BUNDLED_PETS.map((pet) => {
						return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
							onSelect: () => {
								if (!petVisible) setPetVisible(true);
								setPetId(pet.id);
							},
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "flex w-4 items-center justify-center",
								children: pet.id === petId ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, {
									className: "size-3.5",
									"aria-hidden": true
								}) : null
							}), pet.label]
						}, pet.id);
					}),
					customPets.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}) : null,
					customPets.map((model) => {
						return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
							className: "group",
							onSelect: () => {
								if (!petVisible) setPetVisible(true);
								setPetId(model.id);
							},
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "flex w-4 items-center justify-center",
									children: model.id === petId ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, {
										className: "size-3.5",
										"aria-hidden": true
									}) : null
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "flex-1 truncate",
									children: model.label
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
									type: "button",
									className: "ml-2 flex size-5 items-center justify-center rounded text-muted-foreground hover:bg-destructive/15 hover:text-destructive",
									"aria-label": translate("auto.components.status.bar.PetStatusSegment.3668339495", "Remove {{value0}}", { value0: model.label }),
									onClick: (event) => {
										event.stopPropagation();
										event.preventDefault();
										removeCustomPet(model.id);
									},
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, {
										className: "size-3",
										"aria-hidden": true
									})
								})
							]
						}, model.id);
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
						onSelect: () => {
							handleImport();
						},
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Upload, {
							className: "size-3.5",
							"aria-hidden": true
						}), translate("auto.components.status.bar.PetStatusSegment.59b5955621", "Upload your own…")]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
						onSelect: () => {
							handleImportPetBundle();
						},
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PackageOpen, {
							className: "size-3.5",
							"aria-hidden": true
						}), translate("auto.components.status.bar.PetStatusSegment.ed176ad68f", "Import .codex-pet bundle…")]
					})
				]
			}) })] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuItem, {
				onSelect: () => {
					openSettingsTarget({
						pane: "experimental",
						repoId: null,
						sectionId: "experimental-pet"
					});
					openSettingsPage();
				},
				children: translate("auto.components.status.bar.PetStatusSegment.cd8c6c654c", "Pet settings…")
			})
		]
	})] });
}
const PetStatusSegment = import_react.memo(PetStatusSegmentInner);
export { PetStatusSegment };
