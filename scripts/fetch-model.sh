#!/usr/bin/env bash
#
# Download the optimal GGUF model for this machine based on detected hardware RAM
# and selected workload persona.
#
# Hardware Tiers:
#   T1 (<= 10 GB RAM):  3B model   (~2.1 GB) - safe for 8 GB / 5.7 GB usable laptops
#   T2 (11-20 GB RAM):  7B model   (~4.7 GB) - optimal for 16 GB machines
#   T3 (21-48 GB RAM): 14B model   (~8.9 GB) - optimal for 24-32 GB workstations
#   T4 (> 48 GB RAM):  32B model  (~19.9 GB) - maximum local power for 64 GB+ rigs
#
# Usage:
#   ./scripts/fetch-model.sh [persona] [tier]
#
# Examples:
#   ./scripts/fetch-model.sh              # Auto-detects RAM, fetches general model
#   ./scripts/fetch-model.sh coder        # Auto-detects RAM, fetches best Coder model
#   ./scripts/fetch-model.sh coder t1     # Force Tier 1 (3B) Coder model
#   ./scripts/fetch-model.sh researcher t2 # Force Tier 2 (7B) Reasoning model

set -euo pipefail

ROLE="${1:-general}"
TIER_ARG="${2:-auto}"

# 1. Detect System RAM (GiB)
detect_ram_gib() {
  case "$(uname -s)" in
    Linux)
      if [ -f /proc/meminfo ]; then
        local kb
        kb=$(grep MemTotal /proc/meminfo | awk '{print $2}')
        echo $(( kb / 1024 / 1024 ))
        return
      fi
      ;;
    Darwin)
      local bytes
      bytes=$(sysctl -n hw.memsize 2>/dev/null || echo 0)
      echo $(( bytes / 1024 / 1024 / 1024 ))
      return
      ;;
    MINGW*|MSYS*|CYGWIN*)
      local bytes
      bytes=$(powershell.exe -NoProfile -Command "(Get-CimInstance Win32_PhysicalMemory | Measure-Object -Property Capacity -Sum).Sum" 2>/dev/null || echo 0)
      echo $(( bytes / 1024 / 1024 / 1024 ))
      return
      ;;
  esac
  echo 8 # Default fallback
}

SYSTEM_RAM_GIB=$(detect_ram_gib)
echo "==> detected physical RAM: ~${SYSTEM_RAM_GIB} GiB"

# 2. Select Tier
if [ "$TIER_ARG" = "auto" ]; then
  if [ "$SYSTEM_RAM_GIB" -le 10 ]; then
    TIER="t1"
  elif [ "$SYSTEM_RAM_GIB" -le 20 ]; then
    TIER="t2"
  elif [ "$SYSTEM_RAM_GIB" -le 48 ]; then
    TIER="t3"
  else
    TIER="t4"
  fi
else
  TIER="$(echo "$TIER_ARG" | tr '[:upper:]' '[:lower:]')"
fi

echo "==> active deployment tier: $TIER"

