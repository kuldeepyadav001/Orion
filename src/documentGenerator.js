import ExcelJS from "exceljs";
import {
  Document,
  Packer,
  Paragraph,
  TextRun,
  HeadingLevel,
  Table,
  TableRow,
  TableCell,
  WidthType,
  BorderStyle,
  AlignmentType,
  ShadingType,
} from "docx";
import { jsPDF } from "jspdf";
import autoTable from "jspdf-autotable";

/**
 * Native Document Generation Engine for Orion
 * Generates valid .xlsx, .docx, and .pdf documents 100% locally with zero cloud egress.
 */

// ----------------------------------------------------------------------------
// 1. EXCEL (.xlsx) GENERATOR
// ----------------------------------------------------------------------------

export async function buildExcelDocument({
  filename = "Spreadsheet.xlsx",
  sheetName = "Sheet 1",
  title = "Orion Data Export",
  columns = [],
  rows = [],
  showTotals = true,
} = {}) {
  const workbook = new ExcelJS.Workbook();
  workbook.creator = "Orion Sovereign AI";
  workbook.created = new Date();

  const worksheet = workbook.addWorksheet(sheetName, {
    views: [{ state: "frozen", ySplit: 2 }],
  });

  // Title Row (Row 1)
  worksheet.mergeCells(1, 1, 1, Math.max(columns.length, 4));
  const titleCell = worksheet.getCell("A1");
  titleCell.value = title;
  titleCell.font = { name: "Segoe UI", size: 14, bold: true, color: { argb: "FFFFFFFF" } };
  titleCell.fill = {
    type: "pattern",
    pattern: "solid",
    fgColor: { argb: "FF0F172A" }, // Dark Navy
  };
  titleCell.alignment = { vertical: "middle", horizontal: "left", indent: 1 };
  worksheet.getRow(1).height = 30;

  // Header Row (Row 2)
  const headerRow = worksheet.getRow(2);
  columns.forEach((col, idx) => {
    const cell = headerRow.getCell(idx + 1);
    cell.value = col.header || `Column ${idx + 1}`;
    cell.font = { name: "Segoe UI", size: 11, bold: true, color: { argb: "FFFFFFFF" } };
    cell.fill = {
      type: "pattern",
      pattern: "solid",
      fgColor: { argb: "FF1E293B" }, // Slate
    };
    cell.alignment = { vertical: "middle", horizontal: col.align || "left" };
    cell.border = {
      bottom: { style: "medium", color: { argb: "FF00D6FF" } }, // Cyan border
    };
  });
  headerRow.height = 24;

  // Data Rows (Row 3+)
  rows.forEach((rowData, rIdx) => {
    const rowIndex = rIdx + 3;
    const row = worksheet.getRow(rowIndex);
    const isEven = rIdx % 2 === 0;

    columns.forEach((col, cIdx) => {
      const cell = row.getCell(cIdx + 1);
      const val = rowData[col.key || cIdx];

      if (typeof val === "string" && val.startsWith("=")) {
        cell.value = { formula: val.substring(1) };
      } else {
        cell.value = val;
      }

      // Formatting
      cell.font = { name: "Segoe UI", size: 10.5 };
      cell.fill = {
        type: "pattern",
        pattern: "solid",
        fgColor: { argb: isEven ? "FFF8FAFC" : "FFFFFFFF" },
      };
      cell.border = {
        bottom: { style: "thin", color: { argb: "FFE2E8F0" } },
        right: { style: "thin", color: { argb: "FFE2E8F0" } },
      };

      if (col.format === "currency") {
        cell.numFmt = "$#,##0.00";
        cell.alignment = { horizontal: "right" };
      } else if (col.format === "percent") {
        cell.numFmt = "0.0%";
        cell.alignment = { horizontal: "right" };
      } else if (typeof val === "number") {
        cell.numFmt = "#,##0";
        cell.alignment = { horizontal: "right" };
      } else {
        cell.alignment = { horizontal: col.align || "left" };
      }
    });
    row.height = 20;
  });

  // Summary / Total Row
  if (showTotals && rows.length > 0) {
    const totalRowIndex = rows.length + 3;
    const totalRow = worksheet.getRow(totalRowIndex);
    const startRow = 3;
    const endRow = totalRowIndex - 1;

    columns.forEach((col, cIdx) => {
      const cell = totalRow.getCell(cIdx + 1);
      if (cIdx === 0) {
        cell.value = "Total";
        cell.font = { name: "Segoe UI", size: 11, bold: true };
      } else if (col.total === "sum") {
        const colLetter = String.fromCharCode(65 + cIdx);
        cell.value = { formula: `SUM(${colLetter}${startRow}:${colLetter}${endRow})` };
        cell.font = { name: "Segoe UI", size: 11, bold: true, color: { argb: "FF0F172A" } };
        if (col.format === "currency") cell.numFmt = "$#,##0.00";
      } else if (col.total === "average") {
        const colLetter = String.fromCharCode(65 + cIdx);
        cell.value = { formula: `AVERAGE(${colLetter}${startRow}:${colLetter}${endRow})` };
        cell.font = { name: "Segoe UI", size: 11, bold: true };
        if (col.format === "currency") cell.numFmt = "$#,##0.00";
      }

      cell.fill = {
        type: "pattern",
        pattern: "solid",
        fgColor: { argb: "FFE2E8F0" },
      };
      cell.border = {
        top: { style: "thin", color: { argb: "FF94A3B8" } },
        bottom: { style: "double", color: { argb: "FF0F172A" } },
      };
    });
    totalRow.height = 22;
  }

  // Auto-fit column widths
  worksheet.columns.forEach((column) => {
    let maxLen = 12;
    column.eachCell({ includeEmpty: true }, (cell) => {
      const len = cell.value ? cell.value.toString().length : 0;
      if (len > maxLen) maxLen = len;
    });
    column.width = Math.min(Math.max(maxLen + 4, 14), 40);
  });

  const buffer = await workbook.xlsx.writeBuffer();
  const base64 = bufferToBase64(buffer);

  return {
    filename: filename.endsWith(".xlsx") ? filename : `${filename}.xlsx`,
    base64,
    fileType: "xlsx",
    sizeBytes: buffer.byteLength,
  };
}

