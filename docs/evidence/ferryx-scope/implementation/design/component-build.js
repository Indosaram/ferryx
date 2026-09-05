// ui/src/features/ferryx/design/DesignFeedback.tsx
import { useEffect, useReducer, useRef, useState } from "react";

// ui/src/components/ui/button.tsx
import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva } from "class-variance-authority";

// ui/src/lib/cn.ts
import { clsx } from "clsx";
import { twMerge } from "tailwind-merge";
function cn(...inputs) {
  return twMerge(clsx(inputs));
}

// ui/src/components/ui/button.tsx
import { jsxDEV } from "react/jsx-dev-runtime";
var buttonVariants = cva("inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0", {
  variants: {
    variant: {
      default: "bg-primary text-primary-foreground shadow hover:bg-primary/90",
      destructive: "bg-destructive text-destructive-foreground shadow-sm hover:bg-destructive/90",
      outline: "border border-input bg-background shadow-sm hover:bg-accent hover:text-accent-foreground",
      secondary: "bg-secondary text-secondary-foreground shadow-sm hover:bg-secondary/80",
      ghost: "hover:bg-accent hover:text-accent-foreground",
      link: "text-primary underline-offset-4 hover:underline"
    },
    size: {
      default: "h-9 px-4 py-2",
      sm: "h-8 rounded-md px-3 text-xs",
      lg: "h-10 rounded-md px-8",
      icon: "h-9 w-9"
    }
  },
  defaultVariants: {
    variant: "default",
    size: "default"
  }
});
var Button = React.forwardRef(({ className, variant, size, asChild = false, ...props }, ref) => {
  const Comp = asChild ? Slot : "button";
  return /* @__PURE__ */ jsxDEV(Comp, {
    className: cn(buttonVariants({ variant, size, className })),
    ref,
    ...props
  }, undefined, false, undefined, this);
});
Button.displayName = "Button";

// ui/src/components/ui/input.tsx
import * as React2 from "react";
import { jsxDEV as jsxDEV2 } from "react/jsx-dev-runtime";
var Input = React2.forwardRef(({ className, type, ...props }, ref) => {
  return /* @__PURE__ */ jsxDEV2("input", {
    type,
    className: cn("flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-base shadow-sm transition-colors file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 md:text-sm", className),
    ref,
    ...props
  }, undefined, false, undefined, this);
});
Input.displayName = "Input";

// ui/src/components/ui/label.tsx
import * as React3 from "react";
import * as LabelPrimitive from "@radix-ui/react-label";
import { cva as cva2 } from "class-variance-authority";
import { jsxDEV as jsxDEV3 } from "react/jsx-dev-runtime";
var labelVariants = cva2("text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70");
var Label = React3.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ jsxDEV3(LabelPrimitive.Root, {
  ref,
  className: cn(labelVariants(), className),
  ...props
}, undefined, false, undefined, this));
Label.displayName = LabelPrimitive.Root.displayName;

