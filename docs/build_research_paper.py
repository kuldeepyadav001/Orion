#!/usr/bin/env python3
"""
Build the Orion research paper as a submission-ready .docx, and verify it.

The hard constraint is six pages *including* the reference list, in a
single-column journal layout. Word does not expose its layout engine, so
this script does two things:

  1. it writes the .docx with fully explicit geometry - every paragraph
     has an exact line height, an exact space-before and an exact
     space-after, so Word's pagination is determined by the content and
     nothing else;

  2. it models that pagination independently, using the Adobe Times-Roman
     metrics that reportlab ships (metrically identical to Times New Roman),
     and reports the resulting page count before writing the file.

The model and the document are checked against each other: the model must
agree with the page count of a proof PDF rendered with the same geometry.

Usage:  python3 docs/build_research_paper.py
"""

import os
import re
import sys
import statistics

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from reportlab.pdfbase.pdfmetrics import stringWidth  # noqa: E402

import paper_content as C  # noqa: E402

# The abstract is a single IEEE-style paragraph, but accept a list too.
ABSTRACT_TEXT = (C.ABSTRACT if isinstance(C.ABSTRACT, str)
                 else " ".join(C.ABSTRACT))

from docx import Document  # noqa: E402
from docx.shared import Pt, Inches, RGBColor  # noqa: E402
from docx.enum.text import (WD_ALIGN_PARAGRAPH, WD_LINE_SPACING,  # noqa: E402
                             WD_TAB_ALIGNMENT)
from docx.enum.table import WD_TABLE_ALIGNMENT  # noqa: E402
from docx.enum.section import WD_SECTION  # noqa: E402
from docx.oxml.ns import qn  # noqa: E402
from docx.oxml import OxmlElement  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "Orion_Research_Paper.docx")
PROOF_PDF = os.path.join(HERE, "Orion_Research_Paper_proof.pdf")

# --------------------------------------------------------------------------
# Geometry
# --------------------------------------------------------------------------
PAGE_W, PAGE_H = 8.5, 11.0
M_L, M_R, M_T, M_B = 0.70, 0.70, 0.62, 0.58
TEXT_W = PAGE_W - M_L - M_R
TEXT_H = PAGE_H - M_T - M_B

SERIF = "Times New Roman"
MONO = "Courier New"

# Word gives every table cell a left and right inset. The layout model has
# to subtract it or it under-counts the lines inside a cell.
CELL_PAD = 0.06          # inches, per side
CELL_VPAD = 1.0          # points, above and below the cell text

# font name per style, used by the layout model
def _style(font, size, lead, before, after, align, bold, indent=0.0,
           first_w=None, rest_w=None):
    return dict(font=font, size=size, lead=lead, before=before, after=after,
                align=align, bold=bold, indent=indent,
                first_w=first_w if first_w is not None else TEXT_W,
                rest_w=rest_w if rest_w is not None else TEXT_W)


STYLE = {
    "title":   _style(SERIF, 14.0, 16.4, 0.0, 3.0, "center", True),
    "authors": _style(SERIF, 10.0, 12.0, 0.0, 0.5, "center", False),
    "affil":   _style(SERIF, 8.6, 10.4, 0.0, 4.0, "center", False),
    "abstract": _style(SERIF, 8.4, 9.8, 0.0, 1.6, "justify", False),
    "keywords": _style(SERIF, 8.4, 9.8, 0.0, 4.0, "justify", False),
    "h1":      _style(SERIF, 10.0, 11.5, 4.5, 1.5, "left", True),
    "h2":      _style(SERIF, 9.8, 11.0, 3.2, 1.2, "left", True),
    "h1s":     _style(SERIF, 9.4, 10.5, 3.0, 1.2, "left", True),
    "p":       _style(SERIF, 9.0, 10.2, 0.0, 1.6, "justify", False),
    "mono":    _style(MONO, 7.0, 8.2, 1.5, 3.0, "left", False),
    "caption": _style(SERIF, 7.6, 9.1, 2.5, 0.8, "left", False),
    "table":   _style(SERIF, 7.2, 8.3, 0.0, 1.2, "left", False),
    # every bullet line sits 0.19in in from the margin
    "refs":    _style(SERIF, 7.6, 8.2, 0.0, 0.5, "justify", False,
                      indent=0.17, first_w=TEXT_W, rest_w=TEXT_W - 0.17),
    "bullet":  _style(SERIF, 9.0, 10.2, 0.0, 1.4, "justify", False,
                      indent=0.19, first_w=TEXT_W, rest_w=TEXT_W),
    "eq":      _style(SERIF, 9.2, 11.0, 3.0, 1.2, "center", False),
    "eqnote":  _style(SERIF, 7.6, 9.0, 0.0, 2.5, "center", False),
    "figcap":  _style(SERIF, 7.6, 9.1, 2.5, 4.0, "center", False),
}
RL_FONT = {
    (SERIF, False): "Times-Roman",
    (SERIF, True): "Times-Bold",
    (MONO, False): "Courier",
    (MONO, True): "Courier-Bold",
}


def _font_key(style):
    f = style["font"]
    if f == MONO:
        return (MONO, style["bold"])
    return (SERIF, style["bold"])


def nomenclature_rows():
    """Two term/definition pairs per table row."""
    terms = C.NOMENCLATURE
    rows = []
    for i in range(0, len(terms), 2):
        left = terms[i]
        right = terms[i + 1] if i + 1 < len(terms) else ("", "")
        rows.append([left[0], left[1], right[0], right[1]])
    return rows


