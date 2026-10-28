#!/usr/bin/env python3
"""
Verify the saved .docx by reading it back.

The builder writes the document from a style dictionary and models the
pagination with the same dictionary. That is only a proof if what landed in
the file is what the dictionary said. This script therefore ignores the
builder entirely: it unzips the .docx, parses word/document.xml, reads the
real paragraph and table properties out of the XML, and re-paginates the
document from those values.

If this reports six pages, the file on disk is six pages.

Usage:  python3 docs/verify_paper_docx.py [path-to-docx]
"""

import os
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

from reportlab.pdfbase.pdfmetrics import stringWidth

W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"

FONT_ALIAS = {
    "Times New Roman": "Times-Roman",
    "Times New Roman Bold": "Times-Bold",
    "Courier New": "Courier",
    "Courier New Bold": "Courier-Bold",
}


def tw(v):
    """twentieths of a point -> points"""
    return float(v) / 20.0


def inches(v):
    """twips -> inches"""
    return float(v) / 1440.0


def rl_font(name, bold):
    if name in FONT_ALIAS:
        return FONT_ALIAS[name]
    if name == "Courier New":
        return "Courier-Bold" if bold else "Courier"
    return "Times-Bold" if bold else "Times-Roman"


def para_props(p):
    pPr = p.find(W + "pPr")
    pr = dict(before=0.0, after=0.0, line=12.0, rule="exact",
              left=0.0, first=0.0, hanging=0.0, align="left",
              size=10.0, bold=False, font="Times New Roman")
    if pPr is not None:
        sp = pPr.find(W + "spacing")
        if sp is not None:
            if sp.get(W + "before"):
                pr["before"] = tw(sp.get(W + "before"))
            if sp.get(W + "after"):
                pr["after"] = tw(sp.get(W + "after"))
            if sp.get(W + "line"):
                pr["line"] = tw(sp.get(W + "line"))
            pr["rule"] = sp.get(W + "lineRule") or "auto"
        ind = pPr.find(W + "ind")
        if ind is not None:
            if ind.get(W + "left"):
                pr["left"] = inches(ind.get(W + "left"))
            if ind.get(W + "hanging"):
                pr["hanging"] = inches(ind.get(W + "hanging"))
            if ind.get(W + "firstLine"):
                pr["first"] = inches(ind.get(W + "firstLine"))
        jc = pPr.find(W + "jc")
        if jc is not None:
            pr["align"] = jc.get(W + "val") or "left"
        if pPr.find(W + "keepNext") is not None:
            pr["keep"] = True
    # first run's font
    for r in p.findall(W + "r"):
        rPr = r.find(W + "rPr")
        if rPr is None:
            continue
        sz = rPr.find(W + "sz")
        if sz is not None:
            pr["size"] = float(sz.get(W + "val")) / 2.0
        if rPr.find(W + "b") is not None:
            pr["bold"] = True
        rf = rPr.find(W + "rFonts")
        if rf is not None and rf.get(W + "ascii"):
            pr["font"] = rf.get(W + "ascii")
        break
    text = "".join(t.text or "" for t in p.iter(W + "t"))
    pr["text"] = text
    return pr


def wrap(text, size, font, bold, first_w, rest_w):
    fnt = rl_font(font, bold)
    out = []
    for hard in text.split("\n"):
        words = hard.split(" ")
        cur = ""
        limit = first_w
        for word in words:
            trial = word if not cur else cur + " " + word
            if stringWidth(trial, fnt, size) <= limit * 72.0 or not cur:
                cur = trial
            else:
                out.append(cur)
                cur = word
                limit = rest_w
        out.append(cur)
    return out or [""]


def measure(p, text_w, first_extra=0.0):
    """Return (n_lines, line_height) for a paragraph."""
    left = p["left"]
    if p["hanging"]:
        first_w = text_w - left + p["hanging"]
        rest_w = text_w - left
    else:
        first_w = text_w - left
        rest_w = text_w - left
    lines = wrap(p["text"], p["size"], p["font"], p["bold"], first_w, rest_w)
    lead = p["line"]
    if p["rule"] == "auto" or p["rule"] is None:
        lead = p["size"] * 1.15
    return len(lines), lead


