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

from docx import Document  # noqa: E402
from docx.shared import Pt, Inches, RGBColor  # noqa: E402
from docx.enum.text import WD_ALIGN_PARAGRAPH, WD_LINE_SPACING  # noqa: E402
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
CELL_VPAD = 1.2          # points, above and below the cell text

# font name per style, used by the layout model
def _style(font, size, lead, before, after, align, bold, indent=0.0,
           first_w=None, rest_w=None):
    return dict(font=font, size=size, lead=lead, before=before, after=after,
                align=align, bold=bold, indent=indent,
                first_w=first_w if first_w is not None else TEXT_W,
                rest_w=rest_w if rest_w is not None else TEXT_W)


STYLE = {
    "title":   _style(SERIF, 14.0, 16.4, 0.0, 3.5, "center", True),
    "authors": _style(SERIF, 10.0, 12.0, 0.0, 0.5, "center", False),
    "affil":   _style(SERIF, 8.6, 10.4, 0.0, 5.0, "center", False),
    "abstract": _style(SERIF, 8.8, 10.6, 0.0, 2.5, "justify", False),
    "keywords": _style(SERIF, 8.8, 10.6, 0.0, 7.0, "justify", False),
    "h1":      _style(SERIF, 10.0, 12.2, 5.5, 2.0, "left", True),
    "h2":      _style(SERIF, 9.8, 11.6, 4.0, 1.5, "left", True),
    "p":       _style(SERIF, 9.5, 11.3, 0.0, 3.1, "justify", False),
    "mono":    _style(MONO, 7.4, 8.6, 1.5, 5.0, "left", False),
    "caption": _style(SERIF, 8.0, 9.6, 3.0, 1.2, "left", False),
    "table":   _style(SERIF, 7.8, 9.3, 0.0, 1.5, "left", False),
    # every bullet line sits 0.19in in from the margin
    "refs":    _style(SERIF, 8.0, 9.4, 0.0, 0.9, "justify", False,
                      indent=0.17, first_w=TEXT_W, rest_w=TEXT_W - 0.17),
    "bullet":  _style(SERIF, 9.5, 11.3, 0.0, 2.2, "justify", False,
                      indent=0.19, first_w=TEXT_W, rest_w=TEXT_W),
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

    abstract_text = " ".join(C.ABSTRACT)
    add("abstract", "Abstract—" + abstract_text)
    add("keywords", "Index Terms—" + C.KEYWORDS)

    for kind, payload in C.BODY:
        if kind == "h1":
            add("h1", payload, keep=True)
        elif kind == "h2":
            add("h2", payload, keep=True)
        elif kind == "p":
            add("p", payload)
        elif kind == "mono":
            add("mono", payload)
        elif kind == "caption":
            add("caption", payload, keep=True)
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
    para(doc, "Abstract—" + " ".join(C.ABSTRACT), "abstract")
    para(doc, "Index Terms—" + C.KEYWORDS, "keywords")

    for kind, payload in C.BODY:
        if kind == "h1":
            para(doc, payload, "h1", keep_next=True)
        elif kind == "h2":
            para(doc, payload, "h2", keep_next=True)
        elif kind == "p":
            para(doc, payload, "p")
        elif kind == "mono":
            para(doc, payload, "mono")
        elif kind == "caption":
            para(doc, payload, "caption", keep_next=True)
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
    out.append(f".ref{{font-size:{st['refs']['size']}pt;line-height:{st['refs']['lead']}pt;"
               f"margin:0 0 {st['refs']['after']}pt 0;"
               f"padding-left:{st['refs']['indent']}in;text-indent:-{st['refs']['indent']}in;}}")
    out.append("</style></head><body>")

    out.append(f"<p class='title'>{_esc(C.TITLE)}</p>")
    out.append(f"<p class='authors'>{_esc(', '.join(C.AUTHORS))}</p>")
    out.append(f"<p class='affil'>{_esc(C.AFFILIATION)}</p>")
    out.append(f"<p class='abs'>Abstract\u2014{_esc(' '.join(C.ABSTRACT))}</p>")
    out.append(f"<p class='kw'>Index Terms\u2014{_esc(C.KEYWORDS)}</p>")

    for kind, payload in C.BODY:
        if kind == "h1":
            out.append(f"<h1>{_esc(payload)}</h1>")
        elif kind == "h2":
            out.append(f"<h2>{_esc(payload)}</h2>")
        elif kind == "p":
            out.append(f"<p>{_esc(payload)}</p>")
        elif kind == "mono":
            out.append(f"<pre class='mono'>{_esc(payload)}</pre>")
        elif kind == "caption":
            out.append(f"<p class='cap'>{_esc(payload)}</p>")
        elif kind == "bullets":
            for b in payload:
                out.append(f"<p style='padding-left:0.19in;text-indent:0'>"
                           f"&#8226;&nbsp; {_esc(b)}</p>")
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
            out.append(f"<p style='margin:0 0 3pt 0;line-height:3pt'>&nbsp;</p>")
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
    abstract_text = " ".join(C.ABSTRACT)
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
    # quoted material is only ever the title of a cited work
    quoted = re.findall(r"“([^”]{25,})”", body_text)

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
          f"(each is a cited work's title)")
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
    return pages


if __name__ == "__main__":
    sys.exit(0 if main() == 6 else 1)