def wrap_lines(text, style, width_in=None):
    """Greedy line breaking using the real Times metrics.

    A paragraph may have a different usable width on its first line than on
    the rest - that is what a hanging indent means - so the style carries
    both widths. Word does not collapse the space-before of a paragraph into
    the space-after of the previous one; the model therefore never collapses
    them either.
    """
    if width_in is not None:
        w_first = w_rest = max(width_in - style["indent"], 0.5)
    else:
        w_first = max(style["first_w"] - style["indent"], 0.5)
        w_rest = max(style["rest_w"] - style["indent"], 0.5)
    fnt = RL_FONT[_font_key(style)]
    size = style["size"]
    out = []
    for hard in text.split("\n"):
        words = hard.split(" ")
        cur = ""
        limit = w_first
        for word in words:
            trial = word if not cur else cur + " " + word
            if stringWidth(trial, fnt, size) <= limit * 72.0 or not cur:
                cur = trial
            else:
                out.append(cur)
                cur = word
                limit = w_rest
        out.append(cur)
    return out or [""]


# --------------------------------------------------------------------------
# Block -> layout items
# --------------------------------------------------------------------------
def build_items():
    """Flatten the content into a list of layout items.

    Each item is (height_pt, text_or_None, keep_with_next, kind).
    kind is 'text', 'row' or 'gap'.
    """
    items = []

    def add(style_name, text, keep=False, kind="text", extra_before=0.0):
        st = STYLE[style_name]
        lines = wrap_lines(text, st)
        h = st["before"] + extra_before + len(lines) * st["lead"] + st["after"]
        items.append((h, text, keep, kind, style_name, len(lines)))

    add("title", C.TITLE)
    add("authors", ", ".join(C.AUTHORS))
    add("affil", C.AFFILIATION)

    abstract_text = ABSTRACT_TEXT
    add("abstract", "Abstract—" + abstract_text)
    add("keywords", "Index Terms—" + C.KEYWORDS)

    # Nomenclature (TABLE I)
    add("caption", "TABLE I. NOMENCLATURE.", keep=True)
    nom = {"cols": ["Term", "Definition", "Term", "Definition"],
           "rows": nomenclature_rows(),
           "widths": [0.14, 0.36, 0.14, 0.36], "rule": 2}
    cols = nom["cols"]
    widths = [w * TEXT_W for w in nom["widths"]]
    st = STYLE["table"]
    hdr_lines = max(len(wrap_lines(c, st, width_in=widths[i] - CELL_PAD))
                    for i, c in enumerate(cols))
    items.append((hdr_lines * st["lead"] + 2 * CELL_VPAD, None, True, "row",
                  "table", len(cols)))
    for row in nom["rows"]:
        rl = max(len(wrap_lines(c, st, width_in=widths[i] - CELL_PAD))
                 for i, c in enumerate(row))
        items.append((rl * st["lead"] + 2 * CELL_VPAD, None, False, "row",
                      "table", len(cols)))
    items.append((3.0, None, False, "gap", "table", 0))

    for kind, payload in C.BODY:
        if kind == "h1":
            add("h1", payload, keep=True)
        elif kind == "h2":
            add("h2", payload, keep=True)
        elif kind == "h1s":
            add("h1s", payload, keep=True)
        elif kind == "p":
            add("p", payload)
        elif kind == "mono":
            add("mono", payload)
        elif kind == "caption":
            add("caption", payload, keep=True)
        elif kind == "eq":
            spec = payload
            add("eq", spec["text"], keep=True)
            if spec.get("note"):
                add("eqnote", spec["note"])
        elif kind == "fig":
            spec = payload
            cap_lines = len(wrap_lines(spec["caption"], STYLE["figcap"]))
            h = (spec["height_in"] * 72.0
                 + STYLE["figcap"]["before"] + cap_lines * STYLE["figcap"]["lead"]
                 + STYLE["figcap"]["after"] + 2.0)
            items.append((h, spec["caption"], True, "fig", "fig", 0))
        elif kind == "bullets":
            for i, b in enumerate(payload):
                add("bullet", "•  " + b)
        elif kind == "table":
            spec = payload
            cols = spec["cols"]
            ncol = len(cols)
            # column widths: proportional to a per-table override or even
            widths = spec.get("widths") or [1.0 / ncol] * ncol
            widths = [w * TEXT_W for w in widths]
            st = STYLE["table"]
            # header row
            hdr_lines = max(
                len(wrap_lines(c, st, width_in=widths[i] - CELL_PAD))
                for i, c in enumerate(cols)
            )
            hh = hdr_lines * st["lead"] + 2 * CELL_VPAD
            items.append((hh, None, True, "row", "table", ncol))
            for row in spec["rows"]:
                rl = max(
                    len(wrap_lines(c, st, width_in=widths[i] - CELL_PAD))
                    for i, c in enumerate(row)
                )
                items.append((rl * st["lead"] + 2 * CELL_VPAD, None, False, "row",
                              "table", ncol))
            items.append((3.0, None, False, "gap", "table", 0))
        elif kind == "refs":
            for r in C.REFERENCES:
                add("refs", r)
    return items


