#!/usr/bin/env python3
"""
Generate the figures for the Orion research paper.

Three figures, all drawn from measured values in the paper:

  fig1_architecture.png  the system block diagram
  fig2_retrieval.png     recall@4 per configuration, with Wilson 95% intervals
  fig3_context.png       observation-space context cost, AX tree vs pixels

Output is 300 dpi PNG at the paper's 7.10 in text width, so the images land
in the document at their natural size.
"""

import os
import math
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch, FancyArrowPatch

HERE = os.path.dirname(os.path.abspath(__file__))
DPI = 300
TEXT_W_IN = 7.10

INK = "#111111"
GREY = "#5a5a5a"
LIGHT = "#e8e8e8"
ACC = "#1f4e79"

plt.rcParams.update({
    "font.family": "serif",
    "font.serif": ["Liberation Serif", "DejaVu Serif"],
    "text.color": INK,
    "axes.labelcolor": INK,
    "xtick.color": INK,
    "ytick.color": INK,
    "axes.edgecolor": INK,
    "savefig.facecolor": "white",
    "figure.facecolor": "white",
})


# ---------------------------------------------------------------- figure 1
def fig_architecture(path):
    fig, ax = plt.subplots(figsize=(TEXT_W_IN, 2.10))
    ax.set_xlim(0, 100)
    ax.set_ylim(0, 100)
    ax.axis("off")

    def box(x, y, w, h, label, sub=None, fc=LIGHT, ec=INK, fs=7.4):
        ax.add_patch(FancyBboxPatch(
            (x, y), w, h, boxstyle="round,pad=0.35,rounding_size=1.2",
            linewidth=0.8, edgecolor=ec, facecolor=fc, zorder=2))
        if sub:
            ax.text(x + w / 2, y + h * 0.62, label, ha="center", va="center",
                    fontsize=fs, fontweight="bold", zorder=3)
            ax.text(x + w / 2, y + h * 0.26, sub, ha="center", va="center",
                    fontsize=fs - 1.2, color=GREY, zorder=3)
        else:
            ax.text(x + w / 2, y + h / 2, label, ha="center", va="center",
                    fontsize=fs, zorder=3)

    def arrow(x1, y1, x2, y2, style="-|>", color=INK, lw=0.9, ls="-", rad=0.0):
        ax.add_patch(FancyArrowPatch(
            (x1, y1), (x2, y2), arrowstyle=style, mutation_scale=7,
            linewidth=lw, color=color, linestyle=ls, zorder=1,
            connectionstyle=f"arc3,rad={rad}"))

    # outer shell
    ax.add_patch(FancyBboxPatch(
        (2, 22), 96, 74, boxstyle="round,pad=0.4,rounding_size=1.5",
        linewidth=1.1, edgecolor=INK, facecolor="none", zorder=1))
    ax.text(4.2, 92.5, "Orion shell  (Tauri v2 / Rust)", fontsize=7.6,
            fontweight="bold", va="center", zorder=3)
    ax.text(95.8, 92.5, "React UI · tray · hotkey · file drop",
            fontsize=6.6, color=GREY, ha="right", va="center", zorder=3)

    row = [
        (5.5, "residency", "resolver"),
        (22.5, "chat", "streaming"),
        (39.5, "RAG", "hybrid BM25+dense"),
        (56.5, "ingest", "PDF DOCX XLSX"),
        (73.5, "voice", "Whisper / Piper"),
    ]
    for x, t, s in row:
        box(x, 66, 15, 13, t, s)
    for i in range(len(row) - 1):
        arrow(row[i][0] + 15, 72.5, row[i + 1][0], 72.5)

    # storage
    box(5.5, 45, 30, 12, "SQLite", "FTS5 + sqlite-vec · chats · audit log")
    # sidecar
    box(39.5, 45, 26, 12, "llama.cpp sidecar", "exactly one model resident")
    # automation
    box(69.5, 45, 25.5, 12, "automation", "AX-tree browser + OS driver")

    arrow(13, 66, 13, 57)
    arrow(52.5, 66, 52.5, 57)
    arrow(82, 66, 82, 57)

    # broker
    box(5.5, 27, 89.5, 12.5, "capability broker",
        "T0 read · T1 propose · T2 confirm · T3 approve  —  "
        "refuses any call whose provenance traces to untrusted content",
        fc="#dce6f1", fs=7.6)

    arrow(20, 45, 20, 39.5)
    arrow(52.5, 45, 52.5, 39.5)
    arrow(82, 45, 82, 39.5)

    # egress deny
    ax.text(95, 33.2, "no egress", fontsize=6.6, color=ACC,
            ha="right", va="center", style="italic")

    fig.subplots_adjust(left=0.004, right=0.996, top=0.996, bottom=0.004)
    fig.savefig(path, dpi=DPI)
    plt.close(fig)