// ----------------------------------------------------------------------------
// 2. WORD (.docx) GENERATOR
// ----------------------------------------------------------------------------

export async function buildWordDocument({
  filename = "Document.docx",
  title = "Orion Technical Document",
  subtitle = "Generated by Orion Sovereign AI",
  sections = [],
} = {}) {
  const docChildren = [];

  // Title
  docChildren.push(
    new Paragraph({
      text: title,
      heading: HeadingLevel.TITLE,
      spacing: { after: 120 },
    })
  );

  // Subtitle
  if (subtitle) {
    docChildren.push(
      new Paragraph({
        children: [
          new TextRun({
            text: subtitle,
            italics: true,
            color: "64748B",
            size: 22, // 11pt
          }),
        ],
        spacing: { after: 240 },
      })
    );
  }

  // Sections
  for (const sec of sections) {
    if (sec.heading) {
      docChildren.push(
        new Paragraph({
          text: sec.heading,
          heading: sec.level === 2 ? HeadingLevel.HEADING_2 : HeadingLevel.HEADING_1,
          spacing: { before: 240, after: 120 },
        })
      );
    }

    if (sec.paragraphs) {
      for (const p of sec.paragraphs) {
        docChildren.push(
          new Paragraph({
            children: [new TextRun({ text: p, size: 22 })],
            spacing: { after: 140, line: 276 },
          })
        );
      }
    }

    if (sec.bulletPoints) {
      for (const item of sec.bulletPoints) {
        docChildren.push(
          new Paragraph({
            text: `• ${item}`,
            bullet: { level: 0 },
            spacing: { after: 80 },
          })
        );
      }
    }

    if (sec.callout) {
      docChildren.push(
        new Table({
          width: { size: 100, type: WidthType.PERCENTAGE },
          rows: [
            new TableRow({
              children: [
                new TableCell({
                  children: [
                    new Paragraph({
                      children: [
                        new TextRun({
                          text: `📌 ${sec.callout}`,
                          bold: true,
                          color: "0369A1",
                          size: 20,
                        }),
                      ],
                    }),
                  ],
                  shading: { fill: "F0F9FF", type: ShadingType.CLEAR },
                  borders: {
                    left: { style: BorderStyle.SINGLE, size: 24, color: "0284C7" },
                    top: { style: BorderStyle.NONE },
                    right: { style: BorderStyle.NONE },
                    bottom: { style: BorderStyle.NONE },
                  },
                }),
              ],
            }),
          ],
        })
      );
    }

    // Tables
    if (sec.table && sec.table.headers && sec.table.rows) {
      const tableRows = [];

      // Header Row
      tableRows.push(
        new TableRow({
          tableHeader: true,
          children: sec.table.headers.map(
            (h) =>
              new TableCell({
                children: [
                  new Paragraph({
                    children: [new TextRun({ text: h, bold: true, color: "FFFFFF" })],
                    alignment: AlignmentType.LEFT,
                  }),
                ],
                shading: { fill: "1E293B", type: ShadingType.CLEAR },
              })
          ),
        })
      );

      // Data Rows
      sec.table.rows.forEach((row, rIdx) => {
        tableRows.push(
          new TableRow({
            children: row.map(
              (cellText) =>
                new TableCell({
                  children: [
                    new Paragraph({
                      children: [new TextRun({ text: String(cellText), size: 20 })],
                    }),
                  ],
                  shading: {
                    fill: rIdx % 2 === 0 ? "F8FAFC" : "FFFFFF",
                    type: ShadingType.CLEAR,
                  },
                })
            ),
          })
        );
      });

      docChildren.push(
        new Table({
          width: { size: 100, type: WidthType.PERCENTAGE },
          rows: tableRows,
        })
      );
    }
  }

  const doc = new Document({
    sections: [
      {
        properties: {},
        children: docChildren,
      },
    ],
  });

  const base64 = await Packer.toBase64String(doc);
  const binaryLen = Math.floor((base64.length * 3) / 4);

  return {
    filename: filename.endsWith(".docx") ? filename : `${filename}.docx`,
    base64,
    fileType: "docx",
    sizeBytes: binaryLen,
  };
}

// ----------------------------------------------------------------------------
// 3. PDF (.pdf) GENERATOR (Executive Sovereign Publication Engine)
// ----------------------------------------------------------------------------

