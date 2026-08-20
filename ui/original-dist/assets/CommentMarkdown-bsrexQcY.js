import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { t as X } from "./x-BrGKE4uz.js";
import { c as DialogTrigger, n as DialogClose, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as remarkGfm } from "./lib-CtirWBBB.js";
import { c as defaultUrlTransform, n as defaultSchema, r as rehypeRaw, s as Markdown, t as rehypeSanitize } from "./lib-D08jHVMa.js";
import { t as remarkBreaks } from "./lib-Cpbsvy64.js";
import { t as MermaidBlock } from "./MermaidBlock-gW3wAx0A.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function CommentMermaidBlock({ content, className }) {
	const settings = useAppStore((s) => s.settings);
	const isDark = settings?.theme === "dark" || settings?.theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn(className),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MermaidBlock, {
			content,
			isDark,
			htmlLabels: false
		})
	});
}
function isMermaidFence(className) {
	return /\blanguage-mermaid\b/.test(className ?? "");
}
function renderMermaidFence(children, className) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommentMermaidBlock, {
		content: String(children).trimEnd(),
		className
	});
}
function isMermaidPre(children) {
	const child = import_react.Children.toArray(children)[0];
	if (!import_react.isValidElement(child)) return false;
	const className = child.props?.className;
	return isMermaidFence(className);
}
function isGitHubUserAttachmentUrl(href) {
	if (!href) return false;
	try {
		const url = new URL(href);
		return url.protocol === "https:" && url.hostname === "github.com" && url.pathname.startsWith("/user-attachments/assets/");
	} catch {
		return false;
	}
}
function isBareAutolink(children, href) {
	return import_react.Children.toArray(children).join("").trim() === href;
}
function isGitHubUserAttachmentVideoLink(href, children) {
	return isGitHubUserAttachmentUrl(href) && isBareAutolink(children, href);
}
function AttachmentFallbackLink({ href, children }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
		href,
		target: "_blank",
		rel: "noreferrer",
		className: "break-all text-primary underline underline-offset-2 hover:text-primary/80",
		onClick: (e) => e.stopPropagation(),
		children
	});
}
function GitHubUserAttachmentVideo({ href, children }) {
	const [failed, setFailed] = import_react.useState(false);
	if (failed) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AttachmentFallbackLink, {
		href,
		children
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("video", {
		src: href,
		controls: true,
		preload: "metadata",
		playsInline: true,
		className: "my-3 max-h-[28rem] max-w-full rounded-md bg-black/80 outline outline-1 outline-black/10 dark:outline-white/10",
		onClick: (e) => e.stopPropagation(),
		onError: () => setFailed(true),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
			href,
			target: "_blank",
			rel: "noreferrer",
			children
		})
	});
}
function GitHubUserAttachmentImage({ src, alt }) {
	const [failed, setFailed] = import_react.useState(false);
	const label = alt?.trim() || src;
	if (failed) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AttachmentFallbackLink, {
		href: src,
		children: label
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
		href: src,
		target: "_blank",
		rel: "noreferrer",
		className: "inline-block max-w-full",
		onClick: (e) => e.stopPropagation(),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
			src,
			alt: alt ?? "",
			className: "my-3 max-h-96 max-w-full rounded-md object-contain outline outline-1 outline-black/10 dark:outline-white/10",
			onError: () => setFailed(true)
		})
	});
}
function ExpandableMarkdownImage({ src, alt, className, triggerClassName }) {
	const [open, setOpen] = import_react.useState(false);
	const label = alt?.trim() || translate("auto.components.sidebar.MarkdownImageLightbox.image", "Image");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Dialog, {
		open,
		onOpenChange: setOpen,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				className: cn("my-3 block max-w-full cursor-zoom-in border-0 bg-transparent p-0 text-left", triggerClassName),
				onClick: (event) => {
					event.stopPropagation();
				},
				"aria-label": translate("auto.components.sidebar.MarkdownImageLightbox.expand", "Expand image"),
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
					src,
					alt: alt ?? "",
					className: cn(className, "pointer-events-none")
				})
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			"aria-describedby": void 0,
			showCloseButton: false,
			className: "flex h-[90dvh] w-[90vw] max-w-[90vw] flex-col gap-0 overflow-hidden p-0 sm:max-w-[90vw]",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
					className: "sr-only",
					children: label
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex shrink-0 items-center justify-between border-b border-border px-3 py-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "min-w-0 truncate text-sm font-medium text-foreground",
						children: label
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogClose, {
						asChild: true,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "ghost",
							size: "icon-sm",
							"aria-label": translate("auto.components.sidebar.MarkdownImageLightbox.close", "Close"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-4" })
						})
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex min-h-0 flex-1 items-center justify-center overflow-auto bg-muted/20 p-4 scrollbar-editor",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
						src,
						alt: label,
						className: "max-h-full max-w-full rounded-md object-contain"
					})
				})
			]
		})]
	});
}
function isTrustedCompactImageSrc(src) {
	if (!src) return false;
	const normalized = src.trim().toLowerCase();
	return normalized.startsWith("blob:") || /^data:image\/(?:png|jpe?g|gif|webp);base64,/.test(normalized);
}
function handleMarkdownAnchorClick(event, href, onLinkClick) {
	event.stopPropagation();
	if (href?.trim().toLowerCase().startsWith("file:")) event.preventDefault();
	onLinkClick?.(event, href);
}
function handleMarkdownImageClick(event, src, onLinkClick) {
	if (!onLinkClick) return;
	event.stopPropagation();
	onLinkClick(event, src);
}
function createCompactCommentMarkdownComponents(onLinkClick, expandImages = false) {
	return {
		p: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "comment-md-p",
			children
		}),
		a: ({ href, children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
			href: href || void 0,
			target: "_blank",
			rel: "noreferrer",
			className: "underline underline-offset-2 text-foreground/80 hover:text-foreground",
			onClick: (e) => handleMarkdownAnchorClick(e, href, onLinkClick),
			children
		}),
		code: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", {
			className: "rounded bg-accent px-1 py-px text-[10px] font-mono [overflow-wrap:anywhere]",
			children
		}),
		pre: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
			className: "my-1 max-h-32 max-w-full overflow-x-auto rounded bg-accent p-1.5 text-[10px] font-mono",
			children
		}),
		ul: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("ul", {
			className: "my-0.5 ml-3 list-disc space-y-0",
			children
		}),
		ol: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("ol", {
			className: "my-0.5 ml-3 list-decimal space-y-0",
			children
		}),
		li: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", {
			className: "leading-normal [&>input]:pointer-events-none",
			children
		}),
		h1: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "comment-md-h comment-md-h1 font-bold",
			role: "heading",
			"aria-level": 1,
			children
		}),
		h2: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "comment-md-h comment-md-h2 font-bold",
			role: "heading",
			"aria-level": 2,
			children
		}),
		h3: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "comment-md-h comment-md-h3 font-semibold",
			role: "heading",
			"aria-level": 3,
			children
		}),
		h4: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "comment-md-h font-semibold",
			role: "heading",
			"aria-level": 4,
			children
		}),
		h5: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "comment-md-h font-semibold",
			role: "heading",
			"aria-level": 5,
			children
		}),
		h6: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "comment-md-h font-semibold",
			role: "heading",
			"aria-level": 6,
			children
		}),
		hr: () => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("hr", { className: "my-1 border-border/50" }),
		blockquote: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("blockquote", {
			className: "my-0.5 border-l-2 border-border/60 pl-2 text-muted-foreground/80",
			children
		}),
		img: ({ alt, src }) => {
			if (!isTrustedCompactImageSrc(src)) {
				if (!src) return alt ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: alt }) : null;
				return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
					href: src || void 0,
					target: "_blank",
					rel: "noreferrer",
					className: "underline underline-offset-2 text-foreground/80 hover:text-foreground",
					onClick: (e) => handleMarkdownAnchorClick(e, src, onLinkClick),
					children: alt || src
				});
			}
			if (expandImages) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExpandableMarkdownImage, {
				src,
				alt,
				triggerClassName: "my-1",
				className: "max-h-32 max-w-full rounded-sm object-contain outline outline-1 outline-border/70"
			});
			const image = /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
				src,
				alt: alt ?? "",
				className: "my-1 max-h-32 max-w-full rounded-sm object-contain outline outline-1 outline-border/70"
			});
			return src ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
				href: src || void 0,
				target: "_blank",
				rel: "noreferrer",
				onClick: (e) => handleMarkdownAnchorClick(e, src, onLinkClick),
				children: image
			}) : image;
		},
		table: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "my-1 max-w-full overflow-x-auto",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("table", {
				className: "text-[10px] border-collapse [&_td]:border [&_td]:border-border/40 [&_td]:px-1 [&_td]:py-0.5 [&_th]:border [&_th]:border-border/40 [&_th]:px-1 [&_th]:py-0.5 [&_th]:font-semibold [&_th]:text-left",
				children
			})
		})
	};
}
function createDocumentCommentMarkdownComponents(onLinkClick) {
	return {
		p: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
			className: "my-2 first:mt-0 last:mb-0",
			children
		}),
		a: ({ href, children }) => isGitHubUserAttachmentVideoLink(href, children) ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitHubUserAttachmentVideo, {
			href,
			children
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
			href: href || void 0,
			target: "_blank",
			rel: "noreferrer",
			className: "break-all text-primary underline underline-offset-2 hover:text-primary/80",
			onClick: (e) => handleMarkdownAnchorClick(e, href, onLinkClick),
			children
		}),
		code: ({ className, children }) => isMermaidFence(className) ? renderMermaidFence(children, "my-3 min-w-0 max-w-full overflow-x-auto rounded-md border border-border/60 p-3 [&_.mermaid-block]:min-w-0 [&_.mermaid-block_pre]:my-0 [&_.mermaid-block_pre]:max-h-80 [&_.mermaid-block_pre]:max-w-full [&_.mermaid-block_pre]:overflow-x-auto [&_.mermaid-block_pre]:rounded-md [&_.mermaid-block_pre]:bg-accent [&_.mermaid-block_pre]:p-3 [&_.mermaid-block_pre]:font-mono [&_.mermaid-block_pre]:text-[12px]") : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", {
			className: "rounded bg-accent px-1.5 py-0.5 font-mono text-[0.92em] [overflow-wrap:anywhere]",
			children
		}),
		pre: ({ children }) => isMermaidPre(children) ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_jsx_runtime.Fragment, { children }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
			className: "my-3 max-h-80 max-w-full overflow-x-auto rounded-md bg-accent p-3 font-mono text-[12px]",
			children
		}),
		ul: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("ul", {
			className: "my-2 ml-5 list-disc space-y-1",
			children
		}),
		ol: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("ol", {
			className: "my-2 ml-5 list-decimal space-y-1",
			children
		}),
		li: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", {
			className: "leading-relaxed [&>input]:pointer-events-none",
			children
		}),
		h1: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h1", {
			className: "mb-2 mt-4 text-[18px] font-semibold leading-tight first:mt-0",
			children
		}),
		h2: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
			className: "mb-2 mt-4 text-[16px] font-semibold leading-tight first:mt-0",
			children
		}),
		h3: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
			className: "mb-2 mt-3 text-[15px] font-semibold leading-tight first:mt-0",
			children
		}),
		h4: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h4", {
			className: "mb-1 mt-3 font-semibold first:mt-0",
			children
		}),
		h5: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h5", {
			className: "mb-1 mt-3 font-semibold first:mt-0",
			children
		}),
		h6: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h6", {
			className: "mb-1 mt-3 font-semibold first:mt-0",
			children
		}),
		hr: () => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("hr", { className: "my-4 border-border/60" }),
		blockquote: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("blockquote", {
			className: "my-3 border-l-2 border-border/70 pl-3 text-muted-foreground",
			children
		}),
		img: ({ alt, src }) => {
			if (isGitHubUserAttachmentUrl(src)) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitHubUserAttachmentImage, {
				src,
				alt
			});
			if (!src) return alt ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: alt }) : null;
			if (onLinkClick) {
				const imageClassName = [
					"my-3 max-h-96 max-w-full rounded-md object-contain",
					"outline outline-1 outline-black/10 dark:outline-white/10",
					"cursor-pointer"
				].join(" ");
				return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
					src,
					alt: alt ?? "",
					className: imageClassName,
					onClick: (e) => handleMarkdownImageClick(e, src, onLinkClick)
				});
			}
			return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExpandableMarkdownImage, {
				src,
				alt,
				className: "max-h-96 max-w-full rounded-md object-contain outline outline-1 outline-black/10 dark:outline-white/10"
			});
		},
		table: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "my-3 max-w-full overflow-x-auto rounded-md border border-border/60",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("table", {
				className: "min-w-full border-collapse text-[13px] [&_td]:border [&_td]:border-border/50 [&_td]:px-2 [&_td]:py-1.5 [&_th]:border [&_th]:border-border/50 [&_th]:bg-muted/60 [&_th]:px-2 [&_th]:py-1.5 [&_th]:text-left [&_th]:font-semibold",
				children
			})
		})
	};
}
const compactCommentMarkdownComponents = createCompactCommentMarkdownComponents();
const documentCommentMarkdownComponents = createDocumentCommentMarkdownComponents();
var commentMarkdownUrlTransform = (value, key, node) => {
	if (key === "src" && node?.tagName === "img" && isTrustedCompactImageSrc(value)) return value;
	return defaultUrlTransform(value);
};
var commentMarkdownFileUriUrlTransform = (value, key, node) => {
	if (key === "href" && node?.tagName === "a" && value.trim().toLowerCase().startsWith("file:")) return value;
	return commentMarkdownUrlTransform(value, key, node);
};
var remarkPlugins = [remarkGfm, remarkBreaks];
var GITHUB_REFERENCE_PATTERN = /(?:\b([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+))?#([1-9][0-9]*)\b/g;
function createGitHubIssueUrl(owner, repo, number) {
	return `https://github.com/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/issues/${number}`;
}
function isEmbeddedGitHubReference(value, index) {
	if (index === 0) return false;
	return /[A-Za-z0-9_./-]/.test(value[index - 1] ?? "");
}
function createGitHubReferenceLinkNode(label, owner, repo, number) {
	return {
		type: "link",
		url: createGitHubIssueUrl(owner, repo, number),
		title: null,
		children: [{
			type: "text",
			value: label
		}]
	};
}
function splitGitHubReferenceText(value, defaultRepo) {
	const parts = [];
	let cursor = 0;
	for (const match of value.matchAll(GITHUB_REFERENCE_PATTERN)) {
		const label = match[0];
		const index = match.index ?? 0;
		if (isEmbeddedGitHubReference(value, index)) continue;
		const owner = match[1] ?? defaultRepo.owner;
		const repo = match[2] ?? defaultRepo.repo;
		const number = match[3];
		if (!number) continue;
		if (index > cursor) parts.push({
			type: "text",
			value: value.slice(cursor, index)
		});
		parts.push(createGitHubReferenceLinkNode(label, owner, repo, number));
		cursor = index + label.length;
	}
	if (cursor === 0) return [{
		type: "text",
		value
	}];
	if (cursor < value.length) parts.push({
		type: "text",
		value: value.slice(cursor)
	});
	return parts;
}
function transformGitHubReferenceChildren(node, defaultRepo) {
	if (!node.children || node.type === "link" || node.type === "image") return;
	const nextChildren = [];
	for (const child of node.children) if (child.type === "text" && child.value !== void 0) for (const part of splitGitHubReferenceText(child.value, defaultRepo)) nextChildren.push(part);
	else {
		transformGitHubReferenceChildren(child, defaultRepo);
		nextChildren.push(child);
	}
	node.children = nextChildren;
}
function remarkGitHubReferences(defaultRepo) {
	return () => (tree) => transformGitHubReferenceChildren(tree, defaultRepo);
}
var rehypePlugins = [rehypeRaw, [rehypeSanitize, {
	...defaultSchema,
	tagNames: [
		...defaultSchema.tagNames ?? [],
		"details",
		"summary",
		"sub",
		"sup",
		"ins",
		"kbd"
	],
	attributes: {
		...defaultSchema.attributes,
		a: [
			...defaultSchema.attributes?.a ?? [],
			"href",
			"title"
		],
		details: [...defaultSchema.attributes?.details ?? [], "open"],
		img: [
			...defaultSchema.attributes?.img ?? [],
			"src",
			"alt",
			"title",
			"width",
			"height"
		],
		input: [
			...defaultSchema.attributes?.input ?? [],
			"type",
			"checked",
			"disabled"
		],
		td: [...defaultSchema.attributes?.td ?? [], "align"],
		th: [...defaultSchema.attributes?.th ?? [], "align"]
	},
	protocols: {
		...defaultSchema.protocols,
		href: [...defaultSchema.protocols?.href ?? [], "file"],
		src: [
			...defaultSchema.protocols?.src ?? [],
			"data",
			"blob"
		]
	}
}]];
var CommentMarkdown_default = import_react.memo(import_react.forwardRef(function CommentMarkdown$1({ content, className, variant = "compact", githubRepo, onLinkClick, allowFileUriLinks = false, expandImages = false, ...rest }, ref) {
	const components = import_react.useMemo(() => {
		if (!onLinkClick) return variant === "document" ? documentCommentMarkdownComponents : expandImages ? createCompactCommentMarkdownComponents(void 0, true) : compactCommentMarkdownComponents;
		return variant === "document" ? createDocumentCommentMarkdownComponents(onLinkClick) : createCompactCommentMarkdownComponents(onLinkClick, expandImages);
	}, [
		expandImages,
		variant,
		onLinkClick
	]);
	const activeRemarkPlugins = import_react.useMemo(() => githubRepo ? [...remarkPlugins, remarkGitHubReferences(githubRepo)] : remarkPlugins, [githubRepo]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		ref,
		className: cn("[&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_pre_code]:rounded-none", "min-w-0 max-w-full [overflow-wrap:anywhere]", className),
		...rest,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Markdown, {
			remarkPlugins: activeRemarkPlugins,
			rehypePlugins,
			components,
			urlTransform: allowFileUriLinks ? commentMarkdownFileUriUrlTransform : commentMarkdownUrlTransform,
			children: content
		})
	});
}));
export { CommentMarkdown_default as t };