// ui/src/components/ui/select.tsx
import * as React4 from "react";
import * as SelectPrimitive from "@radix-ui/react-select";
import { Check, ChevronDown, ChevronUp } from "lucide-react";
import { jsxDEV as jsxDEV4 } from "react/jsx-dev-runtime";
var Select = SelectPrimitive.Root;
var SelectValue = SelectPrimitive.Value;
var SelectTrigger = React4.forwardRef(({ className, children, ...props }, ref) => /* @__PURE__ */ jsxDEV4(SelectPrimitive.Trigger, {
  ref,
  className: cn("flex h-9 w-full items-center justify-between whitespace-nowrap rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm ring-offset-background placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring disabled:cursor-not-allowed disabled:opacity-50 [&>span]:line-clamp-1", className),
  ...props,
  children: [
    children,
    /* @__PURE__ */ jsxDEV4(SelectPrimitive.Icon, {
      asChild: true,
      children: /* @__PURE__ */ jsxDEV4(ChevronDown, {
        className: "h-4 w-4 opacity-50"
      }, undefined, false, undefined, this)
    }, undefined, false, undefined, this)
  ]
}, undefined, true, undefined, this));
SelectTrigger.displayName = SelectPrimitive.Trigger.displayName;
var SelectScrollUpButton = React4.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ jsxDEV4(SelectPrimitive.ScrollUpButton, {
  ref,
  className: cn("flex cursor-default items-center justify-center py-1", className),
  ...props,
  children: /* @__PURE__ */ jsxDEV4(ChevronUp, {
    className: "h-4 w-4"
  }, undefined, false, undefined, this)
}, undefined, false, undefined, this));
SelectScrollUpButton.displayName = SelectPrimitive.ScrollUpButton.displayName;
var SelectScrollDownButton = React4.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ jsxDEV4(SelectPrimitive.ScrollDownButton, {
  ref,
  className: cn("flex cursor-default items-center justify-center py-1", className),
  ...props,
  children: /* @__PURE__ */ jsxDEV4(ChevronDown, {
    className: "h-4 w-4"
  }, undefined, false, undefined, this)
}, undefined, false, undefined, this));
SelectScrollDownButton.displayName = SelectPrimitive.ScrollDownButton.displayName;
var SelectContent = React4.forwardRef(({ className, children, position = "popper", ...props }, ref) => /* @__PURE__ */ jsxDEV4(SelectPrimitive.Portal, {
  children: /* @__PURE__ */ jsxDEV4(SelectPrimitive.Content, {
    ref,
    className: cn("relative z-50 max-h-96 min-w-[8rem] overflow-hidden rounded-md border bg-popover text-popover-foreground shadow-md data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2", position === "popper" && "data-[side=bottom]:translate-y-1 data-[side=left]:-translate-x-1 data-[side=right]:translate-x-1 data-[side=top]:-translate-y-1", className),
    position,
    ...props,
    children: [
      /* @__PURE__ */ jsxDEV4(SelectScrollUpButton, {}, undefined, false, undefined, this),
      /* @__PURE__ */ jsxDEV4(SelectPrimitive.Viewport, {
        className: cn("p-1", position === "popper" && "h-[var(--radix-select-trigger-height)] w-full min-w-[var(--radix-select-trigger-width)]"),
        children
      }, undefined, false, undefined, this),
      /* @__PURE__ */ jsxDEV4(SelectScrollDownButton, {}, undefined, false, undefined, this)
    ]
  }, undefined, true, undefined, this)
}, undefined, false, undefined, this));
SelectContent.displayName = SelectPrimitive.Content.displayName;
var SelectLabel = React4.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ jsxDEV4(SelectPrimitive.Label, {
  ref,
  className: cn("px-2 py-1.5 text-sm font-semibold", className),
  ...props
}, undefined, false, undefined, this));
SelectLabel.displayName = SelectPrimitive.Label.displayName;
var SelectItem = React4.forwardRef(({ className, children, ...props }, ref) => /* @__PURE__ */ jsxDEV4(SelectPrimitive.Item, {
  ref,
  className: cn("relative flex w-full cursor-default select-none items-center rounded-sm py-1.5 pl-2 pr-8 text-sm outline-none focus:bg-accent focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50", className),
  ...props,
  children: [
    /* @__PURE__ */ jsxDEV4("span", {
      className: "absolute right-2 flex h-3.5 w-3.5 items-center justify-center",
      children: /* @__PURE__ */ jsxDEV4(SelectPrimitive.ItemIndicator, {
        children: /* @__PURE__ */ jsxDEV4(Check, {
          className: "h-4 w-4"
        }, undefined, false, undefined, this)
      }, undefined, false, undefined, this)
    }, undefined, false, undefined, this),
    /* @__PURE__ */ jsxDEV4(SelectPrimitive.ItemText, {
      children
    }, undefined, false, undefined, this)
  ]
}, undefined, true, undefined, this));
SelectItem.displayName = SelectPrimitive.Item.displayName;
var SelectSeparator = React4.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ jsxDEV4(SelectPrimitive.Separator, {
  ref,
  className: cn("-mx-1 my-1 h-px bg-muted", className),
  ...props
}, undefined, false, undefined, this));
SelectSeparator.displayName = SelectPrimitive.Separator.displayName;

