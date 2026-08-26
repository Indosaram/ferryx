import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/** @type {import('tailwindcss').Config} */
const opaque = (token) => `rgb(var(--${token}-rgb) / <alpha-value>)`;
const translucent = (token) => `rgb(var(--${token}-rgb) / calc(var(--${token}-alpha) * <alpha-value>))`;

export default {
  darkMode: ["class"],
  content: [
    path.resolve(__dirname, "./src/**/*.{astro,html,js,jsx,md,mdx,ts,tsx}"),
    path.resolve(__dirname, "../ui/src/**/*.{js,ts,jsx,tsx}"),
  ],
  theme: {
    extend: {
      colors: {
        page: opaque("page"),
        "nav-fill": opaque("nav-fill"),
        "page-raised": opaque("page-raised"),
        surface: opaque("surface"),
        ink: opaque("ink"),
        "ink-hover": opaque("ink-hover"),
        "ink-soft": opaque("ink-soft"),
        "ink-faint": opaque("ink-faint"),
        line: opaque("line"),
        "line-strong": opaque("line-strong"),
        "mark-off": opaque("mark-off"),
        "code-bg": opaque("code-bg"),
        "code-border": opaque("code-border"),
        "code-ink": opaque("code-ink"),
        border: "hsl(var(--border))",
        input: "hsl(var(--input))",
        ring: "hsl(var(--ring))",
        background: "hsl(var(--background))",
        foreground: "hsl(var(--foreground))",
        primary: {
          DEFAULT: "hsl(var(--primary))",
          foreground: "hsl(var(--primary-foreground))",
        },
        secondary: {
          DEFAULT: "hsl(var(--secondary))",
          foreground: "hsl(var(--secondary-foreground))",
        },
        destructive: {
          DEFAULT: "hsl(var(--destructive))",
          foreground: "hsl(var(--destructive-foreground))",
        },
        muted: {
          DEFAULT: "hsl(var(--muted))",
          foreground: "hsl(var(--muted-foreground))",
        },
        accent: {
          DEFAULT: "hsl(var(--accent))",
          foreground: "hsl(var(--accent-foreground))",
        },
        popover: {
          DEFAULT: "hsl(var(--popover))",
          foreground: "hsl(var(--popover-foreground))",
        },
        card: {
          DEFAULT: "hsl(var(--card))",
          foreground: "hsl(var(--card-foreground))",
        },
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
      boxShadow: {
        nav: "var(--shadow-nav)",
        card: "var(--shadow-card)",
        menu: "var(--shadow-menu)",
        window: "var(--shadow-window)",
      },
      backgroundImage: {
        "demo-wash": "var(--demo-wash)",
      },
      spacing: {
        sidebar: "14.75rem",
        titlebar: "2.25rem",
        tabbar: "2rem",
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
      fontFamily: {
        sans: ["Inter", "-apple-system", "BlinkMacSystemFont", "sans-serif"],
        mono: ["JetBrains Mono", "SF Mono", "Menlo", "monospace"],
      },
      keyframes: {
        "pulse-subtle": {
          "0%, 100%": { opacity: "1" },
          "50%": { opacity: "0.85" },
        },
        float: {
          "0%, 100%": { transform: "translateY(0)" },
          "50%": { transform: "translateY(-4px)" },
        },
      },
      animation: {
        "pulse-subtle": "pulse-subtle 4s ease-in-out infinite",
        float: "float 6s ease-in-out infinite",
      },
    },
  },
  plugins: [],
};