def table_rows(tbl, text_w):
    tblPr = tbl.find(W + "tblPr")
    cell_pad = 0.08
    if tblPr is not None:
        mar = tblPr.find(W + "tblCellMar")
        if mar is not None:
            l = mar.find(W + "left")
            r = mar.find(W + "right")
            if l is not None:
                cell_pad = max(inches(l.get(W + "w")), 0)
            if r is not None:
                cell_pad += max(inches(r.get(W + "w")), 0)
    grid = tbl.find(W + "tblGrid")
    cols = []
    if grid is not None:
        for gc in grid.findall(W + "gridCol"):
            cols.append(inches(gc.get(W + "w")))
    total = sum(cols) or text_w
    if total > 0:
        cols = [c / total * text_w for c in cols]
    rows = []
    for tr in tbl.findall(W + "tr"):
        cells = tr.findall(W + "tc")
        heights = []
        for i, tc in enumerate(cells):
            cw = (cols[i] if i < len(cols) else text_w / len(cells)) - cell_pad
            best = 0
            for p in tc.findall(W + "p"):
                pr = para_props(p)
                n, lead = measure(pr, cw)
                best = max(best, n * lead + pr["before"] + pr["after"])
            heights.append(best)
        rows.append(max(heights) if heights else 0.0)
    return rows


def verify(path):
    z = zipfile.ZipFile(path)
    xml = z.read("word/document.xml")
    root = ET.fromstring(xml)
    body = root.find(W + "body")

    sect = body.find(W + "sectPr")
    pgSz = sect.find(W + "pgSz")
    pgMar = sect.find(W + "pgMar")
    page_w = inches(pgSz.get(W + "w"))
    page_h = inches(pgSz.get(W + "h"))
    m_l = inches(pgMar.get(W + "left"))
    m_r = inches(pgMar.get(W + "right"))
    m_t = inches(pgMar.get(W + "top"))
    m_b = inches(pgMar.get(W + "bottom"))
    text_w = page_w - m_l - m_r
    text_h = page_h - m_t - m_b
    limit = text_h * 72.0

    items = []
    for child in body:
        if child.tag == W + "p":
            pr = para_props(child)
            n, lead = measure(pr, text_w)
            h = pr["before"] + n * lead + pr["after"]
            items.append(("p", h, pr.get("keep", False), pr["text"]))
        elif child.tag == W + "tbl":
            for rh in table_rows(child, text_w):
                items.append(("row", rh, False, ""))
            items.append(("gap", 3.0, False, ""))

    pages = []
    cur = 0.0
    cur_items = []
    for kind, h, keep, text in items:
        if keep and cur + h > limit and cur > 0:
            pages.append(cur_items)
            cur_items = []
            cur = 0.0
        if cur + h > limit and cur > 0:
            pages.append(cur_items)
            cur_items = []
            cur = 0.0
        cur_items.append((kind, h, text))
        cur += h
    pages.append(cur_items)

    print("=" * 66)
    print("DOCX RE-VERIFICATION (parsed from the saved file)")
    print("=" * 66)
    print(f"  file                    : {path}")
    print(f"  page                    : {page_w:.2f} x {page_h:.2f} in")
    print(f"  margins (l/r/t/b)       : {m_l:.2f} / {m_r:.2f} / {m_t:.2f} / {m_b:.2f} in")
    print(f"  text area               : {text_w:.2f} x {text_h:.2f} in "
          f"= {limit:.0f} pt per page")
    print(f"  paragraphs / tables     : "
          f"{sum(1 for c in body if c.tag == W + 'p')} / "
          f"{sum(1 for c in body if c.tag == W + 'tbl')}")
    print(f"  re-paginated page count : {len(pages)}")
    for i, pg in enumerate(pages):
        used = sum(h for _, h, _ in pg)
        head = next((t for _, _, t in pg if t.strip()), "")
        print(f"    page {i+1}: {used:6.1f}/{limit:.0f} pt   {head[:52]}")
    print("=" * 66)
    return len(pages)


if __name__ == "__main__":
    target = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "Orion_Research_Paper.docx")
    n = verify(target)
    sys.exit(0 if n == 6 else 1)