// ui/src/features/ferryx/design/DesignFeedback.tsx
import { jsxDEV as jsxDEV5 } from "react/jsx-dev-runtime";
function DesignFeedback({ session, identity, targets, maskPreview }) {
  const [, render] = useReducer((n) => n + 1, 0);
  const [armed, setArmed] = useState(false), [preview, setPreview] = useState(false);
  const [targetKey, setTargetKey] = useState(""), [note, setNote] = useState("");
  const [busy, setBusy] = useState(false), [error, setError] = useState(""), [status, setStatus] = useState("");
  const [imageUrl, setImageUrl] = useState();
  const toggle = useRef(null), noteInput = useRef(null);
  useEffect(() => session.subscribe(() => {
    render();
    if (session.capture) {
      setPreview(true);
      setArmed(false);
    }
  }), [session]);
  useEffect(() => {
    if (!session.capture)
      return;
    const url = URL.createObjectURL(new Blob([session.capture.png.slice()], { type: "image/png" }));
    setImageUrl(url);
    return () => URL.revokeObjectURL(url);
  }, [session.capture]);
  useEffect(() => {
    maskPreview(preview);
    if (preview)
      noteInput.current?.focus();
    return () => maskPreview(false);
  }, [preview, maskPreview]);
  useEffect(() => () => {
    session.cancel().catch((e) => console.error("Design cancellation failed", e));
  }, [session, identity.browserId, identity.webviewLabel, identity.generation, identity.viewportRevision]);
  const run = async (action) => {
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const cancel = () => run(async () => {
    await session.cancel();
    setPreview(false);
    setArmed(false);
    toggle.current?.focus();
  });
  const begin = (mode) => run(async () => {
    await session.begin(identity, mode);
    setArmed(true);
    setStatus("");
  });
  return /* @__PURE__ */ jsxDEV5("section", {
    "aria-label": "Design feedback",
    className: "min-w-0 text-foreground",
    onKeyDown: (e) => {
      if (e.key === "Escape" && !busy) {
        e.stopPropagation();
        cancel();
      }
    },
    children: [
      /* @__PURE__ */ jsxDEV5("div", {
        className: "flex flex-wrap items-center gap-2",
        children: [
          /* @__PURE__ */ jsxDEV5(Button, {
            ref: toggle,
            size: "sm",
            variant: "outline",
            "data-testid": "design-mode-toggle",
            "aria-pressed": armed,
            disabled: busy,
            onClick: () => void (armed ? cancel() : begin("element")),
            children: "Design mode"
          }, undefined, false, undefined, this),
          /* @__PURE__ */ jsxDEV5(Button, {
            size: "sm",
            variant: "ghost",
            "data-testid": "design-element",
            disabled: busy,
            onClick: () => void begin("element"),
            children: "Select element"
          }, undefined, false, undefined, this),
          /* @__PURE__ */ jsxDEV5(Button, {
            size: "sm",
            variant: "ghost",
            "data-testid": "design-area",
            disabled: busy,
            onClick: () => void begin("rectangle"),
            children: "Select area"
          }, undefined, false, undefined, this)
        ]
      }, undefined, true, undefined, this),
      armed && /* @__PURE__ */ jsxDEV5("p", {
        className: "py-2 text-xs text-muted-foreground",
        children: "Select in the page. Escape cancels."
      }, undefined, false, undefined, this),
      preview && session.capture && /* @__PURE__ */ jsxDEV5("div", {
        "data-testid": "design-preview",
        className: "mt-3 grid min-w-0 gap-3 rounded-md border border-border bg-card p-3",
        children: [
          /* @__PURE__ */ jsxDEV5("h3", {
            className: "text-sm font-medium",
            children: "Design feedback"
          }, undefined, false, undefined, this),
          imageUrl && /* @__PURE__ */ jsxDEV5("img", {
            src: imageUrl,
            alt: "Selected browser viewport crop",
            className: "max-h-64 max-w-full object-contain"
          }, undefined, false, undefined, this),
          /* @__PURE__ */ jsxDEV5("p", {
            className: "break-all font-mono text-xs text-muted-foreground",
            children: session.capture.selection.element?.selector ?? "Viewport area"
          }, undefined, false, undefined, this),
          session.capture.selection.element?.contextUnavailable && /* @__PURE__ */ jsxDEV5("p", {
            role: "status",
            className: "text-xs text-muted-foreground",
            children: "Frame internals are unavailable. The image contains visible viewport pixels."
          }, undefined, false, undefined, this),
          /* @__PURE__ */ jsxDEV5(Label, {
            htmlFor: "design-note",
            children: "Note"
          }, undefined, false, undefined, this),
          /* @__PURE__ */ jsxDEV5(Input, {
            ref: noteInput,
            id: "design-note",
            "data-testid": "design-note",
            value: note,
            disabled: busy || !!session.draft,
            onChange: (e) => setNote(e.target.value)
          }, undefined, false, undefined, this),
          /* @__PURE__ */ jsxDEV5(Label, {
            htmlFor: "design-target",
            children: "Send to agent"
          }, undefined, false, undefined, this),
          /* @__PURE__ */ jsxDEV5(Select, {
            value: targetKey,
            onValueChange: setTargetKey,
            disabled: busy || !!session.draft,
            children: [
              /* @__PURE__ */ jsxDEV5(SelectTrigger, {
                id: "design-target",
                "data-testid": "design-target",
                children: /* @__PURE__ */ jsxDEV5(SelectValue, {
                  placeholder: "Choose an agent explicitly"
                }, undefined, false, undefined, this)
              }, undefined, false, undefined, this),
              /* @__PURE__ */ jsxDEV5(SelectContent, {
                children: targets.map(({ target, label, supportsImages }) => /* @__PURE__ */ jsxDEV5(SelectItem, {
                  value: JSON.stringify(target),
                  disabled: !supportsImages,
                  children: [
                    label,
                    " (",
                    target.hostId,
                    ")",
                    !supportsImages ? " - images unsupported" : ""
                  ]
                }, JSON.stringify(target), true, undefined, this))
              }, undefined, false, undefined, this)
            ]
          }, undefined, true, undefined, this),
          targets.length === 0 && /* @__PURE__ */ jsxDEV5("p", {
            role: "status",
            className: "text-xs text-muted-foreground",
            children: "No available agent targets. Your selection is retained."
          }, undefined, false, undefined, this),
          session.draft && /* @__PURE__ */ jsxDEV5("p", {
            className: "text-xs text-muted-foreground",
            children: [
              "Confirmed for ",
              session.draft.target.hostId,
              " / ",
              session.draft.target.backendSessionId,
              ". The note and attachment are locked."
            ]
          }, undefined, true, undefined, this),
          /* @__PURE__ */ jsxDEV5("div", {
            className: "flex flex-wrap justify-end gap-2",
            children: [
              /* @__PURE__ */ jsxDEV5(Button, {
                variant: "ghost",
                size: "sm",
                disabled: busy,
                onClick: () => void cancel(),
                children: "Cancel"
              }, undefined, false, undefined, this),
              !session.draft && /* @__PURE__ */ jsxDEV5(Button, {
                size: "sm",
                disabled: busy || !targets.some((t) => JSON.stringify(t.target) === targetKey && t.supportsImages),
                onClick: () => void run(async () => {
                  const chosen = targets.find((t) => JSON.stringify(t.target) === targetKey && t.supportsImages);
                  if (!chosen)
                    throw new Error("TARGET_EXPIRED");
                  await session.confirm(chosen.target, note, crypto.randomUUID());
                }),
                children: busy ? "Transferring image..." : "Confirm draft"
              }, undefined, false, undefined, this),
              /* @__PURE__ */ jsxDEV5(Button, {
                "data-testid": "design-send",
                size: "sm",
                disabled: busy || !session.draft || !!status,
                onClick: () => void run(async () => {
                  const receipt = await session.send();
                  setStatus(receipt.stage);
                }),
                children: busy ? "Sending..." : "Send feedback"
              }, undefined, false, undefined, this)
            ]
          }, undefined, true, undefined, this)
        ]
      }, undefined, true, undefined, this),
      (error || session.error) && /* @__PURE__ */ jsxDEV5("p", {
        role: "alert",
        className: "py-2 text-xs text-destructive",
        children: error || session.error?.message
      }, undefined, false, undefined, this),
      status && /* @__PURE__ */ jsxDEV5("p", {
        role: "status",
        className: "py-2 text-xs",
        children: [
          "Delivery: ",
          status
        ]
      }, undefined, true, undefined, this)
    ]
  }, undefined, true, undefined, this);
}
export {
  DesignFeedback
};