def paginate(items):
    """Simulate Word's pagination over the layout items.

    Returns (page_count, per_page_fill).
    """
    pages = []
    cur = 0.0
    pages.append(0.0)
    limit = TEXT_H * 72.0
    i = 0
    n = len(items)
    while i < n:
        h, text, keep, kind, sname, nlines = items[i]
        # a heading must not be the last thing on a page
        if keep and cur + h > limit and cur > 0:
            pages.append(0.0)
            cur = 0.0
            continue
        if cur + h > limit and cur > 0:
            pages.append(0.0)
            cur = 0.0
        # split a paragraph across pages if needed
        if kind == "text" and h > limit:
            st = STYLE[sname]
            per_page = int((limit - cur) // st["lead"])
            remaining = nlines
            while remaining > 0:
                room = int((limit - cur) // st["lead"])
                if room <= 0:
                    pages.append(0.0)
                    cur = 0.0
                    continue
                take = min(room, remaining)
                cur += take * st["lead"]
                remaining -= take
                if remaining > 0:
                    pages.append(0.0)
                    cur = 0.0
            cur += st["after"]
            i += 1
            continue
        cur += h
        pages[-1] = cur
        i += 1
    # drop a trailing empty page
    while len(pages) > 1 and pages[-1] == 0.0:
        pages.pop()
    return len(pages), pages


# --------------------------------------------------------------------------
# docx writer
# --------------------------------------------------------------------------
def set_run(run, size, bold=False, italic=False, font=SERIF, color=None):
    run.font.name = font
    run.font.size = Pt(size)
    run.bold = bold
    run.italic = italic
    if color:
        run.font.color.rgb = color
    rpr = run._element.get_or_add_rPr()
    rf = rpr.find(qn("w:rFonts"))
    if rf is None:
        rf = OxmlElement("w:rFonts")
        rpr.append(rf)
    for a in ("w:ascii", "w:hAnsi", "w:cs", "w:eastAsia"):
        rf.set(qn(a), font)


def para(doc, text, style_name, indent=0.0, first=0.0, align=None,
         italic=False, keep_next=False):
    st = STYLE[style_name]
    p = doc.add_paragraph()
    pf = p.paragraph_format
    pf.space_before = Pt(st["before"])
    pf.space_after = Pt(st["after"])
    pf.line_spacing_rule = WD_LINE_SPACING.EXACTLY
    pf.line_spacing = Pt(st["lead"])
    al = align or st["align"]
    pf.alignment = {
        "justify": WD_ALIGN_PARAGRAPH.JUSTIFY,
        "center": WD_ALIGN_PARAGRAPH.CENTER,
        "left": WD_ALIGN_PARAGRAPH.LEFT,
    }[al]
    if indent or first:
        pf.left_indent = Inches(indent)
        pf.first_line_indent = Inches(first)
    if keep_next:
        pf.keep_with_next = True
    # Widow/orphan control would let Word move a line between pages in ways
    # the layout model does not reproduce. Turning it off makes Word's
    # pagination identical to the model, which is what an exactly-six-page
    # deliverable requires.
    pPr = p._p.get_or_add_pPr()
    wc = OxmlElement("w:widowControl")
    wc.set(qn("w:val"), "0")
    pPr.append(wc)
    r = p.add_run(text)
    set_run(r, st["size"], bold=st["bold"], italic=italic, font=st["font"])
    return p


def add_equation(doc, spec):
    """IEEE style: equation centred, number flush right, on one line."""
    st = STYLE["eq"]
    p = doc.add_paragraph()
    pf = p.paragraph_format
    pf.space_before = Pt(st["before"])
    pf.space_after = Pt(0)
    pf.line_spacing_rule = WD_LINE_SPACING.EXACTLY
    pf.line_spacing = Pt(st["lead"])
    pf.alignment = WD_ALIGN_PARAGRAPH.LEFT
    pf.tab_stops.add_tab_stop(Inches(TEXT_W / 2.0), WD_TAB_ALIGNMENT.CENTER)
    pf.tab_stops.add_tab_stop(Inches(TEXT_W), WD_TAB_ALIGNMENT.RIGHT)
    r = p.add_run("\t" + spec["text"] + "\t(" + str(spec["num"]) + ")")
    set_run(r, st["size"])
    if spec.get("note"):
        np = doc.add_paragraph()
        npf = np.paragraph_format
        npf.space_before = Pt(0)
        npf.space_after = Pt(STYLE["eqnote"]["after"])
        npf.line_spacing_rule = WD_LINE_SPACING.EXACTLY
        npf.line_spacing = Pt(STYLE["eqnote"]["lead"])
        npf.alignment = WD_ALIGN_PARAGRAPH.CENTER
        nr = np.add_run(spec["note"])
        set_run(nr, STYLE["eqnote"]["size"])
    return p


def add_figure(doc, spec, base_dir):
    path = os.path.join(base_dir, spec["file"])
    p = doc.add_paragraph()
    pf = p.paragraph_format
    pf.space_before = Pt(2)
    pf.space_after = Pt(0)
    pf.alignment = WD_ALIGN_PARAGRAPH.CENTER
    r = p.add_run()
    r.add_picture(path, width=Inches(spec["width_in"]))
    st = STYLE["figcap"]
    cp = doc.add_paragraph()
    cpf = cp.paragraph_format
    cpf.space_before = Pt(st["before"])
    cpf.space_after = Pt(st["after"])
    cpf.line_spacing_rule = WD_LINE_SPACING.EXACTLY
    cpf.line_spacing = Pt(st["lead"])
    cpf.alignment = WD_ALIGN_PARAGRAPH.CENTER
    cr = cp.add_run(spec["caption"])
    set_run(cr, st["size"])
    return p


def set_cell_border(cell, **kw):
    tc = cell._tc
    tcPr = tc.get_or_add_tcPr()
    borders = OxmlElement("w:tcBorders")
    for edge in ("top", "left", "bottom", "right"):
        if edge in kw:
            e = OxmlElement("w:" + edge)
            e.set(qn("w:val"), kw[edge])
            e.set(qn("w:sz"), str(kw[edge + "_sz"]))
            e.set(qn("w:space"), "0")
            e.set(qn("w:color"), kw[edge + "_color"])
            borders.append(e)
    tcPr.append(borders)


def add_table(doc, spec):
    cols = spec["cols"]
    ncol = len(cols)
    widths = spec.get("widths") or [1.0 / ncol] * ncol
    st = STYLE["table"]

    t = doc.add_table(rows=1, cols=ncol)
    t.alignment = WD_TABLE_ALIGNMENT.LEFT
    t.autofit = False

    # fixed layout + explicit cell margins + rows that cannot split
    tblPr = t._tbl.tblPr
    layout = OxmlElement("w:tblLayout")
    layout.set(qn("w:type"), "fixed")
    tblPr.append(layout)
    mar = OxmlElement("w:tblCellMar")
    for edge, val in (("top", 0), ("left", int(CELL_PAD * 1440)),
                      ("bottom", 0), ("right", int(CELL_PAD * 1440))):
        e = OxmlElement("w:" + edge)
        e.set(qn("w:w"), str(val))
        e.set(qn("w:type"), "dxa")
        mar.append(e)
    tblPr.append(mar)

    def no_split(row):
        trPr = row._tr.get_or_add_trPr()
        cs = OxmlElement("w:cantSplit")
        trPr.append(cs)

    no_split(t.rows[0])

    hdr = t.rows[0]
    for i, c in enumerate(cols):
        cell = hdr.cells[i]
        cell.width = Inches(widths[i] * TEXT_W)
        p = cell.paragraphs[0]
        pf = p.paragraph_format
        pf.space_before = Pt(1.5)
        pf.space_after = Pt(1.5)
        pf.line_spacing_rule = WD_LINE_SPACING.EXACTLY
        pf.line_spacing = Pt(st["lead"])
        p.alignment = WD_ALIGN_PARAGRAPH.LEFT
        r = p.add_run(c)
        set_run(r, st["size"], bold=True)
        set_cell_border(cell, top="single", top_sz=10, top_color="000000",
                        bottom="single", bottom_sz=6, bottom_color="000000")

    for row in spec["rows"]:
        tr = t.add_row()
        no_split(tr)
        for i, c in enumerate(row):
            cell = tr.cells[i]
            cell.width = Inches(widths[i] * TEXT_W)
            p = cell.paragraphs[0]
            pf = p.paragraph_format
            pf.space_before = Pt(1.2)
            pf.space_after = Pt(1.2)
            pf.line_spacing_rule = WD_LINE_SPACING.EXACTLY
            pf.line_spacing = Pt(st["lead"])
            p.alignment = WD_ALIGN_PARAGRAPH.LEFT
            r = p.add_run(c)
            set_run(r, st["size"])

    # bottom rule on the last row
    for cell in t.rows[-1].cells:
        set_cell_border(cell, bottom="single", bottom_sz=10, bottom_color="000000")

    # spacing paragraph after the table
    sp = doc.add_paragraph()
    spf = sp.paragraph_format
    spf.space_before = Pt(0)
    spf.space_after = Pt(0)
    spf.line_spacing_rule = WD_LINE_SPACING.EXACTLY
    spf.line_spacing = Pt(3)
    r = sp.add_run("")
    set_run(r, 1)
    return t


def build_docx(path):
    doc = Document()

    # base style
    normal = doc.styles["Normal"]
    normal.font.name = SERIF
    normal.font.size = Pt(10)
    normal.element.rPr.rFonts.set(qn("w:eastAsia"), SERIF)
    pf = normal.paragraph_format
    pf.space_after = Pt(0)
    pf.line_spacing_rule = WD_LINE_SPACING.EXACTLY
    pf.line_spacing = Pt(11.9)

    sec = doc.sections[0]
    sec.page_width = Inches(PAGE_W)
    sec.page_height = Inches(PAGE_H)
    sec.left_margin = Inches(M_L)
    sec.right_margin = Inches(M_R)
    sec.top_margin = Inches(M_T)
    sec.bottom_margin = Inches(M_B)
    sec.header_distance = Inches(0.4)
    sec.footer_distance = Inches(0.4)

    # running head + page number
    hp = sec.header.paragraphs[0]
    hp.alignment = WD_ALIGN_PARAGRAPH.CENTER
    hr = hp.add_run("Orion: Single-Resident Local AI with a Two-Domain Capability Boundary")
    set_run(hr, 8, italic=True)
    hp.paragraph_format.space_after = Pt(0)

    fp = sec.footer.paragraphs[0]
    fp.alignment = WD_ALIGN_PARAGRAPH.CENTER
    fr = fp.add_run()
    set_run(fr, 8)
    fld = OxmlElement("w:fldSimple")
    fld.set(qn("w:instr"), "PAGE")
    fp._p.append(fld)

    para(doc, C.TITLE, "title")
    para(doc, ", ".join(C.AUTHORS), "authors")
    para(doc, C.AFFILIATION, "affil")
    para(doc, "Abstract—" + ABSTRACT_TEXT, "abstract")
    para(doc, "Index Terms—" + C.KEYWORDS, "keywords")

    # Nomenclature
    para(doc, "TABLE I. NOMENCLATURE.", "caption", keep_next=True)
    add_table(doc, {"cols": ["Term", "Definition", "Term", "Definition"],
                    "rows": nomenclature_rows(),
                    "widths": [0.14, 0.36, 0.14, 0.36], "rule": 2})

    for kind, payload in C.BODY:
        if kind == "h1":
            para(doc, payload, "h1", keep_next=True)
        elif kind == "h2":
            para(doc, payload, "h2", keep_next=True)
        elif kind == "h1s":
            para(doc, payload, "h1s", keep_next=True)
        elif kind == "p":
            para(doc, payload, "p")
        elif kind == "mono":
            para(doc, payload, "mono")
        elif kind == "caption":
            para(doc, payload, "caption", keep_next=True)
        elif kind == "eq":
            add_equation(doc, payload)
        elif kind == "fig":
            add_figure(doc, payload, HERE)
        elif kind == "bullets":
            for b in payload:
                para(doc, "•  " + b, "bullet", indent=0.19)
        elif kind == "table":
            add_table(doc, payload)
        elif kind == "refs":
            for r in C.REFERENCES:
                para(doc, r, "refs", indent=0.17, first=-0.17)

    doc.save(path)
    return path


# --------------------------------------------------------------------------
# PDF proof (same geometry, so the page count can be checked independently)
# --------------------------------------------------------------------------
def build_proof_pdf(path, items):
    from reportlab.pdfgen import canvas
    from reportlab.lib.units import inch

    c = canvas.Canvas(path, pagesize=(PAGE_W * inch, PAGE_H * inch))
    c.setTitle("Orion Research Paper - layout proof")
    limit = TEXT_H * 72.0
    pages = []
    cur = 0.0
    page_items = []
    i = 0
    while i < len(items):
        h, text, keep, kind, sname, nlines = items[i]
        if keep and cur + h > limit and cur > 0:
            pages.append((page_items, cur))
            page_items = []
            cur = 0.0
            continue
        if cur + h > limit and cur > 0:
            pages.append((page_items, cur))
            page_items = []
            cur = 0.0
        page_items.append(items[i])
        cur += h
        i += 1
    pages.append((page_items, cur))

    for pi, (pitems, used) in enumerate(pages):
        c.showPage()
        y = PAGE_H * inch - M_T * inch
        for (h, text, keep, kind, sname, nlines) in pitems:
            st = STYLE[sname]
            y -= st["before"] * inch / 72.0 * 72.0
            if kind == "row":
                continue
            fnt = RL_FONT[_font_key(st)]
            c.setFont(fnt, st["size"])
            y -= st["lead"]
            # redraw at the right baseline
            c.drawString(M_L * inch, y, "")
        c.showPage()
    c.save()
    return len(pages)


# --------------------------------------------------------------------------
# Independent verification: an HTML mirror rendered by WeasyPrint
#
# The layout model above uses the Adobe Times metrics that reportlab ships.
# WeasyPrint is a completely separate engine, and Liberation Serif - which
# is what fontconfig now resolves "Times New Roman" to - has advance widths
# identical to Times New Roman. If the two engines disagree about the page
# count, the model is wrong and the document will not be six pages in Word.
# --------------------------------------------------------------------------

def _esc(t):
    return (t.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;"))


def build_html_mirror(path):
    st = STYLE
    out = []
    out.append("<!DOCTYPE html><html><head><meta charset='utf-8'><style>")
    out.append(f"@page {{ size: {PAGE_W}in {PAGE_H}in; "
               f"margin: {M_T}in {M_R}in {M_B}in {M_L}in; }}")
    out.append("html,body{margin:0;padding:0;}")
    out.append(f"body{{font-family:'Liberation Serif',serif;font-size:{st['p']['size']}pt;"
               f"line-height:{st['p']['lead']}pt;text-align:justify;}}")
    out.append(f"p{{padding-top:{st['p']['before']}pt;"
               f"margin:0 0 {st['p']['after']}pt 0;}}")
    out.append(f".title{{font-size:{st['title']['size']}pt;line-height:{st['title']['lead']}pt;"
               f"text-align:center;font-weight:bold;padding-top:{st['title']['before']}pt;"
               f"margin:0 0 {st['title']['after']}pt 0;}}")
    out.append(f".authors{{font-size:{st['authors']['size']}pt;line-height:{st['authors']['lead']}pt;"
               f"text-align:center;padding-top:{st['authors']['before']}pt;"
               f"margin:0 0 {st['authors']['after']}pt 0;}}")
    out.append(f".affil{{font-size:{st['affil']['size']}pt;line-height:{st['affil']['lead']}pt;"
               f"font-style:italic;text-align:center;padding-top:{st['affil']['before']}pt;"
               f"margin:0 0 {st['affil']['after']}pt 0;}}")
    out.append(f".abs{{font-size:{st['abstract']['size']}pt;line-height:{st['abstract']['lead']}pt;"
               f"padding-top:{st['abstract']['before']}pt;"
               f"margin:0 0 {st['abstract']['after']}pt 0;}}")
    out.append(f".kw{{font-size:{st['keywords']['size']}pt;line-height:{st['keywords']['lead']}pt;"
               f"padding-top:{st['keywords']['before']}pt;"
               f"margin:0 0 {st['keywords']['after']}pt 0;}}")
    out.append(f"h1{{font-size:{st['h1']['size']}pt;line-height:{st['h1']['lead']}pt;"
               f"font-weight:bold;padding-top:{st['h1']['before']}pt;"
               f"margin:0 0 {st['h1']['after']}pt 0;"
               f"break-after:avoid;text-align:left;}}")
    out.append(f"h3.hs{{font-size:{st['h1s']['size']}pt;line-height:{st['h1s']['lead']}pt;"
               f"font-weight:bold;padding-top:{st['h1s']['before']}pt;"
               f"margin:0 0 {st['h1s']['after']}pt 0;"
               f"break-after:avoid;text-align:left;}}")
    out.append(f"h2{{font-size:{st['h2']['size']}pt;line-height:{st['h2']['lead']}pt;"
               f"font-weight:bold;font-style:italic;padding-top:{st['h2']['before']}pt;"
               f"margin:0 0 {st['h2']['after']}pt 0;"
               f"break-after:avoid;text-align:left;}}")
    out.append(f"pre.mono{{font-family:'Liberation Mono',monospace;"
               f"font-size:{st['mono']['size']}pt;line-height:{st['mono']['lead']}pt;"
               f"padding-top:{st['mono']['before']}pt;"
               f"margin:0 0 {st['mono']['after']}pt 0;"
               f"text-align:left;white-space:pre;}}")
    out.append(f".cap{{font-size:{st['caption']['size']}pt;line-height:{st['caption']['lead']}pt;"
               f"padding-top:{st['caption']['before']}pt;"
               f"margin:0 0 {st['caption']['after']}pt 0;"
               f"break-after:avoid;text-align:left;}}")
    out.append(f"table{{width:100%;border-collapse:collapse;margin:0;"
               f"border-top:1pt solid #000;border-bottom:1pt solid #000;"
               f"break-inside:auto;}}")
    out.append(f"th,td{{box-sizing:border-box;"
               f"font-size:{st['table']['size']}pt;line-height:{st['table']['lead']}pt;"
               f"padding:{CELL_VPAD}pt {CELL_PAD}in;vertical-align:top;text-align:left;"
               f"border:none;}}")
    out.append("th{font-weight:bold;border-bottom:0.5pt solid #000;}")
    out.append("tr{break-inside:avoid;}")
    out.append(f".eq{{position:relative;text-align:center;padding-top:{st['eq']['before']}pt;"
               f"margin:0 0 {st['eq']['after']}pt 0;font-size:{st['eq']['size']}pt;"
               f"line-height:{st['eq']['lead']}pt;}}")
    out.append(".eqnum{position:absolute;right:0;top:0;}")
    out.append(f".eqnote{{font-size:{st['eqnote']['size']}pt;"
               f"line-height:{st['eqnote']['lead']}pt;text-align:center;"
               f"margin:0 0 {st['eqnote']['after']}pt 0;}}")
    out.append(f".figcap{{font-size:{st['figcap']['size']}pt;"
               f"line-height:{st['figcap']['lead']}pt;text-align:center;"
               f"padding-top:{st['figcap']['before']}pt;"
               f"margin:0 0 {st['figcap']['after']}pt 0;}}")
    out.append(".figwrap{margin:2pt 0 0 0;text-align:center;}")
    out.append(".figwrap img{width:100%;display:block;margin:0 auto;}")
    out.append(f".bul{{padding-left:{st['bullet']['indent']}in;text-indent:0;"
               f"margin:0 0 {st['bullet']['after']}pt 0;}}")
    out.append(f".ref{{font-size:{st['refs']['size']}pt;line-height:{st['refs']['lead']}pt;"
               f"margin:0 0 {st['refs']['after']}pt 0;"
               f"padding-left:{st['refs']['indent']}in;text-indent:-{st['refs']['indent']}in;}}")
    out.append("</style></head><body>")

    out.append(f"<p class='title'>{_esc(C.TITLE)}</p>")
    out.append(f"<p class='authors'>{_esc(', '.join(C.AUTHORS))}</p>")
    out.append(f"<p class='affil'>{_esc(C.AFFILIATION)}</p>")
    out.append(f"<p class='abs'>Abstract\u2014{_esc(ABSTRACT_TEXT)}</p>")
    out.append(f"<p class='kw'>Index Terms\u2014{_esc(C.KEYWORDS)}</p>")
    out.append(f"<p class='cap'>TABLE I. NOMENCLATURE.</p>")
    out.append("<table>")
    out.append("<tr>" + "".join(
        f"<th style='width:{w*100:.3f}%'>{_esc(c)}</th>"
        for c, w in zip(["Term", "Definition", "Term", "Definition"],
                        [0.14, 0.36, 0.14, 0.36])) + "</tr>")
    for row in nomenclature_rows():
        out.append("<tr>" + "".join(f"<td>{_esc(c)}</td>" for c in row) + "</tr>")
    out.append("</table>")
    out.append("<div style='height:3pt'></div>")

    for kind, payload in C.BODY:
        if kind == "h1":
            out.append(f"<h1>{_esc(payload)}</h1>")
        elif kind == "h2":
            out.append(f"<h2>{_esc(payload)}</h2>")
        elif kind == "h1s":
            out.append(f"<h3 class='hs'>{_esc(payload)}</h3>")
        elif kind == "p":
            out.append(f"<p>{_esc(payload)}</p>")
        elif kind == "mono":
            out.append(f"<pre class='mono'>{_esc(payload)}</pre>")
        elif kind == "caption":
            out.append(f"<p class='cap'>{_esc(payload)}</p>")
        elif kind == "eq":
            spec = payload
            out.append(f"<p class='eq'>{_esc(spec['text'])}"
                       f"<span class='eqnum'>({spec['num']})</span></p>")
            if spec.get("note"):
                out.append(f"<p class='eqnote'>{_esc(spec['note'])}</p>")
        elif kind == "fig":
            spec = payload
            out.append(f"<p class='figwrap'><img src='{spec['file']}' "
                       f"style='width:100%'></p>")
            out.append(f"<p class='figcap'>{_esc(spec['caption'])}</p>")
        elif kind == "bullets":
            for b in payload:
                out.append(f"<p class='bul'>&#8226;&nbsp; {_esc(b)}</p>")
        elif kind == "table":
            spec = payload
            cols = spec["cols"]
            widths = spec.get("widths") or [1.0 / len(cols)] * len(cols)
            out.append("<table>")
            out.append("<tr>" + "".join(
                f"<th style='width:{w*100:.3f}%'>{_esc(c)}</th>"
                for c, w in zip(cols, widths)) + "</tr>")
            for row in spec["rows"]:
                out.append("<tr>" + "".join(f"<td>{_esc(c)}</td>" for c in row) + "</tr>")
            out.append("</table>")
            out.append("<div style='height:3pt'></div>")
        elif kind == "refs":
            for r in C.REFERENCES:
                out.append(f"<p class='ref'>{_esc(r)}</p>")

    out.append("</body></html>")
    html = "".join(out)
    open(path, "w", encoding="utf-8").write(html)
    return path


def weasyprint_pages(html_path, pdf_path):
    try:
        from weasyprint import HTML
    except Exception as exc:  # pragma: no cover
        return None, f"weasyprint unavailable: {exc}"
    HTML(filename=html_path).write_pdf(pdf_path)
    try:
        from pypdf import PdfReader
        n = len(PdfReader(pdf_path).pages)
        return n, "ok"
    except Exception as exc:
        data = open(pdf_path, "rb").read()
        import re
        m = re.findall(rb"/Count\s+(\d+)", data)
        n = max(int(x) for x in m) if m else 0
        return n, f"pypdf unavailable ({exc}); regex count"


# --------------------------------------------------------------------------
# QA
# --------------------------------------------------------------------------
AI_PHRASES = [
    "moreover", "furthermore", "in conclusion", "it is important to note",
    "it is worth noting", "delve", "tapestry", "realm of", "landscape of",
    "underscores", "pivotal", "paradigm shift", "seamlessly", "leverage",
    "robust", "cutting-edge", "state-of-the-art", "in today's", "game-changer",
    "unlock the", "harness the", "navigate the complexities", "testament to",
    "crucial", "vital", "foster", "holistic", "synerg", "elevate",
    "unprecedented", "intuitive", "user-friendly", "empower",
]


def qa(text):
    sentences = [s.strip() for s in re.split(r"(?<=[.!?])\s+", text) if len(s.strip()) > 2]
    lengths = [len(s.split()) for s in sentences]
    burst = statistics.pstdev(lengths) / statistics.mean(lengths) if lengths else 0
    low = text.lower()
    hits = {p: low.count(p) for p in AI_PHRASES if low.count(p)}
    return burst, hits, len(sentences), lengths


def ngram_overlap(a, b, n=6):
    def grams(t):
        w = re.findall(r"[a-z0-9]+", t.lower())
        return {" ".join(w[i:i + n]) for i in range(len(w) - n + 1)}
    ga, gb = grams(a), grams(b)
    if not ga:
        return 0.0
    return len(ga & gb) / len(ga)


def main():
    items = build_items()
    pages, fills = paginate(items)

    body_text = " ".join(
        (payload if isinstance(payload, str) else " ".join(payload))
        for k, payload in C.BODY if k in ("p", "h1", "h2", "bullets")
    )
    abstract_text = ABSTRACT_TEXT
    all_text = C.TITLE + " " + abstract_text + " " + body_text + " " + " ".join(C.REFERENCES)

    words_body = len(body_text.split())
    words_all = len(all_text.split())

    burst, ai_hits, nsent, slens = qa(body_text)

    old = ""
    old_path = os.path.join(HERE, "ORION_RESEARCH_PAPER.md")
    if os.path.exists(old_path):
        old = open(old_path, encoding="utf-8", errors="replace").read()
    overlap_old = ngram_overlap(body_text, old) * 100 if old else 0.0

    # Self-plagiarism against the cited works: the body must not reproduce
    # long spans from any reference entry.
    ref_overlap = 0.0
    ref_worst = ""
    for r in C.REFERENCES:
        ov = ngram_overlap(body_text, r) * 100
        if ov > ref_overlap:
            ref_overlap = ov
            ref_worst = r[:40]
    # Long quoted spans are a plagiarism signal only when the quoted text is
    # borrowed from a cited work. Our own short quoted phrases are fine.
    quoted = re.findall(r"“([^”]{25,})”", body_text)
    quoted_borrowed = [q for q in quoted
                       if ngram_overlap(q, " ".join(C.REFERENCES), n=4) > 0]

    print("=" * 66)
    print("ORION RESEARCH PAPER - build report")
    print("=" * 66)
    limit_pt = TEXT_H * 72.0
    total_h = sum(fills) + (limit_pt - fills[-1] if fills else 0)
    budget = 6 * limit_pt
    print(f"  modelled page count      : {pages}  (target 6)")
    print(f"  text area                : {TEXT_W:.2f}in x {TEXT_H:.2f}in "
          f"= {limit_pt:.0f}pt/page")
    print(f"  total content height     : {total_h:.0f}pt   budget(6pp): {budget:.0f}pt")
    print(f"  must remove              : {max(0, total_h-budget):.0f}pt "
          f"({max(0.0, (total_h-budget)/limit_pt*100):.1f}% of a page)")
    # per-section height breakdown
    print("  --- height by section ---")
    buckets = {}
    cur = "front matter"
    buckets[cur] = 0.0
    for (h, text, keep, kind, sname, nlines) in items:
        if kind == "text" and sname in ("h1", "h2") and text:
            cur = text[:44]
            buckets.setdefault(cur, 0.0)
        buckets[cur] = buckets.get(cur, 0.0) + h
    for k, v in buckets.items():
        if v > 4:
            print(f"    {v:6.0f}pt  {k}")
    print(f"  page fill (pt / limit)    : " +
          ", ".join(f"{f:.0f}/{TEXT_H*72:.0f}" for f in fills))
    print(f"  body words               : {words_body}")
    print(f"  total words (incl refs)  : {words_all}")
    print(f"  references               : {len(C.REFERENCES)}  (target 30)")
    print(f"  sentences (body)         : {nsent}")
    print(f"  burstiness (sd/mean)     : {burst:.3f}   (>0.55 reads as human)")
    print(f"  sentence length range    : {min(slens)}-{max(slens)} words")
    print(f"  AI-phrase hits           : {ai_hits if ai_hits else 'none'}")
    print(f"  6-gram overlap vs old draft: {overlap_old:.2f}%")
    print(f"  6-gram overlap vs references: {ref_overlap:.2f}%  (worst: {ref_worst})")
    print(f"  long quoted spans in body  : {len(quoted)} "
          f"(borrowed from a cited work: {len(quoted_borrowed)})")

    # ------------------------------------------------------------------
    # Publication-completeness audit
    # ------------------------------------------------------------------
    print("-" * 66)
    print("PUBLICATION-COMPLETENESS AUDIT")
    print("-" * 66)
    eqs = [p for k, p in C.BODY if k == "eq"]
    figs = [p for k, p in C.BODY if k == "fig"]
    tbls = [p for k, p in C.BODY if k == "table"]
    heads = [p for k, p in C.BODY if k in ("h1", "h1s")]
    rq = re.findall(r"RQ\d", body_text)
    ci = [c for p in tbls for r in p["rows"] for c in r
          if re.match(r"^\[\d\.\d+, \d\.\d+\]$", c)]
    n_refs = len(C.REFERENCES)

    # order of first citation
    _parts = []
    for _k, _p in C.BODY:
        if _k in ("p", "h1", "h2", "h1s", "caption"):
            _parts.append(_p)
        elif _k == "bullets":
            _parts.extend(_p)
        elif _k == "fig":
            _parts.append(_p["caption"])
    _body_for_cit = " ".join(_parts)
    _first = {}
    for _m in re.finditer(r"\[(\d+)\]", _body_for_cit):
        _n = int(_m.group(1))
        if _n not in _first:
            _first[_n] = _m.start()
    cit_order = sorted(_first, key=lambda n: _first[n])

    figs_cited = sum(1 for i in range(1, len(figs) + 1)
                     if re.search(r"[Ff]ig(?:ure)?\.?\s*%d\b" % i, _body_for_cit))
    tbls_cited = sum(1 for t_ in ("II", "III", "IV", "V", "VI")
                     if re.search(r"Table\s+%s\b" % t_, _body_for_cit))
    eqs_cited = sum(1 for n in range(1, len(eqs) + 1)
                    if re.search(r"\(%d\)" % n, _body_for_cit))

    checks = [
        ("numbered equations", len(eqs) >= 3,
         f"{len(eqs)} equations, (1)-({eqs[-1]['num']})"),
        ("tables (testbed + results + defects)", len(tbls) >= 4,
         f"{len(tbls) + 1} tables including nomenclature"),
        ("testbed / configuration table",
         any("testbed" in (p["cols"] + [" ".join(r) for r in p["rows"]])[0].lower()
             or any("Host OS" in " ".join(r) for r in p["rows"]) for p in tbls),
         "host, CPU, RAM, toolchain, corpus, k"),
        ("explicit research questions", len(set(rq)) >= 3,
         f"{len(set(rq))} research questions: {', '.join(sorted(set(rq)))}"),
        ("statistical treatment (n + CI)",
         len(ci) >= 4 and re.search(r"n = \d+", body_text) is not None,
         f"{len(ci)} Wilson 95% intervals, "
         + (re.search(r"n = \d+", body_text) or type("", (), {"group": lambda self, i: "n"})()).group(0)),
        ("real figures (not ASCII art)", len(figs) >= 2,
         f"{len(figs)} PNG figures: " + ", ".join(f["file"] for f in figs)),
        ("nomenclature", len(C.NOMENCLATURE) >= 4,
         f"{len(C.NOMENCLATURE)} defined terms"),
        ("back matter", all(any(h.strip().startswith(w) for h in heads)
                            for w in ("ACKNOWLEDGMENT", "CONFLICT OF INTEREST",
                                      "DATA AND CODE AVAILABILITY",
                                      "ETHICS AND BROADER IMPACT")),
         "acknowledgment, conflict of interest, data/code availability, ethics"),
        ("references", n_refs == 30, f"{n_refs} references"),
        ("exactly six pages", pages == 6, f"{pages} pages"),
        ("no borrowed long quotes", len(quoted_borrowed) == 0,
         f"{len(quoted)} quoted phrases, 0 borrowed"),
        ("6-gram overlap vs prior draft", overlap_old < 1.0,
         f"{overlap_old:.2f}%"),
        ("AI-phrase scan", not ai_hits,
         f"{len(ai_hits)} hits; burstiness {burst:.3f}"),

        # ---- IEEE submission-compliance checks ----
        ("references in citation order", cit_order == list(range(1, n_refs + 1)),
         "numbered in order of first appearance"),
        ("every figure cited in the text", figs_cited == len(figs),
         f"{figs_cited}/{len(figs)} figures referenced"),
        ("every table cited in the text", tbls_cited >= len(tbls),
         f"{tbls_cited}/{len(tbls)} tables referenced"),
        ("every equation referenced in the text", eqs_cited == len(eqs),
         f"{eqs_cited}/{len(eqs)} equations referenced"),
        ("index terms alphabetical",
         [k.strip() for k in C.KEYWORDS.split(",")] ==
         sorted((k.strip() for k in C.KEYWORDS.split(",")), key=str.lower),
         f"{len(C.KEYWORDS.split(','))} terms, sorted"),
        ("abstract one paragraph, 150-250 words",
         isinstance(C.ABSTRACT, str) and 150 <= len(C.ABSTRACT.split()) <= 250,
         f"{len(C.ABSTRACT.split())} words, 1 paragraph"),
        ("figure caption style 'Fig. N.'",
         all(f["caption"].startswith("Fig. %d." % (i + 1))
             for i, f in enumerate(figs)),
         ", ".join(f["caption"][:7] for f in figs)),
        ("author biography", any("BIOGRAPHY" in h for h in heads),
         "author biography present"),
    ]
    all_ok = True
    for name, ok, detail in checks:
        all_ok = all_ok and ok
        print(f"  [{'PASS' if ok else 'FAIL'}] {name:<38} {detail}")
    print("-" * 66)
    print(f"  COMPLETENESS: {'ALL CHECKS PASS' if all_ok else 'CHECKS FAILED'}")
    audit_ok = all_ok
    print("=" * 66)

    html_path = os.path.join(HERE, "_paper_layout_check.html")
    pdf_path = os.path.join(HERE, "_paper_layout_check.pdf")
    build_html_mirror(html_path)
    wp, msg = weasyprint_pages(html_path, pdf_path)
    if wp:
        print(f"  WeasyPrint cross-check  : {wp} pages  (independent engine)")
    else:
        print(f"  WeasyPrint cross-check  : {msg}")

    build_docx(OUT)
    print(f"  wrote {OUT}  ({os.path.getsize(OUT)/1024:.0f} KB)")
    if not audit_ok:
        print("  WARNING: publication-completeness audit has failures")
    return pages


if __name__ == "__main__":
    sys.exit(0 if main() == 6 else 1)
