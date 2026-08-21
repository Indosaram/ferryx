/** @type {import('tailwindcss').Config} */
const opaque = (token) => `rgb(var(--${token}-rgb) / <alpha-value>)`;
const translucent = (token) => `rgb(var(--${token}-rgb) / calc(var(--${token}-alpha) * <alpha-value>))`;

export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        background: opaque("background"),
        foreground: opaque("foreground"),
        card: opaque("card"),
        "card-foreground": opaque("card-foreground"),
        popover: opaque("popover"),
        "popover-foreground": opaque("popover-foreground"),
        primary: opaque("primary"),
        "primary-foreground": opaque("primary-foreground"),
        secondary: opaque("secondary"),
        "secondary-foreground": opaque("secondary-foreground"),
        muted: opaque("muted"),
        "muted-foreground": opaque("muted-foreground"),
        accent: opaque("accent"),
        "accent-foreground": opaque("accent-foreground"),
        destructive: opaque("destructive"),
        "destructive-foreground": opaque("destructive-foreground"),
        border: translucent("border"),
        input: translucent("input"),
        ring: opaque("ring"),
        terminal: opaque("terminal"),
        sidebar: opaque("sidebar"),
        "sidebar-foreground": opaque("sidebar-foreground"),
        "sidebar-primary": opaque("sidebar-primary"),
        "sidebar-primary-foreground": opaque("sidebar-primary-foreground"),
        "sidebar-accent": opaque("sidebar-accent"),
        "sidebar-accent-foreground": opaque("sidebar-accent-foreground"),
        "sidebar-border": translucent("sidebar-border"),
        "sidebar-ring": opaque("sidebar-ring"),
        "worktree-sidebar": opaque("worktree-sidebar"),
        "worktree-sidebar-foreground": opaque("worktree-sidebar-foreground"),
        "worktree-sidebar-accent": opaque("worktree-sidebar-accent"),
        "worktree-sidebar-accent-foreground": opaque("worktree-sidebar-accent-foreground"),
        "worktree-sidebar-border": translucent("worktree-sidebar-border"),
        "worktree-sidebar-ring": opaque("worktree-sidebar-ring"),
        status: {
          working: opaque("status-working"),
          warning: opaque("status-warning"),
          success: opaque("status-success"),
          idle: opaque("status-idle"),
        },
      },
      fontFamily: {
        sans: ["Geist", "ui-sans-serif", "system-ui", "-apple-system", "BlinkMacSystemFont", "Segoe UI", "sans-serif"],
      },
      spacing: {
        sidebar: "16.5rem",
        titlebar: "2.5rem",
        tabbar: "2.25rem",
      },
      minWidth: {
        tab: "8rem",
      },
      maxWidth: {
        tab: "14rem",
        agent: "12rem",
        error: "28rem",
      },
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
      },
      keyframes: {
        enter: {
          from: { opacity: "0", transform: "translateY(2px)" },
          to: { opacity: "1", transform: "translateY(0)" },
        },
      },
      animation: {
        enter: "enter 140ms ease-out",
      },
    },
  },
  plugins: [],
};
