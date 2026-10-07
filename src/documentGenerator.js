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
}) {
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
}) {
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
// 3. PDF (.pdf) GENERATOR
// ----------------------------------------------------------------------------

export async function buildPdfDocument({
  filename = "Document.pdf",
  title = "Orion Report",
  subtitle = "Generated by Orion Sovereign Edge AI",
  sections = [],
}) {
  const doc = new jsPDF({
    orientation: "portrait",
    unit: "pt",
    format: "letter",
  });

  const pageWidth = doc.internal.pageSize.getWidth();
  const pageHeight = doc.internal.pageSize.getHeight();
  const margin = 40;
  let y = margin;

  // Title Banner
  doc.setFillColor(15, 23, 42); // #0F172A
  doc.rect(0, 0, pageWidth, 55, "F");

  // Cyan Accent Line
  doc.setFillColor(0, 214, 255); // Cyan
  doc.rect(0, 55, pageWidth, 3, "F");

  doc.setTextColor(255, 255, 255);
  doc.setFont("helvetica", "bold");
  doc.setFontSize(16);
  doc.text(title, margin, 35);

  y = 80;

  if (subtitle) {
    doc.setTextColor(100, 116, 139);
    doc.setFont("helvetica", "italic");
    doc.setFontSize(10);
    doc.text(subtitle, margin, y);
    y += 24;
  }

  for (const sec of sections) {
    // Check page overflow
    if (y > pageHeight - 80) {
      doc.addPage();
      y = margin;
    }

    if (sec.heading) {
      doc.setTextColor(15, 23, 42);
      doc.setFont("helvetica", "bold");
      doc.setFontSize(13);
      doc.text(sec.heading, margin, y);
      y += 16;
    }

    if (sec.paragraphs) {
      doc.setTextColor(51, 65, 85);
      doc.setFont("helvetica", "normal");
      doc.setFontSize(10);

      for (const p of sec.paragraphs) {
        const splitText = doc.splitTextToSize(p, pageWidth - margin * 2);
        if (y + splitText.length * 14 > pageHeight - 60) {
          doc.addPage();
          y = margin;
        }
        doc.text(splitText, margin, y);
        y += splitText.length * 14 + 10;
      }
    }

    if (sec.bulletPoints) {
      doc.setTextColor(51, 65, 85);
      doc.setFont("helvetica", "normal");
      doc.setFontSize(9.5);

      for (const item of sec.bulletPoints) {
        const itemText = `•  ${item}`;
        const splitText = doc.splitTextToSize(itemText, pageWidth - margin * 2 - 10);
        if (y + splitText.length * 13 > pageHeight - 60) {
          doc.addPage();
          y = margin;
        }
        doc.text(splitText, margin + 8, y);
        y += splitText.length * 13 + 4;
      }
      y += 6;
    }

    if (sec.table && sec.table.headers && sec.table.rows) {
      if (y > pageHeight - 120) {
        doc.addPage();
        y = margin;
      }

      autoTable(doc, {
        startY: y,
        head: [sec.table.headers],
        body: sec.table.rows,
        theme: "striped",
        headStyles: {
          fillColor: [30, 41, 59],
          textColor: [255, 255, 255],
          fontStyle: "bold",
          fontSize: 9.5,
        },
        bodyStyles: {
          fontSize: 9,
          textColor: [30, 41, 59],
        },
        alternateRowStyles: {
          fillColor: [248, 250, 252],
        },
        margin: { left: margin, right: margin },
      });

      y = doc.lastAutoTable.finalY + 20;
    }
  }

  // Page numbering in footer
  const totalPages = doc.internal.getNumberOfPages();
  for (let i = 1; i <= totalPages; i++) {
    doc.setPage(i);
    doc.setFont("helvetica", "normal");
    doc.setFontSize(8.5);
    doc.setTextColor(148, 163, 184);
    doc.text(
      `Orion Sovereign AI • Page ${i} of ${totalPages}`,
      pageWidth / 2,
      pageHeight - 20,
      { align: "center" }
    );
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
