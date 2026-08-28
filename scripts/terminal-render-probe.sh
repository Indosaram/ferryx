#!/bin/sh
# terminal-render-probe.sh
# Exhaustive SGR attribute probe and line-wrap stressor for Ferryx native terminal visual QA.
# POSIX sh compatible, no external dependencies.

set -eu

ESC=$(printf '\033')
CSI="${ESC}["
RST="${CSI}0m"

printf "%s=== Ferryx Native Terminal SGR & Render Probe ===%s\n\n" "${CSI}1m" "${RST}"

# --- SGR Text Attributes ---
printf "Normal text: %sThe quick brown fox jumps over the lazy dog.%s\n" "${RST}" "${RST}"
printf "Bold (SGR 1): %sThe quick brown fox jumps over the lazy dog.%s\n" "${CSI}1m" "${RST}"
printf "Faint (SGR 2): %sThe quick brown fox jumps over the lazy dog.%s\n" "${CSI}2m" "${RST}"
printf "Italic (SGR 3): %sThe quick brown fox jumps over the lazy dog.%s\n" "${CSI}3m" "${RST}"
printf "Underline (SGR 4): %sThe quick brown fox jumps over the lazy dog.%s\n" "${CSI}4m" "${RST}"
printf "Blink (SGR 5): %sThe quick brown fox jumps over the lazy dog.%s\n" "${CSI}5m" "${RST}"
printf "Inverse (SGR 7): %sThe quick brown fox jumps over the lazy dog.%s\n" "${CSI}7m" "${RST}"
printf "Invisible (SGR 8): [VISIBLE-PREFIX->|%sHIDDEN TEXT MUST NOT LEAK%s|<-VISIBLE-SUFFIX]\n" "${CSI}8m" "${RST}"
printf "Strikethrough (SGR 9): %sThe quick brown fox jumps over the lazy dog.%s\n" "${CSI}9m" "${RST}"
printf "Overline (SGR 53): %sThe quick brown fox jumps over the lazy dog.%s\n" "${CSI}53m" "${RST}"

printf "\n"

# --- 16-Color Foreground Ramp ---
printf "16-color foreground ramp:\n"
printf "  Standard (30-37): "
c=30
while [ "$c" -le 37 ]; do
  printf "%s[%02d]%s " "${CSI}${c}m" "$c" "${RST}"
  c=$((c + 1))
done
printf "\n"

printf "  Bright   (90-97): "
c=90
while [ "$c" -le 97 ]; do
  printf "%s[%02d]%s " "${CSI}${c}m" "$c" "${RST}"
  c=$((c + 1))
done
printf "\n\n"

# --- 256-Color Ramp Sample ---
printf "256-color ramp sample:\n"
printf "  Cube 6x6x6: "
i=16
while [ "$i" -le 231 ]; do
  printf "%s#%s" "${CSI}38;5;${i}m" "${RST}"
  i=$((i + 3))
done
printf "\n"

printf "  Grayscale:  "
i=232
while [ "$i" -le 255 ]; do
  printf "%s#%s" "${CSI}38;5;${i}m" "${RST}"
  i=$((i + 1))
done
printf "\n\n"

# --- Truecolor Gradient Sample ---
printf "Truecolor gradient sample:\n"
printf "  RGB 24-bit: "
step=0
while [ "$step" -lt 72 ]; do
  if [ "$step" -lt 24 ]; then
    r=$((255 - step * 10))
    g=$((step * 10))
    b=0
  elif [ "$step" -lt 48 ]; then
    s=$((step - 24))
    r=0
    g=$((255 - s * 10))
    b=$((s * 10))
  else
    s=$((step - 48))
    r=$((s * 10))
    g=0
    b=$((255 - s * 10))
  fi
  printf "%s█%s" "${CSI}38;2;${r};${g};${b}m" "${RST}"
  step=$((step + 1))
done
printf "\n\n"

# --- Wrap Stressor (200 Numbered Columns) ---
printf "Wrap stressor (200 columns):\n"

# Ruler 1: Tens indicators (10 cols per block: "0000000001", "1111111112", etc.)
col=1
while [ "$col" -le 200 ]; do
  block=$(((col - 1) / 10))
  printf "%d" "$((block % 10))"
  col=$((col + 1))
done
printf "\n"

# Ruler 2: Repeating 0123456789 units ruler
col=1
while [ "$col" -le 200 ]; do
  printf "%d" "$(((col - 1) % 10))"
  col=$((col + 1))
done
printf "\n"

# 200-column single text line with column boundary markers
col=1
while [ "$col" -le 200 ]; do
  if [ "$col" -eq 1 ]; then
    printf "<"
  elif [ "$col" -eq 200 ]; then
    printf ">"
  elif [ "$((col % 10))" -eq 0 ]; then
    printf "|"
  else
    printf "="
  fi
  col=$((col + 1))
done
printf "\n"