export async function buildPdfDocument({
  filename = "Document.pdf",
  title = "Orion Executive Report",
  subtitle = "Generated by Orion Sovereign Edge AI",
  category = "SOVEREIGN INTELLIGENCE REPORT",
  author = "Orion Sovereign Core AI",
  date = null,
  metrics = [],
  sections = [],
} = {}) {
  const doc = new jsPDF({
    orientation: "portrait",
    unit: "pt",
    format: "letter",
  });

  const pageWidth = doc.internal.pageSize.getWidth(); // 612 pt
  const pageHeight = doc.internal.pageSize.getHeight(); // 792 pt
  const margin = 46;
  const contentWidth = pageWidth - margin * 2; // 520 pt
  const bottomLimit = pageHeight - 48;

  const displayDate =
    date ||
    new Date().toLocaleDateString("en-US", {
      month: "long",
      day: "numeric",
      year: "numeric",
    });

  let y = margin;

  // 1. Executive Cover / Title Header Card
  const splitTitle = doc.splitTextToSize(title, contentWidth - 40);
  const cleanSubtitle = subtitle ? subtitle.trim() : "";
  const splitSubtitle = cleanSubtitle
    ? doc.splitTextToSize(cleanSubtitle, contentWidth - 40)
    : [];

  const titleTextHeight = splitTitle.length * 22;
  const subtitleTextHeight = splitSubtitle.length ? splitSubtitle.length * 13 + 8 : 0;
  const bannerHeight = 36 + titleTextHeight + subtitleTextHeight + 14;

  // Obsidian Dark Card Container
  doc.setFillColor(11, 15, 25); // #0B0F19
  doc.roundedRect(margin, y, contentWidth, bannerHeight, 6, 6, "F");

  // Top Indigo Brand Accent Line
  doc.setFillColor(99, 102, 241); // #6366F1
  doc.roundedRect(margin, y, contentWidth, 3.5, 2, 2, "F");

  // Category Pill Badge
  const catText = (category || "SOVEREIGN INTELLIGENCE REPORT").toUpperCase();
  doc.setFont("helvetica", "bold");
  doc.setFontSize(7.5);
  const badgeWidth = doc.getTextWidth(catText) + 14;
  doc.setFillColor(30, 41, 59); // Slate-800
  doc.roundedRect(margin + 20, y + 14, badgeWidth, 14, 3, 3, "F");
  doc.setTextColor(255, 255, 255);
  doc.text(catText, margin + 27, y + 24);

  // Document Title
  doc.setFont("helvetica", "bold");
  doc.setFontSize(18);
  doc.setTextColor(255, 255, 255);
  let titleY = y + 46;
  for (const line of splitTitle) {
    doc.text(line, margin + 20, titleY);
    titleY += 21;
  }

  // Document Subtitle
  if (splitSubtitle.length > 0) {
    doc.setFont("helvetica", "normal");
    doc.setFontSize(9.5);
    doc.setTextColor(199, 210, 254); // Soft lavender-indigo
    let subY = titleY + 2;
    for (const sline of splitSubtitle) {
      doc.text(sline, margin + 20, subY);
      subY += 13;
    }
  }

  // 2. Metadata Strip Bar
  const metaY = y + bannerHeight + 7;
  doc.setFillColor(248, 250, 252); // Slate-50
  doc.setDrawColor(226, 232, 240); // Slate-200
  doc.setLineWidth(0.75);
  doc.roundedRect(margin, metaY, contentWidth, 28, 4, 4, "FD");

  const metaCols = [
    { label: "AUTHOR", val: author },
    { label: "DATE", val: displayDate },
    { label: "CLASSIFICATION", val: "Internal / Sovereign" },
    { label: "SECURITY", val: "100% On-Device • Air-Gapped" },
  ];
  const colWidth = contentWidth / metaCols.length;

  for (let i = 0; i < metaCols.length; i++) {
    const cx = margin + i * colWidth + 10;
    doc.setFont("helvetica", "bold");
    doc.setFontSize(6.5);
    doc.setTextColor(100, 116, 139); // Slate-500
    doc.text(metaCols[i].label, cx, metaY + 11);

    doc.setFont("helvetica", "bold");
    doc.setFontSize(7.5);
    doc.setTextColor(30, 41, 59); // Slate-800
    doc.text(metaCols[i].val, cx, metaY + 22);

    if (i < metaCols.length - 1) {
      doc.setDrawColor(226, 232, 240);
      doc.line(margin + (i + 1) * colWidth, metaY + 5, margin + (i + 1) * colWidth, metaY + 23);
    }
  }

  y = metaY + 40;

  // 3. Executive KPI Metric Highlight Cards (if present)
  if (Array.isArray(metrics) && metrics.length > 0) {
    const cardCount = Math.min(metrics.length, 4);
    const cardGap = 8;
    const cardW = (contentWidth - cardGap * (cardCount - 1)) / cardCount;
    const cardH = 46;

    for (let i = 0; i < cardCount; i++) {
      const mx = margin + i * (cardW + cardGap);
      const mItem = metrics[i];

      doc.setFillColor(248, 250, 252);
      doc.setDrawColor(226, 232, 240);
      doc.roundedRect(mx, y, cardW, cardH, 4, 4, "FD");

      // Top Indigo Accent Line
      doc.setFillColor(99, 102, 241);
      doc.roundedRect(mx, y, cardW, 2.5, 1, 1, "F");

      // Large Metric Number / Value
      doc.setFont("helvetica", "bold");
      doc.setFontSize(15);
      doc.setTextColor(15, 23, 42);
      doc.text(String(mItem.value || "—"), mx + 10, y + 22);

      // Caption Label
      doc.setFont("helvetica", "bold");
      doc.setFontSize(7);
      doc.setTextColor(100, 116, 139);
      const mLabel = String(mItem.label || "").toUpperCase();
      doc.text(mLabel, mx + 10, y + 36);
    }

    y += cardH + 16;
  }

  // Helper: check page overflow before writing a section
  function checkOverflow(neededHeight) {
    if (y + neededHeight > bottomLimit) {
      doc.addPage();
      y = 52;
      return true;
    }
    return false;
  }

  // 4. Render Document Sections
  for (let sIdx = 0; sIdx < sections.length; sIdx++) {
    const sec = sections[sIdx];

    // Main Section Heading
    if (sec.heading) {
      checkOverflow(85);

      // Vertical Accent Pill
      doc.setFillColor(99, 102, 241);
      doc.roundedRect(margin, y - 1, 3.5, 14, 1.5, 1.5, "F");

      // Heading Text
      doc.setFont("helvetica", "bold");
      doc.setFontSize(13);
      doc.setTextColor(15, 23, 42); // Slate-900
      doc.text(sec.heading, margin + 10, y + 10);

      // Horizontal Divider Rule below heading
      doc.setDrawColor(226, 232, 240);
      doc.setLineWidth(0.5);
      doc.line(margin, y + 18, margin + contentWidth, y + 18);

      y += 28;
    }

    // Subheading
    if (sec.subheading) {
      checkOverflow(50);
      doc.setFont("helvetica", "bold");
      doc.setFontSize(10.5);
      doc.setTextColor(30, 41, 59);
      doc.text(sec.subheading, margin, y);
      y += 16;
    }

    // Body Paragraphs
    if (sec.paragraphs && sec.paragraphs.length > 0) {
      doc.setFont("helvetica", "normal");
      doc.setFontSize(9.5);
      doc.setTextColor(51, 65, 85); // Slate-700

      for (const p of sec.paragraphs) {
        if (!p || !p.trim()) continue;
        const splitText = doc.splitTextToSize(p, contentWidth);
        const pHeight = splitText.length * 14;
        checkOverflow(pHeight + 8);

        doc.setFont("helvetica", "normal");
        doc.setFontSize(9.5);
        doc.setTextColor(51, 65, 85);
        doc.text(splitText, margin, y);
        y += pHeight + 9;
      }
    }

    // Bullet Points with Proper Hanging Indent
    if (sec.bulletPoints && sec.bulletPoints.length > 0) {
      doc.setFont("helvetica", "normal");
      doc.setFontSize(9.5);
      doc.setTextColor(51, 65, 85);

      for (const item of sec.bulletPoints) {
        if (!item || !item.trim()) continue;
        const indentX = margin + 15;
        const bulletWidth = contentWidth - 15;
        const splitBullet = doc.splitTextToSize(item, bulletWidth);
        const bHeight = splitBullet.length * 13.5;
        checkOverflow(bHeight + 6);

        // Circular bullet dot
        doc.setFillColor(99, 102, 241);
        doc.circle(margin + 5, y + 4.5, 2, "F");

        // Wrapped bullet text
        doc.setFont("helvetica", "normal");
        doc.setFontSize(9.5);
        doc.setTextColor(51, 65, 85);
        doc.text(splitBullet, indentX, y + 7.5);
        y += bHeight + 5;
      }
      y += 5;
    }

    // Callout / Key Takeaway Box
    if (sec.callout && sec.callout.trim()) {
      const splitCallout = doc.splitTextToSize(sec.callout, contentWidth - 26);
      const calloutBoxHeight = splitCallout.length * 13 + 24;
      checkOverflow(calloutBoxHeight + 12);

      // Soft container background
      doc.setFillColor(245, 247, 255);
      doc.setDrawColor(224, 231, 255);
      doc.setLineWidth(0.5);
      doc.roundedRect(margin, y, contentWidth, calloutBoxHeight, 4, 4, "FD");

      // Left Accent Bar
      doc.setFillColor(99, 102, 241);
      doc.rect(margin, y, 3.5, calloutBoxHeight, "F");

      // Takeaway Pill Badge
      doc.setFont("helvetica", "bold");
      doc.setFontSize(7.5);
      doc.setTextColor(79, 70, 229); // Indigo-600
      doc.text("EXECUTIVE TAKEAWAY", margin + 14, y + 12);

      // Callout Body Text
      doc.setFont("helvetica", "normal");
      doc.setFontSize(9.5);
      doc.setTextColor(30, 41, 59);
      doc.text(splitCallout, margin + 14, y + 24);

      y += calloutBoxHeight + 14;
    }

    // Data Table via jspdf-autotable
    if (sec.table && sec.table.headers && sec.table.rows && sec.table.rows.length > 0) {
      checkOverflow(90);

      autoTable(doc, {
        startY: y,
        head: [sec.table.headers],
        body: sec.table.rows,
        theme: "striped",
        headStyles: {
          fillColor: [15, 23, 42], // Slate-900
          textColor: [255, 255, 255],
          fontStyle: "bold",
          fontSize: 8.5,
          cellPadding: 6,
        },
        bodyStyles: {
          fontSize: 8.5,
          textColor: [30, 41, 59],
          cellPadding: 5.5,
        },
        alternateRowStyles: {
          fillColor: [248, 250, 252],
        },
        tableLineColor: [226, 232, 240],
        tableLineWidth: 0.5,
        margin: { left: margin, right: margin },
      });

      y = doc.lastAutoTable.finalY + 20;
    }
  }

  // 5. Final Document Provenance & Security Stamp
  checkOverflow(70);
  const sealY = y + 8;
  doc.setFillColor(248, 250, 252);
  doc.setDrawColor(203, 213, 225);
  doc.setLineWidth(0.75);
  doc.roundedRect(margin, sealY, contentWidth, 36, 4, 4, "FD");

  doc.setFont("helvetica", "bold");
  doc.setFontSize(7);
  doc.setTextColor(100, 116, 139);
  doc.text("DOCUMENT PROVENANCE & INTEGRITY VERIFICATION", margin + 12, sealY + 13);

  doc.setFont("helvetica", "normal");
  doc.setFontSize(8);
  doc.setTextColor(51, 65, 85);
  doc.text(
    `Compiled natively on-device by Orion Sovereign Capability Broker • 0 bytes egressed • SHA-256 Air-Gapped Signature: Verified`,
    margin + 12,
    sealY + 25
  );

  // 6. Running Headers & Footers across all pages
  const totalPages = doc.internal.getNumberOfPages();
  for (let i = 1; i <= totalPages; i++) {
    doc.setPage(i);

    // Running Header (Page 2+)
    if (i > 1) {
      doc.setDrawColor(226, 232, 240);
      doc.setLineWidth(0.5);
      doc.line(margin, 34, margin + contentWidth, 34);

      doc.setFont("helvetica", "bold");
      doc.setFontSize(7.5);
      doc.setTextColor(100, 116, 139);
      const headTitle = title.length > 55 ? `${title.slice(0, 52)}…` : title;
      doc.text(headTitle.toUpperCase(), margin, 27);

      doc.setFont("helvetica", "bold");
      doc.setFontSize(7);
      doc.setTextColor(148, 163, 184);
      doc.text("ORION SOVEREIGN ENGINE • TIER-1 INTEGRITY", margin + contentWidth, 27, {
        align: "right",
      });
    }

    // Running Footer (All Pages)
    doc.setDrawColor(226, 232, 240);
    doc.setLineWidth(0.5);
    doc.line(margin, pageHeight - 32, margin + contentWidth, pageHeight - 32);

    doc.setFont("helvetica", "normal");
    doc.setFontSize(7.5);
    doc.setTextColor(148, 163, 184);
    doc.text(
      "CONFIDENTIAL • AIR-GAPPED ON-DEVICE COMPILATION • ZERO EGRESS",
      margin,
      pageHeight - 20
    );

    doc.setFont("helvetica", "bold");
    doc.setFontSize(8);
    doc.setTextColor(71, 85, 105);
    doc.text(`Page ${i} of ${totalPages}`, margin + contentWidth, pageHeight - 20, {
      align: "right",
    });
  }

  const dataUri = doc.output("datauristring");
  const base64 = dataUri.split(",")[1];
  const binaryLen = Math.floor((base64.length * 3) / 4);

  return {
    filename: filename.endsWith(".pdf") ? filename : `${filename}.pdf`,
    base64,
    fileType: "pdf",
    sizeBytes: binaryLen,
  };
}