# 3. Model Matrix (Role x Tier)
case "$ROLE" in
  coder|developer|dev)
    case "$TIER" in
      t1)
        REPO="Qwen/Qwen2.5-Coder-3B-Instruct-GGUF"
        FILE="qwen2.5-coder-3b-instruct-q4_k_m.gguf"
        DESC="Qwen2.5-Coder-3B (SOTA 3B Coding Architecture)"
        ;;
      t2)
        REPO="Qwen/Qwen2.5-Coder-7B-Instruct-GGUF"
        FILE="qwen2.5-coder-7b-instruct-q4_k_m.gguf"
        DESC="Qwen2.5-Coder-7B (Flagship 7B Coding Model)"
        ;;
      t3)
        REPO="Qwen/Qwen2.5-Coder-14B-Instruct-GGUF"
        FILE="qwen2.5-coder-14b-instruct-q4_k_m.gguf"
        DESC="Qwen2.5-Coder-14B (Deep Refactoring & Systems Engineering)"
        ;;
      *)
        REPO="Qwen/Qwen2.5-Coder-32B-Instruct-GGUF"
        FILE="qwen2.5-coder-32b-instruct-q4_k_m.gguf"
        DESC="Qwen2.5-Coder-32B (Maximum Coding Power)"
        ;;
    esac
    ;;

  researcher|analyst|reasoner)
    case "$TIER" in
      t1)
        REPO="Qwen/Qwen2.5-3B-Instruct-GGUF"
        FILE="qwen2.5-3b-instruct-q4_k_m.gguf"
        DESC="Qwen2.5-3B (Structured Analysis & Citation Synthesis)"
        ;;
      t2)
        REPO="unsloth/DeepSeek-R1-Distill-Qwen-7B-GGUF"
        FILE="DeepSeek-R1-Distill-Qwen-7B-Q4_K_M.gguf"
        DESC="DeepSeek-R1-Distill-Qwen-7B (Deep Chain-of-Thought Reasoning)"
        ;;
      t3)
        REPO="unsloth/DeepSeek-R1-Distill-Qwen-14B-GGUF"
        FILE="DeepSeek-R1-Distill-Qwen-14B-Q4_K_M.gguf"
        DESC="DeepSeek-R1-Distill-Qwen-14B (High-Order Scientific Reasoning)"
        ;;
      *)
        REPO="unsloth/DeepSeek-R1-Distill-Qwen-32B-GGUF"
        FILE="DeepSeek-R1-Distill-Qwen-32B-Q4_K_M.gguf"
        DESC="DeepSeek-R1-Distill-Qwen-32B (Apex Reasoning Model)"
        ;;
    esac
    ;;

  *) # general or creative
    case "$TIER" in
      t1)
        REPO="Qwen/Qwen2.5-3B-Instruct-GGUF"
        FILE="qwen2.5-3b-instruct-q4_k_m.gguf"
        DESC="Qwen2.5-3B (Balanced Conversational & General Intelligence)"
        ;;
      t2)
        REPO="Qwen/Qwen2.5-7B-Instruct-GGUF"
        FILE="qwen2.5-7b-instruct-q4_k_m.gguf"
        DESC="Qwen2.5-7B (Balanced High-Capability General Intelligence)"
        ;;
      t3)
        REPO="Qwen/Qwen2.5-14B-Instruct-GGUF"
        FILE="qwen2.5-14b-instruct-q4_k_m.gguf"
        DESC="Qwen2.5-14B (High-Performance General Intelligence)"
        ;;
      *)
        REPO="Qwen/Qwen2.5-32B-Instruct-GGUF"
        FILE="qwen2.5-32b-instruct-q4_k_m.gguf"
        DESC="Qwen2.5-32B (Flagship General Intelligence)"
        ;;
    esac
    ;;
esac

# 4. Resolve Target Directory
case "$(uname -s)" in
  Linux)  DATA="${XDG_DATA_HOME:-$HOME/.local/share}/orion" ;;
  Darwin) DATA="$HOME/Library/Application Support/orion" ;;
  *)      DATA="${APPDATA:-$HOME}/orion" ;;
esac

DEST="$DATA/models"
mkdir -p "$DEST"

echo "==> selected model: $DESC"
echo "    file: $FILE"
echo "    from: $REPO"
echo "    into: $DEST"

if [ -f "$DEST/$FILE" ]; then
  echo "==> already installed: $DEST/$FILE"
  exit 0
fi

URL="https://huggingface.co/${REPO}/resolve/main/${FILE}?download=true"
echo "==> downloading $FILE..."

curl -fL --progress-bar "$URL" -o "$DEST/$FILE.part"
mv "$DEST/$FILE.part" "$DEST/$FILE"

echo "==> successfully installed $FILE"
echo "    Orion will automatically load this model on next query."