# ---------------------------------------------------------------- figure 2
def wilson(k, n, z=1.96):
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    m = (z / d) * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return c - m, c + m


def fig_retrieval(path):
    labels = ["BM25 only", "Dense only", "Hybrid\n(BM25+dense)", "Hybrid,\nno heading trail"]
    ks = [19, 18, 20, 16]
    n = 26
    vals = [k / n for k in ks]
    errs = [[v - wilson(k, n)[0] for v, k in zip(vals, ks)],
            [wilson(k, n)[1] - v for v, k in zip(vals, ks)]]
    colors = ["#b9c6d6", "#b9c6d6", "#1f4e79", "#c9c9c9"]

    fig, ax = plt.subplots(figsize=(TEXT_W_IN, 1.72))
    x = range(len(labels))
    bars = ax.bar(x, vals, width=0.58, color=colors, edgecolor=INK, linewidth=0.7,
                  zorder=3)
    ax.errorbar(list(x), vals, yerr=errs, fmt="none", ecolor=INK,
                elinewidth=0.9, capsize=3.5, zorder=4)

    for i, (b, v) in enumerate(zip(bars, vals)):
        ax.text(b.get_x() + b.get_width() / 2, v + errs[1][i] + 0.022,
                f"{v:.3f}", ha="center", va="bottom", fontsize=7.6,
                fontweight="bold")

    ax.set_xticks(list(x))
    ax.set_xticklabels(labels, fontsize=7.4)
    ax.set_ylabel("recall@4", fontsize=8)
    ax.set_ylim(0, 1.16)
    ax.set_yticks([0, 0.2, 0.4, 0.6, 0.8, 1.0])
    ax.grid(axis="y", linewidth=0.4, color="#cccccc", zorder=0)
    ax.set_axisbelow(True)
    for s in ("top", "right"):
        ax.spines[s].set_visible(False)
    ax.tick_params(length=2.5, labelsize=7.4)
    ax.annotate("bars: 26 answerable questions   "
                "whiskers: Wilson 95% interval",
                xy=(0.5, 1.055), xycoords="axes fraction", ha="center", va="bottom",
                fontsize=6.8, color=GREY)

    fig.subplots_adjust(left=0.075, right=0.995, top=0.995, bottom=0.185)
    fig.savefig(path, dpi=DPI)
    plt.close(fig)


# ---------------------------------------------------------------- figure 3
def fig_context(path):
    fig, ax = plt.subplots(figsize=(TEXT_W_IN, 1.02))
    names = ["Accessibility tree\n(our AX driver)", "Pixel dump\n(screenshot)"]
    vals = [5.0, 1400.0]
    labels = ["2-5 KB", "1.4 MB"]
    colors = ["#1f4e79", "#c9c9c9"]

    y = [1, 0]
    bars = ax.barh(y, vals, height=0.5, color=colors, edgecolor=INK,
                   linewidth=0.7, zorder=3)
    ax.set_xscale("log")
    ax.set_xlim(1, 40000)
    for yy, v, lb in zip(y, vals, labels):
        ax.text(v * 1.35, yy, lb, va="center", ha="left", fontsize=7.6,
                fontweight="bold")
    ax.set_yticks(y)
    ax.set_yticklabels(names, fontsize=7.4)
    ax.set_xlabel("context bytes per observation (log scale)", fontsize=7.6)
    ax.set_xticks([1, 10, 100, 1000, 10000])
    ax.set_xticklabels(["1 B", "10 B", "100 B", "1 KB", "10 KB"], fontsize=7.0)
    ax.grid(axis="x", linewidth=0.4, color="#cccccc", zorder=0)
    ax.set_axisbelow(True)
    for s in ("top", "right"):
        ax.spines[s].set_visible(False)
    ax.tick_params(length=2.5)
    ax.text(0.30, 0.50, "280-700x smaller", transform=ax.transAxes,
            ha="left", va="center", fontsize=7.2, color=ACC, style="italic",
            zorder=5)

    fig.subplots_adjust(left=0.215, right=0.995, top=0.985, bottom=0.31)
    fig.savefig(path, dpi=DPI)
    plt.close(fig)


if __name__ == "__main__":
    fig_architecture(os.path.join(HERE, "fig1_architecture.png"))
    fig_retrieval(os.path.join(HERE, "fig2_retrieval.png"))
    fig_context(os.path.join(HERE, "fig3_context.png"))
    for f in ("fig1_architecture.png", "fig2_retrieval.png", "fig3_context.png"):
        p = os.path.join(HERE, f)
        print(f"  {f}  {os.path.getsize(p)/1024:.0f} KB")