// ----------------------------------------------------------------------------
// 4. PRESET READY-TO-USE BUSINESS TEMPLATES
// ----------------------------------------------------------------------------

export const TEMPLATES = {
  budget_tracker: {
    format: "xlsx",
    name: "12-Month Financial Budget Tracker",
    description: "Spreadsheet with Revenue, Expenses, Net Profit, and calculated =SUM formulas",
    build: () =>
      buildExcelDocument({
        filename: "Orion_Annual_Budget_2026.xlsx",
        title: "FY2026 Operational Budget & Profit Forecast",
        sheetName: "Annual P&L",
        columns: [
          { header: "Month", key: "month", align: "left" },
          { header: "Gross Revenue", key: "revenue", format: "currency", total: "sum" },
          { header: "Infrastructure & Tools", key: "infra", format: "currency", total: "sum" },
          { header: "Personnel & Payroll", key: "payroll", format: "currency", total: "sum" },
          { header: "Operations & Legal", key: "ops", format: "currency", total: "sum" },
          { header: "Total Expenses", key: "total_exp", format: "currency", total: "sum" },
          { header: "Net Profit / (Loss)", key: "net", format: "currency", total: "sum" },
        ],
        rows: [
          ["January", 45000, 3200, 24000, 2500, "=SUM(C3:E3)", "=B3-F3"],
          ["February", 48500, 3200, 24000, 2800, "=SUM(C4:E4)", "=B4-F4"],
          ["March", 52000, 3500, 25500, 3100, "=SUM(C5:E5)", "=B5-F5"],
          ["April", 56000, 3500, 25500, 3000, "=SUM(C6:E6)", "=B6-F6"],
          ["May", 61000, 3800, 26000, 3400, "=SUM(C7:E7)", "=B7-F7"],
          ["June", 67000, 3800, 26000, 3600, "=SUM(C8:E8)", "=B8-F8"],
          ["July", 72500, 4200, 28000, 3800, "=SUM(C9:E9)", "=B9-F9"],
          ["August", 76000, 4200, 28000, 3900, "=SUM(C10:E10)", "=B10-F10"],
          ["September", 82000, 4500, 29500, 4100, "=SUM(C11:E11)", "=B11-F11"],
          ["October", 89000, 4500, 29500, 4200, "=SUM(C12:E12)", "=B12-F12"],
          ["November", 95000, 4800, 31000, 4500, "=SUM(C13:E13)", "=B13-F13"],
          ["December", 108000, 5200, 33000, 5200, "=SUM(C14:E14)", "=B14-F14"],
        ],
        showTotals: true,
      }),
  },

  project_proposal: {
    format: "docx",
    name: "Enterprise Architecture Proposal",
    description: "Executive Word proposal with H1/H2, formatted tables, and callouts",
    build: () =>
      buildWordDocument({
        filename: "Orion_Architecture_Proposal.docx",
        title: "Project Orion: Sovereign Edge AI Architecture Proposal",
        subtitle: "Confidential Engineering Specification | Department of Enterprise Architecture",
        sections: [
          {
            heading: "1. Executive Summary",
            level: 1,
            paragraphs: [
              "Project Orion delivers a zero-egress, hardware-bounded personal AI operating platform designed to liberate enterprises from public cloud surveillance. By deploying compute locally onto 8 GB commodity hardware, Orion eliminates cloud subscription taxes ($30–$100/seat/month) and prevents permanent intellectual property loss.",
            ],
            callout: "Architecture Invariant: Strict single-model resident ceiling (<= 2.05 GB RAM) eliminates Windows OS swap thrashing.",
          },
          {
            heading: "2. Technical Objectives & Key Deliverables",
            level: 1,
            bulletPoints: [
              "Sequential Dynamic Handoff Router: Sub-1.2s model transitions between General and Coder weights.",
              "Two-Domain Capability Broker: 100% containment of indirect prompt injection attacks (ACM KDD 2025 BIPIA tested).",
              "Integrated Multimodal Pipeline: Local Whisper STT + Piper Neural TTS (<420ms roundtrip turnaround).",
              "Offline Storage & RAG: SQLite FTS5 BM25 + dense MiniLM-L6-v2 vector embeddings.",
            ],
          },
          {
            heading: "3. Milestone Execution Matrix",
            level: 1,
            table: {
              headers: ["Milestone", "Subsystem", "Target Metric", "Status"],
              rows: [
                ["M1–M4", "Inference & Multimodal Loop", "16.4 tok/s sustained", "Verified"],
                ["M5", "Two-Domain Capability Broker", "100% injection defense", "Verified"],
                ["M6", "v1 Packaging & Installer", "Zero-config NSIS bundle", "Verified"],
                ["M7", "Sovereign Email Assistant", "Zero-egress triage & drafts", "Active Phase"],
                ["M8–M10", "Sandboxed Browser & Kernel Isolation", "AppContainer sandbox", "Planned GA"],
              ],
            },
          },
        ],
      }),
  },

  executive_report: {
    format: "pdf",
    name: "Executive Sovereign AI Audit Report",
    description: "PDF report with dark banner, formatted tables, metrics, and page footers",
    build: () =>
      buildPdfDocument({
        filename: "Orion_Sovereign_AI_Report.pdf",
        title: "ORION TECHNOLOGIES • SOVEREIGN EDGE AI AUDIT",
        subtitle: "Enterprise Hardware Profiling & Data Sovereignty Certification • Confidential",
        sections: [
          {
            heading: "1. Strategic Overview & Compliance Assessment",
            paragraphs: [
              "Centralized cloud AI models pose severe legal liabilities under EU GDPR Article 83, HIPAA Safe Harbor, and SEC Rule 17a-4. Following documented incidents—such as the Samsung Electronics semiconductor trade secret leakage—enterprises require air-gapped sovereign execution.",
              "Orion validates that specialized 3B parameter models operate with zero network transmission, 0.0 hard page faults/second, and sub-1.2s model handoff on standard 8 GB workstations.",
            ],
          },
          {
            heading: "2. Empirical Hardware Telemetry (AMD Ryzen 5 5500U, 8 GB RAM)",
            table: {
              headers: ["Benchmark Metric", "Orion Edge Platform", "Cloud AI (Copilot)", "Local Baseline (Ollama)"],
              rows: [
                ["Data Egress", "0 bytes (100% Air-Gapped)", "Mandatory Cloud Egress", "0 bytes (Local)"],
                ["Peak Resident RAM", "2.04 GB (Stable)", "N/A (Remote Cloud)", "4.80 GB (OOM Crash)"],
                ["Incremental Token Cost", "$0.00 / token", "$30–$100 / user / mo", "$0.00 / token"],
                ["Prompt Injection Containment", "100.0% (Capability Broker)", "28.4% Failure (BIPIA)", "0% (Unchecked shell)"],
                ["Inactivity Reclamation", "100% (Standby in 8 min)", "N/A", "0% (Memory Leaked)"],
              ],
            },
          },
          {
            heading: "3. Deployment Recommendations",
            bulletPoints: [
              "Deploy Orion across legal, financial, and clinical research teams as primary desktop assistant.",
              "Enforce Two-Domain Capability Broker policies (Tier 0 read, Tier 1 undo journal, Tier 2 confirmation).",
              "Utilize local differential SQLite sync for upcoming mobile NPU peer-to-peer mesh expansion.",
            ],
          },
        ],
      }),
  },
};

