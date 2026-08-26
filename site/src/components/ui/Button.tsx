import * as React from "react";
import { cn } from "@/lib/utils";

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "default" | "secondary" | "outline" | "ghost" | "link";
  size?: "default" | "sm" | "lg" | "icon";
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "default", size = "default", ...props }, ref) => {
    return (
      <button
        className={cn(
          "inline-flex items-center justify-center gap-2 whitespace-nowrap text-sm font-medium transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ink/30 disabled:pointer-events-none disabled:opacity-50",
          {
            "bg-ink text-page hover:bg-ink-hover":
              variant === "default",
            "border border-line bg-surface text-ink hover:border-line-strong hover:bg-page-raised":
              variant === "secondary",
            "border border-line bg-transparent text-ink hover:border-line-strong hover:bg-page-raised":
              variant === "outline",
            "text-ink-soft hover:bg-ink/[0.04] hover:text-ink":
              variant === "ghost",
            "text-ink underline-offset-4 hover:underline":
              variant === "link",
            "h-9 rounded-full px-4 py-2": size === "default",
            "h-8 rounded-md px-3 text-xs": size === "sm",
            "h-11 rounded-full px-8 text-base": size === "lg",
            "h-9 w-9 rounded-md": size === "icon",
          },
          className
        )}
        ref={ref}
        {...props}
      />
    );
  }
);
Button.displayName = "Button";

export { Button };