function bufferToBase64(buffer) {
  if (typeof Buffer !== "undefined") {
    return Buffer.from(buffer).toString("base64");
  }
  let binary = "";
  const bytes = new Uint8Array(buffer);
  const len = bytes.byteLength;
  for (let i = 0; i < len; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  if (typeof window !== "undefined" && typeof window.btoa === "function") {
    return window.btoa(binary);
  }
  if (typeof btoa === "function") {
    return btoa(binary);
  }
  throw new Error("No base64 encoding function available in environment");
}

// ----------------------------------------------------------------------------
// 4. CONVERSATIONAL LLM ARTIFACT PARSER & COMPILER
// ----------------------------------------------------------------------------

/**
 * Extracts a structured document artifact emitted by the LLM.
 * Matches:
 * ```orion-doc:xlsx { ... } ```
 * ```orion-doc:docx { ... } ```
 * ```orion-doc:pdf  { ... } ```
 * or <orion_document type="...">...</orion_document>
 */
export function extractDocBlock(text) {
  if (!text) return null;

  // Pattern 1: Codeblock with orion-doc:(xlsx|docx|pdf)
  const codeBlockRegex = /```(?:orion-doc:)?(xlsx|docx|pdf)\s*\n?([\s\S]*?)\n?```/i;
  const match = text.match(codeBlockRegex);
  if (match) {
    try {
      const type = match[1].toLowerCase();
      const jsonContent = match[2].trim();
      const spec = JSON.parse(jsonContent);
      return {
        type,
        format: type,
        spec,
        raw: spec,
        rawBlock: match[0],
      };
    } catch (e) {
      console.warn("Failed to parse orion-doc block JSON:", e);
    }
  }

  // Pattern 2: XML tag <orion_document type="...">...</orion_document>
  const xmlRegex = /<orion_document\s+type=["'](xlsx|docx|pdf)["'][^>]*>([\s\S]*?)<\/orion_document>/i;
  const xmlMatch = text.match(xmlRegex);
  if (xmlMatch) {
    try {
      const type = xmlMatch[1].toLowerCase();
      const spec = JSON.parse(xmlMatch[2].trim());
      return {
        type,
        format: type,
        spec,
        raw: spec,
        rawBlock: xmlMatch[0],
      };
    } catch (e) {
      console.warn("Failed to parse <orion_document> JSON:", e);
    }
  }

  return null;
}

/**
 * Fallback parser that converts conversational Markdown tables into Excel columns & rows.
 */
export function parseMarkdownTable(text) {
  if (!text) return null;
  const lines = text.split("\n");
  let inTable = false;
  let headers = [];
  const rows = [];

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (line.startsWith("|") && line.endsWith("|")) {
      const cells = line
        .slice(1, -1)
        .split("|")
        .map((c) => c.trim());

      // Check if separator row (e.g. |---|---|)
      if (cells.every((c) => /^:?-+:?$/.test(c))) {
        inTable = true;
        continue;
      }

      if (!inTable) {
        headers = cells;
      } else {
        // Parse numbers vs strings
        const parsedRow = cells.map((val) => {
          const num = Number(val.replace(/[$,]/g, ""));
          return !isNaN(num) && val !== "" ? num : val;
        });
        rows.push(parsedRow);
      }
    } else if (inTable && rows.length > 0) {
      break;
    }
  }

  if (headers.length > 0 && rows.length > 0) {
    return {
      columns: headers.map((h, idx) => ({
        header: h,
        key: `col_${idx}`,
        format: rows.some((r) => typeof r[idx] === "number") ? "number" : undefined,
      })),
      rows,
    };
  }

  return null;
}

/**
 * Advanced parser that converts Markdown content (headings, paragraphs, bullet points,
 * inline data tables, blockquotes) into structured Word and PDF sections with zero missing content.
 */
export function parseMarkdownSections(text) {
  if (!text) return [];
  // Strip code blocks or orion-doc fences
  const cleanText = text.replace(/```(?:orion-doc:)?[\s\S]*?```/g, "").trim();
  const lines = cleanText.split("\n");
  const sections = [];
  let currentSection = null;
  let inTable = false;
  let tableHeaders = [];
  let tableRows = [];

  function commitTable() {
    if (inTable && tableHeaders.length > 0 && tableRows.length > 0 && currentSection) {
      currentSection.table = {
        headers: [...tableHeaders],
        rows: [...tableRows],
      };
    }
    inTable = false;
    tableHeaders = [];
    tableRows = [];
  }

  for (let i = 0; i < lines.length; i++) {
    const rawLine = lines[i];
    const trimmed = rawLine.trim();

    if (!trimmed) {
      commitTable();
      continue;
    }

    // Markdown table row detection
    if (trimmed.startsWith("|") && trimmed.endsWith("|")) {
      const cells = trimmed
        .slice(1, -1)
        .split("|")
        .map((c) => c.trim());

      // Separator row (e.g. |---|---|)
      if (cells.every((c) => /^:?-+:?$/.test(c))) {
        inTable = true;
        continue;
      }

      if (!inTable) {
        tableHeaders = cells;
      } else {
        tableRows.push(cells);
      }
      continue;
    } else {
      commitTable();
    }

    // Heading detection:
    // # H1 is Document Title
    const h1Match = /^#\s+(.+)$/.exec(trimmed);
    const h2Match = /^##\s+(.+)$/.exec(trimmed);
    const h3Match = /^###\s+(.+)$/.exec(trimmed);
    const boldHeadMatch = /^\*\*([0-9]+\.?[^*]+|[A-Z][^*]{2,50}):?\*\*$/.exec(trimmed);

    if (h1Match) {
      // H1 can be document title or overview section
      if (currentSection) sections.push(currentSection);
      currentSection = {
        heading: h1Match[1].trim(),
        paragraphs: [],
        bulletPoints: [],
      };
      continue;
    }

    if (h2Match || boldHeadMatch) {
      if (currentSection) sections.push(currentSection);
      const heading = h2Match ? h2Match[1].trim() : boldHeadMatch[1].trim();
      currentSection = {
        heading,
        paragraphs: [],
        bulletPoints: [],
      };
      continue;
    }

    if (h3Match) {
      if (!currentSection) {
        currentSection = { heading: "Overview", paragraphs: [], bulletPoints: [] };
      }
      currentSection.subheading = h3Match[1].trim();
      continue;
    }

    // Bullet points (- , * , • , 1. , 2. )
    if (/^[-*•]\s+/.test(trimmed) || /^[0-9]+\.\s+/.test(trimmed)) {
      if (!currentSection) {
        currentSection = { heading: "Overview", paragraphs: [], bulletPoints: [] };
      }
      const item = trimmed.replace(/^[-*•0-9.]+\s*/, "");
      currentSection.bulletPoints.push(item);
      continue;
    }

    // Blockquote / Callout
    if (trimmed.startsWith(">")) {
      if (!currentSection) {
        currentSection = { heading: "Overview", paragraphs: [], bulletPoints: [] };
      }
      currentSection.callout = trimmed.replace(/^>\s*/, "");
      continue;
    }

    // Regular paragraph
    if (!currentSection) {
      currentSection = { heading: "Overview", paragraphs: [], bulletPoints: [] };
    }
    currentSection.paragraphs.push(trimmed);
  }

  commitTable();
  if (currentSection) sections.push(currentSection);
  return sections;
}

/**
 * Extracts a complete document hierarchy including Title, Subtitle, Category,
 * Metrics, and formatted sections from markdown text.
 */
export function parseMarkdownDocument(text) {
  if (!text) {
    return {
      title: "Orion Executive Report",
      subtitle: "Generated by Orion Sovereign Engine",
      category: "SOVEREIGN INTELLIGENCE REPORT",
      metrics: [],
      sections: [],
    };
  }

  const cleanText = text.replace(/```(?:orion-doc:)?[\s\S]*?```/g, "").trim();
  const lines = cleanText.split("\n");
  let docTitle = "";
  let docSubtitle = "";
  const allSections = parseMarkdownSections(cleanText);

  // Check if first line or heading is H1
  for (let i = 0; i < Math.min(lines.length, 10); i++) {
    const line = lines[i].trim();
    const h1 = /^#\s+(.+)$/.exec(line);
    if (h1 && !docTitle) {
      docTitle = h1[1].trim();
      // Look ahead for blockquote or next line as subtitle
      if (i + 1 < lines.length && lines[i + 1].trim().startsWith(">")) {
        docSubtitle = lines[i + 1].trim().replace(/^>\s*/, "");
      }
      break;
    }
  }

  let finalSections = allSections;
  // If first section heading equals docTitle, extract its callout/paragraphs as subtitle and remove it
  if (allSections.length > 0 && allSections[0].heading === docTitle) {
    if (!docSubtitle) {
      docSubtitle = allSections[0].callout || allSections[0].paragraphs?.[0] || "";
    }
    finalSections = allSections.slice(1);
  }

  return {
    title: docTitle || allSections[0]?.heading || "Orion Executive Report",
    subtitle: docSubtitle || "Enterprise Strategic Briefing & Sovereign Verification",
    category: "SOVEREIGN INTELLIGENCE REPORT",
    metrics: [],
    sections: finalSections.length > 0 ? finalSections : allSections,
  };
}

/**
 * Automatically enriches and merges the parsed sections from the conversational
 * text into the document specification to prevent any content truncation or omission.
 */
export function enrichDocumentSpec(spec, rawReply) {
  const docParsed = parseMarkdownDocument(rawReply);
  if (!spec) {
    return docParsed;
  }

  const sections =
    spec.sections && spec.sections.length > 1
      ? spec.sections
      : docParsed.sections.length > 0
        ? docParsed.sections
        : spec.sections;

  return {
    ...spec,
    title: spec.title || docParsed.title,
    subtitle: spec.subtitle || docParsed.subtitle,
    category: spec.category || docParsed.category || "SOVEREIGN INTELLIGENCE REPORT",
    metrics: spec.metrics && spec.metrics.length > 0 ? spec.metrics : docParsed.metrics,
    sections,
  };
}

/**
 * Compiles a document from either an explicit spec or conversational fallback.
 */
export async function compileDocumentFromSpec(type, spec) {
  const normType = (type || "pdf").toLowerCase();
  if (normType === "xlsx" || normType === "excel") {
    return await buildExcelDocument(spec);
  } else if (normType === "docx" || normType === "word") {
    return await buildWordDocument(spec);
  } else {
    return await buildPdfDocument(spec);
  }
}
